//! Downloading a release's pre-built declarations from the project's GitHub Releases into
//! `~/.ascot`, checksum verified, where [`crate::release`] then finds them.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::release::{Release, cache_dir};

const RELEASES: &str = "https://github.com/intersystems-community/ascot/releases/download";

/// Downloads `release`'s declaration file into `~/.ascot` and returns its path; one already there
/// is returned as is.
pub(crate) fn download(release: &Release<'_>) -> Result<PathBuf, String> {
    fetch_into(release, &format!("{RELEASES}/declarations"), &cache_dir()?)
}

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
    use crate::iris::Declarations;
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    fn gzipped(text: &str) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(text.as_bytes()).unwrap();
        encoder.finish().unwrap()
    }

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
