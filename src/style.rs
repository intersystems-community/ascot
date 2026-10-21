//! Workspace-wide style rules (`bindings::Style`, declared in the WIT), fixed for an analysis
//! session: `--style <file>` on the CLI, the `style` of an MCP `create_workspace`, or the
//! second argument of a JS `createWorkspace`.

use crate::bindings::{CommandStyle, KeywordCase, KeywordForm, Style};
use serde_json::Value;
use tygr_objectscript::cos::KeywordUse;

impl CommandStyle {
    pub fn is_set(&self) -> bool {
        self.form.is_some() || self.case.is_some()
    }

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

// `{"commands": {"form": "Long", "case": "Pascal"}}` -- variant names as the WIT-generated
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
        assert!(!style(None, None).is_set());
        assert_eq!(
            style(None, None).expected(&kw("sEt", "Set", Some("S"))),
            None
        );
    }

    #[test]
    fn parse_rejects_unknown_keys_and_values() {
        let err = parse(r#"{"commands": {"from": "Long"}}"#).unwrap_err();
        assert!(err.contains("unknown field `commands.from`"), "{err}");
        let err = parse(r#"{"commands": {"form": "long"}}"#).unwrap_err();
        assert!(
            err.contains("unknown variant `long`, expected `Long` or `Short`"),
            "{err}"
        );
        assert!(parse(r#"{"commands": {}}"#).is_ok());
    }
}
