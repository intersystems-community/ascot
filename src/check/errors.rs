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
        TypeErrorKind::Unimplemented { what: msg }.into()
    }

    pub fn undefined_macro(name: String, suggestions: Vec<String>) -> Self {
        TypeErrorKind::UndefinedMacro {
            name,
            suggestions: Suggestions(suggestions),
        }
        .into()
    }

    pub fn unreachable() -> Self {
        TypeErrorKind::Unreachable.into()
    }

    pub(crate) fn condition_always(value: bool) -> Self {
        TypeErrorKind::ConditionAlways { value }.into()
    }

    pub(crate) fn return_nothing_for_x(wanted: Ty, declared_at: Location) -> Self {
        Self::WithoutRange(
            TypeErrorKind::ReturnNothingForX { wanted },
            related(declared_at, "A return type is declared."),
        )
    }

    pub(crate) fn return_x_for_nothing(actual: Ty, declared_at: Location) -> Self {
        Self::WithoutRange(
            TypeErrorKind::ReturnXForNothing { actual },
            related(declared_at, "No return type is declared."),
        )
    }

    pub(crate) fn quit_x_in_control(actual: Ty, location_of_control: Location) -> Self {
        Self::WithoutRange(
            TypeErrorKind::QuitXInControl { actual },
            related(location_of_control, "The control operation."),
        )
    }

    pub fn undefined() -> Self {
        TypeErrorKind::Undefined { expr: Detail(None) }.into()
    }

    pub fn maybe_undefined(ty: Ty) -> Self {
        TypeErrorKind::MaybeUndefined {
            ty,
            expr: Detail(None),
        }
        .into()
    }

    // Fills in `Undefined`/`MaybeUndefined`'s expression text — the only two kinds that benefit
    // from echoing the source back, since every other kind already names what's wrong. Called
    // once, by the outermost ranged expression wrapping the failure (see `CheckExpr` in
    // `check::mod`), so the text always matches whatever the diagnostic's range highlights.
    pub(crate) fn with_expr_text(mut self, text: impl FnOnce() -> String) -> Self {
        if let Self::WithoutRange(
            TypeErrorKind::Undefined { expr } | TypeErrorKind::MaybeUndefined { expr, .. },
            _,
        ) = &mut self
            && expr.0.is_none()
        {
            expr.0 = Some(text());
        }
        self
    }

    pub fn type_incompatible(source: Ty, target: Ty) -> Self {
        TypeErrorKind::TypeIncompatible { source, target }.into()
    }

    pub fn assign_incompatible(name: String, source: Ty, target: Ty) -> Self {
        TypeErrorKind::AssignIncompatible {
            name,
            source,
            target,
        }
        .into()
    }

    pub fn access_instance_only_member_from_class_context(name: String) -> Self {
        TypeErrorKind::AccessInstanceOnlyMemberFromClassContext { name }.into()
    }

    pub fn node_without_subscripts(name: String) -> Self {
        TypeErrorKind::NodeWithoutSubscripts { name }.into()
    }

    pub fn undocumented_argument(function: String, documented: usize, given: usize) -> Self {
        TypeErrorKind::UndocumentedArgument {
            function,
            documented,
            given,
        }
        .into()
    }

    pub fn not_multidimensional(name: String) -> Self {
        TypeErrorKind::NotMultidimensional { name }.into()
    }

    pub fn parameter_is_constant(name: String) -> Self {
        TypeErrorKind::ParameterIsConstant { name }.into()
    }

    pub fn not_in_class() -> Self {
        TypeErrorKind::NotInClass.into()
    }

    pub fn require_private_variable(name: String) -> Self {
        TypeErrorKind::RequirePrivateVariable { name }.into()
    }

    pub fn too_few_arguments(next_arg: String, declared_at: Option<Location>) -> Self {
        Self::WithoutRange(
            TypeErrorKind::TooFewArguments { next_arg },
            declared_at
                .map(|location| {
                    related(
                        location,
                        "Parameter declared here — add a default value to make it optional.",
                    )
                })
                .unwrap_or_default(),
        )
    }

    pub fn too_many_arguments() -> Self {
        TypeErrorKind::TooManyArguments.into()
    }

    pub fn case_default_not_last() -> Self {
        TypeErrorKind::CaseDefaultNotLast.into()
    }

    pub fn unknown_function(name: String, suggestions: Vec<String>) -> Self {
        TypeErrorKind::UnknownFunction {
            name,
            suggestions: Suggestions(suggestions),
        }
        .into()
    }

    pub fn unknown_special_variable(name: String, suggestions: Vec<String>) -> Self {
        TypeErrorKind::UnknownSpecialVariable {
            name,
            suggestions: Suggestions(suggestions),
        }
        .into()
    }

    pub fn invalid_macro_argument(msg: impl Into<String>) -> Self {
        TypeErrorKind::InvalidMacroArgument { reason: msg.into() }.into()
    }

    pub fn invalid_zstrip_action(action: String, reason: &'static str) -> Self {
        TypeErrorKind::InvalidZStripAction { action, reason }.into()
    }

    pub fn number_implicitly_converted_to_string(value: String) -> Self {
        TypeErrorKind::NumberImplicitlyConvertedToString { value }.into()
    }

    pub fn discarded_return_value(class: String) -> Self {
        TypeErrorKind::DiscardedReturnValue { class }.into()
    }

    pub fn invalid_fnumber_format(format: String, reason: String) -> Self {
        TypeErrorKind::InvalidFNumberFormat { format, reason }.into()
    }

    pub fn cannot_find_member(
        cls: String,
        mem: String,
        suggestions: Vec<String>,
        has_get: bool,
    ) -> Self {
        TypeErrorKind::CannotFindMember {
            dynamic: DynamicGet(has_get.then(|| mem.clone())),
            cls,
            mem,
            suggestions: Suggestions(suggestions),
        }
        .into()
    }

    pub fn cannot_find_class(cls: String) -> Self {
        TypeErrorKind::CannotFindClass { cls }.into()
    }

    pub fn super_without_method(method: String, cls: String) -> Self {
        TypeErrorKind::SuperWithoutMethod { method, cls }.into()
    }

    pub fn cannot_find_label(label: String, routine: Option<String>) -> Self {
        TypeErrorKind::CannotFindLabel {
            label,
            routine: InRoutine(routine),
        }
        .into()
    }

    pub fn goto_out_of_block(label: String) -> Self {
        TypeErrorKind::GotoOutOfBlock { label }.into()
    }

    pub fn cannot_find_routine(routine: String) -> Self {
        TypeErrorKind::CannotFindRoutine { routine }.into()
    }

    pub fn routine_not_on_iris(routine: String) -> Self {
        TypeErrorKind::RoutineNotOnIris { routine }.into()
    }

    pub fn private_variable_not_allowed(function: String, name: String) -> Self {
        TypeErrorKind::PrivateVariableNotAllowed { function, name }.into()
    }

    pub fn by_ref_argument_not_allowed(function: String) -> Self {
        TypeErrorKind::ByRefArgumentNotAllowed { function }.into()
    }

    pub fn omitted_argument(function: String, position: usize) -> Self {
        TypeErrorKind::OmittedArgument { function, position }.into()
    }

    pub fn set_vector_without_type() -> Self {
        TypeErrorKind::SetVectorWithoutType.into()
    }

    pub fn routine_without_source(routine: String) -> Self {
        TypeErrorKind::RoutineWithoutSource { routine }.into()
    }

    pub fn iris_lookup_failed(kind: crate::DocKind, name: String, reason: String) -> Self {
        TypeErrorKind::IrisLookupFailed {
            lookup: IrisLookup { kind, name },
            reason,
        }
        .into()
    }

    pub fn continue_not_in_loop() -> Self {
        TypeErrorKind::ContinueNotInLoop.into()
    }

    pub(crate) fn inconsistent_declaration(
        name: String,
        ty1: Option<Ty>,
        ty2: Option<Ty>,
    ) -> QuasiTypeError {
        TypeErrorKind::InconsistentDeclaration {
            name,
            ty1: Declared(ty1),
            ty2: Declared(ty2),
        }
        .into()
    }

    pub(crate) fn mixed_precedence() -> QuasiTypeError {
        TypeErrorKind::MixedPrecedence.into()
    }
}

