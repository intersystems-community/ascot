//! Optional live-IRIS lookups over the Atelier REST API (`--iris <url>` on the CLI and LSP, the
//! `iris` argument of the MCP `create_workspace` tool).
//!
//! Supplies classes, `.inc` files and routines that aren't in the local workspace, system and
//! library code above all, so the checker can confirm them instead of leaving a hint. Results
//! are cached per connection. `IrisClient` is the trait `lib.rs` depends on; `AtelierConnection`
//! is its HTTP implementation.
//!
//! A native (non-wasm32) `IrisConnection`-equivalent backed by the Atelier REST API, ported from
//! `language-server`'s `server/src/ascot/index.ts` `IrisConnection` class -- the JS host-side
//! implementation of the same WIT `imported.iris-connection` interface, which can't be
//! implemented by native Rust guest code at all (see `src/lib.rs`'s `IrisHandle`).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use base64::Engine;
use percent_encoding::percent_decode_str;
use serde::Deserialize;
use serde_json::Value;

use crate::bindings::{
    ArgMode, ClassInfo, CollectionKind, MemberInfo, MemberKind, MethodInfo, NameInfo, NormalArg,
    ParameterInfo, Position, PropertyInfo, Range, RoutineSource, VariadicArg,
};

pub trait IrisClient: Send + Sync {
    // `cls`'s own (not inherited) members, superclasses, and other class-level metadata --
    // everything ascot needs to resolve a member, "did you mean" a name, or check a type
    // parameter, in one round trip. `None` if `cls` doesn't exist (or can't be reached).
    fn get_cls(&self, cls: &str) -> Option<ClassInfo>;
    // The raw source of the include file `name.inc` (an `#include`d name), for one that isn't
    // already open in the workspace.
    fn get_inc(&self, name: &str) -> Option<RoutineSource>;
    // The raw source of the routine `name` (`.mac` if it exists, else `.int`), for a `Label^name`
    // call into a routine that isn't open in the workspace.
    fn get_int_or_mac(&self, name: &str) -> Option<RoutineSource>;
}

const CACHE_TTL: Duration = Duration::from_secs(60 * 60);

struct Cache<V: Clone>(Mutex<HashMap<String, (Instant, V)>>);

impl<V: Clone> Cache<V> {
    fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }

    fn get(&self, key: &str) -> Option<V> {
        let entries = self.0.lock().unwrap();
        let (at, value) = entries.get(key)?;
        (at.elapsed() < CACHE_TTL).then(|| value.clone())
    }

    fn set(&self, key: String, value: V) {
        self.0.lock().unwrap().insert(key, (Instant::now(), value));
    }
}

pub struct AtelierConnection {
    base_url: String,
    // `scheme://host:port/prefix/api/atelier`, for the namespace listing in `check`'s message.
    api_root: String,
    namespace: String,
    username: String,
    auth_header: String,
    agent: ureq::Agent,
    // Set on the first request failure, so we warn once (not once per lookup) and short-circuit
    // every later call instead of re-attempting and re-timing-out on a connection already known
    // to be broken.
    broken: AtomicBool,
    cls_cache: Cache<Option<ClassInfo>>,
    routine_cache: Cache<Option<RoutineSource>>,
}

