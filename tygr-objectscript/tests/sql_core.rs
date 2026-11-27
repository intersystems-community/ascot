//! The desugaring into the core level of the SQL grammar: a node a rule synthesizes stands at the source
//! of the node it replaces. The statements each level accepts, the core text each rule gives and that
//! text's being a fixed point of the desugaring are corpus documents under ByFeature/SQL.

use tygr::{Parse, Ranged};
use tygr_objectscript::sql::core;
use tygr_objectscript::sql::desugar::Desugar;
use tygr_objectscript::sql::*;

fn core_query(text: &str) -> core::QuerySpecification {
    let sql = SQL::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
    let core::Statement::SelectStatement(statement) = sql.desugar().statement.ranged else {
        panic!("{text}")
    };
    let Some(core::QueryTerm::QuerySpecification(query)) =
        statement.query.0.items.into_iter().next()
    else {
        panic!("{text}")
    };
    query.ranged
}

fn term_of<B: tygr::Grammar + Default>(
    expr: &core::ValueExpression<B>,
) -> &Ranged<core::ValueExpressionPrimary> {
    let core::Operand::Plain(_, term) = &expr.first else {
        panic!("parentheses")
    };
    term
}

fn clauses(query: &core::QuerySpecification) -> &core::SelectClauses {
    let core::SelectBody::Plain(clauses, _) = &query.body else {
        panic!("TOP")
    };
    clauses
}

fn item_term(query: &core::QuerySpecification, i: usize) -> &Ranged<core::ValueExpressionPrimary> {
    let core::SelectItem::Expr { expr, .. } = &clauses(query).select_list.items[i].ranged else {
        panic!("a star")
    };
    term_of(expr)
}

/// A call's name and TRIM's blank stand at their term, a lifted NOT and its parenthesized condition at
/// the condition, and a comma's CROSS JOIN at the table entry.
#[test]
fn synthesized_nodes_stand_at_the_source_they_replace() {
    let text = "SELECT TRIM(b), %SQLUPPER c FROM t, u WHERE d NOT IN (1, 2)";
    let query = core_query(text);
    let at = |node: &Ranged<core::ValueExpressionPrimary>| &text[node.range()];
    let core::ValueExpressionPrimary::Builtin(trim) = &item_term(&query, 0).ranged else {
        panic!("{text}")
    };
    let core::Builtin::Trim(_, (_, args)) = &**trim else {
        panic!("{text}")
    };
    let core::TrimArgs::Full(_, chars, ..) = &args.wrapped;
    assert_eq!(at(term_of(chars)), "TRIM(b)");
    let core::ValueExpressionPrimary::Call(call) = &item_term(&query, 1).ranged else {
        panic!("{text}")
    };
    assert_eq!(&text[call.name.name.range()], "%SQLUPPER c");
    let clauses = clauses(&query);
    let core::TableReference::Join(tables) = &clauses.from.as_ref().unwrap().1.tables.first;
    assert_eq!(&text[tables.joins[0].1.range()], "u");
    let condition = &clauses
        .where_clause
        .as_ref()
        .unwrap()
        .1
        .prefixed
        .first
        .first;
    let core::BooleanFactor::Plain(_, not) = condition else {
        panic!("{text}")
    };
    let core::Factor::Not(_, inner) = &not.ranged else {
        panic!("{text}")
    };
    let core::BooleanFactor::Paren(_, paren) = &**inner else {
        panic!("{text}")
    };
    let core::BooleanFactor::Plain(_, factor) = &paren.wrapped.first.first else {
        panic!("{text}")
    };
    assert_eq!(&text[factor.range()], "d NOT IN (1, 2)");
}

// On a wide stack: a parenthesis recurses a frame per level, past a test thread's 2 MiB.
#[test]
fn malformed_nested_parens_fail_fast() {
    let sql = format!(
        "SELECT a FROM t WHERE {}a = = 1{}",
        "(".repeat(20),
        ")".repeat(20)
    );
    let start = std::time::Instant::now();
    let parse = move || drop(SQL::parse(&sql));
    let thread = std::thread::Builder::new().stack_size(8 << 20);
    thread.spawn(parse).unwrap().join().unwrap();
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
}
