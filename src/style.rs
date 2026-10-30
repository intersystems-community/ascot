//! Workspace-wide style rules (`bindings::Style`, declared in the WIT), fixed for an analysis
//! session: `--style <file>` on the CLI, the `style` of an MCP `create_workspace`, or the
//! second argument of a JS `createWorkspace`.

use crate::bindings::{KeywordCase, KeywordForm, KeywordStyle, Style};
use serde_json::Value;
use std::collections::HashMap;
use tygr_objectscript::cos::{KeywordKind, KeywordUse};

const UNSET: KeywordStyle = KeywordStyle {
    form: None,
    case: None,
};

// A `Style`'s rules, resolved once per workspace: each `<kind>-<name>` field merged with its
// `<kind>` default, keyed by the field's name as serde spells it (`command_do`).
pub struct KeywordStyles {
    defaults: HashMap<String, KeywordStyle>,
    by_name: HashMap<String, KeywordStyle>,
}

impl KeywordStyles {
    pub fn new(style: &Style) -> Self {
        let Ok(Value::Object(fields)) = serde_json::to_value(style) else {
            unreachable!("a record serializes to an object")
        };
        let (defaults, named): (Vec<_>, Vec<_>) = fields
            .into_iter()
            .filter_map(|(key, value)| Some((key, serde_json::from_value(value).ok()?)))
            .partition(|(key, _): &(String, KeywordStyle)| !key.contains('_'));
        let defaults: HashMap<_, _> = defaults.into_iter().collect();
        let by_name = named
            .into_iter()
            .map(|(key, style)| {
                let default = key
                    .split_once('_')
                    .and_then(|(kind, _)| defaults.get(kind))
                    .unwrap_or(&UNSET);
                let merged = KeywordStyle {
                    form: style.form.or(default.form),
                    case: style.case.or(default.case),
                };
                (key, merged)
            })
            .collect();
        KeywordStyles { defaults, by_name }
    }

    pub fn of(&self, kw: &KeywordUse) -> &KeywordStyle {
        let kind = kind_field(kw.kind);
        self.by_name
            .get(&format!("{kind}_{}", kw.full.to_ascii_lowercase()))
            .or_else(|| self.defaults.get(kind))
            .unwrap_or(&UNSET)
    }
}

fn kind_field(kind: KeywordKind) -> &'static str {
    match kind {
        KeywordKind::Command => "command",
        KeywordKind::Function => "function",
    }
}

// An abbreviation as `pascal` spells it: the full name's own casing where it's a prefix of it
// (`Li` for `List`), capitals otherwise (`LB` for `ListBuild`).
fn spell_abbr(full: &str, abbr: &str) -> String {
    match full.get(..abbr.len()) {
        Some(prefix) if prefix.eq_ignore_ascii_case(abbr) => prefix.to_string(),
        _ => abbr.to_ascii_uppercase(),
    }
}

impl KeywordStyle {
    // How `kw` should have been written under this style, or `None` if it already conforms.
    pub fn expected(&self, kw: &KeywordUse) -> Option<String> {
        let full = kw.full.to_string();
        let abbr = kw.abbr.map(|abbr| spell_abbr(kw.full, abbr));
        let base = match self.form {
            Some(KeywordForm::Long) => full,
            None if kw.text.eq_ignore_ascii_case(kw.full) => full,
            Some(KeywordForm::Short) | None => abbr.unwrap_or(full),
        };
        let expected = match self.case {
            Some(KeywordCase::Lower) => base.to_ascii_lowercase(),
            Some(KeywordCase::Upper) => base.to_ascii_uppercase(),
            None if kw.text.eq_ignore_ascii_case(&base) => return None,
            Some(KeywordCase::Pascal) | None => base,
        };
        (kw.text != expected).then_some(expected)
    }
}

// `{"command": {"form": "Long"}, "function_piece": {"form": "Short"}}` -- variant names as the
// WIT-generated types spell them.
pub fn parse(json: &str) -> Result<Style, String> {
    from_value(serde_json::from_str(json).map_err(|err| err.to_string())?)
}

pub fn from_value(input: Value) -> Result<Style, String> {
    let style: Style = serde_json::from_value(input.clone()).map_err(|err| err.to_string())?;
    // The generated types ignore keys they don't know; a misspelled one must not silently do
    // nothing, so compare against what was actually understood.
    let known = serde_json::to_value(style).map_err(|err| err.to_string())?;
    match unknown_key(&input, &known, "") {
        Some(key) => Err(format!("unknown field `{key}`")),
        None => Ok(style),
    }
}

