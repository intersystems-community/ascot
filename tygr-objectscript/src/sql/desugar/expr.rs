//! The expression rules of the desugaring (see `grammar`), as `query`'s. Each spelling here was
//! measured against its core spelling on IRIS 2026.2.

use std::ops::Deref;

use tygr::{Grammar, Prefix, Print, Raw, StringEq, StringEqCI, Wrap};

use super::Desugar;
use crate::convert::VecSep;
use crate::sql::grammar::{core, full};
use crate::sql::leaf::{ArgSep, DottedIdent, Ident, Literal, Space, StringLiteral};
use crate::sql::src::Src;

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

/// `{fn NAME(args)}` is the call `NAME(args)`, `{fn PI}` and the like `PI()`, `{fn USER}` the niladic
/// `USER`, TIMESTAMPADD and TIMESTAMPDIFF their builtins.
fn odbc_function(function: &full::ODBCFunc) -> core::Term {
    calls!(ODBCFunc: function;
        Abs Acos Asin Atan Ceiling Cos Cot Exp Floor Log Log10 Mod Power Sin Sqrt Tan Ascii Char
        Concat DayName DayOfMonth DayOfWeek DayOfYear Hour LCase Left Length LTrim Minute Month
        MonthName Quarter Repeat Right Round RTrim Second Sign Space Substring Truncate UCase Week
        Year;
        Pi CurDate CurTime Database Now;
        full::ODBCFunc::User(kw, _) => {
            core::Term::Niladic(core::Niladic::User(*kw), Default::default())
        }
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
