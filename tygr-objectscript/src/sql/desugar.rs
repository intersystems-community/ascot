//! `Desugar`: a full SQL node into its core copy, the sugar rewritten (see `sql`). The grammar's
//! types get their impls from `levels`, each `#[sugar]` mark naming its rule here; the shared leaves
//! and tygr's containers get theirs below. Each rule was measured on IRIS 2026.2.

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

/// `a, b`.
fn commas<T>(items: Vec<T>) -> VecSep<T, ArgSep> {
    let sep = (Space::default(), Default::default(), Space::blank());
    let seps = vec![sep; items.len().saturating_sub(1)];
    VecSep { items, seps }
}

/// `(a, b)`.
fn list(args: Vec<core::Scalx>) -> core::ArgsN {
    let open = (Space::default(), Default::default(), Space::default());
    let items = (!args.is_empty()).then(|| commas(args));
    Wrap::new(open, items, (Space::default(), Default::default()))
}

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
fn call(name: &str, args: Vec<core::Scalx>) -> core::Term {
    let name = DottedIdent {
        head: Ident::simple(name),
        rest: Vec::new(),
    };
    core::Term::Call(core::Call {
        name: core::SQLFunctionName(Src::synthesized(name)),
        args: list(args),
    })
}

/// The arguments a function's argument list gives the call, in order.
trait Arg {
    fn args(&self, out: &mut Vec<core::Scalx>);
}

fn args(shape: &impl Arg) -> Vec<core::Scalx> {
    let mut out = Vec::new();
    shape.args(&mut out);
    out
}

impl Arg for full::Scalx {
    fn args(&self, out: &mut Vec<core::Scalx>) {
        out.push(self.desugar())
    }
}

impl Arg for Src<full::Term> {
    fn args(&self, out: &mut Vec<core::Scalx>) {
        out.push(core::Scalx::Legacy(VecSep::one(self.desugar())))
    }
}

impl Arg for VecSep<full::Scalx, ArgSep> {
    fn args(&self, out: &mut Vec<core::Scalx>) {
        out.extend(self.items.iter().map(Desugar::desugar))
    }
}

impl Arg for full::Args0 {
    fn args(&self, _: &mut Vec<core::Scalx>) {}
}

impl<T: Arg> Arg for Option<T> {
    fn args(&self, out: &mut Vec<core::Scalx>) {
        if let Some(arg) = self {
            arg.args(out)
        }
    }
}

impl<K, T: Arg> Arg for (K, T) {
    fn args(&self, out: &mut Vec<core::Scalx>) {
        self.1.args(out)
    }
}

impl<K, T: Arg, U: Arg> Arg for (K, T, U) {
    fn args(&self, out: &mut Vec<core::Scalx>) {
        self.1.args(out);
        self.2.args(out)
    }
}

impl<T: Arg> Arg for Wrap<Open, T, Close> {
    fn args(&self, out: &mut Vec<core::Scalx>) {
        self.wrapped.args(out)
    }
}

/// The argument shapes of the functions called by name, each its arguments in order.
macro_rules! shapes {
    ($($Shape:ident<$($T:ident),*> { $($field:ident)* })*) => {$(
        impl<$($T: Arg + Grammar),*> Arg for full::$Shape<$($T),*> {
            fn args(&self, out: &mut Vec<core::Scalx>) {
                $(self.$field.args(out);)*
            }
        }
    )*};
}

shapes! {
    ArgsRR<T1, T2> { arg1 arg2 }
    ArgsRRR<T1, T2, T3> { arg1 arg2 arg3 }
    ArgsRRRR<T1, T2, T3, T4> { arg1 arg2 arg3 arg4 }
    ArgsRO<T1, T2> { arg1 arg2 }
    ArgsROO<T1, T2, T3> { arg1 rest }
    ArgsRRO<T1, T2, T3> { arg1 arg2 arg3 }
    ArgsRROO<T1, T2, T3, T4> { arg1 arg2 rest }
}

/// `POSITION(a IN b)` has the arguments of `CHARINDEX(a, b)`.
impl Arg for full::SQLPositionArgs {
    fn args(&self, out: &mut Vec<core::Scalx>) {
        self.needle.args(out);
        self.haystack.args(out)
    }
}

