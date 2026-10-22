//! Checks against a live IRIS instance, given as `ASCOT_TEST_IRIS`
//! (`http://user:password@host:port`); each test passes without doing anything when it's unset.
//! CONTRIBUTING.md shows how to run one in Docker.

use ascot::bindings::{DiagnosticSeverity, GuestWorkspace};
use ascot::{IrisClient, Workspace};

fn iris() -> Option<Box<dyn IrisClient>> {
    let Ok(url) = std::env::var("ASCOT_TEST_IRIS") else {
        eprintln!("ASCOT_TEST_IRIS is unset; skipped");
        return None;
    };
    Some(ascot::open_iris(&url, None).unwrap_or_else(|e| panic!("{e}")))
}

#[test]
fn system_class_has_its_members() {
    let Some(iris) = iris() else { return };
    let cls = iris.get_cls("%SYSTEM.Version").expect("%SYSTEM.Version");
    assert!(
        cls.members.iter().any(|m| m.name.content == "GetVersion"),
        "{:?}",
        cls.members
            .iter()
            .map(|m| &m.name.content)
            .collect::<Vec<_>>()
    );
    assert!(iris.get_cls("No.Such.Class").is_none());
}

#[test]
fn system_include_has_its_source() {
    let Some(iris) = iris() else { return };
    let inc = iris.get_inc("%occStatus").expect("%occStatus.inc");
    assert!(inc.text.contains("#define"), "{}", inc.text);
    assert!(iris.get_inc("NoSuchInclude").is_none());
}

#[test]
fn missing_routine_is_none() {
    let Some(iris) = iris() else { return };
    assert!(iris.get_int_or_mac("NoSuchRoutine").is_none());
}

#[test]
fn members_of_a_system_class_are_checked() {
    let Some(iris) = iris() else { return };
    let workspace = Workspace::new_with_iris(Some(iris), None);
    let uri = "Live.Probe.cls".to_string();
    workspace.open(
        uri.clone(),
        "Class Live.Probe\n{\nClassMethod Run()\n{\n Write ##class(%SYSTEM.Version).GetVersion()\n Write ##class(%SYSTEM.Version).NoSuchMethod()\n}\n}\n"
            .to_string(),
    );
    let diagnostics = workspace.diagnostics(uri);
    let [diagnostic] = diagnostics.as_slice() else {
        panic!("{diagnostics:?}");
    };
    assert_eq!(diagnostic.range.start.line, 5, "{diagnostic:?}");
    assert_eq!(
        diagnostic.severity,
        DiagnosticSeverity::Error,
        "{diagnostic:?}"
    );
}

#[test]
fn optional_arguments_of_dynamic_objects_are_optional() {
    let Some(iris) = iris() else { return };
    let workspace = Workspace::new_with_iris(Some(iris), None);
    let uri = "Live.Dynamic.cls".to_string();
    workspace.open(
        uri.clone(),
        "Class Live.Dynamic\n{\nClassMethod Run()\n{\n Set s = ##class(%Library.DynamicObject).%New()\n Set a = ##class(%Library.DynamicArray).%New()\n Do s.%Set(\"a\", 1), a.%Push(1)\n Write s.%Get(\"a\"), a.%Get(0), s.%ToJSON()\n}\n}\n"
            .to_string(),
    );
    let diagnostics = workspace.diagnostics(uri);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}
