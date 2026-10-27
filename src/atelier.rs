//! `Atelier`, the two Atelier REST API requests, and `AtelierContext`, the cached
//! `IrisContext` ascot makes out of them: the `%Dictionary` queries behind `get_cls`, and the
//! parsing behind `get_rtn`. ascot's own `Atelier` is `RawAtelier` (native only).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Deref;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;
use tygr::{Grammar, GrammarRule, Prefix, Raw, StringEq, StringOf1, VecSep};

#[cfg(not(target_arch = "wasm32"))]
use {
    crate::DocKind,
    crate::iris::{DECLARATIONS_VERSION, Declarations},
    base64::Engine,
    percent_encoding::percent_decode_str,
    std::collections::BTreeMap,
    std::sync::atomic::{AtomicUsize, Ordering},
    std::sync::mpsc,
};

use crate::IrisContext;
use crate::bindings::{
    ArgMode, Cardinality, ClassInfo, CollectionKind, MemberInfo, MemberKind, MethodInfo, NameInfo,
    NormalArg, ParameterInfo, Position, PropertyInfo, Range, RelationshipInfo, RoutineInfo, Source,
    VariadicArg,
};
use crate::cos::{LocalVariableName, OSString, Space, Type};

const CACHE_TTL: Duration = Duration::from_secs(60 * 60);
#[cfg(not(target_arch = "wasm32"))]
const BATCH: usize = 100;
#[cfg(not(target_arch = "wasm32"))]
const WORKERS: usize = 8;

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
pub trait Atelier: Send + Sync {
    /// The rows of `/action/query` as a JSON array, each an object keyed by column name.
    fn query(&self, sql: &str, params: &[&str]) -> Result<String, String>;
    /// The text of `/doc/{name}`; `None` if there's no such document.
    fn get_doc(&self, name: &str) -> Result<Option<String>, String>;
}

#[cfg(target_arch = "wasm32")]
impl Atelier for crate::bindings::Atelier {
    fn query(&self, sql: &str, params: &[&str]) -> Result<String, String> {
        let params: Vec<String> = params.iter().map(|param| param.to_string()).collect();
        crate::bindings::Atelier::query(self, sql, &params)
    }

    fn get_doc(&self, name: &str) -> Result<Option<String>, String> {
        crate::bindings::Atelier::get_doc(self, name)
    }
}

/// The `IrisContext` that looks code up through an `Atelier`, caching what it finds.
pub struct AtelierContext {
    atelier: Box<dyn Atelier>,
    cls_cache: Cache<Option<ClassInfo>>,
    routine_cache: Cache<Option<Source>>,
    rtn_cache: Cache<Option<RoutineInfo>>,
}

impl<A: Atelier + 'static> From<A> for AtelierContext {
    fn from(atelier: A) -> Self {
        Self {
            atelier: Box::new(atelier),
            cls_cache: Cache::new(),
            routine_cache: Cache::new(),
            rtn_cache: Cache::new(),
        }
    }
}

impl IrisContext for AtelierContext {
    fn get_cls(&self, cls: &str) -> Result<Option<ClassInfo>, String> {
        if let Some(cached) = self.cls_cache.get(cls) {
            return Ok(cached);
        }
        let result = class_infos(self, "= ?", &[cls])?.into_values().next();
        self.cls_cache.set(cls.to_string(), result.clone());
        Ok(result)
    }

    fn get_inc(&self, name: &str) -> Result<Option<Source>, String> {
        self.fetch_routine(&[format!("{name}.inc")])
    }

    // The `.int` first: it is the instance's own macro expansion of the `.mac`, so no expansion
    // (or include resolution) is needed on our side.
    fn get_rtn(&self, name: &str) -> Result<Option<RoutineInfo>, String> {
        if let Some(cached) = self.rtn_cache.get(name) {
            return Ok(cached);
        }
        let result = match self.fetch_routine(&[format!("{name}.int"), format!("{name}.mac")])? {
            Some(source) => Some(crate::routine_info(&source.text, source.uri)),
            // Without source, its OBJ says whether it exists.
            None => routine_names(self, &format!("{name}.obj"))?
                .contains(name)
                .then_some(RoutineInfo {
                    labels: None,
                    uri: None,
                }),
        };
        self.rtn_cache.set(name.to_string(), result.clone());
        Ok(result)
    }
}