impl From<TypeErrorKind> for QuasiTypeError {
    fn from(kind: TypeErrorKind) -> Self {
        Self::WithoutRange(kind, vec![])
    }
}

impl From<TypeError> for QuasiTypeError {
    fn from(err: TypeError) -> Self {
        Self::WithRange(err)
    }
}

fn related(location: Location, message: &str) -> Vec<DiagnosticRelatedInformation> {
    vec![DiagnosticRelatedInformation {
        location,
        message: message.into(),
    }]
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
// any, explains it. `#[severity]` names a `DiagnosticSeverity` variant.
macro_rules! type_error_kinds {
    ($(
        $(#[doc = $doc:literal])*
        #[code = $code:literal]
        #[severity = $severity:ident]
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
                #[doc = concat!("Code: `", $code, "`, severity: ", stringify!($severity))]
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

            pub fn severity(&self) -> DiagnosticSeverity {
                match self {
                    $(Self::$variant { .. } => DiagnosticSeverity::$severity,)*
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
    #[severity = Error]
    #[message = "Cannot find class '{cls}' in the workspace or on the IRIS instance"]
    CannotFindClass { cls: String },
    /// A class that isn't open in the workspace. With an IRIS connection, the instance doesn't
    /// have it either; without one, there's nothing to check it against.
    #[code = "A1054"]
    #[severity = Hint]
    #[message = "Cannot {lookup}"]
    UnconfirmedClass { lookup: ClassLookup },
    #[code = "A3139"]
    #[severity = Error]
    #[message = "Cannot find member '{mem}' in {cls}{suggestions}{dynamic}"]
    CannotFindMember {
        cls: String,
        mem: String,
        suggestions: Suggestions,
        dynamic: DynamicGet,
    },
    #[code = "A9360"]
    #[severity = Hint]
    #[message = "Cannot confirm '{mem}' exists on {on}{cls} — its class hierarchy includes a class {reason}"]
    UnconfirmedMember {
        cls: String,
        mem: String,
        on: SuperclassOf,
        reason: UnconfirmedReason,
    },
    /// `##super()` binds to the superclass's version of the method at compile time; no
    /// `%DispatchMethod` and no same-named property stands in.
    #[code = "A7461"]
    #[severity = Error]
    #[message = "Method '{method}' does not exist in any superclass of {cls}, so ##super has nothing to call"]
    SuperWithoutMethod { method: String, cls: String },
    #[code = "A7604"]
    #[severity = Error]
    #[message = "Label '{label}' has no formal parameter list, so $$ cannot call it (IRIS: <PARAMETER>); `{label}()` would make it a function"]
    LabelNotAFunction { label: String },
    #[code = "A6851"]
    #[severity = Error]
    #[message = "Cannot find label '{label}' in {routine}"]
    CannotFindLabel { label: String, routine: InRoutine },
    #[code = "A1073"]
    #[severity = Error]
    #[message = "Cannot GOTO label '{label}' from inside a procedure block: it is outside the block"]
    GotoOutOfBlock { label: String },
    #[code = "A3303"]
    #[severity = Warning]
    #[message = "Cannot find routine '{routine}' in this workspace, with no IRIS connection to check it against"]
    CannotFindRoutine { routine: String },
    #[code = "A4439"]
    #[severity = Error]
    #[message = "Cannot find routine '{routine}' in the workspace or on the IRIS instance"]
    RoutineNotOnIris { routine: String },
    #[code = "A3794"]
    #[severity = Hint]
    #[message = "The labels of routine '{routine}' aren't known, so calls into it can't be checked precisely"]
    RoutineWithoutSource { routine: String },
    #[code = "A7831"]
    #[severity = Error]
    #[message = "Include file '{name}' is not defined in the current workspace"]
    CannotFindInclude { name: String },
    /// An `#if` whose condition isn't a literal `1` or `0` once its macros are expanded. IRIS
    /// evaluates any expression at compile time; ascot doesn't, and goes on with the first branch.
    #[code = "A5540"]
    #[severity = Hint]
    #[message = "Cannot decide `#if {condition}`{expansion} at compile time, so its first branch is assumed"]
    UndecidedIf { condition: String, expansion: Expansion },
    /// A lookup the IRIS context couldn't answer, so what it looked for is taken on trust.
    #[code = "A4471"]
    #[severity = Hint]
    #[message = "Cannot look up {lookup} on the IRIS instance ({reason}), so it is taken on trust"]
    IrisLookupFailed { lookup: IrisLookup, reason: String },
    #[code = "A5090"]
    #[severity = Error]
    #[message = "The macro '{name}' expands recursively without end"]
    MacroExpansionTooDeep { name: String },
    #[code = "A2716"]
    #[severity = Error]
    #[message = "Too many arguments to the macro '{name}', which takes {expected}"]
    MacroTooManyArguments { name: String, expected: usize },
    #[code = "A6127"]
    #[severity = Error]
    #[message = "Not enough arguments to the macro '{name}', which takes {expected}"]
    MacroTooFewArguments { name: String, expected: usize },
    #[code = "A3485"]
    #[severity = Error]
    #[message = "{function} can't take '{name}', which is private to this procedure block"]
    PrivateVariableNotAllowed { function: String, name: String },
    #[code = "A5268"]
    #[severity = Error]
    #[message = "{function} takes no by-reference argument"]
    ByRefArgumentNotAllowed { function: String },
    #[code = "A7342"]
    #[severity = Error]
    #[message = "Argument {position} of {function} can't be left empty"]
    OmittedArgument { function: String, position: usize },
    /// The handle a conversion keeps its state in has to be a plain local variable.
    #[code = "A4652"]
    #[severity = Error]
    #[message = "{function} takes only a plain local variable as its handle"]
    HandleNotALocal { function: String },
    #[code = "A8820"]
    #[severity = Error]
    #[message = "SET $VECTOR needs its third argument, the type"]
    SetVectorWithoutType,
    #[code = "A6463"]
    #[severity = Error]
    #[message = "The property keyword '{keyword}' needs a value"]
    PropertyKeywordWithoutValue { keyword: String },
    #[code = "A8914"]
    #[severity = Error]
    #[message = "Cannot reference instance-only member '{name}' in a class method"]
    AccessInstanceOnlyMemberFromClassContext { name: String },
    #[code = "A4983"]
    #[severity = Error]
    #[message = "Variable '{name}' is already declared"]
    RedeclareVariable { name: String },
    #[code = "A1817"]
    #[severity = Warning]
    #[message = "Variable '{name}' is already NEW'd; this NEW resets it to undefined, discarding its current value — `Kill {name}` says that explicitly"]
    RedundantNew { name: String },
    #[code = "A6866"]
    #[severity = Error]
    #[message = "Cannot NEW the private variable '{name}' in a procedure block, where every local variable is already NEW'd on entry"]
    NewInProcedureBlock { name: String },
    #[code = "A5573"]
    #[severity = Warning]
    #[message = "Class keyword '{keyword}' has no value"]
    ClassKeywordWithoutValue { keyword: String },
    /// An entry IRIS compiles past and drops: not a class keyword, or a value outside one's shape.
    #[code = "A6203"]
    #[severity = Warning]
    #[message = "'{entry}' is not a class keyword IRIS honors, so it is ignored; expected {expected}"]
    UnrecognizedClassKeyword { entry: String, expected: String },
    /// A `ClassType` IRIS refuses to compile.
    #[code = "A7718"]
    #[severity = Error]
    #[message = "'{entry}' is not a class keyword IRIS accepts; expected {expected}"]
    InvalidClassKeyword { entry: String, expected: String },
    #[code = "A9322"]
    #[severity = Error]
    #[message = "'{keyword}' is not a recognized property keyword; available: {available}"]
    UnrecognizedPropertyKeyword { keyword: String, available: Available },
    #[code = "A2357"]
    #[severity = Error]
    #[message = "'{name}' is not a parameter of {class}{hint}"]
    UnrecognizedTypeParam {
        name: String,
        class: String,
        hint: TypeParamHint,
    },
    #[code = "A2174"]
    #[severity = Error]
    #[message = "'{keyword}' is not a recognized parameter keyword; available: {available}"]
    UnrecognizedParameterKeyword { keyword: String, available: Available },
    #[code = "A8317"]
    #[severity = Warning]
    #[message = "'{meta_type}' is not a recognized parameter type; available: {available}"]
    UnrecognizedParameterType { meta_type: String, available: Available },
    #[code = "A7155"]
    #[severity = Warning]
    #[message = "ObjectScript has no operator precedence and evaluates strictly left-to-right; this may not compute what you expect"]
    MixedPrecedence,
    #[code = "A5718"]
    #[severity = Warning]
    #[message = "Variable '{name}' is assigned but never read"]
    UnreadAssignment { name: String },
    #[code = "A4476"]
    #[severity = Warning]
    #[message = "Output parameter '{name}' is never assigned; its caller will get an undefined value"]
    OutputNeverAssigned { name: String },
    #[code = "A8892"]
    #[severity = Error]
    #[message = "Variable '{name}' is {ty1} in one branch but {ty2} in another"]
    InconsistentDeclaration {
        name: String,
        ty1: Declared,
        ty2: Declared,
    },
    #[code = "A5967"]
    #[severity = Error]
    #[message = "{source} is incompatible with {target}"]
    TypeIncompatible { source: Ty, target: Ty },
    #[code = "A9180"]
    #[severity = Error]
    #[message = "Cannot assign {source} to '{name}' declared as {target}"]
    AssignIncompatible {
        name: String,
        source: Ty,
        target: Ty,
    },
    #[code = "A1203"]
    #[severity = Warning]
    #[message = "Unreachable code"]
    Unreachable,
    #[code = "A8456"]
    #[severity = Warning]
    #[message = "This condition is always {value}"]
    ConditionAlways { value: bool },
    /// IRIS compiles `..X(subs)` for any property but, unless `X` is `MultiDimensional`, throws
    /// `<OBJECT DISPATCH>` on reaching it, whatever `X`'s type.
    #[code = "A8496"]
    #[severity = Error]
    #[message = "Property '{name}' is not MultiDimensional, so it takes no subscripts"]
    NotMultidimensional { name: String },
    /// IRIS throws `<OBJECT DISPATCH>` "missing subscripts" on reading `..X()`, and refuses to
    /// compile `Set ..X() = v`.
    #[code = "A3942"]
    #[severity = Error]
    #[message = "Property '{name}' is MultiDimensional, so `{name}()` names no node: give it a subscript"]
    NodeWithoutSubscripts { name: String },
    /// IRIS compiles and runs the call, but the documentation gives the extra argument no meaning.
    #[code = "A5156"]
    #[severity = Warning]
    #[message = "${function} is documented to take at most {documented} argument(s), but was given {given}; IRIS tolerates the extra"]
    UndocumentedArgument { function: String, documented: usize, given: usize },
    #[code = "A6319"]
    #[severity = Error]
    #[message = "Parameter '{name}' is a constant: it takes no arguments and can't be assigned"]
    ParameterIsConstant { name: String },
    #[code = "A6705"]
    #[severity = Warning]
    #[message = "Too few arguments: '{next_arg}' is required. Only arguments with default values and `Output` arguments are considered optional."]
    TooFewArguments { next_arg: String },
    #[code = "A5523"]
    #[severity = Error]
    #[message = "Too many arguments"]
    TooManyArguments,
    #[code = "A7139"]
    #[severity = Error]
    #[message = "$CASE's `:default` must be its last argument"]
    CaseDefaultNotLast,
    #[code = "A3850"]
    #[severity = Error]
    #[message = "${name} is not a recognized function{suggestions}"]
    UnknownFunction {
        name: String,
        suggestions: Suggestions,
    },
    #[code = "A9399"]
    #[severity = Error]
    #[message = "${name} is not a recognized special variable{suggestions}"]
    UnknownSpecialVariable {
        name: String,
        suggestions: Suggestions,
    },
    #[code = "A6587"]
    #[severity = Error]
    #[message = "Invalid macro argument: {reason}"]
    InvalidMacroArgument { reason: String },
    #[code = "A3286"]
    #[severity = Error]
    #[message = "'{action}' is not a valid $ZSTRIP action: {reason}"]
    InvalidZStripAction {
        action: String,
        reason: &'static str,
    },
    #[code = "A9421"]
    #[severity = Warning]
    #[message = "{value} is a number here, implicitly converted to the string \"{value}\""]
    NumberImplicitlyConvertedToString { value: String },
    #[code = "A4556"]
    #[severity = Warning]
    #[message = "This call returns {class}, which Do discards -- consider Set to capture it, or checking it directly"]
    DiscardedReturnValue { class: String },
    #[code = "A2839"]
    #[severity = Warning]
    #[message = "{what} `{found}` should be written `{expected}`"]
    KeywordStyle {
        // "Command keyword" or "Function".
        what: &'static str,
        found: String,
        expected: String,
    },
    #[code = "A7895"]
    #[severity = Error]
    #[message = "'{format}' is not a valid $FNUMBER format: {reason}"]
    InvalidFNumberFormat { format: String, reason: String },
    #[code = "A6781"]
    #[severity = Error]
    #[message = "Expected return value of type '{wanted}' but got nothing"]
    ReturnNothingForX { wanted: Ty },
    #[code = "A8997"]
    #[severity = Warning]
    #[message = "Unexpected return value of type '{actual}'"]
    ReturnXForNothing { actual: Ty },
    #[code = "A4963"]
    #[severity = Error]
    #[message = "Quit with {actual} inside control structure"]
    QuitXInControl { actual: Ty },
    #[code = "A5579"]
    #[severity = Warning]
    #[message = "The parameter '{name}' is declared as {wanted} parameter but got {actual} argument"]
    MismatchArgumentAndArgumentMode {
        name: String,
        wanted: ArgMode,
        actual: ArgMode,
    },
    #[code = "A6826"]
    #[severity = Warning]
    #[message = "This '{ty}' value may be undefined{expr}"]
    MaybeUndefined { ty: Ty, expr: Detail },
    #[code = "A7095"]
    #[severity = Error]
    #[message = "This value is undefined{expr}"]
    Undefined { expr: Detail },
    #[code = "A1368"]
    #[severity = Error]
    #[message = "Not in a class context"]
    NotInClass,
    #[code = "A4025"]
    #[severity = Error]
    #[message = "The procedure call requires the private variable '{name}' to be public"]
    RequirePrivateVariable { name: String },
    #[code = "A4043"]
    #[severity = Information]
    #[message = "Type checking not implemented for '{what}'"]
    Unimplemented { what: String },
    #[code = "A3904"]
    #[severity = Warning]
    #[message = "The macro '{name}' is defined in the body of method '{method}'; IRIS keeps it for the methods compiled after it (alphabetically), but it belongs in an include file or at class level"]
    MacroFromMethodBody { name: String, method: String },
    #[code = "A2797"]
    #[severity = Error]
    #[message = "The macro '{name}' is undefined{suggestions}"]
    UndefinedMacro {
        name: String,
        suggestions: Suggestions,
    },
    #[code = "A5789"]
    #[severity = Error]
    #[message = "CONTINUE is not within a loop"]
    ContinueNotInLoop,
    #[code = "A3119"]
    #[severity = Error]
    #[message = "The member name {name} is defined twice"]
    DuplicatedMemberName { name: String },
    #[code = "A3717"]
    #[severity = Warning]
    #[message = "Class '{name}' is also declared in {other_uri}"]
    DuplicateClassName { name: String, other_uri: String },
    /// Code that doesn't parse. Where the grammar knows what went wrong, such as an unknown
    /// command or a function given the wrong arguments, the message says so; otherwise it lists
    /// what the parser was looking for.
    #[code = "A8073"]
    #[severity = Error]
    #[message = "{err}"]
    InvalidSyntax { err: SyntaxErrorText },
    #[code = "A1727"]
    #[severity = Warning]
    #[message = "Syntax error, looking for:\n{err}"]
    DocumentSyntaxError { err: tygr::Error },
    #[code = "A7548"]
    #[severity = Warning]
    #[message = "Missing an explicit QUIT or RETURN; execution falls through to whatever follows"]
    MissingQuit,
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

/// What a failed IRIS lookup was for.
#[derive(Debug)]
pub struct IrisLookup {
    pub kind: crate::DocKind,
    pub name: String,
}

impl std::fmt::Display for IrisLookup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self.kind {
            crate::DocKind::Cls => "class",
            crate::DocKind::Inc => "include file",
            crate::DocKind::Rtn => "routine",
        };
        write!(f, "{kind} '{}'", self.name)
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

/// What an `#if` condition's macros expanded to, when that differs from what was written.
#[derive(Debug)]
pub struct Expansion(pub Option<String>);

impl std::fmt::Display for Expansion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(text) => write!(f, " (`{text}`)"),
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

    fn at(kind: TypeErrorKind, range: Range) -> Self {
        Self::new(kind, range, vec![])
    }

    pub fn too_many_arguments(range: Range) -> Self {
        QuasiTypeError::too_many_arguments().into_error(|| range)
    }

    // Built with a range directly, unlike `QuasiTypeError`'s deferred ones — there's no `Flow`
    // yet to attach one lazily this early (during macro expansion).
    pub(crate) fn unconfirmed_class(cls: String, has_iris: bool, range: Range) -> Self {
        Self::at(
            TypeErrorKind::UnconfirmedClass {
                lookup: ClassLookup { cls, has_iris },
            },
            range,
        )
    }

    pub(crate) fn unconfirmed_member(
        cls: String,
        mem: String,
        superclass: bool,
        has_iris: bool,
        range: Range,
    ) -> Self {
        Self::at(
            TypeErrorKind::UnconfirmedMember {
                cls,
                mem,
                on: SuperclassOf(superclass),
                reason: UnconfirmedReason { has_iris },
            },
            range,
        )
    }

    pub(crate) fn class_keyword_without_value(keyword: String, range: Range) -> Self {
        Self::at(TypeErrorKind::ClassKeywordWithoutValue { keyword }, range)
    }

    pub(crate) fn duplicate_class_name(range: Range, name: String, other: Location) -> Self {
        Self::new(
            TypeErrorKind::DuplicateClassName {
                name,
                other_uri: other.uri.clone(),
            },
            range,
            related(other, "Also declared here"),
        )
    }

    pub(crate) fn document_syntax_error(range: Range, err: tygr::Error) -> Self {
        Self::at(TypeErrorKind::DocumentSyntaxError { err }, range)
    }

    pub fn cannot_find_include(name: String, range: Range) -> Self {
        Self::at(TypeErrorKind::CannotFindInclude { name }, range)
    }

    pub fn undecided_if(condition: String, expansion: Option<String>, range: Range) -> Self {
        Self::at(
            TypeErrorKind::UndecidedIf {
                condition,
                expansion: Expansion(expansion),
            },
            range,
        )
    }

    pub fn iris_lookup_failed(
        kind: crate::DocKind,
        name: String,
        reason: String,
        range: Range,
    ) -> Self {
        QuasiTypeError::iris_lookup_failed(kind, name, reason).into_error(|| range)
    }

    pub fn macro_expansion_too_deep(name: String, range: Range) -> Self {
        Self::at(TypeErrorKind::MacroExpansionTooDeep { name }, range)
    }

    pub fn macro_too_many_arguments(name: String, expected: usize, range: Range) -> Self {
        Self::at(
            TypeErrorKind::MacroTooManyArguments { name, expected },
            range,
        )
    }

    pub(crate) fn property_keyword_without_value(keyword: String, range: Range) -> Self {
        Self::at(
            TypeErrorKind::PropertyKeywordWithoutValue { keyword },
            range,
        )
    }

    pub fn macro_too_few_arguments(name: String, expected: usize, range: Range) -> Self {
        Self::at(
            TypeErrorKind::MacroTooFewArguments { name, expected },
            range,
        )
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
                .map(|location| related(location, message))
                .unwrap_or_default(),
        )
    }

    pub fn unimplemented(msg: String, range: Range) -> Self {
        QuasiTypeError::unimplemented(msg).into_error(|| range)
    }

    pub fn invalid_syntax(range: Range, err: tygr::Error) -> Self {
        Self::at(
            TypeErrorKind::InvalidSyntax {
                err: SyntaxErrorText(err),
            },
            range,
        )
    }

    pub fn unread_assignment(name: String, range: Range) -> TypeError {
        Self::at(TypeErrorKind::UnreadAssignment { name }, range)
    }

    pub fn output_never_assigned(name: String, range: Range) -> TypeError {
        Self::at(TypeErrorKind::OutputNeverAssigned { name }, range)
    }

    pub fn duplicated_member_name(range: Range, name: String, previous: Location) -> Self {
        Self::new(
            TypeErrorKind::DuplicatedMemberName { name },
            range,
            related(previous, "Previous definition"),
        )
    }

    pub fn missing_quit(range: Range) -> Self {
        Self::at(TypeErrorKind::MissingQuit, range)
    }

    pub fn redeclare_variable(name: String, range: Range, previous: Location) -> Self {
        Self::new(
            TypeErrorKind::RedeclareVariable { name },
            range,
            related(previous, "Previous declaration"),
        )
    }

    pub fn keyword_style(
        what: &'static str,
        found: String,
        expected: String,
        range: Range,
    ) -> Self {
        Self::at(
            TypeErrorKind::KeywordStyle {
                what,
                found,
                expected,
            },
            range,
        )
    }

    pub fn redundant_new(name: String, range: Range, previous: Location) -> Self {
        Self::new(
            TypeErrorKind::RedundantNew { name },
            range,
            related(previous, "Previous NEW"),
        )
    }

    pub fn macro_from_method_body(name: String, method: String, range: Range) -> Self {
        Self::at(TypeErrorKind::MacroFromMethodBody { name, method }, range)
    }

    pub fn handle_not_a_local(function: String, range: Range) -> Self {
        Self::at(TypeErrorKind::HandleNotALocal { function }, range)
    }

    pub fn label_not_a_function(label: String, range: Range) -> Self {
        Self::at(TypeErrorKind::LabelNotAFunction { label }, range)
    }

    pub fn new_in_procedure_block(name: String, range: Range) -> Self {
        Self::at(TypeErrorKind::NewInProcedureBlock { name }, range)
    }

    pub fn unrecognized_class_keyword(
        entry: String,
        expected: String,
        strict: bool,
        range: Range,
    ) -> Self {
        let kind = if strict {
            TypeErrorKind::InvalidClassKeyword { entry, expected }
        } else {
            TypeErrorKind::UnrecognizedClassKeyword { entry, expected }
        };
        Self::at(kind, range)
    }

    pub fn unrecognized_property_keyword(keyword: String, range: Range) -> Self {
        Self::at(
            TypeErrorKind::UnrecognizedPropertyKeyword {
                keyword,
                available: Available(PROPERTY_KEYWORDS),
            },
            range,
        )
    }

    pub fn unrecognized_type_param(
        name: String,
        class: String,
        is_property_keyword: bool,
        suggestions: Vec<String>,
        range: Range,
    ) -> Self {
        Self::at(
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
        )
    }

    pub fn unrecognized_parameter_keyword(keyword: String, range: Range) -> Self {
        Self::at(
            TypeErrorKind::UnrecognizedParameterKeyword {
                keyword,
                available: Available(PARAMETER_KEYWORDS),
            },
            range,
        )
    }

    pub fn unrecognized_parameter_type(meta_type: String, range: Range) -> Self {
        Self::at(
            TypeErrorKind::UnrecognizedParameterType {
                meta_type,
                available: Available(PARAMETER_TYPE_KEYWORDS),
            },
            range,
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
