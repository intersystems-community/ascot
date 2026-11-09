//! ascot's own connection to a live IRIS instance over its Atelier REST API, for
//! `--iris http://...`. Part of the `network` feature; a host with its own transport implements
//! [`Atelier`] instead and needs none of this.

use std::sync::Mutex;
use std::time::Duration;

use base64::Engine;
use percent_encoding::percent_decode_str;
use serde_json::Value;

use crate::atelier::Atelier;

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

    fn agent(&self) -> Result<ureq::Agent, String> {
        self.agent.lock().unwrap().clone()
    }

    // The instance's `$ZVERSION`, from the API listing at `api_root/`.
    #[cfg(feature = "declare")]
    pub(crate) fn server_version(&self) -> Result<String, String> {
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

    /// The namespace this connection was opened on.
    #[cfg(feature = "declare")]
    pub(crate) fn namespace(&self) -> &str {
        &self.namespace
    }
}

impl Drop for RawAtelier {
    fn drop(&mut self) {
        if let Ok(agent) = self.agent() {
            let _ = agent
                .get(&format!("{}/?IRISLogout=end", self.api_root))
                .call();
        }
    }
}

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

    fn stub_server(routes: &'static [(&str, &str, &str)]) -> String {
        recording_stub_server(routes).0
    }

    fn recording_stub_server(
        routes: &'static [(&str, &str, &str)],
    ) -> (String, std::sync::Arc<Mutex<Vec<String>>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://user:pass@{}", listener.local_addr().unwrap());
        let requests = std::sync::Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                // The whole request, so a POST's body isn't still in flight when the response
                // closes the connection.
                let mut bytes = Vec::new();
                let mut buf = [0; 4096];
                while let Ok(n) = stream.read(&mut buf) {
                    bytes.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&bytes);
                    let Some((head, body)) = text.split_once("\r\n\r\n") else {
                        continue;
                    };
                    let length = head
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if n == 0 || body.len() >= length {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let path = request.split_whitespace().nth(1).unwrap_or("");
                let cookie = request
                    .lines()
                    .find_map(|line| line.strip_prefix("cookie: "))
                    .unwrap_or("");
                log.lock().unwrap().push(format!(
                    "{} {path} [{cookie}]",
                    request.split_whitespace().next().unwrap_or("")
                ));
                let (_, status, body) = routes
                    .iter()
                    .find(|(suffix, ..)| path.ends_with(suffix))
                    .unwrap();
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nSet-Cookie: CSPSESSIONID-SP-1-UP-api-atelier-=abc123; path=/api/atelier/\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (url, requests)
    }

    #[test]
    fn one_session_is_kept_and_ended() {
        let (url, requests) =
            recording_stub_server(&[("", "200 OK", r#"{"result":{"content":[]}}"#)]);
        {
            let raw = RawAtelier::connect(&url, "USER").unwrap();
            let _ = raw.query("SELECT 1", &[]);
            let _ = raw.get_doc("X.inc");
        }
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 4, "{requests:?}");
        assert!(requests[0].ends_with(" []"), "{requests:?}");
        for request in &requests[1..] {
            assert!(
                request.contains(" [CSPSESSIONID-SP-1-UP-api-atelier-=abc123"),
                "{requests:?}"
            );
        }
        assert!(
            requests[3].starts_with("GET /api/atelier/?IRISLogout=end"),
            "{requests:?}"
        );
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