impl AtelierContext {
    fn rows(&self, sql: &str, params: &[&str]) -> Result<Vec<Value>, String> {
        let rows = self.atelier.query(sql, params)?;
        serde_json::from_str(&rows)
            .map_err(|err| format!("query answered with other than rows: {err}"))
    }

    // The first of `docs` that exists, cached under the whole candidate list.
    fn fetch_routine(&self, docs: &[String]) -> Result<Option<Source>, String> {
        let key = docs.join("|");
        if let Some(cached) = self.routine_cache.get(&key) {
            return Ok(cached);
        }
        let mut result = None;
        for doc in docs {
            if let Some(text) = self.atelier.get_doc(doc)? {
                // The Atelier REST API has no uri scheme of its own to offer here.
                result = Some(Source { text, uri: None });
                break;
            }
        }
        self.routine_cache.set(key, result.clone());
        Ok(result)
    }
}

// The routines matching `spec` (e.g. `*.mac,*.int,*.obj`), system ones included and
// class-generated ones (`Name.1.obj`) left out.
fn routine_names(ctx: &AtelierContext, spec: &str) -> Result<BTreeSet<String>, String> {
    let sql = "SELECT Name FROM %Library.RoutineMgr_StudioOpenDialog(?,1,1,1,1,1,0,\
               'NOT (Name %PATTERN ''.E1\".\"0.1\"G\"1N1\".obj\"'' AND $LENGTH(Name,''.'') > 3)')";
    let rows = ctx.rows(sql, &[spec])?;
    Ok(rows
        .iter()
        .filter_map(|row| Some(doc_basename(row.get("Name")?.as_str()?)))
        .collect())
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
fn class_infos(
    conn: &AtelierContext,
    filter: &str,
    params: &[&str],
) -> Result<HashMap<String, ClassInfo>, String> {
    let sql = |description: &str| {
        format!(
            "SELECT Name, {description}, Super, Inheritance, PropertyClass, IncludeCode, ClassType, Deprecated \
             FROM %Dictionary.CompiledClass WHERE Name {filter}"
        )
    };
    // Same `{`-leading-text failure mode `all_mem_rows` guards against.
    let rows = conn
        .rows(&sql("Description"), params)
        .or_else(|_| conn.rows(&sql("'' AS Description"), params))?;
    if rows.is_empty() {
        return Ok(HashMap::new());
    }
    let members = resolve_stubs(conn, filter, params, all_mem_rows(conn, filter, params)?)?;
    Ok(assemble(rows, members))
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
                inheritance_right: str_field("Inheritance") == "right",
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
fn all_mem_rows(
    conn: &AtelierContext,
    filter: &str,
    params: &[&str],
) -> Result<Vec<MemberRow>, String> {
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
    conn.rows(&sql("Description"), &params)
        .or_else(|_| conn.rows(&sql("'' AS Description"), &params))
        .map(|rows| {
            rows.into_iter()
                .filter_map(|row| serde_json::from_value(row).ok())
                .collect()
        })
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
    conn: &AtelierContext,
    filter: &str,
    params: &[&str],
    rows: Vec<MemberRow>,
) -> Result<Vec<MemberRow>, String> {
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
            .rows(&sql("Description"), params)
            .or_else(|_| conn.rows(&sql("'' AS Description"), params))?;
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
    Ok(rows
        .into_iter()
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
        .collect())
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

// A method's `FormalSpec` as `%Dictionary.CompiledMethod` reports it: minified, e.g.
// `*out:%String,&ref:%Integer,x:%String="a",args...`.
#[derive(Grammar)]
struct CompiledFormalSpec(Option<VecSep<CompiledFormal, Comma>>);

type Comma = Raw<(Space, StringEq!(","), Space)>;

#[derive(Grammar)]
struct CompiledFormal {
    call_type: Option<CompiledCallType>,
    name: LocalVariableName,
    variadic: Option<StringEq!("...")>,
    as_type: Option<Prefix<StringEq!(":"), Type>>,
    default: Option<Prefix<StringEq!("="), CompiledDefault>>,
}

#[derive(Grammar)]
enum CompiledCallType {
    Output(StringEq!("*")),
    ByRef(StringEq!("&")),
}

// A default value, up to the next top-level comma: strings, parentheses and braces balanced,
// anything else (a macro, `..#PARAM`) taken as it comes.
#[derive(Grammar)]
struct CompiledDefault(Raw<Vec<CompiledDefaultUnit>>);

#[derive(Grammar)]
enum CompiledDefaultUnit {
    String(OSString),
    Paren(StringEq!("("), Raw<Vec<CompiledNestedUnit>>, StringEq!(")")),
    Brace(StringEq!("{"), Raw<Vec<CompiledNestedUnit>>, StringEq!("}")),
    Text(StringOf1<IsCompiledDefaultText>),
}

#[derive(Grammar)]
enum CompiledNestedUnit {
    Unit(CompiledDefaultUnit),
    Comma(StringEq!(",")),
}

tygr::char_class!(IsCompiledDefaultText, "default_char", |ch| {
    !matches!(ch, ',' | '"' | '(' | ')' | '{' | '}')
});

fn parse_formal_spec(spec: &str) -> (Vec<NormalArg>, Option<VariadicArg>) {
    let Ok(CompiledFormalSpec(Some(formals))) = CompiledFormalSpec::parse(spec) else {
        return (vec![], None);
    };
    let mut normal = vec![];
    let mut variadic = None;
    for formal in &formals.items {
        let mode = match formal.call_type {
            Some(CompiledCallType::Output(_)) => ArgMode::Output,
            Some(CompiledCallType::ByRef(_)) => ArgMode::ByRef,
            None => ArgMode::Default,
        };
        let name = formal.name.print();
        let t = formal.as_type.as_ref().map(|t| t.deref().print());
        if formal.variadic.is_some() {
            variadic = Some(VariadicArg {
                mode,
                name,
                t,
                declared_at: None,
            });
        } else {
            normal.push(NormalArg {
                mode,
                name,
                t,
                default: formal.default.as_ref().map(|d| d.deref().print()),
                declared_at: None,
            });
        }
    }
    (normal, variadic)
}

#[cfg(not(target_arch = "wasm32"))]
/// ascot's own `Atelier`: a connection to IRIS over the Atelier REST API.
pub struct RawAtelier {
    base_url: String,
    // `scheme://host:port/prefix/api/atelier`, for the namespace listing in `check`'s message.
    api_root: String,
    namespace: String,
    username: String,
    auth_header: String,
    // Why IRIS couldn't be reached, once it couldn't: later requests then fail at once instead of
    // each waiting out a timeout.
    agent: Mutex<Result<ureq::Agent, String>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl RawAtelier {
    /// `url` is `scheme://user:pass@host:port`, optionally with a path prefix (e.g. `/iris`,
    /// when the web server routes IRIS instances by path). `namespace` is a plain name (e.g.
    /// `%SYS`) -- not URL-encoded, since it arrives as a separate CLI/tool argument. Makes one
    /// request up front, so a wrong URL, password or namespace is a clear error at startup rather
    /// than a one-line warning followed by every system macro reported undefined.
    pub fn connect(url: &str, namespace: &str) -> Result<Self, String> {
        let parsed = url::Url::parse(url).map_err(|e| format!("invalid --iris URL: {e}"))?;
        let scheme = parsed.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(format!("--iris URL must be http or https, got {scheme:?}"));
        }
        let host = parsed.host_str().ok_or("--iris URL is missing a host")?;
        let port = parsed
            .port_or_known_default()
            .ok_or("--iris URL is missing a port")?;
        let path_prefix = match parsed.path() {
            "" | "/" => "",
            path => path.trim_end_matches('/'),
        };
        // `Url` hands these back still percent-encoded; IRIS wants the real characters.
        let username = percent_decode_str(parsed.username()).decode_utf8_lossy();
        let password = percent_decode_str(parsed.password().unwrap_or("")).decode_utf8_lossy();
        let auth_header = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"))
        );
        let api_root = format!("{scheme}://{host}:{port}{path_prefix}/api/atelier");
        // Bounded to connect and to start answering; a long answer (`declare` reads 100 classes'
        // members at once) takes what it takes.
        // The OS's trust store, as browsers and curl use, rather than rustls's bundled roots: a
        // company CA or an accepted self-signed certificate lives there.
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(5)))
                .timeout_recv_response(Some(Duration::from_secs(60)))
                .tls_config(tls)
                .build(),
        );
        let raw = Self {
            base_url: format!("{api_root}/v8/{}", percent_encode(namespace)),
            api_root,
            namespace: namespace.to_string(),
            username: username.into_owned(),
            auth_header,
            agent: Mutex::new(Ok(agent)),
        };
        raw.check()?;
        Ok(raw)
    }

    fn check(&self) -> Result<(), String> {
        let agent = self.agent()?;
        let get = |url: &str| {
            agent
                .get(url)
                .header("Authorization", &self.auth_header)
                .call()
        };
        match get(&self.base_url) {
            Ok(_) => Ok(()),
            Err(ureq::Error::StatusCode(404)) => {
                let namespaces: Option<Vec<String>> =
                    get(&format!("{}/", self.api_root)).ok().and_then(|resp| {
                        let body: Value = resp.into_body().read_json().ok()?;
                        let names = body.get("result")?.get("content")?.get("namespaces")?;
                        serde_json::from_value(names.clone()).ok()
                    });
                Err(match namespaces {
                    Some(names) => format!(
                        "namespace '{}' does not exist at {}; it has: {}",
                        self.namespace,
                        self.api_root,
                        names.join(", ")
                    ),
                    None => format!(
                        "no Atelier API at {} (http status 404); check the URL's path prefix",
                        self.api_root
                    ),
                })
            }
            Err(ureq::Error::StatusCode(401)) => Err(format!(
                "authentication failed for user '{}' at {} (http status 401)",
                self.username, self.api_root
            )),
            Err(ureq::Error::StatusCode(status)) => {
                Err(format!("{} answered http status {status}", self.base_url))
            }
            Err(ureq::Error::Timeout(_)) => Err(format!("cannot reach {}", self.api_root)),
            Err(err) => Err(format!("cannot reach {}: {err}", self.api_root)),
        }
    }

    /// Every class, include file and routine in the namespace, for `ascot declare`;
    /// `progress(kind, done, total)` once each total is known and after each batch.
    pub fn declarations(
        self,
        mut progress: impl FnMut(DocKind, usize, usize),
    ) -> Result<Declarations, String> {
        let namespace = self.namespace.clone();
        let iris_version = self.server_version()?;
        let ctx = AtelierContext::from(self);
        // `%sqlcq.*` are SQL cached queries; `declare`'s own queries create some, so without this
        // each run would add classes to the next run's file.
        let mut class_names: Vec<String> = ctx
            .rows("SELECT Name FROM %Dictionary.CompiledClass", &[])?
            .into_iter()
            .filter_map(|row| Some(row.get("Name")?.as_str()?.to_string()))
            .filter(|name| !name.starts_with("%sqlcq."))
            .collect();
        class_names.sort();
        let inc_names: Vec<String> = routine_names(&ctx, "*.inc")?.into_iter().collect();
        // Every routine, system ones included, so one without source is still declared.
        let all_rtn_names = routine_names(&ctx, "*.mac,*.int,*.obj")?;
        // Hand-written routines only (a class's `.int`s are generated), read as the `.int`
        // where there is one (compiled, macro-expanded; what `get_rtn` prefers), else the `.mac`.
        let rtn_names: Vec<String> = routine_names(&ctx, "*.mac,*.int")?.into_iter().collect();

        let totals = [class_names.len(), inc_names.len(), rtn_names.len()];
        for kind in [DocKind::Cls, DocKind::Inc, DocKind::Rtn] {
            progress(kind, 0, totals[kind as usize]);
        }
        let jobs: Vec<(DocKind, &[String])> = (class_names.chunks(BATCH))
            .map(|batch| (DocKind::Cls, batch))
            .chain(inc_names.chunks(BATCH).map(|batch| (DocKind::Inc, batch)))
            .chain(rtn_names.chunks(BATCH).map(|batch| (DocKind::Rtn, batch)))
            .collect();
        let next = AtomicUsize::new(0);
        let classes = Mutex::new(BTreeMap::new());
        let includes = Mutex::new(BTreeMap::new());
        let routines = Mutex::new(
            all_rtn_names
                .into_iter()
                .map(|name| {
                    let info = RoutineInfo {
                        labels: None,
                        uri: None,
                    };
                    (name, info)
                })
                .collect::<BTreeMap<_, _>>(),
        );
        let failed = Mutex::new(None);
        let (tx, rx) = mpsc::channel();
        let mut done = [0; 3];
        std::thread::scope(|scope| {
            let (jobs, next, ctx, failed) = (&jobs, &next, &ctx, &failed);
            let (classes, includes, routines) = (&classes, &includes, &routines);
            for _ in 0..WORKERS.min(jobs.len()) {
                let tx = tx.clone();
                scope.spawn(move || {
                    while failed.lock().unwrap().is_none() {
                        let Some(&(kind, batch)) = jobs.get(next.fetch_add(1, Ordering::Relaxed))
                        else {
                            break;
                        };
                        let fetched = match kind {
                            DocKind::Cls => {
                                let params: Vec<&str> = batch.iter().map(String::as_str).collect();
                                let filter = format!("IN ({})", vec!["?"; params.len()].join(","));
                                class_infos(ctx, &filter, &params)
                                    .map(|found| classes.lock().unwrap().extend(found))
                            }
                            DocKind::Inc => sources(ctx, batch, &["inc"])
                                .map(|found| includes.lock().unwrap().extend(found)),
                            DocKind::Rtn => sources(ctx, batch, &["int", "mac"]).map(|found| {
                                let found = found.into_iter().map(|(name, source)| {
                                    (name, crate::routine_info(&source.text, None))
                                });
                                routines.lock().unwrap().extend(found)
                            }),
                        };
                        if let Err(err) = fetched {
                            failed.lock().unwrap().get_or_insert(err);
                            break;
                        }
                        let _ = tx.send((kind, batch.len()));
                    }
                });
            }
            drop(tx);
            for (kind, count) in rx {
                done[kind as usize] += count;
                progress(kind, done[kind as usize], totals[kind as usize]);
            }
        });
        if let Some(err) = failed.into_inner().unwrap() {
            return Err(err);
        }
        Ok(Declarations {
            version: DECLARATIONS_VERSION,
            namespace,
            iris_version,
            classes: classes.into_inner().unwrap(),
            includes: includes.into_inner().unwrap(),
            routines: routines.into_inner().unwrap(),
        })
    }

    fn agent(&self) -> Result<ureq::Agent, String> {
        self.agent.lock().unwrap().clone()
    }

    // The instance's `$ZVERSION`, from the API listing at `api_root/`.
    fn server_version(&self) -> Result<String, String> {
        let resp = self
            .agent()?
            .get(&format!("{}/", self.api_root))
            .header("Authorization", &self.auth_header)
            .call()
            .map_err(|err| self.failed("version", err))?;
        let body: Value = resp
            .into_body()
            .read_json()
            .map_err(|err| self.failed("version", err))?;
        body.get("result")
            .and_then(|r| r.get("content")?.get("version")?.as_str())
            .map(str::to_string)
            .ok_or_else(|| format!("version at {} failed: {}", self.api_root, body["status"]))
    }

    fn failed(&self, context: &str, err: ureq::Error) -> String {
        let message = format!("{context} at {} failed: {err}", self.base_url);
        use ureq::Error::{ConnectionFailed, HostNotFound, Io, Timeout, Tls};
        if matches!(
            err,
            Io(_) | Timeout(_) | HostNotFound | ConnectionFailed | Tls(_)
        ) {
            *self.agent.lock().unwrap() = Err(message.clone());
        }
        message
    }
}