fn unknown_key(input: &Value, known: &Value, path: &str) -> Option<String> {
    let (Value::Object(input), Value::Object(known)) = (input, known) else {
        return None;
    };
    input.iter().find_map(|(key, value)| {
        let path = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };
        match known.get(key) {
            None => Some(path),
            Some(known) => unknown_key(value, known, &path),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kw(text: &str, full: &'static str, abbr: Option<&'static str>) -> KeywordUse {
        KeywordUse {
            kind: KeywordKind::Command,
            start: 0,
            text: text.to_string(),
            full,
            abbr,
        }
    }

    fn style(form: Option<KeywordForm>, case: Option<KeywordCase>) -> KeywordStyle {
        KeywordStyle { form, case }
    }

    fn function(text: &str, full: &'static str, abbr: Option<&'static str>) -> KeywordUse {
        KeywordUse {
            kind: KeywordKind::Function,
            ..kw(text, full, abbr)
        }
    }

    #[test]
    fn form_alone_accepts_any_casing_and_suggests_the_documented_one() {
        let long = style(Some(KeywordForm::Long), None);
        assert_eq!(long.expected(&kw("SET", "Set", Some("S"))), None);
        assert_eq!(
            long.expected(&kw("s", "Set", Some("S"))).as_deref(),
            Some("Set")
        );
        let short = style(Some(KeywordForm::Short), None);
        assert_eq!(
            short.expected(&kw("Set", "Set", Some("S"))).as_deref(),
            Some("S")
        );
        assert_eq!(short.expected(&kw("while", "While", None)), None);
    }

    #[test]
    fn case_alone_keeps_the_form_written() {
        let lower = style(None, Some(KeywordCase::Lower));
        assert_eq!(
            lower.expected(&kw("S", "Set", Some("S"))).as_deref(),
            Some("s")
        );
        assert_eq!(lower.expected(&kw("set", "Set", Some("S"))), None);
        let pascal = style(None, Some(KeywordCase::Pascal));
        assert_eq!(
            pascal
                .expected(&kw("tstart", "TStart", Some("Ts")))
                .as_deref(),
            Some("TStart")
        );
        assert_eq!(
            pascal.expected(&kw("ts", "TStart", Some("Ts"))).as_deref(),
            Some("TS")
        );
    }

    #[test]
    fn pascal_spells_an_abbreviation_from_its_full_name() {
        let short = style(Some(KeywordForm::Short), Some(KeywordCase::Pascal));
        let expected = |text, full, abbr| short.expected(&function(text, full, Some(abbr)));
        assert_eq!(expected("list", "List", "LI").as_deref(), Some("Li"));
        assert_eq!(
            expected("ListBuild", "ListBuild", "LB").as_deref(),
            Some("LB")
        );
        assert_eq!(
            expected("zdth", "ZDateTimeH", "ZDTH").as_deref(),
            Some("ZDTH")
        );
        assert_eq!(expected("ZIncr", "ZIncrement", "ZINCR"), None);
    }

    #[test]
    fn unset_style_accepts_everything() {
        assert_eq!(
            style(None, None).expected(&kw("sEt", "Set", Some("S"))),
            None
        );
    }

    #[test]
    fn a_command_overrides_the_default_field_by_field() {
        let styles = KeywordStyles::new(
            &parse(r#"{"command": {"form": "Long", "case": "Pascal"}, "command_do": {"form": "Short"}}"#)
                .unwrap(),
        );
        let style = styles.of(&kw("do", "Do", Some("D")));
        assert_eq!(
            style.expected(&kw("do", "Do", Some("D"))).as_deref(),
            Some("D")
        );
        let style = styles.of(&kw("s", "Set", Some("S")));
        assert_eq!(
            style.expected(&kw("s", "Set", Some("S"))).as_deref(),
            Some("Set")
        );
    }

    // The WIT's `command-<name>` fields and the grammar's command keywords must list the same
    // commands: one missing a field couldn't be styled on its own; a field without a command
    // would never apply.
    #[test]
    fn every_command_has_a_style_field_and_every_field_a_command() {
        use tygr_objectscript::cos::COMMAND_NAMES;
        let names: Vec<String> = COMMAND_NAMES
            .iter()
            .map(|(name, _)| *name)
            .filter(|name| name.chars().all(|ch| ch.is_ascii_alphabetic()) && name.len() > 1)
            .map(|name| name.to_ascii_lowercase())
            .collect();
        let json: serde_json::Map<String, Value> = names
            .iter()
            .map(|name| (format!("command_{name}"), serde_json::json!({})))
            .collect();
        parse(&Value::Object(json).to_string()).expect("every command has a field");
        let Value::Object(fields) = serde_json::to_value(parse("{}").unwrap()).unwrap() else {
            unreachable!()
        };
        for key in fields.keys().filter(|key| key.starts_with("command_")) {
            let name = &key["command_".len()..];
            assert!(names.iter().any(|n| n == name), "{key} names no command");
        }
    }

    // The functions with a spelling (`FUNCTION_KEYWORDS` and `SIMPLE_FUNCTIONS`) are exactly
    // those `FUNCTION_NAMES` lists, and the WIT has one field for each.
    #[test]
    fn every_function_has_a_style_field_and_every_field_a_function() {
        use tygr_objectscript::cos::{FUNCTION_KEYWORDS, FUNCTION_NAMES, SIMPLE_FUNCTIONS};
        let spelled: Vec<&str> = FUNCTION_KEYWORDS
            .iter()
            .map(|(full, _)| *full)
            .chain(SIMPLE_FUNCTIONS.iter().map(|f| f.names[0]))
            .collect();
        let abbreviations: Vec<&str> = FUNCTION_KEYWORDS
            .iter()
            .filter_map(|(_, abbr)| *abbr)
            .chain(
                SIMPLE_FUNCTIONS
                    .iter()
                    .filter_map(|f| f.names.get(1).copied()),
            )
            .collect();
        for name in FUNCTION_NAMES {
            assert!(
                spelled
                    .iter()
                    .chain(&abbreviations)
                    .any(|s| s.eq_ignore_ascii_case(name)),
                "{name} has no spelling"
            );
        }
        for name in &spelled {
            assert!(
                FUNCTION_NAMES.iter().any(|n| n.eq_ignore_ascii_case(name)),
                "{name} is missing from FUNCTION_NAMES"
            );
        }
        let names: Vec<String> = spelled.iter().map(|f| f.to_ascii_lowercase()).collect();
        let json: serde_json::Map<String, Value> = names
            .iter()
            .map(|name| (format!("function_{name}"), serde_json::json!({})))
            .collect();
        parse(&Value::Object(json).to_string()).expect("every function has a field");
        let Value::Object(fields) = serde_json::to_value(parse("{}").unwrap()).unwrap() else {
            unreachable!()
        };
        for key in fields.keys().filter(|key| key.starts_with("function_")) {
            let name = &key["function_".len()..];
            assert!(names.iter().any(|n| n == name), "{key} names no function");
        }
    }

    #[test]
    fn commands_and_functions_have_their_own_defaults() {
        let styles = KeywordStyles::new(
            &parse(
                r#"{"command": {"form": "Short"}, "function": {"form": "Long"}, "function_piece": {"form": "Short"}}"#,
            )
            .unwrap(),
        );
        let set = kw("Set", "Set", Some("S"));
        assert_eq!(styles.of(&set).expected(&set).as_deref(), Some("S"));
        let length = function("L", "Length", Some("L"));
        assert_eq!(
            styles.of(&length).expected(&length).as_deref(),
            Some("Length")
        );
        let piece = function("Piece", "Piece", Some("P"));
        assert_eq!(styles.of(&piece).expected(&piece).as_deref(), Some("P"));
    }

    #[test]
    fn parse_rejects_unknown_keys_and_values() {
        let err = parse(r#"{"command": {"from": "Long"}}"#).unwrap_err();
        assert!(err.contains("unknown field `command.from`"), "{err}");
        let err = parse(r#"{"command_sett": {}}"#).unwrap_err();
        assert!(err.contains("unknown field `command_sett`"), "{err}");
        let err = parse(r#"{"command": {"form": "long"}}"#).unwrap_err();
        assert!(
            err.contains("unknown variant `long`, expected `Long` or `Short`"),
            "{err}"
        );
        assert!(parse("{}").is_ok());
    }
}
