//! Every diagnostic the checker can raise, as the `TypeErrorKind` enum, with its message text,
//! severity and related-information locations (`TypeError` converts into `bindings::Diagnostic`).
//!
//! `QuasiTypeError` is an error found before its source range is known; rules in `mod.rs` attach
//! the range with `IntoTypeError::with_range`. Adding or rewording a user-visible message means
//! editing this file.

use crate::{
    bindings::{DiagnosticRelatedInformation, DiagnosticSeverity, Location, Range, RefMode},
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

    pub fn quit_exits() -> Self {
        TypeErrorKind::QuitExits.into()
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

    pub fn pattern_mismatch(value: String, what: String) -> Self {
        TypeErrorKind::PatternMismatch { value, what }.into()
    }

    pub fn downcast_from_abstract(name: String, source: Ty, target: Ty) -> Self {
        TypeErrorKind::DowncastFromAbstract {
            name,
            source,
            target,
        }
        .into()
    }

    pub fn type_downcast(source: Ty, target: Ty) -> Self {
        TypeErrorKind::TypeDowncast { source, target }.into()
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

    pub fn too_many_arguments(takes: usize) -> Self {
        TypeErrorKind::TooManyArguments {
            takes: ArgCount(takes),
        }
        .into()
    }

    pub fn case_default_not_last() -> Self {
        TypeErrorKind::CaseDefaultNotLast.into()
    }

    pub fn legacy_zutil(call: String, replacement: Option<&str>) -> Self {
        TypeErrorKind::LegacyZUtil {
            call,
            replacement: Replacement(replacement.map(str::to_string)),
        }
        .into()
    }

    pub fn undocumented_zutil(call: String) -> Self {
        TypeErrorKind::UndocumentedZUtil { call }.into()
    }

    pub fn undocumented_function(name: String) -> Self {
        TypeErrorKind::UndocumentedFunction { name }.into()
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

    pub fn user_command(name: String) -> Self {
        TypeErrorKind::UserCommand { name }.into()
    }

    pub fn cannot_find_member_on_abstract(
        cls: String,
        mem: String,
        suggestions: Vec<String>,
    ) -> Self {
        TypeErrorKind::CannotFindMemberOnAbstract {
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
            pub const CODES: &[&str] = &[$($code,)*];

            /// Every kind's code and doc comment, whose ```` ```objectscript ```` blocks are the
            /// examples `tests/examples.rs` and `scripts/verify-examples.py` check.
            pub const DOCS: &[(&str, &str)] = &[$(($code, concat!($($doc, "\n",)*)),)*];

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
    /// `##class(X)`, a declared type or an `Extends` names a class that is neither open in the
    /// workspace nor known to the environment, so the reference can only fail: IRIS reports
    /// `<CLASS DOES NOT EXIST>` when the call runs. Without an environment (`--iris none`), every
    /// class outside the workspace is such a class, the system ones included. Fix the name or open
    /// the class's file in the workspace; for a system class, check against an IRIS instance or its
    /// declarations.
    ///
    /// ```objectscript raises=<CLASS DOES NOT EXIST> do ##class(Demo.MissingClass).Call()
    /// Class Demo.MissingClass
    /// {
    ///
    /// ClassMethod Call() As %String
    /// {
    ///     Quit ##class(Nope.Missing).Hello() //! A8061
    /// }
    ///
    /// }
    /// ```
    #[code = "A8061"]
    #[severity = Error]
    #[message = "Cannot find class '{cls}' in the workspace or on the IRIS instance"]
    CannotFindClass { cls: String },
    /// A property's type or an `Extends` names a class that is neither open in the workspace nor
    /// in the environment. IRIS refuses to compile a class whose property type or superclass does
    /// not exist: check the name, or open the class's file in the workspace. Without an
    /// environment (`--iris none`) the same reference is A8061.
    ///
    /// ```objectscript rejected
    /// Class Demo.UnconfirmedClass
    /// {
    ///
    /// Property Owner As Demo.Nobody; //! A1054
    ///
    /// }
    /// ```
    #[code = "A1054"]
    #[severity = Hint]
    #[message = "Cannot {lookup}"]
    UnconfirmedClass { lookup: ClassLookup },
    /// No class along the receiver's hierarchy declares the member, nor does IRIS generate one
    /// with that name; the nearest declared names follow as suggestions. IRIS refuses to compile
    /// `..Name` for a property or method the class doesn't have. Spell the member as the class
    /// declares it, or declare it.
    ///
    /// ```objectscript
    /// Class Demo.MissingMember
    /// {
    ///
    /// Property Name As %String;
    ///
    /// Method Greet() As %String
    /// {
    ///     Quit "Hello, " _ ..Nmae //! A3139
    /// }
    ///
    /// }
    /// ```
    #[code = "A3139"]
    #[severity = Error]
    #[message = "Cannot find member '{mem}' in {cls}{suggestions}{dynamic}"]
    CannotFindMember {
        cls: String,
        mem: String,
        suggestions: Suggestions,
        dynamic: DynamicGet,
    },
    /// The receiver's class is abstract and declares no such member, so the value, always some
    /// subclass's, may still have it; the nearest declared names follow as suggestions. IRIS
    /// compiles the call and resolves the member when it runs. Declare the member on the abstract
    /// class, or type the value as the subclass that has it.
    ///
    /// ```objectscript compiles
    /// Class Demo.CannotFindMemberOnAbstract Extends %RegisteredObject [ Abstract ]
    /// {
    ///
    /// Method Area() As %Numeric [ Abstract ]
    /// {
    /// }
    ///
    /// ClassMethod Describe(shape As Demo.CannotFindMemberOnAbstract) As %String
    /// {
    ///     Quit "Perimeter: " _ shape.Perimeter() //! A4718
    /// }
    ///
    /// }
    /// ```
    #[code = "A4718"]
    #[severity = Warning]
    #[message = "Cannot find member '{mem}' in {cls}, an abstract class: the value is a subclass's, which may have it{suggestions}"]
    CannotFindMemberOnAbstract {
        cls: String,
        mem: String,
        suggestions: Suggestions,
    },
    /// No class ascot can see declares the member, and the receiver's hierarchy includes a class
    /// it can't see: a superclass neither in the workspace nor in the environment, or one whose
    /// workspace file doesn't parse. That class may well declare the member, so it is taken as
    /// present and untyped; open the superclass in the workspace, or check against an IRIS
    /// instance or its declarations, to have it confirmed. Without an environment (`--iris none`),
    /// every system superclass is such a class, and the `Extends` naming it is A8061.
    ///
    /// ```objectscript env=none
    /// Class Demo.UnconfirmedMember Extends %Persistent //! A8061
    /// {
    ///
    /// Method Store() As %Status
    /// {
    ///     Quit ..%Save() //! A9360
    /// }
    ///
    /// }
    /// ```
    #[code = "A9360"]
    #[severity = Hint]
    #[message = "Cannot confirm '{mem}' exists on {on}{cls} — its class hierarchy includes {unseen}"]
    UnconfirmedMember {
        cls: String,
        mem: String,
        on: SuperclassOf,
        unseen: UnseenClass,
    },
    /// `##super()` binds to the superclass's version of the enclosing method at compile time, and
    /// no superclass declares a method of that name; no `%DispatchMethod` and no same-named
    /// property stands in. IRIS refuses to compile the method. Remove the call, or spell the
    /// method as the superclass does.
    ///
    /// ```objectscript
    /// Class Demo.SuperWithoutMethod Extends %RegisteredObject
    /// {
    ///
    /// Method Describe() As %String
    /// {
    ///     Quit "Demo: " _ ##super() //! A7461
    /// }
    ///
    /// }
    /// ```
    #[code = "A7461"]
    #[severity = Error]
    #[message = "Method '{method}' does not exist in any superclass of {cls}, so ##super has nothing to call"]
    SuperWithoutMethod { method: String, cls: String },
    /// `$$Label` calls a label as a function, which a label is only when it declares a formal
    /// parameter list, even an empty `()`. `Do Label` reaches any label, but `$$` of a bare one
    /// compiles and raises `<PARAMETER>` when it runs. Give the label a parameter list, or call it
    /// with `Do`.
    ///
    /// ```objectscript raises=<PARAMETER> do Main^Demo.Labels
    /// ROUTINE Demo.Labels
    /// Greet
    ///     Write "hi", !
    ///     Quit
    /// Main
    ///     Write $$Greet() //! A7604
    ///     Quit
    /// ```
    #[code = "A7604"]
    #[severity = Error]
    #[message = "Label '{label}' has no formal parameter list, so $$ cannot call it (IRIS: <PARAMETER>); `{label}()` would make it a function"]
    LabelNotAFunction { label: String },
    /// `Do Label`, `$$Label()`, `Goto Label` or `Set $ZTRAP = "Label"` names a label the routine
    /// or body doesn't declare, or `Label^Routine` names one that routine lacks. IRIS refuses to
    /// compile a reference to a label the routine itself lacks, and raises `<NOLINE>` when a call
    /// into another routine reaches one it lacks; spell the label as it is declared.
    ///
    /// ```objectscript
    /// ROUTINE Demo.MissingLabel
    /// Main
    ///     Do Reprot //! A6851
    ///     Quit
    /// Report
    ///     Write "report", !
    ///     Quit
    /// ```
    #[code = "A6851"]
    #[severity = Error]
    #[message = "Cannot find label '{label}' in {routine}"]
    CannotFindLabel { label: String, routine: InRoutine },
    /// `Label+n` enters the label's code `n` lines down, where its formal list no longer applies,
    /// and which line that is depends on the compiled routine. ascot confirms the label exists but
    /// leaves the call untyped and the offset unchecked; IRIS compiles and runs it. The code is not
    /// in question, only how far ascot can check it; a call without the offset is checked in full.
    ///
    /// ```objectscript compiles
    /// ROUTINE Demo.LabelOffset
    /// Main
    ///     Do Report+1 //! A5213
    ///     Quit
    /// Report
    ///     Write "heading", !
    ///     Write "details", !
    ///     Quit
    /// ```
    #[code = "A5213"]
    #[severity = Hint]
    #[message = "The label is confirmed, but not where its offset lands: the call is checked as untyped and the offset is not checked"]
    LabelOffset,
    /// A `Goto` inside a procedure block, `Label() { }`, or a `$ZTRAP` set there, names one of
    /// the routine's labels outside the block, where a block's `Goto` cannot jump. IRIS refuses to
    /// compile the routine (`<NOLINE>`); `Do Label` may still call the label, or the block can
    /// `Quit` and leave what follows to its caller.
    ///
    /// ```objectscript
    /// ROUTINE Demo.GotoOutOfBlock
    /// Main(count) {
    ///     If count < 1 Goto Empty //! A1073
    ///     Write count, " items", !
    ///     Quit
    /// }
    /// Empty
    ///     Write "nothing to report", !
    ///     Quit
    /// ```
    #[code = "A1073"]
    #[severity = Error]
    #[message = "Cannot GOTO label '{label}' from inside a procedure block: it is outside the block"]
    GotoOutOfBlock { label: String },
    /// IRIS takes a label inside a `{ }` block as a label of the whole body, which the braces hide;
    /// ascot reads it where it stands, so its checks of the lines after the label may not match
    /// what IRIS runs. The routine compiles and runs as IRIS reads it: the warning is about ascot's
    /// reading, and a label at the top level of the body is read alike by both.
    ///
    /// ```objectscript compiles
    /// ROUTINE Demo.LabelInBlock
    /// Main(count)
    ///     If count > 0 {
    /// Next //! A7408
    ///         Write count, !
    ///         Set count = count - 1
    ///         If count > 0 Goto Next
    ///         Quit
    ///     }
    ///     Quit
    /// ```
    #[code = "A7408"]
    #[severity = Warning]
    #[message = "Label '{label}' is inside nested braces; ascot only understands labels at the top level of a method or routine body, so the checks after it may be inaccurate or incomplete"]
    LabelInBlock { label: String },
    /// `Label^Routine` or `^Routine` names a routine that isn't open in the workspace, and there is
    /// no environment (`--iris none`) to look it up in, so the call is checked as untyped. IRIS
    /// compiles a call into any routine, and raises `<NOROUTINE>` when it runs if the routine
    /// doesn't exist; open the routine in the workspace, or check against an IRIS instance or its
    /// declarations.
    ///
    /// ```objectscript env=none compiles
    /// ROUTINE Demo.UnknownRoutine
    /// Main
    ///     Do Nightly^Demo.Reports //! A3303
    ///     Quit
    /// ```
    #[code = "A3303"]
    #[severity = Warning]
    #[message = "Cannot find routine '{routine}' in this workspace, with no IRIS connection to check it against"]
    CannotFindRoutine { routine: String },
    /// `Label^Routine` or `^Routine` names a routine that is neither open in the workspace nor on
    /// the IRIS instance (or in its declarations), so the call can only fail: IRIS compiles it and
    /// raises `<NOROUTINE>` when it runs. Fix the name, or open the routine in the workspace.
    ///
    /// ```objectscript raises=<NOROUTINE> do Main^Demo.MissingRoutine
    /// ROUTINE Demo.MissingRoutine
    /// Main
    ///     Do Cleanup^Demo.Housekeeping //! A4439
    ///     Quit
    /// ```
    #[code = "A4439"]
    #[severity = Error]
    #[message = "Cannot find routine '{routine}' in the workspace or on the IRIS instance"]
    RoutineNotOnIris { routine: String },
    /// `Label^Routine` or `^Routine` calls into a routine that isn't open in the workspace and
    /// that the IRIS instance holds as object code only, without source (or with source ascot
    /// cannot parse), so its labels and their arguments are unknown and the call is checked as
    /// untyped. IRIS compiles and runs such a call like any other; open the routine's source in the
    /// workspace to have calls into it checked.
    ///
    /// ```objectscript compiles
    /// ROUTINE Demo.CompiledRoutine
    /// Main
    ///     Do ^%SYS.LICENSE //! A3794
    ///     Quit
    /// ```
    #[code = "A3794"]
    #[severity = Hint]
    #[message = "The labels of routine '{routine}' aren't known, so calls into it can't be checked precisely"]
    RoutineWithoutSource { routine: String },
    /// `#include Name` in a routine or method body, or `Include Name` before a class, names an
    /// include file that is neither open in the workspace nor known to the environment. IRIS
    /// refuses to compile (`MPP5635: No include file`); open the file in the workspace, or fix the
    /// name.
    ///
    /// ```objectscript
    /// ROUTINE Demo.MissingInclude
    /// #include Demo.ReportMacros //! A7831
    /// Main
    ///     Quit
    /// ```
    #[code = "A7831"]
    #[severity = Error]
    #[message = "Include file '{name}' is not defined in the current workspace"]
    CannotFindInclude { name: String },
    /// The condition of an `#if` or `#elseif` is not the literal `1` or `0` once its macros are
    /// expanded. IRIS evaluates any expression at compile time and compiles the branch it picks;
    /// ascot does not evaluate, so it checks the first branch and skips the rest, which may not be
    /// what IRIS compiles. The code is fine as far as IRIS is concerned; a macro that expands to
    /// `1` or `0` (`#define DEBUG 1`, `#if $$$DEBUG`) would make the choice visible to ascot.
    ///
    /// ```objectscript compiles
    /// ROUTINE Demo.UndecidedIf
    /// #define MODE "DEBUG"
    /// Main()
    ///     #if $$$MODE="DEBUG" //! A5540
    ///     Write "tracing on", !
    ///     #else
    ///     Write "tracing off", !
    ///     #endif
    ///     Quit
    /// ```
    #[code = "A5540"]
    #[severity = Hint]
    #[message = "Cannot decide `#if {condition}`{expansion} at compile time, so its first branch is assumed"]
    UndecidedIf { condition: String, expansion: Expansion },
    /// The IRIS connection failed to answer a lookup: a class, routine or include file the
    /// workspace doesn't hold was asked of the instance, and the request itself failed (the
    /// instance is down, the credentials are refused), as opposed to the instance not having it.
    /// The name is taken to exist and nothing about it is checked. The code may well be fine;
    /// restore the connection, or check without one (`--iris none`), for a verdict on it.
    #[code = "A4471"]
    #[severity = Hint]
    #[message = "Cannot look up {lookup} on the IRIS instance ({reason}), so it is taken on trust"]
    IrisLookupFailed { lookup: IrisLookup, reason: String },
    /// A macro's body invokes the macro itself, directly or through other macros, so expanding it
    /// never ends; ascot gives up after 200 substitutions and leaves the call as written. IRIS
    /// refuses to compile the line (MPP5649, too many macros referenced on one line). Have the
    /// body name the macro it meant.
    ///
    /// ```objectscript
    /// ROUTINE Demo.MacroExpansionTooDeep
    /// #define VERSION "1.0"
    /// #define BANNER "Demo "_$$$BANNER
    /// Main()
    ///     Write $$$BANNER, ! //! A5090
    ///     Quit
    /// ```
    #[code = "A5090"]
    #[severity = Error]
    #[message = "The macro '{name}' expands recursively without end"]
    MacroExpansionTooDeep { name: String },
    /// A macro is called with more arguments than its `#define` declares parameters. IRIS refuses
    /// to compile the line (MPP5613); ascot leaves the call unexpanded. Drop the extra arguments,
    /// or declare the parameters the macro needs (`#def1arg` takes the whole argument text as one).
    ///
    /// ```objectscript
    /// ROUTINE Demo.MacroTooManyArguments
    /// #define MAX(%a,%b) $SELECT(%a>%b:%a,1:%b)
    /// Main()
    ///     Write $$$MAX(1,2,3), ! //! A2716
    ///     Quit
    /// ```
    #[code = "A2716"]
    #[severity = Error]
    #[message = "Too many arguments to the macro '{name}', which takes {expected}"]
    MacroTooManyArguments { name: String, expected: usize },
    /// A macro is called with fewer arguments than its `#define` declares parameters. IRIS refuses
    /// to compile the line (MPP5614); ascot leaves the call unexpanded. Pass every parameter the
    /// definition declares.
    ///
    /// ```objectscript
    /// ROUTINE Demo.MacroTooFewArguments
    /// #define BETWEEN(%value,%lo,%hi) ((%value>=%lo)&&(%value<=%hi))
    /// Main()
    ///     Write $$$BETWEEN(7,1), ! //! A6127
    ///     Quit
    /// ```
    #[code = "A6127"]
    #[severity = Error]
    #[message = "Not enough arguments to the macro '{name}', which takes {expected}"]
    MacroTooFewArguments { name: String, expected: usize },
    /// `$NAME` and `$QUERY` take a variable the code can reach by name at run time, which a
    /// procedure block's private locals are not: IRIS refuses to compile either on a local the
    /// method has not listed as public. Add the variable to the method's `PublicList`, or use a
    /// global.
    ///
    /// ```objectscript
    /// Class Demo.PrivateVariableNotAllowed
    /// {
    ///
    /// ClassMethod Path() As %String
    /// {
    ///     Set data("a", 1) = "x"
    ///     Quit $NAME(data("a", 1)) //! A3485
    /// }
    ///
    /// }
    /// ```
    #[code = "A3485"]
    #[severity = Error]
    #[message = "{function} can't take '{name}', which is private to this procedure block"]
    PrivateVariableNotAllowed { function: String, name: String },
    /// `$ZF(-3)` has no by-reference arguments: a `.x` in the call is a syntax error IRIS refuses
    /// to compile. Pass the value itself and take what the function returns.
    ///
    /// ```objectscript
    /// Class Demo.ByRefArgumentNotAllowed
    /// {
    ///
    /// ClassMethod Call() As %Integer
    /// {
    ///     Quit $ZF(-3, "mylib", "Func", .out) //! A5268
    /// }
    ///
    /// }
    /// ```
    #[code = "A5268"]
    #[severity = Error]
    #[message = "{function} takes no by-reference argument"]
    ByRefArgumentNotAllowed { function: String },
    /// An argument between two commas is left empty where the function takes no empty argument:
    /// `$FNUMBER(amount,,2)` is fine, `$JUSTIFY(amount,,2)` is not. IRIS refuses to compile the
    /// call. Spell the argument out.
    ///
    /// ```objectscript
    /// Class Demo.OmittedArgument
    /// {
    ///
    /// ClassMethod Cents(amount As %Numeric) As %String
    /// {
    ///     Quit $JUSTIFY(amount, , 2) //! A7342
    /// }
    ///
    /// }
    /// ```
    #[code = "A7342"]
    #[severity = Error]
    #[message = "Argument {position} of {function} can't be left empty"]
    OmittedArgument { function: String, position: usize },
    /// `$ZCONVERT(string, mode, table, handle)` keeps the state of an I/O translation cut
    /// mid-character in its fourth argument, and IRIS takes nothing but a plain local variable
    /// there: a subscripted node, a global or a literal is a compile error. Give it an
    /// unsubscripted local.
    ///
    /// ```objectscript
    /// Class Demo.HandleNotALocal
    /// {
    ///
    /// ClassMethod ToUtf8(text As %String) As %String
    /// {
    ///     Quit $ZCONVERT(text, "O", "UTF8", state(1)) //! A4652
    /// }
    ///
    /// }
    /// ```
    #[code = "A4652"]
    #[severity = Error]
    #[message = "{function} takes only a plain local variable as its handle"]
    HandleNotALocal { function: String },
    /// `Set $VECTOR(v, position) = value` leaves out the element type, which the settable form
    /// always takes as its third argument: IRIS refuses to compile it, a vector that already
    /// exists included. Write `$VECTOR(v, position, "integer")`, or whichever type the vector
    /// holds.
    ///
    /// ```objectscript
    /// Class Demo.SetVectorWithoutType
    /// {
    ///
    /// ClassMethod Sum() As %Integer
    /// {
    ///     Set $VECTOR(scores, 1, "integer") = 3
    ///     Set $VECTOR(scores, 2) = 4 //! A8820
    ///     Quit $VECTOR(scores, 1) + $VECTOR(scores, 2)
    /// }
    ///
    /// }
    /// ```
    #[code = "A8820"]
    #[severity = Error]
    #[message = "SET $VECTOR needs its third argument, the type"]
    SetVectorWithoutType,
    /// A class method runs without an instance, so `..Name` has no object to read a property from
    /// and `..Method()` none to call an instance method on; IRIS refuses to compile either. Make
    /// the method an instance method, or reach the member through an object (`obj.Name`).
    ///
    /// ```objectscript
    /// Class Demo.AccessInstanceOnlyMemberFromClassContext Extends %RegisteredObject
    /// {
    ///
    /// Property Name As %String;
    ///
    /// ClassMethod Show()
    /// {
    ///     Write ..Name, ! //! A8914
    /// }
    ///
    /// }
    /// ```
    #[code = "A8914"]
    #[severity = Error]
    #[message = "Cannot reference instance-only member '{name}' in a class method"]
    AccessInstanceOnlyMemberFromClassContext { name: String },
    /// A method's or label's formal parameter list declares the same name twice, so one parameter
    /// would shadow the other. IRIS refuses to compile it: `#1050 Duplicate variable names in
    /// parameter list`. Give each parameter its own name.
    ///
    /// ```objectscript
    /// Class Demo.RedeclareVariable
    /// {
    ///
    /// ClassMethod Area(width As %Integer, width As %Integer) As %Integer //! A4983
    /// {
    ///     Quit width * width
    /// }
    ///
    /// }
    /// ```
    #[code = "A4983"]
    #[severity = Error]
    #[message = "Variable '{name}' is already declared"]
    RedeclareVariable { name: String },
    /// A `New` of a variable the same body has already `New`'d. IRIS compiles and runs it; all it
    /// does is leave the variable undefined until it is set again, its current value gone, which
    /// `Kill` says outright. Delete the second `New`, or write the `Kill` it stands for.
    ///
    /// ```objectscript compiles
    /// ROUTINE Demo.RedundantNew
    /// Main
    ///     New count
    ///     Set count = 1
    ///     Write count, !
    ///     New count //! A1817
    ///     Set count = 2
    ///     Write count, !
    ///     Quit
    /// ```
    #[code = "A1817"]
    #[severity = Warning]
    #[message = "Variable '{name}' is already NEW'd; this NEW resets it to undefined, discarding its current value — `Kill {name}` says that explicitly"]
    RedundantNew { name: String },
    /// `New` names a variable private to the procedure block: a parameter, or a local the body
    /// creates, which the block already scopes to itself on entry. IRIS refuses to compile it:
    /// `#1038 Private variable not allowed`. Delete the `New`, or `Kill` the variable if its value
    /// is to go; only a variable of the block's public list can be `New`'d.
    ///
    /// ```objectscript
    /// Class Demo.NewInProcedureBlock
    /// {
    ///
    /// ClassMethod Total(hours As %Integer, overtime As %Integer) As %Integer
    /// {
    ///     New sum //! A6866
    ///     Set sum = hours + overtime
    ///     Quit sum
    /// }
    ///
    /// }
    /// ```
    #[code = "A6866"]
    #[severity = Error]
    #[message = "Cannot NEW the private variable '{name}' in a procedure block, where every local variable is already NEW'd on entry"]
    NewInProcedureBlock { name: String },
    /// An entry in the class's `[ ... ]` keyword list is no class keyword as written: misspelled,
    /// or a member's keyword in the wrong list. IRIS compiles the class without the entry, so what
    /// it was meant to do silently doesn't happen; worse, its loader (2026.1) drops the header's
    /// `Extends` clause with it, leaving the class with no superclass. Fix the spelling, or move
    /// the keyword to the member it belongs to.
    ///
    /// ```objectscript compiles
    /// Class Demo.UnrecognizedClassKeyword Extends %RegisteredObject [ Abstact ] //! A6203
    /// {
    ///
    /// Property Name As %String;
    ///
    /// }
    /// ```
    #[code = "A6203"]
    #[severity = Warning]
    #[message = "'{entry}' is not a class keyword as written, though IRIS compiles it"]
    UnrecognizedClassKeyword { entry: String },
    /// A property type's `( ... )` parameters are the type class's own (`%String(MAXLEN = 50)`),
    /// and this name is none of them: misspelled, with the nearest declared names as suggestions,
    /// or a property keyword that belongs in the `[ ... ]` list. IRIS refuses to compile the class
    /// (`Property parameter not declared`). Fix the spelling, or move the keyword to the `[ ... ]`
    /// list.
    ///
    /// ```objectscript
    /// Class Demo.UnrecognizedTypeParam Extends %RegisteredObject
    /// {
    ///
    /// Property Name As %String(MAXLN = 50); //! A2357
    ///
    /// }
    /// ```
    #[code = "A2357"]
    #[severity = Error]
    #[message = "'{name}' is not a parameter of {class}{hint}"]
    UnrecognizedTypeParam {
        name: String,
        class: String,
        hint: TypeParamHint,
    },
    /// An entry in a member's `[ ... ]` keyword list is no keyword of that kind of member as
    /// written (misspelled, or another kind's), or the class's `ClassType` names no class type.
    /// IRIS refuses to compile the class. Fix the spelling, move the keyword to the member it
    /// belongs to, or give `ClassType` a class type IRIS knows.
    ///
    /// ```objectscript
    /// Class Demo.UnrecognizedKeyword Extends %RegisteredObject
    /// {
    ///
    /// Property Name As %String [ Requried ]; //! A9322
    ///
    /// }
    /// ```
    #[code = "A9322"]
    #[severity = Error]
    #[message = "'{entry}' is not {kind} keyword IRIS accepts"]
    UnrecognizedKeyword { kind: &'static str, entry: String },
    /// A class parameter's `As` clause takes a compiler meta-type (`STRING`, `INTEGER`,
    /// `BOOLEAN`, `CLASSNAME`, ...), not a datatype class, and this name is none of them. IRIS
    /// compiles the parameter all the same; pick the listed name that fits, or drop the clause.
    ///
    /// ```objectscript compiles
    /// Class Demo.UnrecognizedParameterType Extends %RegisteredObject
    /// {
    ///
    /// Parameter GREETING As %String = "hello"; //! A8317
    ///
    /// }
    /// ```
    #[code = "A8317"]
    #[severity = Warning]
    #[message = "'{meta_type}' is not a recognized parameter type; available: {available}"]
    UnrecognizedParameterType { meta_type: String, available: Available },
    /// ObjectScript has no operator precedence: `a + b * c` is `(a + b) * c`. The warning marks an
    /// expression in which an operator that convention binds loosely is directly followed by one it
    /// binds tightly (`+` then `*`, `=` then `+`, `&&` then `=`), since that is where left-to-right
    /// evaluation departs from the conventional reading; `a * b + c` comes out the same either way.
    /// IRIS compiles and computes the expression as written; add parentheses to say which reading
    /// is meant.
    ///
    /// ```objectscript compiles
    /// Class Demo.MixedPrecedence
    /// {
    ///
    /// ClassMethod Total(price As %Numeric, quantity As %Integer, shipping As %Numeric) As %Numeric
    /// {
    ///     Quit shipping + price * quantity //! A7155
    /// }
    ///
    /// }
    /// ```
    #[code = "A7155"]
    #[severity = Warning]
    #[message = "ObjectScript has no operator precedence and evaluates strictly left-to-right; this may not compute what you expect"]
    MixedPrecedence,
    /// A `Set` whose value no path reads before the variable is assigned again, killed or out of
    /// scope; a parameter the body never reads is reported the same way, its value being the
    /// caller's assignment. IRIS compiles and runs the code; the assignment is dead. Delete it, or
    /// read the variable where it was meant to be read.
    ///
    /// ```objectscript compiles
    /// Class Demo.UnreadAssignment
    /// {
    ///
    /// ClassMethod Greet(name As %String) As %String
    /// {
    ///     Set greeting = "Hello, " _ name //! A5718
    ///     Quit "Hi, " _ name
    /// }
    ///
    /// }
    /// ```
    #[code = "A5718"]
    #[severity = Warning]
    #[message = "Variable '{name}' is assigned but never read"]
    UnreadAssignment { name: String },
    /// An `Output` parameter no path of the body assigns. The caller passes a variable to receive a
    /// value and gets none, so its first read of the variable raises `<UNDEFINED>`; IRIS compiles
    /// the method all the same, since `Output` only documents intent. Assign the parameter on every
    /// path, or take it out of the signature.
    ///
    /// ```objectscript raises=<UNDEFINED> do ##class(Demo.OutputNeverAssigned).Double(5, .result) write result
    /// Class Demo.OutputNeverAssigned
    /// {
    ///
    /// ClassMethod Double(value As %Integer, Output result As %Integer) As %Integer //! A4476
    /// {
    ///     Quit value * 2
    /// }
    ///
    /// }
    /// ```
    #[code = "A4476"]
    #[severity = Warning]
    #[message = "Output parameter '{name}' is never assigned; its caller will get an undefined value"]
    OutputNeverAssigned { name: String },
    /// A `ByRef` parameter whose incoming value no path reads before writing it: the body only
    /// produces the value, so the caller need not pass one, which is what `Output` declares and
    /// what lets a caller leave the argument out. IRIS compiles either; the caller's `.` alone
    /// passes by reference, and the mode documents intent. If the parameter is indeed only
    /// produced, `Output` says so; if a caller is meant to pass a value the body reads in a case
    /// ascot doesn't see, `ByRef` is right.
    ///
    /// ```objectscript compiles
    /// Class Demo.ByRefNeverRead
    /// {
    ///
    /// ClassMethod Defaults(ByRef timeout As %Integer) //! A7762
    /// {
    ///     Set timeout = 30
    /// }
    ///
    /// }
    /// ```
    #[code = "A7762"]
    #[severity = Hint]
    #[message = "'{name}' is never read before it is assigned; `Output` fits better than `ByRef`"]
    ByRefNeverRead { name: String },
    /// An `Output` parameter read before any path writes it: the body depends on the value passed
    /// in, which `Output` disclaims, since a caller may leave the argument out or pass an undefined
    /// variable; ascot takes the parameter as undefined on entry, so the read is also reported as
    /// undefined. IRIS compiles either; the caller's `.` alone passes by reference, and the mode
    /// documents intent. If the body does rely on the incoming value, `ByRef` says so; assigning
    /// the parameter before reading it keeps `Output`.
    ///
    /// ```objectscript compiles
    /// Class Demo.OutputReadFirst
    /// {
    ///
    /// ClassMethod Tally(count As %Integer, Output total As %Integer) //! A7781
    /// {
    ///     Set total = total + count //! A7095
    /// }
    ///
    /// }
    /// ```
    #[code = "A7781"]
    #[severity = Hint]
    #[message = "'{name}' is read before it is assigned; `ByRef` fits better than `Output`"]
    OutputReadFirst { name: String },
    /// Two branches that join here each `#Dim` the variable, as different types, so it has no one
    /// declared type past the join. IRIS compiles and runs the code regardless: a `#Dim` type binds
    /// nothing at run time. Declare the variable once, before the branches, with a type both fit.
    ///
    /// ```objectscript compiles
    /// Class Demo.InconsistentDeclaration
    /// {
    ///
    /// ClassMethod Describe(asNumber As %Boolean) As %String
    /// {
    ///     If asNumber { //! A8892
    ///         #Dim value As %Integer
    ///         Set value = 42
    ///     } Else {
    ///         #Dim value As %String
    ///         Set value = "forty-two"
    ///     }
    ///     Quit value
    /// }
    ///
    /// }
    /// ```
    #[code = "A8892"]
    #[severity = Error]
    #[message = "Variable '{name}' is {ty1} in one branch but {ty2} in another"]
    InconsistentDeclaration {
        name: String,
        ty1: Declared,
        ty2: Declared,
    },
    /// A `Quit` or `Return` value's class is neither the method's declared return class nor a
    /// subclass of it; a variable or a parameter given such a value is A9180, which names it.
    /// IRIS compiles the method all the same, and the caller, trusting the declaration, fails on
    /// using the value as that class, here with `<METHOD DOES NOT EXIST>`. Return an instance of
    /// the declared class, or declare the class the method returns.
    ///
    /// ```objectscript raises=<METHOD DOES NOT EXIST> do ##class(Demo.TypeIncompatible).Call()
    /// Class Demo.TypeIncompatible
    /// {
    ///
    /// ClassMethod Names() As %ListOfDataTypes
    /// {
    ///     Quit ##class(%ArrayOfDataTypes).%New() //! A5967
    /// }
    ///
    /// ClassMethod Call() As %Status
    /// {
    ///     Set names = ..Names()
    ///     Quit names.Insert("Ann")
    /// }
    ///
    /// }
    /// ```
    #[code = "A5967"]
    #[severity = Error]
    #[message = "{source} is incompatible with {target}"]
    TypeIncompatible { source: Ty, target: Ty },
    /// `Set` gives a `#Dim`'d variable, or a call gives a typed parameter, a value whose class is
    /// neither the declared class nor a subclass of it (a datatype takes any value, and `""` is
    /// the null OREF). IRIS compiles the assignment all the same, and the code that trusts the
    /// declaration fails on using the value as that class, here with `<METHOD DOES NOT EXIST>`.
    /// Pass or assign an instance of the declared class, or declare the class the value has.
    ///
    /// ```objectscript raises=<METHOD DOES NOT EXIST> do ##class(Demo.AssignIncompatible).Call()
    /// Class Demo.AssignIncompatible
    /// {
    ///
    /// ClassMethod Fill(list As %ListOfDataTypes) As %Status
    /// {
    ///     Quit list.Insert("one")
    /// }
    ///
    /// ClassMethod Call() As %Status
    /// {
    ///     Set array = ##class(%ArrayOfDataTypes).%New()
    ///     Quit ..Fill(array) //! A9180
    /// }
    ///
    /// }
    /// ```
    #[code = "A9180"]
    #[severity = Error]
    #[message = "Cannot assign {source} to '{name}' declared as {target}"]
    AssignIncompatible {
        name: String,
        source: Ty,
        target: Ty,
    },
    /// A constant that doesn't match the `PATTERN` of the type it is assigned to, passed as or
    /// returned as, `"12"` for a `%String(PATTERN = "3N")`. IRIS holds a property to its pattern
    /// only when the object is validated or saved, which fails with ERROR #7209, and a method
    /// argument, a local or a return value never, so the value goes through. Match the pattern, or
    /// widen it where it is declared.
    ///
    /// ```objectscript
    /// Class Demo.PatternMismatch Extends %RegisteredObject
    /// {
    ///
    /// Property Code As %String(PATTERN = "3N");
    ///
    /// ClassMethod Take(code As %String(PATTERN = "3N")) As %String
    /// {
    ///     Return code
    /// }
    ///
    /// ClassMethod Call() As %String
    /// {
    ///     Set me = ..%New()
    ///     Set me.Code = "12" //! A7731
    ///     Return ..Take("abc") //! A7731
    /// }
    ///
    /// }
    /// ```
    #[code = "A7731"]
    #[severity = Warning]
    #[message = "{value} doesn't match the pattern of {what}"]
    PatternMismatch { value: String, what: String },
    /// `Set` gives a `#Dim`'d variable, or a call gives a typed parameter, a value typed as an
    /// abstract class where a subclass of it is declared. No instance is of the abstract class
    /// itself, so the value is some subclass's, which may be the declared one: IRIS compiles and
    /// runs the code, and it fails only when another subclass's instance arrives. Test `%IsA`
    /// first where one can.
    ///
    /// ```objectscript
    /// Class Demo.DowncastFromAbstract Extends %RegisteredObject [ Abstract ]
    /// {
    ///
    /// Property Name As %String;
    ///
    /// }
    /// ```
    ///
    /// ```objectscript
    /// Class Demo.DowncastFromAbstractCircle Extends Demo.DowncastFromAbstract
    /// {
    ///
    /// Property Radius As %Numeric;
    ///
    /// ClassMethod Diameter(circle As Demo.DowncastFromAbstractCircle) As %Numeric
    /// {
    ///     Quit circle.Radius * 2
    /// }
    ///
    /// ClassMethod Call(shape As Demo.DowncastFromAbstract) As %Numeric
    /// {
    ///     Quit ..Diameter(shape) //! A6274
    /// }
    ///
    /// }
    /// ```
    #[code = "A6274"]
    #[severity = Warning]
    #[message = "Assigning {source}, an abstract class, to '{name}' declared as {target}: the value is a subclass's, which may be one"]
    DowncastFromAbstract {
        name: String,
        source: Ty,
        target: Ty,
    },
    /// A `Quit` or `Return` value typed as an abstract class leaves a method whose declared return
    /// class is a subclass of it; a variable or a parameter given such a value is A6274, which
    /// names it. No instance is of the abstract class itself, so the value is some subclass's,
    /// which may be the declared one: IRIS compiles and runs the code, and the caller fails only
    /// when another subclass's instance arrives. Test `%IsA` first where one can.
    ///
    /// ```objectscript
    /// Class Demo.TypeDowncast Extends %RegisteredObject [ Abstract ]
    /// {
    ///
    /// Property Name As %String;
    ///
    /// }
    /// ```
    ///
    /// ```objectscript
    /// Class Demo.TypeDowncastCircle Extends Demo.TypeDowncast
    /// {
    ///
    /// Property Radius As %Numeric;
    ///
    /// ClassMethod AsCircle(shape As Demo.TypeDowncast) As Demo.TypeDowncastCircle
    /// {
    ///     Quit shape //! A8139
    /// }
    ///
    /// }
    /// ```
    #[code = "A8139"]
    #[severity = Warning]
    #[message = "{source} is abstract, so this value is a subclass's, which may be the {target} it is taken for"]
    TypeDowncast { source: Ty, target: Ty },
    /// Every path to this statement has already quit, returned or thrown, so it never runs. IRIS
    /// compiles it all the same. Delete the statement, or move it before the exit that precedes
    /// it.
    ///
    /// ```objectscript compiles
    /// Class Demo.Unreachable
    /// {
    ///
    /// ClassMethod Answer(verbose As %Boolean) As %Integer
    /// {
    ///     Quit 42
    ///     If verbose Write "never printed", ! //! A1203
    /// }
    ///
    /// }
    /// ```
    #[code = "A1203"]
    #[severity = Warning]
    #[message = "Unreachable: every path before this statement has quit, returned or thrown"]
    Unreachable,
    /// Every value the condition can hold decides it the same way, so one of its branches never
    /// runs. IRIS compiles and runs the code as written. A loop's own condition is exempt, since
    /// `While 1 { ... Quit }` is an idiom; for an `If`, remove the test or let the value vary.
    ///
    /// ```objectscript compiles
    /// Class Demo.ConditionAlways
    /// {
    ///
    /// ClassMethod Save() As %String
    /// {
    ///     Set verbose = 1
    ///     If verbose Write "saving", ! //! A8456
    ///     Quit "saved"
    /// }
    ///
    /// }
    /// ```
    #[code = "A8456"]
    #[severity = Warning]
    #[message = "This condition is always {value}"]
    ConditionAlways { value: bool },
    /// `..X(subs)` addresses a node of the property `X`, which only a `MultiDimensional` property
    /// has. IRIS compiles `..X(subs)` for any property but throws `<OBJECT DISPATCH>` on reaching
    /// it, whatever `X`'s type. A collection's element is `..X.GetAt(key)`; a property meant to
    /// hold nodes is declared `[ MultiDimensional ]`.
    ///
    /// ```objectscript raises=<OBJECT DISPATCH> do ##class(Demo.NotMultidimensional).%New().Call()
    /// Class Demo.NotMultidimensional Extends %RegisteredObject
    /// {
    ///
    /// Property Tags As list Of %String;
    ///
    /// Method Call() As %String
    /// {
    ///     Quit ..Tags(1) //! A8496
    /// }
    ///
    /// }
    /// ```
    #[code = "A8496"]
    #[severity = Error]
    #[message = "Property '{name}' is not MultiDimensional, so it takes no subscripts"]
    NotMultidimensional { name: String },
    /// `..X()` names a node of the `MultiDimensional` property `X` without the subscripts that
    /// pick one; `..X` without parentheses is the property's own value. IRIS throws
    /// `<OBJECT DISPATCH>` ("missing subscripts") on reading `..X()`, and refuses to compile
    /// `Set ..X() = v`. Give it a subscript, or drop the parentheses.
    ///
    /// ```objectscript raises=<OBJECT DISPATCH> do ##class(Demo.NodeWithoutSubscripts).%New().Call()
    /// Class Demo.NodeWithoutSubscripts Extends %RegisteredObject
    /// {
    ///
    /// Property Cache As %String [ MultiDimensional ];
    ///
    /// Method Call() As %String
    /// {
    ///     Quit ..Cache() //! A3942
    /// }
    ///
    /// }
    /// ```
    #[code = "A3942"]
    #[severity = Error]
    #[message = "Property '{name}' is MultiDimensional, so `{name}()` names no node: give it a subscript"]
    NodeWithoutSubscripts { name: String },
    /// `$ZF(-1, command)` is called with more arguments than its documentation names. IRIS
    /// compiles and runs the call all the same, and the documentation gives the extra argument
    /// no meaning. Drop it.
    ///
    /// ```objectscript
    /// Class Demo.UndocumentedArgument
    /// {
    ///
    /// ClassMethod Shell() As %Integer
    /// {
    ///     Quit $ZF(-1, "ls", 1) //! A5156
    /// }
    ///
    /// }
    /// ```
    #[code = "A5156"]
    #[severity = Warning]
    #[message = "${function} is documented to take at most {documented} argument(s), but was given {given}; IRIS tolerates the extra"]
    UndocumentedArgument { function: String, documented: usize, given: usize },
    /// The leading literal arguments make this `$ZUTIL` (`$ZU`) call one that the "Replacements
    /// for ObjectScript $ZUTIL Functions" list supersedes or deprecates. IRIS compiles and runs it
    /// all the same; use the replacement the message names. A `$ZU` a macro expands to is not
    /// reported.
    ///
    /// ```objectscript compiles
    /// Class Demo.LegacyZUtil
    /// {
    ///
    /// ClassMethod Namespace() As %String
    /// {
    ///     Quit $ZU(5) //! A4418
    /// }
    ///
    /// }
    /// ```
    #[code = "A4418"]
    #[severity = Warning]
    #[message = "{call} {replacement}"]
    LegacyZUtil { call: String, replacement: Replacement },
    /// The leading literal arguments of this `$ZUTIL` (`$ZU`) call match nothing in the
    /// "Replacements for ObjectScript $ZUTIL Functions" list, and IRIS documents no `$ZUTIL`
    /// function. IRIS compiles any `$ZU` call: one its kernel still has runs, one it has not
    /// raises `<FUNCTION>`. The arguments are checked and the result is Any; prefer the class
    /// method or special variable that does the job. A `$ZU` a macro expands to is not reported.
    ///
    /// ```objectscript compiles
    /// Class Demo.UndocumentedZUtil
    /// {
    ///
    /// ClassMethod Setting() As %String
    /// {
    ///     Quit $ZU(115, 5) //! A5847
    /// }
    ///
    /// }
    /// ```
    #[code = "A5847"]
    #[severity = Hint]
    #[message = "{call} is not documented for IRIS; $ZUTIL was superseded by class methods and special variables"]
    UndocumentedZUtil { call: String },
    /// A command spelled `Z` and letters that IRIS has none of is a user-defined command, for the
    /// instance's `^%ZLANGC00` routine to implement. IRIS compiles it and looks the definition up
    /// when it runs, raising `<SYNTAX>` where the routine has none. The arguments are checked,
    /// the command itself is not.
    ///
    /// ```objectscript raises=<SYNTAX> do ##class(Demo.UserCommand).Log("started")
    /// Class Demo.UserCommand
    /// {
    ///
    /// ClassMethod Log(message As %String)
    /// {
    ///     ZLog message //! A5362
    /// }
    ///
    /// }
    /// ```
    #[code = "A5362"]
    #[severity = Hint]
    #[message = "'{name}' is a user-defined command (^%ZLANGC00), so the call is not checked"]
    UserCommand { name: String },
    /// A `$Z` function IRIS compiles and runs but does not document (`$ZOBJEXPORT`, `$ZOBJVAL`,
    /// `$ZOBJNEW`, `$ZOBJNEXT`): the object-system internals its own classes call. ascot models
    /// documented ObjectScript only, so the arguments are checked, the call is not, and its
    /// result is Any.
    ///
    /// ```objectscript compiles
    /// Class Demo.UndocumentedFunction
    /// {
    ///
    /// ClassMethod Export(text As %String) As %String
    /// {
    ///     Quit $ZOBJEXPORT(text, 5) //! A6037
    /// }
    ///
    /// }
    /// ```
    #[code = "A6037"]
    #[severity = Hint]
    #[message = "${name} is not documented for IRIS, so the call is not checked"]
    UndocumentedFunction { name: String },
    /// `..#NAME(args)` calls the class parameter `NAME`, or `Set ..#NAME = v` assigns it, and a
    /// parameter is a compile-time constant that does neither. IRIS refuses to compile both. Read
    /// the parameter bare, as `..#NAME`; a value that varies belongs in a property or a method.
    ///
    /// ```objectscript
    /// Class Demo.ParameterIsConstant
    /// {
    ///
    /// Parameter LIMIT = 10;
    ///
    /// ClassMethod Call(limit As %Integer)
    /// {
    ///     Set ..#LIMIT = limit //! A6319
    /// }
    ///
    /// }
    /// ```
    #[code = "A6319"]
    #[severity = Error]
    #[message = "Parameter '{name}' is a constant: it takes no arguments and can't be assigned"]
    ParameterIsConstant { name: String },
    /// The call leaves out a parameter that has no default and isn't `Output`, so the callee
    /// finds it undefined, and reading it raises `<UNDEFINED>` unless the callee tests `$Data`
    /// first. IRIS compiles the call, as it does any call with fewer arguments than parameters.
    /// Pass the argument, or give the parameter a default.
    ///
    /// ```objectscript raises=<UNDEFINED> do ##class(Demo.TooFewArguments).Call()
    /// Class Demo.TooFewArguments
    /// {
    ///
    /// ClassMethod Greet(name As %String) As %String
    /// {
    ///     Quit "Hello, " _ name
    /// }
    ///
    /// ClassMethod Call() As %String
    /// {
    ///     Quit ..Greet() //! A6705
    /// }
    ///
    /// }
    /// ```
    #[code = "A6705"]
    #[severity = Warning]
    #[message = "Too few arguments: '{next_arg}' is required. Only arguments with default values and `Output` arguments are considered optional."]
    TooFewArguments { next_arg: String },
    /// The call passes more arguments than the callee declares, and the callee has no `args...`
    /// to take the rest. IRIS compiles the call and raises `<PARAMETER>` when it runs. The first
    /// argument beyond the declared ones is marked; drop the extra arguments, or declare the
    /// parameters they are meant for.
    ///
    /// ```objectscript raises=<PARAMETER> do ##class(Demo.TooManyArguments).Call()
    /// Class Demo.TooManyArguments
    /// {
    ///
    /// ClassMethod Add(left As %Integer, right As %Integer) As %Integer
    /// {
    ///     Quit left + right
    /// }
    ///
    /// ClassMethod Call() As %Integer
    /// {
    ///     Quit ..Add(1, 2, 3) //! A5523
    /// }
    ///
    /// }
    /// ```
    #[code = "A5523"]
    #[severity = Error]
    #[message = "Too many arguments: the callee takes {takes}, and this one is extra"]
    TooManyArguments { takes: ArgCount },
    /// `$CASE` takes its `:default` as the last argument only; an arm after it is a syntax error
    /// IRIS refuses to compile. Move the `:default` to the end.
    ///
    /// ```objectscript
    /// Class Demo.CaseDefaultNotLast
    /// {
    ///
    /// ClassMethod Name(code As %Integer) As %String
    /// {
    ///     Quit $CASE(code, 1:"one", :"many", 2:"two") //! A7139
    /// }
    ///
    /// }
    /// ```
    #[code = "A7139"]
    #[severity = Error]
    #[message = "$CASE's `:default` must be its last argument"]
    CaseDefaultNotLast,
    /// A `$` function with a name ObjectScript has no function for, usually a misspelling; the
    /// nearest names follow as suggestions. IRIS refuses to compile the call.
    ///
    /// ```objectscript
    /// Class Demo.UnknownFunction
    /// {
    ///
    /// ClassMethod Size(text As %String) As %Integer
    /// {
    ///     Quit $LENGHT(text) //! A3850
    /// }
    ///
    /// }
    /// ```
    #[code = "A3850"]
    #[severity = Error]
    #[message = "${name} is not a recognized function{suggestions}"]
    UnknownFunction {
        name: String,
        suggestions: Suggestions,
    },
    /// `$NAME` without an argument list is a special variable, and IRIS has none of this name, so
    /// the compiler refuses the code (`Invalid special variable`); the message suggests the
    /// nearest names. A `$Z` name is the exception IRIS compiles, for `^%ZLANGV00` to define:
    /// undefined, it raises `<SYNTAX>` when it runs.
    ///
    /// ```objectscript
    /// Class Demo.UnknownSpecialVariable
    /// {
    ///
    /// ClassMethod Owner() As %String
    /// {
    ///     Quit $USER //! A9399
    /// }
    ///
    /// }
    /// ```
    #[code = "A9399"]
    #[severity = Error]
    #[message = "${name} is not a recognized special variable{suggestions}"]
    UnknownSpecialVariable {
        name: String,
        suggestions: Suggestions,
    },
    /// An argument of a compile-time macro (one IRIS evaluates with `##expression` or
    /// `##safeexpression`, such as `$$$ERROR`) that doesn't parse as an expression, typically a
    /// message that lost its quotes. IRIS pastes the evaluated body in and refuses to compile the
    /// result. Quote the text, or pass an expression.
    ///
    /// ```objectscript
    /// Class Demo.InvalidMacroArgument
    /// {
    ///
    /// ClassMethod Save() As %Status
    /// {
    ///     Quit $$$ERROR($$$GeneralError, Not implemented) //! A6587
    /// }
    ///
    /// }
    /// ```
    #[code = "A6587"]
    #[severity = Error]
    #[message = "Invalid macro argument: {reason}"]
    InvalidMacroArgument { reason: String },
    /// `$ZSTRIP`'s action is `*`, or some of `<`, `>` and `=`, then mask codes from
    /// `E A P C N L U W`, a `'` before each one to negate, the negated after the plain; this
    /// constant action does not fit that form. IRIS compiles the call and raises `<FUNCTION>`
    /// when it runs. Spell the action from those codes.
    ///
    /// ```objectscript raises=<FUNCTION> do ##class(Demo.InvalidZStripAction).Trim(" x ")
    /// Class Demo.InvalidZStripAction
    /// {
    ///
    /// ClassMethod Trim(text As %String) As %String
    /// {
    ///     Quit $ZSTRIP(text, "<>S") //! A3286
    /// }
    ///
    /// }
    /// ```
    #[code = "A3286"]
    #[severity = Error]
    #[message = "'{action}' is not a valid $ZSTRIP action: {reason}"]
    InvalidZStripAction {
        action: String,
        reason: &'static str,
    },
    /// `$ZSTRIP`'s action and `$FNUMBER`'s format are strings with a grammar of their own, and
    /// this one is a bare number, which IRIS turns into its string form when the call runs. No
    /// number spells a valid action or format, so A3286 or A7895 comes with this and IRIS raises
    /// `<FUNCTION>`; write the action or format as a quoted string.
    ///
    /// ```objectscript raises=<FUNCTION> do ##class(Demo.NumberImplicitlyConvertedToString).Amount(1234.5)
    /// Class Demo.NumberImplicitlyConvertedToString
    /// {
    ///
    /// ClassMethod Amount(value As %Numeric) As %String
    /// {
    ///     Quit $FNUMBER(value, 2) //! A9421 A7895
    /// }
    ///
    /// }
    /// ```
    #[code = "A9421"]
    #[severity = Warning]
    #[message = "{value} is a number here, implicitly converted to the string \"{value}\""]
    NumberImplicitlyConvertedToString { value: String },
    /// `Do` runs the call and drops its value, which for a `%Status` or `%Boolean` return is the
    /// callee's report of whether it worked: the caller goes on as if it had. IRIS compiles and
    /// runs the call. Where the outcome matters, `Set sc = ..Save(...)` followed by `$$$ISERR(sc)`,
    /// or `If ..Save(...)`, puts the result to use; where it does not, the `Do` is a choice, not a
    /// mistake.
    ///
    /// ```objectscript compiles
    /// Class Demo.DiscardedReturnValue
    /// {
    ///
    /// ClassMethod Save(name As %String) As %Status
    /// {
    ///     Set ^Demo.Names(name) = $Horolog
    ///     Quit $$$OK
    /// }
    ///
    /// ClassMethod Call()
    /// {
    ///     Do ..Save("Ann") //! A4556
    /// }
    ///
    /// }
    /// ```
    #[code = "A4556"]
    #[severity = Warning]
    #[message = "This call returns {class}, which Do discards -- consider Set to capture it, or checking it directly"]
    DiscardedReturnValue { class: String },
    /// A command or intrinsic function keyword spelled in a form or case the style's `command` or
    /// `function` rule, or a per-keyword `command_<name>` or `function_<name>`, doesn't allow: `s`
    /// under `{"form":"Long"}`, `SET` under `{"case":"Pascal"}`. IRIS compiles every spelling alike;
    /// write the one the message gives.
    ///
    /// ```objectscript style={"command":{"form":"Long","case":"Pascal"},"function":{"form":"Long","case":"Pascal"}}
    /// Class Demo.KeywordStyle
    /// {
    ///
    /// ClassMethod FirstName(fullName As %String) As %String
    /// {
    ///     s first = $Piece(fullName, " ") //! S2839
    ///     Quit $zcvt(first, "U") //! S2839
    /// }
    ///
    /// }
    /// ```
    #[code = "S2839"]
    #[severity = Warning]
    #[message = "{kind} `{found}` should be written `{expected}`"]
    KeywordStyle {
        kind: tygr_objectscript::cos::KeywordKind,
        found: String,
        expected: String,
    },
    /// Under `{"allow_quit_for_return":false}`, a `Quit` that leaves the method or procedure, with no
    /// loop or `Try` block around it to leave instead. There it does what `Return` does, and IRIS runs
    /// either; write `Return`, keeping `Quit` for loops and `Try` blocks.
    ///
    /// ```objectscript style={"allow_quit_for_return":false}
    /// Class Demo.QuitExits
    /// {
    ///
    /// ClassMethod CountTo(limit As %Integer) As %Integer
    /// {
    ///     Set count = 0
    ///     For {
    ///         Set count = count + 1
    ///         If count >= limit Quit
    ///     }
    ///     Quit count //! S4151
    /// }
    ///
    /// }
    /// ```
    #[code = "S4151"]
    #[severity = Warning]
    #[message = "Use `Return` to leave a method or procedure; `Quit` is for loops and `Try` blocks"]
    QuitExits,
    /// Under `{"command":{"allow_postcondition_on_argument":false}}` (or `command_do`, `command_goto`,
    /// `command_xecute` for one command), a postconditional on an argument of `Do`, `Goto` or
    /// `Xecute`, `Do ^R:x`, rather than on the keyword, `Do:x ^R`. IRIS accepts both, and with a
    /// single argument they mean the same; move the condition onto the keyword.
    ///
    /// ```objectscript style={"command":{"allow_postcondition_on_argument":false}}
    /// Class Demo.ArgumentPostconditional
    /// {
    ///
    /// ClassMethod Run(verbose As %Boolean)
    /// {
    ///     Do ..Log("starting"):verbose //! S6307
    ///     Do:verbose ..Log("done")
    /// }
    ///
    /// ClassMethod Log(text As %String)
    /// {
    ///     Write text, !
    /// }
    ///
    /// }
    /// ```
    #[code = "S6307"]
    #[severity = Warning]
    #[message = "Put the postconditional on the command keyword, not on its argument"]
    ArgumentPostconditional,
    /// Under `{"condition_and":"DoubleAmpersand"}`, the conditions of an `If`, `ElseIf` or `While`
    /// joined by a comma, `If a, b`; under `"Comma"`, ones joined by `&&`. To IRIS the two mean the
    /// same; join them as the message says.
    ///
    /// ```objectscript style={"condition_and":"DoubleAmpersand"}
    /// Class Demo.AndOperator
    /// {
    ///
    /// ClassMethod InRange(n As %Integer, low As %Integer, high As %Integer) As %Boolean
    /// {
    ///     If n >= low, n <= high Quit 1 //! S7442
    ///     Quit 0
    /// }
    ///
    /// }
    /// ```
    #[code = "S7442"]
    #[severity = Warning]
    #[message = "Join these conditions with `{wanted}`"]
    AndOperator { wanted: crate::bindings::AndOperator },
    /// Under `{"max_new_per_block":N}`, each `New` past the Nth in one method body, label body or
    /// `{ }` block; a nested block counts its own. IRIS runs any number of them; gather the names into
    /// one `New a, b`.
    ///
    /// ```objectscript style={"max_new_per_block":1}
    /// ROUTINE Demo.NewPerBlock
    /// Report
    ///     New count
    ///     New total //! S3918
    ///     Set count = 3, total = 10
    ///     Write count, " items, total ", total, !
    ///     Quit
    /// ```
    #[code = "S3918"]
    #[severity = Warning]
    #[message = "The style allows {max} `New` per block; this is one more"]
    NewPerBlock { max: u32 },
    /// Under `{"max_commands_per_line":N}`, each command past the Nth on one line; a brace-less `If`,
    /// `Else` or `For` and the commands it governs count as one. IRIS reads the line as it would the
    /// commands on separate lines; give each its own.
    ///
    /// ```objectscript style={"max_commands_per_line":1}
    /// Class Demo.CommandsPerLine
    /// {
    ///
    /// ClassMethod Greet(name As %String)
    /// {
    ///     Set greeting = "Hello, " _ name Write greeting, ! //! S5260
    /// }
    ///
    /// }
    /// ```
    #[code = "S5260"]
    #[severity = Warning]
    #[message = "The style allows {max} command(s) per line; this is one more"]
    CommandsPerLine { max: u32 },
    /// Under `{"require_description":{"min_body_lines":N}}`, a method, or a routine label with a
    /// parameter list, whose body spans at least N lines and has no description: `///` lines right
    /// above a method, comment lines right above a label. Private methods are exempt. IRIS compiles
    /// it with or without; say what it does above it.
    ///
    /// ```objectscript style={"require_description":{"min_body_lines":2}}
    /// Class Demo.Undescribed
    /// {
    ///
    /// /// The larger of `a` and `b`.
    /// ClassMethod Max(a As %Integer, b As %Integer) As %Integer
    /// {
    ///     If a > b Quit a
    ///     Quit b
    /// }
    ///
    /// ClassMethod Min(a As %Integer, b As %Integer) As %Integer //! S7126
    /// {
    ///     If a < b Quit a
    ///     Quit b
    /// }
    ///
    /// }
    /// ```
    #[code = "S7126"]
    #[severity = Warning]
    #[message = "Describe this {kind}: its body is {lines} lines"]
    Undescribed { kind: String, lines: usize },
    /// Under `{"open_brace":{"newline_before":false}}`, a block's `{` on a line of its own below its
    /// command; under `true`, one on the command's line, `If x {`. IRIS accepts either layout; move
    /// the brace.
    ///
    /// ```objectscript style={"open_brace":{"newline_before":false}}
    /// Class Demo.OpenBraceBreak
    /// {
    ///
    /// ClassMethod Sign(n As %Integer) As %Integer
    /// {
    ///     If n < 0
    ///     { //! S8215
    ///         Quit -1
    ///     }
    ///     Quit 1
    /// }
    ///
    /// }
    /// ```
    #[code = "S8215"]
    #[severity = Warning]
    #[message = "Put {wanted} before `{{`"]
    OpenBraceBreak { wanted: Break },
    /// Under `{"open_brace":{"newline_after":true}}`, a block's first command on the line of its `{`,
    /// `If x { Set y = 1`; under `false`, one on the next line. IRIS accepts either layout; break the
    /// line after the brace, or join it.
    ///
    /// ```objectscript style={"open_brace":{"newline_after":true}}
    /// Class Demo.BodyBreak
    /// {
    ///
    /// ClassMethod Sign(n As %Integer) As %Integer
    /// {
    ///     If n < 0 { Quit -1 } //! S4933
    ///     Quit 1
    /// }
    ///
    /// }
    /// ```
    #[code = "S4933"]
    #[severity = Warning]
    #[message = "Put {wanted} after `{{`"]
    BodyBreak { wanted: Break },
    /// Under `{"close_brace":{"newline_before":true}}`, a block's `}` on the line of its last command,
    /// `Set y = 1 }`; under `false`, one on a line of its own. IRIS accepts either layout; move the
    /// brace.
    ///
    /// ```objectscript style={"close_brace":{"newline_before":true}}
    /// Class Demo.CloseBraceBreak
    /// {
    ///
    /// ClassMethod Sign(n As %Integer) As %Integer
    /// {
    ///     If n < 0 {
    ///         Quit -1 } //! S3627
    ///     Quit 1
    /// }
    ///
    /// }
    /// ```
    #[code = "S3627"]
    #[severity = Warning]
    #[message = "Put {wanted} before `}}`"]
    CloseBraceBreak { wanted: Break },
    /// Under `{"close_brace":{"newline_after":false}}`, an `Else`, `ElseIf` or `Catch` on the line
    /// below the `}` it follows; under `true`, one on the brace's line, `} Else {`. IRIS accepts either
    /// layout; move the keyword.
    ///
    /// ```objectscript style={"close_brace":{"newline_after":false}}
    /// Class Demo.ChainBreak
    /// {
    ///
    /// ClassMethod Sign(n As %Integer) As %Integer
    /// {
    ///     If n < 0 {
    ///         Quit -1
    ///     }
    ///     Else { //! S9048
    ///         Quit 1
    ///     }
    /// }
    ///
    /// }
    /// ```
    #[code = "S9048"]
    #[severity = Warning]
    #[message = "Put {wanted} between `}}` and `{keyword}`"]
    ChainBreak {
        keyword: &'static str,
        wanted: Break,
    },
    /// Under `{"space_after_comma":true}`, a list's comma with no space after it or one before,
    /// `$ListBuild(a,b)`; under `false`, one with a space on either side. A comma whose whitespace holds
    /// a line break, and the two around an omitted argument, are left alone. IRIS ignores the
    /// spacing; respace the comma.
    ///
    /// ```objectscript style={"space_after_comma":true}
    /// Class Demo.CommaSpacing
    /// {
    ///
    /// ClassMethod Pair(a As %String, b As %String) As %List
    /// {
    ///     Quit $ListBuild(a,b) //! S5583
    /// }
    ///
    /// }
    /// ```
    #[code = "S5583"]
    #[severity = Warning]
    #[message = "Put {wanted}"]
    CommaSpacing { wanted: CommaSpace },
    /// `$FNUMBER`'s format is a string of the codes `+ - , . D E G L N O P T`, in any combination
    /// but the pairs IRIS forbids (such as `,` with `.`); this constant format has a character
    /// that is no code, or such a pair. IRIS compiles the call and raises `<FUNCTION>` when it
    /// runs. Drop the stray character, or one code of the pair.
    ///
    /// ```objectscript raises=<FUNCTION> do ##class(Demo.InvalidFNumberFormat).Amount(1234.5)
    /// Class Demo.InvalidFNumberFormat
    /// {
    ///
    /// ClassMethod Amount(value As %Numeric) As %String
    /// {
    ///     Quit $FNUMBER(value, ",.", 2) //! A7895
    /// }
    ///
    /// }
    /// ```
    #[code = "A7895"]
    #[severity = Error]
    #[message = "'{format}' is not a valid $FNUMBER format: {reason}"]
    InvalidFNumberFormat { format: String, reason: String },
    /// The method declares a return type, and this `Quit` or `Return` has no value to give it.
    /// IRIS compiles the method, and a caller that takes the value (`Set x = ..Half(3)`) raises
    /// `<COMMAND>` when this `Quit` runs; `Do` tolerates it. Return a value of the declared type,
    /// or drop the `As` clause if the method has nothing to return.
    ///
    /// ```objectscript raises=<COMMAND> write ##class(Demo.ReturnNothingForX).Half(3)
    /// Class Demo.ReturnNothingForX
    /// {
    ///
    /// ClassMethod Half(n As %Integer) As %Integer
    /// {
    ///     If n # 2 Quit //! A6781
    ///     Quit n / 2
    /// }
    ///
    /// }
    /// ```
    #[code = "A6781"]
    #[severity = Error]
    #[message = "Expected return value of type '{wanted}' but got nothing"]
    ReturnNothingForX { wanted: Ty },
    /// The method declares no return type, and this `Quit` or `Return` carries a value: a caller
    /// that asks for one gets it, but nothing in the signature says so. IRIS compiles and runs
    /// it. Declare the type with `As`, or drop the value.
    ///
    /// ```objectscript compiles
    /// Class Demo.ReturnXForNothing
    /// {
    ///
    /// ClassMethod Log(message As %String)
    /// {
    ///     Write message, !
    ///     Quit 1 //! A8997
    /// }
    ///
    /// }
    /// ```
    #[code = "A8997"]
    #[severity = Warning]
    #[message = "Unexpected return value of type '{actual}'"]
    ReturnXForNothing { actual: Ty },
    /// This `Quit` carries a value inside a `For`, `While`, `Do ... While`, `Try` or `Catch`
    /// block, which it leaves instead of the method, so it has no caller to hand the value to.
    /// IRIS refuses to compile it in a `While`, `Do ... While`, `Try` or `Catch` block (`QUIT
    /// argument not allowed`), and compiles it in a `For` loop but raises `<COMMAND>` when it
    /// runs. Leave the block with a bare `Quit` and return the value after it, or `Return` it
    /// from inside.
    ///
    /// ```objectscript raises=<COMMAND> do ##class(Demo.QuitXInControl).FirstNegative($ListBuild(1, -2, 3))
    /// Class Demo.QuitXInControl
    /// {
    ///
    /// ClassMethod FirstNegative(list As %List) As %Integer
    /// {
    ///     For i = 1:1:$ListLength(list) {
    ///         Set item = $List(list, i)
    ///         If item < 0 Quit item //! A4963
    ///     }
    ///     Quit 0
    /// }
    ///
    /// }
    /// ```
    #[code = "A4963"]
    #[severity = Error]
    #[message = "Quit with {actual} inside control structure"]
    QuitXInControl { actual: Ty },
    /// The argument and the parameter disagree on by-reference passing: a plain argument where
    /// the parameter is `ByRef` or `Output`, so the callee's assignments never reach the caller,
    /// or a dotted one where the parameter is neither, so they do without the signature saying
    /// so. Only the caller's `.` passes by reference; `ByRef` and `Output` just declare the
    /// intent, so IRIS compiles and runs either way. Add or drop the `.`, or correct the
    /// declaration.
    ///
    /// ```objectscript compiles
    /// Class Demo.MismatchArgumentAndArgumentMode
    /// {
    ///
    /// ClassMethod Increment(ByRef counter As %Integer)
    /// {
    ///     Set counter = counter + 1
    /// }
    ///
    /// ClassMethod Call() As %Integer
    /// {
    ///     Set counter = 0
    ///     Do ..Increment(counter) //! A5579
    ///     Quit counter
    /// }
    ///
    /// }
    /// ```
    #[code = "A5579"]
    #[severity = Warning]
    #[message = "Passing {actual} argument to '{name}', {wanted} parameter"]
    MismatchArgumentAndArgumentMode {
        name: String,
        wanted: Mode,
        actual: Mode,
    },
    /// The variable has a value on some of the paths that reach this use and none on the others:
    /// an assignment under a condition, or a `Kill` on one branch. IRIS compiles the code and raises
    /// `<UNDEFINED>` when it runs along a path without the value. Assign it on every path, or read
    /// it through `$Get`.
    ///
    /// ```objectscript raises=<UNDEFINED> do ##class(Demo.MaybeUndefined).Label(1)
    /// Class Demo.MaybeUndefined
    /// {
    ///
    /// ClassMethod Label(count As %Integer) As %String
    /// {
    ///     If count > 1 Set suffix = "s"
    ///     Quit "item" _ suffix //! A6826
    /// }
    ///
    /// }
    /// ```
    #[code = "A6826"]
    #[severity = Warning]
    #[message = "This '{ty}' value may be undefined{expr}"]
    MaybeUndefined { ty: Ty, expr: Detail },
    /// The variable has no value on any path that reaches this use: nothing assigns it, or every
    /// assignment is killed or skipped on the way here. IRIS compiles the code and raises
    /// `<UNDEFINED>` when it runs. Assign it first, or read it through `$Get`.
    ///
    /// ```objectscript raises=<UNDEFINED> do ##class(Demo.Undefined).Total()
    /// Class Demo.Undefined
    /// {
    ///
    /// ClassMethod Total() As %Integer
    /// {
    ///     Set sum = 0
    ///     Quit sum + count //! A7095
    /// }
    ///
    /// }
    /// ```
    #[code = "A7095"]
    #[severity = Error]
    #[message = "This value is undefined{expr}"]
    Undefined { expr: Detail },
    /// `..Member` or `$this` in a routine, which has no class to resolve the member against, so the
    /// value is taken as `Any`. IRIS compiles it and binds `$this` when it runs, to the object of a
    /// method that called the routine, if one did; with no current object, `..Member` raises `<NO
    /// CURRENT OBJECT>`. Called only from methods, the code works; passing the object in as an
    /// argument makes the dependency explicit and lets ascot check the member.
    ///
    /// ```objectscript raises=<NO CURRENT OBJECT> do Main^Demo.NotInClass
    /// ROUTINE Demo.NotInClass
    /// Main
    ///     Write ..Name, ! //! A1368
    ///     Quit
    /// ```
    #[code = "A1368"]
    #[severity = Warning]
    #[message = "Not in a class context; this relies on `$this` being set when it runs"]
    NotInClass,
    /// The callee, a procedure, names the variable in its public list, but here it is private, as
    /// every local of a method or `Label() { }` block is, so the callee reaches the public variable
    /// of that name instead. IRIS compiles the call and runs it against that other variable,
    /// undefined unless something outside the block set it (`<UNDEFINED>`). Name the variable in
    /// this block's own public list, or pass it as an argument.
    ///
    /// ```objectscript raises=<UNDEFINED> do Main^Demo.RequirePrivateVariable
    /// ROUTINE Demo.RequirePrivateVariable
    /// Main() Public {
    ///     Set total = 0
    ///     Do Add(5) //! A4025
    ///     Write total, !
    ///     Quit
    /// }
    /// Add(n) [total] {
    ///     Set total = total + n
    ///     Quit
    /// }
    /// ```
    #[code = "A4025"]
    #[severity = Error]
    #[message = "The procedure call requires the private variable '{name}' to be public"]
    RequirePrivateVariable { name: String },
    /// A construct ascot doesn't model: an argumentless `Do` and its dotted block, a stand-alone
    /// `Else` or `ElseIf`, a `Goto` without an argument or with several, a `Break` with an
    /// argument, a `Kill` of a macro, a `#Dim` type it can't read, the legacy COM binding
    /// `_obj!sub` and `##quoteExp(...)`. IRIS compiles them; ascot skips the statement and goes on
    /// after it, so what it does is unseen. The modern form (an `If` with a block, a `Do` of a
    /// label) is checked.
    ///
    /// ```objectscript compiles
    /// ROUTINE Demo.Unimplemented
    /// Main
    ///     Set count = 0
    ///     Do //! A4043
    ///     . Set count = count + 1
    ///     Write count, !
    ///     Quit
    /// ```
    #[code = "A4043"]
    #[severity = Information]
    #[message = "Type checking not implemented for '{what}'"]
    Unimplemented { what: String },
    /// A `#define` in one method's body is used from another method. IRIS compiles a class's
    /// methods in name order into one routine and keeps a body's definitions for the rest of it,
    /// so the macro reaches the methods after it alphabetically and no other, and renaming either
    /// method changes what compiles. Define it in an include file the class `Include`s.
    ///
    /// ```objectscript compiles
    /// Class Demo.MacroFromMethodBody
    /// {
    ///
    /// ClassMethod Greet() As %String
    /// {
    ///     #define GREETING "Hello"
    ///     Quit $$$GREETING
    /// }
    ///
    /// ClassMethod Shout() As %String
    /// {
    ///     Quit $$$GREETING_"!" //! A3904
    /// }
    ///
    /// }
    /// ```
    #[code = "A3904"]
    #[severity = Warning]
    #[message = "The macro '{name}' is defined in the body of method '{method}'; IRIS keeps it for the methods compiled after it (alphabetically), but it belongs in an include file or at class level"]
    MacroFromMethodBody { name: String, method: String },
    /// `$$$Name` names a macro no `#define` in reach declares: not in the file, its `#include`s,
    /// nor, for a class, the system include files every class gets; the nearest known names
    /// follow as suggestions. IRIS refuses to compile the line (MPP5610). Fix the name, or
    /// `#include` (in a routine) or `Include` (in a class) the file that defines it.
    ///
    /// ```objectscript
    /// Class Demo.UndefinedMacro
    /// {
    ///
    /// ClassMethod Check(sc As %Status) As %Boolean
    /// {
    ///     Quit $$$ISOKAY(sc) //! A2797
    /// }
    ///
    /// }
    /// ```
    #[code = "A2797"]
    #[severity = Error]
    #[message = "The macro '{name}' is undefined{suggestions}"]
    UndefinedMacro {
        name: String,
        suggestions: Suggestions,
    },
    /// `Continue` starts the next iteration of the `For`, `While` or `Do ... While` loop around it,
    /// and none is around this one: an `If` or a `Try` block alone is no loop. IRIS refuses to
    /// compile the code (`CONTINUE is not within a loop`); `Quit` or `Return` leaves the method.
    ///
    /// ```objectscript
    /// Class Demo.ContinueNotInLoop
    /// {
    ///
    /// ClassMethod Keep(item As %String) As %Boolean
    /// {
    ///     If item = "" Continue //! A5789
    ///     Quit 1
    /// }
    ///
    /// }
    /// ```
    #[code = "A5789"]
    #[severity = Error]
    #[message = "CONTINUE is not within a loop"]
    ContinueNotInLoop,
    /// Two members of one kind share a name (two properties `Name`, say), so one would silently
    /// replace the other. IRIS refuses to load the class (`Element name conflict`). The later
    /// declaration is marked: rename it, or merge the two.
    ///
    /// ```objectscript
    /// Class Demo.DuplicatedMemberName Extends %RegisteredObject
    /// {
    ///
    /// Property Name As %String;
    ///
    /// Property Name As %String(MAXLEN = 100); //! A3119
    ///
    /// }
    /// ```
    #[code = "A3119"]
    #[severity = Error]
    #[message = "The member name {name} is defined twice"]
    DuplicatedMemberName { name: String },
    /// Another open document declares a class of this name, so the workspace holds two definitions
    /// and whichever claimed the name last answers for it wherever the class is looked up. IRIS
    /// keeps one definition per class name, so loading both makes the second overwrite the first.
    /// Keep one file, or rename one class.
    #[code = "A3717"]
    #[severity = Warning]
    #[message = "Class '{name}' is also declared in {other_uri}"]
    DuplicateClassName { name: String, other_uri: String },
    /// A method body, or an argument default in `{braces}`, that doesn't parse; the rest of the
    /// body goes unchecked. IRIS refuses to compile the class. The message names the mistake where
    /// it is clear, such as an unknown command or a function given the wrong arguments, and
    /// otherwise lists what could come next at that point; correct the marked line.
    ///
    /// ```objectscript
    /// Class Demo.InvalidSyntax
    /// {
    ///
    /// ClassMethod Greet(name As %String)
    /// {
    ///     Wrte "Hello, ", name, ! //! A8073
    /// }
    ///
    /// }
    /// ```
    #[code = "A8073"]
    #[severity = Error]
    #[message = "{err}"]
    InvalidSyntax { err: SyntaxErrorText },
    /// A class header or member declaration, or a line of a routine or include file, that doesn't
    /// parse; a method body that doesn't is A8073 instead, and the message reads the same way.
    /// Nothing else in the document is checked, and IRIS refuses to compile it. Correct the marked
    /// line.
    ///
    /// ```objectscript rejected
    /// Class Demo.DocumentSyntaxError
    /// {
    ///
    /// Property Name As %String [ Required ; //! A1727
    ///
    /// }
    /// ```
    #[code = "A1727"]
    #[severity = Warning]
    #[message = "{err}"]
    DocumentSyntaxError { err: SyntaxErrorText },
    /// The body of a label, or a routine's main body, runs off its end without a `Quit` or
    /// `Return`, and no label follows for it to fall into: it is the last, or a procedure block
    /// comes next. IRIS compiles it and quits there implicitly, so the code runs; an explicit `Quit`
    /// says the body ends on purpose.
    ///
    /// ```objectscript compiles
    /// ROUTINE Demo.MissingQuit
    /// Main
    ///     Do Report
    ///     Write "done", !
    ///     Quit
    /// Report
    ///     Write "reporting", ! //! A7548
    /// ```
    #[code = "A7548"]
    #[severity = Warning]
    #[message = "Missing an explicit QUIT or RETURN; execution falls through to whatever follows"]
    MissingQuit,
    /// A form IRIS refuses where it stands, named with IRIS's own error: an error trap set inside
    /// a `Try` block, a `*` or routine-qualified `$ZTRAP` value in a procedure block, an offset in
    /// a trap label, a `New` of a special variable other than `$ESTACK`, `$ETRAP`, `$NAMESPACE`
    /// and `$ROLES`. `ZQUIT` in a procedure block with private variables is the one IRIS compiles,
    /// to raise `<COMMAND>` when it runs. In the example the `Catch` already handles the error, so
    /// the trap goes.
    ///
    /// ```objectscript
    /// Class Demo.NotAllowedIn
    /// {
    ///
    /// ClassMethod Parse(text As %String) As %Integer
    /// {
    ///     Try {
    ///         Set $ZTrap = "Fail" //! A4304
    ///         Return +text
    ///     } Catch ex {
    ///         Return -1
    ///     }
    /// Fail
    ///     Return 0
    /// }
    ///
    /// }
    /// ```
    #[code = "A4304"]
    #[severity = Error]
    #[message = "{what} is not allowed in {context} (IRIS: {iris})"]
    NotAllowedIn {
        what: String,
        context: &'static str,
        iris: &'static str,
    },
    /// `ZQUIT` takes an integer of 1 or more; a literal below 1, such as `0`, a negative or a
    /// non-number, acts as a bare `ZQUIT` and clears the whole stack, and a fraction is truncated,
    /// so `1.5` is `1`. IRIS compiles it. Write the integer meant, or a bare `ZQUIT` if clearing
    /// the stack is what is meant.
    ///
    /// ```objectscript compiles
    /// ROUTINE Demo.ZQuitArgument
    /// Main
    ///     Set $ZTrap = "Fail"
    ///     Do Work
    ///     Write "done", !
    ///     Quit
    /// Work
    ///     Set value = ^Demo.ZQuitArgument("missing")
    ///     Write value, !
    ///     Quit
    /// Fail
    ///     Write "failed: ", $ZError, !
    ///     ZQuit 0 //! A6613
    ///     Quit
    /// ```
    #[code = "A6613"]
    #[severity = Warning]
    #[message = "ZQUIT wants an integer of 1 or more; {value} acts as a bare ZQUIT and clears the whole stack"]
    ZQuitArgument { value: String },
    /// An error raised on purpose (`ZTrap`, `Throw` outside a `Try`, a non-empty `$ECODE`) inside
    /// the handler the trap still names, or a `$ETRAP` command that raises one. IRIS compiles it,
    /// and the error enters the same handler again, and so on without end. Clearing the trap first
    /// (`Set $ZTrap = ""`) lets the error go up to the caller.
    ///
    /// ```objectscript compiles
    /// ROUTINE Demo.TrapReenters
    /// Main
    ///     Set $ZTrap = "Fail"
    ///     Set rate = ^Demo.TrapReenters("rate")
    ///     Write rate, !
    ///     Quit
    /// Fail
    ///     Write "no rate: ", $ZError, !
    ///     ZTrap "RATE" //! A4344
    /// ```
    #[code = "A4344"]
    #[severity = Warning]
    #[message = "An error here re-enters the handler {var} still names; set {var} to \"\" first"]
    TrapReenters { var: &'static str },
    /// This marks a limit of ascot, not a fault in the code. ascot follows error handling only in
    /// the forms it can read: a `Try`/`Catch`, a literal `$ZTRAP` label, a `$ETRAP` that is a plain
    /// `Goto Label`. A `$ETRAP` command beyond that, or a `$ZTRAP`, `$ETRAP`, `$ECODE` or `ZQUIT`
    /// value that isn't a literal, is taken on trust, so what happens when an error comes goes
    /// unchecked. IRIS compiles `$ETRAP` code only when it runs, and in a procedure block refuses a
    /// non-literal `$ZTRAP` value outright. Nothing here needs changing for IRIS; a literal label
    /// or a `Try` block would let ascot check the handling.
    ///
    /// ```objectscript compiles
    /// ROUTINE Demo.NotChecked
    /// Main
    ///     Set $ETrap = "Do Report Quit" //! A3913
    ///     Set rate = ^Demo.NotChecked("rate")
    ///     Write rate, !
    ///     Quit
    /// Report
    ///     Write "no rate: ", $ZError, !
    ///     Quit
    /// ```
    #[code = "A3913"]
    #[severity = Hint]
    #[message = "{what} is not checked"]
    NotChecked { what: &'static str },
    /// `ZQUIT` in a procedure block without private variables, where IRIS lets it unwind; the same
    /// block with any, an argument or a local, fails with `<COMMAND>` when it runs. The code works
    /// as it stands; the hint points at that fragility, since a variable added later changes the
    /// block's error handling.
    ///
    /// ```objectscript compiles
    /// Class Demo.ZQuitWithoutPrivates
    /// {
    ///
    /// ClassMethod Unwind()
    /// {
    ///     Write "unwinding", !
    ///     ZQuit 1 //! A5780
    /// }
    ///
    /// }
    /// ```
    #[code = "A5780"]
    #[severity = Hint]
    #[message = "ZQUIT unwinds here only because this procedure block has no private variables; one with any fails with <COMMAND>, so its error handling may not be what you expect"]
    ZQuitWithoutPrivates,
    /// A `$ETRAP` whose code is a single `Goto Label` does what `Set $ZTRAP = "Label"` does, and
    /// ascot checks it as that handler, which is how it can check it at all. The two differ in
    /// scope: `$ZTRAP` is this frame's handler, `$ETRAP` is inherited by the frames it calls. IRIS
    /// compiles and runs either. If the inherited scope is wanted, the code is right as it stands;
    /// if only this frame is meant, `$ZTRAP` says so.
    ///
    /// ```objectscript compiles
    /// ROUTINE Demo.ETrapGoto
    /// Main
    ///     Set $ETrap = "Goto Fail" //! A1765
    ///     Set rate = ^Demo.ETrapGoto("rate")
    ///     Write rate, !
    ///     Quit
    /// Fail
    ///     Write "no rate: ", $ZError, !
    ///     Quit
    /// ```
    #[code = "A1765"]
    #[severity = Hint]
    #[message = "A $ETRAP that only jumps to a label is what `Set $ZTrap = \"{label}\"` does; $ZTRAP is scoped to this frame, where $ETRAP is inherited by callees"]
    ETrapGoto { label: String },
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

/// What replaces a legacy `$ZUTIL` function, if anything.
#[derive(Debug)]
pub struct Replacement(pub Option<String>);

impl std::fmt::Display for Replacement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(replacement) => write!(f, "is superseded: use {replacement}"),
            None => f.write_str("is deprecated, with no replacement"),
        }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mode(pub Option<RefMode>);

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            None => "a normal",
            Some(RefMode::Output) => "an output",
            Some(RefMode::ByRef) => "a by-ref",
        })
    }
}

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

/// A class in a hierarchy that can't be read: its workspace file doesn't parse, or nothing holds it.
#[derive(Debug)]
pub enum UnseenClass {
    Unparsed(String),
    Missing(String),
}

impl std::fmt::Display for UnseenClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unparsed(cls) => write!(f, "{cls}, whose file in the workspace doesn't parse"),
            Self::Missing(cls) => {
                write!(
                    f,
                    "{cls}, which is not in the workspace or on the IRIS instance"
                )
            }
        }
    }
}

impl std::fmt::Display for SuperclassOf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0 { "a superclass of " } else { "" })
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

