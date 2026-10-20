//! Optional live-IRIS lookups over the Atelier REST API (`--iris <url>` on the CLI and LSP, the
//! `iris` argument of the MCP `create_workspace` tool), or the same answers read back from a
//! declaration file `ascot declare` wrote (`--iris <file>`).
//!
//! Supplies classes, `.inc` files and routines that aren't in the local workspace, system and
//! library code above all, so the checker can confirm them instead of leaving a hint. Results
//! are cached per connection. `IrisClient` is the trait `lib.rs` depends on; `AtelierConnection`
//! is its HTTP implementation and `Declarations` the offline one; `open` picks between them.
//!
//! A native (non-wasm32) `IrisConnection`-equivalent backed by the Atelier REST API, ported from
//! `language-server`'s `server/src/ascot/index.ts` `IrisConnection` class -- the JS host-side
//! implementation of the same WIT `imported.iris-connection` interface, which can't be
//! implemented by native Rust guest code at all (see `src/lib.rs`'s `IrisHandle`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use base64::Engine;
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
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

pub const DEFAULT_NAMESPACE: &str = "%SYS";

/// Opens an `--iris` argument: a `scheme://user:pass@host:port[/prefix]` URL is a live
/// connection (checked up front), anything else the path of a declaration file `ascot declare`
/// wrote. `namespace` defaults to `%SYS` for a URL; for a file it must match the one declared.
pub fn open(iris: &str, namespace: Option<&str>) -> Result<Box<dyn IrisClient>, String> {
    if iris.contains("://") {
        let conn = AtelierConnection::connect(iris, namespace.unwrap_or(DEFAULT_NAMESPACE))?;
        conn.check()?;
        return Ok(Box::new(conn));
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

const DECLARATIONS_VERSION: u32 = 1;
// Classes or documents per round trip in `AtelierConnection::declarations`.
const BATCH: usize = 100;

/// One namespace as `ascot declare` writes it: the three `IrisClient` lookups, each as a map
/// from its argument to its answer, so analysis can run against a file instead of a live
/// instance. A class is its `get_cls` metadata (no method code); an include file or routine is
/// its source, as `get_inc`/`get_int_or_mac` would fetch it.
#[derive(Serialize, Deserialize, Clone)]
pub struct Declarations {
    pub version: u32,
    pub namespace: String,
    pub classes: BTreeMap<String, ClassInfo>,
    /// `.inc` files by name without the extension.
    pub includes: BTreeMap<String, RoutineSource>,
    /// Routines by name without the extension: the `.int` where there is one, else the `.mac`.
    pub routines: BTreeMap<String, RoutineSource>,
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
                "{} is a version {} declaration file; this ascot reads version {DECLARATIONS_VERSION}",
                path.display(),
                declarations.version
            ));
        }
        Ok(declarations)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }
}

impl IrisClient for Declarations {
    fn get_cls(&self, cls: &str) -> Option<ClassInfo> {
        self.classes.get(cls).cloned()
    }

    fn get_inc(&self, name: &str) -> Option<RoutineSource> {
        self.includes.get(name).cloned()
    }

