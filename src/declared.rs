//! A class as a declaration file or the mask stores it: its header and each member's, in the
//! class syntax, with the docs alongside. Read back through the parser a workspace class goes
//! through, so the file holds what the source would, and no more.

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::IrisContext;
use crate::bindings::{
    ClassInfo, IndexSubscript, MemberInfo, MemberKind, RefMode, RoutineInfo, Signature, Source,
};
use crate::details::ClassDetails;

/// The declaration format's version, semver: a file is readable while its major part matches.
pub(crate) const DECLARATIONS_VERSION: &str = "1.0.0";

/// One namespace as `ascot declare` writes it: each `IrisContext` lookup as a map from its
/// argument to its answer. A class holds only its own members; ascot walks its superclasses.
#[derive(Serialize, Deserialize, Default)]
pub struct Declarations {
    pub version: String,
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
    /// A declaration file's content, gzipped or not, as `ascot declare` or the pre-built assets
    /// hold it.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let mut text = Vec::new();
        if bytes.starts_with(&[0x1f, 0x8b]) {
            flate2::read::GzDecoder::new(bytes)
                .read_to_end(&mut text)
                .map_err(|e| format!("not a gzip file: {e}"))?;
        }
        let text = if text.is_empty() { bytes } else { &text[..] };
        let declarations: Self = serde_json::from_slice(text)
            .map_err(|e| format!("not an ascot declaration file: {e}"))?;
        let major = |v: &str| v.split('.').next().unwrap_or_default().to_string();
        if major(&declarations.version) != major(DECLARATIONS_VERSION) {
            return Err(format!(
                "written by an incompatible ascot (format {}, this one reads {}); regenerate it \
                 with `ascot declare`",
                declarations.version, DECLARATIONS_VERSION
            ));
        }
        Ok(declarations)
    }
}

/// A class as a declaration file stores it: its header and each member's, in the class syntax,
/// with the docs alongside. Read back through the parser a workspace class goes through, so the
/// file holds what the source would, and no more.
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

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct DeclaredClass {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub doc: String,
    /// `Class Name Extends (A, B) [ keywords ]`.
    pub header: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub import: Vec<String>,
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
        format!("\"{}\"", name.replace('"', "\"\""))
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
        // IRIS drops the default mode from what it reports; a parameter named like a mode keyword
        // (`SetOutput(ByVal Output As ...)`) needs it back to parse, as it did in its source.
        None if ["ByVal", "ByRef", "Output"]
            .iter()
            .any(|k| k.eq_ignore_ascii_case(name)) =>
        {
            "ByVal "
        }
        None => "",
    };
    let default = default
        .map(|d| {
            if literal(d) {
                format!(" = {d}")
            } else {
                format!(" = {{{d}}}")
            }
        })
        .unwrap_or_default();
    format!("{mode}{name}{}{default}", as_type(t))
}

// A number, one quoted string, or an expression already in the `{}` any other default needs in
// class syntax (IRIS reports some with their braces and some without).
fn literal(d: &str) -> bool {
    if d.parse::<f64>().is_ok() || (d.starts_with('{') && d.ends_with('}')) {
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
                        let property: Vec<String> =
                            k.property.split('.').map(member_name).collect();
                        format!(
                            "{}{subscript}{}",
                            property.join("."),
                            collation.unwrap_or_default()
                        )
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
        if info.abstract_ {
            keywords.push("Abstract".to_string());
        }
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
            header: format!(
                "Class {}{extends}{}",
                short(&info.name),
                keyword_list(&keywords)
            ),
            import: info.import.clone().unwrap_or_default(),
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
        for (keyword, names) in [("Import", &self.import), ("Include", &self.include)] {
            match names.as_slice() {
                [] => {}
                [one] => text.push_str(&format!("{keyword} {one}\n")),
                many => text.push_str(&format!("{keyword} ({})\n", many.join(", "))),
            }
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
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg(feature = "declare")]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::{MemberInfo, MemberKind, RefMode, Signature};

    #[test]
    fn a_parameter_named_like_a_mode_keyword_gets_its_byval_back() {
        assert_eq!(
            formal(None, "Output", Some("%String"), None),
            "ByVal Output As %String"
        );
        assert_eq!(formal(None, "byRef", None, Some("0")), "ByVal byRef = 0");
        assert_eq!(formal(None, "out", None, None), "out");
    }

    // IRIS quotes a name with a character outside the identifier set, in an index's key list as
    // in a member's own name.
    #[test]
    fn an_index_key_with_an_underscore_is_quoted() {
        let index = MemberInfo {
            doc: String::new(),
            name: "IDKEY".to_string(),
            aliases: vec![],
            deprecated: false,
            kind: MemberKind::Index(crate::bindings::IndexInfo {
                on: Some(vec![crate::bindings::IndexKey {
                    property: "element_key".to_string(),
                    subscript: None,
                    collation: None,
                }]),
                abstract_: false,
                condition: None,
                coshard_with: None,
                data: vec![],
                deferred: false,
                extent: false,
                id_key: false,
                internal: false,
                primary_key: false,
                shard_key: false,
                sql_name: None,
                type_: None,
                unique: false,
            }),
            uri: None,
            origin: None,
        };
        assert_eq!(
            member_header(&index).unwrap(),
            "Index IDKEY On \"element_key\""
        );
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
            import: None,
            uri: None,
            class_type: Some(ClassType::Persistent),
            deprecated: false,
            abstract_: true,
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
}
