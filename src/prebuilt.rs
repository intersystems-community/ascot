//! The pre-built declarations of a Community release's namespace (`--iris 2026.2`, or
//! `--iris irishealth-community:2026.2:HSLIB` for IRIS for Health's library):
//! downloaded once from the project's GitHub Releases into `~/.ascot`, and read from there
//! afterwards.

#[cfg(feature = "prebuilt")]
use std::fs;
#[cfg(feature = "prebuilt")]
use std::io::Read;
#[cfg(feature = "prebuilt")]
use std::path::Path;
use std::path::PathBuf;
#[cfg(feature = "prebuilt")]
use std::time::Duration;

#[cfg(feature = "prebuilt")]
use sha2::{Digest, Sha256};

/// The release `--iris` means when it isn't given: the newest one ascot was built for.
pub const DEFAULT_RELEASE: &str = "2026.2";
#[cfg(feature = "prebuilt")]
const RELEASES: &str = "https://github.com/intersystems-community/ascot/releases/download";

/// A pre-built declaration file's coordinates as `--iris` names them, `[image:]tag[:namespace]`:
/// the Community image, `iris-community` unless named; its tag, a release such as `2026.2`, or
/// `latest` for the newest this ascot knows; the namespace, `%SYS` unless named
/// (`irishealth-community:2026.2:HSLIB` for IRIS for Health's library).
#[derive(Debug, PartialEq)]
pub struct Release<'a> {
    pub image: &'a str,
    pub tag: &'a str,
    pub namespace: Option<&'a str>,
}

impl<'a> Release<'a> {
    /// `None` for what isn't one: a URL, a path, `none`.
    pub fn parse(iris: &'a str) -> Option<Self> {
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        let is_tag = |s: &str| {
            s == crate::iris::LATEST
                || matches!(s.split_once('.'), Some((year, minor)) if year.len() == 4 && digits(year) && digits(minor))
        };
        let is_image = |s: &str| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        };
        let mut parts = iris.split(':');
        let first = parts.next()?;
        let (image, tag) = if is_tag(first) {
            ("iris-community", first)
        } else if is_image(first) {
            (first, parts.next().filter(|tag| is_tag(tag))?)
        } else {
            return None;
        };
        let namespace = parts.next().filter(|ns| !ns.is_empty());
        if parts.next().is_some() {
            return None;
        }
        let tag = if tag == crate::iris::LATEST {
            DEFAULT_RELEASE
        } else {
            tag
        };
        Some(Release {
            image,
            tag,
            namespace,
        })
    }

    pub fn namespace(&self) -> &'a str {
        self.namespace.unwrap_or(crate::iris::DEFAULT_NAMESPACE)
    }

    /// The asset in the `declarations` GitHub Release: image and tag dash-joined, then the
    /// namespace unless it is `%SYS`.
    pub fn asset(&self) -> String {
        let namespace = match self.namespace() {
            crate::iris::DEFAULT_NAMESPACE => String::new(),
            other => format!(".{other}"),
        };
        format!("{}-{}{namespace}.d.json.gz", self.image, self.tag)
    }
}

impl std::fmt::Display for Release<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.image, self.tag)?;
        match self.namespace {
            Some(namespace) => write!(f, ":{namespace}"),
            None => Ok(()),
        }
    }
}

/// The cached declaration file of `release`, downloaded first if `~/.ascot` lacks it. A machine
/// that can't reach GitHub, or a build without the `prebuilt` feature, gets the file put there by
/// hand.
pub fn fetch(release: &Release<'_>) -> Result<PathBuf, String> {
    let dir = std::env::home_dir()
        .ok_or("cannot find the home directory to keep the declarations in")?
        .join(".ascot");
    let path = dir.join(release.asset().trim_end_matches(".gz"));
    if path.exists() {
        return Ok(path);
    }
    #[cfg(feature = "prebuilt")]
    return fetch_into(release, &format!("{RELEASES}/declarations"), &dir);
    #[cfg(not(feature = "prebuilt"))]
    Err(format!(
        "--iris {release} names pre-built declarations, which this build of ascot cannot download \
         (it lacks the `prebuilt` feature): put the file at {}, or give a declaration file's path",
        path.display()
    ))
}

#[cfg(feature = "prebuilt")]
pub(crate) fn fetch_into(release: &Release<'_>, base: &str, dir: &Path) -> Result<PathBuf, String> {
    let asset = release.asset();
    let path = dir.join(asset.trim_end_matches(".gz"));
    if path.exists() {
        return Ok(path);
    }
    let url = format!("{}/{asset}", base.trim_end_matches('/'));
    eprintln!("--iris {release}: downloading {url} into {}", dir.display());
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(300)))
            .tls_config(tls)
            .build(),
    );
    let body = get(&agent, &url)?;
    let expected = String::from_utf8_lossy(&get(&agent, &format!("{url}.sha256"))?)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let digest = format!("{:x}", Sha256::digest(&body));
    if digest != expected {
        return Err(format!(
            "{url} does not match its checksum ({digest} rather than {expected}); nothing saved"
        ));
    }
    let mut text = Vec::new();
    flate2::read::GzDecoder::new(&body[..])
        .read_to_end(&mut text)
        .map_err(|e| format!("{url} is not a gzip file: {e}"))?;
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    // Written whole, then renamed: a run interrupted mid-write leaves no half file to load.
    let part = path.with_extension("part");
    fs::write(&part, &text).map_err(|e| format!("cannot write {}: {e}", part.display()))?;
    fs::rename(&part, &path).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    eprintln!(
        "--iris {release}: saved {} ({} MB)",
        path.display(),
        text.len() / 1_000_000
    );
    Ok(path)
}

