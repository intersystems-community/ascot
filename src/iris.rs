//! `open`, which answers an `--iris` argument with a live `RawAtelier` (a URL) or a
//! `Declarations` file `ascot declare` wrote (a path), which never connects to anything.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::IrisContext;
use crate::atelier::{AtelierContext, RawAtelier};
use crate::bindings::{
    ClassInfo, IndexSubscript, MemberInfo, MemberKind, RefMode, RoutineInfo, Signature, Source,
};
use crate::details::ClassDetails;

pub const DEFAULT_NAMESPACE: &str = "%SYS";

/// A URL is a live connection (checked up front), anything else a declaration file's path.
/// `namespace` defaults to `%SYS` for a URL; for a file it must match the one declared.
pub fn open(iris: &str, namespace: Option<&str>) -> Result<Box<dyn IrisContext>, String> {
    if iris.contains("://") {
        let raw = RawAtelier::connect(iris, namespace.unwrap_or(DEFAULT_NAMESPACE))?;
        return Ok(Box::new(AtelierContext::from(raw)));
    }
    let declarations = Declarations::load(Path::new(iris))?;
    if let Some(namespace) = namespace
        && namespace != declarations.namespace
    {
        return Err(format!(
            "{iris} declares namespace '{}', not '{namespace}'",
            declarations.namespace
        ));
    }
    Ok(Box::new(declarations))
}

pub(crate) const DECLARATIONS_VERSION: u32 = 5;

/// One namespace as `ascot declare` writes it: each `IrisContext` lookup as a map from its
/// argument to its answer. A class holds only its own members; ascot walks its superclasses.
#[derive(Serialize, Deserialize, Default)]
pub struct Declarations {
    pub version: u32,
    pub namespace: String,
    /// The instance's `$ZVERSION`; empty in a file from before it was recorded.
    #[serde(default)]
    pub iris_version: String,
    pub classes: BTreeMap<String, DeclaredClass>,
    // Each class parsed once per `Declarations`, on first lookup.
    #[serde(skip)]
    pub(crate) parsed: Mutex<HashMap<String, Option<ClassInfo>>>,
    /// `.inc` files by name without the extension.
    pub includes: BTreeMap<String, Source>,
    /// Routines by name without the extension.
    pub routines: BTreeMap<String, RoutineInfo>,
}

impl Declarations {
    pub fn load(path: &Path) -> Result<Self, String> {
        // Anything without `://` lands here, so a URL missing its scheme reads as a file name.
        let text = std::fs::read_to_string(path).map_err(|e| {
            format!(
                "cannot read declaration file {}: {e} (a live instance's URL starts with http:// \
                 or https://)",
                path.display()
            )
        })?;
        let declarations: Self = serde_json::from_str(&text)
            .map_err(|e| format!("{} is not an ascot declaration file: {e}", path.display()))?;
        if declarations.version != DECLARATIONS_VERSION {
            return Err(format!(
                "{} was written by an incompatible ascot; regenerate it with `ascot declare`",
                path.display()
            ));
        }
        Ok(declarations)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }
}