// Each of `names` that has source, read from the first of `exts` it exists as.
#[cfg(not(target_arch = "wasm32"))]
fn sources(
    ctx: &AtelierContext,
    names: &[String],
    exts: &[&str],
) -> Result<Vec<(String, Source)>, String> {
    let mut found = vec![];
    for name in names {
        for ext in exts {
            if let Some(text) = ctx.atelier.get_doc(&format!("{name}.{ext}"))? {
                found.push((name.clone(), Source { text, uri: None }));
                break;
            }
        }
    }
    Ok(found)
}

#[cfg(not(target_arch = "wasm32"))]
impl Atelier for RawAtelier {
    fn query(&self, sql: &str, params: &[&str]) -> Result<String, String> {
        let resp = self
            .agent()?
            .post(&format!("{}/action/query", self.base_url))
            .header("Authorization", &self.auth_header)
            .header("Content-Type", "application/json")
            .send_json(serde_json::json!({ "query": sql, "parameters": params }))
            .map_err(|err| self.failed("query", err))?;
        let body: Value = resp
            .into_body()
            .read_json()
            .map_err(|err| self.failed("query", err))?;
        match body.get("result").and_then(|result| result.get("content")) {
            Some(rows) => Ok(rows.to_string()),
            None => Err(format!(
                "query at {} failed: {}",
                self.base_url, body["status"]
            )),
        }
    }

