//! The leaves every level of the SQL grammar shares: character classes, blanks and comments,
//! identifiers, literals, and host variables and parameters.

use either::Either;
use tygr::*;

use crate::convert::VecSep;
use crate::cos;

char_class!(pub IsSpace, "simple_space", |ch| matches!(
    ch,
    ' ' | '\t' | '\n' | '\r'
));
pub type Space1 = Hidden<StringOf1<IsSpace>>;

char_class!(pub IsSimpleLatinLetter, "simple latin letter", |ch| {
    ch.is_ascii_alphabetic()
});

char_class!(pub IsNonAsciiLetter, "non-ASCII letter", |ch| {
    !ch.is_ascii() && cos::is_letter(ch)
});

char_class!(pub IsNotDQ, "non double-quote character", |ch| ch != '"');
char_class!(pub IsNotSQ, "non single-quote character", |ch| ch != '\'');
char_class!(pub IsDigit, "digit", |ch| ch.is_ascii_digit());
char_class!(pub IsNotNL, "not_newline", |ch| ch != '\n');
char_class!(pub IsNotStar, "not_star", |ch| ch != '*');
char_class!(pub IsAnyChar, "any_char", |_ch| true);

// tygr 0.2.0 dropped its own version of this (previously public) helper;
// reimplemented locally against tygr's still-public building blocks.
pub type StringEndsWith<T> = Raw<(Vec<(NotFollowedBy<T>, IsAnyChar)>, T)>;

pub type SQLLineComment = Wrap<StringEq!("--"), StringOf<IsNotNL>, Option<StringEq!("\n")>>;

pub type SQLMultiLineComment = Prefix<StringEq!("/*"), StringEndsWith<StringEq!("*/")>>;

char_class!(pub IsExtFuncChar, "non left paren character", |ch| {
    !matches!(ch, '(' | '$')
});

char_class!(pub IsDbIdentStart, "db_identifier start", |ch| {
    ch.is_ascii_alphabetic() || ch == '_'
});

char_class!(pub IsDbIdentPart, "db_identifier part", |ch| {
    ch.is_ascii_alphabetic() || ch.is_ascii_digit() || ch == '_' || ch == '-'
});

char_class!(pub IsNotParen, "any tokens except \"(\" and \")\"", |ch| ch
    != '('
    && ch != ')');

#[derive(Grammar, Clone)]
#[tygr(hidden)]
pub enum SpaceUnit {
    Raw(Space1),
    LC(SQLLineComment),
    MC(SQLMultiLineComment),
}

#[derive(Grammar, Clone)]
#[tygr(hidden)]
pub struct Space(Vec<SpaceUnit>);

#[derive(Grammar, Clone)]
pub enum Ident {
    Simple(Identifier),
    Delimited(DelimitedIdentifier),
}

#[derive(Grammar, Clone)]
#[tygr(validated, inline)]
pub struct SQLIdentNonKeyword(pub Ident);

impl Validate for SQLIdentNonKeyword {
    const REQUIREMENT: &'static str = "not be a reserved keyword";
    fn validate(&self) -> bool {
        match &self.0 {
            Ident::Delimited(_) => true, // Delimited identifiers can be anything
            Ident::Simple(s) => {
                let text = s.0.to_string().to_uppercase();
                !is_sql_reserved_keyword(&text)
            }
        }
    }
}

