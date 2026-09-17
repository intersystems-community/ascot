//! Every diagnostic the checker can raise, as the `TypeErrorKind` enum, with its message text,
//! severity and related-information locations (`TypeError` converts into `bindings::Diagnostic`).
//!
//! `QuasiTypeError` is an error found before its source range is known; rules in `mod.rs` attach
//! the range with `IntoTypeError::with_range`. Adding or rewording a user-visible message means
//! editing this file.

use crate::{
    bindings::{ArgMode, DiagnosticRelatedInformation, DiagnosticSeverity, Location, Range},
    check::types::*,
};

#[derive(Debug)]
pub enum QuasiTypeError {
    WithoutRange(TypeErrorKind, Vec<DiagnosticRelatedInformation>),
    WithRange(TypeError),
}

impl QuasiTypeError {
    pub fn into_error(self, f: impl FnOnce() -> Range) -> TypeError {
        match self {
            QuasiTypeError::WithoutRange(kind, related_information) => {
                TypeError::new(kind, f(), related_information)
            }
            QuasiTypeError::WithRange(err) => err,
        }
    }

    pub fn unimplemented(msg: String) -> Self {
        Self::WithoutRange(TypeErrorKind::Unimplemented(msg), vec![])
    }

    pub fn undefined_macro(name: String, suggestions: Vec<String>) -> Self {
        Self::WithoutRange(TypeErrorKind::UndefinedMacro { name, suggestions }, vec![])
    }

    pub fn unreachable() -> Self {
        Self::WithoutRange(TypeErrorKind::Unreachable, vec![])
    }

    pub(crate) fn condition_always(value: bool) -> Self {
        Self::WithoutRange(TypeErrorKind::ConditionAlways(value), vec![])
    }

    pub(crate) fn return_nothing_for_x(wanted: Ty, declared_at: Location) -> Self {
        Self::WithoutRange(
            TypeErrorKind::ReturnNothingForX { wanted },
            vec![DiagnosticRelatedInformation {
                location: declared_at,
                message: "A return type is declared.".into(),
            }],
        )
    }

    pub(crate) fn return_x_for_nothing(actual: Ty, declared_at: Location) -> Self {
        Self::WithoutRange(
            TypeErrorKind::ReturnXForNothing { actual },
            vec![DiagnosticRelatedInformation {
                location: declared_at,
                message: "No return type is declared.".into(),
            }],
        )
    }

    pub(crate) fn quit_x_in_control(actual: Ty, location_of_control: Location) -> Self {
        Self::WithoutRange(
            TypeErrorKind::QuitXInControl { actual },
            vec![DiagnosticRelatedInformation {
                location: location_of_control,
                message: "The control operation.".into(),
            }],
        )
    }

    pub fn undefined() -> Self {
        Self::WithoutRange(TypeErrorKind::Undefined { expr: None }, vec![])
    }

    pub fn maybe_undefined(ty: Ty) -> Self {
        Self::WithoutRange(TypeErrorKind::MaybeUndefined { ty, expr: None }, vec![])
    }

    // Fills in `Undefined`/`MaybeUndefined`'s expression text — the only two kinds that benefit
    // from echoing the source back, since every other kind already names what's wrong. Called
    // once, by the outermost ranged expression wrapping the failure (see `CheckExpr` in
    // `check::mod`), so the text always matches whatever the diagnostic's range highlights.
    pub(crate) fn with_expr_text(self, text: impl FnOnce() -> String) -> Self {
        match self {
            Self::WithoutRange(TypeErrorKind::Undefined { expr: None }, related_information) => {
                Self::WithoutRange(
                    TypeErrorKind::Undefined { expr: Some(text()) },
                    related_information,
                )
            }
            Self::WithoutRange(
                TypeErrorKind::MaybeUndefined { ty, expr: None },
                related_information,
            ) => Self::WithoutRange(
                TypeErrorKind::MaybeUndefined {
                    ty,
                    expr: Some(text()),
                },
                related_information,
            ),
            other => other,
        }
    }

