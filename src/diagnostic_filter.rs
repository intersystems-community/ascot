//! `--diagnostics` and `create_workspace`'s `diagnostics`: which codes are reported, and at what
//! severity.

use std::str::FromStr;

use tygr::{Grammar, GrammarRule, StringEq, StringOf1, Suffix, VecSep, bnf_rules, char_class};

use crate::bindings::{Diagnostic, DiagnosticSeverity};
use crate::check::TypeErrorKind;

/// The `VERB:TARGETS` values in order: the last one naming a code, or `all`, decides it.
#[derive(Default)]
pub struct Filter(pub Vec<Batch>);

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
    Error(StringEq!("error")),
    Warning(StringEq!("warning")),
    Information(StringEq!("information")),
    Hint(StringEq!("hint")),
}

#[derive(Clone, Grammar)]
pub enum Target {
    All(StringEq!("all")),
    Code(Code),
}

#[derive(Clone, Grammar)]
pub struct Code(StringOf1<CodeChar>);

char_class!(pub CodeChar, "letters or digits", |c| c.is_ascii_alphanumeric());

/// The syntax of a value, for help and error messages.
pub fn bnf() -> String {
    bnf_rules![Batch, Verb, Target, Code]
}

impl Filter {
    pub fn apply(&self, diagnostic: Diagnostic) -> Option<Diagnostic> {
        let batch = (self.0.iter().rev()).find(|batch| batch.names(&diagnostic.code));
        match batch.map(|batch| &*batch.verb) {
            Some(Verb::Off(_)) => None,
            None | Some(Verb::On(_)) => Some(diagnostic),
            Some(Verb::Error(_)) => diagnostic.at(DiagnosticSeverity::Error),
            Some(Verb::Warning(_)) => diagnostic.at(DiagnosticSeverity::Warning),
            Some(Verb::Information(_)) => diagnostic.at(DiagnosticSeverity::Information),
            Some(Verb::Hint(_)) => diagnostic.at(DiagnosticSeverity::Hint),
        }
    }
}

impl Diagnostic {
    fn at(mut self, severity: DiagnosticSeverity) -> Option<Self> {
        self.severity = severity;
        Some(self)
    }
}

impl Batch {
    fn names(&self, code: &str) -> bool {
        self.targets.items.iter().any(|target| match target {
            Target::All(_) => true,
            Target::Code(known) => known.print() == code,
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