/// A class as a declaration file stores it: its header and each member's, in the class syntax,
/// with the docs alongside. Read back through the parser a workspace class goes through, so the
/// file holds what the source would, and no more.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct DeclaredClass {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub doc: String,
    /// `Class Name Extends (A, B) [ keywords ]`.
    pub header: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    /// The storage class a persistent class also inherits from, named by its storage definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<String>,
    pub members: Vec<DeclaredMember>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct DeclaredMember {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub doc: String,
    /// `ClassMethod Name(args) As Type [ keywords ]`: a member with a body, without the body.
    pub header: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

// A member name as the class syntax spells it: quoted unless it is a plain identifier.
fn member_name(name: &str) -> String {
    let plain = name
        .chars()
        .enumerate()
        .all(|(i, c)| c.is_ascii_alphanumeric() || (i == 0 && c == '%'));
    if plain {
        name.to_string()
    } else {
        format!("\"{name}\"")
    }
}

fn keyword_list(entries: &[String]) -> String {
    if entries.is_empty() {
        String::new()
    } else {
        format!(" [ {} ]", entries.join(", "))
    }
}

fn as_type(t: Option<&str>) -> String {
    t.map(|t| format!(" As {}", short(t))).unwrap_or_default()
}

// `%Library.X` as `%X`, the short name the class syntax allows.
fn short(class: &str) -> String {
    class.replace("%Library.", "%")
}

// `{:?}` of a WIT enum is its variant's name, which is how the class syntax spells the value.
fn spelled<T: std::fmt::Debug>(value: &T) -> String {
    let debug = format!("{value:?}");
    debug.rsplit("::").next().unwrap_or(&debug).to_lowercase()
}

fn formal(mode: Option<RefMode>, name: &str, t: Option<&str>, default: Option<&str>) -> String {
    let mode = match mode {
        Some(RefMode::ByRef) => "ByRef ",
        Some(RefMode::Output) => "Output ",
        None => "",
    };
    let default = default
        .map(|d| if literal(d) { format!(" = {d}") } else { format!(" = {{{d}}}") })
        .unwrap_or_default();
    format!("{mode}{name}{}{default}", as_type(t))
}

// A number or one quoted string; any other default needs `{}` in class syntax.
fn literal(d: &str) -> bool {
    if d.parse::<f64>().is_ok() {
        return true;
    }
    let Some(body) = d.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return false;
    };
    body.split("\"\"").all(|part| !part.contains('"'))
}

// `(args) As Type` of a method or query.
fn signature(sig: &Signature) -> String {
    let mut args: Vec<String> = sig
        .normal
        .iter()
        .map(|a| formal(a.mode, &a.name, a.t.as_deref(), a.default.as_deref()))
        .collect();
    if let Some(v) = &sig.variadic {
        args.push(formal(
            v.mode,
            &format!("{}...", v.name),
            v.t.as_deref(),
            None,
        ));
    }
    format!("({}){}", args.join(", "), as_type(sig.t.as_deref()))
}

// Whether a member of this header has a `{ }` body in the class syntax.
fn has_body(header: &str) -> bool {
    matches!(
        header.split_whitespace().next(),
        Some("Method" | "ClassMethod" | "ClientMethod" | "Query" | "Trigger" | "XData")
    )
}

