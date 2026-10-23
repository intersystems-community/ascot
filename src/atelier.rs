//! `AtelierLike`, the two Atelier REST API requests, and `AtelierBasedClient`, the cached
//! `IrisClient` ascot makes out of them: the `%Dictionary` queries behind `get_cls`, and the
//! parsing behind `get_rtn`. ascot's own `AtelierLike` is `http::AtelierConnection`.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

use crate::IrisClient;
use crate::bindings::{
    ArgMode, Cardinality, ClassInfo, CollectionKind, MemberInfo, MemberKind, MethodInfo, NameInfo,
    NormalArg, ParameterInfo, Position, PropertyInfo, Range, RelationshipInfo, RoutineInfo, Source,
    VariadicArg,
};

const CACHE_TTL: Duration = Duration::from_secs(60 * 60);

struct Cache<V: Clone>(Mutex<HashMap<String, (Option<Instant>, V)>>);

impl<V: Clone> Cache<V> {
    fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }

    fn get(&self, key: &str) -> Option<V> {
        let entries = self.0.lock().unwrap();
        let (at, value) = entries.get(key)?;
        at.is_none_or(|at| at.elapsed() < CACHE_TTL)
            .then(|| value.clone())
    }

    fn set(&self, key: String, value: V) {
        // wasm32-unknown-unknown has no clock, so entries there never expire.
        let at = (!cfg!(target_arch = "wasm32")).then(Instant::now);
        self.0.lock().unwrap().insert(key, (at, value));
    }
}

fn doc_basename(doc: &str) -> String {
    doc.rsplit_once('.')
        .map_or(doc, |(name, _)| name)
        .to_string()
}

/// The two Atelier REST API requests the lookups are made of, for a host with its own connection.
pub trait AtelierLike: Send + Sync {
    /// The rows of `/action/query`, each keyed by column name; `None` if the query failed.
    fn query(&self, sql: &str, params: &[&str]) -> Option<Vec<Value>>;
    /// The lines of `/doc/{name}`; `None` if it doesn't exist or can't be fetched.
    fn get_doc(&self, name: &str) -> Option<Vec<String>>;
}

impl<T: AtelierLike + ?Sized> AtelierLike for Box<T> {
    fn query(&self, sql: &str, params: &[&str]) -> Option<Vec<Value>> {
        (**self).query(sql, params)
    }

    fn get_doc(&self, name: &str) -> Option<Vec<String>> {
        (**self).get_doc(name)
    }
}

#[cfg(target_arch = "wasm32")]
impl AtelierLike for crate::bindings::Atelier {
    fn query(&self, sql: &str, params: &[&str]) -> Option<Vec<Value>> {
        let params: Vec<String> = params.iter().map(|param| param.to_string()).collect();
        let rows = crate::bindings::Atelier::query(self, sql, &params)?;
        rows.iter()
            .map(|row| serde_json::from_str(row).ok())
            .collect()
    }

    fn get_doc(&self, name: &str) -> Option<Vec<String>> {
        crate::bindings::Atelier::get_doc(self, name)
    }
}

/// The `IrisClient` that looks code up through an `AtelierLike`, caching what it finds.
pub struct AtelierBasedClient(Cached<Box<dyn AtelierLike>>);

impl<A: AtelierLike + 'static> From<A> for AtelierBasedClient {
    fn from(atelier: A) -> Self {
        Self(Cached::new(Box::new(atelier)))
    }
}

impl IrisClient for AtelierBasedClient {
    fn get_cls(&self, cls: &str) -> Option<ClassInfo> {
        self.0.get_cls(cls)
    }

    fn get_inc(&self, name: &str) -> Option<Source> {
        self.0.get_inc(name)
    }

    fn get_rtn(&self, name: &str) -> Option<RoutineInfo> {
        self.0.get_rtn(name)
    }
}

