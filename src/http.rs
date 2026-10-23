//! ascot's own connection to IRIS over the Atelier REST API (`--iris <url>` on the CLI and LSP,
//! the `iris` argument of the MCP `create_workspace` tool), and `ascot declare`.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

use base64::Engine;
use percent_encoding::percent_decode_str;
use serde_json::Value;

use crate::atelier::{AtelierLike, Cached, class_infos, routine_names};
use crate::bindings::{ClassInfo, RoutineInfo, Source};
use crate::iris::{DECLARATIONS_VERSION, Declarations};
use crate::{DocKind, IrisClient};

const BATCH: usize = 100;
const WORKERS: usize = 8;

pub struct AtelierConnection(Cached<Http>);

impl AtelierConnection {
    /// `url` is `scheme://user:pass@host:port`, optionally with a path prefix (e.g. `/iris`,
    /// when the web server routes IRIS instances by path). `namespace` is a plain name (e.g.
    /// `%SYS`) -- not URL-encoded, since it arrives as a separate CLI/tool argument. Makes one
    /// request up front, so a wrong URL, password or namespace is a clear error at startup rather
    /// than a one-line warning followed by every system macro reported undefined.
    pub fn connect(url: &str, namespace: &str) -> Result<Self, String> {
        Ok(Self(Cached::new(Http::connect(url, namespace)?)))
    }

    /// Every class, include file and routine in the namespace, for `ascot declare`;
    /// `progress(kind, done, total)` once each total is known and after each batch.
    pub fn declarations(
        &self,
        progress: impl FnMut(DocKind, usize, usize),
    ) -> Result<Declarations, String> {
        self.0.atelier.declarations(progress)
    }
}

impl IrisClient for AtelierConnection {
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

struct Http {
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
}

impl Http {
    fn connect(url: &str, namespace: &str) -> Result<Self, String> {
        let http = Self::new(url, namespace)?;
        http.check()?;
        Ok(http)
    }