// A member's header, or `None` for one the file leaves out: a foreign key or a storage
// definition, which no check reads and whose syntax needs more than the record holds.
fn member_header(member: &MemberInfo) -> Option<String> {
    let name = member_name(&member.name);
    let mut keywords = vec![];
    if member.deprecated {
        keywords.push("Deprecated".to_string());
    }
    let header = match &member.kind {
        MemberKind::Parameter(p) => {
            let value =
                p.v.as_deref()
                    .map(|v| format!(" = {v}"))
                    .unwrap_or_default();
            format!(
                "Parameter {name}{}{}{value}",
                as_type(p.t.as_deref()),
                keyword_list(&keywords)
            )
        }
        MemberKind::Property(p) => {
            let of = p
                .collection
                .map(|c| format!("{} Of ", spelled(&c)))
                .unwrap_or_default();
            let t =
                p.t.as_deref()
                    .map(|t| format!(" As {of}{}", short(t)))
                    .unwrap_or_default();
            if p.multidimensional {
                keywords.push("MultiDimensional".to_string());
            }
            format!("Property {name}{t}{}", keyword_list(&keywords))
        }
        MemberKind::Relationship(r) => {
            if let Some(c) = r.cardinality {
                keywords.push(format!("Cardinality = {}", spelled(&c)));
            }
            format!(
                "Relationship {name}{}{}",
                as_type(r.t.as_deref()),
                keyword_list(&keywords)
            )
        }
        MemberKind::Index(i) => {
            let on = i.on.as_ref().map(|keys| {
                let keys: Vec<String> = keys
                    .iter()
                    .map(|k| {
                        let subscript = match k.subscript {
                            Some(IndexSubscript::Keys) => "(KEYS)",
                            Some(IndexSubscript::Elements) => "(ELEMENTS)",
                            None => "",
                        };
                        let collation = k.collation.as_deref().map(|c| format!(" As {c}"));
                        format!("{}{subscript}{}", k.property, collation.unwrap_or_default())
                    })
                    .collect();
                match keys.as_slice() {
                    [one] => format!(" On {one}"),
                    many => format!(" On ({})", many.join(", ")),
                }
            });
            let flags = [
                (i.abstract_, "Abstract"),
                (i.deferred, "Deferred"),
                (i.extent, "Extent"),
                (i.id_key, "IdKey"),
                (i.internal, "Internal"),
                (i.primary_key, "PrimaryKey"),
                (i.shard_key, "ShardKey"),
                (i.unique, "Unique"),
            ];
            keywords.extend(
                flags
                    .iter()
                    .filter(|(on, _)| *on)
                    .map(|(_, kw)| kw.to_string()),
            );
            if let Some(c) = &i.condition {
                keywords.push(format!("Condition = {{{c}}}"));
            }
            if let Some(c) = &i.coshard_with {
                keywords.push(format!("CoshardWith = {c}"));
            }
            if !i.data.is_empty() {
                keywords.push(format!("Data = ({})", i.data.join(", ")));
            }
            if let Some(n) = &i.sql_name {
                keywords.push(format!("SqlName = {n}"));
            }
            if let Some(t) = i.type_ {
                keywords.push(format!("Type = {}", spelled(&t)));
            }
            keywords.sort();
            format!(
                "Index {name}{}{}",
                on.unwrap_or_default(),
                keyword_list(&keywords)
            )
        }
        MemberKind::Query(sig) => {
            format!("Query {name}{}{}", signature(sig), keyword_list(&keywords))
        }
        MemberKind::Method(sig) => {
            format!("Method {name}{}{}", signature(sig), keyword_list(&keywords))
        }
        MemberKind::ClassMethod(sig) => {
            format!(
                "ClassMethod {name}{}{}",
                signature(sig),
                keyword_list(&keywords)
            )
        }
        MemberKind::ClientMethod(sig) => {
            format!(
                "ClientMethod {name}{}{}",
                signature(sig),
                keyword_list(&keywords)
            )
        }
        MemberKind::Projection => format!("Projection {name}{}", keyword_list(&keywords)),
        MemberKind::Trigger => format!("Trigger {name}{}", keyword_list(&keywords)),
        MemberKind::XData => format!("XData {name}{}", keyword_list(&keywords)),
        MemberKind::ForeignKey | MemberKind::Storage => return None,
    };
    Some(header)
}

impl From<&ClassInfo> for DeclaredClass {
    fn from(info: &ClassInfo) -> Self {
        let extends = match info.extends.as_slice() {
            [] => String::new(),
            [one] => format!(" Extends {}", short(one)),
            many => format!(" Extends ({})", short(&many.join(", "))),
        };
        let mut keywords = vec![];
        if let Some(t) = info.class_type {
            keywords.push(format!("ClassType = {}", spelled(&t)));
        }
        if info.deprecated {
            keywords.push("Deprecated".to_string());
        }
        if let Some(i) = info.inheritance {
            keywords.push(format!("Inheritance = {}", spelled(&i)));
        }
        if let Some(l) = info.language {
            keywords.push(format!("Language = {}", spelled(&l)));
        }
        match info.procedure_block {
            Some(true) => keywords.push("ProcedureBlock".to_string()),
            Some(false) => keywords.push("Not ProcedureBlock".to_string()),
            None => {}
        }
        if !info.property_class.is_empty() {
            keywords.push(format!(
                "PropertyClass = ({})",
                short(&info.property_class.join(", "))
            ));
        }
        Self {
            doc: info.doc.clone(),
            header: format!("Class {}{extends}{}", short(&info.name), keyword_list(&keywords)),
            include: info.include.clone(),
            storage: info.storage.clone(),
            members: info
                .members
                .iter()
                .filter_map(|m| {
                    Some(DeclaredMember {
                        doc: m.doc.clone(),
                        header: member_header(m)?,
                        aliases: m.aliases.clone(),
                    })
                })
                .collect(),
        }
    }
}

