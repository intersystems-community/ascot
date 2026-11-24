//! `Desugar`: a node of the full SQL grammar into its copy in the core grammar, the sugar rewritten
//! (see `sql`). The grammar's own types get their impls from the `levels` attribute, each `#[sugar]`
//! mark naming its rule here; the leaves both levels share and tygr's containers get theirs below.
//! Each rule's spelling was measured against its core spelling on IRIS 2026.2.

use std::ops::Deref;

use either::Either;
use tygr::{
    Grammar, Hidden, NotFollowedBy, Prefix, Print, Ranged, Raw, StringEq, StringEqCI, Suffix, Vec1,
    Wrap,
};

use super::{core, full, *};
use crate::convert::VecSep;
use crate::cos::{MacroArgUnit, OSText};

pub trait Desugar {
    type Core;
    fn desugar(&self) -> Self::Core;
}

/// The types both levels share, each its own core.
macro_rules! identity {
    ($($ty:ty),* $(,)?) => {
        $(impl Desugar for $ty {
            type Core = Self;
            fn desugar(&self) -> Self {
                self.clone()
            }
        })*
    };
}

identity!(
    (),
    IsSpace,
    IsSimpleLatinLetter,
    IsNonAsciiLetter,
    IsNotDQ,
    IsNotSQ,
    IsDigit,
    IsNotNL,
    IsNotStar,
    IsAnyChar,
    IsExtFuncChar,
    IsDbIdentStart,
    IsDbIdentPart,
    IsNotParen,
    SpaceUnit,
    Space,
    Ident,
    SQLIdentNonKeyword,
    IdentifierStart,
    IdentifierPart,
    IdentifierStruct,
    Identifier,
    SQLDelimitedIdentStruct,
    DelimitedIdentifierPart,
    DelimitedIdentifier,
    DottedIdent,
    StdCharacterRepresentation,
    StringLiteral,
    Integer,
    SQLExponent,
    ExpoSymbol,
    Number,
    NumberBase,
    Literal,
    ParameterMarker,
    HostVar,
    InstanceVariableHostRef,
    PropertyHostRef,
    Var,
    HostRef,
    HostIdentifier,
    HostIdentifierPart,
    Param,
    Sub,
    MacroArgUnit,
    OSText,
);

macro_rules! tuples {
    ($(($($t:ident $i:tt),+))*) => {
        $(impl<$($t: Desugar),+> Desugar for ($($t,)+) {
            type Core = ($($t::Core,)+);
            fn desugar(&self) -> Self::Core {
                ($(self.$i.desugar(),)+)
            }
        })*
    };
}

tuples! {
    (A 0, B 1)
    (A 0, B 1, C 2)
    (A 0, B 1, C 2, D 3)
    (A 0, B 1, C 2, D 3, E 4)
    (A 0, B 1, C 2, D 3, E 4, F 5)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10)
}

impl<T: Desugar> Desugar for Option<T> {
    type Core = Option<T::Core>;
    fn desugar(&self) -> Self::Core {
        self.as_ref().map(Desugar::desugar)
    }
}

impl<T: Desugar> Desugar for Vec<T> {
    type Core = Vec<T::Core>;
    fn desugar(&self) -> Self::Core {
        self.iter().map(Desugar::desugar).collect()
    }
}

impl<T: Desugar> Desugar for Vec1<T> {
    type Core = Vec1<T::Core>;
    fn desugar(&self) -> Self::Core {
        Vec1::try_from((**self).desugar())
            .ok()
            .expect("one item or more")
    }
}

impl<T: Desugar> Desugar for Box<T> {
    type Core = Box<T::Core>;
    fn desugar(&self) -> Self::Core {
        Box::new((**self).desugar())
    }
}

impl<L: Desugar, R: Desugar> Desugar for Either<L, R> {
    type Core = Either<L::Core, R::Core>;
    fn desugar(&self) -> Self::Core {
        match self {
            Either::Left(left) => Either::Left(left.desugar()),
            Either::Right(right) => Either::Right(right.desugar()),
        }
    }
}

impl<T: Desugar, S: Desugar> Desugar for VecSep<T, S> {
    type Core = VecSep<T::Core, S::Core>;
    fn desugar(&self) -> Self::Core {
        VecSep {
            items: self.items.desugar(),
            seps: self.seps.desugar(),
        }
    }
}

impl<P: Desugar, T: Desugar, S: Desugar> Desugar for Wrap<P, T, S> {
    type Core = Wrap<P::Core, T::Core, S::Core>;
    fn desugar(&self) -> Self::Core {
        Wrap::new(
            self.prefix.desugar(),
            self.wrapped.desugar(),
            self.suffix.desugar(),
        )
    }
}

impl<P: Desugar, T: Desugar> Desugar for Prefix<P, T> {
    type Core = Prefix<P::Core, T::Core>;
    fn desugar(&self) -> Self::Core {
        Prefix::new(self.prefix.desugar(), self.prefixed.desugar())
    }
}

impl<T: Desugar, S: Desugar> Desugar for Suffix<T, S> {
    type Core = Suffix<T::Core, S::Core>;
    fn desugar(&self) -> Self::Core {
        Suffix::new(self.suffixed.desugar(), self.suffix.desugar())
    }
}

/// Covers `StringOf` and `StringOf1`, aliases of `Raw`.
impl<T: Desugar> Desugar for Raw<T> {
    type Core = Raw<T::Core>;
    fn desugar(&self) -> Self::Core {
        Raw::new(self.as_str())
    }
}

impl<T: Desugar> Desugar for Hidden<T> {
    type Core = Hidden<T::Core>;
    fn desugar(&self) -> Self::Core {
        Hidden(self.0.desugar())
    }
}

impl<T: Desugar> Desugar for Ranged<T> {
    type Core = Ranged<T::Core>;
    fn desugar(&self) -> Self::Core {
        Ranged::new(self.start, self.ranged.desugar(), self.end)
    }
}

