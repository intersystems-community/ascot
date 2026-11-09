//! Checks against a live IRIS instance, given as `ASCOT_TEST_IRIS`
//! (`http://user:password@host:port`); each test passes without doing anything when it's unset.
//! CONTRIBUTING.md shows how to run one in Docker.

#![cfg(feature = "atelier")]

use ascot::IrisContext;
use ascot::bindings::MemberKind;

fn iris() -> Option<Box<dyn IrisContext>> {
    let Ok(url) = std::env::var("ASCOT_TEST_IRIS") else {
        eprintln!("ASCOT_TEST_IRIS is unset; skipped");
        return None;
    };
    ascot::open_iris(&url, None).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn system_class_has_its_members() {
    let Some(iris) = iris() else { return };
    let cls = iris
        .get_cls("%SYSTEM.Version")
        .unwrap()
        .expect("%SYSTEM.Version");
    assert!(
        cls.members.iter().any(|m| m.name == "GetVersion"),
        "{:?}",
        cls.members.iter().map(|m| &m.name).collect::<Vec<_>>()
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
            .map(|m| (&m.name, &m.origin))
            .collect::<Vec<_>>()
    );
    assert!(cls.members.iter().any(|m| {
        matches!(&m.kind, MemberKind::Index(index) if index.on.as_deref().is_some_and(|keys| keys.len() == 1 && keys[0].property == "NameLowerCase"))
    }));
    assert!(
        cls.members
            .iter()
            .any(|m| m.name == "Detail" && matches!(m.kind, MemberKind::Query(_)))
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