fn doc_lines(doc: &str, out: &mut String) {
    if !doc.is_empty() {
        for line in doc.split('\n') {
            out.push_str("///");
            out.push_str(line);
            out.push('\n');
        }
    }
}

impl DeclaredClass {
    /// The class source these declarations stand for: the headers with empty bodies.
    pub fn source(&self) -> String {
        let mut text = String::new();
        doc_lines(&self.doc, &mut text);
        match self.include.as_slice() {
            [] => {}
            [one] => text.push_str(&format!("Include {one}\n")),
            many => text.push_str(&format!("Include ({})\n", many.join(", "))),
        }
        text.push_str(&self.header);
        text.push_str("\n{\n\n");
        for member in &self.members {
            doc_lines(&member.doc, &mut text);
            text.push_str(&member.header);
            text.push_str(if has_body(&member.header) {
                "\n{\n}\n\n"
            } else {
                ";\n\n"
            });
        }
        text.push_str("}\n");
        text
    }

    /// What a live instance would report for this class, or `None` if its source doesn't parse.
    pub fn to_class_info(&self, name: &str) -> Option<ClassInfo> {
        let details = ClassDetails::try_from(self.source().as_str()).ok()?;
        let mut info = ClassInfo::from(&details);
        info.name = name.to_string();
        info.storage = self.storage.clone();
        for member in &mut info.members {
            member.origin = Some(name.to_string());
            if let Some(declared) = self
                .members
                .iter()
                .find(|m| m.header.contains(&member_name(&member.name)))
            {
                member.aliases = declared.aliases.clone();
            }
        }
        Some(info)
    }

    /// `info` as declared, minus any member whose header the class parser rejects, named in the
    /// second value, so that the rest of the class stays loadable.
    pub fn validated(info: &ClassInfo) -> (Self, Vec<String>) {
        let mut declared = Self::from(info);
        if declared.to_class_info(&info.name).is_some() {
            return (declared, vec![]);
        }
        let probe = |member: &DeclaredMember| {
            let one = Self {
                members: vec![member.clone()],
                ..declared.clone()
            };
            one.to_class_info(&info.name).is_some()
        };
        let (kept, dropped): (Vec<_>, Vec<_>) = declared.members.iter().cloned().partition(probe);
        declared.members = kept;
        (declared, dropped.into_iter().map(|m| m.header).collect())
    }
}

impl IrisContext for Declarations {
    fn get_cls(&self, cls: &str) -> Result<Option<ClassInfo>, String> {
        let mut parsed = self.parsed.lock().unwrap();
        if let Some(info) = parsed.get(cls) {
            return Ok(info.clone());
        }
        let info = self.classes.get(cls).and_then(|c| c.to_class_info(cls));
        parsed.insert(cls.to_string(), info.clone());
        Ok(info)
    }

    fn get_inc(&self, name: &str) -> Result<Option<Source>, String> {
        Ok(self.includes.get(name).cloned())
    }

    fn get_rtn(&self, name: &str) -> Result<Option<RoutineInfo>, String> {
        Ok(self.routines.get(name).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class_info(name: &str) -> ClassInfo {
        ClassInfo {
            doc: String::new(),
            name: name.to_string(),
            extends: vec!["%RegisteredObject".to_string()],
            ..Default::default()
        }
    }

    fn declaration_file(name: &str, declarations: &Declarations) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("ascot-{name}-{}.json", std::process::id()));
        declarations.save(&path).unwrap();
        path
    }

    fn source(text: &str) -> Source {
        Source {
            text: text.to_string(),
            uri: None,
        }
    }

