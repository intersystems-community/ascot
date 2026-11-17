//! `open`, which answers an `--iris` argument with a live `RawAtelier` (a URL), a Community
//! release's pre-built declarations (a tag such as `2026.2` or `latest`, downloaded into `~/.ascot`
//! once), or a `Declarations` file `ascot declare` wrote (a path).

use std::path::Path;

use crate::IrisContext;
#[cfg(feature = "network")]
use crate::atelier::AtelierContext;
pub(crate) use crate::declared::Declarations;
#[cfg(any(feature = "declare", test))]
pub(crate) use crate::declared::{DECLARATIONS_VERSION, DeclaredClass};
#[cfg(feature = "network")]
use crate::iris_connection::RawAtelier;

pub const DEFAULT_NAMESPACE: &str = "%SYS";

/// `--iris` as given: `none` is no environment at all, the strictest check; a Community release
/// (`2026.2`, `irishealth-community:2026.2:HSLIB`) is the pre-built declarations it names, fetched
/// if need be; a URL is a live connection (checked up front); anything else, `file://` included,
/// a declaration file's path. `namespace` is a live instance's, `%SYS` by default; a release
/// names its own after its tag, and a file carries its own.
pub fn open(iris: &str, namespace: Option<&str>) -> Result<Option<Box<dyn IrisContext>>, String> {
    if iris == NONE {
        return Ok(None);
    }
    if let Some(namespace) = namespace
        && !iris.contains("://")
    {
        return Err(format!(
            "--iris-namespace {namespace} is for a live instance; a release names its namespace after its tag, and a declaration file carries its own"
        ));
    }
    if let Some(release) = crate::release::Release::parse(iris) {
        let file = release.cached_file()?;
        if !file.exists() {
            #[cfg(feature = "network")]
            crate::download::fetch(&release)?;
            #[cfg(not(feature = "network"))]
            return Err(format!(
                "pre-built declarations this build of ascot cannot download (it lacks the \
                 `network` feature): put the file at {}, or give a declaration file's path",
                file.display()
            ));
        }
        return open_file(&file, Some(release.namespace())).map(Some);
    }
    if let Some(path) = iris.strip_prefix("file://") {
        return open_file(Path::new(path), namespace).map(Some);
    }
    if iris.contains("://") {
        #[cfg(feature = "network")]
        {
            let raw = RawAtelier::connect(iris, namespace.unwrap_or(DEFAULT_NAMESPACE))?;
            return Ok(Some(Box::new(AtelierContext::from(raw))));
        }
        #[cfg(not(feature = "network"))]
        return Err(
            "a live instance, which this build of ascot cannot reach (it lacks the \
                    `network` feature); give a declaration file's path"
                .to_string(),
        );
    }
    open_file(Path::new(iris), namespace).map(Some)
}

/// The `--iris` values as given, asked in order: the first source that holds a document answers
/// for it, so `latest` before an instance reads the system classes from the snapshot and the
/// instance's own classes from it. `none` among others is a no-op. `namespace` goes to the live
/// instances in the list; with none in it, `open`'s refusal stands.
pub fn open_all(
    irises: &[&str],
    namespace: Option<&str>,
) -> Result<Option<Box<dyn IrisContext>>, String> {
    if irises.is_empty() {
        return open(LATEST, namespace).map_err(|err| format!("{LATEST}: {err}"));
    }
    let live = irises.iter().any(|iris| iris.contains("://"));
    let mut sources = Vec::new();
    for iris in irises {
        let namespace = namespace.filter(|_| !live || iris.contains("://"));
        if let Some(source) = open(iris, namespace).map_err(|err| format!("{iris}: {err}"))? {
            sources.push(source);
        }
    }
    Ok(match sources.len() {
        0 => None,
        1 => sources.pop(),
        _ => Some(Box::new(Chain(sources))),
    })
}

/// Sources asked in order. A document is the first source's that has it; a source that fails is
/// passed over, and its failure is the answer only when no later source has the document.
pub struct Chain(pub Vec<Box<dyn IrisContext>>);

