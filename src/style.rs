//! Workspace-wide style rules (`bindings::Style`, declared in the WIT): `--style <file>` on the
//! CLI, the `style` setting of the LSP and MCP servers, or the second argument of a JS
//! `createWorkspace`.

use crate::bindings::{CommandStyle, FunctionStyle, KeywordCase, KeywordForm, Style};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::collections::HashMap;
use tygr_objectscript::cos::{KeywordKind, KeywordUse};

// A `Style`, resolved once per workspace: `rules` as given, keyword styles merged.
pub struct Styles {
    pub rules: Style,
    pub keywords: KeywordStyles,
}

impl Styles {
    pub fn new(style: &Style) -> Self {
        Styles {
            rules: *style,
            keywords: KeywordStyles::new(style),
        }
    }
}

// A `Style`'s keyword rules: each `<kind><Name>` field merged with its `<kind>` default, keyed by
// the name lowercased (`do`, `listbuild`).
pub struct KeywordStyles {
    commands: Kind<CommandStyle>,
    functions: Kind<FunctionStyle>,
}

struct Kind<T> {
    default: T,
    by_name: HashMap<String, T>,
}

impl<T: DeserializeOwned> Kind<T> {
    // The `<prefix>` and `<prefix><Name>` fields of `fields`, the latter completed by the former.
    fn new(fields: &Map<String, Value>, prefix: &str) -> Self {
        let default = fields.get(prefix).cloned().unwrap_or(Value::Null);
        let by_name = fields
            .iter()
            .filter_map(|(key, value)| {
                let name = key.strip_prefix(prefix)?;
                name.chars().next().filter(char::is_ascii_uppercase)?;
                let mut merged = default.as_object().cloned().unwrap_or_default();
                for (field, value) in value.as_object()? {
                    if !value.is_null() {
                        merged.insert(field.clone(), value.clone());
                    }
                }
                Some((
                    name.to_ascii_lowercase(),
                    serde_json::from_value(Value::Object(merged)).ok()?,
                ))
            })
            .collect();
        let default = serde_json::from_value(default)
            .or_else(|_| serde_json::from_value(Value::Object(Map::new())))
            .expect("a record of optional fields deserializes from an empty object");
        Kind { default, by_name }
    }

    fn of(&self, name: &str) -> &T {
        self.by_name.get(name).unwrap_or(&self.default)
    }
}

impl KeywordStyles {
    pub fn new(style: &Style) -> Self {
        let Ok(Value::Object(fields)) = serde_json::to_value(style) else {
            unreachable!("a record serializes to an object")
        };
        KeywordStyles {
            commands: Kind::new(&fields, "command"),
            functions: Kind::new(&fields, "function"),
        }
    }

    // How `kw` should have been written, or `None` if it already conforms.
    pub fn expected(&self, kw: &KeywordUse) -> Option<String> {
        let name = kw.full.to_ascii_lowercase();
        let (form, case) = match kw.kind {
            KeywordKind::Command => {
                let style = self.commands.of(&name);
                (style.form, style.case)
            }
            KeywordKind::Function => {
                let style = self.functions.of(&name);
                (style.form, style.case)
            }
        };
        expected(form, case, kw)
    }

