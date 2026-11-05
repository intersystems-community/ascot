//! `open`, which answers an `--iris` argument with a live `RawAtelier` (a URL) or a
//! `Declarations` file `ascot declare` wrote (a path), which never connects to anything.

use std::path::Path;

use crate::IrisContext;
use crate::atelier::{AtelierContext, RawAtelier};
pub(crate) use crate::declared::{DECLARATIONS_VERSION, Declarations, DeclaredClass};

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
    if let Some(release) = crate::prebuilt::Release::parse(iris) {
        let file = crate::prebuilt::fetch(&release)?;
        return open_file(&file, Some(release.namespace())).map(Some);
    }
    if let Some(path) = iris.strip_prefix("file://") {
        return open_file(Path::new(path), namespace).map(Some);
    }
    if iris.contains("://") {
        let raw = RawAtelier::connect(iris, namespace.unwrap_or(DEFAULT_NAMESPACE))?;
        return Ok(Some(Box::new(AtelierContext::from(raw))));
    }
    open_file(Path::new(iris), namespace).map(Some)
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
        assert_eq!(check(" Do Proc^Lib(1,2,3)"), vec!["Too many arguments"]);
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