/// `SUBSTRING(x FROM s FOR l)` has the arguments of `SUBSTRING(x, s, l)`.
impl Arg for full::SubstringArgsInternal {
    fn args(&self, out: &mut Vec<core::Scalx>) {
        match self {
            Self::Keyword {
                expr,
                start,
                for_clause,
                ..
            } => {
                expr.args(out);
                start.args(out);
                for_clause.args(out)
            }
            Self::Position {
                expr,
                start,
                length,
                ..
            } => {
                expr.args(out);
                start.args(out);
                length.args(out)
            }
        }
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

/// A parenthesized datepart loses its parentheses.
fn unwrapped<T: Grammar>(part: &full::OptionallyWrapped<T>) -> &T {
    match part {
        full::OptionallyWrapped::Wrapped(parens) => &parens.wrapped,
        full::OptionallyWrapped::Scalar(part) => part,
    }
}

impl Arg for full::DatePart {
    fn args(&self, out: &mut Vec<core::Scalx>) {
        out.push(datepart(unwrapped(self)))
    }
}

impl Arg for full::DatePartPart {
    fn args(&self, out: &mut Vec<core::Scalx>) {
        out.push(match unwrapped(&self.0) {
            full::DatePartPartRaw::SqlTimestamp(_) => string("sqltimestamp"),
            full::DatePartPartRaw::Common(common) => datepart(common),
        })
    }
}

/// The call a function's keyword and argument list spell; a niladic-looking one gets `()`.
macro_rules! calls {
    ($Enum:ident: $value:expr; $($Variant:ident)*; $($Niladic:ident)*; $($arm:tt)*) => {
        match $value {
            $(full::$Enum::$Variant(kw, a) => call(&kw.print(), args(a)),)*
            $(full::$Enum::$Niladic(kw, _) => call(&kw.print(), Vec::new()),)*
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
        full::ScalarFunction::Position(_, a) => call("CHARINDEX", args(a)),
    )
}

/// `{fn NAME(args)}` is `NAME(args)`, `{fn PI}` and its kin `PI()`, TIMESTAMPADD and TIMESTAMPDIFF builtins.
fn odbc_function(function: &full::ODBCFunc) -> core::Term {
    calls!(ODBCFunc: function;
        Abs Acos Asin Atan Ceiling Cos Cot Exp Floor Log Log10 Mod Power Sin Sqrt Tan Ascii Char
        Concat DayName DayOfMonth DayOfWeek DayOfYear Hour LCase Left Length LTrim Minute Month
        MonthName Quarter Repeat Right Round RTrim Second Sign Space Substring Truncate UCase Week
        Year;
        Pi CurDate CurTime Database Now;
        full::ODBCFunc::TimestampAdd(kw, a) => {
            core::Term::Builtin(Box::new(core::Builtin::TimestampAdd(*kw, a.desugar())))
        }
        full::ODBCFunc::TimestampDiff(kw, a) => {
            core::Term::Builtin(Box::new(core::Builtin::TimestampDiff(*kw, a.desugar())))
        }
    )
}

/// `{d x}`, `{t x}` and `{ts x}` are `CAST(x AS DATE)`, `CAST(x AS TIME)` and `CAST(x AS TIMESTAMP)`.
fn cast(value: &full::DTSVal) -> core::Term {
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
        open: (Space::default(), Default::default(), Space::default()),
        expr,
        as_kw: (Space::blank(), Default::default(), Space::blank()),
        cast_type,
        close: (Space::default(), Default::default()),
    };
    core::Term::Builtin(Box::new(core::Builtin::Cast(
        Default::default(),
        Box::new(cast),
    )))
}

/// An ODBC escape is what it stands for: see `odbc_function` and `cast`.
pub fn odbc_escape<E: Deref<Target = full::ODBCEscape>>(escape: &E) -> core::Term {
    match &escape.wrapped {
        full::DTS::DateTime(value) => cast(value),
        full::DTS::Function(function) => odbc_function(&function.prefixed),
    }
}

/// `%SQLUPPER x` is the call `%SQLUPPER(x)`; an `x` in parentheses gives the call its own.
pub fn percent_call<T: Deref<Target = Src<full::Term>>>(
    op: &full::PercentOp,
    _: &Space,
    term: &T,
) -> core::Term {
    let arg = match &term.node {
        full::Term::Scalx(parens) => (*parens.wrapped).desugar(),
        _ => core::Scalx::Legacy(VecSep::one((**term).desugar())),
    };
    call(&op.print(), vec![arg])
}

/// `%TRUNCATE(x, n)` is the call, as `%STRING`, `%SQLSTRING` and `%SQLUPPER` with a truncation are.
pub fn string_operation<O: Deref<Target = full::StringOperation>>(operation: &O) -> core::Term {
    let truncation = match &operation.truncation {
        full::ColTrun::Const(constant) => {
            let core::PConst::Literal(literal) = constant.desugar();
            core::Term::Literal(literal)
        }
        full::ColTrun::HostVar(host_var) => core::Term::HostVar(host_var.clone()),
    };
    let args = vec![operation.expr.desugar(), term(truncation)];
    call(&operation.op.print(), args)
}

/// `ROWS n PRECEDING` is `ROWS BETWEEN n PRECEDING AND CURRENT ROW`, the standard's lone-bound reading.
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
    _: &Space,
    _: &Option<(StringEqCI!("FROM"), Space)>,
    expr: &full::Scalx,
) -> core::TrimArgs {
    trim(spec.desugar(), string(" "), expr)
}

/// `TRIM(c FROM x)` is `TRIM(BOTH c FROM x)`, `TRIM(x)` `TRIM(BOTH ' ' FROM x)`.
pub fn trim_both(
    chars: &Option<(full::Scalx, Space, StringEqCI!("FROM"), Space)>,
    expr: &full::Scalx,
) -> core::TrimArgs {
    let chars = chars
        .as_ref()
        .map_or_else(|| string(" "), |(chars, ..)| chars.desugar());
    trim(core::TrimSpec::Both(Default::default()), chars, expr)
}

/// `SPEC chars FROM x`.
fn trim(spec: core::TrimSpec, chars: core::Scalx, expr: &full::Scalx) -> core::TrimArgs {
    core::TrimArgs::SpecScalxScalx(
        spec,
        Space::blank(),
        chars,
        Space::blank(),
        Default::default(),
        Space::blank(),
        expr.desugar(),
    )
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

/// A condition: its predicate, in `NOT (...)` when the predicate is negated. IRIS applies a
/// quantified `NOT[` or `NOT]` to each row (measured on 2026.2), so under the `NOT` its quantifier
/// flips: `x NOT[ ANY (q)` is `NOT (x [ ALL (q))`.
pub fn cond(cond: &full::Cond, mut plain: core::Cond) -> core::Cond {
    let Some(not_kw) = negation(cond) else {
        return plain;
    };
    if let core::Cond::Comparison {
        tail:
            core::Comparison::Query {
                op: core::QuantifiedOp::Op { quantifier, .. },
                ..
            },
        ..
    } = &mut plain
        && let full::Cond::Comparison {
            tail:
                full::Comparison::Query {
                    op: full::QuantifiedOp::Op { op, .. },
                    ..
                },
            ..
        } = cond
        && negated(op)
    {
        *quantifier = match quantifier {
            core::Quantifier::All(_) => core::Quantifier::Any(Default::default()),
            core::Quantifier::Any(_) => core::Quantifier::All(Default::default()),
        };
    }
    core::Cond::Not {
        not_kw: (not_kw, Space::blank()),
        condition: Box::new(Src::synthesized(core::Cond::Paren {
            open: (Default::default(), Space::default()),
            predicate: Box::new(core::Prdct(VecSep::one(core::And(VecSep::one(
                Src::synthesized(plain),
            ))))),
            close: (Space::default(), Default::default()),
        })),
    }
}

/// The `NOT` of a negated predicate: the keyword's own, or one made for `NOT[` and `NOT]`.
fn negation(cond: &full::Cond) -> Option<full::NotKw> {
    use full::{Comparison::*, QuantifiedOp};
    let full::Cond::Comparison { tail, .. } = cond else {
        return None;
    };
    match tail {
        IsNull { null_kw, .. } => null_kw.negation(),
        Between { between_kw, .. } => between_kw.negation(),
        Like { like_kw, .. } => like_kw.negation(),
        Matches { matches_kw, .. } => matches_kw.negation(),
        In { in_kw, .. } => in_kw.negation(),
        InList { inlist_kw, .. } => inlist_kw.negation(),
        IsJSON { json_kw, .. } => json_kw.negation(),
        Query {
            op: QuantifiedOp::In(in_kw),
            ..
        } => in_kw.negation(),
        Scalx { op, .. }
        | Query {
            op: QuantifiedOp::Op { op, .. },
            ..
        } if negated(op) => Some(Default::default()),
        _ => None,
    }
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

/// An omitted keyword stands as spelled, after a blank.
pub fn spelled<K: Grammar + Default>(_: &()) -> core::Implied<K> {
    core::Implied::Spelled(Space::blank(), K::default())
}

/// `FROM a, b` is `FROM a CROSS JOIN b`, each join at its reference's range, in parentheses when it has joins.
pub fn tables(t: &full::Tables, plain: core::Tables) -> core::Tables {
    let core::Tref::Join(mut first) = plain.first;
    for (_, tref) in &t.rest {
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
        first
            .joins
            .push((Space::blank(), Src::new(start..end, cross)));
    }
    core::Tables {
        first: core::Tref::Join(first),
    }
}

/// `JOIN` is `INNER JOIN`.
pub fn inner(join_kw: &StringEqCI!("JOIN")) -> core::JOp {
    core::JOp::Typed {
        jtype: core::JType::Inner(Default::default()),
        sp: Space::blank(),
        join_kw: *join_kw,
    }
}

/// `OFFSET m [ROWS] [FETCH … n …]` is `LIMIT n OFFSET m`, `LIMIT ALL OFFSET m` without the FETCH.
pub fn offset_fetch(offset: &full::Offset, fetch: &Option<full::Fetch>) -> core::RowLimitingClause {
    let spec = match fetch {
        None => core::LimitSpec::All(Default::default(), Default::default()),
        Some(fetch) => core::LimitSpec::Val(fetch.valspec.desugar()),
    };
    core::RowLimitingClause::LimitOffset {
        limit: limit(spec),
        offset: Some(core::Offset {
            kw: (Space::blank(), Default::default(), Space::blank()),
            count: offset.count.desugar(),
        }),
    }
}

/// `FETCH [FIRST|NEXT] n [ROWS] [ONLY]` is `LIMIT n`.
pub fn fetch_only(fetch: &full::Fetch) -> core::RowLimitingClause {
    core::RowLimitingClause::LimitOffset {
        limit: limit(core::LimitSpec::Val(fetch.valspec.desugar())),
        offset: None,
    }
}

/// ` LIMIT spec`.
fn limit(spec: core::LimitSpec) -> core::Limit {
    Prefix::new((Space::blank(), Default::default(), Space::blank()), spec)
}

/// `LIMIT ALL` alone, which limits nothing, goes.
pub fn selct(_: &full::Selct, mut plain: core::Selct) -> core::Selct {
    let unlimited = |clause: &core::RowLimitingClause| {
        matches!(clause, core::RowLimitingClause::LimitOffset { limit, offset: None }
            if matches!(limit.prefixed, core::LimitSpec::All(..)))
    };
    let clause = plain.row_limiting_clause.take();
    plain.row_limiting_clause = clause.filter(|clause| !unlimited(clause));
    plain
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

/// `INSERT ... SET a = x, b = y` is `INSERT ... (a, b) VALUES (x, y)`.
pub fn set_as_values(
    _: &StringEqCI!("SET"),
    (_, list): &(Space, VecSep<Src<full::SQLSetAssignment>, ArgSep>),
) -> core::InsertValues {
    let columns = list.items.iter().map(|a| a.column.desugar()).collect();
    let values = list.items.iter().map(|a| a.value.desugar()).collect();
    core::InsertValues::ColumnList {
        columns: Some(Wrap::new(
            (Space::default(), Default::default(), Space::default()),
            commas(columns),
            (Space::default(), Default::default()),
        )),
        values: (
            Space::blank(),
            core::ValueSource::ValSpec {
                values_kw: Default::default(),
                sp: Space::blank(),
                spec: core::ValSpec::Scalx {
                    open: (Default::default(), Space::default()),
                    exprs: commas(values),
                    close: (Space::default(), Default::default()),
                },
            },
        ),
    }
}

/// `UPDATE t (a, b) VALUES (x, y)` is `UPDATE t SET a = x, b = y`; a query or host array as values stays.
pub fn asign(asign: &full::Asign, plain: core::Asign) -> core::Asign {
    match asign {
        full::Asign::ColumnList {
            columns: Some(columns),
            values:
                (
                    _,
                    full::ValueSource::ValSpec {
                        spec: full::ValSpec::Scalx { exprs, .. },
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
            core::Asign::SetList {
                set_kw: Default::default(),
                assignments: (Space::blank(), commas(items)),
            }
        }
        _ => plain,
    }
}