impl<T: Desugar> Desugar for Src<T> {
    type Core = Src<T::Core>;
    fn desugar(&self) -> Self::Core {
        Src::new(self.range(), self.rewriting(|| self.node.desugar()))
    }
}

impl<G: Desugar> Desugar for NotFollowedBy<G> {
    type Core = NotFollowedBy<G::Core>;
    fn desugar(&self) -> Self::Core {
        NotFollowedBy::default()
    }
}

type Open = (Space, StringEq!("("), Space);
type Close = (Space, StringEq!(")"));

/// A term as an expression of its own, standing for the node being rewritten.
fn term(term: core::Term) -> core::Scalx {
    core::Scalx::Legacy(VecSep::one(Src::synthesized(term)))
}

/// `'text'`.
fn string(text: &str) -> core::Scalx {
    let literal = StringLiteral::Std(Raw::new(format!("'{text}'")));
    term(core::Term::Literal(Literal::String(literal)))
}

/// `name(args)`, the call by name.
fn call(name: &str, args: core::ArgsN) -> core::Term {
    let name = DottedIdent {
        head: Ident::simple(name),
        rest: Vec::new(),
    };
    core::Term::Call(core::Call {
        name: core::SQLFunctionName(Src::synthesized(name)),
        args,
    })
}

/// `()`.
fn empty() -> core::ArgsN {
    Wrap::new(
        (Space::default(), Default::default(), Space::default()),
        None,
        (Space::default(), Default::default()),
    )
}

/// An argument of a call.
trait Arg {
    fn arg(&self) -> core::Scalx;

    /// The comments of the tokens the argument loses, which go before it.
    fn before(&self) -> Space {
        Space::default()
    }
}

impl Arg for full::Scalx {
    fn arg(&self) -> core::Scalx {
        self.desugar()
    }
}

impl Arg for Src<full::Term> {
    fn arg(&self) -> core::Scalx {
        core::Scalx::Legacy(VecSep::one(self.desugar()))
    }
}

/// A datepart keyword is its full name in a string literal; a literal or host variable stays.
fn datepart(part: &full::DatePartCommon) -> core::Scalx {
    use full::DatePartCommon::*;
    string(match part {
        Year(_) => "year",
        Quarter(_) => "quarter",
        Month(_) => "month",
        Week(_) => "week",
        Weekday(_) => "weekday",
        DayOfYear(_) => "dayofyear",
        Day(_) => "day",
        Hour(_) => "hour",
        Minute(_) => "minute",
        Second(_) => "second",
        Millisecond(_) => "millisecond",
        Microsecond(_) => "microsecond",
        Nanosecond(_) => "nanosecond",
        Literal(literal) => return term(core::Term::Literal(literal.clone())),
        HostVar(host_var) => return term(core::Term::HostVar(host_var.clone())),
    })
}

/// A parenthesized datepart loses its parentheses: the part, and the comments the parentheses held.
fn unwrapped<T: Grammar>(part: &full::OptionallyWrapped<T>) -> (&T, Space) {
    match part {
        full::OptionallyWrapped::Wrapped(parens) => {
            let comments = Space::default()
                .join(&parens.prefix.1)
                .join(&parens.suffix.0);
            (&parens.wrapped, comments)
        }
        full::OptionallyWrapped::Scalar(part) => (part, Space::default()),
    }
}

impl Arg for full::DatePart {
    fn arg(&self) -> core::Scalx {
        datepart(unwrapped(self).0)
    }

    fn before(&self) -> Space {
        unwrapped(self).1
    }
}

impl Arg for full::DatePartPart {
    fn arg(&self) -> core::Scalx {
        match unwrapped(&self.0).0 {
            full::DatePartPartRaw::SqlTimestamp(_) => string("sqltimestamp"),
            full::DatePartPartRaw::Common(common) => datepart(common),
        }
    }

    fn before(&self) -> Space {
        unwrapped(&self.0).1
    }
}

/// The argument list of a call; the comments an argument loses go after the opening parenthesis or
/// the comma before it.
fn list(open: &Open, args: &[&dyn Arg], seps: &[ArgSep], close: &Close) -> core::ArgsN {
    let (mut open, mut seps) = (open.clone(), seps.to_vec());
    let items: Vec<core::Scalx> = args
        .iter()
        .enumerate()
        .map(|(i, arg)| {
            let before = match i.checked_sub(1) {
                None => &mut open.2,
                Some(previous) => &mut seps[previous].2,
            };
            *before = before.join(&arg.before());
            arg.arg()
        })
        .collect();
    let list = (!items.is_empty()).then_some(VecSep { items, seps });
    Wrap::new(open, list, close.clone())
}

/// A comma in a dropped keyword's place, with the keyword's comments.
fn comma<K>((before, _, after): &(Space, K, Space)) -> ArgSep {
    (
        Space::default().join(before),
        Default::default(),
        Space::blank().join(after),
    )
}

/// A function's argument list as a call's.
trait Args {
    fn args(&self) -> core::ArgsN;
}

impl<T: Arg> Args for Wrap<Open, T, Close> {
    fn args(&self) -> core::ArgsN {
        list(&self.prefix, &[&self.wrapped], &[], &self.suffix)
    }
}

impl<T: Arg> Args for Wrap<Open, Option<T>, Close> {
    fn args(&self) -> core::ArgsN {
        let args: Vec<&dyn Arg> = self.wrapped.iter().map(|arg| arg as &dyn Arg).collect();
        list(&self.prefix, &args, &[], &self.suffix)
    }
}

impl Args for Wrap<Open, VecSep<full::Scalx, ArgSep>, Close> {
    fn args(&self) -> core::ArgsN {
        Wrap::new(
            self.prefix.clone(),
            Some(self.wrapped.desugar()),
            self.suffix.clone(),
        )
    }
}