    fn new(url: &str, namespace: &str) -> Result<Self, String> {
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
            base_url: format!("{api_root}/v8/{}", percent_encode(namespace)),
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
        })
    }

    fn check(&self) -> Result<(), String> {
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

    fn declarations(
        &self,
        mut progress: impl FnMut(DocKind, usize, usize),
    ) -> Result<Declarations, String> {
        let failed = || format!("IRIS connection at {} failed", self.base_url);
        // `%sqlcq.*` are SQL cached queries; `declare`'s own queries create some, so without this
        // each run would add classes to the next run's file.
        let mut class_names: Vec<String> = self
            .query("SELECT Name FROM %Dictionary.CompiledClass", &[])
            .ok_or_else(failed)?
            .into_iter()
            .filter_map(|row| Some(row.get("Name")?.as_str()?.to_string()))
            .filter(|name| !name.starts_with("%sqlcq."))
            .collect();
        class_names.sort();
        let inc_names: Vec<String> = routine_names(self, "*.inc")
            .ok_or_else(failed)?
            .into_iter()
            .collect();
        // Every routine, system ones included, so one without source is still declared.
        let all_rtn_names = routine_names(self, "*.mac,*.int,*.obj").ok_or_else(failed)?;
        // Hand-written routines only (a class's `.int`s are generated), read as the `.int`
        // where there is one (compiled, macro-expanded; what `get_int_or_mac` prefers), else the `.mac`.
        let rtn_names: Vec<String> = routine_names(self, "*.mac,*.int")
            .ok_or_else(failed)?
            .into_iter()
            .collect();

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
        let (tx, rx) = mpsc::channel();
        let mut done = [0; 3];
        std::thread::scope(|scope| {
            let (jobs, next) = (&jobs, &next);
            let (classes, includes, routines) = (&classes, &includes, &routines);
            for _ in 0..WORKERS.min(jobs.len()) {
                let tx = tx.clone();
                scope.spawn(move || {
                    while !self.broken.load(Ordering::Relaxed) {
                        let Some(&(kind, batch)) = jobs.get(next.fetch_add(1, Ordering::Relaxed))
                        else {
                            break;
                        };
                        match kind {
                            DocKind::Cls => {
                                let params: Vec<&str> = batch.iter().map(String::as_str).collect();
                                let filter = format!("IN ({})", vec!["?"; params.len()].join(","));
                                let found = class_infos(self, &filter, &params);
                                classes.lock().unwrap().extend(found);
                            }
                            DocKind::Inc => {
                                includes
                                    .lock()
                                    .unwrap()
                                    .extend(self.sources(batch, &["inc"]));
                            }
                            DocKind::Rtn => {
                                let found = self.sources(batch, &["int", "mac"]);
                                let found = found.into_iter().map(|(name, source)| {
                                    (name, crate::routine_info(&source.text, None))
                                });
                                routines.lock().unwrap().extend(found);
                            }
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
        if self.broken.load(Ordering::Relaxed) || done != totals {
            return Err(failed());
        }
        Ok(Declarations {
            version: DECLARATIONS_VERSION,
            namespace: self.namespace.clone(),
            classes: classes.into_inner().unwrap(),
            includes: includes.into_inner().unwrap(),
            routines: routines.into_inner().unwrap(),
        })
    }

    // Each of `names` that has source, read from the first of `exts` it is non-empty as.
    fn sources(&self, names: &[String], exts: &[&str]) -> Vec<(String, Source)> {
        names
            .iter()
            .filter_map(|name| {
                let lines = exts.iter().find_map(|ext| {
                    self.get_doc(&format!("{name}.{ext}"))
                        .filter(|lines| !lines.is_empty())
                })?;
                let source = Source {
                    text: lines.join("\n"),
                    // The Atelier REST API has no uri scheme of its own to offer here.
                    uri: None,
                };
                Some((name.clone(), source))
            })
            .collect()
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
}

impl AtelierLike for Http {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iris::open;

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
        let conn = Http::new("http://us%40er:p%40ss%25word@localhost:8080", "USER").unwrap();
        let expected = base64::engine::general_purpose::STANDARD.encode("us@er:p@ss%word");
        assert_eq!(conn.auth_header, format!("Basic {expected}"));
    }

    #[test]
    fn connect_percent_encodes_the_namespace() {
        let conn = Http::new("http://user:pass@localhost:8080", "%SYS").unwrap();
        assert_eq!(conn.base_url, "http://localhost:8080/api/atelier/v8/%25SYS");
    }

    #[test]
    fn connect_keeps_a_path_prefix_before_api_atelier() {
        let conn = Http::new("http://user:pass@localhost:8080/iris", "%SYS").unwrap();
        assert_eq!(
            conn.base_url,
            "http://localhost:8080/iris/api/atelier/v8/%25SYS"
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
        let err = Http::new(&url, "NOPE").unwrap().check().unwrap_err();
        assert!(err.contains("namespace 'NOPE' does not exist"), "{err}");
        assert!(err.contains("%SYS, USER"), "{err}");
    }

    #[test]
    fn check_blames_the_path_prefix_when_there_is_no_atelier_api_at_all() {
        let url = stub_server(&[("", "404 Not Found", "")]);
        let err = Http::new(&url, "USER").unwrap().check().unwrap_err();
        assert!(err.contains("no Atelier API at"), "{err}");
    }

    #[test]
    fn check_reports_a_bad_password() {
        let url = stub_server(&[("", "401 Unauthorized", "")]);
        let err = Http::new(&url, "USER").unwrap().check().unwrap_err();
        assert!(
            err.contains("authentication failed for user 'user'"),
            "{err}"
        );
    }

    #[test]
    fn check_passes_on_a_reachable_namespace() {
        let url = stub_server(&[("", "200 OK", r#"{"result":{"content":{"name":"USER"}}}"#)]);
        Http::new(&url, "USER").unwrap().check().unwrap();
    }

    #[test]
    fn connect_strips_a_trailing_slash_from_the_path_prefix() {
        let conn = Http::new("http://user:pass@localhost:8080/iris/", "%SYS").unwrap();
        assert_eq!(
            conn.base_url,
            "http://localhost:8080/iris/api/atelier/v8/%25SYS"
        );
    }
}
