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

    pub fn goto_out_of_block(label: String) -> Self {
        Self::WithoutRange(TypeErrorKind::GotoOutOfBlock(label), vec![])
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

// Declares `TypeErrorKind` with each variant's diagnostic code next to it, so the code, the
// variant's docs.rs anchor and the code shown in its docs all come from one place.
macro_rules! type_error_kinds {
    ($(
        $(#[doc = $doc:literal])*
        #[code = $code:literal]
        $variant:ident
        $(( $($tuple:ty),* ))?
        $({ $( $(#[doc = $fdoc:literal])* $field:ident: $fty:ty ),* $(,)? })?,
    )*) => {
        /// Every kind of diagnostic ascot reports.
        ///
        /// Each has a code, e.g. `A1234`, that clients can filter or key quick fixes on. A code is
        /// fixed once assigned and never reused, whatever later happens to the variant's name or
        /// message; it is random, so it says nothing about the kind.
        #[derive(Debug)]
        #[warn(missing_docs)]
        pub enum TypeErrorKind {
            $(
                $(#[doc = $doc])*
                #[doc = ""]
                #[doc = concat!("Code: `", $code, "`")]
                #[doc(alias = $code)]
                $variant $(( $($tuple),* ))? $({ $( $(#[doc = $fdoc])* $field: $fty ),* })?,
            )*
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
    /// A class is referenced that is neither in the workspace nor on the connected IRIS instance.
    #[code = "A8061"]
    CannotFindClass {
        /// The class name as written.
        cls: String,
    },
    /// A class named in a declaration (a type, a superclass) isn't in the workspace and can't be
    /// confirmed: there is no IRIS connection, or the instance doesn't have it either. A hint, since
    /// the class may well exist on the server the code runs on.
    #[code = "A1054"]
    UnconfirmedClass {
        /// The class name as written.
        cls: String,
        /// Whether an IRIS connection was available (and didn't have the class).
        has_iris: bool,
    },
    /// A property, method or parameter is accessed that its class doesn't have.
    #[code = "A3139"]
    CannotFindMember {
        /// The class searched.
        cls: String,
        /// The member name as written.
        mem: String,
        /// Similarly named members of the class.
        suggestions: Vec<String>,
        /// Whether the class has `%Get`, so `mem` could be a dynamic property.
        has_get: bool,
    },
    /// A member can't be confirmed to exist because the class hierarchy it would come from includes a
    /// class outside the workspace that IRIS couldn't supply. A hint, since the member may be
    /// inherited from that class.
    #[code = "A9360"]
    UnconfirmedMember {
        /// The class the member is looked up on.
        cls: String,
        /// The member name as written.
        mem: String,
        /// Whether it is looked up on a superclass (`##super`), not on `cls` itself.
        superclass: bool,
        /// Whether an IRIS connection was available (and didn't have the class).
        has_iris: bool,
    },
    /// A `Do`, `Goto` or `$$` names a label its routine doesn't define.
    #[code = "A6851"]
    CannotFindLabel {
        /// The label as written.
        label: String,
        /// The routine named after `^`, if any; otherwise the current one.
        routine: Option<String>,
    },
    /// A `Goto` inside a procedure block targets a label outside it, which IRIS doesn't allow (a
    /// `Do` may still call out, and `Goto label^routine` is allowed).
    #[code = "A1073"]
    GotoOutOfBlock(String),
    /// A `label^routine` reference names a routine that is neither in the workspace nor on the
    /// connected IRIS instance.
    #[code = "A3303"]
    CannotFindRoutine(String),
    /// An `#include` or class `Include` names an include file that is neither in the workspace nor on
    /// the connected IRIS instance, so its macros are unknown.
    #[code = "A7831"]
    CannotFindInclude(String),
    /// A macro's expansion includes itself, directly or through other macros, without end.
    #[code = "A5090"]
    MacroExpansionTooDeep(String),
    /// A class method uses an instance member (a property, or `..Method` of an instance method), which
    /// needs an object it doesn't have.
    #[code = "A8914"]
    AccessInstanceOnlyMemberFromClassContext {
        /// The member name.
        name: String,
    },
    /// A `#dim` declares a variable that is already declared.
    #[code = "A4983"]
    RedeclareVariable(String),
    /// A `New` names a variable that is already `New`'d, so it only discards the current value.
    #[code = "A1817"]
    RedundantNew(String),
    /// A `New` inside a procedure block, where every local variable is already private.
    #[code = "A6866"]
    NewInProcedureBlock(String),
    /// A class keyword that requires a value, such as `Extends`, is given without one.
    #[code = "A5573"]
    ClassKeywordWithoutValue(String),
    /// A property's `[ ... ]` list has a keyword properties don't take.
    #[code = "A9322"]
    UnrecognizedPropertyKeyword(String),
    /// A type's `( ... )` parameters name one that type doesn't have, e.g. `%String(MAXLEN=10,
    /// NOSUCH=1)`.
    #[code = "A2357"]
    UnrecognizedTypeParam {
        /// The parameter name as written.
        name: String,
        /// The type it was given to.
        class: String,
        /// Whether `name` is a property keyword, which belongs in `[ ... ]` instead.
        is_property_keyword: bool,
        /// Similarly named parameters of the type.
        suggestions: Vec<String>,
    },
    /// A class `Parameter`'s `[ ... ]` list has a keyword parameters don't take.
    #[code = "A2174"]
    UnrecognizedParameterKeyword(String),
    /// A class `Parameter` is declared `As` a type parameters can't have.
    #[code = "A8317"]
    UnrecognizedParameterType(String),
    /// An expression mixes operators as if they had precedence, which ObjectScript doesn't: `1+2*3`
    /// is `9`, evaluated strictly left to right.
    #[code = "A7155"]
    MixedPrecedence,
    /// A variable is assigned a value that nothing reads before it is overwritten or goes out of
    /// scope.
    #[code = "A5718"]
    UnreadAssignment(String),
    /// An `Output` parameter is never assigned on any path, so the caller gets an undefined value.
    #[code = "A4476"]
    OutputNeverAssigned(String),
    /// A variable is `#dim`'d differently, or only, in some of the branches that meet afterwards.
    #[code = "A8892"]
    InconsistentDeclaration {
        /// The variable.
        name: String,
        /// Its declared type in one branch; `None` if undeclared there.
        ty1: Option<Ty>,
        /// Its declared type in the other branch; `None` if undeclared there.
        ty2: Option<Ty>,
    },
    /// A value's type doesn't fit where it is used, e.g. an argument of the wrong type.
    #[code = "A5967"]
    TypeIncompatible {
        /// The value's type.
        source: Ty,
        /// The type expected there.
        target: Ty,
    },
    /// A `Set` assigns a value whose type doesn't fit the variable's declared type.
    #[code = "A9180"]
    AssignIncompatible {
        /// The variable.
        name: String,
        /// The value's type.
        source: Ty,
        /// The variable's declared type.
        target: Ty,
    },
    /// Code that can never run, e.g. after an unconditional `Quit`.
    #[code = "A1203"]
    Unreachable,
    /// A condition that always has the same value, so one of its branches never runs.
    #[code = "A8456"]
    ConditionAlways(bool),
    /// Something that isn't a method or routine is called like one.
    #[code = "A8496"]
    NotProcedure(Ty),
    /// A method is referenced without `()`: ObjectScript has no method values, so it must be called.
    #[code = "A3671"]
    ProcedureNotCalled,
    /// A call leaves out an argument that has no default value and isn't `Output`.
    #[code = "A6705"]
    TooFewArguments {
        /// The first missing parameter.
        next_arg: String,
    },
    /// A call passes more arguments than the method or routine has parameters.
    #[code = "A5523"]
    TooManyArguments,
    /// A `$CASE` has its `:default` somewhere other than last.
    #[code = "A7139"]
    CaseDefaultNotLast,
    /// A `$name(...)` call names no intrinsic function.
    #[code = "A3850"]
    UnknownFunction {
        /// The name after `$`.
        name: String,
        /// Similarly named functions.
        suggestions: Vec<String>,
    },
    /// A `$name` names no special variable.
    #[code = "A9399"]
    UnknownSpecialVariable {
        /// The name after `$`.
        name: String,
        /// Similarly named special variables.
        suggestions: Vec<String>,
    },
    /// A macro is called with arguments its definition can't take.
    #[code = "A6587"]
    InvalidMacroArgument(String),
    /// A `$ZSTRIP` action string asks for something `$ZSTRIP` doesn't do.
    #[code = "A3286"]
    InvalidZStripAction {
        /// The action string.
        action: String,
        /// What is wrong with it.
        reason: &'static str,
    },
    /// A number literal is used where a string is expected, so IRIS converts it, in canonical form
    /// (`007` becomes `"7"`).
    #[code = "A9421"]
    NumberImplicitlyConvertedToString(String),
    /// A `Do` calls a method that returns a value, such as a `%Status`, which `Do` throws away.
    #[code = "A4556"]
    DiscardedReturnValue(String),
    /// A command keyword or intrinsic function is written in a form the workspace's style rules don't
    /// allow, e.g. `S` where the full `Set` is required.
    #[code = "A2839"]
    KeywordStyle {
        /// "Command keyword" or "Function".
        what: &'static str,
        /// The keyword as written.
        found: String,
        /// The form the style rules ask for.
        expected: String,
    },
    /// A `$FNUMBER` format string asks for something `$FNUMBER` doesn't do.
    #[code = "A7895"]
    InvalidFNumberFormat {
        /// The format string.
        format: String,
        /// What is wrong with it.
        reason: String,
    },
    /// A `Quit` or `Return` without a value where the method declares a return type, or where an
    /// earlier return in the same procedure returned a value.
    #[code = "A6781"]
    ReturnNothingForX {
        /// The type expected.
        wanted: Ty,
    },
    /// A `Quit` or `Return` with a value where the method declares no return type, or where an earlier
    /// return in the same procedure returned nothing.
    #[code = "A8997"]
    ReturnXForNothing {
        /// The value's type.
        actual: Ty,
    },
    /// A `Quit` with a value inside a loop, where `Quit` only leaves the loop and can't return a value.
    #[code = "A4963"]
    QuitXInControl {
        /// The value's type.
        actual: Ty,
    },
    /// An argument is passed by value where the parameter is `Output` or `ByRef`, or by reference
    /// (`.x`) where it isn't.
    #[code = "A5579"]
    MismatchArgumentAndArgumentMode {
        /// The parameter.
        name: String,
        /// The parameter's declared mode.
        wanted: ArgMode,
        /// How the argument was passed.
        actual: ArgMode,
    },
    /// A variable is read that is undefined on some of the paths reaching it.
    #[code = "A6826"]
    MaybeUndefined {
        /// The type it has where it is defined.
        ty: Ty,
        /// The expression read, if it has a name.
        expr: Option<String>,
    },
    /// A variable is read that is undefined on every path reaching it.
    #[code = "A7095"]
    Undefined {
        /// The expression read, if it has a name.
        expr: Option<String>,
    },
    /// Something that needs an enclosing class, such as `..Member` or `##super`, is used in a
    /// routine.
    #[code = "A1368"]
    NotInClass,
    /// A call's result is used, but the method or routine returns nothing.
    #[code = "A7244"]
    ProcedureCallReturnsNothing,
    /// A call to a procedure relies on a variable it can't see: the procedure would need the variable
    /// in its public list.
    #[code = "A4025"]
    RequirePrivateVariable(String),
    /// Code ascot doesn't type check yet; the rest of the method isn't checked. Informational, not a
    /// problem in the code.
    #[code = "A4043"]
    Unimplemented(String),
    /// A `$$$name` names a macro that no `#define` or include file in scope defines.
    #[code = "A2797"]
    UndefinedMacro {
        /// The macro name.
        name: String,
        /// Similarly named macros in scope.
        suggestions: Vec<String>,
    },
    /// A `Continue` outside any loop.
    #[code = "A5789"]
    ContinueNotInLoop,
    /// A class defines two members with the same name.
    #[code = "A3119"]
    DuplicatedMemberName {
        /// The member name.
        name: String,
    },
    /// Two documents in the workspace declare the same class.
    #[code = "A3717"]
    DuplicateClassName {
        /// The class name.
        name: String,
        /// The other document's uri.
        other_uri: String,
    },
    /// A syntax error inside a method body or routine.
    #[code = "A8073"]
    InvalidSyntax {
        /// The parser's error.
        err: tygr::Error,
    },
    /// The document as a whole can't be parsed, so nothing in it is checked. A warning, since the
    /// cause may be syntax ascot's grammar doesn't cover yet.
    #[code = "A1727"]
    DocumentSyntaxError {
        /// The parser's error.
        err: tygr::Error,
    },
    /// A label's code runs off its end with no label to run into, falling through to whatever follows.
    #[code = "A7548"]
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
            TypeErrorKind::GotoOutOfBlock(_) => DiagnosticSeverity::Error,
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
            TypeErrorKind::KeywordStyle { .. } => DiagnosticSeverity::Warning,
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
            TypeErrorKind::UnconfirmedClass { .. } => DiagnosticSeverity::Hint,
            TypeErrorKind::UnconfirmedMember { .. } => DiagnosticSeverity::Hint,
            TypeErrorKind::ClassKeywordWithoutValue(_) => DiagnosticSeverity::Warning,
            TypeErrorKind::DuplicateClassName { .. } => DiagnosticSeverity::Error,
            TypeErrorKind::DocumentSyntaxError { .. } => DiagnosticSeverity::Warning,
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
            TypeErrorKind::UnconfirmedClass {
                cls,
                has_iris: true,
            } => {
                write!(
                    f,
                    "Cannot find class '{cls}' in the workspace or on the IRIS instance"
                )
            }
            TypeErrorKind::UnconfirmedClass {
                cls,
                has_iris: false,
            } => write!(
                f,
                "Cannot confirm class '{cls}' exists — it isn't open in this workspace, with no IRIS connection to check it against"
            ),
            TypeErrorKind::UnconfirmedMember {
                cls,
                mem,
                superclass,
                has_iris,
            } => {
                let on = if *superclass { "a superclass of " } else { "" };
                let reason = if *has_iris {
                    "that isn't on the IRIS instance"
                } else {
                    "outside this workspace, with no IRIS connection to check it against"
                };
                write!(
                    f,
                    "Cannot confirm '{mem}' exists on {on}{cls} — its class hierarchy includes a class {reason}"
                )
            }
            TypeErrorKind::ClassKeywordWithoutValue(keyword) => {
                write!(f, "Class keyword '{keyword}' has no value")
            }
            TypeErrorKind::DuplicateClassName { name, other_uri } => {
                write!(f, "Class '{name}' is also declared in {other_uri}")
            }
            TypeErrorKind::DocumentSyntaxError { err } => {
                write!(f, "Syntax error, looking for:\n{err}")
            }
            TypeErrorKind::CannotFindLabel { label, routine } => match routine {
                Some(routine) => write!(f, "Cannot find label '{label}' in routine '{routine}'"),
                None => write!(f, "Cannot find label '{label}' in this routine"),
            },
            TypeErrorKind::GotoOutOfBlock(label) => write!(
                f,
                "Cannot GOTO label '{label}' from inside a procedure block: it is outside the block"
            ),
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
            TypeErrorKind::KeywordStyle {
                what,
                found,
                expected,
            } => {
                write!(f, "{what} `{found}` should be written `{expected}`")
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
    pub(crate) fn unconfirmed_class(cls: String, has_iris: bool, range: Range) -> Self {
        Self::new(
            TypeErrorKind::UnconfirmedClass { cls, has_iris },
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
            superclass,
            has_iris,
        };
        Self::new(kind, range, vec![])
    }

    pub(crate) fn class_keyword_without_value(keyword: String, range: Range) -> Self {
        Self::new(
            TypeErrorKind::ClassKeywordWithoutValue(keyword),
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
