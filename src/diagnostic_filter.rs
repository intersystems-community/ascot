//! `--diagnostics` and `create_workspace`'s `diagnostics`: which codes are reported, and at what
//! severity.

use std::str::FromStr;

use tygr::{Grammar, Parse, Print, StringEq, StringOf1, Suffix, char_class};
use tygr_objectscript::cos::VecSep;

use crate::bindings::{Diagnostic, DiagnosticSeverity};
use crate::check::TypeErrorKind;

/// The `VERB:TARGETS` values, applied in order; each later one overrides the earlier.
#[derive(Clone, Default)]
pub struct Filter(pub Vec<Batch>);

impl FromStr for Filter {
    type Err = String;

    /// Batches separated by whitespace; none at all is every code at its own severity.
    fn from_str(text: &str) -> Result<Self, String> {
        text.split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map(Filter)
    }
}

#[derive(Clone, Grammar)]
pub struct Batch {
    verb: Suffix<Verb, StringEq!(":")>,
    targets: VecSep<Target, StringEq!(",")>,
}

#[derive(Clone, Grammar)]
pub enum Verb {
    Off(StringEq!("off")),
    /// At the code's own severity.
    On(StringEq!("on")),
    At(Severity),
}

#[derive(Clone, Grammar)]
pub enum Severity {
    Error(StringEq!("error")),
    Warning(StringEq!("warning")),
    Information(StringEq!("information")),
    Hint(StringEq!("hint")),
}

/// A code, or every code currently reported at a severity, or every code.
#[derive(Clone, Grammar)]
pub enum Target {
    All(StringEq!("all")),
    Severity(Severity),
    Code(Code),
}

#[derive(Clone, Grammar)]
pub struct Code(StringOf1<CodeChar>);

char_class!(pub CodeChar, "letters or digits", |c| c.is_ascii_alphanumeric());

/// The syntax of a value, for help and error messages.
pub fn bnf() -> String {
    Batch::bnf_rules()
        .iter()
        .map(|rule| format!("{rule}\n"))
        .collect()
}

impl Filter {
    /// `diagnostic` arrives at its code's own severity.
    pub fn apply(&self, mut diagnostic: Diagnostic) -> Option<Diagnostic> {
        let own = diagnostic.severity;
        let mut on = true;
        for batch in &self.0 {
            if !batch.names(&diagnostic) {
                continue;
            }
            match &*batch.verb {
                Verb::Off(_) => on = false,
                Verb::On(_) => (on, diagnostic.severity) = (true, own),
                Verb::At(severity) => (on, diagnostic.severity) = (true, severity.into()),
            }
        }
        on.then_some(diagnostic)
    }
}

impl From<&Severity> for DiagnosticSeverity {
    fn from(severity: &Severity) -> Self {
        match severity {
            Severity::Error(_) => Self::Error,
            Severity::Warning(_) => Self::Warning,
            Severity::Information(_) => Self::Information,
            Severity::Hint(_) => Self::Hint,
        }
    }
}

impl Batch {
    fn names(&self, diagnostic: &Diagnostic) -> bool {
        self.targets.items.iter().any(|target| match target {
            Target::All(_) => true,
            Target::Severity(severity) => diagnostic.severity == severity.into(),
            Target::Code(known) => known.print() == diagnostic.code,
        })
    }
}

impl FromStr for Batch {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let batch = Self::parse(text).map_err(|_| format!("{text:?} is not\n{}", bnf()))?;
        for target in &batch.targets.items {
            if let Target::Code(code) = target
                && !TypeErrorKind::CODES.contains(&code.print().as_str())
            {
                return Err(format!("{} is not a diagnostic code", code.print()));
            }
        }
        Ok(batch)
    }
}
