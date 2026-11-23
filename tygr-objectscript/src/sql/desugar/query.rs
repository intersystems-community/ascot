//! The predicate and query rules of the desugaring (see `grammar`): each the `into` of a `#[sugar]`
//! mark, given the sugar node's fields, or the whole node, by reference.

use std::ops::Deref;

use tygr::{Grammar, Prefix, StringEq, StringEqCI};

use super::Desugar;
use crate::convert::VecSep;
use crate::sql::grammar::{core, full};
use crate::sql::leaf::{Ident, Literal, Space};
use crate::sql::src::Src;

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
    use full::{CmpOp, Cond::*, ContainsPredicate, FollowsPredicate, QuantifiedOp};
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
        Compare {
            op:
                CmpOp::Contains(ContainsPredicate::NotLB(_))
                | CmpOp::Follows(FollowsPredicate::NotFollows(_)),
            ..
        } => return Some((Default::default(), Space::blank())),
        _ => None,
    };
    keyword.map(|(not, sp)| (*not, sp.clone()))
}

/// Field by field, as an unmarked type converts.
macro_rules! variants {
    ($value:expr; $($Variant:ident { $($field:ident),* })*) => {
        match $value {
            $(full::Cond::$Variant { $($field),* } => core::Cond::$Variant { $($field: $field.desugar()),* },)*
        }
    };
}

/// The predicate, its negation read as its keyword (see `Negatable`).
fn predicate(cond: &full::Cond) -> core::Cond {
    variants!(cond;
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
    )
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
/// the range of the reference it comes from.
pub fn tables(t: &full::Tables) -> core::Tables {
    let core::Tref::Join(mut first) = t.first.desugar();
    for ((before, _, after), tref) in &t.rest {
        let (full::Tref::Join(jtab) | full::Tref::OJSyntax { table: jtab, .. }) = tref;
        let core::Tref::Join(table) = tref.desugar();
        let start = jtab.formit.range().start;
        let end = jtab
            .joins
            .last()
            .map_or(jtab.formit.range().end, |(_, join)| join.range().end);
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

/// `FETCH [FIRST|NEXT] n [ROWS] [ONLY]` is `LIMIT n`.
pub fn fetch_only(fetch: &full::Fetch) -> core::RowLimitingClause {
    let (before_count, _) = fetch_blanks(fetch);
    core::RowLimitingClause::LimitOffset {
        limit: Prefix::new(
            (fetch.keyword.0.desugar(), Default::default(), before_count),
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

/// `ROWS` after the offset goes.
pub fn offset(o: &full::Offset) -> core::Offset {
    core::Offset {
        kw: o.kw.desugar(),
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
