//! What a macro argument spells out is checked where it stands: a `$ZU` inside an argument gets
//! its hint however the argument is spaced, one only a `#define`d body contains gets none, and
//! one a body pastes twice gets it once. The environment is the pre-built declarations of the newest IRIS release (`--iris latest`,
//! downloaded once into `~/.ascot`), or the live instance `ASCOT_TEST_IRIS` names.

#![cfg(feature = "network")]

use std::collections::BTreeSet;

use ascot::IrisContext;
use ascot::bindings::{Diagnostic, GuestWorkspace};

fn environment() -> Option<Box<dyn IrisContext>> {
    let iris = std::env::var("ASCOT_TEST_IRIS").unwrap_or_else(|_| ascot::LATEST_IRIS.to_string());
    let context = ascot::open_iris(&iris, None)
        .unwrap_or_else(|e| panic!("{e}"))
        .unwrap_or_else(|| panic!("`{iris}` is no environment"));
    Some(context)
}

/// The diagnostics with `code` on `uri` once `documents` are open together.
fn diagnostics_with(documents: &[(&str, &str)], uri: &str, code: &str) -> Vec<Diagnostic> {
    let workspace = ascot::Workspace::native(environment(), None);
    for (uri, text) in documents {
        workspace.open(uri.to_string(), text.to_string());
    }
    let mut diagnostics = workspace.diagnostics(uri.to_string());
    diagnostics.retain(|d| d.code == code);
    diagnostics
}

/// The lines of `uri` (counted from 0) that get `code` once `documents` are open together.
fn lines_with(documents: &[(&str, &str)], uri: &str, code: &str) -> BTreeSet<u32> {
    (diagnostics_with(documents, uri, code).iter())
        .map(|d| d.range.start.line)
        .collect()
}

#[test]
fn a_zu_in_a_macro_argument_is_reported_however_the_argument_is_spaced() {
    let mut class = "Class Spaced\n{\n".to_string();
    for (name, spaces) in [("None", ""), ("One", " "), ("Two", "  ")] {
        class += &format!(
            "\nClassMethod Direct{name}(a As %String) As %Status\n{{\n\
             \tSet x = a_$$$OK\n\
             \tQuit $$$ERROR($$$GeneralError,{spaces}\"text \"_$ZU(41))_x\n}}\n\
             \nClassMethod Nested{name}() As %Status\n{{\n\
             \tSet sc = $$$ERROR($$$GeneralError,{spaces}$$$FormatText(\"%1\", $ZU(41)))\n\
             \tQuit sc\n}}\n\
             \nClassMethod Pasted{name}(sc As %Status) As %Status\n{{\n\
             \tSet sc = $$$ADDSC(sc,{spaces}$$$ERROR($$$GeneralError, \"text \"_$ZU(41)))\n\
             \tQuit sc\n}}\n"
        );
    }
    class += "\n}\n";
    let expected: BTreeSet<u32> = (class.lines().enumerate())
        .filter(|(_, line)| line.contains("$ZU(41)"))
        .map(|(line, _)| line as u32)
        .collect();
    assert_eq!(expected.len(), 9);
    assert_eq!(
        lines_with(&[("Spaced.cls", &class)], "Spaced.cls", "A5847"),
        expected
    );
}

#[test]
fn a_zu_only_a_macro_body_contains_is_not_reported() {
    let include = "ROUTINE Macros [Type=INC]\n#define HiddenZU(%a,%b) $ZU(41)_%a_%b\n";
    let class = "Include Macros\n\nClass Hidden\n{\n\n\
                 ClassMethod Hidden(a As %String, b As %String) As %String\n{\n\
                 \tQuit $$$HiddenZU(a, b)\n}\n\n}\n";
    let documents = [("Macros.inc", include), ("Hidden.cls", class)];
    assert_eq!(
        lines_with(&documents, "Hidden.cls", "A5847"),
        BTreeSet::new()
    );
}

#[test]
fn an_argument_a_macro_body_pastes_twice_is_reported_once() {
    let include = "ROUTINE Macros [Type=INC]\n#define Twice(%a) %a_%a\n";
    let class = "Include Macros\n\nClass Twice\n{\n\n\
                 ClassMethod Twice() As %String\n{\n\tQuit $$$Twice($ZU(41))\n}\n\n}\n";
    let routine = "ROUTINE Twice\n#include Macros\nTwice()\n\tQuit $$$Twice($ZU(41))\n";
    let documents = [
        ("Macros.inc", include),
        ("Twice.cls", class),
        ("Twice.mac", routine),
    ];
    for uri in ["Twice.cls", "Twice.mac"] {
        let hints = diagnostics_with(&documents, uri, "A5847");
        assert_eq!(hints.len(), 1, "{uri}: {hints:?}");
    }
}