pub(crate) struct Cached<A> {
    pub(crate) atelier: A,
    cls_cache: Cache<Option<ClassInfo>>,
    routine_cache: Cache<Option<Source>>,
    rtn_cache: Cache<Option<RoutineInfo>>,
}

impl<A: AtelierLike> Cached<A> {
    pub(crate) fn new(atelier: A) -> Self {
        Self {
            atelier,
            cls_cache: Cache::new(),
            routine_cache: Cache::new(),
            rtn_cache: Cache::new(),
        }
    }
}

impl<A: AtelierLike> IrisClient for Cached<A> {
    fn get_cls(&self, cls: &str) -> Option<ClassInfo> {
        if let Some(cached) = self.cls_cache.get(cls) {
            return cached;
        }
        let result = class_infos(&self.atelier, "= ?", &[cls])
            .into_values()
            .next();
        self.cls_cache.set(cls.to_string(), result.clone());
        result
    }

    fn get_inc(&self, name: &str) -> Option<Source> {
        self.fetch_routine(&[format!("{name}.inc")])
    }

    // The `.int` first: it is the instance's own macro expansion of the `.mac`, so no expansion
    // (or include resolution) is needed on our side.
    fn get_rtn(&self, name: &str) -> Option<RoutineInfo> {
        if let Some(cached) = self.rtn_cache.get(name) {
            return cached;
        }
        let result = match self.fetch_routine(&[format!("{name}.int"), format!("{name}.mac")]) {
            Some(source) => Some(crate::routine_info(&source.text, source.uri)),
            // Without source, its OBJ says whether it exists; when that can't be asked, it might.
            None => routine_names(&self.atelier, &format!("{name}.obj"))
                .is_none_or(|names| names.contains(name))
                .then_some(RoutineInfo {
                    labels: None,
                    uri: None,
                }),
        };
        self.rtn_cache.set(name.to_string(), result.clone());
        result
    }
}

// The routines matching `spec` (e.g. `*.mac,*.int,*.obj`), system ones included and
// class-generated ones (`Name.1.obj`) left out.
pub(crate) fn routine_names(conn: &dyn AtelierLike, spec: &str) -> Option<BTreeSet<String>> {
    let sql = "SELECT Name FROM %Library.RoutineMgr_StudioOpenDialog(?,1,1,1,1,1,0,\
               'NOT (Name %PATTERN ''.E1\".\"0.1\"G\"1N1\".obj\"'' AND $LENGTH(Name,''.'') > 3)')";
    let rows = conn.query(sql, &[spec])?;
    Some(
        rows.iter()
            .filter_map(|row| Some(doc_basename(row.get("Name")?.as_str()?)))
            .collect(),
    )
}

impl<A: AtelierLike> Cached<A> {
    // The first of `docs` that exists, cached under the whole candidate list.
    fn fetch_routine(&self, docs: &[String]) -> Option<Source> {
        let key = docs.join("|");
        if let Some(cached) = self.routine_cache.get(&key) {
            return cached;
        }
        let result = docs
            .iter()
            .find_map(|doc| self.atelier.get_doc(doc))
            .map(|lines| Source {
                text: lines.join("\n"),
                // The Atelier REST API has no uri scheme of its own to offer here.
                uri: None,
            });
        self.routine_cache.set(key, result.clone());
        result
    }
}

