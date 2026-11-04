//! `open`, which answers an `--iris` argument with a live `RawAtelier` (a URL) or a
//! `Declarations` file `ascot declare` wrote (a path), which never connects to anything.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::IrisContext;
use crate::atelier::{AtelierContext, RawAtelier};
use crate::bindings::{ClassInfo, RoutineInfo, Source};
pub use crate::declared::DeclaredClass;

pub const DEFAULT_NAMESPACE: &str = "%SYS";

/// A Community release (`2026.2`) is its pre-built declarations, fetched if need be; a URL is a
/// live connection (checked up front); anything else, `file://` included, a declaration file's
/// path. `namespace` defaults to `%SYS` for a URL; for a file it must match the one declared.
pub fn open(iris: &str, namespace: Option<&str>) -> Result<Box<dyn IrisContext>, String> {
    if crate::prebuilt::is_release(iris) {
        return open_file(&crate::prebuilt::fetch(iris)?, namespace);
    }
    if let Some(path) = iris.strip_prefix("file://") {
        return open_file(Path::new(path), namespace);
    }
    if iris.contains("://") {
        let raw = RawAtelier::connect(iris, namespace.unwrap_or(DEFAULT_NAMESPACE))?;
        return Ok(Box::new(AtelierContext::from(raw)));
    }
    open_file(Path::new(iris), namespace)
}

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

/// The declaration format's version, semver: a file is readable while its major part matches.
pub(crate) const DECLARATIONS_VERSION: &str = "1.0.0";

/// One namespace as `ascot declare` writes it: each `IrisContext` lookup as a map from its
/// argument to its answer. A class holds only its own members; ascot walks its superclasses.
#[derive(Serialize, Deserialize, Default)]
pub struct Declarations {
    pub version: String,
    pub namespace: String,
    /// The instance's `$ZVERSION`; empty in a file from before it was recorded.
    #[serde(default)]
    pub iris_version: String,
    pub classes: BTreeMap<String, DeclaredClass>,
    // Each class parsed once per `Declarations`, on first lookup.
    #[serde(skip)]
    pub(crate) parsed: Mutex<HashMap<String, Option<ClassInfo>>>,
    /// `.inc` files by name without the extension.
    pub includes: BTreeMap<String, Source>,
    /// Routines by name without the extension.
    pub routines: BTreeMap<String, RoutineInfo>,
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
        let major = |v: &str| v.split('.').next().unwrap_or_default().to_string();
        if major(&declarations.version) != major(DECLARATIONS_VERSION) {
            return Err(format!(
                "{} was written by an incompatible ascot (format {}, this one reads {}); \
                 regenerate it with `ascot declare`",
                path.display(),
                declarations.version,
                DECLARATIONS_VERSION
            ));
        }
        Ok(declarations)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }
}

/// A class as a declaration file stores it: its header and each member's, in the class syntax,
/// with the docs alongside. Read back through the parser a workspace class goes through, so the
/// file holds what the source would, and no more.
impl IrisContext for Declarations {
    fn get_cls(&self, cls: &str) -> Result<Option<ClassInfo>, String> {
        let mut parsed = self.parsed.lock().unwrap();
        if let Some(info) = parsed.get(cls) {
            return Ok(info.clone());
        }
        let info = self.classes.get(cls).and_then(|c| c.to_class_info(cls));
        parsed.insert(cls.to_string(), info.clone());
        Ok(info)
    }

    fn get_inc(&self, name: &str) -> Result<Option<Source>, String> {
        Ok(self.includes.get(name).cloned())
    }

    fn get_rtn(&self, name: &str) -> Result<Option<RoutineInfo>, String> {
        Ok(self.routines.get(name).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            r#"{"version":"0.9.0","namespace":"USER","classes":{},"includes":{},"routines":{}}"#,
        )
        .unwrap();
        let err = open(path.to_str().unwrap(), None).err().unwrap();
        assert!(
            err.contains("was written by an incompatible ascot"),
            "{err}"
        );
        std::fs::write(&path, "Class A.B {}").unwrap();
        let err = open(path.to_str().unwrap(), None).err().unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(err.contains("is not an ascot declaration file"), "{err}");
        let err = open("/no/such/declarations.json", None).err().unwrap();
        assert!(err.contains("cannot read declaration file"), "{err}");
    }
}
