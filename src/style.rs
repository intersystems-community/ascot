//! Workspace-wide style rules (`bindings::Style`, declared in the WIT), fixed for an analysis
//! session: `--style <file>` on the CLI, the `style` of an MCP `create_workspace`, or the
//! second argument of a JS `createWorkspace`.

use crate::bindings::{CommandStyle, KeywordCase, KeywordForm, Style};
use serde_json::Value;
use std::collections::HashMap;
use tygr_objectscript::cos::KeywordUse;

// A `Style`'s command rules, resolved once per workspace: `command` merged into each
// `command-<name>`, keyed by the lowercased name.
pub struct CommandStyles {
    default: CommandStyle,
    by_command: HashMap<String, CommandStyle>,
}

impl CommandStyles {
    pub fn new(style: &Style) -> Self {
        let default = style.command.unwrap_or(CommandStyle {
            form: None,
            case: None,
        });
        let Ok(Value::Object(fields)) = serde_json::to_value(style) else {
            unreachable!("a record serializes to an object")
        };
        let by_command = fields
            .into_iter()
            .filter_map(|(key, value)| {
                let name = key.strip_prefix("command_")?.to_string();
                let style: CommandStyle = serde_json::from_value(value).ok()?;
                Some((
                    name,
                    CommandStyle {
                        form: style.form.or(default.form),
                        case: style.case.or(default.case),
                    },
                ))
            })
            .collect();
        CommandStyles {
            default,
            by_command,
        }
    }

    pub fn of(&self, kw: &KeywordUse) -> &CommandStyle {
        self.by_command
            .get(&kw.full.to_ascii_lowercase())
            .unwrap_or(&self.default)
    }
}

impl CommandStyle {
    // How `kw` should have been written under this style, or `None` if it already conforms.
    pub fn expected(&self, kw: &KeywordUse) -> Option<String> {
        let base = match self.form {
            Some(KeywordForm::Long) => kw.full,
            Some(KeywordForm::Short) => kw.abbr.unwrap_or(kw.full),
            None if kw.text.eq_ignore_ascii_case(kw.full) => kw.full,
            None => kw.abbr.unwrap_or(kw.full),
        };
        let expected = match self.case {
            Some(KeywordCase::Lower) => base.to_ascii_lowercase(),
            Some(KeywordCase::Upper) => base.to_ascii_uppercase(),
            Some(KeywordCase::Pascal) => base.to_string(),
            None if kw.text.eq_ignore_ascii_case(base) => return None,
            None => base.to_string(),
        };
        (kw.text != expected).then_some(expected)
    }
}

// `{"command": {"form": "Long"}, "command_do": {"form": "Short"}}` -- variant names as the WIT-generated
// types spell them.
pub fn parse(json: &str) -> Result<Style, String> {
    let input: Value = serde_json::from_str(json).map_err(|err| err.to_string())?;
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
            start: 0,
            text: text.to_string(),
            full,
            abbr,
        }
    }

    fn style(form: Option<KeywordForm>, case: Option<KeywordCase>) -> CommandStyle {
        CommandStyle { form, case }
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
            pascal.expected(&kw("TS", "TStart", Some("Ts"))).as_deref(),
            Some("Ts")
        );
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
        let styles = CommandStyles::new(
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
        for key in fields.keys().filter(|key| *key != "command") {
            let name = key
                .strip_prefix("command_")
                .expect("only command fields so far");
            assert!(names.iter().any(|n| n == name), "{key} names no command");
        }
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