fn split_classes(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

// Every class satisfying `Name {filter}` (`= ?`, `IN (?,?)`; `params` bind the `?`s) with its
// own members, keyed by name: one round trip per dictionary table for the whole set.
pub(crate) fn class_infos(
    conn: &dyn AtelierLike,
    filter: &str,
    params: &[&str],
) -> HashMap<String, ClassInfo> {
    let sql = |description: &str| {
        format!(
            "SELECT Name, {description}, Super, PropertyClass, IncludeCode, ClassType, Deprecated \
             FROM %Dictionary.CompiledClass WHERE Name {filter}"
        )
    };
    // Same `{`-leading-text failure mode `all_mem_rows` guards against.
    let rows = conn
        .query(&sql("Description"), params)
        .or_else(|| conn.query(&sql("'' AS Description"), params))
        .unwrap_or_default();
    if rows.is_empty() {
        return HashMap::new();
    }
    let members = resolve_stubs(conn, filter, params, all_mem_rows(conn, filter, params));
    assemble(rows, members)
}

fn assemble(class_rows: Vec<Value>, member_rows: Vec<MemberRow>) -> HashMap<String, ClassInfo> {
    let mut members: HashMap<String, Vec<MemberInfo>> = HashMap::new();
    for row in member_rows {
        if !row.name.is_empty() {
            members
                .entry(row.parent.clone())
                .or_default()
                .push(member_row_to_info(row));
        }
    }
    let zero = Position {
        line: 0,
        character: 0,
    };
    class_rows
        .into_iter()
        .filter_map(|row| {
            let str_field = |key: &str| row.get(key).and_then(Value::as_str).unwrap_or_default();
            let cls = str_field("Name").to_string();
            if cls.is_empty() {
                return None;
            }
            let info = ClassInfo {
                doc: str_field("Description").to_string(),
                name: NameInfo {
                    before: zero,
                    content: cls.clone(),
                    after: zero,
                },
                extends: split_classes(str_field("Super")),
                deprecated: is_truthy(&row.get("Deprecated").cloned().unwrap_or(Value::Null)),
                members: members.remove(&cls).unwrap_or_default(),
                property_class_names: split_classes(str_field("PropertyClass")),
                includes: split_classes(str_field("IncludeCode")),
                is_datatype: Some(str_field("ClassType") == "datatype"),
                // The Atelier REST API has no uri scheme of its own to offer here.
                uri: None,
            };
            Some((cls, info))
        })
        .collect()
}

// Uniform row shape across all three member tables `all_mem_uncached` unions together. IRIS
// reports an unset SQL string column as an empty string, not JSON `null` -- normalized to `None`
// here, at the boundary, so nothing downstream has to know that (or risk treating "" as a real,
// if nonsense, class/type name).
#[derive(Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
struct MemberRow {
    // IRIS names the column after its `parent` relationship; an `AS Parent` alias keeps the case.
    #[serde(default, alias = "parent")]
    parent: String,
    #[serde(default)]
    name: String,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    aliases: Option<String>,
    member_type: String,
    #[serde(deserialize_with = "empty_string_as_none")]
    description: Option<String>,
    #[serde(deserialize_with = "empty_string_as_none")]
    formal_spec: Option<String>,
    #[serde(deserialize_with = "empty_string_as_none")]
    return_type: Option<String>,
    // A property's `list`/`array`, when it is a collection.
    #[serde(default, deserialize_with = "empty_string_as_none")]
    collection: Option<String>,
    #[serde(default)]
    relationship: Value,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    cardinality: Option<String>,
    #[serde(default)]
    multi_dimensional: Value,
    #[serde(deserialize_with = "empty_string_as_none")]
    stub: Option<String>,
    class_method: Value,
    deprecated: Value,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    origin: Option<String>,
}

fn empty_string_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.filter(|s| !s.is_empty()))
}

// IRIS may serialize a SQL bit column as a JSON bool, a "0"/"1" string, or a 0/1 number.
fn is_truthy(v: &Value) -> bool {
    matches!(v, Value::Bool(true)) || v.as_str() == Some("1") || v.as_i64() == Some(1)
}