impl AtelierConnection {
    /// `url` is `scheme://user:pass@host:port`, optionally with a path prefix (e.g. `/iris`,
    /// when the web server routes IRIS instances by path). `namespace` is a plain name (e.g.
    /// `%SYS`) -- not URL-encoded, since it arrives as a separate CLI/tool argument.
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
        Ok(Self {
            base_url: format!("{api_root}/v1/{}", percent_encode(namespace)),
            api_root,
            namespace: namespace.to_string(),
            username: username.into_owned(),
            auth_header,
            agent: ureq::Agent::new_with_config(
                ureq::Agent::config_builder()
                    .timeout_connect(Some(Duration::from_secs(5)))
                    .timeout_global(Some(Duration::from_secs(20)))
                    .build(),
            ),
            broken: AtomicBool::new(false),
            cls_cache: Cache::new(),
            routine_cache: Cache::new(),
        })
    }

    /// One request up front, so a wrong URL, password or namespace is a clear error at startup
    /// rather than a one-line warning followed by every system macro reported undefined.
    pub fn check(&self) -> Result<(), String> {
        let get = |url: &str| {
            self.agent
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
            Err(err) => Err(format!("cannot reach {}: {err}", self.api_root)),
        }
    }

    fn fail(&self, context: &str, err: ureq::Error) {
        if !self.broken.swap(true, Ordering::Relaxed) {
            eprintln!(
                "warning: IRIS connection at {} failed ({context}: {err}); falling back to \
                 unconfirmed lookups for the rest of this session",
                self.base_url
            );
        }
    }

    fn query(&self, sql: &str, params: &[&str]) -> Option<Vec<Value>> {
        if self.broken.load(Ordering::Relaxed) {
            return None;
        }
        let resp = self
            .agent
            .post(&format!("{}/action/query", self.base_url))
            .header("Authorization", &self.auth_header)
            .header("Content-Type", "application/json")
            .send_json(serde_json::json!({ "query": sql, "parameters": params }))
            .map_err(|err| self.fail("query", err))
            .ok()?;
        let body: Value = resp.into_body().read_json().ok()?;
        body.get("result")?.get("content")?.as_array().cloned()
    }

    fn get_doc(&self, path: &str) -> Option<Vec<String>> {
        if self.broken.load(Ordering::Relaxed) {
            return None;
        }
        let resp = match self
            .agent
            .get(&format!("{}/doc/{}", self.base_url, percent_encode(path)))
            .header("Authorization", &self.auth_header)
            .call()
        {
            Ok(resp) => resp,
            // No such document (e.g. `Name.inc` probed before `Name.mac`) -- says nothing about
            // the connection itself.
            Err(ureq::Error::StatusCode(404)) => return None,
            Err(err) => {
                self.fail("doc", err);
                return None;
            }
        };
        let body: Value = resp.into_body().read_json().ok()?;
        let lines = body.get("result")?.get("content")?.as_array()?;
        Some(
            lines
                .iter()
                .filter_map(|l| l.as_str().map(str::to_string))
                .collect(),
        )
    }
}

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

impl IrisClient for AtelierConnection {
    fn get_cls(&self, cls: &str) -> Option<ClassInfo> {
        if let Some(cached) = self.cls_cache.get(cls) {
            return cached;
        }
        let result = get_cls_uncached(self, cls);
        self.cls_cache.set(cls.to_string(), result.clone());
        result
    }

    fn get_inc(&self, name: &str) -> Option<RoutineSource> {
        self.fetch_routine(&[format!("{name}.inc")])
    }

    // The `.int` first: it is the instance's own macro expansion of the `.mac`, so no expansion
    // (or include resolution) is needed on our side.
    fn get_int_or_mac(&self, name: &str) -> Option<RoutineSource> {
        self.fetch_routine(&[format!("{name}.int"), format!("{name}.mac")])
    }
}