impl Args for Wrap<Open, Option<VecSep<full::Scalx, ArgSep>>, Close> {
    fn args(&self) -> core::ArgsN {
        self.desugar()
    }
}

impl Args for full::Args0 {
    fn args(&self) -> core::ArgsN {
        list(&self.open, &[], &[], &self.close)
    }
}

impl<T1: Arg + Grammar, T2: Arg + Grammar> Args for full::ArgsRR<T1, T2> {
    fn args(&self) -> core::ArgsN {
        list(
            &self.open,
            &[&self.arg1, &self.arg2],
            std::slice::from_ref(&self.sep1),
            &self.close,
        )
    }
}

impl<T1: Arg + Grammar, T2: Arg + Grammar, T3: Arg + Grammar> Args for full::ArgsRRR<T1, T2, T3> {
    fn args(&self) -> core::ArgsN {
        list(
            &self.open,
            &[&self.arg1, &self.arg2, &self.arg3],
            &[self.sep1.clone(), self.sep2.clone()],
            &self.close,
        )
    }
}

impl<T1: Arg + Grammar, T2: Arg + Grammar, T3: Arg + Grammar, T4: Arg + Grammar> Args
    for full::ArgsRRRR<T1, T2, T3, T4>
{
    fn args(&self) -> core::ArgsN {
        list(
            &self.open,
            &[&self.arg1, &self.arg2, &self.arg3, &self.arg4],
            &[self.sep1.clone(), self.sep2.clone(), self.sep3.clone()],
            &self.close,
        )
    }
}

impl<T1: Arg + Grammar, T2: Arg + Grammar> Args for full::ArgsRO<T1, T2> {
    fn args(&self) -> core::ArgsN {
        let (mut args, mut seps): (Vec<&dyn Arg>, Vec<ArgSep>) = (vec![&self.arg1], Vec::new());
        if let Some((sep, arg2)) = &self.arg2 {
            seps.push(sep.clone());
            args.push(arg2);
        }
        list(&self.open, &args, &seps, &self.close)
    }
}

impl<T1: Arg + Grammar, T2: Arg + Grammar, T3: Arg + Grammar> Args for full::ArgsROO<T1, T2, T3> {
    fn args(&self) -> core::ArgsN {
        let (mut args, mut seps): (Vec<&dyn Arg>, Vec<ArgSep>) = (vec![&self.arg1], Vec::new());
        if let Some((sep, arg2, arg3)) = &self.rest {
            seps.push(sep.clone());
            args.push(arg2);
            if let Some((sep, arg3)) = arg3 {
                seps.push(sep.clone());
                args.push(arg3);
            }
        }
        list(&self.open, &args, &seps, &self.close)
    }
}

impl<T1: Arg + Grammar, T2: Arg + Grammar, T3: Arg + Grammar> Args for full::ArgsRRO<T1, T2, T3> {
    fn args(&self) -> core::ArgsN {
        let (mut args, mut seps): (Vec<&dyn Arg>, Vec<ArgSep>) =
            (vec![&self.arg1, &self.arg2], vec![self.sep1.clone()]);
        if let Some((sep, arg3)) = &self.arg3 {
            seps.push(sep.clone());
            args.push(arg3);
        }
        list(&self.open, &args, &seps, &self.close)
    }
}

impl<T1: Arg + Grammar, T2: Arg + Grammar, T3: Arg + Grammar, T4: Arg + Grammar> Args
    for full::ArgsRROO<T1, T2, T3, T4>
{
    fn args(&self) -> core::ArgsN {
        let (mut args, mut seps): (Vec<&dyn Arg>, Vec<ArgSep>) =
            (vec![&self.arg1, &self.arg2], vec![self.sep1.clone()]);
        if let Some((sep, arg3, arg4)) = &self.rest {
            seps.push(sep.clone());
            args.push(arg3);
            if let Some((sep, arg4)) = arg4 {
                seps.push(sep.clone());
                args.push(arg4);
            }
        }
        list(&self.open, &args, &seps, &self.close)
    }
}

/// `POSITION(a IN b)` has the arguments of `CHARINDEX(a, b)`.
impl Args for full::SQLPositionArgs {
    fn args(&self) -> core::ArgsN {
        list(
            &self.open,
            &[&self.needle, &self.haystack],
            &[comma(&self.in_kw)],
            &self.close,
        )
    }
}

/// `SUBSTRING(x FROM s FOR l)` has the arguments of `SUBSTRING(x, s, l)`.
impl Args for full::SubstringArgs {
    fn args(&self) -> core::ArgsN {
        use full::SubstringArgsInternal::*;
        let (mut args, mut seps): (Vec<&dyn Arg>, Vec<ArgSep>) = match &self.wrapped {
            Keyword {
                expr,
                from_kw,
                start,
                ..
            } => (vec![expr, start], vec![comma(from_kw)]),
            Position {
                expr, comma, start, ..
            } => (vec![expr, start], vec![comma.clone()]),
        };
        match &self.wrapped {
            Keyword {
                for_clause: Some((for_kw, length)),
                ..
            } => {
                seps.push(comma(for_kw));
                args.push(length);
            }
            Position {
                length: Some((sep, length)),
                ..
            } => {
                seps.push(sep.clone());
                args.push(length);
            }
            _ => {}
        }
        list(&self.prefix, &args, &seps, &self.suffix)
    }
}

/// The call a function's keyword and argument list spell; a niladic-looking one gets `()`.
macro_rules! calls {
    ($Enum:ident: $value:expr; $($Variant:ident)*; $($Niladic:ident)*; $($arm:tt)*) => {
        match $value {
            $(full::$Enum::$Variant(kw, args) => call(&kw.print(), args.args()),)*
            $(full::$Enum::$Niladic(kw, _) => call(&kw.print(), empty()),)*
            $($arm)*
        }
    };
}