// Every member declared directly on the classes matching `Parent {filter}`, across all three
// tables -- one query per batch instead of one per member, since `get_cls` always wants the
// whole list at once.
fn all_mem_rows(conn: &dyn AtelierLike, filter: &str, params: &[&str]) -> Vec<MemberRow> {
    let sql = |description: &str| {
        format!(
            "SELECT Parent, Name, NULL AS Aliases, 'method' AS MemberType, {description}, \
             FormalSpec, ReturnType, NULL AS Collection, 0 AS MultiDimensional, 0 AS \
             Relationship, NULL AS Cardinality, Stub, ClassMethod, Deprecated, Origin FROM \
             %Dictionary.CompiledMethod WHERE Parent {filter} UNION ALL SELECT Parent, Name, \
             Aliases, 'property' AS MemberType, {description}, NULL AS FormalSpec, Type AS \
             ReturnType, Collection, MultiDimensional, Relationship, Cardinality, NULL AS Stub, \
             0 AS ClassMethod, Deprecated, Origin FROM %Dictionary.CompiledProperty WHERE Parent \
             {filter} UNION ALL SELECT Parent, Name, NULL AS Aliases, 'parameter' AS MemberType, \
             {description}, NULL AS FormalSpec, Type AS ReturnType, NULL AS Collection, 0 AS \
             MultiDimensional, 0 AS Relationship, NULL AS Cardinality, NULL AS Stub, 0 AS \
             ClassMethod, Deprecated, Origin FROM %Dictionary.CompiledParameter WHERE Parent \
             {filter}"
        )
    };
    let params = [params, params, params].concat();
    // The Atelier query API fails the whole query over a text value that merely starts with `{`
    // (e.g. a `/// {$$$CPFSave+...}` parameter comment); retry without descriptions.
    conn.query(&sql("Description"), &params)
        .or_else(|| conn.query(&sql("'' AS Description"), &params))
        .map(|rows| {
            rows.into_iter()
                .filter_map(|row| serde_json::from_value(row).ok())
                .collect()
        })
        .unwrap_or_default()
}

// A method row with a `Stub` is a compiler-generated accessor (index/query/property/
// constraint-backed) -- its real metadata lives in a different table, keyed by the stub's
// `origin.subname.kindchar` segments: `(origin, subname, table)`.
fn stub_parts(row: &MemberRow) -> Option<(&str, &str, &'static str)> {
    if row.member_type != "method" {
        return None;
    }
    let mut parts = row.stub.as_deref()?.split('.');
    let origin = parts.next().unwrap_or_default();
    let subname = parts.next().unwrap_or_default();
    let table = match parts.next() {
        Some("i") => "Index",
        Some("q") => "Query",
        Some("a") => "Property",
        Some("n") => "Constraint",
        _ => return None,
    };
    Some((origin, subname, table))
}

// Replaces each stub row with its real metadata -- one query per stub table the classes use,
// not one per stub (a property-heavy class has dozens of generated accessors).
fn resolve_stubs(
    conn: &dyn AtelierLike,
    filter: &str,
    params: &[&str],
    rows: Vec<MemberRow>,
) -> Vec<MemberRow> {
    let tables: HashSet<&str> = rows
        .iter()
        .filter_map(stub_parts)
        .map(|(_, _, t)| t)
        .collect();
    let mut resolved: HashMap<(String, String, String, &str), MemberRow> = HashMap::new();
    for table in tables {
        let sql = |description: &str| {
            format!(
                "SELECT parent->Parent AS Parent, Name, parent->Name AS StubOrigin, 'method' AS \
                 MemberType, {description}, FormalSpec, ReturnType, '' AS Stub, ClassMethod, \
                 Deprecated FROM %Dictionary.Compiled{table}Method WHERE parent->Parent {filter}"
            )
        };
        // Same `{`-leading-text failure mode `all_mem_rows` guards against.
        let found = conn
            .query(&sql("Description"), params)
            .or_else(|| conn.query(&sql("'' AS Description"), params))
            .unwrap_or_default();
        for value in found {
            let origin = value
                .get("StubOrigin")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let Ok(row) = serde_json::from_value::<MemberRow>(value.clone()) else {
                continue;
            };
            resolved.insert(
                (
                    row.parent.clone(),
                    origin.to_string(),
                    row.name.clone(),
                    table,
                ),
                row,
            );
        }
    }
    rows.into_iter()
        .map(|row| match stub_parts(&row) {
            Some((origin, subname, table)) => resolved
                .remove(&(
                    row.parent.clone(),
                    origin.to_string(),
                    subname.to_string(),
                    table,
                ))
                .unwrap_or(row),
            None => row,
        })
        .collect()
}