// The words an alias cannot be: IRIS's reserved words, per
// https://docs.intersystems.com/irislatest/csp/docbook/DocBook.UI.Page.cls?KEY=RSQL_reservedwords,
// plus the clause starters LIMIT, OFFSET and ORDER, which IRIS does not reserve.
fn is_sql_reserved_keyword(word: &str) -> bool {
    matches!(
        word,
        "SELECT"
            | "FROM"
            | "WHERE"
            | "INSERT"
            | "UPDATE"
            | "DELETE"
            | "CREATE"
            | "DROP"
            | "ALTER"
            | "TABLE"
            | "SCHEMA"
            | "GRANT"
            | "REVOKE"
            | "AS"
            | "ON"
            | "IN"
            | "INTO"
            | "VALUES"
            | "SET"
            | "AND"
            | "OR"
            | "NOT"
            | "NULL"
            | "IS"
            | "LIKE"
            | "BETWEEN"
            | "EXISTS"
            | "ALL"
            | "ANY"
            | "SOME"
            | "UNION"
            | "INTERSECT"
            | "EXCEPT"
            | "JOIN"
            | "INNER"
            | "LEFT"
            | "RIGHT"
            | "FULL"
            | "OUTER"
            | "CROSS"
            | "NATURAL"
            | "GROUP"
            | "BY"
            | "HAVING"
            | "ORDER"
            | "ASC"
            | "DESC"
            | "DISTINCT"
            | "UNIQUE"
            | "CONSTRAINT"
            | "PRIMARY"
            | "FOREIGN"
            | "REFERENCES"
            | "CHECK"
            | "DEFAULT"
            | "CASE"
            | "WHEN"
            | "THEN"
            | "ELSE"
            | "END"
            | "WITH"
            | "FOR"
            | "TO"
            | "OF"
            | "INTEGER"
            | "INT"
            | "SMALLINT"
            | "NUMERIC"
            | "DECIMAL"
            | "DEC"
            | "FLOAT"
            | "REAL"
            | "DOUBLE"
            | "CHAR"
            | "CHARACTER"
            | "VARCHAR"
            | "NCHAR"
            | "VARYING"
            | "DATE"
            | "TIME"
            | "BIT"
            | "BEGIN"
            | "COMMIT"
            | "ROLLBACK"
            | "TRANSACTION"
            | "WORK"
            | "ISOLATION"
            | "LEVEL"
            | "READ"
            | "WRITE"
            | "ONLY"
            | "CASCADE"
            | "RESTRICT"
            | "USER"
            | "CURRENT_USER"
            | "SESSION_USER"
            | "SYSTEM_USER"
            | "CURRENT_DATE"
            | "CURRENT_TIME"
            | "CURRENT_TIMESTAMP"
            | "TRUE"
            | "FALSE"
            | "LIMIT"
            | "OFFSET"
            | "FETCH"
            | "FIRST"
            | "NEXT"
            | "ROWS"
            | "CURRENT"
            | "CAST"
            | "CONVERT"
            | "EXTRACT"
            | "SUBSTRING"
            | "TRIM"
            | "UPPER"
            | "LOWER"
            | "COALESCE"
            | "NULLIF"
            | "SUM"
            | "AVG"
            | "COUNT"
            | "MIN"
            | "MAX"
            | "USING"
            | "OPTION"
            | "PROCEDURE"
            | "DECLARE"
            | "CURSOR"
            | "OPEN"
            | "CLOSE"
            | "CONTINUE"
            | "GOTO"
            | "PUBLIC"
            | "TEMPORARY"
            | "GLOBAL"
            | "LOCAL"
            | "COLLATE"
            | "NATIONAL"
            | "EXECUTE"
            | "EXEC"
            | "PREPARE"
            | "DEALLOCATE"
            | "GET"
            | "DIAGNOSTICS"
            | "FOUND"
            | "SQLSTATE"
            | "SQLERROR"
            | "ABSOLUTE"
            | "RELATIVE"
            | "SCROLL"
            | "PRESERVE"
            | "DEFERRABLE"
            | "DEFERRED"
            | "IMMEDIATE"
            | "AUTHORIZATION"
            | "PRIVILEGES"
            | "DESCRIPTOR"
            | "ALLOCATE"
            | "MATCH"
            | "PARTIAL"
            | "CONNECT"
            | "CONNECTION"
            | "DISCONNECT"
            | "SECTION"
            | "DOMAIN"
            | "MODULE"
            | "LANGUAGE"
            | "EXTERNAL"
            | "INPUT"
            | "OUTPUT"
            | "ESCAPE"
            | "INDICATOR"
            | "ASSERTION"
            | "CORRESPONDING"
            | "OVERLAPS"
            | "PAD"
            | "SPACE"
            | "PRIOR"
            | "ROLE"
            | "TIMEZONE_HOUR"
            | "TIMEZONE_MINUTE"
            | "BIT_LENGTH"
            | "CHAR_LENGTH"
            | "CHARACTER_LENGTH"
            | "OCTET_LENGTH"
            | "SYSDATE"
            | "ADD"
            | "BOTH"
            | "DESCRIBE"
            | "EXCEPTION"
            | "GO"
            | "HOUR"
            | "IDENTITY"
            | "INITIALLY"
            | "INSENSITIVE"
            | "INTERVAL"
            | "LAST"
            | "MINUTE"
            | "NAMES"
            | "NO"
            | "SECOND"
            | "STATISTICS"
            | "TOP"
            | "TRAILING"
            | "WHENEVER"
            | "%AFTERHAVING"
            | "%ALLINDEX"
            | "%ALPHAUP"
            | "%ALTER"
            | "%BEGTRANS"
            | "%CHECKPRIV"
            | "%CLASSNAME"
            | "%CLASSPARAMETER"
            | "%DBUGFULL"
            | "%DELDATA"
            | "%DESCRIPTION"
            | "%EXACT"
            | "%EXTERNAL"
            | "%FILE"
            | "%FIRSTTABLE"
            | "%FLATTEN"
            | "%FOREACH"
            | "%FULL"
            | "%ID"
            | "%IDADDED"
            | "%IGNOREINDEX"
            | "%IGNOREINDICES"
            | "%INLIST"
            | "%INORDER"
            | "%INTERNAL"
            | "%INTEXT"
            | "%INTOLIST"
            | "%INTRANS"
            | "%INTRANSACTION"
            | "%KEY"
            | "%MATCHES"
            | "%MCODE"
            | "%MERGE"
            | "%MINUS"
            | "%MVR"
            | "%NOBSL"
            | "%NOCHECK"
            | "%NODELDATA"
            | "%NOFIXEDSTATS"
            | "%NOFLATTEN"
            | "%NOFPLAN"
            | "%NOINDEX"
            | "%NOLOCK"
            | "%NOMERGE"
            | "%NOPARALLEL"
            | "%NOREDUCE"
            | "%NORUNTIME"
            | "%NOSVSO"
            | "%NOTOPOPT"
            | "%NOTRIGGER"
            | "%NOUNIONOROPT"
            | "%NUMROWS"
            | "%ODBCIN"
            | "%ODBCOUT"
            | "%PARALLEL"
            | "%PLUS"
            | "%PROFILE"
            | "%PROFILE_ALL"
            | "%PUBLICROWID"
            | "%ROUTINE"
            | "%ROWCOUNT"
            | "%RUNTIMEIN"
            | "%RUNTIMEOUT"
            | "%SQLSTRING"
            | "%SQLUPPER"
            | "%STARTSWITH"
            | "%STARTTABLE"
            | "%STRING"
            | "%TABLENAME"
            | "%TRUNCATE"
            | "%UPPER"
            | "%VALUE"
            | "%VID"
            | "ARE"
            | "AT"
            | "CONSTRAINTS"
            | "ENDEXEC"
            | "LEADING"
            | "PARTITION"
            | "SHARD"
    )
}

