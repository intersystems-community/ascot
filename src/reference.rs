//! The reference of every diagnostic, Markdown pages rendered from the kinds' definitions by
//! `ascot diagnostics DIR`: `docs/unreleased` holds the pages for `main`, `docs/<version>` those
//! of a release, published on GitHub Pages as the page a diagnostic's code links to. The pages
//! carry no version, so a release's directory is a copy of `unreleased`.

use std::fs;
use std::io;
use std::path::Path;

use crate::bindings::DiagnosticSeverity;
use crate::check::TypeErrorKind;

/// Every page by file name: `index.md`, then `<CODE>.md` per kind.
// `<SELECT>` and `<ILLEGAL VALUE>` in code spans, any other `<` or `>` left as written.
fn span_error_names(message: &str) -> String {
    let mut out = String::with_capacity(message.len() + 4);
    let mut rest = message;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        let name = after.find('>').map(|len| &after[..len]);
        match name {
            Some(name)
                if !name.is_empty()
                    && name.bytes().all(|b| {
                        b.is_ascii_uppercase() || b.is_ascii_digit() || b == b' ' || b == b'_'
                    }) =>
            {
                out.push_str(&rest[..start]);
                out.push('`');
                out.push_str(&rest[start..start + name.len() + 2]);
                out.push('`');
                rest = &after[name.len() + 1..];
            }
            _ => {
                out.push_str(&rest[..=start]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn pages() -> Vec<(String, String)> {
    // The message is a format string, which doubles a literal brace. Markdown takes an IRIS
    // error name such as `<SELECT>` for inline HTML and drops it, so it goes in a code span.
    let mut kinds: Vec<_> = (TypeErrorKind::REFERENCE.iter())
        .map(|(code, severity, message)| {
            (
                *code,
                severity,
                span_error_names(&message.replace("{{", "{").replace("}}", "}")),
            )
        })
        .collect();
    kinds.sort_by_key(|(code, ..)| *code);
    let mut index = String::from("# ascot diagnostics\n\n");
    index += "Every diagnostic ascot reports, from the checker's own definitions (`ascot diagnostics`). \
              A code's page gives its severity and message, when it fires, what IRIS does with such \
              code, what to change, and an example, in which a line ending in `//! A1234` is where \
              that diagnostic lands. A code can be dropped or re-ranked with `-d`, as in \
              `-d off:A1234`; see the README's Diagnostics Filter. The style rules are in STYLE.md, and \
              [style.json](style.json) is the recommended style file: commands long and lowercase, \
              functions and system variables long and uppercase.\n\n";
    for (label, plural) in [
        ("Error", "Errors"),
        ("Warning", "Warnings"),
        ("Information", "Information"),
        ("Hint", "Hints"),
    ] {
        let rows: Vec<_> = (kinds.iter())
            .filter(|(_, severity, _)| name(severity) == label)
            .collect();
        if rows.is_empty() {
            continue;
        }
        index += &format!("## {plural}\n\n| Code | Message |\n| --- | --- |\n");
        for (code, _, message) in rows {
            index += &format!(
                "| [{code}]({code}.md) | {} |\n",
                message.replace('|', "\\|")
            );
        }
        index.push('\n');
    }
    let mut pages = vec![("index.md".to_string(), index)];
    for (code, severity, message) in kinds {
        let doc = (TypeErrorKind::DOCS.iter())
            .find(|(c, _)| *c == code)
            .map_or("", |(_, doc)| doc);
        let mut page = format!("# {code}\n\n{}: {message}\n\n", name(severity));
        page += &rendered(doc, name(severity) == "Error");
        page += "[All diagnostics](index.md)\n";
        pages.push((format!("{code}.md"), page));
    }
    pages
}

/// The recommended style file, `style.json` at the repository root, shipped with the npm package,
/// attached to each release and published with each release's reference.
pub const STYLE_PRESET: &str = include_str!("../style.json");

/// Writes the pages and the style preset into `dir`; no other page is left there: a kind that is
/// gone loses its page.
pub fn write(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            fs::remove_file(path)?;
        }
    }
    for (name, text) in pages() {
        fs::write(dir.join(name), text)?;
    }
    fs::write(dir.join("style.json"), STYLE_PRESET)
}

fn name(severity: &DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Error => "Error",
        DiagnosticSeverity::Warning => "Warning",
        DiagnosticSeverity::Information => "Information",
        DiagnosticSeverity::Hint => "Hint",
    }
}

// The doc comment as written, except that each example's fence loses the words the tests read,
// which become a sentence on what IRIS does with the example.
fn rendered(doc: &str, error: bool) -> String {
    let mut out = String::new();
    let mut lines = doc
        .lines()
        .map(|line| line.strip_prefix(' ').unwrap_or(line));
    while let Some(line) = lines.next() {
        match line.strip_prefix("```objectscript") {
            Some(info) => {
                out += &format!("{}\n\n```objectscript\n", iris(info.trim(), error));
                for line in lines.by_ref() {
                    out += line;
                    out.push('\n');
                    if line == "```" {
                        break;
                    }
                }
            }
            None => {
                out += line;
                out.push('\n');
            }
        }
    }
    out.push('\n');
    out
}

// What IRIS does with the example, from the fence's words `scripts/verify-examples.py` checks: an
// error's example is rejected unless it says `compiles` or `raises=<ERROR> statement`.
fn iris(info: &str, error: bool) -> String {
    // `style=` ends the info string: its JSON may hold spaces.
    let (info, style) = (info.split_once("style=")).map_or((info, None), |(i, s)| (i, Some(s)));
    let words: Vec<&str> = info.split_whitespace().collect();
    let mut sentence = match info
        .strip_prefix("raises=")
        .and_then(|rest| rest.split_once('>'))
    {
        Some((err, statement)) => {
            format!(
                "IRIS compiles the example, and `{}` raises `{err}>` when it runs.",
                statement.trim()
            )
        }
        None if words.contains(&"rejected") || (error && !words.contains(&"compiles")) => {
            "IRIS rejects the example.".to_string()
        }
        None => "IRIS compiles the example.".to_string(),
    };
    if words.contains(&"env=none") {
        sentence += " Checked without an environment, as `--iris none`.";
    }
    if let Some(style) = style {
        sentence += &format!(" Checked with the style `{}`.", style.trim());
    }
    sentence
}
