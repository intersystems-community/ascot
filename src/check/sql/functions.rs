//! The functions IRIS provides to a call by name and the arguments each takes (2026.2, probed one by
//! one: `ROUND(a)`, `CONCAT(a, b, c)`, `ABS(a, 1)`, `LAST_IDENTITY(1)`, `COALESCE()`, `$LENGTH(a, b, c)`
//! and `%EXACT(a, 1)` are refused). The functions with a syntax of their own are the grammar's.

/// The least and, unless unbounded, the most arguments `name` takes; `name` upper case, `$` included.
pub(super) fn signature(name: &str) -> Option<(usize, Option<usize>)> {
    Some(match name {
        "LAST_IDENTITY" | "NOW" | "PI" | "CURDATE" | "CURTIME" | "$TSQL_DB_NAME"
        | "$TSQL_ERROR" | "$TSQL_FETCH_STATUS" | "$TSQL_GETUTCDATE" | "$TSQL_HOST_NAME"
        | "$TSQL_LOCK_TIMEOUT" | "$TSQL_NESTLEVEL" | "$TSQL_SERVERNAME" | "$TSQL_SPID"
        | "$TSQL_SQLSTATUS" | "$TSQL_TRANCOUNT" | "$TSQL_VERSION" | "$TSQL_NEWID" => (0, Some(0)),
        "GETDATE" | "GETUTCDATE" | "UNIX_TIMESTAMP" => (0, Some(1)),
        "%ALPHAUP" | "%EXACT" | "%EXTERNAL" | "%INTERNAL" | "%INTEXT" | "%MINUS" | "%MVR"
        | "%ODBCIN" | "%ODBCOUT" | "%PLUS" | "%RUNTIMEIN" | "%RUNTIMEOUT" | "UPPER" | "%OBJECT"
        | "%OID" | "ABS" | "ACOS" | "ASCII" | "ASIN" | "ATAN" | "CEILING" | "CHAR"
        | "CHAR_LENGTH" | "CHARACTER_LENGTH" | "COS" | "COT" | "DATALENGTH" | "DATE" | "DAY"
        | "DAYNAME" | "DAYOFMONTH" | "DAYOFWEEK" | "DAYOFYEAR" | "DEGREES" | "EXP" | "FLOOR"
        | "HOUR" | "ISNUMERIC" | "LAST_DAY" | "LCASE" | "LEN" | "LENGTH" | "LOG" | "LOG10"
        | "LOWER" | "LTRIM" | "MINUTE" | "MONTH" | "MONTHNAME" | "QUARTER" | "RADIANS"
        | "REVERSE" | "RTRIM" | "SECOND" | "SIGN" | "SIN" | "SPACE" | "SQRT" | "TAN"
        | "TO_NUMBER" | "UCASE" | "WEEK" | "YEAR" | "$LISTLENGTH" | "$TSQL_OBJECT_ID"
        | "$TSQL_OBJECT_NAME" => (1, Some(1)),
        "%STRING" | "%SQLSTRING" | "%SQLUPPER" | "%TRUNCATE" | "EMBEDDING" | "TOCHAR"
        | "TODATE" | "TO_CHAR" | "TO_DATE" | "TO_TIMESTAMP" | "$LENGTH" | "$LISTDATA"
        | "$LISTFROMSTRING" | "$LISTTOSTRING" => (1, Some(2)),
        "STR" | "$EXTRACT" | "$FNUMBER" | "$LIST" | "$LISTGET" => (1, Some(3)),
        "%SIMILARITY" | "ATAN2" | "CONCAT" | "HASHBYTES" | "ISNULL" | "LEFT" | "MOD" | "NULLIF"
        | "NVL" | "PATINDEX" | "POWER" | "REPEAT" | "REPLICATE" | "RIGHT" | "TRUNCATE"
        | "VECTOR_COSINE" | "VECTOR_DOT_PRODUCT" | "$LISTSAME" | "$MVFMT" | "$MVFMTS"
        | "$MVICONV" | "$MVICONVS" | "$MVOCONV" | "$MVOCONVS" => (2, Some(2)),
        "CHARINDEX" | "CONVERT" | "IFNULL" | "LPAD" | "ROUND" | "RPAD" | "SUBSTR" | "SUBSTRING"
        | "$FIND" | "$JUSTIFY" | "$LISTFIND" | "$TRANSLATE" => (2, Some(3)),
        "INSTR" | "$PIECE" => (2, Some(4)),
        "REPLACE" => (3, Some(3)),
        "STUFF" => (4, Some(4)),
        "COALESCE" | "DECODE" | "GREATEST" | "LEAST" | "STRING" => (1, None),
        "$LISTBUILD" => (0, None),
        _ => return None,
    })
}

/// The functions whose second argument, a truncation length, IRIS takes as a literal or a host variable
/// alone (`%SQLUPPER(a, b)` is "LITERAL expected").
pub(super) const LITERAL_LENGTH: &[&str] = &["%STRING", "%SQLSTRING", "%SQLUPPER", "%TRUNCATE"];
