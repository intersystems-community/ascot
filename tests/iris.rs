//! Checks against a live IRIS instance, given as `ASCOT_TEST_IRIS`
//! (`http://user:password@host:port`); each test passes without doing anything when it's unset.
//! CONTRIBUTING.md shows how to run one in Docker.

use ascot::bindings::{DiagnosticSeverity, GuestWorkspace, MemberKind};
use ascot::{IrisContext, Workspace};

fn iris() -> Option<Box<dyn IrisContext>> {
    let Ok(url) = std::env::var("ASCOT_TEST_IRIS") else {
        eprintln!("ASCOT_TEST_IRIS is unset; skipped");
        return None;
    };
    Some(ascot::open_iris(&url, None).unwrap_or_else(|e| panic!("{e}")))
}

#[test]
fn system_class_has_its_members() {
    let Some(iris) = iris() else { return };
    let cls = iris
        .get_cls("%SYSTEM.Version")
        .unwrap()
        .expect("%SYSTEM.Version");
    assert!(
        cls.members.iter().any(|m| m.name.content == "GetVersion"),
        "{:?}",
        cls.members
            .iter()
            .map(|m| &m.name.content)
            .collect::<Vec<_>>()
    );
    assert!(iris.get_cls("No.Such.Class").unwrap().is_none());
}

#[test]
fn a_class_has_only_its_own_members_plus_its_storage_class() {
    let Some(iris) = iris() else { return };
    let cls = iris
        .get_cls("Security.Users")
        .unwrap()
        .expect("Security.Users");
    assert_eq!(cls.storage.as_deref(), Some("%SYSTEM.Security.Storage"));
    assert!(
        cls.members
            .iter()
            .all(|m| m.origin.as_deref() == Some("Security.Users")),
        "{:?}",
        cls.members
            .iter()
            .filter(|m| m.origin.as_deref() != Some("Security.Users"))
            .map(|m| (&m.name.content, &m.origin))
            .collect::<Vec<_>>()
    );
    assert!(cls.members.iter().any(|m| {
        matches!(&m.kind, MemberKind::Index(index) if index.properties == ["NameLowerCase"])
    }));
    assert!(
        cls.members
            .iter()
            .any(|m| m.name.content == "Detail" && m.kind == MemberKind::Query)
    );
}

// Inherited, storage-class and generated members resolve through the walk.
#[test]
fn inherited_and_generated_members_resolve_through_the_walk() {
    let Some(iris) = iris() else { return };
    let workspace = Workspace::native(Some(iris), None);
    let uri = "P.cls".to_string();
    workspace.open(
        uri.clone(),
        "Class Probe.P Extends %Persistent\n{\nProperty Name As %String;\nIndex ByName On Name [ Unique ];\nClassMethod T() As %Status\n{\n Set u = ##class(Security.Users).%OpenId(\"x\")\n Write u.Name, u.NameLowerCaseIndexExists(\"x\"), ##class(Security.Users).DetailFunc()\n Set p = ..ByNameOpen(\"n\")\n Write p.NameGet(), p.%Id(), ##class(Security.Users).%Exists(\"x\")\n Do p.Nope()\n Quit p.%Save()\n}\n}\n".to_string(),
    );
    let diagnostics = workspace.diagnostics(uri);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(
        diagnostics[0].message.contains("Cannot find member 'Nope'"),
        "{diagnostics:?}"
    );
}

#[test]
fn system_include_has_its_source() {
    let Some(iris) = iris() else { return };
    let inc = iris.get_inc("%occStatus").unwrap().expect("%occStatus.inc");
    assert!(inc.text.contains("#define"), "{}", inc.text);
    assert!(iris.get_inc("NoSuchInclude").unwrap().is_none());
}

#[test]
fn missing_routine_is_none() {
    let Some(iris) = iris() else { return };
    assert!(iris.get_rtn("NoSuchRoutine").unwrap().is_none());
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

#[test]
fn system_signatures_without_defaults_are_masked() {
    let Some(iris) = iris() else { return };
    let workspace = Workspace::native(Some(iris), None);
    let uri = "Live.Masked.cls".to_string();
    workspace.open(
        uri.clone(),
        "Class Live.Masked Extends %RegisteredObject\n{\nClassMethod Take(ByRef x As %Any)\n{\n Set x = 1\n}\nClassMethod Run()\n{\n #Dim c As %ClassDefinition\n Do ..Take(.c)\n Set o = ..%New(), p = o.%ConstructClone(), r = ##class(%ResultSet).%New(\"X:Y\")\n While r.Next() { Write c, ..%ClassName(), p.%ClassName(1), $SYSTEM.Status.GetErrorText(1) }\n}\n}\n"
            .to_string(),
    );
    let diagnostics = workspace.diagnostics(uri);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}