impl Chain {
    fn first<T>(
        &self,
        get: impl Fn(&dyn IrisContext) -> Result<Option<T>, String>,
    ) -> Result<Option<T>, String> {
        let mut failed = None;
        for source in &self.0 {
            match get(source.as_ref()) {
                Ok(Some(found)) => return Ok(Some(found)),
                Ok(None) => {}
                Err(err) => {
                    failed.get_or_insert(err);
                }
            }
        }
        failed.map_or(Ok(None), Err)
    }
}

impl IrisContext for Chain {
    fn get_cls(&self, cls: &str) -> Result<Option<crate::bindings::ClassInfo>, String> {
        self.first(|source| source.get_cls(cls))
    }

    fn get_inc(&self, name: &str) -> Result<Option<crate::bindings::Source>, String> {
        self.first(|source| source.get_inc(name))
    }

    fn get_rtn(&self, name: &str) -> Result<Option<crate::bindings::RoutineInfo>, String> {
        self.first(|source| source.get_rtn(name))
    }
}

/// The `--iris` value for no environment: nothing outside the workspace exists.
pub const NONE: &str = "none";
/// The `--iris` value for the newest release's pre-built declarations, the default.
pub const LATEST: &str = "latest";

fn open_file(iris: &Path, namespace: Option<&str>) -> Result<Box<dyn IrisContext>, String> {
    let declarations = Declarations::load(iris)?;
    if let Some(namespace) = namespace
        && namespace != declarations.namespace
    {
        return Err(format!(
            "{} declares namespace '{}', not '{namespace}'",
            iris.display(),
            declarations.namespace
        ));
    }
    Ok(Box::new(declarations))
}

impl Declarations {
    pub fn load(path: &Path) -> Result<Self, String> {
        // Anything without `://` lands here, so a URL missing its scheme reads as a file name.
        let bytes = std::fs::read(path).map_err(|e| {
            format!(
                "cannot read declaration file {}: {e} (a live instance's URL starts with http:// \
                 or https://)",
                path.display()
            )
        })?;
        Self::from_bytes(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    #[cfg(any(feature = "declare", test))]
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::bindings::{ClassInfo, RoutineInfo, Source};

    fn class_info(name: &str) -> ClassInfo {
        ClassInfo {
            doc: String::new(),
            name: name.to_string(),
            extends: vec!["%RegisteredObject".to_string()],
            ..Default::default()
        }
    }

    fn declaration_file(name: &str, declarations: &Declarations) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("ascot-{name}-{}.json", std::process::id()));
        declarations.save(&path).unwrap();
        path
    }

    fn source(text: &str) -> Source {
        Source {
            text: text.to_string(),
            uri: None,
        }
    }