/// A scalar function is the call by its name, `POSITION(a IN b)` the call `CHARINDEX(a, b)`.
pub fn function<F: Deref<Target = full::ScalarFunction>>(function: &F) -> core::Term {
    calls!(ScalarFunction: &**function;
        PercentObject PercentOid Similarity Abs Ascii Ceiling CharacterLength CharIndex CharLength
        Coalesce Convert DataLength Date DateAdd DateDiff DateName DatePart DateTrunc Day Decode
        Embedding Floor GetDate GetUtcDate Greatest HashBytes IfNull Instr IsNull IsNumeric LastDay
        LastIdentity LCase Least Left Len Length Lower LPad LTrim Month Now NullIf Nvl PatIndex Power
        Repeat Replace Replicate Reverse Right Round RPad RTrim Sign Space Str String Stuff Substr
        Substring ToChar ToDate ToUSChar ToUSDate ToNumber ToTimestamp UCase UnixTimestamp
        VectorCosine VectorDotProduct Year;;
        full::ScalarFunction::Position(_, args) => call("CHARINDEX", args.args()),
    )
}

/// `{fn NAME(args)}` is the call `NAME(args)`, `{fn PI}` and the like `PI()`, TIMESTAMPADD and
/// TIMESTAMPDIFF their builtins.
fn odbc_function(function: &full::ODBCFunc) -> core::Term {
    calls!(ODBCFunc: function;
        Abs Acos Asin Atan Ceiling Cos Cot Exp Floor Log Log10 Mod Power Sin Sqrt Tan Ascii Char
        Concat DayName DayOfMonth DayOfWeek DayOfYear Hour LCase Left Length LTrim Minute Month
        MonthName Quarter Repeat Right Round RTrim Second Sign Space Substring Truncate UCase Week
        Year;
        Pi CurDate CurTime Database Now;
        full::ODBCFunc::TimestampAdd(kw, args) => {
            core::Term::Builtin(Box::new(core::Builtin::TimestampAdd(*kw, args.desugar())))
        }
        full::ODBCFunc::TimestampDiff(kw, args) => {
            core::Term::Builtin(Box::new(core::Builtin::TimestampDiff(*kw, args.desugar())))
        }
    )
}

/// `{d x}`, `{t x}` and `{ts x}` are `CAST(x AS DATE)`, `CAST(x AS TIME)` and `CAST(x AS TIMESTAMP)`.
fn cast(after_brace: &Space, value: &full::DTSVal, before_brace: &Space) -> core::Term {
    let cast_type = match &value.dts_type {
        full::DTSType::Date(_) => core::CastType::Date(Default::default()),
        full::DTSType::Time(_) => core::CastType::Time(Default::default()),
        full::DTSType::Timestamp(_) => core::CastType::Timestamp(Default::default()),
    };
    let expr = term(match &value.val_spec {
        full::DTSValSpec::Literal(literal) => core::Term::Literal(literal.clone()),
        full::DTSValSpec::HostVar(host_var) => core::Term::HostVar(host_var.clone()),
    });
    let cast = core::SQLCastExpression {
        open: (
            Space::default(),
            Default::default(),
            Space::default().join(after_brace).join(&value.sp),
        ),
        expr,
        as_kw: (Space::blank(), Default::default(), Space::blank()),
        cast_type,
        close: (Space::default().join(before_brace), Default::default()),
    };
    core::Term::Builtin(Box::new(core::Builtin::Cast(
        Default::default(),
        Box::new(cast),
    )))
}

/// An ODBC escape is what it stands for (see `odbc_function` and `cast`); the comments within its
/// braces go before the parentheses of the call.
pub fn odbc_escape<E: Deref<Target = full::ODBCEscape>>(escape: &E) -> core::Term {
    let Wrap {
        prefix: (_, after_brace),
        wrapped,
        suffix: (before_brace, _),
    } = &**escape;
    let (after_fn, function) = match wrapped {
        full::DTS::DateTime(value) => return cast(after_brace, value, before_brace),
        full::DTS::Function(function) => (&function.prefix.1, &function.prefixed),
    };
    let mut term = odbc_function(function);
    let parens = match &mut term {
        core::Term::Call(call) => Some((&mut call.args.prefix.0, &mut call.args.suffix.0)),
        core::Term::Builtin(builtin) => match &mut **builtin {
            core::Builtin::TimestampAdd(_, args) => Some((&mut args.open.0, &mut args.close.0)),
            core::Builtin::TimestampDiff(_, args) => Some((&mut args.open.0, &mut args.close.0)),
            _ => None,
        },
        _ => None,
    };
    if let Some((before_open, before_close)) = parens {
        *before_open = before_open.join(after_brace).join(after_fn);
        *before_close = before_close.join(before_brace);
    }
    term
}

/// `%SQLUPPER x` is the call `%SQLUPPER(x)`; an `x` in parentheses gives the call its own.
pub fn percent_call<T: Deref<Target = Src<full::Term>>>(
    op: &full::PercentOp,
    space: &Space,
    term: &T,
) -> core::Term {
    let (open, arg, close) = match &term.node {
        full::Term::Scalx(parens) => (
            parens.prefix.1.clone(),
            (*parens.wrapped).desugar(),
            parens.suffix.0.clone(),
        ),
        _ => (Space::default(), (**term).arg(), Space::default()),
    };
    let args = Wrap::new(
        (Space::default().join(space), Default::default(), open),
        Some(VecSep::one(arg)),
        (close, Default::default()),
    );
    call(&op.print(), args)
}