impl AtelierConnection {
    // The first of `docs` that exists, cached under the whole candidate list.
    fn fetch_routine(&self, docs: &[String]) -> Option<RoutineSource> {
        let key = docs.join("|");
        if let Some(cached) = self.routine_cache.get(&key) {
            return cached;
        }
        let result = docs
            .iter()
            .find_map(|doc| self.get_doc(doc))
            .map(|lines| RoutineSource {
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

fn get_cls_uncached(conn: &AtelierConnection, cls: &str) -> Option<ClassInfo> {
    let sql = |description: &str| {
        format!(
            "SELECT {description}, Super, PropertyClass, IncludeCode, ClassType, Deprecated FROM \
             %Dictionary.CompiledClass WHERE Name = ?"
        )
    };
    // Same `{`-leading-text failure mode `all_mem_uncached` guards against.
    let row = conn
        .query(&sql("Description"), &[cls])
        .or_else(|| conn.query(&sql("'' AS Description"), &[cls]))
        .and_then(|rows| rows.into_iter().next())?;
    let str_field = |key: &str| row.get(key).and_then(Value::as_str).unwrap_or_default();
    let zero = Position {
        line: 0,
        character: 0,
    };
    let members = resolve_stubs(conn, cls, all_mem_uncached(conn, cls))
        .into_iter()
        .filter(|row| !row.name.is_empty())
        .map(member_row_to_info)
        .collect();
    Some(ClassInfo {
        doc: str_field("Description").to_string(),
        name: NameInfo {
            before: zero,
            content: cls.to_string(),
            after: zero,
        },
        extends: split_classes(str_field("Super")),
        deprecated: is_truthy(&row.get("Deprecated").cloned().unwrap_or(Value::Null)),
        members,
        property_class_names: split_classes(str_field("PropertyClass")),
        includes: split_classes(str_field("IncludeCode")),
        is_datatype: Some(str_field("ClassType") == "datatype"),
        // The Atelier REST API has no uri scheme of its own to offer here.
        uri: None,
    })
}

// Uniform row shape across all three member tables `all_mem_uncached` unions together. IRIS
// reports an unset SQL string column as an empty string, not JSON `null` -- normalized to `None`
// here, at the boundary, so nothing downstream has to know that (or risk treating "" as a real,
// if nonsense, class/type name).
#[derive(Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
struct MemberRow {
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

// Every member declared directly on `cls`, across all three tables -- one query per class
// instead of one per member, since `get_cls` always wants the whole list at once.
fn all_mem_uncached(conn: &AtelierConnection, cls: &str) -> Vec<MemberRow> {
    let sql = |description: &str| {
        format!(
            "SELECT Name, NULL AS Aliases, 'method' AS MemberType, {description}, FormalSpec, \
             ReturnType, NULL AS Collection, 0 AS MultiDimensional, 0 AS Relationship, NULL AS \
             Cardinality, Stub, ClassMethod, Deprecated, Origin FROM %Dictionary.CompiledMethod \
             WHERE Parent = ? UNION ALL SELECT Name, Aliases, 'property' AS MemberType, \
             {description}, NULL AS FormalSpec, Type AS ReturnType, Collection, MultiDimensional, \
             Relationship, Cardinality, NULL AS Stub, 0 AS ClassMethod, Deprecated, Origin FROM \
             %Dictionary.CompiledProperty WHERE Parent = ? UNION ALL SELECT Name, NULL AS \
             Aliases, 'parameter' AS MemberType, {description}, NULL AS FormalSpec, Type AS \
             ReturnType, NULL AS Collection, 0 AS MultiDimensional, 0 AS Relationship, NULL AS \
             Cardinality, NULL AS Stub, 0 AS ClassMethod, Deprecated, Origin FROM \
             %Dictionary.CompiledParameter WHERE Parent = ?"
        )
    };
    // The Atelier query API fails the whole query over a text value that merely starts with `{`
    // (e.g. a `/// {$$$CPFSave+...}` parameter comment); retry without descriptions.
    conn.query(&sql("Description"), &[cls, cls, cls])
        .or_else(|| conn.query(&sql("'' AS Description"), &[cls, cls, cls]))
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

// Replaces each stub row with its real metadata -- one query per stub table the class uses,
// not one per stub (a property-heavy class has dozens of generated accessors).
fn resolve_stubs(conn: &AtelierConnection, cls: &str, rows: Vec<MemberRow>) -> Vec<MemberRow> {
    let tables: HashSet<&str> = rows
        .iter()
        .filter_map(stub_parts)
        .map(|(_, _, t)| t)
        .collect();
    let mut resolved: HashMap<(String, String, &str), MemberRow> = HashMap::new();
    for table in tables {
        let sql = |description: &str| {
            format!(
                "SELECT Name, parent->Name AS StubOrigin, 'method' AS MemberType, {description}, \
                 FormalSpec, ReturnType, '' AS Stub, ClassMethod, Deprecated FROM \
                 %Dictionary.Compiled{table}Method WHERE parent->Parent = ?"
            )
        };
        // Same `{`-leading-text failure mode `all_mem_uncached` guards against.
        let found = conn
            .query(&sql("Description"), &[cls])
            .or_else(|| conn.query(&sql("'' AS Description"), &[cls]))
            .unwrap_or_default();
        for value in found {
            let origin = value
                .get("StubOrigin")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let Ok(row) = serde_json::from_value::<MemberRow>(value.clone()) else {
                continue;
            };
            resolved.insert((origin.to_string(), row.name.clone(), table), row);
        }
    }
    rows.into_iter()
        .map(|row| match stub_parts(&row) {
            Some((origin, subname, table)) => resolved
                .remove(&(origin.to_string(), subname.to_string(), table))
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
        // A `many`/`children` relationship is the collection of the related objects.
        "property" if is_truthy(&row.relationship) => MemberKind::Relationship(
            match row
                .cardinality
                .as_deref()
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("many" | "children") => Some("%Library.RelationshipObject".to_string()),
                _ => row.return_type.clone(),
            },
        ),
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
    fn connect_decodes_percent_encoded_credentials() {
        let conn =
            AtelierConnection::connect("http://us%40er:p%40ss%25word@localhost:8080", "USER")
                .unwrap();
        let expected = base64::engine::general_purpose::STANDARD.encode("us@er:p@ss%word");
        assert_eq!(conn.auth_header, format!("Basic {expected}"));
    }

    #[test]
    fn connect_percent_encodes_the_namespace() {
        let conn = AtelierConnection::connect("http://user:pass@localhost:8080", "%SYS").unwrap();
        assert_eq!(conn.base_url, "http://localhost:8080/api/atelier/v1/%25SYS");
    }

    #[test]
    fn connect_keeps_a_path_prefix_before_api_atelier() {
        let conn =
            AtelierConnection::connect("http://user:pass@localhost:8080/iris", "%SYS").unwrap();
        assert_eq!(
            conn.base_url,
            "http://localhost:8080/iris/api/atelier/v1/%25SYS"
        );
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
        let err = AtelierConnection::connect(&url, "NOPE")
            .unwrap()
            .check()
            .unwrap_err();
        assert!(err.contains("namespace 'NOPE' does not exist"), "{err}");
        assert!(err.contains("%SYS, USER"), "{err}");
    }

    #[test]
    fn check_blames_the_path_prefix_when_there_is_no_atelier_api_at_all() {
        let url = stub_server(&[("", "404 Not Found", "")]);
        let err = AtelierConnection::connect(&url, "USER")
            .unwrap()
            .check()
            .unwrap_err();
        assert!(err.contains("no Atelier API at"), "{err}");
    }

    #[test]
    fn check_reports_a_bad_password() {
        let url = stub_server(&[("", "401 Unauthorized", "")]);
        let err = AtelierConnection::connect(&url, "USER")
            .unwrap()
            .check()
            .unwrap_err();
        assert!(
            err.contains("authentication failed for user 'user'"),
            "{err}"
        );
    }

    #[test]
    fn check_passes_on_a_reachable_namespace() {
        let url = stub_server(&[("", "200 OK", r#"{"result":{"content":{"name":"USER"}}}"#)]);
        AtelierConnection::connect(&url, "USER")
            .unwrap()
            .check()
            .unwrap();
    }

    #[test]
    fn connect_strips_a_trailing_slash_from_the_path_prefix() {
        let conn =
            AtelierConnection::connect("http://user:pass@localhost:8080/iris/", "%SYS").unwrap();
        assert_eq!(
            conn.base_url,
            "http://localhost:8080/iris/api/atelier/v1/%25SYS"
        );
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