    #[test]
    fn declarations_round_trip_through_their_file_and_answer_every_lookup() {
        let declarations = Declarations {
            version: DECLARATIONS_VERSION,
            namespace: "USER".to_string(),
            iris_version: "IRIS for UNIX 2026.1".to_string(),
            classes: [("A.B".to_string(), DeclaredClass::from(&class_info("A.B")))].into(),
            includes: [(
                "MyInc".to_string(),
                source("ROUTINE MyInc [Type=INC]\n#define FIVE 5\n"),
            )]
            .into(),
            routines: [
                (
                    "Lib".to_string(),
                    crate::routine_info(
                        "ROUTINE Lib [Type=INT]\n Quit\nPlain\n Quit\nWithArgs(a,&b,c...)\n \
                         Quit\nProc(a,b) [x,y] {\n Quit\n}\n",
                        None,
                    ),
                ),
                // Doesn't parse, so its labels are unknown and any call into it is taken on trust.
                (
                    "Unparsed".to_string(),
                    crate::routine_info("ROUTINE Unparsed [Type=INT]\n{\n", None),
                ),
                (
                    "Compiled".to_string(),
                    RoutineInfo {
                        labels: None,
                        uri: None,
                    },
                ),
            ]
            .into(),
            ..Default::default()
        };
        let path = declaration_file("round-trip", &declarations);
        let loaded = Declarations::load(&path).unwrap();
        assert_eq!(loaded.namespace, "USER");
        assert_eq!(loaded.iris_version, "IRIS for UNIX 2026.1");
        assert_eq!(loaded.get_cls("A.B"), Ok(Some(class_info("A.B"))));
        assert_eq!(loaded.get_cls("A.C"), Ok(None));
        assert_eq!(
            loaded.get_inc("MyInc"),
            Ok(Some(declarations.includes["MyInc"].clone()))
        );
        assert_eq!(loaded.get_inc("%occErrors"), Ok(None));
        assert_eq!(loaded.get_rtn("Nope"), Ok(None));

        use crate::bindings::{DiagnosticSeverity, GuestWorkspace};
        let check = |body: &str| {
            let loaded = Declarations::load(&path).unwrap();
            let workspace = crate::Workspace::native(Some(Box::new(loaded)), None);
            let uri = "Test.mac".to_string();
            workspace.open(
                uri.clone(),
                format!("ROUTINE Test [Type=MAC]\n#include MyInc\n{body}\n Quit\n"),
            );
            let diagnostics: Vec<String> = workspace
                .diagnostics(uri)
                .into_iter()
                .filter(|d| {
                    matches!(
                        d.severity,
                        DiagnosticSeverity::Error | DiagnosticSeverity::Warning
                    )
                })
                .map(|d| d.message)
                .collect();
            diagnostics
        };
        assert_eq!(
            check(
                " Write $$$FIVE\n Do Plain^Lib\n Do WithArgs^Lib(1,.v,2,3)\n Do Proc^Lib(1,2)\n Do Anything^Unparsed(1)"
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            check(" Do Nope^Lib"),
            vec!["Cannot find label 'Nope' in routine 'Lib'"]
        );
        assert_eq!(check(" Do Proc^Lib(1,2,3)"), vec!["Too many arguments"]);
        assert_eq!(
            check(" Do Plain^NoSuchRoutine"),
            vec!["Cannot find routine 'NoSuchRoutine' in the workspace or on the IRIS instance"]
        );
        // Only a hint, which `check` leaves out.
        assert_eq!(check(" Do Plain^Compiled(1)"), Vec::<String>::new());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_declared_class_round_trips_through_its_source() {
        use crate::bindings::{
            Cardinality, ClassType, IndexInfo, IndexKey, IndexType, Inheritance, NormalArg,
            ParameterInfo, PropertyInfo, RelationshipInfo, VariadicArg,
        };
        let member = |name: &str, doc: &str, kind: MemberKind| MemberInfo {
            doc: doc.to_string(),
            name: name.to_string(),
            aliases: vec![],
            deprecated: name == "Make",
            kind,
            uri: None,
            origin: Some("Demo.Thing".to_string()),
        };
        let signature = Signature {
            normal: vec![
                NormalArg {
                    mode: None,
                    name: "a".to_string(),
                    t: Some("%String".to_string()),
                    default: Some("\"x\"".to_string()),
                },
                NormalArg {
                    mode: Some(RefMode::Output),
                    name: "b".to_string(),
                    t: None,
                    default: None,
                },
            ],
            variadic: Some(VariadicArg {
                mode: Some(RefMode::ByRef),
                name: "rest".to_string(),
                t: Some("%List".to_string()),
            }),
            t: Some("%Status".to_string()),
            procedure_block: Some(vec![]),
        };
        let info = ClassInfo {
            doc: "A thing.\nTwo lines.".to_string(),
            name: "Demo.Thing".to_string(),
            extends: vec!["%Persistent".to_string(), "%XML.Adaptor".to_string()],
            storage: Some("%Storage.Persistent".to_string()),
            include: vec!["%occInclude".to_string()],
            uri: None,
            class_type: Some(ClassType::Persistent),
            deprecated: false,
            inheritance: Some(Inheritance::Right),
            language: None,
            procedure_block: Some(false),
            property_class: vec!["Demo.PropertyClass".to_string()],
            members: vec![
                member(
                    "MAX",
                    "The limit.",
                    MemberKind::Parameter(ParameterInfo {
                        t: Some("INTEGER".to_string()),
                        v: Some("10".to_string()),
                    }),
                ),
                member(
                    "Names",
                    "",
                    MemberKind::Property(PropertyInfo {
                        t: Some("%String".to_string()),
                        collection: Some(crate::bindings::CollectionKind::List),
                        multidimensional: false,
                    }),
                ),
                member(
                    "Children",
                    "",
                    MemberKind::Relationship(RelationshipInfo {
                        t: Some("Demo.Child".to_string()),
                        cardinality: Some(Cardinality::Many),
                    }),
                ),
                member(
                    "ByName",
                    "",
                    MemberKind::Index(IndexInfo {
                        on: Some(vec![IndexKey {
                            property: "Names".to_string(),
                            subscript: Some(IndexSubscript::Elements),
                            collation: Some("Exact".to_string()),
                        }]),
                        abstract_: false,
                        condition: None,
                        coshard_with: None,
                        data: vec!["MAX".to_string()],
                        deferred: false,
                        extent: false,
                        id_key: false,
                        internal: false,
                        primary_key: false,
                        shard_key: false,
                        sql_name: Some("BYNAME".to_string()),
                        type_: Some(IndexType::Bitmap),
                        unique: true,
                    }),
                ),
                member(
                    "Find",
                    "",
                    MemberKind::Query(Signature {
                        procedure_block: None,
                        ..signature.clone()
                    }),
                ),
                member("Run", "Does it.", MemberKind::Method(signature.clone())),
                member("Make", "", MemberKind::ClassMethod(signature)),
            ],
        };
        let declared = DeclaredClass::from(&info);
        assert_eq!(
            declared.to_class_info("Demo.Thing").as_ref(),
            Some(&info),
            "{}",
            declared.source()
        );
    }

    #[test]
    fn open_reads_a_declaration_file_and_checks_its_namespace() {
        let declarations = Declarations {
            version: DECLARATIONS_VERSION,
            namespace: "USER".to_string(),
            iris_version: String::new(),
            classes: BTreeMap::new(),
            includes: BTreeMap::new(),
            routines: BTreeMap::new(),
            ..Default::default()
        };
        let path = declaration_file("open", &declarations);
        let arg = path.to_str().unwrap();
        assert!(open(arg, None).is_ok());
        assert!(open(arg, Some("USER")).is_ok());
        let err = open(arg, Some("%SYS")).err().unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(
            err.contains("declares namespace 'USER', not '%SYS'"),
            "{err}"
        );
    }

    #[test]
    fn open_rejects_a_declaration_file_of_another_version_or_no_such_file_at_all() {
        let path = std::env::temp_dir().join(format!("ascot-version-{}.json", std::process::id()));
        std::fs::write(
            &path,
            r#"{"version":1,"namespace":"USER","classes":{},"includes":{},"routines":{}}"#,
        )
        .unwrap();
        let err = open(path.to_str().unwrap(), None).err().unwrap();
        assert!(
            err.contains("was written by an incompatible ascot"),
            "{err}"
        );
        std::fs::write(&path, "Class A.B {}").unwrap();
        let err = open(path.to_str().unwrap(), None).err().unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(err.contains("is not an ascot declaration file"), "{err}");
        let err = open("/no/such/declarations.json", None).err().unwrap();
        assert!(err.contains("cannot read declaration file"), "{err}");
    }
}