    pub fn type_incompatible(source: Ty, target: Ty) -> Self {
        Self::WithoutRange(TypeErrorKind::TypeIncompatible { source, target }, vec![])
    }

    pub fn assign_incompatible(name: String, source: Ty, target: Ty) -> Self {
        Self::WithoutRange(
            TypeErrorKind::AssignIncompatible {
                name,
                source,
                target,
            },
            vec![],
        )
    }

    pub fn access_instance_only_member_from_class_context(name: String) -> Self {
        Self::WithoutRange(
            TypeErrorKind::AccessInstanceOnlyMemberFromClassContext { name },
            vec![],
        )
    }

    pub fn not_procedure(ty: Ty) -> Self {
        Self::WithoutRange(TypeErrorKind::NotProcedure(ty), vec![])
    }

    pub fn procedure_not_called() -> Self {
        Self::WithoutRange(TypeErrorKind::ProcedureNotCalled, vec![])
    }

    pub fn not_in_class() -> Self {
        Self::WithoutRange(TypeErrorKind::NotInClass, vec![])
    }

    pub fn procedure_call_returns_nothing() -> Self {
        Self::WithoutRange(TypeErrorKind::ProcedureCallReturnsNothing, vec![])
    }

    pub fn require_private_variable(name: String) -> Self {
        Self::WithoutRange(TypeErrorKind::RequirePrivateVariable(name), vec![])
    }

    pub fn too_few_arguments(next_arg: String, declared_at: Option<Location>) -> Self {
        let related_information = declared_at
            .map(|location| {
                vec![DiagnosticRelatedInformation {
                    location,
                    message: "Parameter declared here — add a default value to make it optional."
                        .into(),
                }]
            })
            .unwrap_or_default();
        Self::WithoutRange(
            TypeErrorKind::TooFewArguments { next_arg },
            related_information,
        )
    }

    pub fn too_many_arguments() -> Self {
        Self::WithoutRange(TypeErrorKind::TooManyArguments, vec![])
    }

    pub fn case_default_not_last() -> Self {
        Self::WithoutRange(TypeErrorKind::CaseDefaultNotLast, vec![])
    }

    pub fn unknown_function(name: String, suggestions: Vec<String>) -> Self {
        Self::WithoutRange(TypeErrorKind::UnknownFunction { name, suggestions }, vec![])
    }

    pub fn unknown_special_variable(name: String, suggestions: Vec<String>) -> Self {
        Self::WithoutRange(
            TypeErrorKind::UnknownSpecialVariable { name, suggestions },
            vec![],
        )
    }

    pub fn invalid_macro_argument(msg: impl Into<String>) -> Self {
        Self::WithoutRange(TypeErrorKind::InvalidMacroArgument(msg.into()), vec![])
    }

    pub fn invalid_zstrip_action(action: String, reason: &'static str) -> Self {
        Self::WithoutRange(
            TypeErrorKind::InvalidZStripAction { action, reason },
            vec![],
        )
    }

    pub fn number_implicitly_converted_to_string(value: String) -> Self {
        Self::WithoutRange(
            TypeErrorKind::NumberImplicitlyConvertedToString(value),
            vec![],
        )
    }

    pub fn discarded_return_value(class: String) -> Self {
        Self::WithoutRange(TypeErrorKind::DiscardedReturnValue(class), vec![])
    }

    pub fn invalid_fnumber_format(format: String, reason: String) -> Self {
        Self::WithoutRange(
            TypeErrorKind::InvalidFNumberFormat { format, reason },
            vec![],
        )
    }

    pub fn cannot_find_member(
        cls: String,
        mem: String,
        suggestions: Vec<String>,
        has_get: bool,
    ) -> Self {
        Self::WithoutRange(
            TypeErrorKind::CannotFindMember {
                cls,
                mem,
                suggestions,
                has_get,
            },
            vec![],
        )
    }