// A property's alias list is a comma-separated %Translate'd string.
fn split_aliases(aliases: Option<&str>) -> Vec<String> {
    aliases
        .map(|aliases| {
            aliases
                .split(',')
                .map(|alias| alias.chars().filter(|c| !c.is_whitespace()).collect())
                .filter(|alias: &String| !alias.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn member_row_to_info(row: MemberRow) -> MemberInfo {
    let name = row.name.clone();
    let aliases = split_aliases(row.aliases.as_deref());
    let origin = row.origin.clone();
    let zero = Position {
        line: 0,
        character: 0,
    };
    MemberInfo {
        doc: row.description.clone().unwrap_or_default(),
        before: zero,
        name: NameInfo {
            before: zero,
            content: name,
            after: zero,
        },
        aliases,
        deprecated: is_truthy(&row.deprecated),
        kind: row_to_member_kind(&row),
        after: zero,
        uri: None,
        origin,
    }
}

// A collection property's type is the collection class -- the same mapping the parser's
// `PropertyType` gets for a `list Of X` declaration.
fn property_type(
    collection: Option<&str>,
    ty: Option<String>,
    multidimensional: bool,
) -> PropertyInfo {
    PropertyInfo {
        t: ty,
        collection: match collection.map(str::to_ascii_lowercase).as_deref() {
            Some("list") => Some(CollectionKind::List),
            Some("array") => Some(CollectionKind::Array),
            _ => None,
        },
        multidimensional,
    }
}

fn row_to_member_kind(row: &MemberRow) -> MemberKind {
    match row.member_type.as_str() {
        "property" if is_truthy(&row.relationship) => MemberKind::Relationship(RelationshipInfo {
            t: row.return_type.clone(),
            cardinality: match row
                .cardinality
                .as_deref()
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("one") => Some(Cardinality::One),
                Some("many") => Some(Cardinality::Many),
                Some("parent") => Some(Cardinality::Parent),
                Some("children") => Some(Cardinality::Children),
                _ => None,
            },
        }),
        "property" => MemberKind::Property(property_type(
            row.collection.as_deref(),
            row.return_type.clone(),
            is_truthy(&row.multi_dimensional),
        )),
        "parameter" => MemberKind::Parameter(ParameterInfo {
            t: row.return_type.clone(),
            v: None,
        }),
        _ => {
            let (normal, variadic) = parse_formal_spec(row.formal_spec.as_deref().unwrap_or(""));
            let zero = Position {
                line: 0,
                character: 0,
            };
            let info = MethodInfo {
                normal,
                variadic,
                t: row.return_type.clone(),
                procedure_block: None,
                body: Range {
                    start: zero,
                    end: zero,
                },
            };
            if is_truthy(&row.class_method) {
                MemberKind::ClassMethod(info)
            } else {
                MemberKind::Method(info)
            }
        }
    }
}

// Ported from `splitTopLevel` in `server/src/ascot/index.ts`: splits on top-level commas,
// respecting quote and paren nesting (a type or default value may itself contain either).
fn split_top_level(spec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut in_quote = false;
    for c in spec.chars() {
        if in_quote {
            cur.push(c);
            if c == '"' {
                in_quote = false;
            }
        } else if c == '"' {
            in_quote = true;
            cur.push(c);
        } else if c == '(' {
            depth += 1;
            cur.push(c);
        } else if c == ')' {
            depth -= 1;
            cur.push(c);
        } else if c == ',' && depth == 0 {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

#[derive(PartialEq)]
enum FormalSpecStage {
    Name,
    Type,
    Default,
}

// Ported from `parseFormalSpec` in `server/src/ascot/index.ts`: parses IRIS's minified
// `FormalSpec` (e.g. `*out:%String,&ref:%Integer,x...`) into structured args. Prefixes: `*`
// output, `&` by-ref; `:` starts the type, `=` the default; a trailing `...` marks the variadic
// arg. Types/defaults may contain commas/parens/quotes, so `split_top_level` respects nesting.
fn parse_formal_spec(spec: &str) -> (Vec<NormalArg>, Option<VariadicArg>) {
    let mut normal = Vec::new();
    let mut variadic = None;
    for raw in split_top_level(spec) {
        let mut s = raw.trim();
        if s.is_empty() {
            continue;
        }
        let mode = if let Some(rest) = s.strip_prefix('*') {
            s = rest;
            ArgMode::Output
        } else if let Some(rest) = s.strip_prefix('&') {
            s = rest;
            ArgMode::ByRef
        } else {
            ArgMode::Default
        };

        let mut name = String::new();
        let mut ty = String::new();
        let mut default = String::new();
        let mut stage = FormalSpecStage::Name;
        let mut depth = 0i32;
        let mut in_quote = false;
        for c in s.chars() {
            if in_quote {
                if c == '"' {
                    in_quote = false;
                }
            } else if c == '"' {
                in_quote = true;
            } else if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
            } else if depth == 0 && stage == FormalSpecStage::Name && c == ':' {
                stage = FormalSpecStage::Type;
                continue;
            } else if depth == 0 && stage != FormalSpecStage::Default && c == '=' {
                stage = FormalSpecStage::Default;
                continue;
            }
            match stage {
                FormalSpecStage::Name => name.push(c),
                FormalSpecStage::Type => ty.push(c),
                FormalSpecStage::Default => default.push(c),
            }
        }

        let t = (!ty.is_empty()).then_some(ty);
        if let Some(base) = name.strip_suffix("...") {
            variadic = Some(VariadicArg {
                mode,
                name: base.to_string(),
                t,
                declared_at: None,
            });
        } else {
            normal.push(NormalArg {
                mode,
                name,
                t,
                default: (!default.is_empty()).then_some(default),
                declared_at: None,
            });
        }
    }
    (normal, variadic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_collection_property_keeps_its_element_type() {
        let list = property_type(Some("list"), Some("%String".into()), false);
        assert_eq!(list.collection, Some(CollectionKind::List));
        assert_eq!(list.t.as_deref(), Some("%String"));
        let array = property_type(Some("array"), Some("Foo.Bar".into()), false);
        assert_eq!(array.collection, Some(CollectionKind::Array));
        assert_eq!(
            property_type(None, Some("%String".into()), false).collection,
            None
        );
    }

    #[test]
    fn assemble_groups_member_rows_by_their_parent_class() {
        let class_rows = vec![
            serde_json::json!({"Name": "A.B", "Super": "%RegisteredObject", "ClassType": ""}),
            serde_json::json!({"Name": "A.C", "Super": "", "ClassType": "datatype", "Deprecated": 1}),
        ];
        let member_rows: Vec<MemberRow> = [
            serde_json::json!({"parent": "A.C", "Name": "P", "MemberType": "property", "Description": "", "FormalSpec": "", "ReturnType": "%String", "Stub": "", "ClassMethod": 0, "Deprecated": 0}),
            serde_json::json!({"parent": "A.B", "Name": "M", "MemberType": "method", "Description": "", "FormalSpec": "x:%String", "ReturnType": "%Status", "Stub": "", "ClassMethod": 1, "Deprecated": 0}),
            serde_json::json!({"Parent": "A.B", "Name": "", "MemberType": "method", "Description": "", "FormalSpec": "", "ReturnType": "", "Stub": "", "ClassMethod": 0, "Deprecated": 0}),
        ]
        .into_iter()
        .map(|row| serde_json::from_value(row).unwrap())
        .collect();
        let classes = assemble(class_rows, member_rows);
        assert_eq!(classes.len(), 2);
        let b = &classes["A.B"];
        assert_eq!(b.extends, vec!["%RegisteredObject"]);
        assert_eq!(b.is_datatype, Some(false));
        assert_eq!(b.members.len(), 1, "the nameless row is dropped");
        assert_eq!(b.members[0].name.content, "M");
        assert!(matches!(b.members[0].kind, MemberKind::ClassMethod(_)));
        let c = &classes["A.C"];
        assert!(c.deprecated);
        assert_eq!(c.is_datatype, Some(true));
        assert_eq!(c.members[0].name.content, "P");
    }

    #[test]
    fn a_host_answers_the_lookups_through_its_own_atelier() {
        struct Docs;
        impl AtelierLike for Docs {
            fn query(&self, _: &str, _: &[&str]) -> Option<Vec<Value>> {
                Some(vec![])
            }
            fn get_doc(&self, name: &str) -> Option<Vec<String>> {
                (name == "Util.mac")
                    .then(|| vec!["ROUTINE Util".into(), "Foo(a)".into(), " Quit".into()])
            }
        }
        let iris = AtelierBasedClient::from(Docs);
        let util = iris.get_rtn("Util").unwrap().labels.unwrap();
        let labels: Vec<_> = util.iter().map(|label| label.name.as_str()).collect();
        assert_eq!(labels, ["Foo"]);
        assert!(iris.get_inc("Util").is_none());
        assert!(iris.get_rtn("NoSuchRoutine").is_none());
        assert!(iris.get_cls("No.Such.Class").is_none());
    }

    #[test]
    fn doc_basename_strips_the_extension() {
        assert_eq!(doc_basename("Foo.Bar.inc"), "Foo.Bar");
    }

    #[test]
    fn parses_a_normal_and_variadic_arg_with_types_and_a_default() {
        let (normal, variadic) =
            parse_formal_spec(r#"*out:%String,&ref:%Integer,x:%String="a",y..."#);
        assert_eq!(normal.len(), 3);
        assert_eq!(normal[0].mode, ArgMode::Output);
        assert_eq!(normal[0].name, "out");
        assert_eq!(normal[0].t, Some("%String".to_string()));
        assert_eq!(normal[1].mode, ArgMode::ByRef);
        assert_eq!(normal[1].name, "ref");
        assert_eq!(normal[1].t, Some("%Integer".to_string()));
        assert_eq!(normal[2].mode, ArgMode::Default);
        assert_eq!(normal[2].name, "x");
        assert_eq!(normal[2].t, Some("%String".to_string()));
        assert_eq!(normal[2].default, Some("\"a\"".to_string()));
        let variadic = variadic.unwrap();
        assert_eq!(variadic.name, "y");
    }

    #[test]
    fn parses_a_default_containing_a_comma_inside_quotes() {
        let (normal, _) = parse_formal_spec(r#"x:%String="a,b""#);
        assert_eq!(normal.len(), 1);
        assert_eq!(normal[0].name, "x");
        assert_eq!(normal[0].t, Some("%String".to_string()));
    }

    #[test]
    fn parses_a_type_containing_a_comma_inside_parens() {
        let (normal, _) = parse_formal_spec("x:%Array(%String,%Integer)");
        assert_eq!(normal.len(), 1);
        assert_eq!(normal[0].name, "x");
        assert_eq!(normal[0].t, Some("%Array(%String,%Integer)".to_string()));
    }

    #[test]
    fn empty_formal_spec_has_no_args() {
        let (normal, variadic) = parse_formal_spec("");
        assert!(normal.is_empty());
        assert!(variadic.is_none());
    }

    #[test]
    fn splits_top_level_commas_but_not_ones_inside_parens_or_quotes() {
        assert_eq!(
            split_top_level(r#"a,b(1,2),"c,d""#),
            vec!["a", "b(1,2)", "\"c,d\""]
        );
    }
}
