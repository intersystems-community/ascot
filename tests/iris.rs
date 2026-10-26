//! Checks against a live IRIS instance, given as `ASCOT_TEST_IRIS`
//! (`http://user:password@host:port`); each test passes without doing anything when it's unset.
//! CONTRIBUTING.md shows how to run one in Docker.

use ascot::bindings::{DiagnosticSeverity, GuestWorkspace, MemberKind};
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
fn generated_accessors_keep_their_names_and_origins() {
    let Some(iris) = iris() else { return };
    let cls = iris.get_cls("Security.Users").expect("Security.Users");
    let names: Vec<&String> = cls.members.iter().map(|m| &m.name.content).collect();
    let get = cls
        .members
        .iter()
        .find(|m| m.name.content == "%ConcurrencyGet")
        .unwrap_or_else(|| panic!("{names:?}"));
    assert_eq!(get.origin.as_deref(), Some("%Library.Persistent"));
    assert!(
        matches!(&get.kind, MemberKind::Method(m) if m.t.as_deref() == Some("%Library.RawString")),
        "{:?}",
        get.kind
    );
    assert!(!names.contains(&&"IsValid".to_string()), "{names:?}");
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
    let workspace = Workspace::native(Some(iris), None);
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
    let workspace = Workspace::native(Some(iris), None);
    let uri = "Live.Dynamic.cls".to_string();
    workspace.open(
        uri.clone(),
        "Class Live.Dynamic\n{\nClassMethod Run()\n{\n Set s = ##class(%Library.DynamicObject).%New()\n Set a = ##class(%Library.DynamicArray).%New()\n Do s.%Set(\"a\", 1), a.%Push(1)\n Write s.%Get(\"a\"), a.%Get(0), s.%ToJSON()\n}\n}\n"
            .to_string(),
    );
    let diagnostics = workspace.diagnostics(uri);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn system_macros_are_checked() {
    let Some(iris) = iris() else { return };
    let workspace = Workspace::native(Some(iris), None);
    let uri = "Live.Macros.cls".to_string();
    let mut text = "Include %occInclude\n\nClass Live.Macros\n{\nClassMethod Run() As %Status\n{\n Set sc = $$$ERROR($$$GeneralError, \"a\"), sc = $$$ADDSC(sc, $$$OK), sc = $$$EMBEDSC(sc, $$$OK)\n Write $$$ISERR(sc), $$$ISOK(sc), $$$GETERRORCODE(sc), $$$GETERRORMESSAGE(sc, 1)\n Write $$$FormatMessage(\"en\", \"d\", \"id\", \"x %1\", 1), $$$FormatText(\"x %1\", 1), $$$FormatTextHTML(\"x\"), $$$FormatTextJS(\"x\")\n Write $$$Text(\"x\"), $$$TextHTML(\"x\"), $$$TextJS(\"x\"), $$$UPPER(\"x\"), $$$LOWER(\"x\")\n Quit sc\n}\n".to_string();
    let wrong = [
        "$$$EMBEDSC(1, 1)",
        "$$$FormatText(\"x\")",
        "$$$FormatMessage(\"en\", \"d\", \"id\", \"x\")",
        "$$$TextHTML(\"x\")",
    ];
    for (i, value) in wrong.iter().enumerate() {
        text += &format!(
            "ClassMethod Wrong{i}()\n{{\n #Dim o As %DynamicObject\n Set o = {value}\n}}\n"
        );
    }
    workspace.open(uri.clone(), text + "}\n");
    let diagnostics = workspace.diagnostics(uri);
    let lines: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .map(|d| d.range.start.line)
        .collect();
    assert_eq!(lines, [15, 20, 25, 30], "{diagnostics:?}");
}