    pub fn cannot_find_class(cls: String) -> Self {
        Self::WithoutRange(TypeErrorKind::CannotFindClass { cls }, vec![])
    }

    pub fn cannot_find_label(label: String, routine: Option<String>) -> Self {
        Self::WithoutRange(TypeErrorKind::CannotFindLabel { label, routine }, vec![])
    }

    pub fn cannot_find_routine(routine: String) -> Self {
        Self::WithoutRange(TypeErrorKind::CannotFindRoutine(routine), vec![])
    }

    pub fn continue_not_in_loop() -> Self {
        Self::WithoutRange(TypeErrorKind::ContinueNotInLoop, vec![])
    }

    pub(crate) fn inconsistent_declaration(
        name: String,
        ty1: Option<Ty>,
        ty2: Option<Ty>,
    ) -> QuasiTypeError {
        Self::WithoutRange(
            TypeErrorKind::InconsistentDeclaration { name, ty1, ty2 },
            vec![],
        )
    }

    pub(crate) fn mixed_precedence() -> QuasiTypeError {
        Self::WithoutRange(TypeErrorKind::MixedPrecedence, vec![])
    }
}

impl From<TypeError> for QuasiTypeError {
    fn from(err: TypeError) -> Self {
        Self::WithRange(err)
    }
}

pub trait IntoTypeError<T> {
    fn with_range(self, f: impl FnOnce() -> Range) -> Result<T, TypeError>;
}

impl<T> IntoTypeError<T> for Result<T, QuasiTypeError> {
    fn with_range(self, f: impl FnOnce() -> Range) -> Result<T, TypeError> {
        self.map_err(|e| e.into_error(f))
    }
}

#[derive(Debug)]
pub enum TypeErrorKind {
    CannotFindClass {
        cls: String,
    },
    CannotFindMember {
        cls: String,
        mem: String,
        suggestions: Vec<String>,
        has_get: bool,
    },
    CannotFindLabel {
        label: String,
        routine: Option<String>,
    },
    CannotFindRoutine(String),
    CannotFindInclude(String),
    MacroExpansionTooDeep(String),
    AccessInstanceOnlyMemberFromClassContext {
        name: String,
    },
    RedeclareVariable(String),
    RedundantNew(String),
    NewInProcedureBlock(String),
    UnrecognizedPropertyKeyword(String),
    UnrecognizedTypeParam {
        name: String,
        class: String,
        is_property_keyword: bool,
        suggestions: Vec<String>,
    },
    UnrecognizedParameterKeyword(String),
    UnrecognizedParameterType(String),
    MixedPrecedence,
    UnreadAssignment(String),
    OutputNeverAssigned(String),
    InconsistentDeclaration {
        name: String,
        ty1: Option<Ty>,
        ty2: Option<Ty>,
    },
    TypeIncompatible {
        source: Ty,
        target: Ty,
    },
    AssignIncompatible {
        name: String,
        source: Ty,
        target: Ty,
    },
    Unreachable,
    ConditionAlways(bool),
    NotProcedure(Ty),
    ProcedureNotCalled,
    TooFewArguments {
        next_arg: String,
    },
    TooManyArguments,
    CaseDefaultNotLast,
    UnknownFunction {
        name: String,
        suggestions: Vec<String>,
    },
    UnknownSpecialVariable {
        name: String,
        suggestions: Vec<String>,
    },
    InvalidMacroArgument(String),
    InvalidZStripAction {
        action: String,
        reason: &'static str,
    },
    NumberImplicitlyConvertedToString(String),
    DiscardedReturnValue(String),
    InvalidFNumberFormat {
        format: String,
        reason: String,
    },
    ReturnNothingForX {
        wanted: Ty,
    },
    ReturnXForNothing {
        actual: Ty,
    },
    QuitXInControl {
        actual: Ty,
    },
    MismatchArgumentAndArgumentMode {
        name: String,
        wanted: ArgMode,
        actual: ArgMode,
    },
    MaybeUndefined {
        ty: Ty,
        expr: Option<String>,
    },
    Undefined {
        expr: Option<String>,
    },
    NotInClass,
    ProcedureCallReturnsNothing,
    RequirePrivateVariable(String),
    Unimplemented(String),
    UndefinedMacro {
        name: String,
        suggestions: Vec<String>,
    },
    ContinueNotInLoop,
    DuplicatedMemberName {
        name: String,
    },
    InvalidSyntax {
        err: tygr::Error,
    },
    MissingQuit,
}