    fn get_int_or_mac(&self, name: &str) -> Option<RoutineSource> {
        self.routines.get(name).cloned()
    }
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
            Err(ureq::Error::Timeout(_)) => Err(format!("cannot reach {}", self.api_root)),
            Err(err) => Err(format!("cannot reach {}: {err}", self.api_root)),
        }
    }

    /// Every class, include file and routine in the namespace, `BATCH` at a time: one
    /// round trip per dictionary table (or per document batch), not per class or document.
    /// `progress` gets a one-line status after each batch.
    pub fn declarations(&self, mut progress: impl FnMut(&str)) -> Result<Declarations, String> {
        let failed = || format!("IRIS connection at {} failed", self.base_url);
        let classes = self.declare_classes(&failed, &mut progress)?;

        let mut includes = BTreeMap::new();
        let inc_names = self.docnames("INC").ok_or_else(failed)?;
        for (i, batch) in inc_names.chunks(BATCH).enumerate() {
            for (doc, source) in self.docs(batch).ok_or_else(failed)? {
                includes.insert(doc_basename(&doc), source);
            }
            progress(&format!(
                "{}/{} include files",
                i * BATCH + batch.len(),
                inc_names.len()
            ));
        }

        // Hand-written routines only (`generated=0` skips a class's `.int`s). Read as the `.int`
        // where there is one -- the compiled, macro-expanded form, as `get_int_or_mac` prefers --
        // else as the `.mac`.
        let mut rtn_names: Vec<String> = [
            self.docnames("MAC").ok_or_else(failed)?,
            self.docnames("INT").ok_or_else(failed)?,
        ]
        .concat()
        .iter()
        .map(|doc| doc_basename(doc))
        .collect();
        rtn_names.sort();
        rtn_names.dedup();
        let mut routines = BTreeMap::new();
        for (i, batch) in rtn_names.chunks(BATCH).enumerate() {
            let ints: Vec<String> = batch.iter().map(|name| format!("{name}.int")).collect();
            let mut found = self.docs(&ints).ok_or_else(failed)?;
            let macs: Vec<String> = batch
                .iter()
                .filter(|name| !found.contains_key(&format!("{name}.int")))
                .map(|name| format!("{name}.mac"))
                .collect();
            if !macs.is_empty() {
                found.extend(self.docs(&macs).ok_or_else(failed)?);
            }
            for (doc, source) in found {
                routines.insert(doc_basename(&doc), source);
            }
            progress(&format!(
                "{}/{} routines",
                i * BATCH + batch.len(),
                rtn_names.len()
            ));
        }

        Ok(Declarations {
            version: DECLARATIONS_VERSION,
            namespace: self.namespace.clone(),
            classes,
            includes,
            routines,
        })
    }

    fn declare_classes(
        &self,
        failed: &impl Fn() -> String,
        progress: &mut impl FnMut(&str),
    ) -> Result<BTreeMap<String, ClassInfo>, String> {
        let mut names: Vec<String> = self
            .query("SELECT Name FROM %Dictionary.CompiledClass", &[])
            .ok_or_else(failed)?
            .into_iter()
            .filter_map(|row| Some(row.get("Name")?.as_str()?.to_string()))
            .collect();
        names.sort();
        let mut classes = BTreeMap::new();
        for (i, batch) in names.chunks(BATCH).enumerate() {
            let params: Vec<&str> = batch.iter().map(String::as_str).collect();
            let filter = format!("IN ({})", vec!["?"; params.len()].join(","));
            classes.extend(class_infos(self, &filter, &params));
            if self.broken.load(Ordering::Relaxed) {
                return Err(failed());
            }
            progress(&format!(
                "{}/{} classes",
                i * BATCH + batch.len(),
                names.len()
            ));
        }
        Ok(classes)
    }

    // The names (with extension) of the namespace's non-generated documents of routine type
    // `kind` (`INC`, `MAC`, `INT`).
    fn docnames(&self, kind: &str) -> Option<Vec<String>> {
        if self.broken.load(Ordering::Relaxed) {
            return None;
        }
        let resp = self
            .agent
            .get(&format!(
                "{}/docnames/RTN/{kind}?generated=0",
                self.base_url
            ))
            .header("Authorization", &self.auth_header)
            .call()
            .map_err(|err| self.fail("docnames", err))
            .ok()?;
        let body: Value = resp.into_body().read_json().ok()?;
        let docs = body.get("result")?.get("content")?.as_array()?;
        Some(
            docs.iter()
                .filter_map(|doc| Some(doc.get("name")?.as_str()?.to_string()))
                .collect(),
        )
    }

    // The source of each of `names` (with extension) that exists, by name -- one request for the
    // whole batch. A document that doesn't exist comes back with an error `status` and no
    // content.
    fn docs(&self, names: &[String]) -> Option<HashMap<String, RoutineSource>> {
        if self.broken.load(Ordering::Relaxed) {
            return None;
        }
        let resp = self
            .agent
            .post(&format!("{}/docs", self.base_url))
            .header("Authorization", &self.auth_header)
            .header("Content-Type", "application/json")
            .send_json(names)
            .map_err(|err| self.fail("docs", err))
            .ok()?;
        let body: Value = resp.into_body().read_json().ok()?;
        let docs = body.get("result")?.get("content")?.as_array()?;
        Some(
            docs.iter()
                .filter_map(|doc| {
                    let lines = doc.get("content")?.as_array()?;
                    if lines.is_empty() {
                        return None;
                    }
                    let text: Vec<&str> = lines.iter().filter_map(Value::as_str).collect();
                    let source = RoutineSource {
                        text: text.join("\n"),
                        // The Atelier REST API has no uri scheme of its own to offer here.
                        uri: None,
                    };
                    Some((doc.get("name")?.as_str()?.to_string(), source))
                })
                .collect(),
        )
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

// `Foo.Bar.inc` -> `Foo.Bar`.
fn doc_basename(doc: &str) -> String {
    doc.rsplit_once('.')
        .map_or(doc, |(name, _)| name)
        .to_string()
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
        let result = class_infos(self, "= ?", &[cls]).into_values().next();
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

// Every class whose name satisfies `Name {filter}` (`= ?`, `IN (?,?,...)`; `params` bind the
// `?`s), keyed by name, with its own (not inherited) members. One round trip per dictionary
// table for the whole set.
fn class_infos(
    conn: &AtelierConnection,
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

// Pairs each `%Dictionary.CompiledClass` row with the member rows whose `Parent` it is.
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
    #[serde(default)]
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
fn all_mem_rows(conn: &AtelierConnection, filter: &str, params: &[&str]) -> Vec<MemberRow> {
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
    conn: &AtelierConnection,
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

    fn class_info(name: &str) -> ClassInfo {
        let zero = Position {
            line: 0,
            character: 0,
        };
        ClassInfo {
            doc: String::new(),
            name: NameInfo {
                before: zero,
                content: name.to_string(),
                after: zero,
            },
            extends: vec!["%RegisteredObject".to_string()],
            deprecated: false,
            members: vec![],
            property_class_names: vec![],
            includes: vec![],
            is_datatype: Some(false),
            uri: None,
        }
    }

    fn declaration_file(name: &str, declarations: &Declarations) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("ascot-{name}-{}.json", std::process::id()));
        declarations.save(&path).unwrap();
        path
    }

    #[test]
    fn assemble_groups_member_rows_by_their_parent_class() {
        let class_rows = vec![
            serde_json::json!({"Name": "A.B", "Super": "%RegisteredObject", "ClassType": ""}),
            serde_json::json!({"Name": "A.C", "Super": "", "ClassType": "datatype", "Deprecated": 1}),
        ];
        let member_rows: Vec<MemberRow> = [
            serde_json::json!({"Parent": "A.C", "Name": "P", "MemberType": "property", "Description": "", "FormalSpec": "", "ReturnType": "%String", "Stub": "", "ClassMethod": 0, "Deprecated": 0}),
            serde_json::json!({"Parent": "A.B", "Name": "M", "MemberType": "method", "Description": "", "FormalSpec": "x:%String", "ReturnType": "%Status", "Stub": "", "ClassMethod": 1, "Deprecated": 0}),
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

    fn source(text: &str) -> RoutineSource {
        RoutineSource {
            text: text.to_string(),
            uri: None,
        }
    }

    #[test]
    fn declarations_round_trip_through_their_file_and_answer_every_lookup() {
        let declarations = Declarations {
            version: DECLARATIONS_VERSION,
            namespace: "USER".to_string(),
            classes: [("A.B".to_string(), class_info("A.B"))].into(),
            includes: [(
                "MyInc".to_string(),
                source("ROUTINE MyInc [Type=INC]\n#define FIVE 5\n"),
            )]
            .into(),
            routines: [
                (
                    "Lib".to_string(),
                    source(
                        "ROUTINE Lib [Type=INT]\n Quit\nPlain\n Quit\nWithArgs(a,&b,c...)\n \
                         Quit\nProc(a,b) [x,y] {\n Quit\n}\n",
                    ),
                ),
                // Doesn't parse: its labels are unknown, so a call to any of them is taken on
                // trust, as it is live.
                (
                    "Unparsed".to_string(),
                    source("ROUTINE Unparsed [Type=INT]\n{\n"),
                ),
            ]
            .into(),
        };
        let path = declaration_file("round-trip", &declarations);
        let loaded = Declarations::load(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded.namespace, "USER");
        assert_eq!(loaded.get_cls("A.B"), Some(class_info("A.B")));
        assert_eq!(loaded.get_cls("A.C"), None);
        assert_eq!(
            loaded.get_inc("MyInc"),
            Some(declarations.includes["MyInc"].clone())
        );
        assert_eq!(loaded.get_inc("%occErrors"), None);
        assert_eq!(loaded.get_int_or_mac("Nope"), None);
        assert_eq!(doc_basename("Foo.Bar.inc"), "Foo.Bar");

        // Through the checker: the include's macro, a call checked against a label's signature,
        // and a call into the routine whose labels are unknown.
        use crate::bindings::{DiagnosticSeverity, GuestWorkspace};
        let check = |body: &str| {
            let workspace = crate::Workspace::new_with_iris(Some(Box::new(loaded.clone())));
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
            vec!["Routine 'NoSuchRoutine' is not defined in the current workspace"]
        );
    }

    #[test]
    fn open_reads_a_declaration_file_and_checks_its_namespace() {
        let declarations = Declarations {
            version: DECLARATIONS_VERSION,
            namespace: "USER".to_string(),
            classes: BTreeMap::new(),
            includes: BTreeMap::new(),
            routines: BTreeMap::new(),
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
            r#"{"version":2,"namespace":"USER","classes":{},"includes":{},"routines":{}}"#,
        )
        .unwrap();
        let err = open(path.to_str().unwrap(), None).err().unwrap();
        assert!(err.contains("is a version 2 declaration file"), "{err}");
        std::fs::write(&path, "Class A.B {}").unwrap();
        let err = open(path.to_str().unwrap(), None).err().unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(err.contains("is not an ascot declaration file"), "{err}");
        let err = open("/no/such/declarations.json", None).err().unwrap();
        assert!(err.contains("cannot read declaration file"), "{err}");
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
