//! A Community release's pre-built declarations as `--iris` names them (`2026.2`, `latest`,
//! `irishealth-community:2026.2:HSLIB`), and where `~/.ascot` keeps the file; `download`
//! downloads it there.

use std::path::PathBuf;

/// The literal of [`DEFAULT_RELEASE`], for `concat!` to name the bundled asset by.
macro_rules! default_release {
    () => {
        "2026.2"
    };
}

/// The release `--iris` means when it isn't given: the newest one ascot was built for.
pub const DEFAULT_RELEASE: &str = default_release!();

/// A release's declaration asset carried in the binary.
#[derive(Debug, Clone, Copy)]
pub struct BundledDeclarations {
    /// What `--iris` would name it: `iris-community:2026.2`.
    pub release: &'static str,
    /// The gzipped declaration file, for [`crate::Declarations::from_bytes`].
    pub bytes: &'static [u8],
}

/// [`DEFAULT_RELEASE`]'s `%SYS` declarations, what `latest` names, carried in the binary by the
/// `bundled-declarations` feature so a host resolves IRIS's own classes with nothing downloaded;
/// `None` without the feature.
#[cfg(feature = "bundled-declarations")]
pub const BUNDLED_DECLARATIONS: Option<BundledDeclarations> = Some(BundledDeclarations {
    release: concat!("iris-community:", default_release!()),
    bytes: include_bytes!(concat!(
        "../declarations/iris-community-",
        default_release!(),
        ".ascot.decls"
    )),
});
#[cfg(not(feature = "bundled-declarations"))]
pub const BUNDLED_DECLARATIONS: Option<BundledDeclarations> = None;

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

    /// The asset among a release's: image and tag dash-joined, then the namespace unless it is
    /// `%SYS`.
    pub fn asset(&self) -> String {
        let namespace = match self.namespace() {
            crate::iris::DEFAULT_NAMESPACE => String::new(),
            other => format!(".{other}"),
        };
        format!("{}-{}{namespace}.ascot.decls", self.image, self.tag)
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

/// `~/.ascot/<ascot version>`, where downloaded declaration files live: each release's own.
pub fn cache_dir() -> Result<PathBuf, String> {
    Ok(std::env::home_dir()
        .ok_or("cannot find the home directory to keep the declarations in")?
        .join(".ascot")
        .join(env!("CARGO_PKG_VERSION")))
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
            "irishealth-community-2026.2.HSLIB.ascot.decls"
        );
        for not_one in [
            "2026",
            "v2026.2",
            "user.ascot.decls",
            "http://localhost:52773",
            "2026.2.0",
            "2026.2.ascot.decls",
            "a:b:c:d",
            "irishealth-community:HSLIB",
        ] {
            assert!(Release::parse(not_one).is_none(), "{not_one}");
        }
    }

    #[cfg(feature = "bundled-declarations")]
    #[test]
    fn the_bundled_declarations_are_the_default_release() {
        let bundled = BUNDLED_DECLARATIONS.unwrap();
        assert_eq!(bundled.release, release("latest").to_string());
        let declarations = crate::Declarations::from_bytes(bundled.bytes).unwrap();
        assert_eq!(declarations.namespace, crate::iris::DEFAULT_NAMESPACE);
        assert!(
            declarations.iris_version.contains(DEFAULT_RELEASE),
            "{}",
            declarations.iris_version
        );
        assert!(declarations.classes.contains_key("%Library.String"));
    }
}
