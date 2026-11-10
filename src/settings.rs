//! The settings a workspace is checked with, one shape wherever they come from: the flags of
//! `ascot lsp`, the `initializationOptions` of its LSP `initialize` request, the `settings` of a
//! `workspace/didChangeConfiguration`, and the MCP `create_workspace` request. Every field is
//! optional, and an empty string is "not given", since a plugin's unset `userConfig` entry
//! interpolates to `""`.

use serde::Deserialize;

use crate::{IrisContext, bindings::Style, diagnostic_filter::Filter, iris, style};

#[derive(Clone, Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct Settings {
    /// Where lookups that can't be resolved from documents in this workspace (e.g. a class that
    /// isn't open here) go: an IRIS Community release such as `2026.2`, whose pre-built
    /// declarations ascot downloads once; an IRIS instance, `scheme://user:pass@host:port`,
    /// queried via the Atelier REST API; the path of a declaration file written by
    /// `ascot declare`; or `none`, for no environment at all. Defaults to the newest release ascot
    /// was built for.
    pub iris: Option<String>,
    /// The namespace of a live instance, `%SYS` by default. A release names its own after its tag
    /// and a declaration file carries its own, so giving one with either is an error.
    pub iris_namespace: Option<String>,
    /// Workspace-wide style rules, e.g. `{"command": {"form": "Long"}, "command_do": {"form": "Short"}}` to
    /// have every command keyword but `Do`'s written in full; `function` and `function_<name>` do
    /// the same for intrinsic functions. Applies to every document. The keys are documented at
    /// https://github.com/intersystems-community/ascot/blob/main/STYLE.md. When the server shares
    /// the client's file system, the path of a JSON file holding the object is accepted too.
    pub style: Option<StyleSetting>,
    /// Which codes are reported, and at what severity: `VERB:TARGETS` strings applied in order, a
    /// later one overriding an earlier one. VERB is `off`, `on` (at the code's own severity) or a
    /// severity (`error`, `warning`, `information`, `hint`); TARGETS is a comma-separated list of
    /// codes, severities (every code currently reported at one), or `all`. A list, or one string
    /// of them separated by spaces. For example `["off:A5579,A4043"]`, `"off:hint,information"`,
    /// or `["off:all", "on:A7095,A6851"]`.
    pub diagnostics: Option<Diagnostics>,
}

#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum StyleSetting {
    Path(String),
    Object(serde_json::Map<String, serde_json::Value>),
}

#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Diagnostics {
    Text(String),
    List(Vec<String>),
}

impl Settings {
    /// `over`'s settings on top of these: whatever it gives replaces, the rest stays.
    pub fn overlay(self, over: Settings) -> Settings {
        let (style, diagnostics) = (over.has_style(), over.has_diagnostics());
        Settings {
            iris: over.iris().map(str::to_string).or(self.iris),
            iris_namespace: over.iris_namespace().map(str::to_string).or_else(|| {
                self.iris_namespace
                    .filter(|_| over.iris().is_none_or(|iris| iris.contains("://")))
            }),
            style: if style { over.style } else { self.style },
            diagnostics: if diagnostics {
                over.diagnostics
            } else {
                self.diagnostics
            },
        }
    }

    pub fn iris(&self) -> Option<&str> {
        given(&self.iris)
    }

    pub fn iris_namespace(&self) -> Option<&str> {
        given(&self.iris_namespace)
    }

    pub fn has_style(&self) -> bool {
        match &self.style {
            Some(StyleSetting::Path(path)) => !path.is_empty(),
            Some(StyleSetting::Object(_)) => true,
            None => false,
        }
    }

    pub fn has_diagnostics(&self) -> bool {
        match &self.diagnostics {
            Some(Diagnostics::Text(text)) => !text.trim().is_empty(),
            Some(Diagnostics::List(_)) => true,
            None => false,
        }
    }

