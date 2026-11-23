//! The predicate and query rules of the desugaring (see `grammar`): each the `into` of a `#[sugar]`
//! mark, given the sugar node's fields, or the whole node, by reference.

use tygr::{Grammar, StringEqCI};

use super::Desugar;
use crate::convert::VecSep;
use crate::sql::grammar::{core, full};
use crate::sql::leaf::Space;
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
