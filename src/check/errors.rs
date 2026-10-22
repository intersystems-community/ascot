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
        Self::WithoutRange(TypeErrorKind::Unimplemented { what: msg }, vec![])
    }

    pub fn undefined_macro(name: String, suggestions: Vec<String>) -> Self {
        Self::WithoutRange(
            TypeErrorKind::UndefinedMacro {
                name,
                suggestions: Suggestions(suggestions),
            },
            vec![],
        )
    }

    pub fn unreachable() -> Self {
        Self::WithoutRange(TypeErrorKind::Unreachable, vec![])
    }

    pub(crate) fn condition_always(value: bool) -> Self {
        Self::WithoutRange(TypeErrorKind::ConditionAlways { value }, vec![])
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
        Self::WithoutRange(TypeErrorKind::Undefined { expr: Detail(None) }, vec![])
    }

    pub fn maybe_undefined(ty: Ty) -> Self {
        Self::WithoutRange(
            TypeErrorKind::MaybeUndefined {
                ty,
                expr: Detail(None),
            },
            vec![],
        )
    }

    // Fills in `Undefined`/`MaybeUndefined`'s expression text — the only two kinds that benefit
    // from echoing the source back, since every other kind already names what's wrong. Called
    // once, by the outermost ranged expression wrapping the failure (see `CheckExpr` in
    // `check::mod`), so the text always matches whatever the diagnostic's range highlights.
    pub(crate) fn with_expr_text(self, text: impl FnOnce() -> String) -> Self {
        match self {
            Self::WithoutRange(
                TypeErrorKind::Undefined { expr: Detail(None) },
                related_information,
            ) => Self::WithoutRange(
                TypeErrorKind::Undefined {
                    expr: Detail(Some(text())),
                },
                related_information,
            ),
            Self::WithoutRange(
                TypeErrorKind::MaybeUndefined {
                    ty,
                    expr: Detail(None),
                },
                related_information,
            ) => Self::WithoutRange(
                TypeErrorKind::MaybeUndefined {
                    ty,
                    expr: Detail(Some(text())),
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
        Self::WithoutRange(TypeErrorKind::NotProcedure { ty }, vec![])
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
        Self::WithoutRange(TypeErrorKind::RequirePrivateVariable { name }, vec![])
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
        Self::WithoutRange(
            TypeErrorKind::UnknownFunction {
                name,
                suggestions: Suggestions(suggestions),
            },
            vec![],
        )
    }

    pub fn unknown_special_variable(name: String, suggestions: Vec<String>) -> Self {
        Self::WithoutRange(
            TypeErrorKind::UnknownSpecialVariable {
                name,
                suggestions: Suggestions(suggestions),
            },
            vec![],
        )
    }

    pub fn invalid_macro_argument(msg: impl Into<String>) -> Self {
        Self::WithoutRange(
            TypeErrorKind::InvalidMacroArgument { reason: msg.into() },
            vec![],
        )
    }

    pub fn invalid_zstrip_action(action: String, reason: &'static str) -> Self {
        Self::WithoutRange(
            TypeErrorKind::InvalidZStripAction { action, reason },
            vec![],
        )
    }

    pub fn number_implicitly_converted_to_string(value: String) -> Self {
        Self::WithoutRange(
            TypeErrorKind::NumberImplicitlyConvertedToString { value },
            vec![],
        )
    }

    pub fn discarded_return_value(class: String) -> Self {
        Self::WithoutRange(TypeErrorKind::DiscardedReturnValue { class }, vec![])
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
                dynamic: DynamicGet(has_get.then(|| mem.clone())),
                cls,
                mem,
                suggestions: Suggestions(suggestions),
            },
            vec![],
        )
    }

    pub fn cannot_find_class(cls: String) -> Self {
        Self::WithoutRange(TypeErrorKind::CannotFindClass { cls }, vec![])
    }

    pub fn cannot_find_label(label: String, routine: Option<String>) -> Self {
        Self::WithoutRange(
            TypeErrorKind::CannotFindLabel {
                label,
                routine: InRoutine(routine),
            },
            vec![],
        )
    }

    pub fn goto_out_of_block(label: String) -> Self {
        Self::WithoutRange(TypeErrorKind::GotoOutOfBlock { label }, vec![])
    }

    pub fn cannot_find_routine(routine: String) -> Self {
        Self::WithoutRange(TypeErrorKind::CannotFindRoutine { routine }, vec![])
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
            TypeErrorKind::InconsistentDeclaration {
                name,
                ty1: Declared(ty1),
                ty2: Declared(ty2),
            },
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

// Declares `TypeErrorKind` with each variant's diagnostic code next to it, so the code, the
// variant's docs.rs anchor and the code shown in its docs all come from one place.
// Each variant's `#[message]` is its message, with its fields in `{braces}`; its doc comment, if
// any, explains it.
macro_rules! type_error_kinds {
    ($(
        $(#[doc = $doc:literal])*
        #[code = $code:literal]
        #[message = $message:literal]
        $variant:ident
        $({ $( $(#[doc = $fdoc:literal])* $field:ident: $fty:ty ),* $(,)? })?,
    )*) => {
        /// Every kind of diagnostic ascot reports.
        ///
        /// Each has a code, e.g. `A1234`, that clients can filter or key quick fixes on. A code is
        /// fixed once assigned and never reused, whatever later happens to the variant's name or
        /// message; it is random, so it says nothing about the kind.
        ///
        /// Each kind's first line is its message, with its fields in `{braces}`.
        #[derive(Debug)]
        pub enum TypeErrorKind {
            $(
                #[doc = $message]
                #[doc = ""]
                $(#[doc = $doc])*
                #[doc = ""]
                #[doc = concat!("Code: `", $code, "`")]
                #[doc(alias = $code)]
                $variant $({ $( $(#[doc = $fdoc])* $field: $fty ),* })?,
            )*
        }

        impl std::fmt::Display for TypeErrorKind {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $(Self::$variant $({ $($field),* })? => {
                        write!(f, $message $($(, $field = $field)*)?)
                    })*
                }
            }
        }

        impl TypeErrorKind {
            /// This kind's diagnostic code.
            pub fn code(&self) -> &'static str {
                match self {
                    $(Self::$variant { .. } => $code,)*
                }
            }

            /// This kind's documentation on docs.rs, for the running version.
            pub fn code_description(&self) -> String {
                let variant = match self {
                    $(Self::$variant { .. } => stringify!($variant),)*
                };
                format!(
                    "https://docs.rs/ascot/{}/ascot/check/enum.TypeErrorKind.html#variant.{variant}",
                    env!("CARGO_PKG_VERSION")
                )
            }
        }

        #[cfg(test)]
        const CODES: &[&str] = &[$($code),*];
    };
}

type_error_kinds! {
    #[code = "A8061"]
    #[message = "Cannot find class '{cls}' in the workspace or on the IRIS instance"]
    CannotFindClass { cls: String },
    /// A class that isn't open in the workspace. With an IRIS connection, the instance doesn't
    /// have it either; without one, there's nothing to check it against.
    #[code = "A1054"]
    #[message = "Cannot {lookup}"]
    UnconfirmedClass { lookup: ClassLookup },
    #[code = "A3139"]
    #[message = "Cannot find member '{mem}' in {cls}{suggestions}{dynamic}"]
    CannotFindMember {
        cls: String,
        mem: String,
        suggestions: Suggestions,
        dynamic: DynamicGet,
    },
    #[code = "A9360"]
    #[message = "Cannot confirm '{mem}' exists on {on}{cls} — its class hierarchy includes a class {reason}"]
    UnconfirmedMember {
        cls: String,
        mem: String,
        on: SuperclassOf,
        reason: UnconfirmedReason,
    },
    #[code = "A6851"]
    #[message = "Cannot find label '{label}' in {routine}"]
    CannotFindLabel { label: String, routine: InRoutine },
    #[code = "A1073"]
    #[message = "Cannot GOTO label '{label}' from inside a procedure block: it is outside the block"]
    GotoOutOfBlock { label: String },
    #[code = "A3303"]
    #[message = "Routine '{routine}' is not defined in the current workspace"]
    CannotFindRoutine { routine: String },
    #[code = "A7831"]
    #[message = "Include file '{name}' is not defined in the current workspace"]
    CannotFindInclude { name: String },
    #[code = "A5090"]
    #[message = "The macro '{name}' expands recursively without end"]
    MacroExpansionTooDeep { name: String },
    #[code = "A8914"]
    #[message = "Cannot reference instance-only member '{name}' in a class method"]
    AccessInstanceOnlyMemberFromClassContext { name: String },
    #[code = "A4983"]
    #[message = "Variable '{name}' is already declared"]
    RedeclareVariable { name: String },
    #[code = "A1817"]
    #[message = "Variable '{name}' is already NEW'd; this NEW resets it to undefined, discarding its current value — `Kill {name}` says that explicitly"]
    RedundantNew { name: String },
    #[code = "A6866"]
    #[message = "'{name}' is already NEW'd here — every local variable is implicitly NEW'd on entry to a procedure block"]
    NewInProcedureBlock { name: String },
    #[code = "A5573"]
    #[message = "Class keyword '{keyword}' has no value"]
    ClassKeywordWithoutValue { keyword: String },
    #[code = "A9322"]
    #[message = "'{keyword}' is not a recognized property keyword; available: {available}"]
    UnrecognizedPropertyKeyword { keyword: String, available: Available },
    #[code = "A2357"]
    #[message = "'{name}' is not a parameter of {class}{hint}"]
    UnrecognizedTypeParam {
        name: String,
        class: String,
        hint: TypeParamHint,
    },
    #[code = "A2174"]
    #[message = "'{keyword}' is not a recognized parameter keyword; available: {available}"]
    UnrecognizedParameterKeyword { keyword: String, available: Available },
    #[code = "A8317"]
    #[message = "'{meta_type}' is not a recognized parameter type; available: {available}"]
    UnrecognizedParameterType { meta_type: String, available: Available },
    #[code = "A7155"]
    #[message = "ObjectScript has no operator precedence and evaluates strictly left-to-right; this may not compute what you expect"]
    MixedPrecedence,
    #[code = "A5718"]
    #[message = "Variable '{name}' is assigned but never read"]
    UnreadAssignment { name: String },
    #[code = "A4476"]
    #[message = "Output parameter '{name}' is never assigned; its caller will get an undefined value"]
    OutputNeverAssigned { name: String },
    #[code = "A8892"]
    #[message = "Variable '{name}' is {ty1} in one branch but {ty2} in another"]
    InconsistentDeclaration {
        name: String,
        ty1: Declared,
        ty2: Declared,
    },
    #[code = "A5967"]
    #[message = "{source} is incompatible with {target}"]
    TypeIncompatible { source: Ty, target: Ty },
    #[code = "A9180"]
    #[message = "Cannot assign {source} to '{name}' declared as {target}"]
    AssignIncompatible {
        name: String,
        source: Ty,
        target: Ty,
    },
    #[code = "A1203"]
    #[message = "Unreachable code"]
    Unreachable,
    #[code = "A8456"]
    #[message = "This condition is always {value}"]
    ConditionAlways { value: bool },
    #[code = "A8496"]
    #[message = "Expression is not a procedure; its type is {ty}"]
    NotProcedure { ty: Ty },
    #[code = "A3671"]
    #[message = "ObjectScript has no first-class methods; call it with ()"]
    ProcedureNotCalled,
    #[code = "A6705"]
    #[message = "Too few arguments: '{next_arg}' is required. Only arguments with default values and `Output` arguments are considered optional."]
    TooFewArguments { next_arg: String },
    #[code = "A5523"]
    #[message = "Too many arguments"]
    TooManyArguments,
    #[code = "A7139"]
    #[message = "$CASE's `:default` must be its last argument"]
    CaseDefaultNotLast,
    #[code = "A3850"]
    #[message = "${name} is not a recognized function{suggestions}"]
    UnknownFunction {
        name: String,
        suggestions: Suggestions,
    },
    #[code = "A9399"]
    #[message = "${name} is not a recognized special variable{suggestions}"]
    UnknownSpecialVariable {
        name: String,
        suggestions: Suggestions,
    },
    #[code = "A6587"]
    #[message = "Invalid macro argument: {reason}"]
    InvalidMacroArgument { reason: String },
    #[code = "A3286"]
    #[message = "'{action}' is not a valid $ZSTRIP action: {reason}"]
    InvalidZStripAction {
        action: String,
        reason: &'static str,
    },
    #[code = "A9421"]
    #[message = "{value} is a number here, implicitly converted to the string \"{value}\""]
    NumberImplicitlyConvertedToString { value: String },
    #[code = "A4556"]
    #[message = "This call returns {class}, which Do discards -- consider Set to capture it, or checking it directly"]
    DiscardedReturnValue { class: String },
    #[code = "A2839"]
    #[message = "{what} `{found}` should be written `{expected}`"]
    KeywordStyle {
        // "Command keyword" or "Function".
        what: &'static str,
        found: String,
        expected: String,
    },
    #[code = "A7895"]
    #[message = "'{format}' is not a valid $FNUMBER format: {reason}"]
    InvalidFNumberFormat { format: String, reason: String },
    #[code = "A6781"]
    #[message = "Expected return value of type '{wanted}' but got nothing"]
    ReturnNothingForX { wanted: Ty },
    #[code = "A8997"]
    #[message = "Unexpected return value of type '{actual}'"]
    ReturnXForNothing { actual: Ty },
    #[code = "A4963"]
    #[message = "Quit with {actual} inside control structure"]
    QuitXInControl { actual: Ty },
    #[code = "A5579"]
    #[message = "The parameter '{name}' is declared as {wanted} parameter but got {actual} argument"]
    MismatchArgumentAndArgumentMode {
        name: String,
        wanted: ArgMode,
        actual: ArgMode,
    },
    #[code = "A6826"]
    #[message = "This '{ty}' value may be undefined{expr}"]
    MaybeUndefined { ty: Ty, expr: Detail },
    #[code = "A7095"]
    #[message = "This value is undefined{expr}"]
    Undefined { expr: Detail },
    #[code = "A1368"]
    #[message = "Not in a class context"]
    NotInClass,
    #[code = "A7244"]
    #[message = "Procedure call returns nothing"]
    ProcedureCallReturnsNothing,
    #[code = "A4025"]
    #[message = "The procedure call requires the private variable '{name}' to be public"]
    RequirePrivateVariable { name: String },
    #[code = "A4043"]
    #[message = "Type checking not implemented for '{what}'"]
    Unimplemented { what: String },
    #[code = "A2797"]
    #[message = "The macro '{name}' is undefined{suggestions}"]
    UndefinedMacro {
        name: String,
        suggestions: Suggestions,
    },
    #[code = "A5789"]
    #[message = "CONTINUE is not within a loop"]
    ContinueNotInLoop,
    #[code = "A3119"]
    #[message = "The member name {name} is defined twice"]
    DuplicatedMemberName { name: String },
    #[code = "A3717"]
    #[message = "Class '{name}' is also declared in {other_uri}"]
    DuplicateClassName { name: String, other_uri: String },
    /// Code that doesn't parse. Where the grammar knows what went wrong, such as an unknown
    /// command or a function given the wrong arguments, the message says so; otherwise it lists
    /// what the parser was looking for.
    #[code = "A8073"]
    #[message = "{err}"]
    InvalidSyntax { err: SyntaxErrorText },
    #[code = "A1727"]
    #[message = "Syntax error, looking for:\n{err}"]
    DocumentSyntaxError { err: tygr::Error },
    #[code = "A7548"]
    #[message = "Missing an explicit QUIT or RETURN; execution falls through to whatever follows"]
    MissingQuit,
}

impl TypeErrorKind {
    pub fn severity(&self) -> DiagnosticSeverity {
        match self {
            TypeErrorKind::ReturnXForNothing { .. } => DiagnosticSeverity::Warning,
            TypeErrorKind::Unreachable => DiagnosticSeverity::Warning,
            TypeErrorKind::ConditionAlways { .. } => DiagnosticSeverity::Warning,
            TypeErrorKind::MaybeUndefined { .. } => DiagnosticSeverity::Warning,
            TypeErrorKind::Undefined { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::Unimplemented { .. } => DiagnosticSeverity::Information,
            TypeErrorKind::UndefinedMacro { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::InvalidSyntax { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::MismatchArgumentAndArgumentMode { .. } => DiagnosticSeverity::Warning,
            TypeErrorKind::TooFewArguments { .. } => DiagnosticSeverity::Warning,
            TypeErrorKind::CannotFindClass { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::CannotFindMember { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::CannotFindLabel { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::GotoOutOfBlock { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::CannotFindRoutine { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::CannotFindInclude { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::MacroExpansionTooDeep { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::AccessInstanceOnlyMemberFromClassContext { .. } => {
                DiagnosticSeverity::Error
            }
            TypeErrorKind::RedeclareVariable { .. } => DiagnosticSeverity::Warning,
            TypeErrorKind::RedundantNew { .. } => DiagnosticSeverity::Warning,
            TypeErrorKind::NewInProcedureBlock { .. } => DiagnosticSeverity::Warning,
            TypeErrorKind::UnrecognizedPropertyKeyword { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::UnrecognizedTypeParam { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::UnrecognizedParameterKeyword { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::UnrecognizedParameterType { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::InconsistentDeclaration { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::TypeIncompatible { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::AssignIncompatible { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::NotProcedure { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::ProcedureNotCalled => DiagnosticSeverity::Error,
            TypeErrorKind::TooManyArguments => DiagnosticSeverity::Error,
            TypeErrorKind::UnknownFunction { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::UnknownSpecialVariable { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::InvalidMacroArgument { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::CaseDefaultNotLast => DiagnosticSeverity::Error,
            TypeErrorKind::InvalidZStripAction { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::NumberImplicitlyConvertedToString { .. } => DiagnosticSeverity::Warning,
            TypeErrorKind::DiscardedReturnValue { .. } => DiagnosticSeverity::Warning,
            TypeErrorKind::KeywordStyle { .. } => DiagnosticSeverity::Warning,
            TypeErrorKind::InvalidFNumberFormat { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::ReturnNothingForX { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::QuitXInControl { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::NotInClass => DiagnosticSeverity::Error,
            TypeErrorKind::ProcedureCallReturnsNothing => DiagnosticSeverity::Error,
            TypeErrorKind::RequirePrivateVariable { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::ContinueNotInLoop => DiagnosticSeverity::Error,
            TypeErrorKind::DuplicatedMemberName { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::MixedPrecedence => DiagnosticSeverity::Warning,
            TypeErrorKind::UnreadAssignment { .. } => DiagnosticSeverity::Warning,
            TypeErrorKind::OutputNeverAssigned { .. } => DiagnosticSeverity::Warning,
            TypeErrorKind::MissingQuit => DiagnosticSeverity::Warning,
            TypeErrorKind::UnconfirmedClass { .. } => DiagnosticSeverity::Hint,
            TypeErrorKind::UnconfirmedMember { .. } => DiagnosticSeverity::Hint,
            TypeErrorKind::ClassKeywordWithoutValue { .. } => DiagnosticSeverity::Warning,
            TypeErrorKind::DuplicateClassName { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::DocumentSyntaxError { .. } => DiagnosticSeverity::Warning,
        }
    }
}

// The conditional parts of messages, each rendering nothing or a fragment of its kind's message.

/// "; did you mean: …?", or nothing without suggestions.
#[derive(Debug)]
pub struct Suggestions(pub Vec<String>);

impl std::fmt::Display for Suggestions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.is_empty() {
            Ok(())
        } else {
            write!(f, "; did you mean: {}?", self.0.join(", "))
        }
    }
}

/// The names a keyword could have been, comma-separated.
#[derive(Debug)]
pub struct Available(pub &'static [&'static str]);

impl std::fmt::Display for Available {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.join(", "))
    }
}

/// What an unconfirmed class lookup couldn't do, depending on whether IRIS was asked.
#[derive(Debug)]
pub struct ClassLookup {
    pub cls: String,
    pub has_iris: bool,
}

impl std::fmt::Display for ClassLookup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cls = &self.cls;
        if self.has_iris {
            write!(
                f,
                "find class '{cls}' in the workspace or on the IRIS instance"
            )
        } else {
            write!(
                f,
                "confirm class '{cls}' exists — it isn't open in this workspace, with no IRIS connection to check it against"
            )
        }
    }
}

/// `%Get`'s suggestion for a member, if the class has dynamic properties.
#[derive(Debug)]
pub struct DynamicGet(pub Option<String>);

impl std::fmt::Display for DynamicGet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(mem) => write!(f, " (or, if it's a dynamic property, %Get(\"{mem}\"))"),
            None => Ok(()),
        }
    }
}

/// "a superclass of ", for a member found missing on a superclass.
#[derive(Debug)]
pub struct SuperclassOf(pub bool);

impl std::fmt::Display for SuperclassOf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0 { "a superclass of " } else { "" })
    }
}

/// Why a class in a hierarchy can't be checked, depending on whether IRIS was asked.
#[derive(Debug)]
pub struct UnconfirmedReason {
    pub has_iris: bool,
}

impl std::fmt::Display for UnconfirmedReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.has_iris {
            "that isn't on the IRIS instance"
        } else {
            "outside this workspace, with no IRIS connection to check it against"
        })
    }
}

/// "routine '…'", or "this routine" for a label looked up locally.
#[derive(Debug)]
pub struct InRoutine(pub Option<String>);

impl std::fmt::Display for InRoutine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(routine) => write!(f, "routine '{routine}'"),
            None => f.write_str("this routine"),
        }
    }
}

/// What to say after an unrecognized type parameter.
#[derive(Debug)]
pub enum TypeParamHint {
    // The name is a property keyword, misplaced in the type's parameters.
    PropertyKeyword(String),
    Suggestions(Suggestions),
}

impl std::fmt::Display for TypeParamHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeParamHint::PropertyKeyword(name) => write!(
                f,
                " -- '{name}' is a property keyword, so it belongs in the `[ ... ]` list, not the type's own `( ... )` parameters"
            ),
            TypeParamHint::Suggestions(suggestions) => suggestions.fmt(f),
        }
    }
}

/// "declared as '…'", or "undeclared".
#[derive(Debug)]
pub struct Declared(pub Option<Ty>);

impl std::fmt::Display for Declared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(ty) => write!(f, "declared as '{ty}'"),
            None => f.write_str("undeclared"),
        }
    }
}

/// ": …", the source text an undefined value came from, if known.
#[derive(Debug)]
pub struct Detail(pub Option<String>);

impl std::fmt::Display for Detail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(text) => write!(f, ": {text}"),
            None => Ok(()),
        }
    }
}

/// A syntax error, in the grammar's own words where it has some.
#[derive(Debug)]
pub struct SyntaxErrorText(pub tygr::Error);

impl std::fmt::Display for SyntaxErrorText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let error = &self.0;
        // Prefer a catch-all-first `Unknown*` variant's own friendly failure (see
        // `cos::Command`/`SpecialVariable`) over the generic "expected one of: ..." dump.
        // A function's arguments (`cos::fun_args!`) are named after it; when several of
        // its forms fail at the same spot, the first form's message stands for all.
        let mut named = std::collections::HashSet::new();
        let mut friendly: Vec<String> = error
            .traces()
            .iter()
            .filter_map(|trace| match &trace.expectation {
                tygr::Expectation::GrammarFrom { into, fail, .. } if into.starts_with('$') => {
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
    pub(crate) fn unconfirmed_class(cls: String, has_iris: bool, range: Range) -> Self {
        Self::new(
            TypeErrorKind::UnconfirmedClass {
                lookup: ClassLookup { cls, has_iris },
            },
            range,
            vec![],
        )
    }

    pub(crate) fn unconfirmed_member(
        cls: String,
        mem: String,
        superclass: bool,
        has_iris: bool,
        range: Range,
    ) -> Self {
        let kind = TypeErrorKind::UnconfirmedMember {
            cls,
            mem,
            on: SuperclassOf(superclass),
            reason: UnconfirmedReason { has_iris },
        };
        Self::new(kind, range, vec![])
    }

    pub(crate) fn class_keyword_without_value(keyword: String, range: Range) -> Self {
        Self::new(
            TypeErrorKind::ClassKeywordWithoutValue { keyword },
            range,
            vec![],
        )
    }

    pub(crate) fn duplicate_class_name(range: Range, name: String, other: Location) -> Self {
        let kind = TypeErrorKind::DuplicateClassName {
            name,
            other_uri: other.uri.clone(),
        };
        let related = DiagnosticRelatedInformation {
            location: other,
            message: "Also declared here".into(),
        };
        Self::new(kind, range, vec![related])
    }

    pub(crate) fn document_syntax_error(range: Range, err: tygr::Error) -> Self {
        Self::new(TypeErrorKind::DocumentSyntaxError { err }, range, vec![])
    }

    pub fn cannot_find_include(name: String, range: Range) -> Self {
        Self::new(TypeErrorKind::CannotFindInclude { name }, range, vec![])
    }

    pub fn macro_expansion_too_deep(name: String, range: Range) -> Self {
        Self::new(TypeErrorKind::MacroExpansionTooDeep { name }, range, vec![])
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
        Self::new(TypeErrorKind::Unimplemented { what: msg }, range, vec![])
    }

    pub fn invalid_syntax(range: Range, err: tygr::Error) -> Self {
        Self::new(
            TypeErrorKind::InvalidSyntax {
                err: SyntaxErrorText(err),
            },
            range,
            vec![],
        )
    }

    pub fn continue_not_in_loop(range: Range) -> TypeError {
        Self::new(TypeErrorKind::ContinueNotInLoop, range, vec![])
    }

    pub fn unread_assignment(name: String, range: Range) -> TypeError {
        Self::new(TypeErrorKind::UnreadAssignment { name }, range, vec![])
    }

    pub fn output_never_assigned(name: String, range: Range) -> TypeError {
        Self::new(TypeErrorKind::OutputNeverAssigned { name }, range, vec![])
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
            TypeErrorKind::RedeclareVariable { name },
            range,
            vec![DiagnosticRelatedInformation {
                location: previous,
                message: "Previous declaration".into(),
            }],
        )
    }

    pub fn keyword_style(
        what: &'static str,
        found: String,
        expected: String,
        range: Range,
    ) -> Self {
        Self::new(
            TypeErrorKind::KeywordStyle {
                what,
                found,
                expected,
            },
            range,
            vec![],
        )
    }

    pub fn redundant_new(name: String, range: Range, previous: Location) -> Self {
        Self::new(
            TypeErrorKind::RedundantNew { name },
            range,
            vec![DiagnosticRelatedInformation {
                location: previous,
                message: "Previous NEW".into(),
            }],
        )
    }

    pub fn new_in_procedure_block(name: String, range: Range) -> Self {
        Self::new(TypeErrorKind::NewInProcedureBlock { name }, range, vec![])
    }

    pub fn unrecognized_property_keyword(keyword: String, range: Range) -> Self {
        Self::new(
            TypeErrorKind::UnrecognizedPropertyKeyword {
                keyword,
                available: Available(PROPERTY_KEYWORDS),
            },
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
                hint: if is_property_keyword {
                    TypeParamHint::PropertyKeyword(name.clone())
                } else {
                    TypeParamHint::Suggestions(Suggestions(suggestions))
                },
                name,
                class,
            },
            range,
            vec![],
        )
    }

    pub fn unrecognized_parameter_keyword(keyword: String, range: Range) -> Self {
        Self::new(
            TypeErrorKind::UnrecognizedParameterKeyword {
                keyword,
                available: Available(PARAMETER_KEYWORDS),
            },
            range,
            vec![],
        )
    }

    pub fn unrecognized_parameter_type(meta_type: String, range: Range) -> Self {
        Self::new(
            TypeErrorKind::UnrecognizedParameterType {
                meta_type,
                available: Available(PARAMETER_TYPE_KEYWORDS),
            },
            range,
            vec![],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_distinct_and_well_formed() {
        let distinct: std::collections::HashSet<_> = CODES.iter().collect();
        assert_eq!(distinct.len(), CODES.len(), "a code is used twice");
        for code in CODES {
            let digits = code.strip_prefix('A').unwrap_or_else(|| panic!("{code}"));
            assert!(
                digits.len() == 4 && digits.bytes().all(|b| b.is_ascii_digit()),
                "{code}"
            );
        }
    }
}