#[derive(Grammar, Clone)]
#[tygr(name = "identifier start")]
pub enum IdentifierStart {
    SimpleLatin(IsSimpleLatinLetter),
    P(StringEq!("%")),
    U(StringEq!("_")),
}

#[derive(Grammar, Clone)]
#[tygr(name = "identifier part")]
pub enum IdentifierPart {
    Digit(IsDigit),
    SimpleLatin(IsSimpleLatinLetter),
    H(StringEq!("#")),
    A(StringEq!("@")),
    U(StringEq!("_")),
}

#[derive(Grammar, Clone)]
#[tygr(inline)]
pub struct IdentifierStruct {
    first: IdentifierStart,
    rest: Vec<IdentifierPart>,
}

#[derive(Grammar, Clone)]
pub struct Identifier(Raw<IdentifierStruct>);

#[derive(Grammar, Clone)]
#[tygr(inline)]
pub struct SQLDelimitedIdentStruct {
    open: StringEq!("\""),
    head: DelimitedIdentifierPart,
    tail: Vec<DelimitedIdentifierPart>,
    close: StringEq!("\""),
}

#[derive(Grammar, Clone)]
#[tygr(name = "delimited identifier part")]
pub enum DelimitedIdentifierPart {
    DDQ(StringEq!("\"\"")),
    Char(IsNotDQ),
}

#[derive(Grammar, Clone)]
#[tygr(name = "delimited identifier")]
pub struct DelimitedIdentifier(Raw<SQLDelimitedIdentStruct>);

#[derive(Grammar, Clone)]
#[tygr(inline)]
pub struct DottedIdent {
    pub head: Ident,
    pub rest: Vec<(StringEq!("."), Ident)>,
}

