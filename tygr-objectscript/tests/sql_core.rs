//! The core level of the SQL grammar and the desugaring into it, over hand-written statements: the
//! core text re-parses under the core grammar and prints the same, desugaring it again changes
//! nothing, and every `Src` node maps a range of the core text to a range of the source.

use tygr::{Parse, Print};
use tygr_objectscript::sql::desugar::Desugar;
use tygr_objectscript::sql::{SQL, core, print_mapped};

const STATEMENTS: &[&str] = &[
    "SELECT ALL a, b AS x, c y FROM t u, v WHERE a != 1 AND b NOT> 2 OR c IS NOT NULL",
    "SELECT %SQLUPPER a, %TRUNCATE(a, 3), POSITION(a IN b), SUBSTRING(a FROM 1 FOR 2) FROM t",
    "SELECT {fn NOW()}, {fn PI}, {fn USER}, {fn CONVERT(a, SQL_VARCHAR)}, {d '2020-01-01'} FROM t",
    "SELECT TRIM(LEADING ' ' FROM a), TRIM(a), CASE WHEN a = 1 THEN 'x' END, CASE a WHEN 1 THEN 'x' ELSE 'y' END FROM t",
    "SELECT $H, $J, $ZTS, a _ b, COUNT(ALL a), COUNT(*), SUM(a) OVER (ORDER BY b ROWS 1 PRECEDING) FROM t",
    "SELECT DATEADD(dd, 1, d), DATEDIFF(yyyy, d, e), DATEPART(sts, d), TIMESTAMPADD(SQL_TSI_DAY, 1, d) FROM t",
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

// Until the first `#[sugar]` mark: both levels parse the same text, and desugaring changes nothing.
#[test]
fn without_a_mark_the_levels_are_one_grammar() {
    for text in STATEMENTS {
        assert_eq!(full(text).desugar().print(), *text, "{text}");
        core::SQL::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
    }
}

#[test]
fn the_core_text_reparses_under_the_core_grammar() {
    for text in STATEMENTS {
        let core_text = full(text).desugar().print();
        let reparsed =
            core::SQL::parse(&core_text).unwrap_or_else(|e| panic!("{text}\n{core_text}: {e}"));
        assert_eq!(reparsed.print(), core_text, "{text}");
    }
}

#[test]
fn desugaring_the_core_text_changes_nothing() {
    for text in STATEMENTS {
        let core_text = full(text).desugar().print();
        assert_eq!(full(&core_text).desugar().print(), core_text, "{text}");
    }
}

#[test]
fn every_src_node_maps_the_core_text_to_the_source() {
    for text in STATEMENTS {
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