/// `%TRUNCATE(x, n)` is the call, as `%STRING`, `%SQLSTRING` and `%SQLUPPER` with a truncation are.
pub fn string_operation<O: Deref<Target = full::StringOperation>>(operation: &O) -> core::Term {
    let full::StringOperation {
        op,
        open,
        expr,
        comma,
        truncation,
        close,
    } = &**operation;
    let truncation = match truncation {
        full::ColTrun::Const(constant) => {
            let core::PConst::Literal(literal) = constant.desugar();
            core::Term::Literal(literal)
        }
        full::ColTrun::HostVar(host_var) => core::Term::HostVar(host_var.clone()),
    };
    let args = VecSep {
        items: vec![expr.desugar(), term(truncation)],
        seps: vec![comma.clone()],
    };
    call(
        &op.print(),
        Wrap::new(open.clone(), Some(args), close.clone()),
    )
}

/// `$H` is `$HOROLOG`, and so on: an alias is its full name.
macro_rules! aliases {
    ($($rule:ident => $Variant:ident),* $(,)?) => {
        $(pub fn $rule<T>(_: &T) -> core::SpecialSystemVariable {
            core::SpecialSystemVariable::$Variant(Default::default())
        })*
    };
}

aliases!(
    horolog => Horolog,
    job => Job,
    tlevel => TLevel,
    zhorolog => ZHorolog,
    zjob => ZJob,
    namespace => Namespace,
    ztimestamp => ZTimestamp,
    ztimezone => ZTimezone,
    zversion => ZVersion,
);

/// `_` is `||`.
pub fn concat(_: &StringEq!("_")) -> core::Scalop {
    core::Scalop::Concat(Default::default())
}

/// `ROWS n PRECEDING` is `ROWS BETWEEN n PRECEDING AND CURRENT ROW`, the standard's reading of a
/// lone bound.
pub fn rows_between(
    rows_kw: &(Space, StringEqCI!("ROWS"), Space),
    bound: &full::WindowRowsBound,
) -> core::WindowRows {
    let current_row =
        core::WindowRowsBound::CurrentRow(Default::default(), Space::blank(), Default::default());
    core::WindowRows::Between {
        rows_kw: rows_kw.clone(),
        between_kw: Default::default(),
        start: (Space::blank(), bound.desugar()),
        end: Prefix::new(
            (Space::blank(), Default::default(), Space::blank()),
            current_row,
        ),
    }
}

/// `TRIM(LEADING FROM x)` and `TRIM(LEADING x)` are `TRIM(LEADING ' ' FROM x)`.
pub fn trim_blank(
    spec: &full::TrimSpec,
    sp: &Space,
    from: &Option<(StringEqCI!("FROM"), Space)>,
    expr: &full::Scalx,
) -> core::TrimArgs {
    let (from_kw, after) = match from {
        Some((from_kw, after)) => (*from_kw, after.clone()),
        None => (Default::default(), Space::blank()),
    };
    core::TrimArgs::SpecScalxScalx(
        spec.desugar(),
        sp.clone(),
        string(" "),
        Space::blank(),
        from_kw,
        after,
        expr.desugar(),
    )
}

/// `TRIM(c FROM x)` is `TRIM(BOTH c FROM x)`, `TRIM(x)` `TRIM(BOTH ' ' FROM x)`.
pub fn trim_both(
    chars: &Option<(full::Scalx, Space, StringEqCI!("FROM"), Space)>,
    expr: &full::Scalx,
) -> core::TrimArgs {
    let (chars, sp, from_kw, after) = match chars {
        Some((chars, sp, from_kw, after)) => (chars.desugar(), sp.clone(), *from_kw, after.clone()),
        None => (
            string(" "),
            Space::blank(),
            Default::default(),
            Space::blank(),
        ),
    };
    core::TrimArgs::SpecScalxScalx(
        core::TrimSpec::Both(Default::default()),
        Space::blank(),
        chars,
        sp,
        from_kw,
        after,
        expr.desugar(),
    )
}

/// `!=` and `NOT=` are `<>`.
pub fn diamond<T>(_: &T) -> core::NotEqualsOperation {
    core::NotEqualsOperation::Diamond(Default::default())
}

/// `NOT>` is `<=`.
pub fn le<T>(_: &T) -> core::OrderPredicate {
    core::OrderPredicate::Le(Default::default())
}

/// `NOT<` is `>=`.
pub fn ge<T>(_: &T) -> core::OrderPredicate {
    core::OrderPredicate::Ge(Default::default())
}

/// `NOT[` reads as `[` under the `NOT` that `cond` adds.
pub fn contains<T>(_: &T) -> core::ContainsPredicate {
    core::ContainsPredicate::LB(Default::default())
}

/// `NOT]` reads as `]` under the `NOT` that `cond` adds.
pub fn follows<T>(_: &T) -> core::FollowsPredicate {
    core::FollowsPredicate::Follows(Default::default())
}

/// A negated keyword reads as the keyword, under the `NOT` that `cond` adds.
pub fn plain<K: Desugar>(_: &full::NotKw, _: &Space, keyword: &K) -> core::Negatable<K::Core>
where
    K::Core: Grammar,
{
    core::Negatable::Plain(keyword.desugar())
}

/// `IN (subquery)` is `= ANY (subquery)`.
pub fn in_any(_: &full::Negatable<StringEqCI!("IN")>) -> core::QuantifiedOp {
    core::QuantifiedOp::Op {
        op: core::CmpOp::Eq(Default::default()),
        sp: Space::blank(),
        quantifier: core::Quantifier::Any(Default::default()),
    }
}

/// `SOME` is `ANY`.
pub fn any<T>(_: &T) -> core::Quantifier {
    core::Quantifier::Any(Default::default())
}

/// `!` is `OR`.
pub fn or<T>(_: &T) -> core::OrOperator {
    core::OrOperator::Or(Default::default())
}

/// `&` is `AND`.
pub fn and<T>(_: &T) -> core::AndOperator {
    core::AndOperator::And(Default::default())
}