    // Whether `command` (its full name, lowercased) may put a postconditional on an argument.
    pub fn allows_postcondition_on_argument(&self, command: &str) -> bool {
        self.commands.of(command).allow_postcondition_on_argument != Some(false)
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

fn expected(
    form: Option<KeywordForm>,
    case: Option<KeywordCase>,
    kw: &KeywordUse,
) -> Option<String> {
    let full = kw.full.to_string();
    let abbr = kw.abbr.map(|abbr| spell_abbr(kw.full, abbr));
    let base = match form {
        Some(KeywordForm::Long) => full,
        None if kw.text.eq_ignore_ascii_case(kw.full) => full,
        Some(KeywordForm::Short) | None => abbr.unwrap_or(full),
    };
    let expected = match case {
        Some(KeywordCase::Lower) => base.to_ascii_lowercase(),
        Some(KeywordCase::Upper) => base.to_ascii_uppercase(),
        None if kw.text.eq_ignore_ascii_case(&base) => return None,
        Some(KeywordCase::Pascal) | None => base,
    };
    (kw.text != expected).then_some(expected)
}

// `{"command": {"form": "long"}, "functionPiece": {"form": "short"}}`: camelCase keys and
// lowercase values, the JS binding's spelling of the record, so one style file serves every binding.
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

    struct Spelling(Option<KeywordForm>, Option<KeywordCase>);

    impl Spelling {
        fn expected(&self, kw: &KeywordUse) -> Option<String> {
            expected(self.0, self.1, kw)
        }
    }

    fn style(form: Option<KeywordForm>, case: Option<KeywordCase>) -> Spelling {
        Spelling(form, case)
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

    // `commandDo` for `do`, and back: `None` for a key that names no keyword, `command` itself included.
    fn field(prefix: &str, name: &str) -> String {
        let (head, tail) = name.split_at(1);
        format!("{prefix}{}{tail}", head.to_ascii_uppercase())
    }

    fn named(key: &str, prefix: &str) -> Option<String> {
        let name = key.strip_prefix(prefix)?;
        name.chars().next().filter(char::is_ascii_uppercase)?;
        Some(name.to_ascii_lowercase())
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
            &parse(r#"{"command": {"form": "long", "case": "pascal"}, "commandDo": {"form": "short"}}"#)
                .unwrap(),
        );
        assert_eq!(
            styles.expected(&kw("do", "Do", Some("D"))).as_deref(),
            Some("D")
        );
        assert_eq!(
            styles.expected(&kw("s", "Set", Some("S"))).as_deref(),
            Some("Set")
        );
        assert!(styles.allows_postcondition_on_argument("do"));
        let styles = KeywordStyles::new(
            &parse(r#"{"command": {"allowPostconditionOnArgument": false}, "commandDo": {"allowPostconditionOnArgument": true}}"#)
                .unwrap(),
        );
        assert!(styles.allows_postcondition_on_argument("do"));
        assert!(!styles.allows_postcondition_on_argument("goto"));
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
            .map(|name| (field("command", name), serde_json::json!({})))
            .collect();
        parse(&Value::Object(json).to_string()).expect("every command has a field");
        let Value::Object(fields) = serde_json::to_value(parse("{}").unwrap()).unwrap() else {
            unreachable!()
        };
        for name in fields.keys().filter_map(|key| named(key, "command")) {
            assert!(names.contains(&name), "{name} names no command");
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
            .map(|name| (field("function", name), serde_json::json!({})))
            .collect();
        parse(&Value::Object(json).to_string()).expect("every function has a field");
        let Value::Object(fields) = serde_json::to_value(parse("{}").unwrap()).unwrap() else {
            unreachable!()
        };
        for name in fields.keys().filter_map(|key| named(key, "function")) {
            assert!(names.contains(&name), "{name} names no function");
        }
    }

    #[test]
    fn commands_and_functions_have_their_own_defaults() {
        let styles = KeywordStyles::new(
            &parse(
                r#"{"command": {"form": "short"}, "function": {"form": "long"}, "functionPiece": {"form": "short"}}"#,
            )
            .unwrap(),
        );
        let set = kw("Set", "Set", Some("S"));
        assert_eq!(styles.expected(&set).as_deref(), Some("S"));
        let length = function("L", "Length", Some("L"));
        assert_eq!(styles.expected(&length).as_deref(), Some("Length"));
        let piece = function("Piece", "Piece", Some("P"));
        assert_eq!(styles.expected(&piece).as_deref(), Some("P"));
    }

    #[test]
    fn parse_rejects_unknown_keys_and_values() {
        let err = parse(r#"{"command": {"from": "long"}}"#).unwrap_err();
        assert!(err.contains("unknown field `command.from`"), "{err}");
        let err = parse(r#"{"commandSett": {}}"#).unwrap_err();
        assert!(err.contains("unknown field `commandSett`"), "{err}");
        let err = parse(r#"{"command": {"form": "Long"}}"#).unwrap_err();
        assert!(
            err.contains("unknown variant `Long`, expected `long` or `short`"),
            "{err}"
        );
        assert!(parse("{}").is_ok());
    }
}
