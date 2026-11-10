//! The examples in the diagnostics' doc comments (`TypeErrorKind::DOCS`) hold: each
//! ```` ```objectscript ```` block is a class or routine whose offending lines end in `//! A1234`,
//! and those lines get exactly those diagnostics while no other line gets any. The environment is
//! the pre-built declarations of the newest IRIS release (`--iris latest`, downloaded once into
//! `~/.ascot`), or the live instance `ASCOT_TEST_IRIS` names. `scripts/verify-examples.py` checks
//! the other half of each message, what IRIS itself does with the example.

#![cfg(feature = "network")]

use std::collections::BTreeSet;
use std::sync::Arc;

use ascot::IrisContext;
use ascot::bindings::GuestWorkspace;
use ascot::check::TypeErrorKind;

struct Document {
    uri: String,
    text: String,
    /// `(line, code)` for every `//! CODE` marker, lines counted from 0 after the markers are gone.
    expected: BTreeSet<(u32, String)>,
}

struct Example {
    code: &'static str,
    style: Option<String>,
    /// ```` ```objectscript env=none ````: checked without an environment, as `--iris none`.
    no_environment: bool,
    documents: Vec<Document>,
}

/// The fenced ```` ```objectscript ```` blocks of one kind's doc comment, markers stripped.
fn examples(code: &'static str, doc: &str) -> Option<Example> {
    let mut documents = Vec::new();
    let mut style = None;
    let mut no_environment = false;
    let mut lines = doc.lines();
    while let Some(line) = lines.next() {
        let Some(info) = line.trim().strip_prefix("```objectscript") else {
            continue;
        };
        for word in info.split_whitespace() {
            if let Some(json) = word.strip_prefix("style=") {
                style = Some(json.to_string());
            }
            no_environment |= word == "env=none";
        }
        let mut text = Vec::new();
        let mut expected = BTreeSet::new();
        for line in lines.by_ref() {
            if line.trim() == "```" {
                break;
            }
            // `/// text` is the literal ` text`: the space after the slashes is the comment's.
            let (line, codes) = split_marker(line.strip_prefix(' ').unwrap_or(line));
            for marker in codes {
                expected.insert((text.len() as u32, marker));
            }
            text.push(line);
        }
        let uri = document_uri(&text).unwrap_or_else(|| {
            panic!("{code}: an example must start with `Class Name` or `ROUTINE Name`:\n{text:#?}")
        });
        documents.push(Document {
            uri,
            text: text.join("\n") + "\n",
            expected,
        });
    }
    (!documents.is_empty()).then_some(Example {
        code,
        style,
        no_environment,
        documents,
    })
}

/// `Set x = 1 //! A7095 A5718` is the line `Set x = 1` and the codes `A7095`, `A5718`.
fn split_marker(line: &str) -> (String, Vec<String>) {
    let Some((code, marker)) = line.rsplit_once("//!") else {
        return (line.to_string(), vec![]);
    };
    let codes: Vec<String> = marker.split_whitespace().map(str::to_string).collect();
    let is_code = |c: &String| {
        (c.starts_with('A') || c.starts_with('S')) && c[1..].bytes().all(|b| b.is_ascii_digit())
    };
    if codes.is_empty() || !codes.iter().all(is_code) {
        return (line.to_string(), vec![]);
    }
    (code.trim_end().to_string(), codes)
}

fn document_uri(text: &[String]) -> Option<String> {
    let header = text.iter().find(|l| !l.trim().is_empty())?;
    let mut words = header.split_whitespace();
    let (kind, name) = (words.next()?, words.next()?);
    let extension = match kind {
        "Class" => "cls",
        "ROUTINE" if header.contains("[Type=INC]") => "inc",
        "ROUTINE" if header.contains("[Type=INT]") => "int",
        "ROUTINE" => "mac",
        _ => return None,
    };
    Some(format!("{name}.{extension}"))
}

/// One environment, loaded once, for every example.
struct Shared(Arc<Box<dyn IrisContext>>);

