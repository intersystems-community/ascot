//! The core level of the SQL grammar and the desugaring into it, over hand-written statements: the core
//! text re-parses under the core grammar and desugars to itself, every `Src` node maps a range of it to a
//! range of the source, and the trees take their shapes; the statements each level accepts and the core
//! text each rule gives are corpus documents under ByFeature/SQL.

use tygr::{Parse, Print};
use tygr_objectscript::sql::core;
use tygr_objectscript::sql::desugar::Desugar;
use tygr_objectscript::sql::*;

/// One statement per rule of the rewrite, statements mixing them, and the sugar-free rest of the grammar;
/// the core text each rule gives is pinned by the corpus documents under ByFeature/SQL/Core.
const STATEMENTS: &[&str] = &[
    "SELECT a FROM t WHERE a != 1",
    "SELECT a FROM t WHERE a NOT= 1",
    "SELECT a FROM t WHERE a NOT> 1",
    "SELECT a FROM t WHERE a NOT< 1",
    "SELECT a FROM t WHERE a NOT[ 'x'",
    "SELECT a FROM t WHERE a NOT] 'x'",
    "SELECT a FROM t WHERE a IS NOT NULL",
    "SELECT a FROM t WHERE a not /* c */ BETWEEN 1 AND 2",
    "SELECT a FROM t WHERE a NOT IN (1, 2)",
    "SELECT a FROM t WHERE a NOT LIKE 'x%'",
    "SELECT a FROM t WHERE a NOT %MATCHES 'p'",
    "SELECT a FROM t WHERE a NOT %INLIST :l",
    "SELECT a FROM t WHERE a IS NOT JSON",
    "SELECT a FROM t WHERE NOT a IS NOT NULL",
    "SELECT a FROM t WHERE a IN (SELECT b FROM u)",
    "SELECT a FROM t WHERE a NOT IN (SELECT b FROM u)",
    "SELECT a FROM t WHERE a = SOME (SELECT b FROM u)",
    "SELECT a FROM t WHERE a NOT[ ANY (SELECT b FROM u)",
    "SELECT a FROM t WHERE a NOT] ALL (SELECT b FROM u)",
    "SELECT a FROM t WHERE a = 1 ! b = 2",
    "SELECT a FROM t WHERE a = 1 & b = 2",
    "SELECT ALL a FROM t",
    "SELECT %NOLOCK ALL /* c */ a FROM t",
    "SELECT ALLOWED FROM t",
    "SELECT COUNT(ALL a), COUNT(ALL /* c */ b), COUNT(ALLOWED) FROM t",
    "SELECT a FROM t ORDER BY a ASC, b DESC",
    "SELECT a x FROM t u",
    "SELECT a FROM t, u v, w",
    "SELECT a FROM t, u RIGHT JOIN v ON u.a = v.a",
    "SELECT a FROM {oj t LEFT OUTER JOIN u ON t.a = u.a}",
    "SELECT a FROM t JOIN u ON t.a = u.a",
    "SELECT a FROM t LEFT JOIN u ON t.a = u.a FULL JOIN v USING (a)",
    "SELECT a FROM t NATURAL RIGHT JOIN u",
    "SELECT a FROM t OFFSET 5 ROWS FETCH NEXT 10 ROWS ONLY",
    "SELECT a FROM t FETCH FIRST 10 ROWS ONLY",
    "SELECT a FROM t FETCH FIRST 10 /* c */ ROWS /* d */ ONLY",
    "SELECT a FROM t OFFSET 5 ROWS",
    "SELECT a FROM t LIMIT ALL",
    "SELECT a FROM t LIMIT ALL OFFSET 3",
    "SELECT a FROM t LIMIT 5 OFFSET 3 /* c */ ROWS",
    "SELECT a FROM t WHERE a %INLIST :l SIZE (((10)))",
    "INSERT t SET a = :x, b = 2",
    "INSERT INTO t SET a = 1 /* c */, b = 2",
    "INSERT %NOLOCK t (a) VALUES (1)",
    "UPDATE t (a, b) VALUES (1, 2) WHERE c = 3",
    "UPDATE t (a) SELECT b FROM u",
    "DELETE t WHERE a = 1",
    "DELETE %NOLOCK t",
    "COMMIT WORK",
    "ROLLBACK WORK",
    "%INTRANS",
    "TRUNCATE TABLE %NOLOCK t",
    "SELECT ABS(a), COALESCE(a, b), $LISTBUILD(a, b), CURRENT_TIMESTAMP(3) FROM t",
    "SELECT {fn ABS(a)}, {fn PI}, {fn NOW}, {fn CURDATE()} FROM t",
    "SELECT {fn CONVERT(a, SQL_VARCHAR)}, {fn IFNULL(a, b)}, {fn USER} FROM t",
    "SELECT {fn TIMESTAMPADD(SQL_TSI_DAY, 1, d)}, {fn SUBSTRING(a FROM 1 FOR 2)} FROM t",
    "SELECT {d '2020-01-01'}, {t '10:00:00'}, {ts :x} FROM t",
    "SELECT {fn /* c */ NOW() /* d */} FROM t",
    "SELECT %SQLUPPER a, UPPER a, %EXACT (a), %TRUNCATE(a, 3), %SQLSTRING(a, (3)) FROM t",
    "SELECT %SQLUPPER /* c */ a FROM t",
    "SELECT POSITION(a IN b), SUBSTRING(a FROM 1 FOR 2), SUBSTRING(a FROM 1) FROM t",
    "SELECT POSITION(a /* c */ IN /* d */ b) FROM t",
    "SELECT DATEADD(dd, 1, d), DATEDIFF((yyyy), d, e), DATEPART(sts, d), DATENAME('dw', d), DATE_TRUNC(:p, d) FROM t",
    "SELECT TRIM(a), TRIM(x FROM a), TRIM(LEADING FROM a), TRIM(TRAILING a), TRIM(BOTH x FROM a) FROM t",
    "SELECT CASE WHEN a = 1 THEN 'x' END, CASE a WHEN 1 THEN 'x' END FROM t",
    "SELECT $H, $J, $TL, $ZH, $ZJ, $ZTS, $ZTZ, $ZV, $ZNSPACE FROM t",
    "SELECT a _ b FROM t",
    "SELECT SUM(a) OVER (ORDER BY b ROWS 1 PRECEDING) FROM t",
    "SELECT ALL a, b AS x, c y FROM t u, v WHERE a != 1 AND b NOT> 2 OR c IS NOT NULL",
    "SELECT %SQLUPPER a, %TRUNCATE(a, 3), POSITION(a IN b), SUBSTRING(a FROM 1 FOR 2) FROM t",
    "SELECT {fn NOW()}, {fn PI}, {fn USER}, {fn CONVERT(a, SQL_VARCHAR)}, {d '2020-01-01'} FROM t",
    "SELECT TRIM(LEADING ' ' FROM a), TRIM(a), CASE WHEN a = 1 THEN 'x' END, CASE a WHEN 1 THEN 'x' ELSE 'y' END FROM t",
    "SELECT $H, $J, $ZTS, a _ b, COUNT(ALL a), COUNT(*), SUM(a) OVER (ORDER BY b ROWS 1 PRECEDING) FROM t",
    "SELECT DATEADD(dd, 1, d), DATEDIFF(yyyy, d, e), DATEPART(sts, d), TIMESTAMPADD(SQL_TSI_DAY, 1, d) FROM t",
    "SELECT {fn IFNULL(a, b)}, {fn CONVERT(:x, SQL_INTEGER)}, $LISTBUILD(a, b), $TSQL_SPID(), CURRENT_TIMESTAMP(3), %INTEXT a, %RUNTIMEOUT (a) FROM t",
    "SELECT DATEADD(( /* c */ MM /* d */ ), 1, d), TRIM(both /* c */ from a), %exact /* d */ ((a)), { /* e */ fn /* f */ USER } FROM t",
    "SELECT %SQLUPPER :x, {fn ABS(:y)}, DATEADD(:p, 1, :d), TRIM(:c FROM :s), {ts :when} INTO :a, :b, :c, :d, :e FROM t",
    "SELECT a FROM t WHERE a NOT BETWEEN 1 AND 2 AND b NOT IN (1, 2) AND c NOT LIKE 'x%' AND d IN (SELECT d FROM u) AND e = SOME (SELECT e FROM u)",
    "SELECT a FROM t WHERE a NOT[ 'x' ! b NOT] 'y' & c %INLIST :list SIZE ((10)) AND d NOT %MATCHES 'p' AND e IS NOT JSON",
    "SELECT a FROM t JOIN u ON t.a = u.a LEFT JOIN v ON v.b = u.b, w ORDER BY a ASC, b DESC",
    "SELECT a FROM {oj t LEFT OUTER JOIN u ON t.a = u.a}, v CROSS JOIN w NATURAL JOIN x",
    "SELECT TOP 5 a INTO :x FROM t ORDER BY a OFFSET 5 ROWS FETCH NEXT 10 ROWS ONLY",
    "SELECT a FROM t LIMIT ALL OFFSET 3",
    "SELECT DISTINCT a FROM t UNION %PARALLEL SELECT b FROM u",
    "WITH c AS (SELECT a FROM t) SELECT a FROM c WHERE EXISTS (SELECT 1 FROM u) AND FOR SOME (u AS e) (e.a = c.a)",
    "SELECT CAST(a AS VARCHAR(10)), CONVERT(VARCHAR, a), CONVERT(a, SQL_INTEGER), TO_VECTOR(a, DOUBLE, 3) FROM t",
    "SELECT JSON_OBJECT('k': a), XMLFOREST(a AS e), PREDICT(m), SEARCH_INDEX(t.i, 'x') FROM t WHERE %ID %FIND SEARCH_INDEX(t.i, 'x')",
    "SELECT a FROM t WHERE a *= b",
    "SELECT a /* a comment */ FROM t -- trailing\n",
    "  SELECT ALL /* inside the sugar */ a FROM t  ",
    "INSERT t SET a = :x, b = 2",
    "INSERT %NOLOCK INTO t (a, b) VALUES (1, 2)",
    "UPDATE t (a, b) VALUES (1, 2) WHERE c = 3",
    "UPDATE %NOCHECK t SET a = 1 WHERE b = 2",
    "DELETE t WHERE a = 1",
    "DELETE FROM t",
    "COMMIT WORK",
    "ROLLBACK WORK",
    "%INTRANS",
    "START TRANSACTION %COMMITMODE EXPLICIT",
    "DECLARE c CURSOR FOR SELECT a INTO :x FROM t WHERE b = :y",
    "OPEN c",
    "FETCH c INTO :x",
    "CLOSE c",
    ":r = CALL p(:a, :b)",
    "GRANT SELECT ON t TO u1",
    "REVOKE SELECT ON t FROM u1",
    "CREATE TABLE t (a INT PRIMARY KEY, b VARCHAR(10) NOT NULL)",
    "CREATE VIEW v AS SELECT a FROM t WITH CHECK OPTION",
    "DROP TABLE t",
];

