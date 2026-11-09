//! A check's result as the CLI's JSON output, the MCP server and the SARIF log all hand it over:
//! every document's diagnostics under its uri.

use serde::Serialize;

use crate::bindings::Diagnostic;

/// A document's diagnostics, under its uri.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct FileDiagnostics {
    pub uri: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// Every checked document's diagnostics. An object rather than a bare array, since MCP's
/// `outputSchema` has to describe an object at the top level.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DiagnosticsResult {
    pub files: Vec<FileDiagnostics>,
}