impl IrisContext for Shared {
    fn get_cls(&self, name: &str) -> Result<Option<ascot::bindings::ClassInfo>, String> {
        self.0.get_cls(name)
    }
    fn get_inc(&self, name: &str) -> Result<Option<ascot::bindings::Source>, String> {
        self.0.get_inc(name)
    }
    fn get_rtn(&self, name: &str) -> Result<Option<ascot::bindings::RoutineInfo>, String> {
        self.0.get_rtn(name)
    }
}

fn environment() -> Arc<Box<dyn IrisContext>> {
    let iris = std::env::var("ASCOT_TEST_IRIS").unwrap_or_else(|_| ascot::LATEST_IRIS.to_string());
    let context = ascot::open_iris(&iris, None)
        .unwrap_or_else(|e| panic!("{e}"))
        .unwrap_or_else(|| panic!("`{iris}` is no environment"));
    Arc::new(context)
}

fn all_examples() -> Vec<Example> {
    TypeErrorKind::DOCS
        .iter()
        .filter_map(|(code, doc)| examples(code, doc))
        .collect()
}

#[test]
fn the_examples_in_the_docs_hold() {
    let examples = all_examples();
    let environment = environment();
    let mut failures = Vec::new();
    for example in &examples {
        let style = example.style.as_deref().map(|json| {
            ascot::style::parse(json).unwrap_or_else(|e| panic!("{}: {e}", example.code))
        });
        let iris: Option<Box<dyn IrisContext>> =
            (!example.no_environment).then(|| Box::new(Shared(environment.clone())) as _);
        let workspace = ascot::Workspace::native(iris, style);
        for document in &example.documents {
            workspace.open(document.uri.clone(), document.text.clone());
        }
        for document in &example.documents {
            let diagnostics = workspace.diagnostics(document.uri.clone());
            let actual: BTreeSet<(u32, String)> = diagnostics
                .iter()
                .map(|d| (d.range.start.line, d.code.clone()))
                .collect();
            if actual != document.expected {
                let mut report = format!(
                    "{} ({}): expected {:?}, got:",
                    example.code, document.uri, document.expected
                );
                for d in &diagnostics {
                    report += &format!(
                        "\n    {}:{} {:?} {}",
                        d.range.start.line, d.code, d.severity, d.message
                    );
                }
                failures.push(report);
            }
        }
        assert!(
            example
                .documents
                .iter()
                .any(|d| d.expected.iter().any(|(_, c)| c == example.code)),
            "{}: no line of its example is marked with its own code",
            example.code
        );
    }
    assert!(failures.is_empty(), "\n{}\n", failures.join("\n"));
}

/// The kinds no example can show, each with why.
const NO_EXAMPLE: &[(&str, &str)] = &[
    (
        "A4471",
        "fires only when an IRIS lookup itself fails: a refused connection, a bad password",
    ),
    (
        "A3717",
        "needs two documents declaring one class name, and an example's documents are named after their class",
    ),
];

#[test]
fn every_kind_has_an_example_or_a_reason_not_to() {
    let with: BTreeSet<&str> = all_examples().iter().map(|e| e.code).collect();
    let without: BTreeSet<&str> = TypeErrorKind::DOCS
        .iter()
        .map(|(code, _)| *code)
        .filter(|code| !with.contains(code))
        .collect();
    let excused: BTreeSet<&str> = NO_EXAMPLE.iter().map(|(code, _)| *code).collect();
    assert_eq!(
        without, excused,
        "kinds without an example must be exactly those `NO_EXAMPLE` excuses"
    );
}

#[test]
fn the_reference_has_a_page_per_kind() {
    let pages: BTreeSet<String> = ascot::reference::pages()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let wanted: BTreeSet<String> = (TypeErrorKind::CODES.iter().map(|code| format!("{code}.md")))
        .chain(["index.md".to_string()])
        .collect();
    assert_eq!(pages, wanted);
    // GitHub Pages runs the pages through Liquid, which reads these as template syntax.
    for (name, text) in ascot::reference::pages() {
        assert!(
            !text.contains("{{") && !text.contains("{%"),
            "{name} has Liquid syntax"
        );
    }
}