#[cfg(feature = "prebuilt")]
fn get(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    let mut response = agent.get(url).call().map_err(|e| format!("{url}: {e}"))?;
    response
        .body_mut()
        .with_config()
        .limit(256 << 20)
        .read_to_vec()
        .map_err(|e| format!("{url}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "prebuilt")]
    use crate::iris::Declarations;
    #[cfg(feature = "prebuilt")]
    use std::io::Write;
    #[cfg(feature = "prebuilt")]
    use std::sync::{Arc, Mutex};

    #[cfg(feature = "prebuilt")]
    fn gzipped(text: &str) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(text.as_bytes()).unwrap();
        encoder.finish().unwrap()
    }

    // Serves `routes` (path, body) over HTTP, counting the requests.
    #[cfg(feature = "prebuilt")]
    fn server(routes: Vec<(String, Vec<u8>)>) -> (String, Arc<Mutex<usize>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(Mutex::new(0));
        let count = hits.clone();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buf = [0; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let path = request.split_whitespace().nth(1).unwrap_or("");
                *count.lock().unwrap() += 1;
                let response = match routes.iter().find(|(p, _)| p == path) {
                    Some((_, body)) => {
                        let mut r = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .into_bytes();
                        r.extend_from_slice(body);
                        r
                    }
                    None => {
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_vec()
                    }
                };
                let _ = stream.write_all(&response);
            }
        });
        (url, hits)
    }

    #[cfg(feature = "prebuilt")]
    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ascot-prebuilt-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn release(iris: &str) -> Release<'_> {
        Release::parse(iris).unwrap()
    }

    #[test]
    fn a_release_is_an_image_a_tag_and_a_namespace_the_first_and_last_optional() {
        assert_eq!(
            release("2026.2"),
            Release {
                image: "iris-community",
                tag: "2026.2",
                namespace: None
            }
        );
        assert_eq!(release("2025.10").tag, "2025.10");
        assert_eq!(
            release("irishealth-community:2026.2:HSLIB"),
            Release {
                image: "irishealth-community",
                tag: "2026.2",
                namespace: Some("HSLIB")
            }
        );
        assert_eq!(release("2026.2:USER").namespace, Some("USER"));
        assert_eq!(release("latest").tag, DEFAULT_RELEASE);
        assert_eq!(release("irishealth-community:latest").tag, DEFAULT_RELEASE);
        assert_eq!(
            release("iris-community:2026.2").asset(),
            release("2026.2").asset()
        );
        assert_eq!(
            release("irishealth-community:2026.2:HSLIB").asset(),
            "irishealth-community-2026.2.HSLIB.d.json.gz"
        );
        for not_one in [
            "2026",
            "v2026.2",
            "user.d.json",
            "http://localhost:52773",
            "2026.2.0",
            "2026.2.d.json",
            "a:b:c:d",
            "irishealth-community:HSLIB",
        ] {
            assert!(Release::parse(not_one).is_none(), "{not_one}");
        }
    }

    #[cfg(feature = "prebuilt")]
    #[test]
    fn a_release_is_downloaded_checked_unpacked_and_then_read_from_the_cache() {
        let declarations = Declarations {
            version: crate::declared::DECLARATIONS_VERSION.to_string(),
            namespace: "%SYS".to_string(),
            ..Default::default()
        };
        let gz = gzipped(&serde_json::to_string(&declarations).unwrap());
        let sum = format!("{:x}  {}\n", Sha256::digest(&gz), release("2026.2").asset());
        let (url, hits) = server(vec![
            (format!("/{}", release("2026.2").asset()), gz),
            (
                format!("/{}.sha256", release("2026.2").asset()),
                sum.into_bytes(),
            ),
        ]);
        let dir = temp_dir("ok");
        let path = fetch_into(&release("2026.2"), &url, &dir).unwrap();
        assert_eq!(path, dir.join("iris-community-2026.2.d.json"));
        assert_eq!(Declarations::load(&path).unwrap().namespace, "%SYS");
        assert_eq!(*hits.lock().unwrap(), 2);
        assert_eq!(fetch_into(&release("2026.2"), &url, &dir).unwrap(), path);
        assert_eq!(
            *hits.lock().unwrap(),
            2,
            "the cached file is read without a request"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(feature = "prebuilt")]
    #[test]
    fn a_corrupt_or_missing_download_saves_nothing() {
        let gz = gzipped("{}");
        let (url, _) = server(vec![
            (format!("/{}", release("2026.1").asset()), gz),
            (
                format!("/{}.sha256", release("2026.1").asset()),
                b"0000 wrong\n".to_vec(),
            ),
        ]);
        let dir = temp_dir("bad");
        let err = fetch_into(&release("2026.1"), &url, &dir).unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        let err = fetch_into(&release("2024.1"), &url, &dir).unwrap_err();
        assert!(err.contains("404"), "{err}");
        assert!(!dir.exists(), "nothing is written for a failed download");
    }
}