    /// The environment `iris` names, the newest release's declarations when none is given; `none`
    /// is no environment.
    pub fn open_iris(&self) -> Result<Option<Box<dyn IrisContext>>, String> {
        let name = self.iris().unwrap_or(iris::LATEST);
        iris::open(name, self.iris_namespace()).map_err(|err| format!("iris {name}: {err}"))
    }

    /// The style given, if any. A path is read only when the server shares the client's file
    /// system, since a path is the client's.
    pub fn style(&self, shared_fs: bool) -> Result<Option<Style>, String> {
        match &self.style {
            Some(StyleSetting::Object(object)) => {
                style::from_value(serde_json::Value::Object(object.clone()))
                    .map(Some)
                    .map_err(|err| format!("style: {err}"))
            }
            Some(StyleSetting::Path(path)) if !path.is_empty() => {
                if !shared_fs {
                    return Err(format!(
                        "style: {path}: a path is read only when the server shares the client's \
                         file system (--shared-fs)"
                    ));
                }
                let text =
                    std::fs::read_to_string(path).map_err(|err| format!("style: {path}: {err}"))?;
                style::parse(&text)
                    .map(Some)
                    .map_err(|err| format!("style: {path}: {err}"))
            }
            _ => Ok(None),
        }
    }

    /// The diagnostics filter given; none reports every code at its own severity.
    pub fn filter(&self) -> Result<Filter, String> {
        let text = match &self.diagnostics {
            Some(Diagnostics::Text(text)) => text.clone(),
            Some(Diagnostics::List(list)) => list.join(" "),
            None => String::new(),
        };
        text.parse().map_err(|err| format!("diagnostics: {err}"))
    }
}

fn given(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|value| !value.is_empty())
}

impl From<Style> for StyleSetting {
    fn from(style: Style) -> Self {
        match serde_json::to_value(style) {
            Ok(serde_json::Value::Object(object)) => StyleSetting::Object(object),
            _ => StyleSetting::Object(Default::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Settings {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn an_empty_string_is_not_given_and_does_not_override() {
        let flags = parse(r#"{"iris": "none", "diagnostics": "off:A7095"}"#);
        let options = parse(r#"{"iris": "", "style": "", "diagnostics": ""}"#);
        let settings = flags.overlay(options);
        assert_eq!(settings.iris(), Some("none"));
        assert!(!settings.has_style());
        assert_eq!(settings.filter().unwrap().0.len(), 1);
    }

    #[test]
    fn a_namespace_stays_only_while_iris_stays_a_live_instance() {
        let flags = parse(r#"{"iris": "http://u:p@h:1", "iris_namespace": "USER"}"#);
        let to_url = flags
            .clone()
            .overlay(parse(r#"{"iris": "http://u:p@h:2"}"#));
        assert_eq!(to_url.iris_namespace(), Some("USER"));
        let to_release = flags.overlay(parse(r#"{"iris": "latest"}"#));
        assert_eq!(to_release.iris_namespace(), None);
    }

    #[test]
    fn diagnostics_come_as_a_list_or_as_space_separated_text() {
        let text = parse(r#"{"diagnostics": "off:hint on:A5579"}"#);
        let list = parse(r#"{"diagnostics": ["off:hint", "on:A5579"]}"#);
        assert_eq!(text.filter().unwrap().0.len(), 2);
        assert_eq!(list.filter().unwrap().0.len(), 2);
        assert!(parse(r#"{"diagnostics": "off:Z1"}"#).filter().is_err());
    }

    #[test]
    fn a_style_path_needs_a_shared_file_system() {
        let settings = parse(r#"{"style": "/no/such/style.json"}"#);
        assert!(settings.style(false).unwrap_err().contains("--shared-fs"));
        assert!(settings.style(true).is_err());
        let object = parse(r#"{"style": {"command": {"form": "Long"}}}"#);
        assert!(object.style(false).unwrap().is_some());
        assert!(parse(r#"{"style": {"commnad": {}}}"#).style(false).is_err());
    }
}