/// A condition: its predicate, in `NOT (...)` when the predicate is negated.
pub fn cond(cond: &full::Cond) -> core::Cond {
    let predicate = predicate(cond);
    match negation(cond) {
        None => predicate,
        Some(not_kw) => core::Cond::Not {
            not_kw,
            condition: Box::new(Src::synthesized(core::Cond::Paren {
                open: (Default::default(), Space::default()),
                predicate: Box::new(core::Prdct(VecSep::one(core::And(VecSep::one(
                    Src::synthesized(predicate),
                ))))),
                close: (Space::default(), Default::default()),
            })),
        },
    }
}

/// The `NOT` of a negated predicate with the blank after it: the keyword's own, or one made for
/// `NOT[` and `NOT]`.
fn negation(cond: &full::Cond) -> Option<(full::NotKw, Space)> {
    use full::{Cond::*, QuantifiedOp};
    let keyword = match cond {
        IsNull { null_kw, .. } => null_kw.negation(),
        Between { between_kw, .. } => between_kw.negation(),
        Like { like_kw, .. } => like_kw.negation(),
        Matches { matches_kw, .. } => matches_kw.negation(),
        In { in_kw, .. } => in_kw.negation(),
        InList { inlist_kw, .. } => inlist_kw.negation(),
        IsJson { json_kw, .. } => json_kw.negation(),
        Quantified {
            op: QuantifiedOp::In(in_kw),
            ..
        } => in_kw.negation(),
        Compare { op, .. }
        | Quantified {
            op: QuantifiedOp::Op { op, .. },
            ..
        } if negated(op) => return Some((Default::default(), Space::blank())),
        _ => None,
    };
    keyword.map(|(not, sp)| (*not, sp.clone()))
}

/// `NOT[` or `NOT]`, which read as `[` and `]` under the `NOT` that `cond` adds.
fn negated(op: &full::CmpOp) -> bool {
    use full::{CmpOp, ContainsPredicate, FollowsPredicate};
    matches!(
        op,
        CmpOp::Contains(ContainsPredicate::NotLB(_))
            | CmpOp::Follows(FollowsPredicate::NotFollows(_))
    )
}

/// Field by field, as an unmarked type converts.
macro_rules! variants {
    ($value:expr; $($Variant:ident { $($field:ident),* })*) => {
        match $value {
            $(full::Cond::$Variant { $($field),* } => core::Cond::$Variant { $($field: $field.desugar()),* },)*
        }
    };
}

/// The predicate, its negation read as its keyword (see `Negatable`). IRIS applies a quantified
/// `NOT[` or `NOT]` to each row (measured on 2026.2), so under the `NOT` its quantifier flips:
/// `x NOT[ ANY (q)` is `NOT (x [ ALL (q))`.
fn predicate(cond: &full::Cond) -> core::Cond {
    let mut predicate = variants!(cond;
        Not { not_kw, condition }
        Paren { open, predicate, close }
        Exists { noindex, exists_kw, query }
        ForSomeElement { noindex, for_kw, percent_element, open_paren1, column, close_paren1, label, open_paren2, predicate, close_paren2 }
        ForSome { noindex, for_kw, some_kw, open, tables, close, condition }
        Compare { noindex, lhs, sp1, op, sp2, rhs }
        Quantified { noindex, lhs, sp1, op, sp2, subquery }
        IsNull { noindex, lhs, sp, is_kw, null_kw }
        Between { noindex, lhs, sp, between_kw, lower, and_kw, upper }
        Like { noindex, lhs, sp, like_kw, pattern, escape }
        Matches { noindex, lhs, sp, matches_kw, pattern, escape }
        In { noindex, lhs, sp, in_kw, list }
        InList { noindex, lhs, sp, inlist_kw, list_expr, size_hint }
        Find { noindex, lhs, sp, find_kw, expr, size_hint }
        Pattern { noindex, lhs, sp, pattern_kw, pattern }
        IsJson { noindex, lhs, sp, format, is_kw, json_kw, constraint }
        Contains { noindex, lhs, sp, kw, list }
        StarEq { noindex, lhs, sp, op, rhs }
        EqStar { noindex, lhs, sp, op, rhs }
    );
    if let full::Cond::Quantified {
        op: full::QuantifiedOp::Op { op, .. },
        ..
    } = cond
        && negated(op)
        && let core::Cond::Quantified {
            op: core::QuantifiedOp::Op { quantifier, .. },
            ..
        } = &mut predicate
    {
        *quantifier = match quantifier {
            core::Quantifier::All(_) => core::Quantifier::Any(Default::default()),
            core::Quantifier::Any(_) => core::Quantifier::All(Default::default()),
        };
    }
    predicate
}

/// `SELECT ALL` is `SELECT`: the blank after ALL joins the one before it.
pub fn selex(s: &full::Selex) -> core::Selex {
    let (mut kw, mut options) = (s.kw.desugar(), s.options.desugar());
    if let Some((_, _, after)) = &s.all {
        let before = options.last_mut().map_or(&mut kw.1, trailing);
        *before = before.join(after);
    }
    core::Selex {
        kw,
        options,
        selst: s.selst.desugar(),
        from: s.from.desugar(),
        where_clause: s.where_clause.desugar(),
        group: s.group.desugar(),
        having: s.having.desugar(),
        order: s.order.desugar(),
    }
}

fn trailing(option: &mut core::SelectOption) -> &mut Space {
    use core::SelectOption::*;
    match option {
        NoFPlan((_, sp))
        | NoLock((_, sp))
        | Profile((_, sp))
        | ProfileAll((_, sp))
        | NoRuntime((_, sp))
        | DoRuntime((_, sp))
        | RowLimit(_, _, sp)
        | RowOffset(_, _, sp) => sp,
    }
}