    fn get_doc(&self, name: &str) -> Result<Option<String>, String> {
        let resp = match self
            .agent()?
            .get(&format!("{}/doc/{}", self.base_url, percent_encode(name)))
            .header("Authorization", &self.auth_header)
            .call()
        {
            Ok(resp) => resp,
            // No such document (e.g. `Name.int` probed before `Name.mac`).
            Err(ureq::Error::StatusCode(404)) => return Ok(None),
            Err(err) => return Err(self.failed("doc", err)),
        };
        let body: Value = resp
            .into_body()
            .read_json()
            .map_err(|err| self.failed("doc", err))?;
        let lines: Vec<&str> = body
            .get("result")
            .and_then(|result| result.get("content"))
            .and_then(Value::as_array)
            .map(|lines| lines.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        // An existing document with no content is as good as none.
        Ok((!lines.is_empty()).then(|| lines.join("\n")))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iris::open;

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
            serde_json::json!({"Name": "A.C", "Super": "", "Inheritance": "right", "ClassType": "datatype", "Deprecated": 1}),
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
        assert!(c.inheritance_right && !b.inheritance_right);
        assert_eq!(c.members[0].name.content, "P");
    }

    #[test]
    fn a_host_answers_the_lookups_through_its_own_atelier() {
        struct Docs;
        impl Atelier for Docs {
            fn query(&self, _: &str, _: &[&str]) -> Result<String, String> {
                Ok("[]".into())
            }
            fn get_doc(&self, name: &str) -> Result<Option<String>, String> {
                Ok((name == "Util.mac").then(|| "ROUTINE Util\nFoo(a)\n Quit".into()))
            }
        }
        let iris = AtelierContext::from(Docs);
        let util = iris.get_rtn("Util").unwrap().unwrap().labels.unwrap();
        let labels: Vec<_> = util.iter().map(|label| label.name.as_str()).collect();
        assert_eq!(labels, ["Foo"]);
        assert!(iris.get_inc("Util").unwrap().is_none());
        assert!(iris.get_rtn("NoSuchRoutine").unwrap().is_none());
        assert!(iris.get_cls("No.Such.Class").unwrap().is_none());
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
        let (normal, _) = parse_formal_spec("x:%String(MAXLEN=50,MINLEN=1)");
        assert_eq!(normal.len(), 1);
        assert_eq!(normal[0].name, "x");
        assert_eq!(normal[0].t, Some("%String(MAXLEN=50,MINLEN=1)".to_string()));
    }

    #[test]
    fn empty_formal_spec_has_no_args() {
        let (normal, variadic) = parse_formal_spec("");
        assert!(normal.is_empty());
        assert!(variadic.is_none());
    }

    #[test]
    fn parses_a_default_containing_a_macro_or_a_nested_call() {
        let (normal, _) = parse_formal_spec(r#"sc:%Status=$$$OK,l=$lb(1,"a,b",$g(x)),o={{}}"#);
        let defaults: Vec<_> = normal.iter().map(|arg| arg.default.as_deref()).collect();
        assert_eq!(
            defaults,
            [Some("$$$OK"), Some(r#"$lb(1,"a,b",$g(x))"#), Some("{{}}")]
        );
    }

    #[test]
    fn open_treats_anything_with_a_scheme_as_a_url() {
        let err = open("ftp://user:pass@localhost:8080", None).err().unwrap();
        assert!(err.contains("must be http or https"), "{err}");
        let url = stub_server(&[("", "401 Unauthorized", "")]);
        let err = open(&url, Some("USER")).err().unwrap();
        assert!(err.contains("authentication failed"), "{err}");
    }

    #[test]
    fn connect_decodes_percent_encoded_credentials() {
        let url =
            stub_server(&[("", "200 OK", "{}")]).replace("user:pass", "us%40er:p%40ss%25word");
        let conn = RawAtelier::connect(&url, "USER").unwrap();
        let expected = base64::engine::general_purpose::STANDARD.encode("us@er:p@ss%word");
        assert_eq!(conn.auth_header, format!("Basic {expected}"));
    }

    #[test]
    fn connect_percent_encodes_the_namespace() {
        let url = stub_server(&[("", "200 OK", "{}")]);
        let conn = RawAtelier::connect(&url, "%SYS").unwrap();
        assert_eq!(
            conn.base_url,
            format!("{}/api/atelier/v8/%25SYS", host(&url))
        );
    }

    #[test]
    fn connect_keeps_a_path_prefix_before_api_atelier() {
        let url = stub_server(&[("", "200 OK", "{}")]);
        let conn = RawAtelier::connect(&format!("{url}/iris"), "%SYS").unwrap();
        assert_eq!(
            conn.base_url,
            format!("{}/iris/api/atelier/v8/%25SYS", host(&url))
        );
    }

    fn host(url: &str) -> String {
        url.replace("user:pass@", "")
    }

    // An HTTP server answering each request with the first of `routes` whose path suffix
    // matches the request's path (`("", ...)` matches anything).
    fn stub_server(routes: &'static [(&str, &str, &str)]) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://user:pass@{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buf = [0; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let path = request.split_whitespace().nth(1).unwrap_or("");
                let (_, status, body) = routes
                    .iter()
                    .find(|(suffix, ..)| path.ends_with(suffix))
                    .unwrap();
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        url
    }

    #[test]
    fn server_version_reads_the_api_listing() {
        let url = stub_server(&[
            (
                "/api/atelier/",
                "200 OK",
                r#"{"result":{"content":{"version":"IRIS for UNIX 2026.1 (Build 220U)","namespaces":["%SYS"]}}}"#,
            ),
            ("", "200 OK", "{}"),
        ]);
        let conn = RawAtelier::connect(&url, "%SYS").unwrap();
        assert_eq!(
            conn.server_version().as_deref(),
            Ok("IRIS for UNIX 2026.1 (Build 220U)")
        );
    }

    #[test]
    fn check_names_the_available_namespaces_when_the_requested_one_is_missing() {
        // The listing lives at `/api/atelier/` (trailing slash); the namespace itself 404s.
        let url = stub_server(&[
            (
                "/api/atelier/",
                "200 OK",
                r#"{"result":{"content":{"namespaces":["%SYS","USER"]}}}"#,
            ),
            ("", "404 Not Found", ""),
        ]);
        let err = RawAtelier::connect(&url, "NOPE").err().unwrap();
        assert!(err.contains("namespace 'NOPE' does not exist"), "{err}");
        assert!(err.contains("%SYS, USER"), "{err}");
    }

    #[test]
    fn check_blames_the_path_prefix_when_there_is_no_atelier_api_at_all() {
        let url = stub_server(&[("", "404 Not Found", "")]);
        let err = RawAtelier::connect(&url, "USER").err().unwrap();
        assert!(err.contains("no Atelier API at"), "{err}");
    }

    #[test]
    fn check_reports_a_bad_password() {
        let url = stub_server(&[("", "401 Unauthorized", "")]);
        let err = RawAtelier::connect(&url, "USER").err().unwrap();
        assert!(
            err.contains("authentication failed for user 'user'"),
            "{err}"
        );
    }

    #[test]
    fn check_passes_on_a_reachable_namespace() {
        let url = stub_server(&[("", "200 OK", r#"{"result":{"content":{"name":"USER"}}}"#)]);
        RawAtelier::connect(&url, "USER").unwrap();
    }

    #[test]
    fn connect_strips_a_trailing_slash_from_the_path_prefix() {
        let url = stub_server(&[("", "200 OK", "{}")]);
        let conn = RawAtelier::connect(&format!("{url}/iris/"), "%SYS").unwrap();
        assert_eq!(
            conn.base_url,
            format!("{}/iris/api/atelier/v8/%25SYS", host(&url))
        );
    }
}
