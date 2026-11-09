//! SARIF 2.1.0 for `ascot analyze --format sarif`: one run, one result per diagnostic and one rule
//! per code, so code scanning uploads it and scripts filter by code and file without parsing text.

use std::fmt;

use serde::Serialize;

use crate::bindings::{Diagnostic, DiagnosticSeverity, Location, Range};
use crate::report::FileDiagnostics;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Log {
    #[serde(rename = "$schema")]
    schema: &'static str,
    version: &'static str,
    runs: [Run; 1],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Run {
    tool: Tool,
    results: Vec<Finding>,
}

#[derive(Serialize)]
struct Tool {
    driver: Driver,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Driver {
    name: &'static str,
    version: &'static str,
    information_uri: &'static str,
    rules: Vec<Rule>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Rule {
    id: String,
    help_uri: String,
}

// A SARIF `result`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Finding {
    rule_id: String,
    rule_index: usize,
    level: &'static str,
    message: Message,
    locations: [PhysicalLocation; 1],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    related_locations: Vec<RelatedLocation>,
}

#[derive(Serialize)]
struct Message {
    text: String,
}

impl From<&str> for Message {
    fn from(text: &str) -> Self {
        Self {
            text: text.to_string(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PhysicalLocation {
    physical_location: Physical,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Physical {
    artifact_location: ArtifactLocation,
    region: Region,
}

#[derive(Serialize)]
struct ArtifactLocation {
    uri: String,
}

impl From<&Location> for PhysicalLocation {
    fn from(location: &Location) -> Self {
        Self {
            physical_location: Physical::from(location),
        }
    }
}

// SARIF counts lines and columns from 1, with `end_column` just past the region.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Region {
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RelatedLocation {
    id: usize,
    physical_location: Physical,
    message: Message,
}

impl From<&Range> for Region {
    fn from(range: &Range) -> Self {
        Self {
            start_line: range.start.line + 1,
            start_column: range.start.character + 1,
            end_line: range.end.line + 1,
            end_column: range.end.character + 1,
        }
    }
}

impl From<&Location> for Physical {
    fn from(location: &Location) -> Self {
        Self {
            artifact_location: ArtifactLocation {
                uri: location.uri.clone(),
            },
            region: Region::from(&location.range),
        }
    }
}

impl From<DiagnosticSeverity> for &'static str {
    fn from(severity: DiagnosticSeverity) -> Self {
        match severity {
            DiagnosticSeverity::Error => "error",
            DiagnosticSeverity::Warning => "warning",
            DiagnosticSeverity::Information | DiagnosticSeverity::Hint => "note",
        }
    }
}

impl Log {
    fn rule_index(&mut self, diagnostic: &Diagnostic) -> usize {
        let rules = &mut self.runs[0].tool.driver.rules;
        rules
            .iter()
            .position(|rule| rule.id == diagnostic.code)
            .unwrap_or_else(|| {
                rules.push(Rule {
                    id: diagnostic.code.clone(),
                    help_uri: diagnostic.code_description.clone(),
                });
                rules.len() - 1
            })
    }

    fn add(&mut self, uri: &str, diagnostic: &Diagnostic) {
        let rule_index = self.rule_index(diagnostic);
        self.runs[0].results.push(Finding {
            rule_id: diagnostic.code.clone(),
            rule_index,
            level: diagnostic.severity.into(),
            message: diagnostic.message.as_str().into(),
            locations: [PhysicalLocation::from(&Location {
                uri: uri.to_string(),
                range: diagnostic.range,
            })],
            related_locations: diagnostic
                .related_information
                .iter()
                .enumerate()
                .map(|(i, related)| RelatedLocation {
                    id: i + 1,
                    physical_location: Physical::from(&related.location),
                    message: related.message.as_str().into(),
                })
                .collect(),
        });
    }
}

impl Default for Log {
    fn default() -> Self {
        Self {
            schema: "https://json.schemastore.org/sarif-2.1.0.json",
            version: "2.1.0",
            runs: [Run {
                tool: Tool {
                    driver: Driver {
                        name: "ascot",
                        version: env!("CARGO_PKG_VERSION"),
                        information_uri: "https://github.com/intersystems-community/ascot",
                        rules: Vec::new(),
                    },
                },
                results: Vec::new(),
            }],
        }
    }
}

/// The diagnostics of each file, as one SARIF log.
impl FromIterator<FileDiagnostics> for Log {
    fn from_iter<I: IntoIterator<Item = FileDiagnostics>>(files: I) -> Self {
        let mut log = Self::default();
        for file in files {
            for diagnostic in &file.diagnostics {
                log.add(&file.uri, diagnostic);
            }
        }
        log
    }
}

/// The log as pretty-printed JSON.
impl fmt::Display for Log {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let json = serde_json::to_string_pretty(self).map_err(|_| fmt::Error)?;
        f.write_str(&json)
    }
}