fn full(text: &str) -> SQL {
    SQL::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
}

fn texts() -> impl Iterator<Item = &'static str> {
    STATEMENTS.iter().copied()
}

#[test]
fn the_core_text_reparses_under_the_core_grammar() {
    for text in texts() {
        let core_text = full(text).desugar().print();
        let reparsed =
            core::SQL::parse(&core_text).unwrap_or_else(|e| panic!("{text}\n{core_text}: {e}"));
        assert_eq!(reparsed.print(), core_text, "{text}");
    }
}

#[test]
fn desugaring_the_core_text_changes_nothing() {
    for text in texts() {
        let core_text = full(text).desugar().print();
        assert_eq!(full(&core_text).desugar().print(), core_text, "{text}");
    }
}

#[test]
fn every_src_node_maps_the_core_text_to_the_source() {
    for text in texts() {
        let desugared = full(text).desugar();
        let (core_text, mappings) = print_mapped(&desugared);
        for mapping in &mappings {
            let within =
                |range: &std::ops::Range<usize>, len| range.start <= range.end && range.end <= len;
            assert!(within(&mapping.origin, text.len()), "{text}: {mapping:?}");
            assert!(
                within(&mapping.printed, core_text.len()),
                "{text}: {mapping:?}"
            );
        }
        // Innermost first: the statement, around the others, comes last.
        let stat = mappings.last().unwrap();
        assert_eq!(stat.origin, full(text).stat.range(), "{text}");
        assert_eq!(
            &core_text[stat.printed.clone()],
            desugared.stat.print(),
            "{text}"
        );
    }
}

