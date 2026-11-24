//! The core level of the SQL grammar and the desugaring into it, over hand-written statements: each
//! rule rewrites its statement to the core text given, the core text re-parses under the core grammar
//! and prints the same, desugaring it again changes nothing, and every `Src` node maps a range of
//! the core text to a range of the source.

use tygr::{Parse, Print};
use tygr_objectscript::sql::desugar::Desugar;
use tygr_objectscript::sql::grammar::core;
use tygr_objectscript::sql::{SQL, print_mapped};

/// One statement per rule, with the exact core text.
const RULES: &[(&str, &str)] = &[
    (
        "SELECT a FROM t WHERE a != 1",
        "SELECT a FROM t WHERE a <> 1",
    ),
    (
        "SELECT a FROM t WHERE a NOT= 1",
        "SELECT a FROM t WHERE a <> 1",
    ),
    (
        "SELECT a FROM t WHERE a NOT> 1",
        "SELECT a FROM t WHERE a <= 1",
    ),
    (
        "SELECT a FROM t WHERE a NOT< 1",
        "SELECT a FROM t WHERE a >= 1",
    ),
    (
        "SELECT a FROM t WHERE a NOT[ 'x'",
        "SELECT a FROM t WHERE NOT (a [ 'x')",
    ),
    (
        "SELECT a FROM t WHERE a NOT] 'x'",
        "SELECT a FROM t WHERE NOT (a ] 'x')",
    ),
    (
        "SELECT a FROM t WHERE a IS NOT NULL",
        "SELECT a FROM t WHERE NOT (a IS NULL)",
    ),
    (
        "SELECT a FROM t WHERE a not /* c */ BETWEEN 1 AND 2",
        "SELECT a FROM t WHERE not /* c */ (a BETWEEN 1 AND 2)",
    ),
    (
        "SELECT a FROM t WHERE a NOT IN (1, 2)",
        "SELECT a FROM t WHERE NOT (a IN (1, 2))",
    ),
    (
        "SELECT a FROM t WHERE a NOT LIKE 'x%'",
        "SELECT a FROM t WHERE NOT (a LIKE 'x%')",
    ),
    (
        "SELECT a FROM t WHERE a NOT %MATCHES 'p'",
        "SELECT a FROM t WHERE NOT (a %MATCHES 'p')",
    ),
    (
        "SELECT a FROM t WHERE a NOT %INLIST :l",
        "SELECT a FROM t WHERE NOT (a %INLIST :l)",
    ),
    (
        "SELECT a FROM t WHERE a IS NOT JSON",
        "SELECT a FROM t WHERE NOT (a IS JSON)",
    ),
    (
        "SELECT a FROM t WHERE NOT a IS NOT NULL",
        "SELECT a FROM t WHERE NOT NOT (a IS NULL)",
    ),
    (
        "SELECT a FROM t WHERE a IN (SELECT b FROM u)",
        "SELECT a FROM t WHERE a = ANY (SELECT b FROM u)",
    ),
    (
        "SELECT a FROM t WHERE a NOT IN (SELECT b FROM u)",
        "SELECT a FROM t WHERE NOT (a = ANY (SELECT b FROM u))",
    ),
    (
        "SELECT a FROM t WHERE a = SOME (SELECT b FROM u)",
        "SELECT a FROM t WHERE a = ANY (SELECT b FROM u)",
    ),
    (
        "SELECT a FROM t WHERE a NOT[ ANY (SELECT b FROM u)",
        "SELECT a FROM t WHERE NOT (a [ ALL (SELECT b FROM u))",
    ),
    (
        "SELECT a FROM t WHERE a NOT] ALL (SELECT b FROM u)",
        "SELECT a FROM t WHERE NOT (a ] ANY (SELECT b FROM u))",
    ),
    (
        "SELECT a FROM t WHERE a = 1 ! b = 2",
        "SELECT a FROM t WHERE a = 1 OR b = 2",
    ),
    (
        "SELECT a FROM t WHERE a = 1 & b = 2",
        "SELECT a FROM t WHERE a = 1 AND b = 2",
    ),
    ("SELECT ALL a FROM t", "SELECT a FROM t"),
    (
        "SELECT %NOLOCK ALL /* c */ a FROM t",
        "SELECT %NOLOCK /* c */ a FROM t",
    ),
    ("SELECT ALLOWED FROM t", "SELECT ALLOWED FROM t"),
    (
        "SELECT COUNT(ALL a), COUNT(ALL /* c */ b), COUNT(ALLOWED) FROM t",
        "SELECT COUNT(a), COUNT( /* c */ b), COUNT(ALLOWED) FROM t",
    ),
    (
        "SELECT a FROM t ORDER BY a ASC, b DESC",
        "SELECT a FROM t ORDER BY a, b DESC",
    ),
    ("SELECT a x FROM t u", "SELECT a AS x FROM t AS u"),
    (
        "SELECT a FROM t, u v, w",
        "SELECT a FROM t CROSS JOIN u AS v CROSS JOIN w",
    ),
    (
        "SELECT a FROM t, u RIGHT JOIN v ON u.a = v.a",
        "SELECT a FROM t CROSS JOIN (u RIGHT OUTER JOIN v ON u.a = v.a)",
    ),
    (
        "SELECT a FROM {oj t LEFT OUTER JOIN u ON t.a = u.a}",
        "SELECT a FROM t LEFT OUTER JOIN u ON t.a = u.a",
    ),
    (
        "SELECT a FROM t JOIN u ON t.a = u.a",
        "SELECT a FROM t INNER JOIN u ON t.a = u.a",
    ),
    (
        "SELECT a FROM t LEFT JOIN u ON t.a = u.a FULL JOIN v USING (a)",
        "SELECT a FROM t LEFT OUTER JOIN u ON t.a = u.a FULL OUTER JOIN v USING (a)",
    ),
    (
        "SELECT a FROM t NATURAL RIGHT JOIN u",
        "SELECT a FROM t NATURAL RIGHT OUTER JOIN u",
    ),
    (
        "SELECT a FROM t OFFSET 5 ROWS FETCH NEXT 10 ROWS ONLY",
        "SELECT a FROM t LIMIT 10 OFFSET 5",
    ),
    (
        "SELECT a FROM t FETCH FIRST 10 ROWS ONLY",
        "SELECT a FROM t LIMIT 10",
    ),
    (
        "SELECT a FROM t FETCH FIRST 10 /* c */ ROWS /* d */ ONLY",
        "SELECT a FROM t LIMIT /* c */  /* d */ 10",
    ),
    (
        "SELECT a FROM t OFFSET 5 ROWS",
        "SELECT a FROM t LIMIT ALL OFFSET 5",
    ),
    ("SELECT a FROM t LIMIT ALL", "SELECT a FROM t"),
    (
        "SELECT a FROM t LIMIT ALL OFFSET 3",
        "SELECT a FROM t LIMIT ALL OFFSET 3",
    ),
    (
        "SELECT a FROM t LIMIT 5 OFFSET 3 /* c */ ROWS",
        "SELECT a FROM t LIMIT 5 /* c */ OFFSET 3",
    ),
    (
        "SELECT a FROM t WHERE a %INLIST :l SIZE ((10))",
        "SELECT a FROM t WHERE a %INLIST :l SIZE (10)",
    ),
    (
        "SELECT a FROM t WHERE a %INLIST :l SIZE 10",
        "SELECT a FROM t WHERE a %INLIST :l SIZE (10)",
    ),
    (
        "INSERT t SET a = :x, b = 2",
        "INSERT INTO t (a, b) VALUES (:x, 2)",
    ),
    (
        "INSERT INTO t SET a = 1 /* c */, b = 2",
        "INSERT INTO t (a, b) VALUES (1 /* c */, 2)",
    ),
    (
        "INSERT %NOLOCK t (a) VALUES (1)",
        "INSERT %NOLOCK INTO t (a) VALUES (1)",
    ),
    (
        "UPDATE t (a, b) VALUES (1, 2) WHERE c = 3",
        "UPDATE t SET a = 1, b = 2 WHERE c = 3",
    ),
    (
        "UPDATE t (a) SELECT b FROM u",
        "UPDATE t (a) SELECT b FROM u",
    ),
    ("DELETE t WHERE a = 1", "DELETE FROM t WHERE a = 1"),
    ("DELETE %NOLOCK t", "DELETE %NOLOCK FROM t"),
    ("COMMIT WORK", "COMMIT"),
    ("ROLLBACK WORK", "ROLLBACK"),
    ("%INTRANS", "%INTRANSACTION"),
    ("TRUNCATE TABLE %NOLOCK t", "TRUNCATE TABLE %NOLOCK t"),
    (
        "SELECT ABS(a), COALESCE(a, b), $LISTBUILD(a, b), CURRENT_TIMESTAMP(3) FROM t",
        "SELECT ABS(a), COALESCE(a, b), $LISTBUILD(a, b), CURRENT_TIMESTAMP(3) FROM t",
    ),
    (
        "SELECT {fn ABS(a)}, {fn PI}, {fn NOW}, {fn USER}, {fn CURDATE()} FROM t",
        "SELECT ABS(a), PI(), NOW(), USER, CURDATE() FROM t",
    ),
    (
        "SELECT {fn CONVERT(a, SQL_VARCHAR)}, {fn IFNULL(a, b)} FROM t",
        "SELECT {fn CONVERT(a, SQL_VARCHAR)}, {fn IFNULL(a, b)} FROM t",
    ),
    (
        "SELECT {fn TIMESTAMPADD(SQL_TSI_DAY, 1, d)}, {fn SUBSTRING(a FROM 1 FOR 2)} FROM t",
        "SELECT TIMESTAMPADD(SQL_TSI_DAY, 1, d), SUBSTRING(a, 1, 2) FROM t",
    ),
    (
        "SELECT {d '2020-01-01'}, {t '10:00:00'}, {ts :x} FROM t",
        "SELECT CAST('2020-01-01' AS DATE), CAST('10:00:00' AS TIME), CAST(:x AS TIMESTAMP) FROM t",
    ),
    (
        "SELECT {fn /* c */ NOW() /* d */} FROM t",
        "SELECT NOW /* c */ ( /* d */) FROM t",
    ),
    (
        "SELECT %SQLUPPER a, UPPER a, %EXACT (a), %TRUNCATE(a, 3), %SQLSTRING(a, (3)) FROM t",
        "SELECT %SQLUPPER(a), UPPER(a), %EXACT(a), %TRUNCATE(a, 3), %SQLSTRING(a, 3) FROM t",
    ),
    (
        "SELECT %SQLUPPER /* c */ a FROM t",
        "SELECT %SQLUPPER /* c */ (a) FROM t",
    ),
    (
        "SELECT POSITION(a IN b), SUBSTRING(a FROM 1 FOR 2), SUBSTRING(a FROM 1) FROM t",
        "SELECT CHARINDEX(a, b), SUBSTRING(a, 1, 2), SUBSTRING(a, 1) FROM t",
    ),
    (
        "SELECT POSITION(a /* c */ IN /* d */ b) FROM t",
        "SELECT CHARINDEX(a /* c */ , /* d */ b) FROM t",
    ),
    (
        "SELECT DATEADD(dd, 1, d), DATEDIFF((yyyy), d, e), DATEPART(sts, d), DATENAME('dw', d), DATE_TRUNC(:p, d) FROM t",
        "SELECT DATEADD('day', 1, d), DATEDIFF('year', d, e), DATEPART('sqltimestamp', d), DATENAME('dw', d), DATE_TRUNC(:p, d) FROM t",
    ),
    (
        "SELECT TRIM(a), TRIM(x FROM a), TRIM(LEADING FROM a), TRIM(TRAILING a), TRIM(BOTH x FROM a) FROM t",
        "SELECT TRIM(BOTH ' ' FROM a), TRIM(BOTH x FROM a), TRIM(LEADING ' ' FROM a), TRIM(TRAILING ' ' FROM a), TRIM(BOTH x FROM a) FROM t",
    ),
    (
        "SELECT CASE WHEN a = 1 THEN 'x' END, CASE a WHEN 1 THEN 'x' END FROM t",
        "SELECT CASE WHEN a = 1 THEN 'x' ELSE NULL END, CASE a WHEN 1 THEN 'x' ELSE NULL END FROM t",
    ),
];