/// `COUNT(ALL x)` is `COUNT(x)`, in every aggregate.
pub fn agg(agg: &full::Agg) -> core::Agg {
    match agg {
        full::Agg::AggFuncArgs {
            fun,
            sp,
            open,
            all,
            arg,
            close,
        } => {
            let mut open = open.desugar();
            if let Some((_, _, after)) = all {
                open.1 = open.1.join(after);
            }
            core::Agg::AggFuncArgs {
                fun: fun.desugar(),
                sp: sp.desugar(),
                open,
                arg: arg.desugar(),
                close: close.desugar(),
            }
        }
        full::Agg::CountStar(count, sp, star) => {
            core::Agg::CountStar(count.desugar(), sp.desugar(), star.desugar())
        }
    }
}

/// `ASC`, the order by default, goes.
pub fn ordir(o: &full::Ordir) -> core::Ordir {
    core::Ordir {
        expr: o.expr.desugar(),
        desc: o.desc.desugar(),
    }
}

/// An omitted keyword stands as spelled, after a blank.
pub fn spelled<K: Grammar + Default>(_: &()) -> core::Implied<K> {
    core::Implied::Spelled(Space::blank(), K::default())
}

/// `FROM a, b` is `FROM a CROSS JOIN b`: each reference after the first a CROSS JOIN on it, with
/// the range of the reference it comes from, in parentheses when it has joins of its own.
pub fn tables(t: &full::Tables) -> core::Tables {
    let core::Tref::Join(mut first) = t.first.desugar();
    for ((before, _, after), tref) in &t.rest {
        let (full::Tref::Join(jtab) | full::Tref::OJSyntax { table: jtab, .. }) = tref;
        let core::Tref::Join(mut table) = tref.desugar();
        let start = jtab.formit.range().start;
        let end = jtab
            .joins
            .last()
            .map_or(jtab.formit.range().end, |(_, join)| join.range().end);
        if !table.joins.is_empty() {
            table = core::JTab {
                formit: Src::new(
                    start..end,
                    core::Frmit::JTab {
                        open: (Default::default(), Space::default()),
                        joined: Box::new(table),
                        close: (Space::default(), Default::default()),
                    },
                ),
                joins: Vec::new(),
            };
        }
        let cross = core::Join::Cross {
            cross_kw: (
                Default::default(),
                Space::blank(),
                Default::default(),
                Space::blank(),
            ),
            table: Box::new(table),
        };
        let sp = Space::blank().join(before).join(after);
        first.joins.push((sp, Src::new(start..end, cross)));
    }
    core::Tables {
        first: core::Tref::Join(first),
    }
}

/// `{oj ...}` is the join inside it.
pub fn oj(
    _: &StringEq!("{"),
    _: &(Ident, Space),
    table: &full::JTab,
    _: &(Space, StringEq!("}")),
) -> core::Tref {
    core::Tref::Join(table.desugar())
}

/// `JOIN` is `INNER JOIN`.
pub fn inner(join_kw: &StringEqCI!("JOIN")) -> core::JOp {
    core::JOp::Typed {
        jtype: core::JType::Inner(Default::default()),
        sp: Space::blank(),
        join_kw: *join_kw,
    }
}

/// `OFFSET m [ROWS] [FETCH [FIRST|NEXT] n [ROWS] [ONLY]]` is `LIMIT n OFFSET m`, `LIMIT ALL
/// OFFSET m` without the FETCH.
pub fn offset_fetch(offset: &full::Offset, fetch: &Option<full::Fetch>) -> core::RowLimitingClause {
    let mut before_offset = Space::blank().join(&space_before(&offset.rows));
    let (after_limit, spec) = match fetch {
        None => (
            Space::blank(),
            core::LimitSpec::All(Default::default(), Default::default()),
        ),
        Some(fetch) => {
            let (before_count, after_count) = fetch_blanks(fetch);
            before_offset = before_offset.join(&fetch.keyword.0).join(&after_count);
            (before_count, core::LimitSpec::Val(fetch.valspec.desugar()))
        }
    };
    core::RowLimitingClause::LimitOffset {
        limit: Prefix::new(
            (offset.kw.0.desugar(), Default::default(), after_limit),
            spec,
        ),
        offset: Some(core::Offset {
            kw: (before_offset, Default::default(), offset.kw.2.desugar()),
            count: offset.count.desugar(),
        }),
    }
}

/// `FETCH [FIRST|NEXT] n [ROWS] [ONLY]` is `LIMIT n`, the blanks of the removed keywords between
/// LIMIT and n.
pub fn fetch_only(fetch: &full::Fetch) -> core::RowLimitingClause {
    let (before_count, after_count) = fetch_blanks(fetch);
    core::RowLimitingClause::LimitOffset {
        limit: Prefix::new(
            (
                fetch.keyword.0.desugar(),
                Default::default(),
                before_count.join(&after_count),
            ),
            core::LimitSpec::Val(fetch.valspec.desugar()),
        ),
        offset: None,
    }
}

/// The blanks of a FETCH around its count: within `FETCH [FIRST|NEXT]` and within `[ROWS] [ONLY]`.
fn fetch_blanks(fetch: &full::Fetch) -> (Space, Space) {
    let direction = fetch
        .direction
        .as_ref()
        .map_or_else(Space::default, |(_, sp)| sp.clone());
    (
        fetch.keyword.2.join(&direction),
        space_before(&fetch.row_keyword).join(&space_before(&fetch.only)),
    )
}

fn space_before<T>(option: &Option<(Space, T)>) -> Space {
    option
        .as_ref()
        .map_or_else(Space::default, |(sp, _)| sp.clone())
}

/// `ROWS` after the offset goes: the blank before it joins the one before OFFSET.
pub fn offset(o: &full::Offset) -> core::Offset {
    core::Offset {
        kw: (
            o.kw.0.join(&space_before(&o.rows)),
            o.kw.1,
            o.kw.2.desugar(),
        ),
        count: o.count.desugar(),
    }
}

