//! A Community release's pre-built declarations as `--iris` names them (`2026.2`, `latest`,
//! `irishealth-community:2026.2:HSLIB`), and where `~/.ascot` keeps the file; `download`
//! downloads it there.

use std::path::PathBuf;

/// The release `--iris` means when it isn't given: the newest one ascot was built for.
pub const DEFAULT_RELEASE: &str = "2026.2";

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

impl Release<'_> {
    /// Where `~/.ascot` keeps this release's declaration file, present or not.
    pub fn cached_file(&self) -> Result<PathBuf, String> {
        Ok(cache_dir()?.join(self.asset().trim_end_matches(".gz")))
    }
}

/// `~/.ascot`, where downloaded declaration files live.
pub fn cache_dir() -> Result<PathBuf, String> {
    Ok(std::env::home_dir()
        .ok_or("cannot find the home directory to keep the declarations in")?
        .join(".ascot"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