    // A source holding one class; `down` fails every routine lookup.
    struct Fake(&'static str, bool);

    impl IrisContext for Fake {
        fn get_cls(&self, cls: &str) -> Result<Option<ClassInfo>, String> {
            Ok((cls == self.0).then(|| class_info(cls)))
        }

        fn get_inc(&self, _: &str) -> Result<Option<Source>, String> {
            Ok(None)
        }

        fn get_rtn(&self, _: &str) -> Result<Option<RoutineInfo>, String> {
            if self.1 {
                Err("down".to_string())
            } else {
                Ok(None)
            }
        }
    }

    #[test]
    fn a_chain_answers_from_the_first_source_that_has_the_document() {
        let chain = Chain(vec![Box::new(Fake("A", false)), Box::new(Fake("B", true))]);
        assert_eq!(chain.get_cls("A").unwrap().unwrap().name, "A");
        assert_eq!(chain.get_cls("B").unwrap().unwrap().name, "B");
        assert!(chain.get_cls("C").unwrap().is_none());
        // A failure counts only once every source has been asked.
        assert_eq!(chain.get_rtn("R").unwrap_err(), "down");
        assert!(open_all(&[NONE, NONE], None).unwrap().is_none());
    }

    #[test]
    fn declarations_round_trip_through_their_file_and_answer_every_lookup() {
        let declarations = Declarations {
            version: DECLARATIONS_VERSION.to_string(),
            namespace: "USER".to_string(),
            iris_version: "IRIS for UNIX 2026.1".to_string(),
            classes: [("A.B".to_string(), DeclaredClass::from(&class_info("A.B")))].into(),
            includes: [(
                "MyInc".to_string(),
                source("ROUTINE MyInc [Type=INC]\n#define FIVE 5\n"),
            )]
            .into(),
            routines: [
                (
                    "Lib".to_string(),
                    crate::routine_info(
                        "ROUTINE Lib [Type=INT]\n Quit\nPlain\n Quit\nWithArgs(a,&b,c...)\n \
                         Quit\nProc(a,b) [x,y] {\n Quit\n}\n",
                        None,
                    ),
                ),
                // Doesn't parse, so its labels are unknown and any call into it is taken on trust.
                (
                    "Unparsed".to_string(),
                    crate::routine_info("ROUTINE Unparsed [Type=INT]\n{\n", None),
                ),
                (
                    "Compiled".to_string(),
                    RoutineInfo {
                        labels: None,
                        uri: None,
                    },
                ),
            ]
            .into(),
            ..Default::default()
        };
        let path = declaration_file("round-trip", &declarations);
        let loaded = Declarations::load(&path).unwrap();
        assert_eq!(loaded.namespace, "USER");
        assert_eq!(loaded.iris_version, "IRIS for UNIX 2026.1");
        assert_eq!(loaded.get_cls("A.B"), Ok(Some(class_info("A.B"))));
        assert_eq!(loaded.get_cls("A.C"), Ok(None));
        assert_eq!(
            loaded.get_inc("MyInc"),
            Ok(Some(declarations.includes["MyInc"].clone()))
        );
        assert_eq!(loaded.get_inc("%occErrors"), Ok(None));
        assert_eq!(loaded.get_rtn("Nope"), Ok(None));

        use crate::bindings::{DiagnosticSeverity, GuestWorkspace};
        let check = |body: &str| {
            let loaded = Declarations::load(&path).unwrap();
            let workspace = crate::Workspace::native(Some(Box::new(loaded)), None);
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
        assert_eq!(
            check(" Do Proc^Lib(1,2,3)"),
            vec!["Too many arguments: the callee takes 2 arguments"]
        );
        assert_eq!(
            check(" Do Plain^NoSuchRoutine"),
            vec!["Cannot find routine 'NoSuchRoutine' in the workspace or on the IRIS instance"]
        );
        // Only a hint, which `check` leaves out.
        assert_eq!(check(" Do Plain^Compiled(1)"), Vec::<String>::new());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn open_reads_a_declaration_file_and_checks_its_namespace() {
        let declarations = Declarations {
            version: DECLARATIONS_VERSION.to_string(),
            namespace: "USER".to_string(),
            iris_version: String::new(),
            classes: BTreeMap::new(),
            includes: BTreeMap::new(),
            routines: BTreeMap::new(),
            ..Default::default()
        };
        let path = declaration_file("open", &declarations);
        let arg = path.to_str().unwrap();
        assert!(open(arg, None).unwrap().is_some());
        let err = open(arg, Some("USER")).err().unwrap();
        assert!(err.contains("live instance"), "{err}");
        assert!(open(NONE, None).unwrap().is_none());
        let err = open_file(Path::new(arg), Some("%SYS")).err().unwrap();
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
            r#"{"version":"0.9.0","namespace":"USER","classes":{},"includes":{},"routines":{}}"#,
        )
        .unwrap();
        let err = open(path.to_str().unwrap(), None).err().unwrap();
        assert!(err.contains("written by an incompatible ascot"), "{err}");
        std::fs::write(&path, "Class A.B {}").unwrap();
        let err = open(path.to_str().unwrap(), None).err().unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(err.contains("not an ascot declaration file"), "{err}");
        let err = open("/no/such/declarations.json", None).err().unwrap();
        assert!(err.contains("cannot read declaration file"), "{err}");
    }
}