/// `LIMIT ALL` alone, which limits nothing, goes.
pub fn selct(s: &full::Selct) -> core::Selct {
    let unlimited = |clause: &core::RowLimitingClause| {
        matches!(clause, core::RowLimitingClause::LimitOffset { limit, offset: None }
            if matches!(limit.prefixed, core::LimitSpec::All(..)))
    };
    core::Selct {
        query: s.query.desugar(),
        order: s.order.desugar(),
        row_limiting_clause: s
            .row_limiting_clause
            .desugar()
            .filter(|clause| !unlimited(clause)),
    }
}

/// `SIZE n` is `SIZE (n)`.
pub fn size_in_parens(kw: &(Space, StringEqCI!("SIZE"), Space), size: &Literal) -> core::SizeHint {
    core::SizeHint::Paren {
        kw: kw.desugar(),
        open: Default::default(),
        size: core::PConst::Literal(size.desugar()),
        close: Default::default(),
    }
}

/// `SIZE ((n))` keeps one pair of parentheses: the ones within go.
pub fn unnested<P: Deref<Target = full::PConst>>(
    _: &StringEq!("("),
    inner: &P,
    _: &StringEq!(")"),
) -> core::PConst {
    inner.desugar()
}

/// `INSERT ... SET a = x, b = y` is `INSERT ... (a, b) VALUES (x, y)`.
pub fn set_as_values(
    _: &StringEqCI!("SET"),
    assignments: &(Space, VecSep<Src<full::SQLSetAssignment>, ArgSep>),
) -> core::InsertValues {
    let (after_set, list) = assignments;
    let mut open_sp = Space::default();
    let mut exprs = VecSep {
        items: Vec::new(),
        seps: list.seps.desugar(),
    };
    for (i, assignment) in list.items.iter().enumerate() {
        let eq = assignment.eq.0.join(&assignment.eq.2);
        match i.checked_sub(1) {
            None => open_sp = open_sp.join(&eq),
            Some(before) => exprs.seps[before].2 = exprs.seps[before].2.join(&eq),
        }
        exprs.items.push(assignment.value.desugar());
    }
    let columns = VecSep {
        items: list
            .items
            .iter()
            .map(|assignment| assignment.column.desugar())
            .collect(),
        seps: vec![(Space::default(), Default::default(), Space::blank()); list.seps.len()],
    };
    core::InsertValues::ColumnList {
        columns: Some(Wrap::new(
            (
                Space::default(),
                Default::default(),
                Space::default().join(after_set),
            ),
            columns,
            (Space::default(), Default::default()),
        )),
        values: (
            Space::blank(),
            core::ValueSource::ValSpec {
                values_kw: Default::default(),
                sp: Space::blank(),
                spec: core::ValSpec::Scalx {
                    open: (Default::default(), open_sp),
                    exprs,
                    close: (Space::default(), Default::default()),
                },
            },
        ),
    }
}

/// `UPDATE t (a, b) VALUES (x, y)` is `UPDATE t SET a = x, b = y`; a column list with a query or a
/// host array for its values has no SET form and stays.
pub fn asign(asign: &full::Asign) -> core::Asign {
    match asign {
        full::Asign::SetList {
            set_kw,
            assignments,
        } => core::Asign::SetList {
            set_kw: *set_kw,
            assignments: assignments.desugar(),
        },
        full::Asign::ColumnList {
            columns: Some(columns),
            values:
                (
                    before_values,
                    full::ValueSource::ValSpec {
                        sp,
                        spec: full::ValSpec::Scalx { open, exprs, close },
                        ..
                    },
                ),
        } if columns.wrapped.items.len() == exprs.items.len() => {
            let items = columns
                .wrapped
                .items
                .iter()
                .zip(&exprs.items)
                .map(|(column, value)| {
                    let full::Scalx::Legacy(terms) = value;
                    let last = terms.items.len() - 1;
                    let range = terms.items[0].range().start..terms.items[last].range().end;
                    Src::new(
                        range,
                        core::SQLSetAssignment {
                            column: column.desugar(),
                            eq: (Space::blank(), Default::default(), Space::blank()),
                            value: value.desugar(),
                        },
                    )
                })
                .collect();
            let seps = exprs
                .seps
                .iter()
                .zip(&columns.wrapped.seps)
                .map(|((before, comma, after), (col_before, _, col_after))| {
                    (before.join(col_before), *comma, after.join(col_after))
                })
                .collect();
            let removed = [
                &columns.prefix.0,
                &columns.prefix.2,
                &columns.suffix.0,
                before_values,
                sp,
                &open.1,
                &close.0,
            ];
            let sp = removed
                .into_iter()
                .fold(Space::blank(), |sp, removed| sp.join(removed));
            core::Asign::SetList {
                set_kw: Default::default(),
                assignments: (sp, VecSep { items, seps }),
            }
        }
        full::Asign::ColumnList { columns, values } => core::Asign::ColumnList {
            columns: columns.desugar(),
            values: values.desugar(),
        },
    }
}

/// `%INTRANS` is `%INTRANSACTION`, `COMMIT WORK` is `COMMIT`.
pub fn tp_option(option: &full::TPOption) -> core::TPOption {
    use full::TPOption::*;
    match option {
        InTransaction(kw) => core::TPOption::InTransaction(*kw),
        BegTrans(kw) => core::TPOption::BegTrans(*kw),
        InTrans(_) => core::TPOption::InTransaction(Default::default()),
        Commit { commit_kw, .. } => core::TPOption::Commit {
            commit_kw: *commit_kw,
        },
    }
}

/// `ROLLBACK WORK` is `ROLLBACK`.
pub fn rollback(rollback: &full::RollB) -> core::RollB {
    core::RollB {
        rollback_kw: rollback.rollback_kw,
    }
}