/// Both EBNFs, with their rule counts, under `--nocapture`.
#[test]
fn the_core_grammar_has_fewer_rules() {
    let (full, core) = (SQL::bnf_rules(), core::SQL::bnf_rules());
    for (level, rules) in [("full", &full), ("core", &core)] {
        println!("{level}: {} rules", rules.len());
        for rule in rules {
            println!("{rule}");
        }
    }
    assert!(core.len() < full.len());
}

/// The local parity inputs when present, `--test sql_core -- --ignored --nocapture`.
#[test]
#[ignore]
fn the_local_statements_round_trip_through_the_core() {
    let Ok(json) = std::fs::read_to_string("/tmp/sqlparity/inputs.json") else {
        return;
    };
    let statements: Vec<String> = serde_json::from_str(&json).unwrap();
    let (mut parsed, mut failures) = (0, Vec::new());
    for text in &statements {
        let Ok(sql) = SQL::parse(text) else {
            continue;
        };
        parsed += 1;
        let core_text = sql.desugar().print();
        match core::SQL::parse(&core_text) {
            Ok(reparsed) if reparsed.print() == core_text => {}
            Ok(_) => failures.push(format!("prints differently: {text}")),
            Err(e) => failures.push(format!("{e}: {text}")),
        }
        if full(&core_text).desugar().print() != core_text {
            failures.push(format!("desugars again: {text}"));
        }
    }
    println!(
        "{parsed} of {} statements parse under the full grammar; {} fail to round-trip",
        statements.len(),
        failures.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn src_nodes_map_their_print_to_their_source() {
    let text = "SELECT a, b FROM t WHERE a = 1 ORDER BY b";
    let (printed, mappings) = print_mapped(&SQL::parse(text).unwrap());
    assert_eq!(printed, text);
    assert!(mappings.iter().all(|m| m.printed == m.origin));
    let origins: Vec<&str> = mappings.iter().map(|m| &text[m.origin.clone()]).collect();
    for node in [
        "a",
        "b",
        "t",
        "a = 1",
        "SELECT a, b FROM t WHERE a = 1 ORDER BY b",
    ] {
        assert!(origins.contains(&node), "{node} is not among {origins:?}");
    }
}

fn selex(text: &str) -> Src<Selex> {
    let sql = SQL::parse(text).unwrap();
    let Stat::Selct(stat) = sql.stat.node else {
        panic!("{text}")
    };
    let Some(QTerm::Selex(selex)) = stat.body.query.0.items.into_iter().next() else {
        panic!("{text}")
    };
    selex
}

#[test]
fn terms_take_their_core_shapes() {
    let shapes: Vec<&str> = selex(
        "SELECT CURRENT_DATE, USER, SYSDATE, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP(3), USERNAME, \
         -a, %SQLUPPER a, %TRUNCATE(a, 3), CAST(a AS INT), TIMESTAMPADD(SQL_TSI_DAY, 1, d), \
         CONVERT(a, SQL_VARCHAR), CONVERT(VARCHAR, a), f(a) FROM t",
    )
    .node
    .selst
    .select_list
    .0
    .items
    .iter()
    .map(|item| match &item.node {
        SelectItem::Expr {
            expr: Scalx(terms), ..
        } => match &terms.items[0].node {
            Term::Builtin(_) => "builtin",
            Term::ScalarFunction(_) => "function",
            Term::Call(_) => "call",
            Term::Niladic(..) => "niladic",
            Term::Column(_) => "column",
            Term::Unary { .. } => "unary",
            Term::PercentUnary { .. } => "percent",
            Term::StringOperation(_) => "string operation",
            _ => "other",
        },
        SelectItem::Star(_) => "*",
    })
    .collect();
    assert_eq!(
        shapes,
        [
            "niladic",
            "niladic",
            "niladic",
            "niladic",
            "builtin",
            "column",
            "unary",
            "percent",
            "string operation",
            "builtin",
            "builtin",
            "function",
            "function",
            "call",
        ]
    );
}

#[test]
fn conditions_hold_their_operands() {
    let selex = selex(
        "SELECT a FROM t WHERE a BETWEEN 1 AND 2 AND b IS NOT NULL AND c = ANY (SELECT c FROM u) \
         AND d NOT IN (1, 2) AND %NOINDEX e > 3",
    );
    let shapes: Vec<&str> = selex.node.where_clause.as_ref().unwrap().1.prefixed.0.items[0]
        .0
        .items
        .iter()
        .map(|cond| match &cond.node {
            Cond::Predicate {
                predicate:
                    Predicate::Comparison {
                        lhs,
                        tail: Comparison::Between { lower, upper, .. },
                        ..
                    },
                ..
            } => {
                assert_eq!(
                    (lhs.print(), lower.print(), upper.print()),
                    ("a".into(), "1".into(), "2".into())
                );
                "between"
            }
            Cond::Predicate {
                predicate:
                    Predicate::Comparison {
                        tail:
                            Comparison::IsNull {
                                null_kw: Negatable::Negated(..),
                                ..
                            },
                        ..
                    },
                ..
            } => "is not null",
            Cond::Predicate {
                predicate:
                    Predicate::Comparison {
                        tail:
                            Comparison::Query {
                                op:
                                    QuantifiedOp::Op {
                                        quantifier: Quantifier::Any(_),
                                        ..
                                    },
                                ..
                            },
                        ..
                    },
                ..
            } => "quantified",
            Cond::Predicate {
                predicate:
                    Predicate::Comparison {
                        tail:
                            Comparison::In {
                                in_kw: Negatable::Negated(..),
                                ..
                            },
                        ..
                    },
                ..
            } => "not in",
            Cond::Predicate {
                noindex: Some(_),
                predicate:
                    Predicate::Comparison {
                        tail: Comparison::Scalx { expr, .. },
                        ..
                    },
            } => {
                assert_eq!(expr.print(), "3");
                "compare"
            }
            _ => "other",
        })
        .collect();
    assert_eq!(
        shapes,
        ["between", "is not null", "quantified", "not in", "compare"]
    );
}

#[test]
fn malformed_nested_parens_fail_fast() {
    let sql = format!(
        "SELECT a FROM t WHERE {}a = = 1{}",
        "(".repeat(20),
        ")".repeat(20)
    );
    let start = std::time::Instant::now();
    let _ = SQL::parse(&sql);
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
}

#[test]
fn alter_table_inside_esql_parens() {
    assert!(tygr_objectscript::cos::Command::parse("&sql(ALTER TABLE t ADD COLUMN x INT)").is_ok());
}

#[test]
fn one_order_direction() {
    assert!(SQL::parse("SELECT a FROM t ORDER BY a ASC DESC").is_err());
}

#[test]
fn through_foreign_server() {
    // Not a corpus document: the class would compile only with the server defined.
    assert!(SQL::parse("THROUGH FOREIGN SERVER fs SELECT a FROM t").is_ok());
}
