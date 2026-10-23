//! `open`, which answers an `--iris` argument with a live `RawAtelier` (a URL) or a
//! `Declarations` file `ascot declare` wrote (a path), which never connects to anything.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::IrisContext;
use crate::atelier::{AtelierContext, RawAtelier};
use crate::bindings::{ClassInfo, RoutineInfo, Source};

pub const DEFAULT_NAMESPACE: &str = "%SYS";

/// A URL is a live connection (checked up front), anything else a declaration file's path.
/// `namespace` defaults to `%SYS` for a URL; for a file it must match the one declared.
pub fn open(iris: &str, namespace: Option<&str>) -> Result<Box<dyn IrisContext>, String> {
    if iris.contains("://") {
        let raw = RawAtelier::connect(iris, namespace.unwrap_or(DEFAULT_NAMESPACE))?;
        return Ok(Box::new(AtelierContext::from(raw)));
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

pub(crate) const DECLARATIONS_VERSION: u32 = 3;

/// One namespace as `ascot declare` writes it: each `IrisContext` lookup as a map from its
/// argument to its answer.
#[derive(Serialize, Deserialize, Clone)]
pub struct Declarations {
    pub version: u32,
    pub namespace: String,
    pub classes: BTreeMap<String, ClassInfo>,
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
        if declarations.version != DECLARATIONS_VERSION {
            return Err(format!(
                "{} was written by an incompatible ascot; regenerate it with `ascot declare`",
                path.display()
            ));
        }
        Ok(declarations)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }
}

impl IrisContext for Declarations {
    fn get_cls(&self, cls: &str) -> Option<ClassInfo> {
        self.classes.get(cls).cloned()
    }

    fn get_inc(&self, name: &str) -> Option<Source> {
        self.includes.get(name).cloned()
    }

    fn get_rtn(&self, name: &str) -> Option<RoutineInfo> {
        self.routines.get(name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::{NameInfo, Position};

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

    fn source(text: &str) -> Source {
        Source {
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
        assert_eq!(loaded.get_rtn("Nope"), None);

        use crate::bindings::{DiagnosticSeverity, GuestWorkspace};
        let check = |body: &str| {
            let workspace = crate::Workspace::native(Some(Box::new(loaded.clone())), None);
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
            r#"{"version":1,"namespace":"USER","classes":{},"includes":{},"routines":{}}"#,
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