/// "no arguments", "1 argument", "2 arguments".
#[derive(Debug)]
pub struct ArgCount(pub usize);

impl std::fmt::Display for ArgCount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            0 => f.write_str("no arguments"),
            1 => f.write_str("1 argument"),
            n => write!(f, "{n} arguments"),
        }
    }
}

impl std::fmt::Display for Detail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(text) => write!(f, ": {text}"),
            None => Ok(()),
        }
    }
}

/// Whether a brace style wants a line break at a spot (`true`) or none (`false`).
#[derive(Debug)]
pub struct Break(pub bool);

impl std::fmt::Display for Break {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0 {
            "a line break"
        } else {
            "no line break"
        })
    }
}

/// The `space-after-comma` style: one space after and none before, or none on either side.
#[derive(Debug)]
pub struct CommaSpace(pub bool);

impl std::fmt::Display for CommaSpace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0 {
            "one space after this comma, and none before"
        } else {
            "no space around this comma"
        })
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
            // The parser's own words: `parse error at ..., expected: ...`.
            write!(f, "{error}")
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

    pub fn too_many_arguments(takes: usize, range: Range) -> Self {
        QuasiTypeError::too_many_arguments(takes).into_error(|| range)
    }

    // Built with a range directly, unlike `QuasiTypeError`'s deferred ones — there's no `Flow`
    // yet to attach one lazily this early (during macro expansion).
    pub fn label_in_block(label: String, range: Range) -> Self {
        Self::at(TypeErrorKind::LabelInBlock { label }, range)
    }

    pub(crate) fn cannot_find_class(cls: String, range: Range) -> Self {
        Self::at(TypeErrorKind::CannotFindClass { cls }, range)
    }

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
        unseen: UnseenClass,
        range: Range,
    ) -> Self {
        Self::at(
            TypeErrorKind::UnconfirmedMember {
                cls,
                mem,
                on: SuperclassOf(superclass),
                unseen,
            },
            range,
        )
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
        Self::at(
            TypeErrorKind::DocumentSyntaxError {
                err: SyntaxErrorText(err),
            },
            range,
        )
    }

    pub fn cannot_find_include(name: String, range: Range) -> Self {
        Self::at(TypeErrorKind::CannotFindInclude { name }, range)
    }

    pub fn argument_postconditional(range: Range) -> Self {
        Self::at(TypeErrorKind::ArgumentPostconditional, range)
    }

    pub fn and_operator(wanted: crate::bindings::AndOperator, range: Range) -> Self {
        Self::at(TypeErrorKind::AndOperator { wanted }, range)
    }

    pub fn new_per_block(max: u32, range: Range) -> Self {
        Self::at(TypeErrorKind::NewPerBlock { max }, range)
    }

    pub fn undescribed(kind: &str, lines: usize, range: Range) -> Self {
        Self::at(
            TypeErrorKind::Undescribed {
                kind: kind.to_string(),
                lines,
            },
            range,
        )
    }

    pub fn commands_per_line(max: u32, range: Range) -> Self {
        Self::at(TypeErrorKind::CommandsPerLine { max }, range)
    }

    pub fn open_brace_break(newline: bool, range: Range) -> Self {
        let wanted = Break(newline);
        Self::at(TypeErrorKind::OpenBraceBreak { wanted }, range)
    }

    pub fn body_break(newline: bool, range: Range) -> Self {
        let wanted = Break(newline);
        Self::at(TypeErrorKind::BodyBreak { wanted }, range)
    }

    pub fn close_brace_break(newline: bool, range: Range) -> Self {
        let wanted = Break(newline);
        Self::at(TypeErrorKind::CloseBraceBreak { wanted }, range)
    }

    pub fn chain_break(keyword: &'static str, newline: bool, range: Range) -> Self {
        let wanted = Break(newline);
        Self::at(TypeErrorKind::ChainBreak { keyword, wanted }, range)
    }

    pub fn comma_spacing(space_after: bool, range: Range) -> Self {
        let wanted = CommaSpace(space_after);
        Self::at(TypeErrorKind::CommaSpacing { wanted }, range)
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

    pub fn macro_too_few_arguments(name: String, expected: usize, range: Range) -> Self {
        Self::at(
            TypeErrorKind::MacroTooFewArguments { name, expected },
            range,
        )
    }

    pub fn mismatch_argument_mode(
        name: String,
        wanted: Option<RefMode>,
        actual: Option<RefMode>,
        range: Range,
        declared_at: Option<Location>,
    ) -> Self {
        // The call works as written (the caller's `.` decides); the annotation is what lets the
        // checker follow the variable, so ask for it without sending anyone after the `.`.
        let message = if wanted.is_none() && actual == Some(RefMode::ByRef) {
            format!(
                "Parameter declared here. If the `.` in the call is intended, add `&{name}` for a \
                 label or `ByRef`/`Output` for a method."
            )
        } else {
            "Parameter declared here.".to_string()
        };
        Self::new(
            TypeErrorKind::MismatchArgumentAndArgumentMode {
                name,
                wanted: Mode(wanted),
                actual: Mode(actual),
            },
            range,
            declared_at
                .map(|location| related(location, &message))
                .unwrap_or_default(),
        )
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

    pub fn by_ref_never_read(name: String, range: Range) -> TypeError {
        Self::at(TypeErrorKind::ByRefNeverRead { name }, range)
    }

    pub fn output_read_first(name: String, range: Range) -> TypeError {
        Self::at(TypeErrorKind::OutputReadFirst { name }, range)
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
        kind: tygr_objectscript::cos::KeywordKind,
        found: String,
        expected: String,
        range: Range,
    ) -> Self {
        Self::at(
            TypeErrorKind::KeywordStyle {
                kind,
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

    pub fn label_offset(range: Range) -> Self {
        Self::at(TypeErrorKind::LabelOffset, range)
    }

    pub fn unrecognized_class_keyword(entry: String, range: Range) -> Self {
        Self::at(TypeErrorKind::UnrecognizedClassKeyword { entry }, range)
    }

    pub fn not_allowed_in(
        what: impl Into<String>,
        context: &'static str,
        iris: &'static str,
        range: Range,
    ) -> Self {
        let what = what.into();
        Self::at(
            TypeErrorKind::NotAllowedIn {
                what,
                context,
                iris,
            },
            range,
        )
    }

    pub fn zquit_argument(value: String, range: Range) -> Self {
        Self::at(TypeErrorKind::ZQuitArgument { value }, range)
    }

    pub fn trap_reenters(var: &'static str, range: Range) -> Self {
        Self::at(TypeErrorKind::TrapReenters { var }, range)
    }

    pub fn not_checked(what: &'static str, range: Range) -> Self {
        Self::at(TypeErrorKind::NotChecked { what }, range)
    }

    pub fn etrap_goto(label: String, range: Range) -> Self {
        Self::at(TypeErrorKind::ETrapGoto { label }, range)
    }

    pub fn zquit_without_privates(range: Range) -> Self {
        Self::at(TypeErrorKind::ZQuitWithoutPrivates, range)
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

    pub fn unrecognized_keyword(kind: &'static str, entry: String, range: Range) -> Self {
        Self::at(TypeErrorKind::UnrecognizedKeyword { kind, entry }, range)
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
            // `S` is a style rule, `A` everything else.
            let digits = (code.strip_prefix('A').or_else(|| code.strip_prefix('S')))
                .unwrap_or_else(|| panic!("{code}"));
            assert!(
                digits.len() == 4 && digits.bytes().all(|b| b.is_ascii_digit()),
                "{code}"
            );
        }
    }
}