/// Sugar the core grammar admits, as the rewrite drops a node whose type the core keeps, rewrites
/// one form of a node whose other forms stay, or spells a call by name, whose arguments the core
/// reads as columns.
const ADMITTED: &[&str] = &[
    "SELECT a FROM t LIMIT ALL",
    "UPDATE t (a, b) VALUES (1, 2) WHERE c = 3",
    "SELECT DATEADD(dd, 1, d), DATEDIFF((yyyy), d, e), DATEPART(sts, d), DATENAME('dw', d), DATE_TRUNC(:p, d) FROM t",
];

/// Statements mixing the rules, and the sugar-free rest of the grammar.
const STATEMENTS: &[&str] = &[
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
    RULES
        .iter()
        .map(|(text, _)| *text)
        .chain(STATEMENTS.iter().copied())
}

#[test]
fn each_rule_rewrites_its_statement() {
    for (text, core_text) in RULES {
        assert_eq!(full(text).desugar().print(), *core_text, "{text}");
    }
}

#[test]
fn the_core_grammar_lacks_the_sugar() {
    for (text, core_text) in RULES {
        if text != core_text && !ADMITTED.contains(text) {
            assert!(core::SQL::parse(text).is_err(), "{text}");
        }
    }
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

/// The local parity inputs, when present:
/// `cargo test -p tygr-objectscript --test sql_core -- --ignored --nocapture`.
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