impl DottedIdent {
    pub fn parts(&self) -> impl Iterator<Item = &Ident> {
        std::iter::once(&self.head).chain(self.rest.iter().map(|(_, ident)| ident))
    }
}

pub type StdStringLiteral = Wrap<StringEq!("'"), Vec<StdCharacterRepresentation>, StringEq!("'")>;

#[derive(Grammar, Clone)]
#[tygr(name = "std character representation")]
pub enum StdCharacterRepresentation {
    DSQ(StringEq!("''")),
    Char(IsNotSQ),
}

#[derive(Grammar, Clone)]
#[tygr(name = "string literal")]
pub enum StringLiteral {
    SQLEmpty(StringEq!("''")),
    COSEmpty(StringEq!("\"\"")),
    Std(Raw<StdStringLiteral>),
}

#[derive(Grammar, Clone)]
pub struct Integer {
    sign: Option<Either<StringEq!("+"), StringEq!("-")>>,
    integer: StringOf1<IsDigit>,
}

#[derive(Grammar, Clone)]
#[tygr(inline)]
pub struct SQLExponent {
    e: ExpoSymbol,
    sign: Option<Either<StringEq!("+"), StringEq!("-")>>,
    digits: StringOf1<IsDigit>,
}

#[derive(Grammar, Clone)]
#[tygr(name = "expo symbol")]
pub enum ExpoSymbol {
    U(StringEq!("E")),
    L(StringEq!("e")),
}

#[derive(Grammar, Clone)]
pub struct Number {
    base: NumberBase,
    exponent: Option<SQLExponent>,
}

#[derive(Grammar, Clone)]
pub enum NumberBase {
    Int {
        digits: StringOf1<IsDigit>,
        fraction: Option<(StringEq!("."), StringOf<IsDigit>)>,
    },
    Dot {
        dot: StringEq!("."),
        digits: StringOf1<IsDigit>,
    },
}

#[derive(Grammar, Clone)]
pub enum Literal {
    String(StringLiteral),
    Numeric(Number),
}

#[derive(Grammar, Clone)]
#[tygr(name = "parameter marker")]
pub struct ParameterMarker(pub StringEq!("?"));

#[derive(Grammar, Clone)]
#[tygr(name = "host var")]
pub struct HostVar {
    pub host_ref: HostRef,
    pub subs: Option<SubList>,
}

pub type SubList =
    Wrap<(StringEq!("("), Space), Option<VecSep<Sub, ArgSep>>, (Space, StringEq!(")"))>;

#[derive(Grammar, Clone)]
#[tygr(name = "instance variable host ref")]
pub enum InstanceVariableHostRef {
    I {
        preifx: (Option<StringEq!(":")>, StringEq!("i%")),
        name: HostIdentifier,
    },
    R {
        preifx: (Option<StringEq!(":")>, StringEq!("r%")),
        name: HostIdentifier,
    },
}

#[derive(Grammar, Clone)]
#[tygr(name = "property host ref")]
pub struct PropertyHostRef {
    colon: Option<StringEq!(":")>,
    pub part1: HostIdentifier,
    pub parts: Vec1<(StringEq!("."), Either<HostIdentifier, DelimitedIdentifier>)>,
}

#[derive(Grammar, Clone)]
pub enum Var {
    HostRef(HostRef),
    Ident(Ident),
}

#[derive(Grammar, Clone)]
#[tygr(name = "host ref")]
pub enum HostRef {
    InstanceVariableHostRef(InstanceVariableHostRef),
    PropertyHostRef(PropertyHostRef),
    Plain(Option<StringEq!(":")>, HostIdentifier),
}

#[derive(Grammar, Clone)]
#[tygr(name = "host identifier")]
pub struct HostIdentifier(Raw<(IdentifierStart, Vec<HostIdentifierPart>)>);

#[derive(Grammar, Clone)]
#[tygr(name = "host identifier part")]
pub enum HostIdentifierPart {
    Digit(IsDigit),
    SimpleLatin(IsSimpleLatinLetter),
    Unicode(IsNonAsciiLetter),
}

#[derive(Grammar, Clone)]
pub enum Param {
    HostVar(HostVar),
    Ident(Ident, Option<SubList>),
}

#[derive(Grammar, Clone)]
pub enum Sub {
    Literal(Literal),
    Var(Var),
}

pub type ArgSep = (Space, StringEq!(","), Space);
