//! The IRIS SQL grammar for `&sql(...)` blocks embedded in ObjectScript, reached from `cos.rs`.
//! Syntax only: `ascot` reports parse errors here but does not type-check SQL yet.

pub mod grammar;
pub mod leaf;
pub mod src;

pub use grammar::*;
pub use leaf::*;
pub use src::*;

#[cfg(test)]
mod tests {
    use super::*;
    use tygr::Parse;

    fn all_ok(cases: &[&str]) {
        let failed: Vec<_> = cases.iter().filter(|c| SQL::parse(c).is_err()).collect();
        assert!(failed.is_empty(), "failed to parse: {failed:#?}");
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
    fn in_subquery() {
        all_ok(&[
            "SELECT a FROM t WHERE x IN (SELECT y FROM u)",
            "SELECT a FROM t WHERE x IN (1,2,3)",
            "SELECT a FROM t WHERE x IN (:a,:b)",
            "SELECT a FROM t WHERE x IN ((SELECT y FROM u))",
            "SELECT a FROM t WHERE x NOT IN (SELECT y FROM u WHERE z = 1)",
        ]);
    }

    #[test]
    fn window_functions() {
        all_ok(&[
            "SELECT ROW_NUMBER() OVER (ORDER BY Name) FROM t",
            "SELECT SUM(x) OVER (PARTITION BY d) FROM t",
            "SELECT COUNT(*) OVER () FROM t",
            "SELECT COUNT(*), COUNT( * ), SUM(x), MyFunc(1, 2) FROM t",
            "SELECT COUNT(DISTINCT a), MAX(b), LIST(c) FROM t",
        ]);
    }

    #[test]
    fn cast_types() {
        all_ok(&[
            "SELECT CAST(a AS INTEGER) FROM t",
            "SELECT CAST(a AS INT) FROM t",
            "SELECT CAST(a AS DECIMAL(10,2)) FROM t",
            "SELECT CAST(a AS DEC) FROM t",
            "SELECT CAST(a AS NUMERIC(10, 2)) FROM t",
            "SELECT CAST(a AS TIMESTAMP) FROM t",
            "SELECT CAST(a AS TIME) FROM t",
            "SELECT CAST(a AS DATETIME) FROM t",
            "SELECT CAST(a AS DATE) FROM t",
            "SELECT CAST(a AS SMALLDATETIME) FROM t",
            "SELECT CAST(a AS CHARACTER(10)) FROM t",
            "SELECT CAST(a AS CHARACTER VARYING(10)) FROM t",
            "SELECT CAST(a AS CHAR VARYING) FROM t",
            "SELECT CAST(a AS CHAR(5)) FROM t",
            "SELECT CAST(a AS MONEY) FROM t",
            "SELECT CAST(a AS SMALLMONEY) FROM t",
            "SELECT CAST(a AS POSIXTIME) FROM t",
            "SELECT CAST(a AS VARCHAR(50)) FROM t",
            "SELECT CAST(a AS BINARY VARYING(10)) FROM t",
        ]);
    }

    #[test]
    fn non_reserved_aliases() {
        all_ok(&[
            "SELECT p.Name AS Name FROM Sample.Person p",
            "SELECT a AS Value, b AS Text, c AS Number, d Version FROM t",
            "SELECT a AS State, b AS Year, c AS Month, d AS Day FROM t",
            "SELECT a AS Comment, b AS Result, c AS Path, d AS Size FROM t",
            "SELECT a AS Source, b AS Mode, c AS Owner FROM t",
            "SELECT * FROM Sample.Person Path WHERE Path.Age > 1",
            "SELECT a FROM t GROUP BY a HAVING COUNT(*) > 1",
            "SELECT a FROM t ORDER BY a, b DESC",
            "SELECT a FROM t WHERE a = 1 ORDER BY a",
            "SELECT a FROM t UNION SELECT b FROM u",
            "SELECT a, b INTO :x, :y FROM t",
            "UPDATE t SET a = 1 WHERE b = 2",
            "DELETE FROM t WHERE b = 2",
        ]);
    }

    #[test]
    fn datatype_specs() {
        all_ok(&[
            "CREATE TABLE t (a INTEGER)",
            "CREATE TABLE t (a INT)",
            "CREATE TABLE t (a TIMESTAMP)",
            "CREATE TABLE t (a TIME)",
            "CREATE TABLE t (a DATETIME)",
            "CREATE TABLE t (a DATE)",
            "CREATE TABLE t (a DECIMAL(10,2))",
            "CREATE TABLE t (a DEC)",
            "CREATE TABLE t (a CHARACTER(10))",
            "CREATE TABLE t (a CHAR(10))",
            "CREATE TABLE t (a LONGVARCHAR)",
            "CREATE TABLE t (a LONG VARCHAR)",
            "CREATE TABLE t (a LONGVARBINARY)",
            "CREATE TABLE t (a LONG)",
            "CREATE TABLE t (a VARCHAR2(10))",
            "CREATE TABLE t (a VARCHAR(10))",
            "CREATE TABLE t (a NATIONAL CHARACTER VARYING(10))",
            "CREATE TABLE t (a NATIONAL VARCHAR(10))",
            "CREATE TABLE t (a %Library.String(MAXLEN=50))",
            "CREATE FUNCTION F(x INT) RETURNS INTEGER LANGUAGE OBJECTSCRIPT { QUIT x }",
        ]);
    }

    #[test]
    fn procedure_names_and_bodies() {
        all_ok(&[
            "CREATE PROCEDURE Sample.MyProc(IN a INT) LANGUAGE OBJECTSCRIPT { QUIT }",
            "DROP PROCEDURE Sample.MyProc",
            "CREATE PROCEDURE P() LANGUAGE OBJECTSCRIPT { IF a { SET b = \"}\" } QUIT }",
            "CREATE TABLE t (a INT, b INT COMPUTECODE { SET {b} = 1 })",
        ]);
    }

    #[test]
    fn alter_table_inside_esql_parens() {
        all_ok(&[
            "ALTER TABLE t ADD COLUMN x INT",
            "ALTER TABLE t ADD (x INT, y INT)",
            "ALTER TABLE t MODIFY x VARCHAR(10)",
            "ALTER TABLE t MODIFY (x VARCHAR(10), y INT)",
            "ALTER TABLE t ADD x INT, y INT",
            "ALTER TABLE t ADD CONSTRAINT c UNIQUE (a)",
        ]);
        assert!(crate::cos::Command::parse("&sql(ALTER TABLE t ADD COLUMN x INT)").is_ok());
    }

    #[test]
    fn nullable_first_variants() {
        all_ok(&[
            "CREATE UNIQUE INDEX i ON t (a)",
            "CREATE BITMAP INDEX i ON t (a)",
            "CREATE BITSLICE INDEX i ON t (a)",
            "CREATE COLUMNAR INDEX i ON t (a)",
            "CREATE INDEX i ON t (a)",
            "CREATE TABLE t (a INT) WITH %CLASSPARAMETER DEFAULTGLOBAL = '^x'",
        ]);
    }

    #[test]
    fn table_valued_functions() {
        all_ok(&[
            "SELECT * FROM Sample.SP_Sample_By_Name('A')",
            "SELECT * FROM JSON_TABLE('[1]', '$' COLUMNS (a INT PATH '$'))",
            "SELECT * FROM Sample.Person",
            "SELECT * FROM Sample.Person p, Sample.Company c",
            "SELECT * FROM (SELECT a FROM t) x",
            "SELECT * FROM (t1 JOIN t2 ON t1.a = t2.a)",
        ]);
    }

    #[test]
    fn host_variables() {
        all_ok(&[
            "SELECT a INTO :row() FROM t",
            "SELECT a, b INTO :arr() FROM t",
            "SELECT a FROM t WHERE b = :obj.Prop.Sub",
            "SELECT a FROM t WHERE b = :obj.Prop.Sub.Deep",
            "SELECT a FROM t WHERE b = :i%Prop()",
            "SELECT a FROM t WHERE b = :i%%Data",
            "SELECT a FROM t WHERE b = :Person.\"Home City\"",
            "SELECT a FROM t WHERE b = :naïve",
            "SELECT a FROM t WHERE b = :x",
            "SELECT a FROM t WHERE b = :%x",
            "SELECT a FROM t WHERE b = :arr(1,2)",
            "SELECT a FROM t WHERE b = :arr(\"k\")",
            "SELECT a FROM t WHERE b = :arr(:i)",
            "SELECT a FROM t WHERE b = :obj.Prop",
            "SELECT a FROM t WHERE b = :oref.%Prop",
            "SELECT a FROM t WHERE b = :i%Name",
            "SELECT a FROM t WHERE b = :request.GetValueAt(\"PID:SetIDPID\")",
        ]);
    }

    #[test]
    fn row_limiting() {
        all_ok(&[
            "SELECT a FROM t LIMIT 10",
            "SELECT a FROM t ORDER BY a LIMIT 10 OFFSET 5",
            "SELECT a FROM t OFFSET 5 ROWS FETCH NEXT 10 ROWS ONLY",
            "SELECT a FROM t LIMIT :n",
            "SELECT a FROM t LIMIT (10)",
            "SELECT a FROM t x ORDER BY a LIMIT 10 OFFSET 5",
        ]);
    }

    #[test]
    fn joins() {
        all_ok(&[
            "SELECT * FROM t1 NATURAL JOIN t2",
            "SELECT * FROM t1 NATURAL INNER JOIN t2",
            "SELECT * FROM t1 NATURAL LEFT OUTER JOIN t2",
            "SELECT * FROM t1 LEFT JOIN t2 ON t1.a = t2.a",
            "SELECT * FROM t1 CROSS JOIN t2",
        ]);
    }

    #[test]
    fn distinct_by() {
        all_ok(&[
            "SELECT DISTINCT BY (Name) Name, Age FROM t",
            "SELECT DISTINCT Name FROM t",
            "SELECT COUNT(DISTINCT BY (a) b) FROM t",
        ]);
    }

    #[test]
    fn column_primary_key() {
        all_ok(&[
            "CREATE TABLE t (a INT PRIMARY KEY)",
            "CREATE TABLE t (a INT PRIMARY KEY, b INT NOT NULL)",
            "CREATE TABLE t (a INT, PRIMARY KEY (a))",
        ]);
    }

    #[test]
    fn odbc_escapes() {
        all_ok(&[
            "SELECT a FROM t WHERE d = {ts '2020-01-01 00:00:00'}",
            "SELECT a FROM t WHERE d = {d '2020-01-01'}",
            "SELECT a FROM t WHERE d = {t '00:00:00'}",
        ]);
    }

    #[test]
    fn misc_low() {
        all_ok(&[
            "SELECT TRIM(LEADING ' ' FROM Name) FROM t",
            "SELECT TRIM(BOTH FROM Name) FROM t",
            "SELECT TRIM(' ' FROM Name) FROM t",
            "SELECT TRIM(Name) FROM t",
            "SELECT TRIM(BOTH ' ' FROM Name) FROM t",
            "SELECT COUNT( * ) FROM t",
            "DROP INDEX t.i",
            "DROP INDEX Sample.Person.NameIdx",
            "DROP INDEX i ON t",
            "DROP INDEX i",
            "CREATE QUERY Q(a INT) LANGUAGE OBJECTSCRIPT EXECUTE (INOUT QHandle BINARY(255)) { QUIT } FETCH (INOUT QHandle BINARY(255), INOUT Row %List, INOUT AtEnd INT) { QUIT } CLOSE (INOUT QHandle BINARY(255)) { QUIT }",
            "SELECT $$$MyMacro(1) FROM t",
            "SELECT $$$MyMacro FROM t",
            "SELECT a FROM t WHERE a not> 1 OR a not< 2 OR a not[ 'x' OR a %contains ('y')",
            "SELECT a FROM t WHERE a NOT> 1 OR a %CONTAINS ('y', 'z') OR a %CONTAINSTERM ('w')",
            "SELECT a FROM t WHERE a NOT] 'x' AND a %STARTSWITH 'y' AND a <= 1",
            "( SELECT a FROM t ) UNION ( SELECT b FROM u )",
        ]);
    }

    #[test]
    fn create_trigger() {
        all_ok(&[
            "CREATE TRIGGER trg AFTER INSERT ON t LANGUAGE OBJECTSCRIPT { QUIT }",
            "CREATE OR REPLACE TRIGGER TrigBUOF BEFORE UPDATE OF Home_Street,Home_City ON Sample.Person REFERENCING OLD ROW AS o NEW ROW AS n FOR EACH ROW WHEN (o.Age > 1) LANGUAGE SQL BEGIN INSERT INTO log VALUES (1); END",
            "CREATE TRIGGER TrigAD AFTER UPDATE,DELETE ORDER 4 ON Sample.Person FOR EACH STATEMENT BEGIN INSERT INTO log VALUES (1); END",
            "DROP TRIGGER trg",
            "DROP TRIGGER IF EXISTS trg FROM t",
        ]);
    }

    #[test]
    fn special_system_variable_full_names() {
        all_ok(&[
            "SELECT $HOROLOG + 1 INTO :x",
            "SELECT $H + 1 INTO :x",
            "SELECT Name INTO :x FROM t WHERE a < $HOROLOG",
            "SELECT $JOB, $TLEVEL, $ZHOROLOG, $ZJOB, $ZVERSION INTO :a, :b, :c, :d, :e",
            "SELECT $J, $TL, $ZH, $ZJ, $ZV INTO :a, :b, :c, :d, :e",
        ]);
    }

    #[test]
    fn window_order_direction_and_offsets() {
        all_ok(&[
            "SELECT ROW_NUMBER() OVER (ORDER BY Name DESC) FROM t",
            "SELECT NTH_VALUE(Name, 2) OVER (ORDER BY Name) FROM t",
            "SELECT LAG(Name, 1) OVER (ORDER BY Name) FROM t",
            "SELECT LEAD(a, 1, 0) OVER (PARTITION BY Age ORDER BY Name ASC, a DESC) FROM t",
            "SELECT COUNT(*) OVER (PARTITION BY Age) FROM t",
        ]);
    }

    #[test]
    fn all_before_expression() {
        all_ok(&[
            "SELECT ALL Name FROM t",
            "SELECT COUNT(ALL Name) FROM t",
            "SELECT ALLOWED FROM t",
            "SELECT COUNT(ALLOWED) FROM t",
        ]);
    }

    #[test]
    fn for_some_with_blank_before_condition() {
        all_ok(&[
            "SELECT a FROM t WHERE FOR SOME (u AS e) (e.a = t.a)",
            "SELECT a FROM t WHERE FOR SOME (u)(u.a = t.a)",
        ]);
    }

    #[test]
    fn odbc_escape_blank_before_close() {
        all_ok(&[
            "SELECT {d '2020-01-01' } FROM t",
            "SELECT {fn NOW() } FROM t",
            "SELECT {fn NOW()} + 1 FROM t",
            "SELECT p.Name FROM {oj t p LEFT OUTER JOIN u c ON p.a = c.a}",
            "SELECT p.Name FROM {oj t p LEFT OUTER JOIN u c ON p.a = c.a }",
        ]);
    }

    #[test]
    fn grant_revoke_objects() {
        all_ok(&[
            "GRANT USE ON FOREIGN SERVER s1 TO u1",
            "GRANT USE ON FOREIGN SERVERS s1, s2 TO u1",
            "GRANT USE ON SERVER s1 TO u1",
            "GRANT SELECT ON Servers TO u1",
            "GRANT SELECT ON Server TO u1",
            "GRANT SELECT ON ServerLog TO u1",
            "GRANT SELECT ON Servers1 TO u1",
            "GRANT SELECT ON Servers, Other TO u1",
            "grant select on cubes c1, c2 to u1",
            "REVOKE SELECT ON CUBE c1 FROM u1",
            "REVOKE SELECT ON CUBES c1, c2 FROM u1",
            "REVOKE USE ON FOREIGN SERVER s1 FROM u1",
            "REVOKE SELECT ON Servers FROM u1",
            "REVOKE SELECT ON SCHEMA SQLUser FROM u1",
            "REVOKE SELECT ON * FROM u1",
        ]);
    }

    #[test]
    fn qualified_index_names() {
        all_ok(&[
            "SELECT Name FROM %IGNOREINDEX t.NameIdx t",
            "SELECT Name FROM %IGNOREINDEX SQLUser.t.NameIdx SQLUser.t",
            "SELECT Name FROM %IGNOREINDEX NameIdx t",
            "SELECT Name FROM %IGNOREINDEX SQLUser.t.* SQLUser.t",
            "SELECT Name FROM t WHERE %ID %FIND SEARCH_INDEX(t.NameIdx, 'foo')",
        ]);
    }

    #[test]
    fn reserved_words_are_not_aliases() {
        all_ok(&["SELECT Name %INTOLIST :x FROM t", "SELECT a FROM %NOBSL t"]);
        assert!(SQL::parse("SELECT 1 AS Partition INTO :x").is_err());
        assert!(SQL::parse("SELECT a FROM t %NOBSL").is_err());
        assert!(SQL::parse("SELECT a FROM t Partition").is_err());
    }

    #[test]
    fn longer_keyword_before_its_prefix() {
        all_ok(&[
            "CREATE TABLE t (a TIMESTAMP DEFAULT CURRENT_TIMESTAMP)",
            "CREATE TABLE t (a TIMESTAMP DEFAULT CURRENT_TIMESTAMP(3))",
            "CREATE TABLE t (a TIME DEFAULT CURRENT_TIME)",
            "CREATE TABLE t (a TIMESTAMP ON UPDATE CURRENT_TIMESTAMP)",
            "CREATE TABLE t (a TIME ON UPDATE CURRENT_TIME)",
            "ALTER TABLE t CONVERT SHARDED",
            "ALTER TABLE t CONVERT SHARD",
        ]);
    }

    #[test]
    fn view_check_option_level() {
        all_ok(&[
            "CREATE VIEW v AS SELECT a FROM t WITH CASCADED CHECK OPTION",
            "CREATE VIEW v AS SELECT a FROM t WITH CASCADE CHECK OPTION",
            "CREATE VIEW v AS SELECT a FROM t WITH LOCAL CHECK OPTION",
            "CREATE VIEW v AS SELECT a FROM t WITH CHECK OPTION",
        ]);
        assert!(SQL::parse("CREATE VIEW v AS SELECT a FROM t WITH CHECK CASCADED OPTION").is_err());
        assert!(SQL::parse("CREATE VIEW v AS SELECT a FROM t WITH CHECK LOCAL OPTION").is_err());
    }

    #[test]
    fn blanks_between_adjacent_clauses() {
        all_ok(&[
            "CREATE TABLE t (a INT, b INT, FOREIGN KEY (a) REFERENCES u (a) ON DELETE CASCADE ON UPDATE NO ACTION)",
            "CREATE TABLE t (a INT, FOREIGN KEY (a) REFERENCES u (a) ON UPDATE NO ACTION ON DELETE CASCADE)",
            "CREATE USER u IDENTIFIED BY 'pw'",
            "CREATE USER u IDENTIFY BY pw",
            "ALTER USER u PASSWORD 'pw'",
            "ALTER USER u WITH PASSWORD 'pw'",
            "THROUGH FOREIGN SERVER fs SELECT a FROM t",
            "CREATE INDEX i ON t (a) WITH DATA (b) PAD_INDEX",
            "CREATE INDEX i ON t (a) WITH PAD_INDEX, FILLFACTOR = 80",
            "CREATE INDEX i ON t (a) WITH DATA (b), PAD_INDEX",
        ]);
    }

    #[test]
    fn alter_column_clauses_are_not_types() {
        all_ok(&[
            "ALTER TABLE t ALTER COLUMN b NOT NULL",
            "ALTER TABLE t ALTER COLUMN b SET DEFAULT 1",
            "ALTER TABLE t ALTER COLUMN b DROP DEFAULT",
            "ALTER TABLE t ALTER COLUMN b RENAME c",
            "ALTER TABLE t ALTER COLUMN b COLLATE %EXACT",
            "ALTER TABLE t MODIFY b DEFAULT 'x' NOT NULL",
            "ALTER TABLE t ALTER COLUMN b %String(MAXLEN=20)",
            "ALTER TABLE t ALTER COLUMN b %Library.String(MAXLEN=20)",
            "CREATE TABLE t (a %String(MAXLEN=20), n NUMERIC(10,2))",
        ]);
    }

    #[test]
    fn package_qualified_class_names() {
        all_ok(&[
            "CREATE INDEX i ON t (a) AS %iFind.Index.Basic",
            "DROP PROCEDURE IF EXISTS P FROM Pkg.Cls",
            "DROP FUNCTION f FROM Pkg.Cls",
            "CREATE PROCEDURE P() FOR Pkg.Cls LANGUAGE OBJECTSCRIPT { QUIT }",
        ]);
    }

    #[test]
    fn optional_clauses() {
        all_ok(&[
            "CREATE TABLE t (a INT) SHARD KEY (a)",
            "CREATE TABLE t (a INT) PARTITION BY RANGE (a) INTERVAL 10",
            "CREATE TABLE t (a INT) SHARD KEY (a) PARTITION BY RANGE (a) INTERVAL 10",
            "CREATE TABLE t (a INT) SHARD KEY (a) WITH %CLASSPARAMETER DEFAULTGLOBAL = '^x'",
            "CREATE TABLE t (a INT) IN dbspace",
            "CREATE TABLE t (a INT) EXTENT SIZE 16",
            "DROP FOREIGN SERVER fs",
            "DROP FOREIGN SERVER fs CASCADE",
            "DROP FOREIGN TABLE ft",
            "CREATE TABLE t (a VARCHAR(10) %TRUNCATE(5))",
            "CREATE TABLE t (a VARCHAR(10) %TRUNCATE(5) ALPHAUP)",
            "CREATE TABLE t (a VARCHAR(10) COLLATE %TRUNCATE(5))",
            "CREATE FOREIGN SERVER fs FOREIGN DATA WRAPPER CSV HOST '/tmp'",
            "CREATE FUNCTION f() RETURNS INT LANGUAGE SQL BEGIN RETURN 1; END",
            "CREATE FUNCTION f() RETURNS INT LANGUAGE SQL BEGIN INSERT INTO log VALUES (1); RETURN 1; END",
        ]);
    }

    #[test]
    fn alter_table_alternatives() {
        all_ok(&[
            "ALTER TABLE t DROP ALL PARTITIONS",
            "ALTER TABLE t DROP PARTITION ID '1'",
            "ALTER TABLE t TRUNCATE PARTITION ID '1'",
            "ALTER TABLE t DROP PARTITION ('x')",
            "ALTER TABLE t TRUNCATE PARTITION ('x')",
            "ALTER TABLE t DROP PARTITION BETWEEN 1 AND 2",
            "ALTER TABLE t MOVE PARTITION ('x') TO db",
            "ALTER TABLE t DROP COLUMN b",
            "ALTER TABLE t DROP b, c",
            "ALTER TABLE t ADD FOREIGN KEY fk (a) REFERENCES u (a)",
            "ALTER TABLE t ADD FOREIGN KEY (a) REFERENCES u (a)",
            "ALTER TABLE t CONVERT STORAGETYPE = COLUMNAR",
            "ALTER TABLE t CONVERT TO STORAGETYPE = COLUMNAR",
        ]);
        assert!(SQL::parse("ALTER TABLE t CONVERT COLUMNAR").is_err());
        assert!(SQL::parse("ALTER TABLE t DROP PARTITION HASH (a) IN 4 PARTITIONS").is_err());
    }

    #[test]
    fn single_tokens_as_iris_takes_them() {
        all_ok(&[
            "DROP AGGREGATE IF EXISTS Foo.Bar",
            "DROP AGGREGATE Foo.Bar",
            "CREATE ML CONFIGURATION c PROVIDER PMML",
            "CREATE ML CONFIGURATION c PROVIDER AutoML USING {\"x\":1}",
            "CREATE MODEL m PREDICTING (a) FROM t",
            "CREATE MODEL m PREDICTING (a) FROM t USING {\"seed\":3}",
            "CREATE MODEL m PREDICTING (a) FROM (SELECT a FROM t)",
            "TRAIN MODEL m FROM t",
            "VALIDATE MODEL m FROM t",
            "CREATE TABLE t (a TIMESTAMP DEFAULT GETUTCDATE())",
            "TUNE TABLE t %SAMPLE_PERCENT .5",
            "TUNE TABLE t %SAMPLE_PERCENT 1",
            "TUNE TABLE t %SAMPLE_PERCENT 0.25",
            "LOAD DATA FROM FILE '/x.csv' INTO t USING {\"from\":{\"file\":{\"header\":true}}}",
            "LOAD DATA FROM FILE '/x.csv' INTO t USING '{\"from\":{\"file\":{\"header\":true}}}'",
            "CREATE FOREIGN TABLE ft (a INT) SERVER fs TABLE 'x'",
            "CREATE FOREIGN TABLE ft (a INT) SERVER fs FILE 'x.csv' COLUMNS (a INT) USING {\"x\":1}",
            "CREATE FOREIGN TABLE ft (a INT) SERVER fs QUERY 'SELECT 1'",
            "CREATE FOREIGN TABLE ft (a INT) SERVER fs TABLE 'x' USING '{\"x\":1}'",
        ]);
        for rejected in [
            "DROP IF EXISTS AGGREGATE Foo.Bar",
            "CREATE ML CONFIGURATION c PROVIDER PMMML",
            "CREATE ML CONFIGURATION c PROVIDER AutoML USING '{\"x\":1}'",
            "TUNE TABLE t %SAMPLE_PERCENT 2",
            "TUNE TABLE t %SAMPLE_PERCENT 0",
            "TUNE TABLE t %SAMPLE_PERCENT 25%",
        ] {
            assert!(SQL::parse(rejected).is_err(), "{rejected}");
        }
    }
}