impl TypeErrorKind {
    pub fn severity(&self) -> DiagnosticSeverity {
        match self {
            TypeErrorKind::ReturnXForNothing { .. } => DiagnosticSeverity::Warning,
            TypeErrorKind::Unreachable => DiagnosticSeverity::Warning,
            TypeErrorKind::ConditionAlways(_) => DiagnosticSeverity::Warning,
            TypeErrorKind::MaybeUndefined { .. } => DiagnosticSeverity::Warning,
            TypeErrorKind::Undefined { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::Unimplemented(_) => DiagnosticSeverity::Information,
            TypeErrorKind::UndefinedMacro { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::InvalidSyntax { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::MismatchArgumentAndArgumentMode { .. } => DiagnosticSeverity::Warning,
            TypeErrorKind::TooFewArguments { .. } => DiagnosticSeverity::Warning,
            TypeErrorKind::CannotFindClass { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::CannotFindMember { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::CannotFindLabel { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::CannotFindRoutine(_) => DiagnosticSeverity::Error,
            TypeErrorKind::CannotFindInclude(_) => DiagnosticSeverity::Error,
            TypeErrorKind::MacroExpansionTooDeep(_) => DiagnosticSeverity::Error,
            TypeErrorKind::AccessInstanceOnlyMemberFromClassContext { .. } => {
                DiagnosticSeverity::Error
            }
            TypeErrorKind::RedeclareVariable(_) => DiagnosticSeverity::Warning,
            TypeErrorKind::RedundantNew(_) => DiagnosticSeverity::Warning,
            TypeErrorKind::NewInProcedureBlock(_) => DiagnosticSeverity::Warning,
            TypeErrorKind::UnrecognizedPropertyKeyword(_) => DiagnosticSeverity::Error,
            TypeErrorKind::UnrecognizedTypeParam { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::UnrecognizedParameterKeyword(_) => DiagnosticSeverity::Error,
            TypeErrorKind::UnrecognizedParameterType(_) => DiagnosticSeverity::Error,
            TypeErrorKind::InconsistentDeclaration { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::TypeIncompatible { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::AssignIncompatible { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::NotProcedure(_) => DiagnosticSeverity::Error,
            TypeErrorKind::ProcedureNotCalled => DiagnosticSeverity::Error,
            TypeErrorKind::TooManyArguments => DiagnosticSeverity::Error,
            TypeErrorKind::UnknownFunction { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::UnknownSpecialVariable { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::InvalidMacroArgument(_) => DiagnosticSeverity::Error,
            TypeErrorKind::CaseDefaultNotLast => DiagnosticSeverity::Error,
            TypeErrorKind::InvalidZStripAction { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::NumberImplicitlyConvertedToString(_) => DiagnosticSeverity::Warning,
            TypeErrorKind::DiscardedReturnValue(_) => DiagnosticSeverity::Warning,
            TypeErrorKind::InvalidFNumberFormat { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::ReturnNothingForX { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::QuitXInControl { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::NotInClass => DiagnosticSeverity::Error,
            TypeErrorKind::ProcedureCallReturnsNothing => DiagnosticSeverity::Error,
            TypeErrorKind::RequirePrivateVariable(_) => DiagnosticSeverity::Error,
            TypeErrorKind::ContinueNotInLoop => DiagnosticSeverity::Error,
            TypeErrorKind::DuplicatedMemberName { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::MixedPrecedence => DiagnosticSeverity::Warning,
            TypeErrorKind::UnreadAssignment(_) => DiagnosticSeverity::Warning,
            TypeErrorKind::OutputNeverAssigned(_) => DiagnosticSeverity::Warning,
            TypeErrorKind::MissingQuit => DiagnosticSeverity::Warning,
        }
    }
}

fn write_suggestions(f: &mut std::fmt::Formatter<'_>, suggestions: &[String]) -> std::fmt::Result {
    if suggestions.is_empty() {
        Ok(())
    } else {
        write!(f, "; did you mean: {}?", suggestions.join(", "))
    }
}

impl std::fmt::Display for TypeErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeErrorKind::CannotFindClass { cls } => {
                write!(
                    f,
                    "Cannot find class '{cls}' in the workspace or on the IRIS instance"
                )
            }
            TypeErrorKind::CannotFindMember {
                cls,
                mem,
                suggestions,
                has_get,
            } => {
                write!(f, "Cannot find member '{mem}' in {cls}")?;
                write_suggestions(f, suggestions)?;
                if *has_get {
                    write!(f, " (or, if it's a dynamic property, %Get(\"{mem}\"))")?;
                }
                Ok(())
            }
            TypeErrorKind::CannotFindLabel { label, routine } => match routine {
                Some(routine) => write!(f, "Cannot find label '{label}' in routine '{routine}'"),
                None => write!(f, "Cannot find label '{label}' in this routine"),
            },
            TypeErrorKind::CannotFindRoutine(routine) => {
                write!(
                    f,
                    "Routine '{routine}' is not defined in the current workspace"
                )
            }
            TypeErrorKind::CannotFindInclude(name) => {
                write!(
                    f,
                    "Include file '{name}' is not defined in the current workspace"
                )
            }
            TypeErrorKind::MacroExpansionTooDeep(name) => {
                write!(f, "The macro '{name}' expands recursively without end")
            }
            TypeErrorKind::AccessInstanceOnlyMemberFromClassContext { name } => {
                write!(
                    f,
                    "Cannot reference instance-only member '{name}' in a class method"
                )
            }
            TypeErrorKind::RedeclareVariable(name) => {
                write!(f, "Variable '{}' is already declared", name)
            }
            TypeErrorKind::RedundantNew(name) => {
                write!(
                    f,
                    "Variable '{}' is already NEW'd; this NEW resets it to undefined, discarding its current value — `Kill {0}` says that explicitly",
                    name
                )
            }
            TypeErrorKind::NewInProcedureBlock(name) => {
                write!(
                    f,
                    "'{}' is already NEW'd here — every local variable is implicitly NEW'd on entry to a procedure block",
                    name
                )
            }
            TypeErrorKind::UnrecognizedPropertyKeyword(keyword) => {
                write!(
                    f,
                    "'{keyword}' is not a recognized property keyword; available: {}",
                    PROPERTY_KEYWORDS.join(", ")
                )
            }
            TypeErrorKind::UnrecognizedTypeParam {
                name,
                class,
                is_property_keyword,
                suggestions,
            } => {
                write!(f, "'{name}' is not a parameter of {class}")?;
                if *is_property_keyword {
                    write!(
                        f,
                        " -- '{name}' is a property keyword, so it belongs in the `[ ... ]` list, not the type's own `( ... )` parameters"
                    )?;
                } else {
                    write_suggestions(f, suggestions)?;
                }
                Ok(())
            }
            TypeErrorKind::UnrecognizedParameterKeyword(keyword) => {
                write!(
                    f,
                    "'{keyword}' is not a recognized parameter keyword; available: {}",
                    PARAMETER_KEYWORDS.join(", ")
                )
            }
            TypeErrorKind::UnrecognizedParameterType(meta_type) => {
                write!(
                    f,
                    "'{meta_type}' is not a recognized parameter type; available: {}",
                    PARAMETER_TYPE_KEYWORDS.join(", ")
                )
            }
            TypeErrorKind::InconsistentDeclaration { name, ty1, ty2 } => {
                write!(f, "Variable '{}' is ", name)?;
                match ty1 {
                    Some(ty1) => write!(f, "declared as '{}'", ty1),
                    None => write!(f, "undeclared"),
                }?;
                write!(f, " in one branch but ")?;
                match ty2 {
                    Some(ty2) => write!(f, "declared as '{}'", ty2),
                    None => write!(f, "undeclared"),
                }?;
                write!(f, " in another")?;
                Ok(())
            }
            TypeErrorKind::TypeIncompatible { source, target } => {
                write!(f, "{} is incompatible with {}", source, target)
            }
            TypeErrorKind::AssignIncompatible {
                name,
                source,
                target,
            } => {
                write!(
                    f,
                    "Cannot assign {} to '{}' declared as {}",
                    source, name, target
                )
            }
            TypeErrorKind::Unreachable => {
                write!(f, "Unreachable code")
            }
            TypeErrorKind::ConditionAlways(true) => {
                write!(f, "This condition is always true")
            }
            TypeErrorKind::ConditionAlways(false) => {
                write!(f, "This condition is always false")
            }
            TypeErrorKind::NotProcedure(ty) => {
                write!(f, "Expression is not a procedure; its type is {ty}")
            }
            TypeErrorKind::ProcedureNotCalled => {
                write!(
                    f,
                    "ObjectScript has no first-class methods; call it with ()"
                )
            }
            TypeErrorKind::TooManyArguments => {
                write!(f, "Too many arguments")
            }
            TypeErrorKind::CaseDefaultNotLast => {
                write!(f, "$CASE's `:default` must be its last argument")
            }
            TypeErrorKind::UnknownFunction { name, suggestions }
            | TypeErrorKind::UnknownSpecialVariable { name, suggestions } => {
                let what = if matches!(self, TypeErrorKind::UnknownFunction { .. }) {
                    "function"
                } else {
                    "special variable"
                };
                write!(f, "${name} is not a recognized {what}")?;
                if !suggestions.is_empty() {
                    write!(f, "; did you mean: {}?", suggestions.join(", "))?;
                }
                Ok(())
            }
            TypeErrorKind::TooFewArguments { next_arg } => {
                write!(
                    f,
                    "Too few arguments: '{}' is required. Only arguments with default values and `Output` arguments are considered optional.",
                    next_arg
                )
            }
            TypeErrorKind::InvalidMacroArgument(msg) => {
                write!(f, "Invalid macro argument: {}", msg)
            }
            TypeErrorKind::ReturnNothingForX { wanted } => {
                write!(
                    f,
                    "Expected return value of type '{}' but got nothing",
                    wanted
                )
            }
            TypeErrorKind::ReturnXForNothing { actual } => {
                write!(f, "Unexpected return value of type '{}'", actual)
            }
            TypeErrorKind::QuitXInControl { actual } => {
                write!(f, "Quit with {} inside control structure", actual)
            }
            TypeErrorKind::MismatchArgumentAndArgumentMode {
                name,
                wanted,
                actual,
            } => {
                write!(
                    f,
                    "The parameter '{}' is declared as {} parameter but got {} argument",
                    name, wanted, actual
                )
            }
            TypeErrorKind::MaybeUndefined { ty, expr } => {
                write!(f, "This '{}' value may be undefined", ty)?;
                if let Some(expr) = expr {
                    write!(f, ": {}", expr)?;
                }
                Ok(())
            }
            TypeErrorKind::Undefined { expr } => {
                write!(f, "This value is undefined")?;
                if let Some(expr) = expr {
                    write!(f, ": {}", expr)?;
                }
                Ok(())
            }
            TypeErrorKind::NotInClass => write!(f, "Not in a class context"),
            TypeErrorKind::ProcedureCallReturnsNothing => {
                write!(f, "Procedure call returns nothing")
            }
            TypeErrorKind::RequirePrivateVariable(name) => {
                write!(
                    f,
                    "The procedure call requires the private variable '{}' to be public",
                    name
                )
            }
            TypeErrorKind::Unimplemented(msg) => {
                write!(f, "Type checking not implemented for '{}'", msg)
            }
            TypeErrorKind::UndefinedMacro { name, suggestions } => {
                write!(f, "The macro '{name}' is undefined")?;
                write_suggestions(f, suggestions)
            }
            TypeErrorKind::ContinueNotInLoop => {
                write!(f, "CONTINUE is not within a loop")
            }
            TypeErrorKind::DuplicatedMemberName { name } => {
                write!(f, "The member name {name} is defined twice")
            }
            TypeErrorKind::InvalidZStripAction { action, reason } => {
                write!(f, "'{action}' is not a valid $ZSTRIP action: {reason}")
            }
            TypeErrorKind::NumberImplicitlyConvertedToString(value) => {
                write!(
                    f,
                    "{value} is a number here, implicitly converted to the string \"{value}\""
                )
            }
            TypeErrorKind::DiscardedReturnValue(class) => {
                write!(
                    f,
                    "This call returns {class}, which Do discards -- consider Set to capture it, or checking it directly"
                )
            }
            TypeErrorKind::InvalidFNumberFormat { format, reason } => {
                write!(f, "'{format}' is not a valid $FNUMBER format: {reason}")
            }
            TypeErrorKind::InvalidSyntax { err: error } => {
                // Prefer a catch-all-first `Unknown*` variant's own friendly failure (see
                // `cos::Command`/`SpecialVariable`) over the generic "expected one of: ..." dump.
                // A function's arguments (`cos::fun_args!`) are named after it; when several of
                // its forms fail at the same spot, the first form's message stands for all.
                let mut named = std::collections::HashSet::new();
                let mut friendly: Vec<String> = error
                    .traces()
                    .iter()
                    .filter_map(|trace| match &trace.expectation {
                        tygr::Expectation::GrammarFrom { into, fail, .. }
                            if into.starts_with('$') =>
                        {
                            named.insert(into).then(|| format!("{into} {fail}"))
                        }
                        tygr::Expectation::GrammarFrom { fail, .. } => Some(fail.clone()),
                        _ => None,
                    })
                    .collect();
                friendly.dedup();
                if friendly.is_empty() {
                    write!(f, "Syntax error, looking for:\n{error}")
                } else {
                    write!(f, "{}", friendly.join("\n"))
                }
            }
            TypeErrorKind::MixedPrecedence => {
                write!(
                    f,
                    "ObjectScript has no operator precedence and evaluates strictly left-to-right; this may not compute what you expect"
                )
            }
            TypeErrorKind::UnreadAssignment(name) => {
                write!(f, "Variable '{}' is assigned but never read", name)
            }
            TypeErrorKind::OutputNeverAssigned(name) => {
                write!(
                    f,
                    "Output parameter '{}' is never assigned; its caller will get an undefined value",
                    name
                )
            }
            TypeErrorKind::MissingQuit => {
                write!(
                    f,
                    "Missing an explicit QUIT or RETURN; execution falls through to whatever follows"
                )
            }
        }
    }
}

#[derive(Debug)]
pub struct TypeError {
    pub kind: TypeErrorKind,
    pub range: Range,
    pub related_information: Vec<DiagnosticRelatedInformation>,
}

impl TypeError {
    fn new(
        kind: TypeErrorKind,
        range: Range,
        related_information: Vec<DiagnosticRelatedInformation>,
    ) -> Self {
        Self {
            kind,
            range,
            related_information,
        }
    }

    pub fn too_many_arguments(range: Range) -> Self {
        Self::new(TypeErrorKind::TooManyArguments, range, vec![])
    }

    // Built with a range directly, unlike `QuasiTypeError`'s deferred ones — there's no `Flow`
    // yet to attach one lazily this early (during macro expansion).
    pub fn cannot_find_include(name: String, range: Range) -> Self {
        Self::new(TypeErrorKind::CannotFindInclude(name), range, vec![])
    }

    pub fn macro_expansion_too_deep(name: String, range: Range) -> Self {
        Self::new(TypeErrorKind::MacroExpansionTooDeep(name), range, vec![])
    }

    pub fn mismatch_argument_mode(
        name: String,
        wanted: ArgMode,
        actual: ArgMode,
        range: Range,
        declared_at: Option<Location>,
    ) -> Self {
        let message = if wanted == ArgMode::Default && actual == ArgMode::ByRef {
            "Parameter declared here — add `&` (routine label) or `ByRef` (method) to declare it by-ref."
        } else {
            "Parameter declared here."
        };
        Self::new(
            TypeErrorKind::MismatchArgumentAndArgumentMode {
                name,
                wanted,
                actual,
            },
            range,
            declared_at
                .map(|location| {
                    vec![DiagnosticRelatedInformation {
                        location,
                        message: message.into(),
                    }]
                })
                .unwrap_or_default(),
        )
    }

    pub fn unimplemented(msg: String, range: Range) -> Self {
        Self::new(TypeErrorKind::Unimplemented(msg), range, vec![])
    }

    pub fn invalid_syntax(range: Range, err: tygr::Error) -> Self {
        Self::new(TypeErrorKind::InvalidSyntax { err }, range, vec![])
    }

    pub fn continue_not_in_loop(range: Range) -> TypeError {
        Self::new(TypeErrorKind::ContinueNotInLoop, range, vec![])
    }

    pub fn unread_assignment(name: String, range: Range) -> TypeError {
        Self::new(TypeErrorKind::UnreadAssignment(name), range, vec![])
    }

    pub fn output_never_assigned(name: String, range: Range) -> TypeError {
        Self::new(TypeErrorKind::OutputNeverAssigned(name), range, vec![])
    }

    pub fn duplicated_member_name(range: Range, name: String, previous: Location) -> Self {
        Self::new(
            TypeErrorKind::DuplicatedMemberName { name },
            range,
            vec![DiagnosticRelatedInformation {
                location: previous,
                message: "Previous definition".into(),
            }],
        )
    }

    pub fn missing_quit(range: Range) -> Self {
        Self::new(TypeErrorKind::MissingQuit, range, vec![])
    }

    pub fn redeclare_variable(name: String, range: Range, previous: Location) -> Self {
        Self::new(
            TypeErrorKind::RedeclareVariable(name),
            range,
            vec![DiagnosticRelatedInformation {
                location: previous,
                message: "Previous declaration".into(),
            }],
        )
    }

    pub fn redundant_new(name: String, range: Range, previous: Location) -> Self {
        Self::new(
            TypeErrorKind::RedundantNew(name),
            range,
            vec![DiagnosticRelatedInformation {
                location: previous,
                message: "Previous NEW".into(),
            }],
        )
    }

    pub fn new_in_procedure_block(name: String, range: Range) -> Self {
        Self::new(TypeErrorKind::NewInProcedureBlock(name), range, vec![])
    }

    pub fn unrecognized_property_keyword(keyword: String, range: Range) -> Self {
        Self::new(
            TypeErrorKind::UnrecognizedPropertyKeyword(keyword),
            range,
            vec![],
        )
    }

    pub fn unrecognized_type_param(
        name: String,
        class: String,
        is_property_keyword: bool,
        suggestions: Vec<String>,
        range: Range,
    ) -> Self {
        Self::new(
            TypeErrorKind::UnrecognizedTypeParam {
                name,
                class,
                is_property_keyword,
                suggestions,
            },
            range,
            vec![],
        )
    }

    pub fn unrecognized_parameter_keyword(keyword: String, range: Range) -> Self {
        Self::new(
            TypeErrorKind::UnrecognizedParameterKeyword(keyword),
            range,
            vec![],
        )
    }

    pub fn unrecognized_parameter_type(meta_type: String, range: Range) -> Self {
        Self::new(
            TypeErrorKind::UnrecognizedParameterType(meta_type),
            range,
            vec![],
        )
    }
}
