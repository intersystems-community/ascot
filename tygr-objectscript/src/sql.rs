use either::Either;
use tygr::*;

char_class!(pub IsSpace, "simple_space", |ch| matches!(
    ch,
    ' ' | '\t' | '\n' | '\r'
));
pub type Space1 = Hidden<StringOf1<IsSpace>>;

char_class!(pub IsSimpleLatinLetter, "simple latin letter", |ch| {
    ch.is_ascii_alphabetic()
});

char_class!(pub IsNonAsciiLetter, "non-ASCII letter", |ch| {
    !ch.is_ascii() && ch.is_alphabetic()
});

char_class!(pub IsNotDQ, "non double-quote character", |ch| ch != '"');
char_class!(pub IsNotSQ, "non single-quote character", |ch| ch != '\'');
char_class!(pub IsDigit, "digit", |ch| ch.is_ascii_digit());
char_class!(pub IsNotNL, "not_newline", |ch| ch != '\n');
char_class!(pub IsNotStar, "not_star", |ch| ch != '*');
char_class!(pub IsAnyChar, "any_char", |_ch| true);

// tygr 0.2.0 dropped its own version of this (previously public) helper;
// reimplemented locally against tygr's still-public building blocks.
type StringEndsWith<T> = Raw<(Vec<(NotFollowedBy<T>, CharOf<IsAnyChar>)>, T)>;

pub type SQLLineComment = Wrap<StringEq!("--"), StringOf<IsNotNL>, Option<StringEq!("\n")>>;

pub type SQLMultiLineComment = Prefix<StringEq!("/*"), StringEndsWith<StringEq!("*/")>>;

#[derive(Grammar)]
#[grammar(hidden)]
pub enum SpaceUnit {
    Raw(Space1),
    LC(SQLLineComment),
    MC(SQLMultiLineComment),
}

#[derive(Grammar)]
#[grammar(hidden)]
pub struct Space(Vec<SpaceUnit>);

#[derive(Grammar)]
pub enum Ident {
    Simple(Identifier),
    Delimited(DelimitedIdentifier),
}

#[derive(Grammar)]
#[grammar(validated, inline)]
pub struct SQLIdentNonKeyword(Ident);

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

// SQL reserved words, per https://docs.intersystems.com/irislatest/csp/docbook/DocBook.UI.Page.cls?KEY=RSQL_reservedwords
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
            | "%INTRANS"
            | "%INTRANSACTION"
            | "%KEY"
            | "%MATCHES"
            | "%MCODE"
            | "%MERGE"
            | "%MINUS"
            | "%MVR"
            | "%NOCHECK"
            | "%NODELDATA"
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
            | "SHARD"
    )
}

#[derive(Grammar)]
#[grammar(name = "identifier start")]
pub enum IdentifierStart {
    SimpleLatin(CharOf<IsSimpleLatinLetter>),
    P(StringEq!("%")),
    U(StringEq!("_")),
}

#[derive(Grammar)]
#[grammar(name = "identifier part")]
pub enum IdentifierPart {
    Digit(CharOf<IsDigit>),
    SimpleLatin(CharOf<IsSimpleLatinLetter>),
    H(StringEq!("#")),
    A(StringEq!("@")),
    U(StringEq!("_")),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct IdentifierStruct {
    first: IdentifierStart,
    rest: Vec<IdentifierPart>,
}

#[derive(Grammar)]
pub struct Identifier(Raw<IdentifierStruct>);

#[derive(Grammar)]
#[grammar(inline)]
pub struct SQLDelimitedIdentStruct {
    open: StringEq!("\""),
    head: DelimitedIdentifierPart,
    tail: Vec<DelimitedIdentifierPart>,
    close: StringEq!("\""),
}

#[derive(Grammar)]
#[grammar(name = "delimited identifier part")]
pub enum DelimitedIdentifierPart {
    DDQ(StringEq!("\"\"")),
    Char(CharOf<IsNotDQ>),
}

#[derive(Grammar)]
#[grammar(name = "delimited identifier")]
pub struct DelimitedIdentifier(Raw<SQLDelimitedIdentStruct>);

#[derive(Grammar)]
#[grammar(inline)]
pub struct DottedIdent {
    head: Ident,
    rest: Vec<(StringEq!("."), Ident)>,
}

#[derive(Grammar)]
pub struct TName(DottedIdent);

#[derive(Grammar)]
pub struct Table(VecSep<Ident, (StringEq!("."), Option<StringEq!("_")>)>);

#[derive(Grammar)]
#[grammar(name = "SQL function name")]
pub struct SQLFunctionName(DottedIdent);

#[derive(Grammar)]
#[grammar(name = "colmn")]
pub enum Colmn {
    Str(Ident, Space, StringEq!("."), Space, StringEq!("*")),
    Arr(DottedIdent, Vec<ArrowRef>),
}

/// Arrow reference for object property access (Cache/IRIS extension)
pub type ArrowRef = Prefix<(Space, StringEq!("->"), Space), Ident>;

pub type StdStringLiteral = Wrap<StringEq!("'"), Vec<StdCharacterRepresentation>, StringEq!("'")>;

#[derive(Grammar)]
#[grammar(name = "std character representation")]
pub enum StdCharacterRepresentation {
    DSQ(StringEq!("''")),
    Char(CharOf<IsNotSQ>),
}

#[derive(Grammar)]
#[grammar(name = "string literal")]
pub enum StringLiteral {
    SQLEmpty(StringEq!("''")),
    COSEmpty(StringEq!("\"\"")),
    Std(Raw<StdStringLiteral>),
}

#[derive(Grammar)]
pub struct Integer {
    sign: Option<Either<StringEq!("+"), StringEq!("-")>>,
    integer: StringOf1<IsDigit>,
}

#[derive(Grammar)]
pub struct Precision(NonNegativeInteger);

#[derive(Grammar)]
#[grammar(name = "non-negative integer")]
pub struct NonNegativeInteger(StringOf1<IsDigit>);

#[derive(Grammar)]
#[grammar(name = "positive numeric literal value")]
pub struct PositivceNumericLiteralValue {
    sign: Option<StringEq!("+")>,
    integer: StringOf1<IsDigit>,
    fraction: Option<(StringEq!("."), StringOf1<IsDigit>)>,
    exponent: Option<SQLExponent>,
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct SQLExponent {
    e: ExpoSymbol,
    sign: Option<Either<StringEq!("+"), StringEq!("-")>>,
    digits: StringOf1<IsDigit>,
}

#[derive(Grammar)]
#[grammar(name = "expo symbol")]
pub enum ExpoSymbol {
    U(StringEq!("E")),
    L(StringEq!("e")),
}

#[derive(Grammar)]
pub struct Number {
    base: NumberBase,
    exponent: Option<SQLExponent>,
}

#[derive(Grammar)]
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

#[derive(Grammar)]
pub enum Literal {
    String(StringLiteral),
    Numeric(Number),
}

#[derive(Grammar)]
#[grammar(name = "parameter marker")]
pub struct ParameterMarker(pub StringEq!("?"));

#[derive(Grammar)]
#[grammar(name = "host var")]
pub struct HostVar {
    host_ref: HostRef,
    subs: Option<SubList>,
}

pub type SubList = Wrap<
    (StringEq!("("), Space),
    Option<VecSep<Sub, (Space, StringEq!(","), Space)>>,
    (Space, StringEq!(")")),
>;

#[derive(Grammar)]
#[grammar(name = "instance variable host ref")]
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

#[derive(Grammar)]
#[grammar(name = "property host ref")]
pub struct PropertyHostRef {
    colon: Option<StringEq!(":")>,
    part1: HostIdentifier,
    parts: Vec1<(StringEq!("."), Either<HostIdentifier, DelimitedIdentifier>)>,
}

#[derive(Grammar)]
pub enum Var {
    HostRef(HostRef),
    Ident(Ident),
}

#[derive(Grammar)]
#[grammar(name = "host ref")]
pub enum HostRef {
    InstanceVariableHostRef(InstanceVariableHostRef),
    PropertyHostRef(PropertyHostRef),
    Plain(Option<StringEq!(":")>, HostIdentifier),
}

#[derive(Grammar)]
#[grammar(name = "host identifier")]
pub struct HostIdentifier(Raw<(IdentifierStart, Vec<HostIdentifierPart>)>);

#[derive(Grammar)]
#[grammar(name = "host identifier part")]
pub enum HostIdentifierPart {
    Digit(CharOf<IsDigit>),
    SimpleLatin(CharOf<IsSimpleLatinLetter>),
    Unicode(CharOf<IsNonAsciiLetter>),
}

#[derive(Grammar)]
pub enum Param {
    Var(Var, Option<SubList>),
    InstanceVariableHostRef(InstanceVariableHostRef),
    PropertyHostRef(PropertyHostRef),
}

#[derive(Grammar)]
pub enum Sub {
    Literal(Literal),
    HostRef(HostRef),
    Ident(Ident),
}

#[derive(Grammar)]
#[grammar(name = "not equals operation")]
pub enum NotEqualsOperation {
    Diamond(StringEq!("<>")),
    BangEq(StringEq!("!=")),
    NotEq(StringEqCI!("NOT=")),
}

#[derive(Grammar)]
#[grammar(name = "order predicate")]
pub enum OrderPredicate {
    Le(Either<StringEq!("<="), StringEqCI!("NOT>")>),
    Ge(Either<StringEq!(">="), StringEqCI!("NOT<")>),
    Gt(StringEq!(">")),
    Lt(StringEq!("<")),
}

#[derive(Grammar)]
#[grammar(name = "contains predicate")]
pub enum ContainsPredicate {
    LB(StringEq!("[")),
    NotLB(StringEqCI!("NOT[")),
}

#[derive(Grammar)]
#[grammar(name = "follows predicate")]
pub enum FollowsPredicate {
    Follows(StringEq!("]")),
    NotFollows(StringEqCI!("NOT]")),
}

#[derive(Grammar)]
#[grammar(name = "cmp op")]
pub enum CmpOp {
    StartsWith(StringEqCI!("%STARTSWITH")),
    Eq(StringEq!("=")),
    Ne(NotEqualsOperation),
    Ord(OrderPredicate),
    Contains(ContainsPredicate),
    Follows(FollowsPredicate),
}

#[derive(Grammar)]
#[grammar(name = "unary op")]
pub enum UnaryOp {
    Plus(StringEq!("+")),
    Minus(StringEq!("-")),
    AlphaUp(StringEqCI!("%ALPHAUP")),
    Exact(StringEqCI!("%EXACT")),
    External(StringEqCI!("%EXTERNAL")),
    Internal(StringEqCI!("%INTERNAL")),
    IntExt(StringEqCI!("%INTEXT")),
    MinusOp(StringEqCI!("%MINUS")),
    Mvr(StringEqCI!("%MVR")),
    OdbcIn(StringEqCI!("%ODBCIN")),
    OdbcOut(StringEqCI!("%ODBCOUT")),
    PlusOp(StringEqCI!("%PLUS")),
    RuntimeIn(StringEqCI!("%RUNTIMEIN")),
    RuntimeOut(StringEqCI!("%RUNTIMEOUT")),
    Upper(Either<StringEqCI!("%UPPER"), StringEqCI!("UPPER")>),
}

#[derive(Grammar)]
#[grammar(name = "term")]
pub enum Term {
    Macro(
        StringEq!("$$$"),
        Identifier,
        Option<crate::cos::MacroArgList>,
    ),
    Literal(Literal),
    ScalarFunction(Box<ScalarFunction>),
    Window(Box<Window>),
    Agg(Box<Agg>),
    Unary(Unary),
    XTFun(Box<XTFun>),
    ParameterMarker(ParameterMarker),
    Case(Box<Case>),
    OdbcEscape(Box<ODBCEscape>),
    Scalx {
        open: (StringEq!("("), Space),
        expr: Box<Scalx>,
        close: (Space, StringEq!(")")),
    },
    Query {
        open: (StringEq!("("), Space),
        query: Box<Query>,
        close: (Space, StringEq!(")")),
    },
    UserDefinedFunction(UserDefinedFunction),
    Column(Colmn),
    SpecialSystemVariable(SpecialSystemVariable),
    HostVar(HostVar),
    Null(StringEqCI!("NULL")),
    CurrentDate(StringEqCI!("CURRENT_DATE")),
    User(StringEqCI!("USER")),
}

#[derive(Grammar)]
pub enum Unary {
    UnaryOp {
        op: UnaryOp,
        space: Space,
        term: Box<Term>,
    },
    StringOperation(Box<StringOperation>),
}

#[derive(Grammar)]
#[grammar(name = "%string operation")]
pub enum StringOperation {
    Term(StringOperator, Space, Term),
    ScalxColTrun {
        op: StringOperator,
        open: (Space, StringEq!("("), Space),
        expr: Scalx,
        comma: (Space, StringEq!(","), Space),
        truncation: ColTrun,
        close: (Space, StringEq!(")")),
    },
}

#[derive(Grammar)]
#[grammar(name = "%string operator")]
pub enum StringOperator {
    String(StringEqCI!("%STRING")),
    SqlString(StringEqCI!("%SQLSTRING")),
    SqlUpper(StringEqCI!("%SQLUPPER")),
    Truncate(StringEqCI!("%TRUNCATE")),
}

#[derive(Grammar)]
pub enum ColTrun {
    Const(PConst),
    HostVar(HostVar),
}

#[derive(Grammar)]
#[grammar(name = "special system variable")]
pub enum SpecialSystemVariable {
    H(StringEqCI!("$H")),
    Horolog(StringEqCI!("$HOROLOG")),
    J(StringEqCI!("$J")),
    Job(StringEqCI!("$JOB")),
    Namespace(StringEqCI!("$NAMESPACE")),
    TL(StringEqCI!("$TL")),
    TLevel(StringEqCI!("$TLEVEL")),
    Username(StringEqCI!("$USERNAME")),
    ZH(StringEqCI!("$ZH")),
    ZHorolog(StringEqCI!("$ZHOROLOG")),
    ZJ(StringEqCI!("$ZJ")),
    ZJob(StringEqCI!("$ZJOB")),
    ZNspace(StringEqCI!("$ZNSPACE")),
    ZPi(StringEqCI!("$ZPI")),
    ZTimestamp(StringEqCI!("$ZTIMESTAMP")),
    ZTimezone(StringEqCI!("$ZTIMEZONE")),
    ZTS(StringEqCI!("$ZTS")),
    ZTZ(StringEqCI!("$ZTZ")),
    ZV(StringEqCI!("$ZV")),
    ZVersion(StringEqCI!("$ZVERSION")),
}

#[derive(Grammar)]
#[grammar(name = "user defined function")]
pub struct UserDefinedFunction {
    pub name: SQLFunctionName,
    pub open: (Space, StringEq!("("), Space),
    pub args: Option<VecSep<Box<Scalx>, (Space, StringEq!(","), Space)>>,
    pub close: (Space, StringEq!(")")),
}

#[derive(Grammar)]
pub enum Scalop {
    Concat(StringEq!("||")),
    Mod(StringEq!("#")),
    Mul(StringEq!("*")),
    Add(StringEq!("+")),
    Sub(StringEq!("-")),
    Div(StringEq!("/")),
    IntDiv(StringEq!("\\")),
    Underscore(StringEq!("_")),
}

#[derive(Grammar)]
pub enum Scalx {
    Legacy(VecSep<Term, Wrap<Space, Scalop, Space>>),
}
#[derive(Grammar)]
#[grammar(name = "case")]
pub enum Case {
    Searched(SearchedCase),
    Simple(SimpleCase),
}

#[derive(Grammar)]
#[grammar(name = "searched case")]
pub struct SearchedCase {
    case_kw: (StringEqCI!("CASE"), Space),
    when_clauses: Vec<SQLWhenClause>,
    else_clause: Option<SQLElseClause>,
    end_kw: (Space, StringEqCI!("END")),
}

#[derive(Grammar)]
#[grammar(name = "simple case")]
pub struct SimpleCase {
    case_kw: (StringEqCI!("CASE"), Space),
    expr: Scalx,
    when_clauses: Vec<SQLSimpleCaseWhen>,
    else_clause: Option<SQLElseClause>,
    end_kw: (Space, StringEqCI!("END")),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct SQLWhenClause {
    when_kw: (Space, StringEqCI!("WHEN"), Space),
    condition: Prdct,
    then_kw: (Space, StringEqCI!("THEN"), Space),
    result: Scalx,
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct SQLSimpleCaseWhen {
    when_kw: (Space, StringEqCI!("WHEN"), Space),
    value: Scalx,
    then_kw: (Space, StringEqCI!("THEN"), Space),
    result: Scalx,
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct SQLElseClause {
    else_kw: (Space, StringEqCI!("ELSE"), Space),
    result: Scalx,
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct SQLCastExpression {
    open: (Space, StringEq!("("), Space),
    expr: Scalx,
    as_kw: (Space, StringEqCI!("AS"), Space),
    cast_type: CastType,
    close: (Space, StringEq!(")")),
}

type ArgSep = (Space, StringEq!(","), Space);

#[derive(Grammar)]
#[grammar(inline)]
pub struct Args0 {
    open: (Space, StringEq!("("), Space),
    close: (Space, StringEq!(")")),
}

pub type ArgsR<T = Scalx> = Wrap<(Space, StringEq!("("), Space), T, (Space, StringEq!(")"))>;

#[derive(Grammar)]
#[grammar(inline)]
pub struct ArgsRR<T1: Grammar = Scalx, T2: Grammar = Scalx> {
    open: (Space, StringEq!("("), Space),
    arg1: T1,
    sep1: ArgSep,
    arg2: T2,
    close: (Space, StringEq!(")")),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct ArgsRRR<T1: Grammar = Scalx, T2: Grammar = Scalx, T3: Grammar = Scalx> {
    open: (Space, StringEq!("("), Space),
    arg1: T1,
    sep1: ArgSep,
    arg2: T2,
    sep2: ArgSep,
    arg3: T3,
    close: (Space, StringEq!(")")),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct ArgsRRRR<
    T1: Grammar = Scalx,
    T2: Grammar = Scalx,
    T3: Grammar = Scalx,
    T4: Grammar = Scalx,
> {
    open: (Space, StringEq!("("), Space),
    arg1: T1,
    sep1: ArgSep,
    arg2: T2,
    sep2: ArgSep,
    arg3: T3,
    sep3: ArgSep,
    arg4: T4,
    close: (Space, StringEq!(")")),
}

pub type ArgsO<T = Scalx> =
    Wrap<(Space, StringEq!("("), Space), Option<T>, (Space, StringEq!(")"))>;

#[derive(Grammar)]
#[grammar(inline)]
pub struct ArgsRO<T1: Grammar = Scalx, T2: Grammar = Scalx> {
    open: (Space, StringEq!("("), Space),
    arg1: T1,
    arg2: Option<(ArgSep, T2)>,
    close: (Space, StringEq!(")")),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct ArgsROO<T1: Grammar = Scalx, T2: Grammar = Scalx, T3: Grammar = Scalx> {
    open: (Space, StringEq!("("), Space),
    arg1: T1,
    rest: Option<(ArgSep, T2, Option<(ArgSep, T3)>)>,
    close: (Space, StringEq!(")")),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct ArgsRRO<T1: Grammar = Scalx, T2: Grammar = Scalx, T3: Grammar = Scalx> {
    open: (Space, StringEq!("("), Space),
    arg1: T1,
    sep1: ArgSep,
    arg2: T2,
    arg3: Option<(ArgSep, T3)>,
    close: (Space, StringEq!(")")),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct ArgsRROO<
    T1: Grammar = Scalx,
    T2: Grammar = Scalx,
    T3: Grammar = Scalx,
    T4: Grammar = Scalx,
> {
    open: (Space, StringEq!("("), Space),
    arg1: T1,
    sep1: ArgSep,
    arg2: T2,
    rest: Option<(ArgSep, T3, Option<(ArgSep, T4)>)>,
    close: (Space, StringEq!(")")),
}

pub type ArgsN =
    Wrap<(Space, StringEq!("("), Space), Option<VecSep<Scalx, ArgSep>>, (Space, StringEq!(")"))>;

pub type Args1Plus =
    Wrap<(Space, StringEq!("("), Space), VecSep<Scalx, ArgSep>, (Space, StringEq!(")"))>;

#[derive(Grammar)]
#[grammar(name = "scalar function")]
pub enum ScalarFunction {
    DollarExtract(StringEqCI!("$EXTRACT"), ArgsROO),
    DollarFind(StringEqCI!("$FIND"), ArgsROO),
    DollarFNumber(StringEqCI!("$FNUMBER"), ArgsROO),
    DollarJustify(StringEqCI!("$JUSTIFY"), ArgsRRO),
    DollarLength(StringEqCI!("$LENGTH"), ArgsRO),
    DollarList(StringEqCI!("$LIST"), ArgsROO),
    DollarListBuild(StringEqCI!("$LISTBUILD"), ArgsN),
    DollarListData(StringEqCI!("$LISTDATA"), ArgsRO),
    DollarListFind(StringEqCI!("$LISTFIND"), ArgsRRO),
    DollarListFromString(StringEqCI!("$LISTFROMSTRING"), ArgsRO),
    DollarListGet(StringEqCI!("$LISTGET"), ArgsROO),
    DollarListLength(StringEqCI!("$LISTLENGTH"), ArgsR),
    DollarListSame(StringEqCI!("$LISTSAME"), ArgsRR),
    DollarListToString(StringEqCI!("$LISTTOSTRING"), ArgsRO),
    DollarMvFmt(StringEqCI!("$MVFMT"), ArgsRR),
    DollarMvFmts(StringEqCI!("$MVFMTS"), ArgsRR),
    DollarMvIconv(StringEqCI!("$MVICONV"), ArgsRR),
    DollarMvIconvs(StringEqCI!("$MVICONVS"), ArgsRR),
    DollarMvOconv(StringEqCI!("$MVOCONV"), ArgsRR),
    DollarMvOconvs(StringEqCI!("$MVOCONVS"), ArgsRR),
    DollarPiece(StringEqCI!("$PIECE"), ArgsRROO),
    DollarTranslate(StringEqCI!("$TRANSLATE"), ArgsRRO),
    TSqlDbName(StringEqCI!("$TSQL_DB_NAME"), Args0),
    TSqlError(StringEqCI!("$TSQL_ERROR"), Args0),
    TSqlFetchStatus(StringEqCI!("$TSQL_FETCH_STATUS"), Args0),
    TSqlGetUtcDate(StringEqCI!("$TSQL_GETUTCDATE"), Args0),
    TSqlHostName(StringEqCI!("$TSQL_HOST_NAME"), Args0),
    TSqlLockTimeout(StringEqCI!("$TSQL_LOCK_TIMEOUT"), Args0),
    TSqlNestLevel(StringEqCI!("$TSQL_NESTLEVEL"), Args0),
    TSqlServerName(StringEqCI!("$TSQL_SERVERNAME"), Args0),
    TSqlSpid(StringEqCI!("$TSQL_SPID"), Args0),
    TSqlSqlStatus(StringEqCI!("$TSQL_SQLSTATUS"), Args0),
    TSqlTranCount(StringEqCI!("$TSQL_TRANCOUNT"), Args0),
    TSqlVersion(StringEqCI!("$TSQL_VERSION"), Args0),
    PercentObject(StringEqCI!("%OBJECT"), ArgsR),
    PercentOid(StringEqCI!("%OID"), ArgsR),
    Similarity(StringEqCI!("%SIMILARITY"), ArgsRR),
    XmlConcat(XMLConcat),
    XmlElement(XMLElement),
    XmlForest(XMLForest),
    Abs(StringEqCI!("ABS"), ArgsR),
    Ascii(StringEqCI!("ASCII"), ArgsR),
    Cast(StringEqCI!("CAST"), Box<SQLCastExpression>),
    Ceiling(StringEqCI!("CEILING"), ArgsR),
    CharacterLength(StringEqCI!("CHARACTER_LENGTH"), ArgsR),
    CharIndex(StringEqCI!("CHARINDEX"), ArgsRRO),
    CharLength(StringEqCI!("CHAR_LENGTH"), ArgsR),
    Coalesce(StringEqCI!("COALESCE"), Args1Plus),
    Convert(StringEqCI!("CONVERT"), ArgsRRO),
    CurrentTimestamp(StringEqCI!("CURRENT_TIMESTAMP"), Option<ArgsR>),
    CurrentTime(StringEqCI!("CURRENT_TIME"), Option<ArgsR>),
    DataLength(StringEqCI!("DATALENGTH"), ArgsR),
    Date(StringEqCI!("DATE"), ArgsR),
    DateAdd(StringEqCI!("DATEADD"), ArgsRRR<DatePartAdd>),
    DateDiff(StringEqCI!("DATEDIFF"), ArgsRRR<DatePartDiff>),
    DateName(StringEqCI!("DATENAME"), ArgsRR<DatePartName>),
    DatePart(StringEqCI!("DATEPART"), ArgsRR<DatePartPart>),
    DateTrunc(StringEqCI!("DATE_TRUNC"), ArgsRR<DatePartTrunc>),
    Day(StringEqCI!("DAY"), ArgsR),
    Decode(StringEqCI!("DECODE"), Args1Plus),
    Embedding(StringEqCI!("EMBEDDING"), ArgsRO),
    Floor(StringEqCI!("FLOOR"), ArgsR),
    GetDate(StringEqCI!("GETDATE"), ArgsO),
    GetUtcDate(StringEqCI!("GETUTCDATE"), ArgsO),
    Greatest(StringEqCI!("GREATEST"), Args1Plus),
    HashBytes(StringEqCI!("HASHBYTES"), ArgsRR),
    IfNull(StringEqCI!("IFNULL"), ArgsRRO),
    Instr(StringEqCI!("INSTR"), ArgsRROO),
    IsNull(StringEqCI!("ISNULL"), ArgsRR),
    IsNumeric(StringEqCI!("ISNUMERIC"), ArgsR),
    JSONArray(StringEqCI!("JSON_ARRAY"), SQLJSONArrayArgs),
    JSONObject(StringEqCI!("JSON_OBJECT"), SQLJSONObjectArgs),
    LastDay(StringEqCI!("LAST_DAY"), ArgsR),
    LastIdentity(StringEqCI!("LAST_IDENTITY"), Args0),
    LCase(StringEqCI!("LCASE"), ArgsR),
    Least(StringEqCI!("LEAST"), Args1Plus),
    Left(StringEqCI!("LEFT"), ArgsRR),
    Len(StringEqCI!("LEN"), ArgsR),
    Length(StringEqCI!("LENGTH"), ArgsR),
    Lower(StringEqCI!("LOWER"), ArgsR),
    LPad(StringEqCI!("LPAD"), ArgsRRO),
    LTrim(StringEqCI!("LTRIM"), ArgsR),
    Month(StringEqCI!("MONTH"), ArgsR),
    Now(StringEqCI!("NOW"), Args0),
    NullIf(StringEqCI!("NULLIF"), ArgsRR),
    Nvl(StringEqCI!("NVL"), ArgsRR),
    PatIndex(StringEqCI!("PATINDEX"), ArgsRR),
    Position(StringEqCI!("POSITION"), SQLPositionArgs),
    Power(StringEqCI!("POWER"), ArgsRR),
    Predict(StringEqCI!("PREDICT"), SQLPredictArgs),
    Probability(StringEqCI!("PROBABILITY"), SQLProbabilityArgs),
    Repeat(StringEqCI!("REPEAT"), ArgsRR),
    Replace(StringEqCI!("REPLACE"), ArgsRRR),
    Replicate(StringEqCI!("REPLICATE"), ArgsRR),
    Reverse(StringEqCI!("REVERSE"), ArgsR),
    Right(StringEqCI!("RIGHT"), ArgsRR),
    Round(StringEqCI!("ROUND"), ArgsRR),
    RPad(StringEqCI!("RPAD"), ArgsRRO),
    RTrim(StringEqCI!("RTRIM"), ArgsR),
    SearchIndex(StringEqCI!("SEARCH_INDEX"), SQLSearchIndexArgs),
    Sign(StringEqCI!("SIGN"), ArgsR),
    Space(StringEqCI!("SPACE"), ArgsR),
    Str(StringEqCI!("STR"), ArgsROO),
    String(StringEqCI!("STRING"), Args1Plus),
    Stuff(StringEqCI!("STUFF"), ArgsRRRR),
    Substr(StringEqCI!("SUBSTR"), ArgsRRO),
    Substring(StringEqCI!("SUBSTRING"), SubstringArgs),
    SysDate(StringEqCI!("SYSDATE")),
    ToChar(StringEqCI!("TOCHAR"), ArgsRO),
    ToDate(StringEqCI!("TODATE"), ArgsRO),
    ToUSChar(StringEqCI!("TO_CHAR"), ArgsRO),
    ToUSDate(StringEqCI!("TO_DATE"), ArgsRO),
    ToNumber(StringEqCI!("TO_NUMBER"), ArgsR<Term>),
    ToTimestamp(StringEqCI!("TO_TIMESTAMP"), ArgsRO),
    ToVector(
        StringEqCI!("TO_VECTOR"),
        ArgsROO<Scalx, VectorDatyp, Integer>,
    ),
    Trim(StringEqCI!("TRIM"), Space, Parenthesized<TrimArgs>),
    UCase(StringEqCI!("UCASE"), ArgsR),
    UnixTimestamp(StringEqCI!("UNIX_TIMESTAMP"), ArgsO),
    VectorCosine(StringEqCI!("VECTOR_COSINE"), ArgsRR),
    VectorDotProduct(StringEqCI!("VECTOR_DOT_PRODUCT"), ArgsRR),
    Year(StringEqCI!("YEAR"), ArgsR),
}

#[derive(Grammar)]
#[grammar(name = "datepart_add")]
pub struct DatePartAdd(OptionallyWrapped<DatePartAddRaw>);

#[derive(Grammar)]
#[grammar(name = "datepart_add_raw")]
pub struct DatePartAddRaw(DatePartCommon);

#[derive(Grammar)]
#[grammar(name = "datepart_diff")]
pub struct DatePartDiff(OptionallyWrapped<DatePartDiffRaw>);

#[derive(Grammar)]
#[grammar(name = "datepart_diff_raw")]
pub struct DatePartDiffRaw(DatePartCommon);

#[derive(Grammar)]
#[grammar(name = "datepart_name")]
pub struct DatePartName(OptionallyWrapped<DatePartNameRaw>);

#[derive(Grammar)]
#[grammar(name = "datepart_name_raw")]
pub struct DatePartNameRaw(DatePartCommon);

#[derive(Grammar)]
#[grammar(name = "datepart_part")]
pub struct DatePartPart(OptionallyWrapped<DatePartPartRaw>);

#[derive(Grammar)]
#[grammar(name = "datepart_trunc")]
pub struct DatePartTrunc(OptionallyWrapped<DatePartTruncRaw>);

#[derive(Grammar)]
#[grammar(name = "datepart_trunc_raw")]
pub struct DatePartTruncRaw(DatePartCommon);

#[derive(Grammar)]
#[grammar(inline)]
pub enum OptionallyWrapped<T: Grammar> {
    Wrapped(Parenthesized<T>),
    Scalar(T),
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum DatePartCommon {
    Year(Either<StringEqCI!("YEAR"), Either<StringEqCI!("YYYY"), StringEqCI!("YY")>>),
    Quarter(Either<StringEqCI!("QUARTER"), Either<StringEqCI!("QQ"), StringEqCI!("Q")>>),
    Month(Either<StringEqCI!("MONTH"), Either<StringEqCI!("MM"), StringEqCI!("M")>>),
    Week(Either<StringEqCI!("WEEK"), Either<StringEqCI!("WK"), StringEqCI!("WW")>>),
    Weekday(Either<StringEqCI!("WEEKDAY"), Either<StringEqCI!("DW"), StringEqCI!("W")>>),
    DayOfYear(Either<StringEqCI!("DAYOFYEAR"), Either<StringEqCI!("DY"), StringEqCI!("Y")>>),
    Day(Either<StringEqCI!("DAY"), Either<StringEqCI!("DD"), StringEqCI!("D")>>),
    Hour(Either<StringEqCI!("HOUR"), Either<StringEqCI!("HH"), StringEqCI!("H")>>),
    Minute(Either<StringEqCI!("MINUTE"), Either<StringEqCI!("MI"), StringEqCI!("N")>>),
    Second(Either<StringEqCI!("SECOND"), Either<StringEqCI!("SS"), StringEqCI!("S")>>),
    Millisecond(Either<StringEqCI!("MILLISECOND"), StringEqCI!("MS")>),
    Microsecond(Either<StringEqCI!("MICROSECOND"), StringEqCI!("MCS")>),
    Nanosecond(Either<StringEqCI!("NANOSECOND"), StringEqCI!("NS")>),
    Literal(Literal),
    HostVar(HostVar),
}

#[derive(Grammar)]
#[grammar(name = "datepart_part_raw")]
pub enum DatePartPartRaw {
    SqlTimestamp(Either<StringEqCI!("SQLTIMESTAMP"), StringEqCI!("STS")>),
    Common(DatePartCommon),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct SQLPositionArgs {
    pub open: (Space, StringEq!("("), Space),
    pub needle: Scalx,
    pub in_kw: (Space, StringEqCI!("IN"), Space),
    pub haystack: Scalx,
    pub close: (Space, StringEq!(")")),
}

pub type SubstringArgs =
    Wrap<(Space, StringEq!("("), Space), SubstringArgsInternal, (Space, StringEq!(")"))>;

#[derive(Grammar)]
#[grammar(name = "substring args")]
pub enum SubstringArgsInternal {
    Keyword {
        expr: Scalx,
        from_kw: (Space, StringEqCI!("FROM"), Space),
        start: Scalx,
        for_clause: Option<((Space, StringEqCI!("FOR"), Space), Scalx)>,
    },
    Position {
        expr: Scalx,
        comma: (Space, StringEq!(","), Space),
        start: Scalx,
        length: Option<((Space, StringEq!(","), Space), Scalx)>,
    },
}

#[derive(Grammar)]
pub enum TrimArgs {
    SpecScalx(TrimSpec, Space, StringEqCI!("FROM"), Space, Scalx),
    SpecScalxScalx(
        TrimSpec,
        Space,
        Scalx,
        Option<(Space, StringEqCI!("FROM"), Space, Scalx)>,
    ),
    ScalxScalx(Option<(Scalx, Space, StringEqCI!("FROM"), Space)>, Scalx),
}

#[derive(Grammar)]
pub enum TrimSpec {
    Leading(StringEqCI!("LEADING")),
    Trailing(StringEqCI!("TRAILING")),
    Both(StringEqCI!("BOTH")),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct SQLSearchIndexArgs {
    pub open: (Space, StringEq!("("), Space),
    pub table: Option<(Table, StringEq!("."))>,
    pub index_name: IndexName,
    pub scalx_list: Vec<(ArgSep, Scalx)>,
    pub close: (Space, StringEq!(")")),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct SQLPredictArgs {
    pub open: (Space, StringEq!("("), Space),
    pub model: ModelName,
    pub use_clause: Option<PredictProbabilityUse>,
    pub with_clause: Option<PredictWith>,
    pub close: (Space, StringEq!(")")),
}

pub type PredictProbabilityUse = Prefix<(Space, StringEqCI!("USE"), Space), TrainedModelName>;

#[derive(Grammar)]
#[grammar(name = "trained-model-name")]
pub struct TrainedModelName(Ident);

#[derive(Grammar)]
#[grammar(name = "predict-with")]
pub enum PredictWith {
    Unnamed {
        with_kw: (Space, StringEqCI!("WITH"), Space),
        scalx_list: Parenthesized<VecSep<Scalx, ArgSep>>,
    },
    Named {
        with_kw: (Space, StringEqCI!("WITH"), Space),
        assignments: VecSep<PredictAssign, ArgSep>,
    },
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct PredictAssign {
    ident: Ident,
    eq: (Space, StringEq!("="), Space),
    scalx: Scalx,
}

pub type ProbabilityFor = Prefix<(Space, StringEqCI!("FOR"), Space), Scalx>;

#[derive(Grammar)]
#[grammar(inline)]
pub struct SQLProbabilityArgs {
    pub open: (Space, StringEq!("("), Space),
    pub model: ModelName,
    pub use_clause: Option<PredictProbabilityUse>,
    pub for_clause: Option<ProbabilityFor>,
    pub with_clause: Option<PredictWith>,
    pub close: (Space, StringEq!(")")),
}

pub type SQLJSONArrayArgs = Wrap<
    (Space, StringEq!("("), Space),
    Option<(
        VecSep<Scalx, (Space, StringEq!(","), Space)>,
        Option<JSONArrayNullClause>,
    )>,
    (Space, StringEq!(")")),
>;

pub type SQLJSONObjectArgs = Wrap<
    (Space, StringEq!("("), Space),
    Option<(SQLJSONKeyValuePairs, Option<JSONObjectNullClause>)>,
    (Space, StringEq!(")")),
>;

#[derive(Grammar)]
#[grammar(inline)]
pub struct SQLJSONKeyValuePairs {
    pub first: (Scalx, (Space, StringEq!(":"), Space), Scalx),
    pub rest: Vec<(
        (Space, StringEq!(","), Space),
        Scalx,
        (Space, StringEq!(":"), Space),
        Scalx,
    )>,
}

#[derive(Grammar)]
#[grammar(name = "JSON array null clause")]
pub enum JSONArrayNullClause {
    NullOnNull(
        (
            Space,
            StringEqCI!("NULL"),
            Space,
            StringEqCI!("ON"),
            Space,
            StringEqCI!("NULL"),
        ),
    ),
    AbsentOnNull(
        (
            Space,
            StringEqCI!("ABSENT"),
            Space,
            StringEqCI!("ON"),
            Space,
            StringEqCI!("NULL"),
        ),
    ),
}

#[derive(Grammar)]
#[grammar(name = "JSON object null clause")]
pub enum JSONObjectNullClause {
    NullOnNull(
        (
            Space,
            StringEqCI!("NULL"),
            Space,
            StringEqCI!("ON"),
            Space,
            StringEqCI!("NULL"),
        ),
    ),
    AbsentOnNull(
        (
            Space,
            StringEqCI!("ABSENT"),
            Space,
            StringEqCI!("ON"),
            Space,
            StringEqCI!("NULL"),
        ),
    ),
}

#[derive(Grammar)]
#[grammar(name = "xmlElement")]
pub struct XMLElement {
    pub keyword: StringEqCI!("XMLELEMENT"),
    pub open: (Space, StringEq!("("), Space),
    pub name: Option<SQLXmlElementName>,
    pub space: Space,
    pub values: VecSep<XMLValue, (Space, StringEq!(","), Space)>,
    pub close: (Space, StringEq!(")")),
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum SQLXmlElementName {
    NameIdent(StringEqCI!("NAME"), Space, Ident),
    NameOnly(StringEqCI!("NAME")),
    Ident(Ident),
}

#[derive(Grammar)]
#[grammar(name = "xmlValue")]
pub enum XMLValue {
    Concat(Box<XMLConcat>),
    Attributes(XMLAttributes),
    Expr(Scalx),
    Value2(XMLValue2),
}

#[derive(Grammar)]
#[grammar(name = "xmlValue2")]
pub enum XMLValue2 {
    Element(Box<XMLElement>),
    Forest(Box<XMLForest>),
    Comment(XMLComment),
}

#[derive(Grammar)]
#[grammar(name = "xmlComment")]
pub struct XMLComment((StringEq!("<--"), StringEndsWith<StringEq!("-->")>));

#[derive(Grammar)]
#[grammar(name = "xmlAttributes")]
pub struct XMLAttributes {
    pub keyword: StringEqCI!("XMLATTRIBUTES"),
    pub open: (Space, StringEq!("("), Space),
    pub attrs: VecSep<SQLXmlAttribute, (Space, StringEq!(","), Space)>,
    pub close: (Space, StringEq!(")")),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct SQLXmlAttribute {
    pub expr: Scalx,
    pub alias: Option<((Space, StringEqCI!("AS"), Space), Ident)>,
}

#[derive(Grammar)]
#[grammar(name = "xmlForest")]
pub struct XMLForest {
    pub keyword: StringEqCI!("XMLFOREST"),
    pub open: (Space, StringEq!("("), Space),
    pub items: VecSep<SQLXmlAttribute, (Space, StringEq!(","), Space)>,
    pub close: (Space, StringEq!(")")),
}

#[derive(Grammar)]
#[grammar(name = "xmlConcat")]
pub struct XMLConcat {
    pub keyword: StringEqCI!("XMLCONCAT"),
    pub open: (Space, StringEq!("("), Space),
    pub values: VecSep<XMLValue2, (Space, StringEq!(","), Space)>,
    pub close: (Space, StringEq!(")")),
}

#[derive(Grammar)]
pub struct XTFun {
    pub fun: XTFuncRef,
    pub args: XTFunArgs,
}

pub type XTFuncRef = Prefix<StringEqCI!("$$"), StringOf1<IsExtFuncChar>>;

pub type XTFunArgs = Wrap<
    (StringEq!("("), Space),
    Option<VecSep<Scalx, (Space, StringEq!(","), Space)>>,
    (Space, StringEq!(")")),
>;

char_class!(pub IsExtFuncChar, "non left paren character", |ch| {
    !matches!(ch, '(' | '$')
});

pub type ODBCEscape = Wrap<(StringEq!("{"), Space), DTS, (StringEq!("}"), Space)>;

#[derive(Grammar)]
pub enum DTS {
    Function(FunExt),
    DateTime(DTSVal),
}

pub type FunExt = Prefix<(StringEqCI!("FN"), Space), ODBCFunc>;

#[derive(Grammar)]
#[grammar(name = "odbc func")]
pub enum ODBCFunc {
    // Similar to SQLScalarFunction but in ODBC escape syntax
    Abs(StringEqCI!("ABS"), ArgsR),
    Acos(StringEqCI!("ACOS"), ArgsR),
    Asin(StringEqCI!("ASIN"), ArgsR),
    Atan(StringEqCI!("ATAN"), ArgsR),
    Ceiling(StringEqCI!("CEILING"), ArgsR),
    Cos(StringEqCI!("COS"), ArgsR),
    Cot(StringEqCI!("COT"), ArgsR),
    Exp(StringEqCI!("EXP"), ArgsR),
    Floor(StringEqCI!("FLOOR"), ArgsR),
    Log(StringEqCI!("LOG"), ArgsR),
    Log10(StringEqCI!("LOG10"), ArgsR),
    Mod(StringEqCI!("MOD"), ArgsRR),
    Pi(StringEqCI!("PI"), Option<StringEq!("()")>),
    Power(StringEqCI!("POWER"), ArgsRR),
    Sin(StringEqCI!("SIN"), ArgsR),
    Sqrt(StringEqCI!("SQRT"), ArgsR),
    Tan(StringEqCI!("TAN"), ArgsR),
    Ascii(StringEqCI!("ASCII"), ArgsR),
    Char(StringEqCI!("CHAR"), ArgsR),
    Concat(StringEqCI!("CONCAT"), ArgsRR),
    CurDate(
        StringEqCI!("CURDATE"),
        Option<(StringEq!("("), StringEq!(")"))>,
    ),
    CurTime(
        StringEqCI!("CURTIME"),
        Option<(StringEq!("("), StringEq!(")"))>,
    ),
    Database(
        StringEqCI!("DATABASE"),
        Option<(StringEq!("("), StringEq!(")"))>,
    ),
    DayName(StringEqCI!("DAYNAME"), ArgsR),
    DayOfMonth(StringEqCI!("DAYOFMONTH"), ArgsR),
    DayOfWeek(StringEqCI!("DAYOFWEEK"), ArgsR),
    DayOfYear(StringEqCI!("DAYOFYEAR"), ArgsR),
    Hour(StringEqCI!("HOUR"), ArgsR),
    IfNull(StringEqCI!("IFNULL"), ArgsRR),
    LCase(StringEqCI!("LCASE"), ArgsR),
    Left(StringEqCI!("LEFT"), ArgsRR),
    Length(StringEqCI!("LENGTH"), ArgsR),
    LTrim(StringEqCI!("LTRIM"), ArgsR),
    Minute(StringEqCI!("MINUTE"), ArgsR),
    Month(StringEqCI!("MONTH"), ArgsR),
    MonthName(StringEqCI!("MONTHNAME"), ArgsR),
    Now(StringEqCI!("NOW"), Option<(StringEq!("("), StringEq!(")"))>),
    Quarter(StringEqCI!("QUARTER"), ArgsR),
    Repeat(StringEqCI!("REPEAT"), ArgsRR),
    Right(StringEqCI!("RIGHT"), ArgsRR),
    Round(StringEqCI!("ROUND"), ArgsRR),
    RTrim(StringEqCI!("RTRIM"), ArgsR),
    Second(StringEqCI!("SECOND"), ArgsR),
    Sign(StringEqCI!("SIGN"), ArgsR),
    Space(StringEqCI!("SPACE"), ArgsR),
    Substring(StringEqCI!("SUBSTRING"), SubstringArgs),
    TimestampAdd(StringEqCI!("TIMESTAMPADD"), ArgsRRR<TSAddInterval>),
    TimestampDiff(StringEqCI!("TIMESTAMPDIFF"), ArgsRRR<TSDiffInterval>),
    Truncate(StringEqCI!("TRUNCATE"), ArgsRR),
    UCase(StringEqCI!("UCASE"), ArgsR),
    User(
        StringEqCI!("USER"),
        Option<(StringEq!("("), StringEq!(")"))>,
    ),
    Week(StringEqCI!("WEEK"), ArgsR),
    Year(StringEqCI!("YEAR"), ArgsR),
    Convert(StringEqCI!("CONVERT"), ArgsRR<Scalx, SQLConvertFunc>),
}

#[derive(Grammar)]
pub enum TSAddInterval {
    Unquoted(TSAddIntervalUnquoted),
    Quoted(TSAddIntervalQuoted),
}

#[derive(Grammar)]
#[grammar(name = "tsaddinterval_unquoted")]
pub enum TSAddIntervalUnquoted {
    Year(StringEqCI!("SQL_TSI_YEAR")),
    Quarter(StringEqCI!("SQL_TSI_QUARTER")),
    Month(StringEqCI!("SQL_TSI_MONTH")),
    Week(StringEqCI!("SQL_TSI_WEEK")),
    Day(StringEqCI!("SQL_TSI_DAY")),
    Hour(StringEqCI!("SQL_TSI_HOUR")),
    Minute(StringEqCI!("SQL_TSI_MINUTE")),
    Second(StringEqCI!("SQL_TSI_SECOND")),
    FracSecond(StringEqCI!("SQL_TSI_FRAC_SECOND")),
}

#[derive(Grammar)]
#[grammar(name = "tsaddinterval_quoted")]
pub enum TSAddIntervalQuoted {
    Year(StringEqCI!("\"SQL_TSI_YEAR\"")),
    Quarter(StringEqCI!("\"SQL_TSI_QUARTER\"")),
    Month(StringEqCI!("\"SQL_TSI_MONTH\"")),
    Week(StringEqCI!("\"SQL_TSI_WEEK\"")),
    Day(StringEqCI!("\"SQL_TSI_DAY\"")),
    Hour(StringEqCI!("\"SQL_TSI_HOUR\"")),
    Minute(StringEqCI!("\"SQL_TSI_MINUTE\"")),
    Second(StringEqCI!("\"SQL_TSI_SECOND\"")),
    FracSecond(StringEqCI!("\"SQL_TSI_FRAC_SECOND\"")),
}

#[derive(Grammar)]
pub enum TSDiffInterval {
    Unquoted(TSDiffIntervalUnquoted),
    Quoted(TSDiffIntervalQuoted),
}

#[derive(Grammar)]
#[grammar(name = "tsdiffinterval_unquoted")]
pub enum TSDiffIntervalUnquoted {
    Year(StringEqCI!("SQL_TSI_YEAR")),
    Month(StringEqCI!("SQL_TSI_MONTH")),
    Week(StringEqCI!("SQL_TSI_WEEK")),
    Day(StringEqCI!("SQL_TSI_DAY")),
    Hour(StringEqCI!("SQL_TSI_HOUR")),
    Minute(StringEqCI!("SQL_TSI_MINUTE")),
    Second(StringEqCI!("SQL_TSI_SECOND")),
    FracSecond(StringEqCI!("SQL_TSI_FRAC_SECOND")),
}

#[derive(Grammar)]
#[grammar(name = "tsdiffinterval_quoted")]
pub enum TSDiffIntervalQuoted {
    Year(StringEqCI!("\"SQL_TSI_YEAR\"")),
    Month(StringEqCI!("\"SQL_TSI_MONTH\"")),
    Week(StringEqCI!("\"SQL_TSI_WEEK\"")),
    Day(StringEqCI!("\"SQL_TSI_DAY\"")),
    Hour(StringEqCI!("\"SQL_TSI_HOUR\"")),
    Minute(StringEqCI!("\"SQL_TSI_MINUTE\"")),
    Second(StringEqCI!("\"SQL_TSI_SECOND\"")),
    FracSecond(StringEqCI!("\"SQL_TSI_FRAC_SECOND\"")),
}

#[derive(Grammar)]
pub enum SQLConvertFunc {
    SqlTimestamp(StringEqCI!("SQL_TIMESTAMP")),
    SqlInteger(StringEqCI!("SQL_INTEGER")),
    SqlBigInt(StringEqCI!("SQL_BIGINT")),
    SqlSmallInt(StringEqCI!("SQL_SMALLINT")),
    SqlTinyInt(StringEqCI!("SQL_TINYINT")),
    SqlDate(StringEqCI!("SQL_DATE")),
    SqlTime(StringEqCI!("SQL_TIME")),
    SqlVarChar(StringEqCI!("SQL_VARCHAR")),
    SqlDouble(StringEqCI!("SQL_DOUBLE")),
}

#[derive(Grammar)]
#[grammar(name = "dts val")]
pub struct DTSVal {
    pub dts_type: DTSType,
    pub sp: Space,
    pub val_spec: DTSValSpec,
}

#[derive(Grammar)]
#[grammar(name = "dts type")]
pub enum DTSType {
    Date(StringEqCI!("D")),
    Timestamp(StringEqCI!("TS")),
    Time(StringEqCI!("T")),
}

#[derive(Grammar)]
#[grammar(name = "dts val spec")]
pub enum DTSValSpec {
    Literal(Literal),
    HostVar(HostVar),
}

#[derive(Grammar)]
#[grammar(name = "agg")]
pub enum Agg {
    AggFuncArgs(AggFunc, Space, Parenthesized<AggArg>),
    CountStar(StringEqCI!("COUNT"), Space, Parenthesized<StringEq!("*")>),
    UserDefinedFunction(UserDefinedFunction),
}

#[derive(Grammar)]
#[grammar(name = "agg func")]
pub enum AggFunc {
    Avg(StringEqCI!("AVG")),
    Count(StringEqCI!("COUNT")),
    Sum(StringEqCI!("SUM")),
    Max(StringEqCI!("MAX")),
    Min(StringEqCI!("MIN")),
    List(StringEqCI!("LIST")),
    XmlAgg(StringEqCI!("XMLAGG")),
    DList(StringEqCI!("%DLIST")),
    StdDev(StringEqCI!("STDDEV")),
    StdDevPop(StringEqCI!("STDDEV_POP")),
    StdDevSamp(StringEqCI!("STDDEV_SAMP")),
    Variance(StringEqCI!("VARIANCE")),
    VarPop(StringEqCI!("VAR_POP")),
    VarSamp(StringEqCI!("VAR_SAMP")),
    JSONArrayAgg(StringEqCI!("JSON_ARRAYAGG")),
    ApproxCountDistinct(StringEqCI!("APPROX_COUNT_DISTINCT")),
}

#[derive(Grammar)]
#[grammar(name = "agg arg")]
pub struct AggArg {
    pub distinct: Option<Dist>,
    pub expr: Scalx,
    pub foreach: Option<SQLForEachClause>,
    pub afterhaving: Option<(Space, StringEqCI!("%AFTERHAVING"))>,
}

#[derive(Grammar)]
#[grammar(name = "dist")]
pub enum Dist {
    Distinct {
        keyword: (StringEqCI!("DISTINCT"), Space),
        by: Option<SQLDistinctBy>,
    },
    All(StringEqCI!("ALL")),
}

pub type SQLDistinctBy = Wrap<(StringEqCI!("BY"), Space), List, Space>;

pub type List = Wrap<
    (StringEq!("("), Space),
    VecSep<Scalx, (Space, StringEq!(","), Space)>,
    (Space, StringEq!(")")),
>;

pub type SQLForEachClause = Prefix<(Space, StringEqCI!("%FOREACH"), Space), List>;

#[derive(Grammar)]
#[grammar(name = "window")]
pub struct Window {
    pub fun: WindowFunc,
    pub arg: Parenthesized<Option<Either<StringEq!("*"), Scalx>>>,
    pub over_kw: (Space, StringEqCI!("OVER"), Space),
    pub open: (StringEq!("("), Space),
    pub partition: Option<WindowPartition>,
    pub order: Option<WindowOrder>,
    pub frame: Option<WindowRows>,
    pub close: (Space, StringEq!(")")),
}

#[derive(Grammar)]
#[grammar(name = "window func")]
pub enum WindowFunc {
    RowNumber(StringEqCI!("ROW_NUMBER")),
    Rank(StringEqCI!("RANK")),
    DenseRank(StringEqCI!("DENSE_RANK")),
    PercentRank(StringEqCI!("PERCENT_RANK")),
    CumeDist(StringEqCI!("CUME_DIST")),
    NTile(StringEqCI!("NTILE")),
    FirstValue(StringEqCI!("FIRST_VALUE")),
    LastValue(StringEqCI!("LAST_VALUE")),
    NthValue(StringEqCI!("NTH_VALUE")),
    Lead(StringEqCI!("LEAD")),
    Lag(StringEqCI!("LAG")),
    Sum(StringEqCI!("SUM")),
    Avg(StringEqCI!("AVG")),
    Count(StringEqCI!("COUNT")),
    Min(StringEqCI!("MIN")),
    Max(StringEqCI!("MAX")),
}

pub type WindowPartition = Prefix<
    (
        Space,
        StringEqCI!("PARTITION"),
        Space,
        StringEqCI!("BY"),
        Space,
    ),
    VecSep<Scalx, (Space, StringEq!(","), Space)>,
>;

pub type WindowOrder = Prefix<
    (Space, StringEqCI!("ORDER"), Space, StringEqCI!("BY"), Space),
    VecSep<Scalx, (Space, StringEq!(","), Space)>,
>;

#[derive(Grammar)]
#[grammar(inline)]
pub enum SQLOrderDirection {
    Asc(StringEqCI!("ASC")),
    Desc(StringEqCI!("DESC")),
}

#[derive(Grammar)]
#[grammar(name = "window rows")]
pub enum WindowRows {
    Single((Space, StringEqCI!("ROWS"), Space), WindowRowsBound),
    Between {
        rows_kw: (Space, StringEqCI!("ROWS"), Space),
        between_kw: StringEqCI!("BETWEEN"),
        start: (Space, WindowRowsBound),
        end: Spaced<AndClause<WindowRowsBound>>,
    },
}

#[derive(Grammar)]
#[grammar(name = "window rows bound")]
pub enum WindowRowsBound {
    UnboundedPreceding(StringEqCI!("UNBOUNDED"), Space, StringEqCI!("PRECEDING")),
    UnboundedFollowing(StringEqCI!("UNBOUNDED"), Space, StringEqCI!("FOLLOWING")),
    Preceding(Number, Space, StringEqCI!("PRECEDING")),
    Following(Number, Space, StringEqCI!("FOLLOWING")),
    CurrentRow(StringEqCI!("CURRENT"), Space, StringEqCI!("ROW")),
}
#[derive(Grammar)]
#[grammar(name = "prdct")]
pub struct Prdct {
    pub first: And,
    pub rest: Vec<((Space, OrOperator, Space), And)>,
}

#[derive(Grammar)]
#[grammar(name = "or operator")]
pub enum OrOperator {
    Or(StringEqCI!("OR")),
    Exclamation(StringEq!("!")),
}

#[derive(Grammar)]
#[grammar(name = "and")]
pub struct And {
    pub first: Cond,
    pub rest: Vec<((Space, AndOperator, Space), Cond)>,
}

#[derive(Grammar)]
#[grammar(name = "and operator")]
pub enum AndOperator {
    And(StringEqCI!("AND")),
    Ampersand(StringEq!("&")),
}

#[derive(Grammar)]
#[grammar(name = "cond")]
pub enum Cond {
    Not {
        not_kw: (StringEqCI!("NOT"), Space),
        condition: Box<Cond>,
    },
    Paren {
        open: (StringEq!("("), Space),
        predicate: Box<Prdct>,
        close: (Space, StringEq!(")")),
    },
    Exists {
        noindex: Option<(StringEqCI!("%NOINDEX"), Space)>,
        exists_kw: (StringEqCI!("EXISTS"), Space),
        open: (StringEq!("("), Space),
        query: Box<Query>,
        close: (Space, StringEq!(")")),
    },
    ForSomeElement {
        noindex: Option<(StringEqCI!("%NOINDEX"), Space)>,
        for_kw: (StringEqCI!("FOR"), Space, StringEqCI!("SOME"), Space),
        percent_element: StringEqCI!("%ELEMENT"),
        open_paren1: (Space, StringEq!("("), Space),
        column: Colmn,
        close_paren1: (Space, StringEq!(")")),
        label: Option<(Space, Option<(StringEqCI!("AS"), Space)>, Ident)>,
        open_paren2: (Space, StringEq!("("), Space),
        predicate: Box<Prdct>,
        close_paren2: (Space, StringEq!(")")),
    },
    ForSome {
        noindex: Option<(StringEqCI!("%NOINDEX"), Space)>,
        for_kw: (StringEqCI!("FOR"), Space),
        some_kw: (StringEqCI!("SOME"), Space),
        open: (StringEq!("("), Space),
        tables: Tables,
        close: (Space, StringEq!(")")),
        condition: Option<Box<ForCnd>>,
    },
    Comparison {
        noindex: Option<(StringEqCI!("%NOINDEX"), Space)>,
        scalx: Scalx,
        space: Space,
        comparison: Comparison,
    },
}

#[derive(Grammar)]
pub enum ForCnd {
    Paren(StringEq!("("), Space, Box<Prdct>, Space, StringEq!(")")),
    GroupHaving {
        open: (StringEq!("("), Space),
        group: Option<Group>,
        having: Having,
        close: (Space, StringEq!(")")),
    },
    GroupOnly(StringEq!("("), Space, Group, Space, StringEq!(")")),
}

pub type Spaced<T> = Prefix<Space, T>;

pub type AndClause<T> = Prefix<(StringEqCI!("AND"), Space), T>;

pub type Parenthesized<T> = Wrap<(StringEq!("("), Space), T, (Space, StringEq!(")"))>;

#[derive(Grammar)]
#[grammar(name = "comparison")]
pub enum Comparison {
    StarEq {
        op: (StringEqCI!("*="), Space),
        scalx: Scalx,
    },
    EqStar {
        op: (StringEqCI!("=*"), Space),
        scalx: Scalx,
    },
    IsNull {
        is_kw: (StringEqCI!("IS"), Space),
        not: Option<(StringEqCI!("NOT"), Space)>,
        null_kw: StringEqCI!("NULL"),
    },
    Between {
        not: Option<(StringEqCI!("NOT"), Space)>,
        between_kw: (StringEqCI!("BETWEEN"), Space),
        lower: Scalx,
        upper: Spaced<AndClause<Scalx>>,
    },
    Like {
        not: Option<(StringEqCI!("NOT"), Space)>,
        like_kw: (StringEqCI!("LIKE"), Space),
        pattern: Scalx,
        escape: Option<EscapeScalx>,
    },
    Matches {
        not: Option<(StringEqCI!("NOT"), Space)>,
        matches_kw: (StringEqCI!("%MATCHES"), Space),
        pattern: Scalx,
        escape: Option<EscapeScalx>,
    },
    In {
        not: Option<(StringEqCI!("NOT"), Space)>,
        in_kw: (StringEqCI!("IN"), Space),
        spec: Parenthesized<InSpec>,
    },
    InList {
        not: Option<(StringEqCI!("NOT"), Space)>,
        inlist_kw: (StringEqCI!("%INLIST"), Space),
        list_expr: Scalx,
        size_hint: Option<SizeHint>,
    },
    Find {
        find_kw: (StringEqCI!("%FIND"), Space),
        expr: Scalx,
        size_hint: Option<SizeHint>,
    },
    Pattern {
        pattern_kw: (StringEqCI!("%PATTERN"), Space),
        pattern: Scalx,
    },
    IsJSON {
        format: Option<JSONInputClause>,
        is_kw: (StringEqCI!("IS"), Space),
        not: Option<(StringEqCI!("NOT"), Space)>,
        json_kw: StringEqCI!("JSON"),
        constraint: Option<JSONPredicateTypeConstraint>,
    },
    Contains {
        kw: Either<StringEqCI!("%CONTAINSTERM"), StringEqCI!("%CONTAINS")>,
        list: Args1Plus,
    },
    Query {
        op: CmpOp,
        sp1: Space,
        quantifier: Quantifier,
        sp2: Space,
        expr: SQLSubquery,
    },
    Scalx {
        op: CmpOp,
        space: Space,
        expr: Scalx,
    },
}

pub type EscapeScalx = Prefix<(Space, StringEqCI!("ESCAPE"), Space), Scalx>;

#[derive(Grammar)]
#[grammar(name = "in spec")]
pub enum InSpec {
    Query(Box<Query>),
    Scalxes(VecSep<Scalx, (Space, StringEq!(","), Space)>),
}

pub type SQLSubquery = Wrap<(StringEq!("("), Space), Box<Query>, (Space, StringEq!(")"))>;

#[derive(Grammar)]
#[grammar(name = "quantifier")]
pub enum Quantifier {
    All(StringEqCI!("ALL")),
    Any(StringEqCI!("ANY")),
    Some(StringEqCI!("SOME")),
}

pub type SizeHint = Prefix<(Space, StringEqCI!("SIZE"), Space), PConst>;

#[derive(Grammar)]
pub enum PConst {
    Paren(StringEq!("("), Box<PConst>, StringEq!(")")),
    Literal(Literal),
}

#[derive(Grammar)]
#[grammar(name = "JSON representation")]
pub struct JSONRepresentation(StringEqCI!("JSON"));

pub type JSONInputClause = Prefix<(StringEqCI!("FORMAT"), Space), (JSONRepresentation, Space)>;

#[derive(Grammar)]
#[grammar(name = "JSON predicate type constraint")]
pub enum JSONPredicateTypeConstraint {
    Value((Space, StringEqCI!("VALUE"))),
    Array((Space, StringEqCI!("ARRAY"))),
    Object((Space, StringEqCI!("OBJECT"))),
    Scalar((Space, StringEqCI!("SCALAR"))),
}

#[derive(Grammar)]
#[grammar(name = "with list")]
pub struct WithList(VecSep<WithListElement, (Space, StringEq!(","), Space)>);

pub type WithClause = Prefix<(StringEqCI!("WITH"), Space), WithList>;

#[derive(Grammar)]
#[grammar(name = "with list element")]
pub struct WithListElement {
    pub name: WithQueryName,
    pub columns: Option<(
        (Space, StringEq!("("), Space),
        WithColumnList,
        (Space, StringEq!(")")),
    )>,
    pub as_kw: (Space, StringEqCI!("AS"), Space),
    pub query: WithQuery,
}

#[derive(Grammar)]
#[grammar(name = "with query name")]
pub struct WithQueryName(Ident);

#[derive(Grammar)]
#[grammar(name = "column name")]
pub struct ColumnName(Ident);

#[derive(Grammar)]
#[grammar(name = "with column list")]
pub struct WithColumnList(VecSep<ColumnName, (Space, StringEq!(","), Space)>);

pub type WithQuery = Wrap<(StringEq!("("), Space), Box<Query>, (Space, StringEq!(")"))>;

#[derive(Grammar)]
pub struct Selex {
    pub kw: (StringEqCI!("SELECT"), Space),
    pub selst: Selst,
    pub from: Option<FromClause>,
    pub where_clause: Option<WhereClause>,
    pub group: Option<Group>,
    pub having: Option<Having>,
    pub order: Option<Order>,
}

#[derive(Grammar)]
#[grammar(name = "select option")]
pub enum SelectOption {
    NoFPlan((StringEqCI!("%NOFPLAN"), Space)),
    NoLock((StringEqCI!("%NOLOCK"), Space)),
    Profile((StringEqCI!("%PROFILE"), Space)),
    ProfileAll((StringEqCI!("%PROFILE_ALL"), Space)),
    NoRuntime((StringEqCI!("%NORUNTIME"), Space)),
    DoRuntime((StringEqCI!("%DORUNTIME"), Space)),
    RowLimit((StringEqCI!("%ROWLIMIT"), Space), ValSpec, Space),
    RowOffset((StringEqCI!("%ROWOFFSET"), Space), ValSpec, Space),
}

pub type SQLTopClause = Wrap<(StringEqCI!("TOP"), Space), TopSpec, Space>;

#[derive(Grammar)]
pub enum TopSpec {
    ValSpec(DTSValSpec),
    All(StringEqCI!("ALL")),
}

#[derive(Grammar)]
#[grammar(name = "selst")]
pub struct Selst {
    pub options: Vec<SelectOption>,
    pub distinct: Option<Dist>,
    pub top: Option<SQLTopClause>,
    pub select_list: SelectList,
    pub targt: Targt,
}

#[derive(Grammar)]
#[grammar(name = "select list")]
pub struct SelectList {
    pub first: SelectItem,
    pub rest: Vec<((Space, StringEq!(","), Space), SelectItem)>,
}

#[derive(Grammar)]
#[grammar(name = "select item")]
pub enum SelectItem {
    Star(StringEq!("*")),
    Expr {
        expr: Scalx,
        alias: Option<(
            Space,
            Option<(StringEqCI!("AS"), Space)>,
            SQLIdentNonKeyword,
        )>,
    },
}

// The Space must be inside the Option to prevent partial consumption

type SQLTableAlias = (
    Space,
    Option<(StringEqCI!("AS"), Space)>,
    SQLIdentNonKeyword,
);

pub type Into = Prefix<(StringEqCI!("INTO"), Space), HostList>;

#[derive(Grammar)]
#[grammar(name = "targt")]
pub struct Targt(Option<(Space, TargtEnum)>);

#[derive(Grammar)]
#[grammar(inline)]
pub enum TargtEnum {
    Into(Into),
    IntoList(IntoList),
}

#[derive(Grammar)]
#[grammar(name = "host list")]
pub struct HostList(VecSep<Param, (Space, StringEq!(","), Space)>);

pub type IntoList = Prefix<StringEqCI!("%INTOLIST"), (Space, Param)>;

#[derive(Grammar)]
#[grammar(name = "from")]
pub struct FromClause {
    pub keyword: (Space, StringEqCI!("FROM"), Space),
    pub options: Vec<FromOpt>,
    pub tables: Tables,
}

#[derive(Grammar)]
#[grammar(name = "from_opt")]
pub enum FromOpt {
    InOrder((StringEqCI!("%inorder"), Space)),
    Full((StringEqCI!("%full"), Space)),
    DbugFull((StringEqCI!("%dbugfull"), Space)),
    Flatten((StringEqCI!("%flatten"), Space)),
    NoBSL((StringEqCI!("%nobsl"), Space)),
    NoFixedStats((StringEqCI!("%nofixedstats"), Space), TableRefs, Space),
    NoFlatten((StringEqCI!("%noflatten"), Space)),
    NoMerge((StringEqCI!("%nomerge"), Space)),
    NoReduce((StringEqCI!("%noreduce"), Space)),
    NoSVSO((StringEqCI!("%nosvso"), Space)),
    NoTopOpt((StringEqCI!("%notopopt"), Space)),
    NoFactorOrderByOr((StringEqCI!("%nofactororderbyor"), Space)),
    NoFactorAndFromOr((StringEqCI!("%nofactorandfromor"), Space)),
    NoMergeEquiAgg((StringEqCI!("%nomergeequivagg"), Space)),
    IgnoreIndices((StringEqCI!("%ignoreindices"), Space), IndexRefs, Space),
    IgnoreIndex((StringEqCI!("%ignoreindex"), Space), IndexRefs, Space),
    StartTable((StringEqCI!("%starttable"), Space)),
    AllIndex((StringEqCI!("%allindex"), Space)),
    NoUnionOrOpt((StringEqCI!("%nounionoropt"), Space)),
    FirstTable((StringEqCI!("%firsttable"), Space), Ident, Space),
    Parallel((StringEqCI!("%parallel"), Space)),
    NoParallel((StringEqCI!("%noparallel"), Space)),
}

#[derive(Grammar)]
pub struct TableRefs {
    pub first: TableRef,
    pub rest: Vec<((Space, StringEq!(","), Space), TableRef)>,
}

#[derive(Grammar)]
pub enum TableRef {
    All(StringEq!("*")),
    SchemaAll(Schema, Space, StringEq!(".*")),
    SchemaTable {
        schema: Option<(Schema, Space, StringEq!("."), Space)>,
        table: Table,
    },
}

#[derive(Grammar)]
pub struct IndexRefs {
    pub first: IndexRef,
    pub rest: Vec<((Space, StringEq!(","), Space), IndexRef)>,
}

#[derive(Grammar)]
pub struct IndexRef {
    pub table: Option<(Table, Space, StringEq!("."), Space)>,
    pub index: IndexSpec,
}

#[derive(Grammar)]
pub enum IndexSpec {
    Name(IndexName),
    All(StringEq!("*")),
}

#[derive(Grammar)]
#[grammar(name = "tables")]
pub struct Tables {
    content: VecSep<Tref, (Space, StringEq!(","), Space)>,
}

impl std::ops::Deref for Tables {
    type Target = VecSep<Tref, (Space, StringEq!(","), Space)>;

    fn deref(&self) -> &Self::Target {
        &self.content
    }
}

#[derive(Grammar)]
#[grammar(name = "tref")]
pub enum Tref {
    Join(JTab),
    OJSyntax {
        open: StringEq!("{"),
        oj: Ident,
        table: Box<JTab>,
        close: StringEq!("}"),
    },
}

#[derive(Grammar)]
#[grammar(name = "jtab")]
pub struct JTab {
    pub formit: Frmit,
    pub joins: Vec<Join>,
}

#[derive(Grammar)]
pub enum Join {
    Cross {
        cross_kw: (
            Space,
            StringEqCI!("CROSS"),
            Space,
            StringEqCI!("JOIN"),
            Space,
        ),
        table: Box<JTab>,
    },
    Natural {
        natural_kw: (Space, StringEqCI!("NATURAL"), Space),
        join_type: JOp,
        space: Space,
        table: Box<JTab>,
    },
    Qualified {
        join_type: (Space, JOp),
        space: Space,
        table: Box<JTab>,
        spec: JSpec,
    },
}

pub type JOp = Suffix<Option<(JType, Space)>, StringEqCI!("JOIN")>;

#[derive(Grammar)]
pub enum JType {
    Inner(StringEqCI!("INNER")),
    Left(StringEqCI!("LEFT"), Option<(Space, StringEqCI!("OUTER"))>),
    Right(StringEqCI!("RIGHT"), Option<(Space, StringEqCI!("OUTER"))>),
    Full(StringEqCI!("FULL"), Option<(Space, StringEqCI!("OUTER"))>),
}

#[derive(Grammar)]
pub enum JSpec {
    On {
        on_kw: (Space, StringEqCI!("ON"), Space),
        condition: JCond,
    },
    Using {
        using_kw: (Space, StringEqCI!("USING"), Space),
        flist1: FList1,
    },
}

#[derive(Grammar)]
pub struct JCond(Prdct);

#[derive(Grammar)]
pub enum Frmit {
    JSONTable {
        json_table: JSONTable,
        alias: Option<SQLTableAlias>,
    },
    TVF {
        function: TVF,
        alias: Option<SQLTableAlias>,
    },
    Table {
        name: Table,
        alias: Option<SQLTableAlias>,
    },
    Query {
        lateral: Option<(StringEqCI!("LATERAL"), Space)>,
        open: (StringEq!("("), Space),
        query: Box<Query>,
        close: (Space, StringEq!(")")),
        alias: Option<SQLTableAlias>,
    },
    JTab {
        open: (StringEq!("("), Space),
        joined: Box<JTab>,
        close: (Space, StringEq!(")")),
    },
}

#[derive(Grammar)]
#[grammar(name = "json_table")]
pub struct JSONTable {
    kw: StringEq!("JSON_TABLE"),
    sp1: Space,
    open: StringEq!("("),
    sp2: Space,
    pub common_syntax: JSONApiCommonSyntax,
    sp3: Space,
    pub columns_spec: JSONTableColumnsSpec,
    sp4: Space,
    close: StringEq!(")"),
}

#[derive(Grammar)]
#[grammar(name = "json_api_common_syntax")]
pub struct JSONApiCommonSyntax {
    pub context_item: JSONContextItem,
    sp1: Space,
    comma: StringEq!(","),
    sp2: Space,
    pub path_spec: JSONPathSpecification,
}

#[derive(Grammar)]
#[grammar(name = "json_context_item")]
pub struct JSONContextItem {
    pub value_expr: JSONValueExpression,
}

#[derive(Grammar)]
#[grammar(name = "json_value_expression")]
pub struct JSONValueExpression {
    pub value: StringLiteral,
}

#[derive(Grammar)]
#[grammar(name = "json_table_columns_spec")]
pub enum JSONTableColumnsSpec {
    Columns(JSONTableColumnsClause),
    Type(JSONTableTypeClause),
}

#[derive(Grammar)]
#[grammar(name = "json_table_columns_clause")]
pub struct JSONTableColumnsClause {
    kw: StringEq!("COLUMNS"),
    sp1: Space,
    open: StringEq!("("),
    sp2: Space,
    pub columns: VecSep<JSONTableColumnDefinition, (Space, StringEq!(","), Space)>,
    sp3: Space,
    close: StringEq!(")"),
}

#[derive(Grammar)]
#[grammar(name = "json_table_column_definition")]
pub struct JSONTableColumnDefinition {
    pub regular: JSONTableRegularColumnDefinition,
}

#[derive(Grammar)]
#[grammar(name = "json_table_regular_column_definition")]
pub struct JSONTableRegularColumnDefinition {
    pub name: Ident,
    sp1: Space,
    pub data_type: Datyp,
    pub path_clause: Option<(
        Space,
        StringEq!("PATH"),
        Space,
        JSONTableColumnPathSpecification,
    )>,
}

#[derive(Grammar)]
#[grammar(name = "json_table_column_path_specification")]
pub struct JSONTableColumnPathSpecification {
    pub path_spec: JSONPathSpecification,
}

#[derive(Grammar)]
#[grammar(name = "json_table_type_clause")]
pub struct JSONTableTypeClause {
    kw: StringEq!("%TYPE"),
    sp1: Space,
    pub type_name: StringLiteral,
}

#[derive(Grammar)]
#[grammar(name = "json_path_specification")]
pub struct JSONPathSpecification {
    pub path: StringLiteral,
}

#[derive(Grammar)]
pub struct TVF {
    pub table: Table,
    pub open: (StringEq!("("), Space),
    pub args: Option<VecSep<TVFParameter, (Space, StringEq!(","), Space)>>,
    pub close: (Space, StringEq!(")")),
}

#[derive(Grammar)]
#[grammar(name = "tvf_parameter")]
pub enum TVFParameter {
    Literal(Literal),
    HostVar(HostVar),
}

pub type WhereClause = Prefix<(Space, StringEqCI!("WHERE"), Space), Prdct>;

pub type Group = Prefix<
    (Space, StringEqCI!("GROUP"), Space, StringEqCI!("BY"), Space),
    VecSep<Scalx, (Space, StringEq!(","), Space)>,
>;

pub type Having = Prefix<(Space, StringEqCI!("HAVING"), Space), Prdct>;

pub type Order = Prefix<
    (Space, StringEqCI!("ORDER"), Space, StringEqCI!("BY"), Space),
    VecSep<Ordir, (Space, StringEq!(","), Space)>,
>;

#[derive(Grammar)]
pub struct Ordir {
    pub expr: Ordit,
    pub direction: Option<(Space, SQLOrderDirection)>,
}

#[derive(Grammar)]
pub enum Ordit {
    Literal(Literal),
    Scalx(Scalx),
}

#[derive(Grammar)]
#[grammar(name = "row limiting clause")]
pub enum RowLimitingClause {
    LimitOffset {
        limit: Limit,
        offset: Option<Offset>,
    },
    OffsetFetch {
        offset: Offset,
        fetch: Option<Fetch>,
    },
    FetchOnly(Fetch),
}

pub type Limit = Prefix<(Space, StringEqCI!("LIMIT"), Space), LimitSpec>;

#[derive(Grammar)]
#[grammar(name = "limit spec")]
pub enum LimitSpec {
    Val(RowCount),
    All(StringEqCI!("ALL")),
}

#[derive(Grammar)]
#[grammar(name = "row count")]
pub enum RowCount {
    Val(ValSpec),
    Scalx(Scalx),
}

pub type Offset =
    Wrap<(Space, StringEqCI!("OFFSET"), Space), RowCount, Option<(Space, RowRowsInline)>>;

#[derive(Grammar)]
pub struct Fetch {
    pub keyword: (Space, StringEqCI!("FETCH"), Space),
    pub direction: Option<(Either<StringEqCI!("FIRST"), StringEqCI!("NEXT")>, Space)>,
    pub valspec: RowCount,
    pub row_keyword: Option<(Space, RowRowsInline)>,
    pub only: Option<(Space, StringEqCI!("ONLY"))>,
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum RowRowsInline {
    Rows(StringEqCI!("ROWS")),
    Row(StringEqCI!("ROW")),
}

#[derive(Grammar)]
pub struct Query(VecSep<QTerm, (Space, QueryOp, Space)>);

#[derive(Grammar)]
#[grammar(name = "query op")]
pub enum QueryOp {
    Union {
        union_kw: StringEqCI!("UNION"),
        all: Option<(Space, StringEqCI!("ALL"))>,
        parallel: Option<(Space, StringEqCI!("%parallel"))>,
    },
    Intersect(StringEqCI!("INTERSECT")),
    Except(StringEqCI!("EXCEPT")),
}

#[derive(Grammar)]
pub enum QTerm {
    Query((StringEq!("("), Space), Box<Query>, (Space, StringEq!(")"))),
    Selex(Selex),
}
#[derive(Grammar)]
pub struct Insrt {
    pub insert_kw: StringEqCI!("INSERT"),
    pub or_update: Option<(Space, StringEqCI!("OR"), Space, StringEqCI!("UPDATE"))>,
    pub options: Vec<InsertOption>,
    pub into_kw: Option<(Space, StringEqCI!("INTO"))>,
    pub target: (Space, IUDTarget),
    pub assignment: (Space, Asign),
}

#[derive(Grammar)]
#[grammar(name = "insert option")]
pub enum InsertOption {
    NoCheck((Space, StringEqCI!("%NOCHECK"))),
    NoFPlan((Space, StringEqCI!("%NOFPLAN"))),
    NoLock((Space, StringEqCI!("%NOLOCK"))),
    NoIndex((Space, StringEqCI!("%NOINDEX"))),
    NoTrigger((Space, StringEqCI!("%NOTRIGGER"))),
    NoStream((Space, StringEqCI!("%NOSTREAM"))),
    ObjectFiler((Space, StringEqCI!("%OBJECTFILER"))),
    Profile((Space, StringEqCI!("%PROFILE"))),
    ProfileAll((Space, StringEqCI!("%PROFILE_ALL"))),
    NoRuntime((Space, StringEqCI!("%NORUNTIME"))),
    NoJourn((Space, StringEqCI!("%NOJOURN"))),
    DoRuntime((Space, StringEqCI!("%DORUNTIME"))),
}

#[derive(Grammar)]
#[grammar(name = "iud_target")]
pub enum IUDTarget {
    Table {
        name: Table,
        alias: Option<SQLTableAlias>,
    },
    Subquery {
        open: (StringEq!("("), Space),
        query: Box<Query>,
        close: (Space, StringEq!(")")),
        alias: Option<SQLTableAlias>,
    },
}

#[derive(Grammar)]
pub enum Asign {
    SetList {
        set_kw: StringEqCI!("SET"),
        assignments: (
            Space,
            VecSep<SQLSetAssignment, (Space, StringEq!(","), Space)>,
        ),
    },
    ColumnList {
        columns: Option<SQLColumnListParen>,
        values: (Space, ValueSource),
    },
    DefaultValues(StringEqCI!("DEFAULT"), Space, StringEqCI!("VALUES")),
}

pub type FList1 = Wrap<
    (StringEq!("("), Space),
    VecSep<Ident, (Space, StringEq!(","), Space)>,
    (Space, StringEq!(")")),
>;

pub type SQLColumnListParen = Wrap<
    (Space, StringEq!("("), Space),
    VecSep<Colmn, (Space, StringEq!(","), Space)>,
    (Space, StringEq!(")")),
>;

#[derive(Grammar)]
#[grammar(name = "value source")]
pub enum ValueSource {
    Query {
        query: Box<Query>,
        order: Option<Order>,
    },
    ValSpec {
        values_kw: StringEqCI!("VALUES"),
        sp: Space,
        spec: ValSpec,
    },
}

#[derive(Grammar)]
#[grammar(name = "val spec")]
pub enum ValSpec {
    Scalx {
        open: (StringEq!("("), Space),
        exprs: VecSep<Scalx, (Space, StringEq!(","), Space)>,
        close: (Space, StringEq!(")")),
    },
    Var {
        var: Var,
        open: (StringEq!("("), Space),
        subscripts: Vec<(Sub, StringEq!(","))>,
        close: (Space, StringEq!(")")),
    },
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct SQLSetAssignment {
    pub column: Colmn,
    pub eq: (Space, StringEq!("="), Space),
    pub value: Scalx,
}

#[derive(Grammar)]
#[grammar(name = "updat")]
pub enum Updat {
    UpdateStats {
        update_kw: StringEqCI!("UPDATE"),
        stats_kw: (Space, StringEqCI!("STATISTICS"), Space),
        table: TName,
    },
    Update {
        update_kw: StringEqCI!("UPDATE"),
        options: Vec<UpdateOption>,
        target: (Space, IUDTarget),
        assignment: (Space, Asign),
        from: Option<FromClause>,
        where_clause: Option<WhereWerop>,
    },
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct WhereWerop {
    sp1: Space,
    kw: StringEqCI!("WHERE"),
    sp2: Space,
    werop: Werop,
}

#[derive(Grammar)]
pub enum Werop {
    Prdct(Prdct),
    CurrentOf {
        current_of: (StringEqCI!("CURRENT"), Space, StringEqCI!("OF"), Space),
        ident: Ident,
        prdct: Option<Spaced<AndClause<Prdct>>>,
    },
}

#[derive(Grammar)]
#[grammar(name = "update option")]
pub enum UpdateOption {
    NoCheck((Space, StringEqCI!("%NOCHECK"))),
    NoFPlan((Space, StringEqCI!("%NOFPLAN"))),
    NoLock((Space, StringEqCI!("%NOLOCK"))),
    NoIndex((Space, StringEqCI!("%NOINDEX"))),
    NoTrigger((Space, StringEqCI!("%NOTRIGGER"))),
    NoStream((Space, StringEqCI!("%NOSTREAM"))),
    NoJourn((Space, StringEqCI!("%NOJOURN"))),
    ILock((Space, StringEqCI!("%ILOCK"))),
    ObjectFiler((Space, StringEqCI!("%OBJECTFILER"))),
    Profile((Space, StringEqCI!("%PROFILE"))),
    ProfileAll((Space, StringEqCI!("%PROFILE_ALL"))),
    NoRuntime((Space, StringEqCI!("%NORUNTIME"))),
    DoRuntime((Space, StringEqCI!("%DORUNTIME"))),
}

#[derive(Grammar)]
#[grammar(name = "delet")]
pub struct Delet {
    pub delete_kw: StringEqCI!("DELETE"),
    pub options: Vec<DeleteOption>,
    pub from_kw: Option<(Space, StringEqCI!("FROM"))>,
    pub target: (Space, IUDTarget),
    pub where_clause: Option<WhereWerop>,
}

#[derive(Grammar)]
#[grammar(name = "delete option")]
pub enum DeleteOption {
    NoCheck((Space, StringEqCI!("%NOCHECK"))),
    NoFPlan((Space, StringEqCI!("%NOFPLAN"))),
    NoLock((Space, StringEqCI!("%NOLOCK"))),
    NoIndex((Space, StringEqCI!("%NOINDEX"))),
    NoTrigger((Space, StringEqCI!("%NOTRIGGER"))),
    ObjectFiler((Space, StringEqCI!("%OBJECTFILER"))),
    Profile((Space, StringEqCI!("%PROFILE"))),
    ProfileAll((Space, StringEqCI!("%PROFILE_ALL"))),
    NoRuntime((Space, StringEqCI!("%NORUNTIME"))),
    NoJourn((Space, StringEqCI!("%NOJOURN"))),
    DoRuntime((Space, StringEqCI!("%DORUNTIME"))),
}

#[derive(Grammar)]
#[grammar(name = "truncate")]
pub struct Truncate {
    pub truncate_kw: StringEqCI!("TRUNCATE"),
    pub table_kw: (Space, StringEqCI!("TABLE"), Space),
    pub options: Vec<TruncateOption>,
    pub table: (Space, TName),
}

#[derive(Grammar)]
#[grammar(name = "truncate option")]
pub enum TruncateOption {
    NoCheck((Space, StringEqCI!("%NOCHECK"))),
    NoLock((Space, StringEqCI!("%NOLOCK"))),
    NoJourn((Space, StringEqCI!("%NOJOURN"))),
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum TpRelated {
    StartTransaction(StartTP),
    SetTransaction(SetTP),
    Rollback(RollB),
    TPOption(TP),
}

#[derive(Grammar)]
#[grammar(name = "starttp")]
pub struct StartTP {
    pub kw: (
        StringEqCI!("START"),
        Space,
        StringEqCI!("TRANSACTION"),
        Space,
    ),
    pub commit_mode: Option<(CommitModeSpec, Space)>,
    pub modes: VecSep<Option<TRMode>, (Space, StringEq!(","), Space)>,
}

#[derive(Grammar)]
#[grammar(name = "settp")]
pub struct SetTP {
    pub start_kw: Option<(StringEqCI!("START"), Space)>,
    pub set_kw: StringEqCI!("SET"),
    pub transaction_kw: (Space, StringEqCI!("TRANSACTION")),
    pub commit_mode: Option<(Space, CommitModeSpec)>,
    pub modes: VecSep<Option<(Space, TRMode)>, (Space, StringEq!(","))>,
}

pub type CommitModeSpec = Prefix<StringEqCI!("%COMMITMODE"), (Space, CommitMode)>;

#[derive(Grammar)]
pub enum CommitMode {
    Implicit(StringEqCI!("IMPLICIT")),
    Explicit(StringEqCI!("EXPLICIT")),
    None(StringEqCI!("NONE")),
}

#[derive(Grammar)]
pub enum TRMode {
    IsolationLevel {
        isolation_kw: StringEqCI!("ISOLATION"),
        level_kw: (Space, StringEqCI!("LEVEL"), Space),
        level: GetLevel,
    },
    ReadOnly(StringEqCI!("READ"), Space, StringEqCI!("ONLY")),
    ReadWrite(StringEqCI!("READ"), Space, StringEqCI!("WRITE")),
    DiagnosticsSize {
        diagnostics_kw: StringEqCI!("DIAGNOSTICS"),
        size_kw: (Space, StringEqCI!("SIZE"), Space),
        size: Term,
    },
}

#[derive(Grammar)]
pub enum GetLevel {
    ReadUncommitted(StringEqCI!("READ"), Space, StringEqCI!("UNCOMMITTED")),
    ReadCommitted(StringEqCI!("READ"), Space, StringEqCI!("COMMITTED")),
    ReadVerified(StringEqCI!("READ"), Space, StringEqCI!("VERIFIED")),
    RepeatableRead(StringEqCI!("REPEATABLE"), Space, StringEqCI!("READ")),
    Serializable(StringEqCI!("SERIALIZABLE")),
}

#[derive(Grammar)]
#[grammar(name = "rollb")]
pub struct RollB {
    pub rollback_kw: StringEqCI!("ROLLBACK"),
    pub work: Option<(Space, StringEqCI!("WORK"))>,
}

pub type TP = Prefix<
    Option<(
        StringEqCI!("START"),
        Space,
        Option<(StringEqCI!("SET"), Space)>,
    )>,
    TPOption,
>;

#[derive(Grammar)]
#[grammar(name = "tp option")]
pub enum TPOption {
    InTransaction(StringEqCI!("%INTRANSACTION")),
    BegTrans(StringEqCI!("%BEGTRANS")),
    InTrans(StringEqCI!("%INTRANS")),
    CommitWork {
        commit_kw: StringEqCI!("COMMIT"),
        work: Option<(Space, StringEqCI!("WORK"))>,
    },
}

#[derive(Grammar)]
#[grammar(name = "ofc")]
pub enum OFC {
    Open {
        open_kw: (StringEqCI!("OPEN"), Space),
        cursor: Ident,
    },
    Fetch {
        fetch_kw: (StringEqCI!("FETCH"), Space),
        cursor: Ident,
        targt: Targt,
    },
    Close {
        close_kw: (StringEqCI!("CLOSE"), Space),
        cursor: Ident,
    },
}

#[derive(Grammar)]
#[grammar(name = "decl")]
pub enum Decl {
    Explain {
        declare_kw: StringEqCI!("DECLARE"),
        cursor_name: (Space, Ident),
        cursor_kw: (
            Space,
            StringEqCI!("CURSOR"),
            Space,
            StringEqCI!("FOR"),
            Space,
        ),
        statement: Explain,
        for_clause: Option<SQLDeclareFor>,
    },
    Selct {
        declare_kw: StringEqCI!("DECLARE"),
        cursor_name: (Space, Ident),
        cursor_kw: (
            Space,
            StringEqCI!("CURSOR"),
            Space,
            StringEqCI!("FOR"),
            Space,
        ),
        statement: (Option<(WithClause, Space)>, Selct),
        for_clause: Option<SQLDeclareFor>,
    },
}

pub type SQLDeclareFor = Prefix<(Space, StringEqCI!("FOR"), Space), UpdtCls>;

#[derive(Grammar)]
pub enum UpdtCls {
    ReadOnly(StringEqCI!("READ"), Space, StringEqCI!("ONLY")),
    Update(StringEqCI!("UPDATE")),
}

#[derive(Grammar)]
#[grammar(name = "get")]
pub struct Get {
    pub start_set: Option<(
        StringEqCI!("START"),
        Space,
        Option<(StringEqCI!("SET"), Space)>,
    )>,
    pub get_kw: StringEqCI!("GET"),
    pub assignment: Option<(
        Space,
        Ident,
        Space,
        Var,
        Space,
        StringEq!("="),
        Space,
        StringEqCI!("%ROWCOUNT"),
    )>,
}

#[derive(Grammar)]
#[grammar(name = "call")]
pub struct Call {
    pub output: Option<(StringEq!(":"), HostIdentifier, Space, StringEq!("="), Space)>,
    pub call_kw: StringEqCI!("CALL"),
    pub routine: (Space, RoutineName),
    pub args: SQLArgumentList,
    pub using: Option<(
        Space,
        StringEqCI!("USING"),
        Space,
        StringEq!(":"),
        HostIdentifier,
    )>,
}

#[derive(Grammar)]
#[grammar(name = "routine name")]
pub struct RoutineName(pub SQLFunctionName);

pub type SQLArgumentList = Wrap<
    (Space, StringEq!("("), Space),
    Option<VecSep<SQLArgument, (Space, StringEq!(","), Space)>>,
    (Space, StringEq!(")")),
>;

#[derive(Grammar)]
#[grammar(name = "SQL argument")]
pub enum SQLArgument {
    Expr(Scalx),
    HostVar(StringEq!(":"), HostIdentifier),
}

#[derive(Grammar)]
#[grammar(name = "explain")]
pub struct Explain {
    pub explain_kw: StringEqCI!("EXPLAIN"),
    pub options: Option<ExplainOptionList>,
    pub into: Option<(Space, StringEqCI!("INTO"), Space, Param)>,
    pub statement: (Space, Box<ExplainSQLStatement>),
}

#[derive(Grammar)]
#[grammar(name = "explain option list")]
pub struct ExplainOptionList {
    pub alt: Option<(Space, AltOption)>,
    pub stat: Option<(Space, StatOption)>,
}

#[derive(Grammar)]
#[grammar(name = "alt option")]
pub enum AltOption {
    Alt(StringEqCI!("ALT")),
    All(StringEqCI!("ALL")),
}

#[derive(Grammar)]
#[grammar(name = "stat option")]
pub enum StatOption {
    Stat(StringEqCI!("STAT")),
    Stats(StringEqCI!("STATS")),
}

#[derive(Grammar)]
#[grammar(name = "explain sql statement")]
pub struct ExplainSQLStatement(Vec<ExplainSQLStatementElement>);

#[derive(Grammar)]
#[grammar(inline)]
pub enum ExplainSQLStatementElement {
    Select(Selct),
    With(WithClause),
    Update(Updat),
    Delete(Delet),
}

#[derive(Grammar)]
#[grammar(name = "selct")]
pub struct Selct {
    query: Query,
    order: Option<Order>,
    row_limiting_clause: Option<RowLimitingClause>,
}

#[derive(Grammar)]
#[grammar(name = "option")]
pub struct SQLOption {
    pub set_kw: StringEqCI!("SET"),
    pub option_kw: (Space, StringEqCI!("OPTION"), Space),
    pub setting: OptionSetting,
}

#[derive(Grammar)]
#[grammar(name = "compile option")]
pub struct CompileOption {
    compilemode_kw: StringEqCI!("COMPILEMODE"),
    eq: (Space, StringEq!("="), Space),
    mode: CompileMode,
}

#[derive(Grammar)]
#[grammar(name = "option setting")]
pub enum OptionSetting {
    CompileOption(CompileOption),
    LockTimeout {
        lock_kw: StringEqCI!("LOCK_TIMEOUT"),
        eq: (Space, StringEq!("="), Space),
        value: PositivceNumericLiteralValue,
    },
    PkeyIsIdkey {
        pkey_kw: StringEqCI!("PKEY_IS_IDKEY"),
        eq: (Space, StringEq!("="), Space),
        value: TrueFalse,
    },
    SupportDelimitedIdentifiers {
        support_kw: StringEqCI!("SUPPORT_DELIMITED_IDENTIFIERS"),
        eq: (Space, StringEq!("="), Space),
        value: TrueFalse,
    },
    AutoParallelThreshold {
        auto_kw: StringEqCI!("AUTO_PARALLEL_THRESHOLD"),
        eq: (Space, StringEq!("="), Space),
        value: PositivceNumericLiteralValue,
    },
    DefaultSchema {
        default_kw: StringEqCI!("DEFAULT_SCHEMA"),
        eq: (Space, StringEq!("="), Space),
        schema: Schema,
    },
    ExactDistinct {
        exact_kw: StringEqCI!("EXACT_DISTINCT"),
        eq: (Space, StringEq!("="), Space),
        value: TrueFalse,
    },
    LockEscalationThreshold {
        lock_kw: StringEqCI!("LOCK_ESCALATION_THRESHOLD"),
        eq: (Space, StringEq!("="), Space),
        value: PositivceNumericLiteralValue,
    },
    LocaleOption(LocaleOption),
}

#[derive(Grammar)]
#[grammar(name = "compile mode")]
pub enum CompileMode {
    Deferred(StringEqCI!("DEFERRED")),
    Immediate(StringEqCI!("IMMEDIATE")),
    Install(StringEqCI!("INSTALL")),
    NoCheck(StringEqCI!("NOCHECK")),
}

#[derive(Grammar)]
#[grammar(name = "TRUE/FALSE")]
pub enum TrueFalse {
    False(StringEqCI!("FALSE")),
    True(StringEqCI!("TRUE")),
}

#[derive(Grammar)]
#[grammar(name = "locale option")]
pub struct LocaleOption {
    pub setting: LocaleSetting,
    pub eq: (Space, StringEq!("="), Space),
    pub value: Literal,
}

#[derive(Grammar)]
#[grammar(name = "locale setting")]
pub enum LocaleSetting {
    Am(StringEqCI!("AM")),
    DateFormat(StringEqCI!("DATE_FORMAT")),
    DateMaximum(StringEqCI!("DATE_MAXIMUM")),
    DateMinimum(StringEqCI!("DATE_MINIMUM")),
    DateSeparator(StringEqCI!("DATE_SEPARATOR")),
    DecimalSeparator(StringEqCI!("DECIMAL_SEPARATOR")),
    Midnight(StringEqCI!("MIDNIGHT")),
    MinusSign(StringEqCI!("MINUS_SIGN")),
    MonthAbbr(StringEqCI!("MONTH_ABBR")),
    MonthName(StringEqCI!("MONTH_NAME")),
    Noon(StringEqCI!("NOON")),
    NumericGroupSeparator(StringEqCI!("NUMERIC_GROUP_SEPARATOR")),
    NumericGroupSize(StringEqCI!("NUMERIC_GROUP_SIZE")),
    Pm(StringEqCI!("PM")),
    PlusSign(StringEqCI!("PLUS_SIGN")),
    TimeFormat(StringEqCI!("TIME_FORMAT")),
    TimePrecision(StringEqCI!("TIME_PRECISION")),
    TimeSeparator(StringEqCI!("TIME_SEPARATOR")),
    WeekdayAbbr(StringEqCI!("WEEKDAY_ABBR")),
    WeekdayName(StringEqCI!("WEEKDAY_NAME")),
    YearOption(StringEqCI!("YEAR_OPTION")),
}

#[derive(Grammar)]
#[grammar(name = "setml")]
pub struct Setml {
    pub set_kw: StringEqCI!("SET"),
    pub ml_kw: (Space, StringEqCI!("ML"), Space),
    pub configuration_kw: (StringEqCI!("CONFIGURATION"), Space),
    pub name: MLConfigurationName,
}

#[derive(Grammar)]
#[grammar(name = "ml_configuration_name")]
pub struct MLConfigurationName(Ident);

#[derive(Grammar)]
#[grammar(name = "ml_configuration_list")]
pub struct MLConfigurationList(VecSep<MLConfigurationName, (Space, StringEq!(","), Space)>);

pub type Grant = Prefix<StringEqCI!("GRANT"), (Space, GrantCmd)>;

#[derive(Grammar)]
#[grammar(name = "grant cmd")]
pub enum GrantCmd {
    AdminPrivileges {
        privileges: VecSep<AdminPriv, (Space, StringEq!(","), Space)>,
        to_kw: (Space, StringEqCI!("TO"), Space),
        grantees: Grntee,
        with_admin: Option<(
            Space,
            StringEqCI!("WITH"),
            Space,
            StringEqCI!("ADMIN"),
            Space,
            StringEqCI!("OPTION"),
        )>,
    },
    Roles {
        roles: VecSep<Role, (Space, StringEq!(","), Space)>,
        to_kw: (Space, StringEqCI!("TO"), Space),
        grantees: Grntee,
        with_admin: Option<(
            Space,
            StringEqCI!("WITH"),
            Space,
            StringEqCI!("ADMIN"),
            Space,
            StringEqCI!("OPTION"),
        )>,
    },
    ObjectPrivileges {
        privileges: SimplePrivs,
        on_kw: (Space, StringEqCI!("ON"), Space),
        objects: Objcts,
        to_kw: (Space, StringEqCI!("TO"), Space),
        grantees: Grntee,
        with_grant: Option<(
            Space,
            StringEqCI!("WITH"),
            Space,
            StringEqCI!("GRANT"),
            Space,
            StringEqCI!("OPTION"),
        )>,
    },
    Actions {
        actions: VecSep<Action, (Space, StringEq!(","), Space)>,
        on_kw: (Space, StringEqCI!("ON"), Space),
        object: Object,
        to_kw: (Space, StringEqCI!("TO"), Space),
        grantees: Grntee,
        with_grant: Option<(
            Space,
            StringEqCI!("WITH"),
            Space,
            StringEqCI!("GRANT"),
            Space,
            StringEqCI!("OPTION"),
        )>,
    },
    CubeSelect {
        select_kw: StringEqCI!("SELECT"),
        on_kw: (Space, StringEqCI!("ON"), Space),
        cube_kw: StringEqCI!("CUBE"),
        cubes_suffix: Option<StringEq!("S")>,
        cubes: (Space, CubeList),
        to_kw: (Space, StringEqCI!("TO"), Space),
        grantees: Grntee,
        with_grant: Option<(
            Space,
            StringEqCI!("WITH"),
            Space,
            StringEqCI!("GRANT"),
            Space,
            StringEqCI!("OPTION"),
        )>,
    },
}

pub type Revok = Prefix<StringEqCI!("REVOKE"), (Space, RevokeCmd)>;

#[derive(Grammar)]
#[grammar(name = "revoke cmd")]
pub enum RevokeCmd {
    AdminPrivileges {
        privileges: VecSep<AdminPriv, (Space, StringEq!(","), Space)>,
        from_kw: (Space, StringEqCI!("FROM"), Space),
        grantees: Grntee,
    },
    Roles {
        roles: VecSep<Role, (Space, StringEq!(","), Space)>,
        from_kw: (Space, StringEqCI!("FROM"), Space),
        grantees: Grntee,
    },
    ObjectPrivileges {
        grant_option: Option<(
            StringEqCI!("GRANT"),
            Space,
            StringEqCI!("OPTION"),
            Space,
            StringEqCI!("FOR"),
            Space,
        )>,
        privileges: SimplePrivs,
        on_kw: (Space, StringEqCI!("ON"), Space),
        objects: VecSep<Object, (Space, StringEq!(","), Space)>,
        from_kw: (Space, StringEqCI!("FROM"), Space),
        grantees: Grntee,
        behavior: Option<(Space, RevokeBehavior)>,
        as_clause: Option<(Space, StringEqCI!("AS"), Space, Grantor)>,
    },
    Actions {
        grant_option: Option<(
            StringEqCI!("GRANT"),
            Space,
            StringEqCI!("OPTION"),
            Space,
            StringEqCI!("FOR"),
            Space,
        )>,
        actions: VecSep<Action, (Space, StringEq!(","), Space)>,
        on_kw: (Space, StringEqCI!("ON"), Space),
        object: Object,
        from_kw: (Space, StringEqCI!("FROM"), Space),
        grantees: Grntee,
        behavior: Option<(Space, RevokeBehavior)>,
        as_clause: Option<(Space, StringEqCI!("AS"), Space, Grantor)>,
    },
    CubeSelect {
        grant_option: Option<(
            StringEqCI!("GRANT"),
            Space,
            StringEqCI!("OPTION"),
            Space,
            StringEqCI!("FOR"),
            Space,
        )>,
        select_kw: StringEqCI!("SELECT"),
        on_kw: (Space, StringEqCI!("ON"), Space),
        cube_kw: StringEqCI!("CUBE"),
        cubes_suffix: Option<StringEq!("S")>,
        cubes: CubeList,
        from_kw: (Space, StringEqCI!("FROM"), Space),
        grantees: Grntee,
        behavior: Option<(Space, RevokeBehavior)>,
    },
}

#[derive(Grammar)]
#[grammar(name = "revoke behavior")]
pub enum RevokeBehavior {
    Cascade(StringEqCI!("CASCADE")),
    Restrict(StringEqCI!("RESTRICT")),
}

#[derive(Grammar)]
#[grammar(name = "simple privs")]
pub enum SimplePrivs {
    All {
        all_kw: StringEqCI!("ALL"),
        privileges_kw: Option<(Space, StringEqCI!("PRIVILEGES"))>,
    },
    Star(StringEq!("*")),
    List(VecSep<SimplePriv, (Space, StringEq!(","), Space)>),
    SPPriv(SpPriv),
    MLConfigPriv(MLConfigPrivilege),
}

#[derive(Grammar)]
#[grammar(name = "simple priv")]
pub enum SimplePriv {
    Alter(StringEqCI!("%ALTER")),
    Delete(StringEqCI!("DELETE")),
    OptCol(OptColPriv),
}

#[derive(Grammar)]
#[grammar(name = "sp priv")]
pub enum SpPriv {
    Execute(StringEqCI!("EXECUTE")),
}

#[derive(Grammar)]
#[grammar(name = "ml_configuration priv")]
pub enum MLConfigPrivilege {
    Use(StringEqCI!("USE")),
}

#[derive(Grammar)]
pub enum Action {
    Complex {
        privilege: OptColPriv,
        sp: Space,
        columns: FList1,
    },
    Simple(SimplePriv),
}

#[derive(Grammar)]
#[grammar(name = "opt col priv")]
pub enum OptColPriv {
    Select(StringEqCI!("SELECT")),
    Insert(StringEqCI!("INSERT")),
    Update(StringEqCI!("UPDATE")),
    References(StringEqCI!("REFERENCES")),
}

#[derive(Grammar)]
#[grammar(name = "admin priv")]
pub enum AdminPriv {
    CreateMethod(StringEqCI!("%CREATE_METHOD")),
    DropMethod(StringEqCI!("%DROP_METHOD")),
    CreateFunction(StringEqCI!("%CREATE_FUNCTION")),
    DropFunction(StringEqCI!("%DROP_FUNCTION")),
    CreateProcedure(StringEqCI!("%CREATE_PROCEDURE")),
    DropProcedure(StringEqCI!("%DROP_PROCEDURE")),
    CreateQuery(StringEqCI!("%CREATE_QUERY")),
    DropQuery(StringEqCI!("%DROP_QUERY")),
    CreateTable(StringEqCI!("%CREATE_TABLE")),
    AlterTable(StringEqCI!("%ALTER_TABLE")),
    DropTable(StringEqCI!("%DROP_TABLE")),
    CreateView(StringEqCI!("%CREATE_VIEW")),
    AlterView(StringEqCI!("%ALTER_VIEW")),
    DropView(StringEqCI!("%DROP_VIEW")),
    CreateTrigger(StringEqCI!("%CREATE_TRIGGER")),
    DropTrigger(StringEqCI!("%DROP_TRIGGER")),
    DBObjectDefinition(StringEqCI!("%DB_OBJECT_DEFINITION")),
    NoCheck(StringEqCI!("%NOCHECK")),
    NoTrigger(StringEqCI!("%NOTRIGGER")),
    NoIndex(StringEqCI!("%NOINDEX")),
    NoLock(StringEqCI!("%NOLOCK")),
    CreateMLConfiguration(StringEqCI!("%CREATE_ML_CONFIGURATION")),
    AlterMLConfiguration(StringEqCI!("%ALTER_ML_CONFIGURATION")),
    DropMLConfiguration(StringEqCI!("%DROP_ML_CONFIGURATION")),
    ManageModel(StringEqCI!("%MANAGE_MODEL")),
    UseModel(StringEqCI!("%USE_MODEL")),
    NoJourn(StringEqCI!("%NOJOURN")),
}

#[derive(Grammar)]
pub enum Grntee {
    Star(StringEq!("*")),
    List(SQLGranteeList),
}

#[derive(Grammar)]
pub enum Grantor {
    Star(StringEq!("*")),
    List(SQLGranteeList),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct SQLGranteeList {
    pub first: User,
    pub rest: Vec<((Space, StringEq!(","), Space), User)>,
}

#[derive(Grammar)]
pub enum User {
    Public(StringEqCI!("PUBLIC")),
    Ident(Option<StringEq!("_")>, Ident),
}

#[derive(Grammar)]
pub struct Role(Ident);

#[derive(Grammar)]
pub enum Objcts {
    Star(StringEq!("*")),
    Schemas {
        schema_kw: StringEqCI!("SCHEMA"),
        space: Space,
        schemas: VecSep<Schema, (Space, StringEq!(","), Space)>,
    },
    Tables(VecSep<Object, (Space, StringEq!(","), Space)>),
    Cubes(CubeList),
    MLConfigs(MLConfigurationList),
    ForeignServers(ForeignServerPrivilegeClause),
}

#[derive(Grammar)]
pub struct Object(TName);

#[derive(Grammar)]
pub struct Schema(Ident);

#[derive(Grammar)]
#[grammar(name = "cubelist")]
pub struct CubeList(VecSep<CubeName, (Space, StringEq!(","), Space)>);

#[derive(Grammar)]
#[grammar(name = "cubename")]
pub struct CubeName(pub Ident);

#[derive(Grammar)]
#[grammar(name = "foreign-server-privilege-list")]
pub struct ForeignServerPrivilegeList(VecSep<TName, (Space, StringEq!(","), Space)>);

#[derive(Grammar)]
#[grammar(name = "foreign-server-privilege-clause")]
pub struct ForeignServerPrivilegeClause {
    pub foreign_kw: Option<(StringEqCI!("FOREIGN"), Space)>,
    pub server_kw: ServersServer,
    pub servers: (Space, ForeignServerPrivilegeList),
}

#[derive(Grammar)]
#[grammar(name = "SERVERS/SERVER")]
pub enum ServersServer {
    Server(StringEqCI!("SERVER")),
    Servers(StringEqCI!("SERVERS")),
}

#[derive(Grammar)]
#[grammar(name = "chkprv")]
pub struct ChkPrv {
    pub checkpriv_kw: StringEqCI!("%CHECKPRIV"),
    pub grant_option: Option<(Space, GrantOpt)>,
    pub command: (Space, ChkPrvCmd),
}

#[derive(Grammar)]
#[grammar(name = "grant opt")]
pub enum GrantOpt {
    Grant(
        StringEqCI!("GRANT"),
        Space,
        StringEqCI!("OPTION"),
        Space,
        StringEqCI!("FOR"),
    ),
    Admin(
        StringEqCI!("ADMIN"),
        Space,
        StringEqCI!("OPTION"),
        Space,
        StringEqCI!("FOR"),
    ),
}

#[derive(Grammar)]
#[grammar(name = "chkprv cmd")]
pub enum ChkPrvCmd {
    AdminOrAlt(VecSep<AdminOrAlt, (Space, StringEq!(","), Space)>),
    PrivSpec {
        priv_spec: PrivSpec,
        on_kw: (Space, StringEqCI!("ON"), Space),
        object: Object,
    },
}

#[derive(Grammar)]
#[grammar(name = "admin or alt")]
pub enum AdminOrAlt {
    Admin(AdminPriv),
}

#[derive(Grammar)]
#[grammar(name = "priv spec")]
pub enum PrivSpec {
    Action(Action),
    Threshold(StringEqCI!("%THRESHOLD")),
}

type OptParenScalx = Option<Parenthesized<Scalx>>;
type OptParenScalx2 = Option<Parenthesized<(Scalx, Option<(Space, StringEq!(","), Space, Scalx)>)>>;

#[derive(Grammar)]
#[grammar(name = "cast type")]
pub enum CastType {
    Bigint(StringEqCI!("BIGINT")),
    BinaryVarying(
        StringEqCI!("BINARY"),
        Space,
        StringEqCI!("VARYING"),
        OptParenScalx,
    ),
    Binary(StringEqCI!("BINARY"), OptParenScalx),
    Bit(StringEqCI!("BIT")),
    CharacterVarying(
        StringEqCI!("CHARACTER"),
        Space,
        StringEqCI!("VARYING"),
        OptParenScalx,
    ),
    Character(StringEqCI!("CHARACTER"), OptParenScalx),
    CharVarying(
        StringEqCI!("CHAR"),
        Space,
        StringEqCI!("VARYING"),
        OptParenScalx,
    ),
    Char(StringEqCI!("CHAR"), OptParenScalx),
    Datetime(StringEqCI!("DATETIME")),
    Date(StringEqCI!("DATE")),
    Decimal(StringEqCI!("DECIMAL"), OptParenScalx2),
    Dec(StringEqCI!("DEC"), OptParenScalx2),
    Double(StringEqCI!("DOUBLE")),
    Float(StringEqCI!("FLOAT")),
    Guid(StringEqCI!("GUID")),
    Integer(StringEqCI!("INTEGER")),
    Int(StringEqCI!("INT")),
    Money(StringEqCI!("MONEY")),
    Nchar(StringEqCI!("NCHAR"), OptParenScalx),
    Numeric(StringEqCI!("NUMERIC"), OptParenScalx2),
    Nvarchar(StringEqCI!("NVARCHAR"), OptParenScalx),
    Posixtime(StringEqCI!("POSIXTIME")),
    Real(StringEqCI!("REAL")),
    Smalldatetime(StringEqCI!("SMALLDATETIME")),
    Smallint(StringEqCI!("SMALLINT")),
    Smallmoney(StringEqCI!("SMALLMONEY")),
    Timestamp(StringEqCI!("TIMESTAMP")),
    Time(StringEqCI!("TIME")),
    Tinyint(StringEqCI!("TINYINT")),
    Varbinary(StringEqCI!("VARBINARY"), OptParenScalx),
    Varchar(StringEqCI!("VARCHAR"), OptParenScalx),
}

#[derive(Grammar)]
#[grammar(name = "ddl")]
pub enum DDL {
    CreOrAltView {
        cre_or_alt: CreOrAlt,
        kw: (Space, StringEqCI!("VIEW"), Space),
        tname: TName,
        col_list: Option<(
            (Space, StringEq!("("), Space),
            VecSep<Ident, (Space, StringEq!(","), Space)>,
            (Space, StringEq!(")")),
        )>,
        as_kw: (Space, StringEqCI!("AS"), Space),
        query: Query,
        with_clause: Option<((Space, StringEqCI!("WITH"), Space), ViewWith)>,
    },
    AlterStatsDropFixed {
        kw: (StringEqCI!("ALTER"), Space),
        scope: StatsScope,
        drop_fixed: (
            Space,
            StringEqCI!("DROP"),
            Space,
            StringEqCI!("FIXED"),
            Space,
        ),
        clause: StatsClause,
    },
    AlterStatsFix {
        kw: (StringEqCI!("ALTER"), Space),
        scope: StatsScope,
        fix: (Space, StringEqCI!("FIX"), Space),
        clause: StatsClause,
    },
    AlterStatsSetRuntime {
        kw: (StringEqCI!("ALTER"), Space),
        scope: StatsScope,
        set_runtime: (
            Space,
            StringEqCI!("SET"),
            Space,
            StringEqCI!("RUNTIME"),
            Space,
        ),
        prop: StatsProperty,
        eq: (Space, StringEq!("="), Space),
        val: TrueFalse,
    },
    AlterForeignTable {
        kw: (
            StringEqCI!("ALTER"),
            Space,
            StringEqCI!("FOREIGN"),
            Space,
            StringEqCI!("TABLE"),
            Space,
        ),
        tname: TName,
        action: (Space, AlterFTAction),
    },
    AlterMlConfiguration {
        kw: (
            StringEqCI!("ALTER"),
            Space,
            StringEqCI!("ML"),
            Space,
            StringEqCI!("CONFIGURATION"),
            Space,
        ),
        name: MLConfigurationName,
        provider: Option<((Space, StringEqCI!("PROVIDER"), Space), ProviderName)>,
        desc: Option<((Space, StringEqCI!("%DESCRIPTION"), Space), MlconfigDesc)>,
        using_clause: Option<(Space, MlconfigUsingClause)>,
        settings: Option<(Space, ProviderSpecificSettings)>,
    },
    AlterModel {
        kw: (StringEqCI!("ALTER"), Space, StringEqCI!("MODEL"), Space),
        name: ModelName,
        action: (Space, AlterModelAction),
    },
    AlterTable {
        kw: (StringEqCI!("ALTER"), Space, StringEqCI!("TABLE"), Space),
        tname: TName,
        with_check: Option<((Space, StringEqCI!("WITH"), Space), CheckSpec)>,
        action: Option<(Space, AlterAction)>,
    },
    AlterUser {
        kw: (StringEqCI!("ALTER"), Space, StringEqCI!("USER"), Space),
        name: Ident,
        password: Option<(Space, PasswordClause)>,
        default: Option<(
            (Space, StringEqCI!("DEFAULT"), Space),
            Ident,
            (Space, Ident),
        )>,
        temporary: Option<(
            (Space, StringEqCI!("TEMPORARY"), Space),
            Ident,
            (Space, Ident),
        )>,
    },
    AlterServer {
        kw: (StringEqCI!("ALTER"), Space),
        foreign: Option<(StringEqCI!("FOREIGN"), Space)>,
        server_kw: (StringEqCI!("SERVER"), Space),
        tname: TName,
        action: (Space, AlterServerAction),
    },
    BuildIndex {
        kw: (StringEqCI!("BUILD"), Space, StringEqCI!("INDEX"), Space),
        options: Vec<(BuildOption, Space)>,
        for_kw: (StringEqCI!("FOR"), Space),
        scope: BuildScope,
    },
    CancelQuery {
        kw: (StringEqCI!("CANCEL"), Space, StringEqCI!("QUERY"), Space),
        val: ValSpec,
        cancel_scope: Option<(Space, CancelQueryScope)>,
        timeout: Option<(Space, TimeoutScope)>,
    },
    Collect {
        kw: (StringEqCI!("COLLECT"), Space),
        clause: StatsClause,
        target: (Space, CollectTarget),
        sample: Option<((Space, StringEqCI!("%SAMPLE_PERCENT"), Space), PercentValue)>,
    },
    CreateBitmapextentIndex {
        kw: (
            StringEqCI!("CREATE"),
            Space,
            StringEqCI!("BITMAPEXTENT"),
            Space,
            StringEqCI!("INDEX"),
            Space,
        ),
        name: IndexName,
        on_kw: (Space, StringEqCI!("ON"), Space),
        table_kw: Option<(StringEqCI!("TABLE"), Space)>,
        tname: TName,
    },
    CreateDatabase {
        kw: (StringEqCI!("CREATE"), Space, StringEqCI!("DATABASE"), Space),
        file: Option<(StringEqCI!("FILE"), Space)>,
        name: DatabaseName,
        on_dir: Option<(
            (Space, StringEqCI!("ON"), Space, StringEqCI!("DIRECTORY")),
            Option<(Space, StringEq!("="))>,
            (Space, DirectoryName),
        )>,
        with_opts: Option<(
            (Space, StringEqCI!("WITH"), Space),
            DBOption,
            Vec<(Space, DBOption)>,
        )>,
    },
    CreateForeignTable {
        kw: (
            StringEqCI!("CREATE"),
            Space,
            StringEqCI!("FOREIGN"),
            Space,
            StringEqCI!("TABLE"),
            Space,
        ),
        if_not_exists: Option<(
            StringEqCI!("IF"),
            Space,
            StringEqCI!("NOT"),
            Space,
            StringEqCI!("EXISTS"),
            Space,
        )>,
        tname: TName,
        columns: Option<(
            (Space, StringEq!("("), Space),
            VecSep<(Ident, (Space, Datyp)), (Space, StringEq!(","), Space)>,
            (Space, StringEq!(")")),
        )>,
        server: (Space, StringEqCI!("SERVER"), Space),
        server_name: TName,
        ft_options: Option<(Space, FTOptions)>,
        values: Option<(
            (Space, StringEqCI!("VALUES"), Space, StringEq!("("), Space),
            VecSep<Ident, (Space, StringEq!(","), Space)>,
            (Space, StringEq!(")")),
        )>,
        using_json: Option<((Space, StringEqCI!("USING"), Space), JSONString)>,
        novalidate: Option<(Space, StringEqCI!("NOVALIDATE"))>,
    },
    CreateModel {
        kw: (StringEqCI!("CREATE"), Space, StringEqCI!("MODEL"), Space),
        if_not_exists: Option<(
            StringEqCI!("IF"),
            Space,
            StringEqCI!("NOT"),
            Space,
            StringEqCI!("EXISTS"),
            Space,
        )>,
        name: ModelName,
        predicting: (
            Space,
            StringEqCI!("PREDICTING"),
            Space,
            StringEq!("("),
            Space,
        ),
        pred_col: Ident,
        pred_datyp: Option<(Space, Datyp)>,
        pred_close: (Space, StringEq!(")")),
        with_clause: Option<(Space, WithClauseCreate)>,
        source: Option<(Space, ModelSource)>,
        using_clause: Option<(Space, UsingClause)>,
    },
    CreateRole {
        kw: (StringEqCI!("CREATE"), Space, StringEqCI!("ROLE"), Space),
        name: Ident,
    },
    CreateSchema {
        kw: (StringEqCI!("CREATE"), Space, StringEqCI!("SCHEMA"), Space),
        if_not_exists: Option<(
            StringEqCI!("IF"),
            Space,
            StringEqCI!("NOT"),
            Space,
            StringEqCI!("EXISTS"),
            Space,
        )>,
        name: Ident,
    },
    CreateTableAs {
        kw: (StringEqCI!("CREATE"), Space, StringEqCI!("TABLE"), Space),
        if_not_exists: Option<(
            StringEqCI!("IF"),
            Space,
            StringEqCI!("NOT"),
            Space,
            StringEqCI!("EXISTS"),
            Space,
        )>,
        nojourn: Option<(StringEqCI!("%NOJOURN"), Space)>,
        tname: TName,
        as_kw: (Space, StringEqCI!("AS"), Space),
        with_clause: Option<(WithClause, Space)>,
        query: Query,
        options: Option<(Space, TableOptionList)>,
    },
    CreateUser {
        kw: (StringEqCI!("CREATE"), Space, StringEqCI!("USER"), Space),
        name: Ident,
        password: Option<(Space, PasswordClause)>,
        default: Option<(
            (Space, StringEqCI!("DEFAULT"), Space),
            Ident,
            (Space, Ident),
        )>,
        temporary: Option<(
            (Space, StringEqCI!("TEMPORARY"), Space),
            Ident,
            (Space, Ident),
        )>,
    },
    CreateServer {
        kw: (StringEqCI!("CREATE"), Space),
        foreign: Option<(StringEqCI!("FOREIGN"), Space)>,
        server_kw: (StringEqCI!("SERVER"), Space),
        tname: TName,
        type_clause: Option<((Space, StringEqCI!("TYPE"), Space), ServerType)>,
        fdw: (
            Space,
            StringEqCI!("FOREIGN"),
            Space,
            StringEqCI!("DATA"),
            Space,
            StringEqCI!("WRAPPER"),
            Space,
        ),
        fdw_name: FDWName,
        connection: (Space, CreateServerConnection),
        delimited: Option<(Space, CreateServerDelimitedIds)>,
        passthrough: Option<(Space, CreateServerPassthrough)>,
        pushdown: Option<(Space, FTPushdownClause)>,
        using_json: Option<((Space, StringEqCI!("USING"), Space), JSONString)>,
    },
    CreateIndex {
        kw: (StringEqCI!("CREATE"), Space),
        partitioned: Option<(CreateIndexPartitioned, Space)>,
        qualifiers: Option<(IndexQualifiers, Space)>,
        index_kw: (StringEqCI!("INDEX"), Space),
        name: IndexName,
        on_kw: (Space, StringEqCI!("ON"), Space),
        table_kw: Option<(StringEqCI!("TABLE"), Space)>,
        tname: TName,
        components: (Space, StringEq!("("), Space),
        comp_list: VecSep<ComponentSpec, (Space, StringEq!(","), Space)>,
        comp_close: (Space, StringEq!(")")),
        as_clause: Option<(
            (Space, StringEqCI!("AS"), Space),
            IndexClassName,
            Option<(Space, CreateIndexAsParams)>,
        )>,
        with_clause: Option<(Space, CreateIndexWithClause)>,
        in_on: Option<(Space, InOn, (Space, Token))>,
        defer_immediate: Option<(Space, CreateIndexDeferImmediate)>,
    },
    CreateAggregate {
        kw: (StringEqCI!("CREATE"), Space),
        or_replace: Option<(StringEqCI!("OR"), Space, StringEqCI!("REPLACE"), Space)>,
        agg_kw: (StringEqCI!("AGGREGATE"), Space),
        name: QualifiedSPName,
        open: (Space, StringEq!("("), Space),
        params: Option<ParameterList>,
        close: (Space, StringEq!(")")),
        returns: Option<((Space, StringEqCI!("RETURNS"), Space), Datyp)>,
        initialize: Option<(
            (
                Space,
                StringEqCI!("INITIALIZE"),
                Space,
                StringEqCI!("WITH"),
                Space,
            ),
            QualifiedSPName,
        )>,
        iterate: (
            Space,
            StringEqCI!("ITERATE"),
            Space,
            StringEqCI!("WITH"),
            Space,
        ),
        iterate_name: QualifiedSPName,
        merge: Option<(
            (
                Space,
                StringEqCI!("MERGE"),
                Space,
                StringEqCI!("WITH"),
                Space,
            ),
            QualifiedSPName,
        )>,
        finalize: Option<(
            (
                Space,
                StringEqCI!("FINALIZE"),
                Space,
                StringEqCI!("WITH"),
                Space,
            ),
            QualifiedSPName,
        )>,
    },
    CreateFunction {
        kw: (StringEqCI!("CREATE"), Space),
        or_replace: Option<(StringEqCI!("OR"), Space, StringEqCI!("REPLACE"), Space)>,
        func_kw: (StringEqCI!("FUNCTION"), Space),
        name: QualifiedSPName,
        spec: (Space, CodeSpecification),
    },
    CreateMlConfiguration {
        kw: (StringEqCI!("CREATE"), Space),
        or_replace: Option<(StringEqCI!("OR"), Space, StringEqCI!("REPLACE"), Space)>,
        ml_kw: (
            StringEqCI!("ML"),
            Space,
            StringEqCI!("CONFIGURATION"),
            Space,
        ),
        name: MLConfigurationName,
        provider: (Space, StringEqCI!("PROVIDER"), Space),
        provider_name: ProviderName,
        desc: Option<((Space, StringEqCI!("%DESCRIPTION"), Space), MlconfigDesc)>,
        using_clause: Option<(Space, MlconfigUsingClause)>,
        settings: Option<(Space, ProviderSpecificSettings)>,
    },
    CreateProcedure {
        kw: (StringEqCI!("CREATE"), Space),
        or_replace: Option<(StringEqCI!("OR"), Space, StringEqCI!("REPLACE"), Space)>,
        proc_kw: (StringEqCI!("PROC"), Option<StringEqCI!("EDURE")>, Space),
        name: QualifiedSPName,
        spec: (Space, CodeSpecification),
    },
    CreateTrigger {
        kw: (StringEqCI!("CREATE"), Space),
        or_replace: Option<(StringEqCI!("OR"), Space, StringEqCI!("REPLACE"), Space)>,
        trigger_kw: (StringEqCI!("TRIGGER"), Space),
        name: TName,
        time: (Space, TriggerTime, Space),
        events: VecSep<TriggerEvent, (Space, StringEq!(","), Space)>,
        order: Option<(Space, StringEqCI!("ORDER"), Space, NonNegativeInteger)>,
        on: (Space, StringEqCI!("ON"), Space, TName),
        referencing: Option<(
            Space,
            StringEqCI!("REFERENCING"),
            Vec1<(Space, TriggerReferencing)>,
        )>,
        for_each: Option<(
            Space,
            StringEqCI!("FOR"),
            Space,
            StringEqCI!("EACH"),
            Space,
            TriggerOrientation,
        )>,
        when: Option<(Space, StringEqCI!("WHEN"), Space, Parenthesized<Prdct>)>,
        body: (Space, TriggerBody),
    },
    DropTrigger {
        kw: (StringEqCI!("DROP"), Space, StringEqCI!("TRIGGER"), Space),
        if_exists: Option<(StringEqCI!("IF"), Space, StringEqCI!("EXISTS"), Space)>,
        name: TName,
        from: Option<((Space, StringEqCI!("FROM"), Space), TName)>,
    },
    CreateQuery {
        kw: (StringEqCI!("CREATE"), Space),
        or_replace: Option<(StringEqCI!("OR"), Space, StringEqCI!("REPLACE"), Space)>,
        query_kw: (StringEqCI!("QUERY"), Space),
        name: QualifiedSPName,
        spec: (Space, CodeSpecification),
    },
    CreateMethod {
        kw: (StringEqCI!("CREATE"), Space),
        static_kw: Option<(StringEqCI!("STATIC"), Space)>,
        method_kw: (StringEqCI!("METHOD"), Space),
        name: QualifiedSPName,
        spec: (Space, CodeSpecification),
    },
    CreateTable {
        kw: (StringEqCI!("CREATE"), Space),
        temp: Option<(CreateTableTemp, Space)>,
        table_kw: (StringEqCI!("TABLE"), Space),
        if_not_exists: Option<(
            StringEqCI!("IF"),
            Space,
            StringEqCI!("NOT"),
            Space,
            StringEqCI!("EXISTS"),
            Space,
        )>,
        tname: TName,
        open: (Space, StringEq!("("), Space),
        elems: VecSep<Telem, (Space, StringEq!(","), Space)>,
        close: (Space, StringEq!(")")),
        options: Option<(Space, TableOptionList)>,
    },
    DropDatabase {
        kw: (StringEqCI!("DROP"), Space, StringEqCI!("DATABASE"), Space),
        if_exists: Option<(StringEqCI!("IF"), Space, StringEqCI!("EXISTS"), Space)>,
        name: DatabaseName,
        retain: Option<(Space, StringEqCI!("RETAIN_FILES"))>,
    },
    DropForeignTable {
        kw: (
            StringEqCI!("DROP"),
            Space,
            StringEqCI!("FOREIGN"),
            Space,
            StringEqCI!("TABLE"),
            Space,
        ),
        if_exists: Option<(StringEqCI!("IF"), Space, StringEqCI!("EXISTS"), Space)>,
        tname: TName,
        cascade: (Space, CascadeOpt),
    },
    DropFunction {
        kw: (StringEqCI!("DROP"), Space, StringEqCI!("FUNCTION"), Space),
        if_exists: Option<(StringEqCI!("IF"), Space, StringEqCI!("EXISTS"), Space)>,
        name: QualifiedSPName,
        from: Option<((Space, StringEqCI!("FROM"), Space), ClassName)>,
    },
    DropIndexDot {
        kw: (StringEqCI!("DROP"), Space, StringEqCI!("INDEX"), Space),
        if_exists: Option<(StringEqCI!("IF"), Space, StringEqCI!("EXISTS"), Space)>,
        nojourn: Option<(StringEqCI!("%NOJOURN"), Space)>,
        tname: Ident,
        name: Vec1<((Space, StringEq!("."), Space), Ident)>,
    },
    DropIndexOn {
        kw: (StringEqCI!("DROP"), Space, StringEqCI!("INDEX"), Space),
        if_exists: Option<(StringEqCI!("IF"), Space, StringEqCI!("EXISTS"), Space)>,
        name: IndexName,
        on: Option<(
            (Space, StringEqCI!("ON"), Space),
            Option<(StringEqCI!("TABLE"), Space)>,
            TName,
        )>,
    },
    DropMethod {
        kw: (StringEqCI!("DROP"), Space, StringEqCI!("METHOD"), Space),
        if_exists: Option<(StringEqCI!("IF"), Space, StringEqCI!("EXISTS"), Space)>,
        name: QualifiedSPName,
        from: Option<((Space, StringEqCI!("FROM"), Space), ClassName)>,
    },
    DropMlConfiguration {
        kw: (
            StringEqCI!("DROP"),
            Space,
            StringEqCI!("ML"),
            Space,
            StringEqCI!("CONFIGURATION"),
            Space,
        ),
        if_exists: Option<(StringEqCI!("IF"), Space, StringEqCI!("EXISTS"), Space)>,
        name: MLConfigurationName,
    },
    DropModel {
        kw: (StringEqCI!("DROP"), Space, StringEqCI!("MODEL"), Space),
        if_exists: Option<(StringEqCI!("IF"), Space, StringEqCI!("EXISTS"), Space)>,
        name: ModelName,
    },
    DropProcedure {
        kw: (
            StringEqCI!("DROP"),
            Space,
            StringEqCI!("PROC"),
            Option<StringEqCI!("EDURE")>,
        ),
        sp: Space,
        if_exists: Option<(StringEqCI!("IF"), Space, StringEqCI!("EXISTS"), Space)>,
        name: QualifiedSPName,
        from: Option<((Space, StringEqCI!("FROM"), Space), ClassName)>,
    },
    DropQuery {
        kw: (StringEqCI!("DROP"), Space, StringEqCI!("QUERY"), Space),
        if_exists: Option<(StringEqCI!("IF"), Space, StringEqCI!("EXISTS"), Space)>,
        name: QualifiedSPName,
        from: Option<((Space, StringEqCI!("FROM"), Space), ClassName)>,
    },
    DropRole {
        kw: (StringEqCI!("DROP"), Space, StringEqCI!("ROLE"), Space),
        if_exists: Option<(StringEqCI!("IF"), Space, StringEqCI!("EXISTS"), Space)>,
        name: Ident,
    },
    DropSchema {
        kw: (StringEqCI!("DROP"), Space, StringEqCI!("SCHEMA"), Space),
        if_exists: Option<(StringEqCI!("IF"), Space, StringEqCI!("EXISTS"), Space)>,
        name: Ident,
        cascade_restrict: Option<(Space, DropSchemaCascadeRestrict)>,
    },
    DropTable {
        kw: (StringEqCI!("DROP"), Space, StringEqCI!("TABLE"), Space),
        if_exists: Option<(StringEqCI!("IF"), Space, StringEqCI!("EXISTS"), Space)>,
        nojourn: Option<(StringEqCI!("%NOJOURN"), Space)>,
        tname: TName,
        cascade: Option<(Space, CascadeOpt)>,
        data: Option<(Space, DataOption)>,
    },
    DropUser {
        kw: (StringEqCI!("DROP"), Space, StringEqCI!("USER"), Space),
        if_exists: Option<(StringEqCI!("IF"), Space, StringEqCI!("EXISTS"), Space)>,
        name: Ident,
        cascade: Option<(Space, StringEqCI!("CASCADE"))>,
    },
    DropView {
        kw: (StringEqCI!("DROP"), Space, StringEqCI!("VIEW"), Space),
        if_exists: Option<(StringEqCI!("IF"), Space, StringEqCI!("EXISTS"), Space)>,
        tname: TName,
        cascade: Option<(Space, CascadeOpt)>,
    },
    DropServer {
        kw: (StringEqCI!("DROP"), Space),
        foreign: Option<(StringEqCI!("FOREIGN"), Space)>,
        server_kw: (StringEqCI!("SERVER"), Space),
        tname: TName,
        cascade: (Space, CascadeOpt),
    },
    DropAggregate {
        kw: (StringEqCI!("DROP"), Space),
        if_exists: Option<(StringEqCI!("IF"), Space, StringEqCI!("EXISTS"), Space)>,
        agg_kw: (StringEqCI!("AGGREGATE"), Space),
        name: QualifiedSPName,
    },
    FreezePlans {
        kw: StringEqCI!("FREEZE"),
        plans: (Space, StringEqCI!("PLANS")),
        by: Option<((Space, StringEqCI!("BY"), Space), FreezeScope)>,
    },
    Load {
        kw: (StringEqCI!("LOAD"), Space),
        spec: LoadSpec,
    },
    PurgeQueries {
        kw: (StringEqCI!("PURGE"), Space),
        cached: Option<(StringEqCI!("CACHED"), Space)>,
        queries: StringEqCI!("QUERIES"),
        by: Option<((Space, StringEqCI!("BY"), Space), PurgeScope)>,
    },
    TrainModel {
        kw: (StringEqCI!("TRAIN"), Space, StringEqCI!("MODEL"), Space),
        name: ModelName,
        as_name: Option<((Space, StringEqCI!("AS"), Space), ModelName)>,
        not_default: Option<(Space, StringEqCI!("NOT"), Space, StringEqCI!("DEFAULT"))>,
        for_col: Option<((Space, StringEqCI!("FOR"), Space), ModelColumn)>,
        with_clause: Option<(Space, WithClauseTrain)>,
        source: Option<(Space, ModelSource)>,
        using_clause: Option<(Space, UsingClause)>,
    },
    TuneTable {
        kw: (StringEqCI!("TUNE"), Space, StringEqCI!("TABLE"), Space),
        tname: TName,
        options: Vec<(Space, TuneOption)>,
    },
    UnfreezePlans {
        kw: StringEqCI!("UNFREEZE"),
        plans: (Space, StringEqCI!("PLANS")),
        from_upgrade: Option<(
            Space,
            Option<(StringEqCI!("FROM"), Space)>,
            StringEqCI!("UPGRADE"),
        )>,
        by: Option<((Space, StringEqCI!("BY"), Space), FreezeScope)>,
    },
    UseDatabase {
        kw: (StringEqCI!("USE"), Space),
        database: Option<(StringEqCI!("DATABASE"), Space)>,
        name: DatabaseName,
    },
    ValidateModel {
        kw: (StringEqCI!("VALIDATE"), Space, StringEqCI!("MODEL"), Space),
        name: ModelName,
        as_name: Option<((Space, StringEqCI!("AS"), Space), ModelName)>,
        use_name: Option<((Space, StringEqCI!("USE"), Space), ModelName)>,
        with_clause: Option<(Space, WithClauseTrain)>,
        source: Option<(Space, ModelSource)>,
        using_clause: Option<(Space, UsingClause)>,
    },
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct CreateTableTemp {
    private_global: PrivateGlobal,
    space: Space,
    temporary: StringEqCI!("TEMPORARY"),
}

#[derive(Grammar)]
#[grammar(name = "PRIVATE/GLOBAL")]
pub enum PrivateGlobal {
    Global(StringEqCI!("GLOBAL")),
    Private(StringEqCI!("PRIVATE")),
}

#[derive(Grammar)]
#[grammar(inline, name = "drop schema cascade restrict")]
pub enum DropSchemaCascadeRestrict {
    Cascade(StringEqCI!("CASCADE")),
    Restrict(StringEqCI!("RESTRICT")),
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum CreateIndexPartitioned {
    NonPartitioned(StringEqCI!("NONPARTITIONED")),
    Partitioned(StringEqCI!("PARTITIONED")),
}

#[derive(Grammar)]
#[grammar(name = "create server connection")]
pub enum CreateServerConnection {
    Connection((StringEqCI!("CONNECTION"), Space), Literal),
    Host {
        kw: (StringEqCI!("HOST"), Space),
        host: Literal,
        open: (Space, StringEq!("("), Space),
        options: DBServerOptions,
        close: (Space, StringEq!(")")),
    },
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum CreateServerDelimitedIds {
    Delimited(StringEqCI!("DELIMITEDIDS")),
    NoDelimited(StringEqCI!("NODELIMITEDIDS")),
}

pub type CreateServerPassthrough = Suffix<EnableDisable, (Space, StringEqCI!("PASSTHROUGH"))>;

#[derive(Grammar)]
#[grammar(name = "ENABLE/DISABLE")]
pub enum EnableDisable {
    Enable(StringEqCI!("ENABLE")),
    Disable(StringEqCI!("DISABLE")),
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum CreateIndexAsParams {
    Parens {
        open: (StringEq!("("), Space),
        params: VecSep<IndexParameterSpec, (Space, StringEq!(","), Space)>,
        close: (Space, StringEq!(")")),
    },
    With {
        kw: (StringEqCI!("WITH"), Space),
        params: VecSep<IndexParameterSpec, (Space, StringEq!(","), Space)>,
    },
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct CreateIndexWithClause {
    kw: (StringEqCI!("WITH"), Space),
    data: Option<(
        (StringEqCI!("DATA"), Space, StringEq!("("), Space),
        VecSep<Ident, (Space, StringEq!(","), Space)>,
        (Space, StringEq!(")")),
    )>,
    pad_index: Option<(
        Option<(Space, StringEq!(","), Space)>,
        StringEqCI!("PAD_INDEX"),
    )>,
    fillfactor: Option<(
        Option<(Space, StringEq!(","), Space)>,
        StringEqCI!("FILLFACTOR"),
        (Space, StringEq!("="), Space),
        Literal,
    )>,
    ignore_dup: Option<(
        Option<(Space, StringEq!(","), Space)>,
        StringEqCI!("IGNORE_DUP_KEY"),
    )>,
    drop_existing: Option<(
        Option<(Space, StringEq!(","), Space)>,
        StringEqCI!("DROP_EXISTING"),
    )>,
    stats_norecompute: Option<(
        Option<(Space, StringEq!(","), Space)>,
        StringEqCI!("STATISTICS_NORECOMPUTE"),
    )>,
}

pub type CreateIndexDeferImmediate = Suffix<ImmediateDefer, Option<(Space, StringEqCI!("BUILD"))>>;

#[derive(Grammar)]
#[grammar(name = "IMMEDIATE/DEFER")]
pub enum ImmediateDefer {
    Immediate(StringEqCI!("IMMEDIATE")),
    Defer(StringEqCI!("DEFER")),
}

#[derive(Grammar)]
#[grammar(name = "alter-ft-action")]
pub struct AlterFTAction(Ident);

#[derive(Grammar)]
#[grammar(name = "model-name")]
pub struct ModelName(Ident);

pub type PasswordClause = Prefix<PWPrefix, Password>;

#[derive(Grammar)]
pub enum Password {
    Ident(Ident),
    Literal(Literal),
    NULL(StringEqCI!("NULL")),
}

#[derive(Grammar)]
pub enum PWPrefix {
    IdentifiedBy(StringEqCI!("IDENTIFIED"), Space, StringEqCI!("BY")),
    IdentifyBy(StringEqCI!("IDENTIFY"), Space, StringEqCI!("BY")),
    WithPassword(
        Option<(StringEqCI!("WITH"), Space)>,
        StringEqCI!("PASSWORD"),
    ),
}

#[derive(Grammar)]
#[grammar(name = "build scope")]
pub enum BuildScope {
    All(StringEqCI!("ALL")),
    Schema((StringEqCI!("SCHEMA"), Space), Ident),
    Table {
        table_kw: (StringEqCI!("TABLE"), Space),
        first: Ident,
        rest: Vec<((Space, StringEq!("."), Space), Ident)>,
        index: Option<(
            (Space, StringEqCI!("INDEX"), Space),
            VecSep<Ident, (Space, StringEq!(","), Space)>,
        )>,
    },
}

#[derive(Grammar)]
#[grammar(name = "build option")]
pub enum BuildOption {
    NoJourn(StringEqCI!("%NOJOURN")),
    NoLock(StringEqCI!("%NOLOCK")),
}

#[derive(Grammar)]
#[grammar(name = "purge scope")]
pub enum PurgeScope {
    Age((StringEqCI!("AGE"), Space), Literal),
    Name {
        name_kw: (StringEqCI!("NAME"), Space),
        first: VecSep<Ident, (Space, StringEq!("."), Space)>,
        rest: Vec<(
            (Space, StringEq!(","), Space),
            VecSep<Ident, (Space, StringEq!("."), Space)>,
        )>,
    },
    Table((StringEqCI!("TABLE"), Space), TName),
}

#[derive(Grammar)]
#[grammar(name = "freeze scope")]
pub enum FreezeScope {
    Id((StringEqCI!("ID"), Space), Literal),
    Schema((StringEqCI!("SCHEMA"), Space), Schema),
    Table((StringEqCI!("TABLE"), Space), TName),
}

pub type CancelQueryScope =
    Prefix<(StringEqCI!("IDENTIFIED"), Space, StringEqCI!("BY"), Space), ValSpec>;

pub type TimeoutScope = Prefix<(StringEqCI!("TIMEOUT"), Space), ValSpec>;

pub type WithClauseCreate = Wrap<
    (StringEqCI!("WITH"), Space, StringEq!("("), Space),
    VecSep<ModelColumn, (Space, StringEq!(","), Space)>,
    (Space, StringEq!(")")),
>;

#[derive(Grammar)]
#[grammar(name = "model-column")]
pub struct ModelColumn {
    name: Ident,
    datyp: (Space, Datyp),
}

pub type ModelSource = Prefix<(StringEqCI!("FROM"), Space), ModelSourceQuery>;

#[derive(Grammar)]
#[grammar(name = "model-source-query")]
pub enum ModelSourceQuery {
    Subquery {
        open: (StringEq!("("), Space),
        selex: Selex,
        close: (Space, StringEq!(")")),
    },
    Inline {
        from: FromClause,
        where_clause: Option<((Space, StringEqCI!("WHERE"), Space), Prdct)>,
        group: Option<(Space, Group)>,
        having: Option<(Space, Having)>,
        order: Option<(Space, Order)>,
    },
}

pub type UsingClause = Prefix<(StringEqCI!("USING"), Space), AlgDescription>;

#[derive(Grammar)]
#[grammar(name = "alg description")]
pub struct AlgDescription(JSONString);

pub type WithClauseTrain = Prefix<(StringEqCI!("WITH"), Space), TrainWith>;

#[derive(Grammar)]
#[grammar(name = "train-with")]
pub enum TrainWith {
    List {
        open: (StringEq!("("), Space),
        idents: VecSep<Ident, (Space, StringEq!(","), Space)>,
        close: (Space, StringEq!(")")),
    },
    Assignment(WithValueAssignment),
}

#[derive(Grammar)]
#[grammar(name = "with-value-assignment")]
pub struct WithValueAssignment {
    first_name: Ident,
    first_eq: (Space, StringEq!("="), Space),
    first_val: WithExpression,
    rest: Vec<(
        (Space, StringEq!(","), Space),
        Ident,
        (Space, StringEq!("="), Space),
        WithExpression,
    )>,
}

#[derive(Grammar)]
#[grammar(name = "with-expression")]
pub enum WithExpression {
    Ident(Ident),
    Literal(Literal),
    Scalx(Scalx),
}

#[derive(Grammar)]
#[grammar(name = "alter-model-action")]
pub enum AlterModelAction {
    Default(DefaultTrainedModel),
    Purge(StringEqCI!("PURGE"), Option<(Space, Scope)>),
}

#[derive(Grammar)]
#[grammar(name = "DefaultTrainedModel")]
pub struct DefaultTrainedModel {
    kw: StringEqCI!("DEFAULT"),
    trained_model: Option<(Space, StringEqCI!("TRAINED"), Space, StringEqCI!("MODEL"))>,
    name: (Space, TrainedModelName),
}

#[derive(Grammar)]
#[grammar(name = "scope")]
pub enum Scope {
    Days(Integer, (Space, StringEqCI!("DAYS"))),
    All(StringEqCI!("ALL")),
}

#[derive(Grammar)]
#[grammar(name = "provider_name")]
pub enum ProviderName {
    AutoML(StringEqCI!("AutoML")),
    DataRobot(StringEqCI!("DataRobot")),
    H2O(StringEqCI!("H2O")),
    PMMML(StringEqCI!("PMMML")),
}

#[derive(Grammar)]
#[grammar(name = "mlconfig-desc")]
pub enum MlconfigDesc {
    Literal(Literal),
    Null(StringEqCI!("NULL")),
}

pub type MlconfigUsingClause = Prefix<(StringEqCI!("USING"), Space), MlconfigUsing>;

#[derive(Grammar)]
#[grammar(name = "mlconfig-using")]
pub enum MlconfigUsing {
    JSONString(JSONString),
    Null(StringEqCI!("NULL")),
}

#[derive(Grammar)]
#[grammar(name = "provider_specific_settings")]
pub enum ProviderSpecificSettings {
    DR(DROptions),
    H2O(H2OOptions),
}

#[derive(Grammar)]
#[grammar(name = "DR options")]
pub struct DROptions(VecSep<DROption, (Space, StringEq!(","), Space)>);

#[derive(Grammar)]
#[grammar(name = "H2O options")]
pub struct H2OOptions(VecSep<H2OOption, (Space, StringEq!(","), Space)>);

#[derive(Grammar)]
#[grammar(name = "url_string")]
pub enum UrlString {
    Literal(Literal),
    Null(StringEqCI!("NULL")),
}

#[derive(Grammar)]
#[grammar(name = "token_string")]
pub enum TokenString {
    Literal(Literal),
    Null(StringEqCI!("NULL")),
}

#[derive(Grammar)]
#[grammar(name = "tune option")]
pub enum TuneOption {
    ClearValues(StringEqCI!("%CLEAR_VALUES")),
    KeepUpToDate(StringEqCI!("%KEEP_UP_TO_DATE")),
    RecompileCQ(StringEqCI!("%RECOMPILE_CQ")),
    SamplePercent(StringEqCI!("%SAMPLE_PERCENT"), (Space, PercentValue)),
}

char_class!(pub IsDbIdentifierStart, "db_identifier_start", |ch| {
    ch.is_ascii_alphabetic() || ch == '_'
});

char_class!(pub IsDbIdentifierPart, "db_identifier_part", |ch| {
    ch == '-' || ch.is_ascii_digit() || ch.is_ascii_alphabetic() || ch == '_'
});

#[derive(Grammar)]
#[grammar(name = "DR option")]
pub enum DROption {
    ApiToken(
        StringEqCI!("APITOKEN"),
        Option<(Space, StringEq!("="))>,
        (Space, TokenString),
    ),
    Url(
        StringEqCI!("URL"),
        Option<(Space, StringEq!("="))>,
        (Space, UrlString),
    ),
}

#[derive(Grammar)]
#[grammar(name = "H2O option")]
pub struct H2OOption {
    kw: StringEqCI!("JAVATLSCONFIGFILE"),
    eq: Option<(Space, StringEq!("="))>,
    val: (Space, TokenString),
}

#[derive(Grammar)]
#[grammar(name = "collect target")]
pub enum CollectTarget {
    Schema {
        for_kw: StringEqCI!("FOR"),
        schema_kw: (Space, StringEqCI!("SCHEMA"), Space),
        schema: Schema,
    },
    Table {
        for_kw: StringEqCI!("FOR"),
        table_kw: (Space, StringEqCI!("TABLE"), Space),
        table: TName,
    },
}

#[derive(Grammar)]
#[grammar(name = "db_option")]
pub struct DBOption(Ident);

#[derive(Grammar)]
#[grammar(name = "ft-options")]
pub struct FTOptions(Ident);

#[derive(Grammar)]
#[grammar(name = "json-string")]
pub struct JSONString(StringLiteral);

#[derive(Grammar)]
#[grammar(name = "server-type")]
pub enum ServerType {
    DB(StringEqCI!("'DB'")),
    FILE(StringEqCI!("'FILE'")),
}

#[derive(Grammar)]
#[grammar(name = "fdw-name")]
pub struct FDWName(Ident);

#[derive(Grammar)]
#[grammar(name = "db-server-options")]
pub struct DBServerOptions(Ident);

pub type FTPushdownClause = Prefix<StringEqCI!("PUSHDOWN"), (Space, FTPushdownType)>;

#[derive(Grammar)]
#[grammar(name = "ft-pushdown-type")]
pub enum FTPushdownType {
    Ansi92(StringEqCI!("ANSI92")),
    Extended(StringEqCI!("EXTENDED")),
    Iris(StringEqCI!("IRIS")),
    Off(StringEqCI!("OFF")),
}

#[derive(Grammar)]
#[grammar(name = "index classname")]
pub struct IndexClassName(Ident);

#[derive(Grammar)]
#[grammar(name = "qualified SPname")]
pub struct QualifiedSPName(DottedIdent);

#[derive(Grammar)]
#[grammar(name = "classname")]
pub struct ClassName(Ident);

#[derive(Grammar)]
#[grammar(name = "trigger time")]
pub enum TriggerTime {
    Before(StringEqCI!("BEFORE")),
    After(StringEqCI!("AFTER")),
}

#[derive(Grammar)]
#[grammar(name = "trigger event")]
pub enum TriggerEvent {
    Insert(StringEqCI!("INSERT")),
    Delete(StringEqCI!("DELETE")),
    Update {
        kw: StringEqCI!("UPDATE"),
        of: Option<(
            Space,
            StringEqCI!("OF"),
            Space,
            VecSep<Ident, (Space, StringEq!(","), Space)>,
        )>,
    },
}

#[derive(Grammar)]
#[grammar(name = "trigger referencing")]
pub struct TriggerReferencing {
    old_new: Either<StringEqCI!("OLD"), StringEqCI!("NEW")>,
    row: Option<(Space, StringEqCI!("ROW"))>,
    as_kw: Option<(Space, StringEqCI!("AS"))>,
    alias: (Space, Ident),
}

#[derive(Grammar)]
#[grammar(name = "trigger orientation")]
pub enum TriggerOrientation {
    RowAndObject(StringEqCI!("ROW_AND_OBJECT")),
    Row(StringEqCI!("ROW")),
    Statement(StringEqCI!("STATEMENT")),
}

#[derive(Grammar)]
#[grammar(name = "trigger body")]
pub enum TriggerBody {
    Code(CodeBody),
    Stat {
        lang: Option<(StringEqCI!("LANGUAGE"), Space, StringEqCI!("SQL"), Space)>,
        stat: Box<Stat>,
    },
}

#[derive(Grammar)]
#[grammar(name = "lock spec")]
pub enum LockSpec {
    Row(StringEqCI!("ROW")),
    Page(StringEqCI!("PAGE")),
}

#[derive(Grammar)]
#[grammar(name = "cascade opt")]
pub enum CascadeOpt {
    Restrict(StringEqCI!("RESTRICT")),
    Cascade(StringEqCI!("CASCADE")),
}

#[derive(Grammar)]
#[grammar(name = "data option")]
pub enum DataOption {
    DelData(StringEqCI!("%DELDATA")),
    NoDelData(StringEqCI!("%NODELDATA")),
}

#[derive(Grammar)]
#[grammar(name = "move spec")]
pub struct MoveSpec {
    pub partition_range: Option<(PartitionRangeSpec, Space)>,
    pub to_kw: StringEqCI!("TO"),
    pub database: (Space, DatabaseName),
    pub activate: Option<(Space, SQLActivateSpec)>,
}

#[derive(Grammar)]
#[grammar(name = "activate spec")]
pub struct SQLActivateSpec {
    pub force_or_defer: Option<(Either<StringEqCI!("FORCE"), StringEqCI!("DEFER")>, Space)>,
    pub activate_kw: StringEqCI!("ACTIVATE"),
    pub mappings_kw: Option<(Space, StringEqCI!("MAPPINGS"))>,
}

#[derive(Grammar)]
#[grammar(name = "cre or alt")]
pub enum CreOrAlt {
    Alter(StringEqCI!("ALTER")),
    CreateOrReplace(
        StringEqCI!("CREATE"),
        Space,
        StringEqCI!("OR"),
        Space,
        StringEqCI!("REPLACE"),
    ),
    Create(StringEqCI!("CREATE")),
}

pub type ViewColumnList = Suffix<FList1, Option<(Space, StringEqCI!("AS"))>>;

#[derive(Grammar)]
#[grammar(name = "view with")]
pub enum ViewWith {
    ReadOnly(StringEqCI!("READ"), Space, StringEqCI!("ONLY")),
    Check {
        check_kw: StringEqCI!("CHECK"),
        levels: Option<(Space, LevelsClause)>,
        option_kw: (Space, StringEqCI!("OPTION")),
        constraint: Option<(Space, StringEqCI!("CONSTRAINT"), Space, Ident)>,
    },
}

#[derive(Grammar)]
#[grammar(name = "levels clause")]
pub enum LevelsClause {
    Cascade(StringEqCI!("CASCADE")),
    Cascaded(StringEqCI!("CASCADED")),
    Local(StringEqCI!("LOCAL")),
}

#[derive(Grammar)]
#[grammar(name = "sql")]
pub struct SQL {
    space_before: Space,
    view_column_list: Option<(ViewColumnList, Space)>,
    stat: Stat,
    space_after: Space,
}

#[derive(Grammar)]
pub struct Thr {
    through: StringEqCI!("THROUGH"),
    sp1: Space,
    foreigh: StringEqCI!("FOREIGN"),
    sp2: Space,
    server: StringEqCI!("SERVER"),
    sp3: Space,
    tname: TName,
    remote_statement: RemoteStatement,
}

#[derive(Grammar)]
#[grammar(name = "remote-statement")]
pub struct RemoteStatement {
    with_clause: Option<(WithClause, Space)>,
    selct: Selct,
}

#[derive(Grammar)]
#[grammar(name = "stat")]
pub enum Stat {
    Selct {
        with_clause: Option<(WithClause, Space)>,
        selct: Selct,
    },
    Insert {
        with_clause: Option<(WithClause, Space)>,
        insert: Insrt,
    },
    Update {
        with_clause: Option<(WithClause, Space)>,
        update: Updat,
    },
    Delete {
        with_clause: Option<(WithClause, Space)>,
        delete: Delet,
    },
    Thr(Thr),
    Truncate(Truncate),
    DDL(DDL),
    Declare(Decl),
    OFC(OFC),
    SetOption(SQLOption),
    Grant(Grant),
    Revok(Revok),
    TpRelated(TpRelated),
    Get(Get),
    Call(Call),
    Explain(Explain),
    Setml(Setml),
    ChkPrv(ChkPrv),
}

#[derive(Grammar)]
#[grammar(name = "percent_value")]
pub enum PercentValue {
    Dot(StringEq!("."), CharOf<IsDigit>, CharOf<IsDigit>),
    Percent(CharOf<IsDigit>, CharOf<IsDigit>, StringEq!("%")),
}

#[derive(Grammar)]
#[grammar(name = "directory_name")]
pub struct DirectoryName(Literal);

char_class!(pub IsDbIdentStart, "db_identifier start", |ch| {
    ch.is_ascii_alphabetic() || ch == '_'
});

char_class!(pub IsDbIdentPart, "db_identifier part", |ch| {
    ch.is_ascii_alphabetic() || ch.is_ascii_digit() || ch == '_' || ch == '-'
});

#[derive(Grammar)]
#[grammar(name = "database_name")]
pub enum DatabaseName {
    Percent {
        prefix: StringEq!("%"),
        first: CharOf<IsDbIdentPart>,
        rest: Vec<CharOf<IsDbIdentPart>>,
    },
    Normal {
        first: CharOf<IsDbIdentStart>,
        rest: Vec<CharOf<IsDbIdentPart>>,
    },
}

#[derive(Grammar)]
#[grammar(name = "code specification")]
pub struct CodeSpecification {
    open: (StringEq!("("), Space),
    params: ParameterList,
    close: (Space, StringEq!(")")),
    characteristics: Vec<MethodQueryCharacteristics>,
    body: (Space, CodeBody),
}

#[derive(Grammar)]
#[grammar(name = "parameter list")]
pub struct ParameterList(Option<VecSep<ParameterDeclaration, ArgSep>>);

#[derive(Grammar)]
#[grammar(name = "parameter declaration")]
pub struct ParameterDeclaration {
    direction: Option<(ParameterDirection, Space)>,
    name: Ident,
    datyp: (Space, Datyp),
    default: Option<(Space, DefaultClause)>,
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum ParameterDirection {
    In(StringEqCI!("IN")),
    InOut(StringEqCI!("INOUT")),
    Out(StringEqCI!("OUT")),
}

#[derive(Grammar)]
#[grammar(name = "default clause")]
pub enum DefaultClause {
    Default {
        kw: (StringEqCI!("DEFAULT"), Space),
        value: Scalx,
    },
    Expr(Scalx),
}

#[derive(Grammar)]
#[grammar(name = "method-query characteristics")]
pub enum MethodQueryCharacteristics {
    ContainId {
        kw: (Space, StringEqCI!("CONTAINID"), Space),
        value: Integer,
    },
    Final((Space, StringEqCI!("FINAL"))),
    For {
        kw: (Space, StringEqCI!("FOR"), Space),
        classname: Classname,
    },
    Private((Space, StringEqCI!("PRIVATE"))),
    Proc {
        kw: (Space, StringEqCI!("PROC")),
        edure: Option<StringEqCI!("EDURE")>,
    },
    Results {
        kw: (Space, StringEqCI!("RESULTS"), Space),
        clause: ResultSetClause,
    },
    Returns {
        kw: (Space, StringEqCI!("RETURNS"), Space),
        datyp: Datyp,
    },
    SelectMode {
        kw: (Space, StringEqCI!("SELECTMODE"), Space),
        mode: SelectMode,
    },
    ResultSets {
        dynamic: Option<(Space, StringEqCI!("DYNAMIC"))>,
        kw: (Space, StringEqCI!("RESULT"), Space, StringEqCI!("SETS")),
        count: Option<(Space, Integer)>,
    },
}

#[derive(Grammar)]
#[grammar(name = "result set clause")]
pub struct ResultSetClause {
    open: (StringEq!("("), Space),
    first: ResultColumn,
    rest: Vec<(Space, StringEq!(","), Space, ResultColumn)>,
    close: (Space, StringEq!(")")),
}

#[derive(Grammar)]
#[grammar(name = "result column")]
pub struct ResultColumn {
    name: ColumnName,
    datyp: (Space, Datyp),
}

#[derive(Grammar)]
#[grammar(name = "select mode")]
pub enum SelectMode {
    Display(StringEqCI!("DISPLAY")),
    Logical(StringEqCI!("LOGICAL")),
    Odbc(StringEqCI!("ODBC")),
    Runtime(StringEqCI!("RUNTIME")),
}

#[derive(Grammar)]
#[grammar(name = "code body")]
pub enum CodeBody {
    Cos {
        kw: (StringEqCI!("LANGUAGE"), Space, StringEqCI!("COS"), Space),
        body: COSMethodBody,
    },
    CosQuery {
        kw: (StringEqCI!("LANGUAGE"), Space, StringEqCI!("COS"), Space),
        body: COSQueryBody,
    },
    ObjectScript {
        kw: (
            StringEqCI!("LANGUAGE"),
            Space,
            StringEqCI!("OBJECTSCRIPT"),
            Space,
        ),
        body: COSMethodBody,
    },
    ObjectScriptQuery {
        kw: (
            StringEqCI!("LANGUAGE"),
            Space,
            StringEqCI!("OBJECTSCRIPT"),
            Space,
        ),
        body: COSQueryBody,
    },
    Python {
        kw: (StringEqCI!("LANGUAGE"), Space, StringEqCI!("PYTHON"), Space),
        body: PythonMethodBody,
    },
    Sql {
        kw: Option<(StringEqCI!("LANGUAGE"), Space, StringEqCI!("SQL"), Space)>,
        body: SqlMethodBody,
    },
}

pub type SqlMethodBody = Wrap<
    StringEqCI!("BEGIN"),
    Vec<(
        Space,
        Stat,
        Space,
        StringEq!(";"),
        Option<(Space, ReturnStatement, Space, StringEq!(";"))>,
    )>,
    (Space, StringEqCI!("END")),
>;

#[derive(Grammar)]
#[grammar(name = "return statement")]
pub struct ReturnStatement {
    pub kw: StringEqCI!("RETURN"),
    pub space: Space,
    pub value: Option<ReturnValue>,
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum ReturnValue {
    HostRef(HostRef),
    Literal(Literal),
}

#[derive(Grammar)]
#[grammar(name = "COS query body")]
pub struct COSQueryBody {
    first: QueryMethod,
    rest: Vec<(Space, QueryMethod)>,
}

#[derive(Grammar)]
#[grammar(name = "query method")]
pub enum QueryMethod {
    Close {
        kw: (StringEqCI!("CLOSE"), Space),
        params: Parenthesized<ParameterList>,
        body: (Space, COSMethodBody),
    },
    Execute {
        kw: (StringEqCI!("EXECUTE"), Space),
        params: Parenthesized<ParameterList>,
        body: (Space, COSMethodBody),
    },
    Fetch {
        kw: (StringEqCI!("FETCH"), Space),
        params: Parenthesized<ParameterList>,
        body: (Space, COSMethodBody),
    },
    FetchOdbc {
        kw: (StringEqCI!("FETCHODBC"), Space),
        params: Parenthesized<ParameterList>,
        body: (Space, COSMethodBody),
    },
    FetchRows {
        kw: (StringEqCI!("FETCHROWS"), Space),
        params: Parenthesized<ParameterList>,
        body: (Space, COSMethodBody),
    },
    GetInfo {
        kw: (StringEqCI!("GETINFO"), Space),
        params: Parenthesized<ParameterList>,
        body: (Space, COSMethodBody),
    },
}

pub type COSMethodBody = crate::cos::Brace<crate::cos::OSText>;

pub type PythonMethodBody = Prefix<StringEq!("{"), StringEndsWith<StringEq!("}")>>;

#[derive(Grammar)]
pub enum Language {
    Cos(StringEqCI!("COS")),
    ObjectScript(StringEqCI!("OBJECTSCRIPT")),
    Python(StringEqCI!("PYTHON")),
    Sql(StringEqCI!("SQL")),
}

#[derive(Grammar)]
#[grammar(name = "indexname")]
pub struct IndexName(Ident);

#[derive(Grammar)]
#[grammar(name = "index qualifiers")]
pub enum IndexQualifiers {
    Bitmap(StringEqCI!("BITMAP")),
    Bitslice(StringEqCI!("BITSLICE")),
    Columnar(StringEqCI!("COLUMNAR")),
    Unique {
        kw: (StringEqCI!("UNIQUE"), Space),
        cluster: Cluster,
    },
    Cluster(Cluster),
}

#[derive(Grammar)]
#[grammar(name = "component spec")]
pub struct ComponentSpec {
    name: Ident,
    collection: Option<(Space, CollectionSpec)>,
    collate: Option<(Space, StringEqCI!("COLLATE"), Space, SqlCollation)>,
    direction: Option<(Space, ComponentDirection)>,
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum ComponentDirection {
    Asc(StringEqCI!("ASC")),
    Desc(StringEqCI!("DESC")),
}

#[derive(Grammar)]
#[grammar(name = "collection spec")]
pub enum CollectionSpec {
    Elements(
        StringEq!("("),
        Space,
        StringEqCI!("ELEMENTS"),
        Space,
        StringEq!(")"),
    ),
    Keys(
        StringEq!("("),
        Space,
        StringEqCI!("KEYS"),
        Space,
        StringEq!(")"),
    ),
}

#[derive(Grammar)]
#[grammar(name = "classname")]
pub struct Classname {
    first: Ident,
    rest: Vec<(Space, StringEq!(","), Space, Ident)>,
}

#[derive(Grammar)]
#[grammar(name = "index parameter spec")]
pub struct IndexParameterSpec {
    name: Ident,
    eq: (Space, StringEq!("="), Space),
    value: Literal,
}

#[derive(Grammar)]
#[grammar(name = "alter action")]
pub enum AlterAction {
    CheckConstraintAll {
        check: CheckSpec,
        kw: (Space, StringEqCI!("CONSTRAINT"), Space, StringEqCI!("ALL")),
    },
    DropAction {
        drop_kw: DropKeywd,
        action: (Space, DropAction),
    },
    DropPartition {
        drop_kw: DropKeywd,
        partition: (Space, PartitionSpecification),
    },
    DropAllPartitions {
        drop_kw: DropKeywd,
        kw: (Space, StringEqCI!("ALL"), Space, StringEqCI!("PARTITIONS")),
    },
    Add {
        kw: (StringEqCI!("ADD"), Space),
        actions: Either<Parenthesized<AddActionList>, AddActionList>,
    },
    Alter {
        kw: StringEqCI!("ALTER"),
        column_kw: Option<(Space, StringEqCI!("COLUMN"))>,
        name: (Space, Ident),
        alter: (Space, AlterCol),
    },
    Cleanup(StringEqCI!("CLEANUP")),
    Convert {
        kw: (StringEqCI!("CONVERT"), Space),
        clause: ConvertClause,
    },
    LockMode {
        kw: (
            StringEqCI!("LOCK"),
            Space,
            StringEqCI!("MODE"),
            Space,
            StringEq!("("),
            Space,
        ),
        spec: LockSpec,
        close: (Space, StringEq!(")")),
    },
    Modify {
        kw: (StringEqCI!("MODIFY"), Space),
        specs: Either<Parenthesized<ModSpecList>, ModSpecList>,
    },
    Move {
        kw: (StringEqCI!("MOVE"), Space),
        spec: MoveSpec,
    },
    Rename {
        kw: (StringEqCI!("RENAME"), Space),
        name: Ident,
    },
    Truncate {
        kw: (StringEqCI!("TRUNCATE"), Space),
        partition: PartitionSpecification,
    },
}

#[derive(Grammar)]
#[grammar(name = "convert clause")]
pub enum ConvertClause {
    Partition(PartitionConvertClause),
    StorageDefault {
        clause: StorageDefaultConvertClause,
        del: Option<(Space, DelDataOption)>,
    },
    Shard {
        to: Option<(StringEqCI!("TO"), Space)>,
        clause: ShardConvertClause,
        timing: Option<(Space, ConvertTiming)>,
    },
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum DelDataOption {
    DelData(StringEqCI!("%DELDATA")),
    NoDelData(StringEqCI!("%NODELDATA")),
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum ConvertTiming {
    Defer(StringEqCI!("DEFER")),
    Immediate(StringEqCI!("IMMEDIATE")),
}

#[derive(Grammar)]
#[grammar(name = "shard convert clause")]
pub enum ShardConvertClause {
    Shard(Shard),
    Sharded(Sharded),
    NonSharded(StringEqCI!("NONSHARDED")),
}

#[derive(Grammar)]
#[grammar(name = "partition convert clause")]
pub enum PartitionConvertClause {
    PartitionBy {
        kw: (StringEqCI!("PARTITION"), Space, StringEqCI!("BY"), Space),
        list: PartitionSpecificationList,
    },
    PartitionOnOff {
        kw: StringEqCI!("PARTITION"),
        value: Option<(Space, PartitionOnOff)>,
    },
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum PartitionOnOff {
    On(StringEqCI!("ON")),
    Off(StringEqCI!("OFF")),
}

#[derive(Grammar)]
#[grammar(name = "alter col")]
pub enum AlterCol {
    DatypCollate {
        datyp: Datyp,
        collate: Option<(Space, StringEqCI!("COLLATE"), Space, SqlCollation)>,
    },
    SqlCollation(SqlCollation),
    Collate {
        kw: (StringEqCI!("COLLATE"), Space),
        name: Ident,
    },
    Default {
        kw: (StringEqCI!("DEFAULT"), Space),
        value: Setdef,
    },
    DropDefault(StringEqCI!("DROP"), Space, StringEqCI!("DEFAULT")),
    NotNull(StringEqCI!("NOT"), Space, StringEqCI!("NULL")),
    Null(StringEqCI!("NULL")),
    Rename {
        kw: (StringEqCI!("RENAME"), Space),
        name: Ident,
    },
    SetDefault {
        kw: (StringEqCI!("SET"), Space, StringEqCI!("DEFAULT"), Space),
        value: Setdef,
    },
}

#[derive(Grammar)]
#[grammar(name = "storagedefault convert clause")]
pub enum StorageDefaultConvertClause {
    Column(ColumnStorageDefaultConversion),
    Type(StorageDefaultConversionType),
}

#[derive(Grammar)]
#[grammar(name = "storagedefault conversion type spec")]
pub struct StorageDefaultConversionTypeSpec {
    to: Option<(StringEqCI!("TO"), Space)>,
    kw: (StringEqCI!("STORAGETYPE"), Space, StringEq!("="), Space),
    typ: StorageDefaultConversionType,
}

#[derive(Grammar)]
#[grammar(name = "storagedefault conversion type")]
pub enum StorageDefaultConversionType {
    Columnar(StringEqCI!("COLUMNAR")),
    Row(StringEqCI!("ROW")),
}

#[derive(Grammar)]
#[grammar(name = "column storagedefault conversion")]
pub struct ColumnStorageDefaultConversion {
    kw: StringEqCI!("COLUMN"),
    s: Option<Raw<StringEqCI!("S")>>,
    first: (Space, Ident),
    rest: Vec<(Space, StringEq!(","), Space, Ident)>,
    spec: (Space, StorageDefaultConversionTypeSpec),
}

#[derive(Grammar)]
#[grammar(name = "setdef")]
pub enum Setdef {
    Paren(Parenthesized<DefSpec>),
    Bare(DefSpec),
}

#[derive(Grammar)]
#[grammar(name = "def spec")]
pub enum DefSpec {
    Cos {
        kw: (StringEqCI!("%COS"), Space),
        value: Literal,
    },
    MCode {
        kw: (StringEqCI!("%MCODE"), Space),
        value: Literal,
    },
    Negative {
        minus: (StringEq!("-"), Space),
        value: Number,
    },
    Literal(Literal),
    CurrentDate(StringEqCI!("CURRENT_DATE")),
    CurrentTime {
        kw: StringEqCI!("CURRENT_TIME"),
        precision: Option<Parenthesized<Literal>>,
    },
    CurrentTimestamp {
        kw: StringEqCI!("CURRENT_TIMESTAMP"),
        precision: Option<Parenthesized<Literal>>,
    },
    CurrentUser(StringEqCI!("CURRENT_USER")),
    GetDate {
        kw: (StringEqCI!("GETDATE"), StringEq!("("), Space),
        value: Option<Literal>,
        close: (Space, StringEq!(")")),
    },
    Null(StringEqCI!("NULL")),
    ObjectScript {
        kw: (StringEqCI!("OBJECTSCRIPT"), Space),
        value: Literal,
    },
    SessionUser(StringEqCI!("SESSION_USER")),
    SysDate(StringEqCI!("SYSDATE")),
    SystemUser(StringEqCI!("SYSTEM_USER")),
    ToDate {
        kw: (StringEqCI!("TO_DATE("), Space),
        arg1: Literal,
        arg2: Option<(Space, StringEq!(","), Space, Literal)>,
        close: (Space, StringEq!(")")),
    },
    ToTimestamp {
        kw: (StringEqCI!("TO_TIMESTAMP("), Space),
        arg1: Literal,
        arg2: Option<(Space, StringEq!(","), Space, Literal)>,
        close: (Space, StringEq!(")")),
    },
    User(StringEqCI!("USER")),
}

#[derive(Grammar)]
#[grammar(name = "mod spec")]
pub enum ModSpec {
    Rename {
        name: Ident,
        kw: (Space, StringEqCI!("RENAME"), Space),
        new_name: Ident,
    },
    Modify {
        name: Ident,
        datyp: Option<(Space, Datyp)>,
        mod_col: (Space, ModCol),
    },
}

pub type ModSpecList = VecSep<ModSpec, (Space, StringEq!(","), Space)>;

#[derive(Grammar)]
#[grammar(name = "mod col")]
pub struct ModCol {
    default: Option<(StringEqCI!("DEFAULT"), Space, Setdef, Space)>,
    constraint: Option<(StringEqCI!("CONSTRAINT"), Space, Ident, Space)>,
    null: Option<NullSpec>,
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct NullSpec {
    not: Option<(StringEqCI!("NOT"), Space)>,
    null: StringEqCI!("NULL"),
}

#[derive(Grammar)]
#[grammar(name = "drop keywd")]
pub enum DropKeywd {
    Delete(StringEqCI!("DELETE")),
    Drop(StringEqCI!("DROP")),
}

#[derive(Grammar)]
#[grammar(name = "drop action")]
pub enum DropAction {
    Constraint {
        kw: (StringEqCI!("CONSTRAINT"), Space),
        name: Ident,
    },
    ForeignKey {
        kw: (StringEqCI!("FOREIGN"), Space, StringEqCI!("KEY"), Space),
        name: Ident,
    },
    PrimaryKey(StringEqCI!("PRIMARY"), Space, StringEqCI!("KEY")),
    Column {
        column_kw: Option<(StringEqCI!("COLUMN"), Space)>,
        first: DropColumnItem,
        rest: Vec<(Space, StringEq!(","), Space, DropColumnItem)>,
    },
}

#[derive(Grammar)]
#[grammar(name = "drop column item", inline)]
pub struct DropColumnItem {
    name: Ident,
    cascade: Option<(Space, CascadeOpt)>,
    data: Option<(Space, DataOption)>,
}

#[derive(Grammar)]
#[grammar(name = "add action")]
pub struct AddAction {
    constraint: Option<(StringEqCI!("CONSTRAINT"), Space, NotInformix, Space)>,
    body: AddBody,
}

pub type AddActionList = VecSep<AddAction, (Space, StringEq!(","), Space)>;

#[derive(Grammar)]
#[grammar(name = "add body")]
pub enum AddBody {
    ForeignKey {
        spec: OptPAltFkeySpec,
        on: Option<(Space, StringEqCI!("ON"), Space, Token)>,
    },
    PrimaryKey {
        spec: OptPAltPkey,
        on: Option<(Space, StringEqCI!("ON"), Space, Token)>,
    },
    Unique {
        spec: OptPAltUnique,
        fillfactor: Option<(
            Space,
            StringEqCI!("WITH"),
            Space,
            StringEqCI!("FILLFACTOR"),
            Space,
            StringEq!("="),
            Space,
            Literal,
        )>,
        on: Option<(Space, StringEqCI!("ON"), Space, Token)>,
    },
    ColDef(OptPColdef),
    Check {
        kw: (StringEqCI!("CHECK"), Space),
        parse: Chkparse,
        informix: (Space, Informix),
    },
    Default {
        kw: (StringEqCI!("DEFAULT"), Space),
        value: Setdef,
        for_kw: (Space, StringEqCI!("FOR"), Space),
        name: Ident,
    },
}

#[derive(Grammar)]
#[grammar(name = "opt_p coldef")]
pub enum OptPColdef {
    Paren(Parenthesized<Coldef>),
    Bare(Coldef),
}

pub type Coldef = Prefix<Option<(StringEqCI!("COLUMN"), Space)>, OptPTecdef>;

#[derive(Grammar)]
#[grammar(name = "check spec")]
pub enum CheckSpec {
    Check(StringEqCI!("CHECK")),
    NoCheck(StringEqCI!("NOCHECK")),
}

#[derive(Grammar)]
#[grammar(name = "opt_p alt pkey")]
pub enum OptPAltPkey {
    Paren(Parenthesized<AltPkey>),
    Bare(AltPkey),
}

#[derive(Grammar)]
#[grammar(name = "alt pkey")]
pub struct AltPkey {
    kw: (StringEqCI!("PRIMARY"), Space, StringEqCI!("KEY"), Space),
    spec: PkeySpec,
    informix: (Space, Informix),
}

#[derive(Grammar)]
#[grammar(name = "pkey spec")]
pub enum PkeySpec {
    FList1(FList1),
    IdentFList2 { name: Ident, flist: (Space, FList2) },
}

#[derive(Grammar)]
#[grammar(name = "alt fkey spec")]
pub struct AltFkeySpec {
    foreign: StringEq!("FOREIGN"),
    sp: Space,
    key: StringEq!("KEY"),
    ident: Ident,
    flist2: FList2,
    references: StringEq!("REFERENCES"),
    ref_: Ref,
    informix: Informix,
}

#[derive(Grammar)]
#[grammar(name = "opt_p alt fkey spec")]
pub enum OptPAltFkeySpec {
    Paren {
        open: StringEq!("("),
        inner: AltFkeySpec,
        close: StringEq!(")"),
    },
    Bare(AltFkeySpec),
}

#[derive(Grammar)]
#[grammar(name = "opt_p alt unique")]
pub enum OptPAltUnique {
    Paren(Parenthesized<AltUnique>),
    Bare(AltUnique),
}

#[derive(Grammar)]
#[grammar(name = "alt unique")]
pub struct AltUnique {
    kw: (StringEqCI!("UNIQUE"), Space),
    cluster: Cluster,
    flist: (Space, FList2),
    informix: (Space, Informix),
}

#[derive(Grammar)]
#[grammar(name = "opt_p tecdef")]
pub enum OptPTecdef {
    Paren(Parenthesized<Tecdef>),
    Bare(Tecdef),
}

#[derive(Grammar)]
#[grammar(name = "in_on")]
pub enum InOn {
    In(StringEqCI!("IN")),
    On(StringEqCI!("ON")),
}

#[derive(Grammar)]
#[grammar(name = "token")]
pub struct Token(Ident);

#[derive(Grammar)]
#[grammar(name = "telem")]
pub struct Telem {
    constraint: Option<(StringEqCI!("CONSTRAINT"), Space, Ident, Space)>,
    option: TelemOption,
}

#[derive(Grammar)]
#[grammar(name = "telem option")]
pub enum TelemOption {
    Description {
        kw: (StringEqCI!("%DESCRIPTION"), Space),
        value: Literal,
    },
    File {
        kw: (StringEqCI!("%FILE"), Space),
        value: Literal,
    },
    PublicRowId(StringEqCI!("%PUBLICROWID")),
    Routine {
        kw: (StringEqCI!("%ROUTINE"), Space),
        value: Literal,
    },
    CheckCnstr(CheckCnstr),
    ClassParameter(ClassParameter),
    ExtentsizeClause(ExtentsizeClause),
    Shard(Shard),
    TblForeign(TblForeign),
    TblPrimaryKey(TblPrimaryKey),
    TblUnique(TblUnique),
    Tecdef(Tecdef),
}

#[derive(Grammar)]
#[grammar(name = "tecdef")]
pub struct Tecdef {
    name: Ident,
    typ: (Space, Tectyp),
    qualifiers: Vec<(Space, FldQualifier)>,
}

#[derive(Grammar)]
#[grammar(name = "tectyp")]
pub enum Tectyp {
    Datyp(Datyp),
    Identity {
        datyp: Option<Datyp>,
        identity: (Space, Identity),
    },
}

pub type Identity = Prefix<
    StringEqCI!("IDENTITY"),
    Option<(
        Space,
        StringEq!("("),
        Space,
        Literal,
        Space,
        StringEq!(","),
        Space,
        Literal,
        Space,
        StringEq!(")"),
    )>,
>;

pub type TblUnique = Prefix<(StringEqCI!("UNIQUE"), Space), TblUniqueSpec>;

#[derive(Grammar)]
#[grammar(name = "tbl unique spec")]
pub struct TblUniqueSpec {
    cluster: Cluster,
    flist: (Space, FList2),
    informix: (Space, Informix),
    fillfactor: Option<(
        Space,
        StringEqCI!("WITH"),
        Space,
        StringEqCI!("FILLFACTOR"),
        Space,
        StringEq!("="),
        Space,
        Literal,
    )>,
}

pub type TblPrimaryKey =
    Prefix<(StringEqCI!("PRIMARY"), Space, StringEqCI!("KEY"), Space), TblUniqueSpec>;

#[derive(Grammar)]
#[grammar(name = "tbl foreign")]
pub struct TblForeign {
    kw: (StringEqCI!("FOREIGN"), Space, StringEqCI!("KEY"), Space),
    flist: FList2,
    references_kw: (Space, StringEqCI!("REFERENCES"), Space),
    reference: Ref,
    informix: Informix,
}

#[derive(Grammar)]
#[grammar(name = "extentsize clause")]
pub enum ExtentsizeClause {
    Extentsize {
        kw: (StringEqCI!("%EXTENTSIZE"), Space),
        value: Literal,
    },
    Numrows {
        kw: (StringEqCI!("%NUMROWS"), Space),
        value: Literal,
    },
}

#[derive(Grammar)]
#[grammar(name = "class parameter")]
pub struct ClassParameter {
    kw: (StringEqCI!("%CLASSPARAMETER"), Space),
    name: Ident,
    eq: Option<(Space, StringEq!("="))>,
    value: (Space, Literal),
}

#[derive(Grammar)]
#[grammar(name = "shard")]
pub struct Shard {
    kw: StringEqCI!("SHARD"),
    key: Option<(Space, StringEqCI!("KEY"), Space, FList1)>,
    coshard: Option<(Space, CoshardClause)>,
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct CoshardClause {
    kw: StringEqCI!("COSHARD"),
    with: Option<(Space, StringEqCI!("WITH"))>,
    open: Option<(Space, StringEq!("("))>,
    name: (Space, TName),
    close: Option<(Space, StringEq!(")"))>,
}

#[derive(Grammar)]
#[grammar(name = "sharded")]
pub struct Sharded {
    kw: StringEqCI!("SHARDED"),
    key: Option<(Space, StringEqCI!("KEY"), Space, FList1)>,
    coshard: Option<(Space, CoshardClause)>,
}

#[derive(Grammar)]
#[grammar(name = "table_option_list")]
pub enum TableOptionList {
    WithOptions {
        shard: Option<(Shard, Space)>,
        partition: Option<(PartitionOption, Space)>,
        with_kw: (StringEqCI!("WITH"), Space),
        first: TableOption,
        rest: Vec<(Space, StringEq!(","), Space, TableOption)>,
    },
    InformixCompat(InformixCompatability),
}

#[derive(Grammar)]
#[grammar(name = "informix compatability")]
pub struct InformixCompatability {
    in_on: Option<(InOn, Space, Token)>,
    extent_size: Option<(
        Space,
        StringEqCI!("EXTENT"),
        Space,
        StringEqCI!("SIZE"),
        Space,
        Literal,
    )>,
    next_size: Option<(
        Space,
        StringEqCI!("NEXT"),
        Space,
        StringEqCI!("SIZE"),
        Space,
        Literal,
    )>,
    lock_mode: Option<(
        Space,
        StringEqCI!("LOCK"),
        Space,
        StringEqCI!("MODE"),
        Option<(Space, LockModeType)>,
    )>,
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum LockModeType {
    Page(StringEqCI!("PAGE")),
    Row(StringEqCI!("ROW")),
}

#[derive(Grammar)]
#[grammar(name = "table_option")]
pub enum TableOption {
    ClassParam(ClassParameter),
    StorageType {
        kw: StringEqCI!("STORAGETYPE"),
        eq: Option<(Space, StringEq!("="))>,
        value: (Space, Storagetype),
    },
}

#[derive(Grammar)]
#[grammar(name = "partition option")]
pub enum PartitionOption {
    PartitionBy {
        kw: (StringEqCI!("PARTITION"), Space, StringEqCI!("BY"), Space),
        list: PartitionSpecificationList,
    },
    PartitionOff(StringEqCI!("PARTITION"), Space, StringEqCI!("OFF")),
}

#[derive(Grammar)]
#[grammar(name = "partition specification list")]
pub struct PartitionSpecificationList {
    first: PartitionSpecification,
    second: Option<(Space, StringEq!(","), Space, PartitionSpecification)>,
}

#[derive(Grammar)]
#[grammar(name = "partition specification")]
pub enum PartitionSpecification {
    Hash {
        kw: (StringEqCI!("HASH"), Space, StringEq!("("), Space),
        column: ColumnName,
        close: (Space, StringEq!(")"), Space),
        in_kw: (StringEqCI!("IN"), Space),
        count: PositiveInteger,
        partitions_kw: (Space, StringEqCI!("PARTITIONS")),
    },
    List {
        kw: (StringEqCI!("LIST"), Space, StringEq!("("), Space),
        column: ColumnName,
        close: (Space, StringEq!(")")),
    },
    Range {
        kw: (StringEqCI!("RANGE"), Space, StringEq!("("), Space),
        column: ColumnName,
        close: (Space, StringEq!(")"), Space),
        interval_kw: (StringEqCI!("INTERVAL"), Space),
        interval: PositiveInteger,
        datetime_interval: Option<(Space, PartitionDatetimeInterval)>,
    },
}

#[derive(Grammar)]
#[grammar(name = "partition datetime interval")]
pub enum PartitionDatetimeInterval {
    Day(StringEqCI!("DAY"), Option<StringEqCI!("S")>),
    Hour(StringEqCI!("HOUR"), Option<StringEqCI!("S")>),
    Minute(StringEqCI!("MINUTE"), Option<StringEqCI!("S")>),
    Month(StringEqCI!("MONTH"), Option<StringEqCI!("S")>),
    Year(StringEqCI!("YEAR"), Option<StringEqCI!("S")>),
}

#[derive(Grammar)]
#[grammar(name = "partition range spec")]
pub enum PartitionRangeSpec {
    PartitionPrefixBetween {
        kw: (StringEqCI!("PARTITION"), Space),
        prefix: PartitionPrefix,
        between_kw: (Space, StringEqCI!("BETWEEN"), Space),
        from: PartitionFieldValue,
        and_kw: (Space, StringEqCI!("AND"), Space),
        to: PartitionFieldValue,
    },
    PartitionId {
        kw: (StringEqCI!("PARTITION"), Space, StringEqCI!("ID"), Space),
        prefix: PartitionIdPrefix,
    },
    PartitionIdBetween {
        kw: (StringEqCI!("PARTITION"), Space, StringEqCI!("ID")),
        prefix: Option<(Space, PartitionIdPrefix)>,
        between_kw: (Space, StringEqCI!("BETWEEN"), Space),
        from: PartitionId,
        and_kw: (Space, StringEqCI!("AND"), Space),
        to: PartitionId,
    },
    PartitionBetween {
        kw: StringEqCI!("PARTITION"),
        prefix: Option<(Space, PartitionPrefix)>,
        between_kw: (Space, StringEqCI!("BETWEEN"), Space),
        from: PartitionFieldValue,
        and_kw: (Space, StringEqCI!("AND"), Space),
        to: PartitionFieldValue,
    },
}

#[derive(Grammar)]
#[grammar(name = "partition id prefix")]
pub struct PartitionIdPrefix(Literal);

#[derive(Grammar)]
#[grammar(name = "partition prefix")]
pub struct PartitionPrefix {
    open: (StringEq!("("), Space),
    first: PartitionFieldValue,
    rest: Vec<(Space, StringEq!(","), Space, PartitionFieldValue)>,
    close: (Space, StringEq!(")")),
}

#[derive(Grammar)]
#[grammar(name = "partition id")]
pub struct PartitionId(Literal);

#[derive(Grammar)]
#[grammar(name = "partition field value")]
pub struct PartitionFieldValue(Literal);

#[derive(Grammar)]
#[grammar(name = "fld qualifier")]
pub enum FldQualifier {
    Description {
        kw: (StringEqCI!("%DESCRIPTION"), Space),
        value: Literal,
    },
    Fcon(Fcon),
    SqlCollation(SqlCollation),
    Calculated(StringEqCI!("CALCULATED")),
    CharacterSet {
        kw: (StringEqCI!("CHARACTER"), Space, StringEqCI!("SET"), Space),
        name: Ident,
    },
    CollateIdent {
        kw: (StringEqCI!("COLLATE"), Space),
        value: CollateSpec,
    },
    ComputeCode {
        kw: (StringEqCI!("COMPUTECODE"), Space),
        body: COSMethodBody,
    },
    ComputeOnChange {
        kw: (StringEqCI!("COMPUTEONCHANGE"), Space, StringEq!("("), Space),
        first: Ident,
        rest: Vec<(Space, StringEq!(","), Space, Ident)>,
        close: (Space, StringEq!(")")),
    },
    Default {
        kw: (StringEqCI!("DEFAULT"), Space),
        value: Setdef,
    },
    OnUpdate {
        kw: (StringEqCI!("ON"), Space, StringEqCI!("UPDATE"), Space),
        value: SetUpdate,
    },
    Private(StringEqCI!("PRIVATE")),
    StorageType {
        kw: (StringEqCI!("STORAGETYPE"), Space, StringEq!("="), Space),
        value: Storagetype,
    },
    Transient(StringEqCI!("TRANSIENT")),
}

#[derive(Grammar)]
#[grammar(name = "collate spec")]
pub enum CollateSpec {
    SqlCollation(SqlCollation),
    Ident(Ident),
}

#[derive(Grammar)]
#[grammar(name = "sqlcollation")]
pub enum SqlCollation {
    PctAlphaup(StringEqCI!("%ALPHAUP")),
    PctExact(StringEqCI!("%EXACT")),
    PctMinus(StringEqCI!("%MINUS")),
    PctMvr(StringEqCI!("%MVR")),
    PctPlus(StringEqCI!("%PLUS")),
    PctSpace(StringEqCI!("%SPACE")),
    PctSqlstring {
        kw: StringEqCI!("%SQLSTRING"),
        arg: Option<(Space, StringEq!("("), Space, Literal, Space, StringEq!(")"))>,
    },
    PctSqupper {
        kw: StringEqCI!("%SQLUPPER"),
        arg: Option<(Space, StringEq!("("), Space, Literal, Space, StringEq!(")"))>,
    },
    PctString {
        kw: StringEqCI!("%STRING"),
        arg: Option<(Space, StringEq!("("), Space, Literal, Space, StringEq!(")"))>,
    },
    PctTruncate {
        kw: StringEqCI!("%TRUNCATE"),
        arg: Option<(Space, StringEq!("("), Space, Literal, Space, StringEq!(")"))>,
        alphaup: (Space, StringEqCI!("ALPHAUP")),
    },
    PctUpper(StringEqCI!("%UPPER")),
    Exact(StringEqCI!("EXACT")),
    Minus(StringEqCI!("MINUS")),
    Mvr(StringEqCI!("MVR")),
    Plus(StringEqCI!("PLUS")),
    Space(StringEqCI!("SPACE")),
    Sqlstring {
        kw: StringEqCI!("SQLSTRING"),
        arg: Option<(Space, StringEq!("("), Space, Literal, Space, StringEq!(")"))>,
    },
    Sqlupper {
        kw: StringEqCI!("SQLUPPER"),
        arg: Option<(Space, StringEq!("("), Space, Literal, Space, StringEq!(")"))>,
    },
    StringColl {
        kw: StringEqCI!("STRING"),
        arg: Option<(Space, StringEq!("("), Space, Literal, Space, StringEq!(")"))>,
    },
    Truncate {
        kw: StringEqCI!("TRUNCATE"),
        arg: Option<(Space, StringEq!("("), Space, Literal, Space, StringEq!(")"))>,
    },
    Upper(StringEqCI!("UPPER")),
}

#[derive(Grammar)]
#[grammar(name = "storagetype")]
pub enum Storagetype {
    Columnar(StringEqCI!("COLUMNAR")),
    Row(StringEqCI!("ROW")),
}

#[derive(Grammar)]
#[grammar(name = "datyp")]
pub struct Datyp {
    spec: DatypSpec,
    args: Option<(Space, StringEq!("("), DatypArgs, StringEq!(")"))>,
}

char_class!(pub IsNotParen, "any tokens except \"(\" and \")\"", |ch| ch
    != '('
    && ch != ')');

#[derive(Grammar)]
#[grammar(inline)]
pub struct DatypArgs(StringOf<IsNotParen>);

#[derive(Grammar)]
#[grammar(name = "datyp spec")]
pub enum DatypSpec {
    HashParen {
        open: StringEq!("("),
        hashes: Vec<(Space, StringEq!("#"))>,
        close: (Space, StringEq!(")")),
    },
    Binary {
        kw: StringEqCI!("BINARY"),
        varying: Option<(Space, StringEqCI!("VARYING"))>,
    },
    Bit(StringEqCI!("BIT")),
    Character {
        kw: StringEqCI!("CHARACTER"),
        varying: Option<(Space, StringEqCI!("VARYING"))>,
    },
    Char {
        kw: StringEqCI!("CHAR"),
        varying: Option<(Space, StringEqCI!("VARYING"))>,
    },
    Datetime(StringEqCI!("DATETIME")),
    Date(StringEqCI!("DATE")),
    Decimal(StringEqCI!("DECIMAL")),
    Dec(StringEqCI!("DEC")),
    Double {
        kw: StringEqCI!("DOUBLE"),
        precision: Option<(Space, StringEqCI!("PRECISION"))>,
    },
    Float(StringEqCI!("FLOAT")),
    Image(StringEqCI!("IMAGE")),
    Integer(StringEqCI!("INTEGER")),
    Int(StringEqCI!("INT")),
    LongVarchar(StringEqCI!("LONG"), Space, StringEqCI!("VARCHAR")),
    Longvarbinary(StringEqCI!("LONGVARBINARY")),
    Longvarchar(StringEqCI!("LONGVARCHAR")),
    Long(StringEqCI!("LONG")),
    Money(StringEqCI!("MONEY")),
    NationalVarchar(StringEqCI!("NATIONAL"), Space, StringEqCI!("VARCHAR")),
    NationalCharacter {
        kw: StringEqCI!("NATIONAL"),
        character_kw: (
            Space,
            StringEqCI!("CHARACTER"),
            Option<(Space, StringEqCI!("VARYING"))>,
        ),
    },
    NationalChar {
        kw: StringEqCI!("NATIONAL"),
        char_kw: Option<(
            Space,
            StringEqCI!("CHAR"),
            Option<(Space, StringEqCI!("VARYING"))>,
        )>,
    },
    Nchar(StringEqCI!("NCHAR")),
    Ntext(StringEqCI!("NTEXT")),
    Number(StringEqCI!("NUMBER")),
    NumericRaw(StringEqCI!("NUMERIC"), Space, StringEqCI!("RAW")),
    Real(StringEqCI!("REAL")),
    Smallint(StringEqCI!("SMALLINT")),
    Smallmoney(StringEqCI!("SMALLMONEY")),
    Text(StringEqCI!("TEXT")),
    Timestamp(StringEqCI!("TIMESTAMP")),
    Time(StringEqCI!("TIME")),
    Tinyint(StringEqCI!("TINYINT")),
    Varbinary(StringEqCI!("VARBINARY")),
    Varchar2(StringEqCI!("VARCHAR2")),
    Varchar(StringEqCI!("VARCHAR")),
    Vector(StringEqCI!("VECTOR")),
    AnyToken(DottedIdent),
}

#[derive(Grammar)]
#[grammar(name = "fcon")]
pub enum Fcon {
    CheckCnstr(CheckCnstr),
    Constraint {
        kw: (StringEqCI!("CONSTRAINT"), Space),
        name: TName,
        spec: (Space, NamedFconSpec),
    },
    Identity {
        kw: StringEqCI!("IDENTITY"),
        args: Option<(
            Space,
            StringEq!("("),
            Space,
            Literal,
            Option<(Space, StringEq!(","), Space, Literal)>,
            Space,
            StringEq!(")"),
        )>,
    },
    NotNull {
        kw: (StringEqCI!("NOT"), Space, StringEqCI!("NULL")),
        informix: Informix,
    },
    Null(StringEqCI!("NULL")),
    Primary {
        kw: (
            StringEqCI!("PRIMARY"),
            Option<(Space, StringEqCI!("KEY"))>,
            Space,
        ),
        cluster: Cluster,
        informix: Informix,
    },
    References {
        kw: (StringEqCI!("REFERENCES"), Space),
        reference: Ref,
        informix: Informix,
    },
    Unique {
        kw: (StringEqCI!("UNIQUE"), Space),
        cluster: Cluster,
        informix: Informix,
    },
}

#[derive(Grammar)]
#[grammar(name = "check cnstr")]
pub struct CheckCnstr {
    kw: (StringEqCI!("CHECK"), Space),
    parse: Chkparse,
    informix: Informix,
}

#[derive(Grammar)]
#[grammar(name = "named fcon spec")]
pub enum NamedFconSpec {
    Check {
        kw: (StringEqCI!("CHECK"), Space),
        parse: Chkparse,
    },
    Default {
        kw: (StringEqCI!("DEFAULT"), Space),
        value: Setdef,
    },
    NotNull(StringEqCI!("NOT"), Space, StringEqCI!("NULL")),
    PrimaryKey {
        kw: (StringEqCI!("PRIMARY"), Space, StringEqCI!("KEY"), Space),
        cluster: Cluster,
    },
    References {
        kw: (StringEqCI!("REFERENCES"), Space),
        reference: Ref,
    },
    Unique {
        kw: (StringEqCI!("UNIQUE"), Space),
        cluster: Cluster,
    },
}

#[derive(Grammar)]
#[grammar(name = "setupdate")]
pub struct SetUpdate(UpdateSpec);

#[derive(Grammar)]
#[grammar(name = "update spec")]
pub enum UpdateSpec {
    Negative {
        minus: (StringEq!("-"), Space),
        value: Number,
    },
    Literal(Literal),
    CurrentDate(StringEqCI!("CURRENT_DATE")),
    CurrentTime {
        kw: StringEqCI!("CURRENT_TIME"),
        precision: Option<(
            Space,
            StringEq!("("),
            Space,
            Precision,
            Space,
            StringEq!(")"),
        )>,
    },
    CurrentTimestamp {
        kw: StringEqCI!("CURRENT_TIMESTAMP"),
        precision: Option<(
            Space,
            StringEq!("("),
            Space,
            Precision,
            Space,
            StringEq!(")"),
        )>,
    },
    CurrentUser(StringEqCI!("CURRENT_USER")),
    GetDate {
        kw: (StringEqCI!("GETDATE"), StringEq!("(")),
        prec: Option<(Space, Precision)>,
        close: (Space, StringEq!(")")),
    },
    GetUtcDate {
        kw: (StringEqCI!("GETUTCDATE"), StringEq!("(")),
        prec: Option<(Space, Precision)>,
        close: (Space, StringEq!(")")),
    },
    Null(StringEqCI!("NULL")),
    SessionUser(StringEqCI!("SESSION_USER")),
    SysDate(StringEqCI!("SYSDATE")),
    SystemUser(StringEqCI!("SYSTEM_USER")),
    User(StringEqCI!("USER")),
}

#[derive(Grammar)]
#[grammar(name = "cluster")]
pub struct Cluster(Option<ClusterType>);

#[derive(Grammar)]
#[grammar(inline)]
pub enum ClusterType {
    Clustered(StringEqCI!("CLUSTERED")),
    NonClustered(StringEqCI!("NONCLUSTERED")),
}

#[derive(Grammar)]
#[grammar(name = "ref")]
pub struct Ref {
    name: TName,
    flist: Option<(Space, FList1)>,
    match_spec: Option<(Space, StringEqCI!("MATCH"), Space, MatchSpec)>,
    triggered_action: Option<(Space, TriggeredAction)>,
    nocheck: Option<(Space, StringEqCI!("NOCHECK"))>,
}

/// `<triggered action> ::= <delete rule> [ <update rule> ] | <update rule> [ <delete rule> ]`
#[derive(Grammar)]
#[grammar(name = "triggered action")]
pub enum TriggeredAction {
    DeleteFirst {
        delete_rule: DeleteRule,
        update_rule: Option<UpdateRule>,
    },
    UpdateFirst {
        update_rule: UpdateRule,
        delete_rule: Option<DeleteRule>,
    },
}

#[derive(Grammar)]
#[grammar(name = "positive integer", validated)]
pub struct PositiveInteger(Number);

impl Validate for PositiveInteger {
    const REQUIREMENT: &'static str = "be a positive integer";
    fn validate(&self) -> bool {
        self.0
            .print()
            .parse::<i32>()
            .map(|i| i > 0)
            .unwrap_or(false)
    }
}

pub type UpdateRule = Prefix<(StringEqCI!("ON"), Space, StringEqCI!("UPDATE"), Space), RefAction>;

pub type DeleteRule = Prefix<(StringEqCI!("ON"), Space, StringEqCI!("DELETE"), Space), RefAction>;

#[derive(Grammar)]
#[grammar(name = "match spec")]
pub enum MatchSpec {
    Full(StringEqCI!("FULL")),
    Partial(StringEqCI!("PARTIAL")),
}

#[derive(Grammar)]
#[grammar(name = "ref action")]
pub enum RefAction {
    Cascade(StringEqCI!("CASCADE")),
    NoAction(StringEqCI!("NO"), Space, StringEqCI!("ACTION")),
    SetDefault(StringEqCI!("SET"), Space, StringEqCI!("DEFAULT")),
    SetNull(StringEqCI!("SET"), Space, StringEqCI!("NULL")),
}

#[derive(Grammar)]
#[grammar(name = "informix")]
pub struct Informix(Option<(Space, StringEqCI!("CONSTRAINT"), Space, TName)>);

#[derive(Grammar)]
#[grammar(name = "not informix")]
pub struct NotInformix(TName);

pub type Chkparse = Prefix<StringEq!("("), StringEndsWith<StringEq!(")")>>;

#[derive(Grammar)]
#[grammar(name = "flist 2")]
pub enum FList2 {
    FList1(FList1),
    IdentList {
        first: Ident,
        rest: Vec<(Space, StringEq!(","), Space, Ident)>,
        flist: Option<(Space, FList1)>,
    },
}

#[derive(Grammar)]
#[grammar(name = "load-spec")]
pub enum LoadSpec {
    Sql {
        kw: (StringEqCI!("SQL"), Space),
        spec: LoadSqlSpec,
    },
    Data {
        hints: Option<(LoadHints, Space)>,
        kw: (StringEqCI!("DATA"), Space),
        spec: LoadDataSpec,
    },
}

#[derive(Grammar)]
#[grammar(name = "load-hints")]
pub struct LoadHints(Vec<(LoadHint, Space)>);

#[derive(Grammar)]
#[grammar(name = "load-hint")]
pub enum LoadHint {
    NoCheck(StringEqCI!("%NOCHECK")),
    NoIndex(StringEqCI!("%NOINDEX")),
    NoJourn(StringEqCI!("%NOJOURN")),
    NoLock(StringEqCI!("%NOLOCK")),
    Bulk(StringEqCI!("BULK")),
}

#[derive(Grammar)]
#[grammar(name = "load-data-spec")]
pub enum LoadDataSpec {
    Inline(LoadDataSpecInline),
    WithUsing(LoadDataSpecWithUsing),
}

#[derive(Grammar)]
#[grammar(name = "load-data-spec-inline")]
pub struct LoadDataSpecInline {
    from: LoadDataFrom,
    into: (Space, LoadDataInto),
    using: Option<(Space, LoadDataUsing)>,
}

#[derive(Grammar)]
#[grammar(name = "load-data-spec-with-using")]
pub struct LoadDataSpecWithUsing {
    from: Option<(LoadDataFrom, Space)>,
    into: Option<(LoadDataInto, Space)>,
    using: LoadDataUsing,
}

pub type LoadDataFrom = Prefix<(StringEqCI!("FROM"), Space), DataSource>;

#[derive(Grammar)]
#[grammar(name = "data-source")]
pub enum DataSource {
    File {
        kw: (StringEqCI!("FILE"), Space),
        path: Literal,
        columns: Option<(
            Space,
            StringEqCI!("COLUMNS"),
            Space,
            StringEq!("("),
            Space,
            ColumnDefList,
            Space,
            StringEq!(")"),
        )>,
    },
    Jdbc {
        kw: (StringEqCI!("JDBC"), Space),
        cnxn: JdbcCnxnSpec,
        data: (Space, JdbcDataSpec),
    },
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct ColumnDefList {
    first: ColumnDefItem,
    rest: Vec<(Space, StringEq!(","), Space, ColumnDefItem)>,
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct ColumnDefItem {
    name: Ident,
    datyp: (Space, Datyp),
}

#[derive(Grammar)]
#[grammar(name = "jdbc-cnxn-spec")]
pub enum JdbcCnxnSpec {
    Connection {
        kw: Option<(StringEqCI!("CONNECTION"), Space)>,
        spec: CnxnSpec,
    },
    Url {
        kw: Option<(StringEqCI!("URL"), Space)>,
        value: Literal,
    },
}

#[derive(Grammar)]
#[grammar(name = "jdbc-data-spec")]
pub enum JdbcDataSpec {
    Query {
        kw: (StringEqCI!("QUERY"), Space),
        value: Literal,
    },
    Table {
        kw: (StringEqCI!("TABLE"), Space),
        first: Ident,
        rest: Vec<(StringEq!("."), Ident)>,
    },
}

#[derive(Grammar)]
#[grammar(name = "cnxn-spec")]
pub enum CnxnSpec {
    Ident(Ident),
    Literal(Literal),
}

#[derive(Grammar)]
#[grammar(name = "load-data-into")]
pub struct LoadDataInto {
    kw: (StringEqCI!("INTO"), Space),
    table: TName,
    columns: Option<(
        Space,
        StringEq!("("),
        Space,
        Ident,
        Vec<(Space, StringEq!(","), Space, Ident)>,
        Space,
        StringEq!(")"),
    )>,
    values: Option<(
        Space,
        StringEqCI!("VALUES"),
        Space,
        StringEq!("("),
        Space,
        Ident,
        Vec<(Space, StringEq!(","), Space, Ident)>,
        Space,
        StringEq!(")"),
    )>,
}

pub type LoadDataUsing = Prefix<(StringEqCI!("USING"), Space), LoadOptions>;

#[derive(Grammar)]
#[grammar(name = "load-options")]
pub enum LoadOptions {
    Json(LoadOptionsJson),
    Name(LoadOptionsName),
}

#[derive(Grammar)]
#[grammar(name = "load-options-json")]
pub struct LoadOptionsJson(Literal);

#[derive(Grammar)]
#[grammar(name = "load-options-name")]
pub struct LoadOptionsName(Ident);

#[derive(Grammar)]
#[grammar(name = "load-sql-spec")]
pub struct LoadSqlSpec {
    kw: (StringEqCI!("FROM"), Space),
    source: SqlSource,
    dialect: Option<(Space, LoadSqlDialect)>,
    delimiter: Option<(Space, LoadSqlDelimiter)>,
    verbose: Option<(Space, StringEqCI!("VERBOSE"))>,
    log_to_file: Option<(Space, LoadSqlLogToFile)>,
    ignore_errors: Option<(Space, StringEqCI!("IGNORE"), Space, StringEqCI!("ERRORS"))>,
}

#[derive(Grammar)]
#[grammar(name = "sql-source")]
pub enum SqlSource {
    Directory {
        kw: (StringEqCI!("DIRECTORY"), Space),
        path: Literal,
        recurse: Option<(Space, StringEqCI!("RECURSE"))>,
    },
    File {
        kw: (StringEqCI!("FILE"), Space),
        path: Literal,
    },
}

pub type LoadSqlDialect = Prefix<(StringEqCI!("DIALECT"), Space), Literal>;

pub type LoadSqlDelimiter = Prefix<(StringEqCI!("DELIMITER"), Space), Literal>;

pub type LoadSqlLogToFile = Prefix<
    (
        StringEqCI!("LOG"),
        Space,
        StringEqCI!("TO"),
        Space,
        StringEqCI!("FILE"),
    ),
    Option<(Space, Literal)>,
>;

#[derive(Grammar)]
#[grammar(name = "alter-server-action")]
pub enum AlterServerAction {
    Alter {
        kw: (StringEqCI!("ALTER"), Space),
        action: SubAlterServerAction,
    },
    Modify {
        kw: (StringEqCI!("MODIFY"), Space),
        first: SubAlterServerAction,
        rest: Vec<(Space, StringEq!(","), Space, SubAlterServerAction)>,
    },
}

#[derive(Grammar)]
#[grammar(name = "sub-alter-server-action")]
pub enum SubAlterServerAction {
    FtPushdown(FTPushdownClause),
    Connection {
        kw: (StringEqCI!("CONNECTION"), Space),
        value: Literal,
    },
    DelimitedIds(StringEqCI!("DELIMITEDIDS")),
    Host {
        kw: (StringEqCI!("HOST"), Space),
        value: Literal,
    },
    NoDelimitedIds(StringEqCI!("NODELIMITEDIDS")),
    Password {
        kw: (StringEqCI!("PASSWORD"), Space),
        value: Literal,
    },
    Port {
        kw: (StringEqCI!("PORT"), Space),
        value: Literal,
    },
    Username {
        kw: (StringEqCI!("USERNAME"), Space),
        value: Literal,
    },
}

#[derive(Grammar)]
#[grammar(name = "vector-datyp")]
pub enum VectorDatyp {
    Decimal(StringEqCI!("DECIMAL")),
    Double(StringEqCI!("DOUBLE")),
    Float(StringEqCI!("FLOAT")),
    Int(StringEqCI!("INT")),
    Integer(StringEqCI!("INTEGER")),
    String(StringEqCI!("STRING")),
}

#[derive(Grammar)]
#[grammar(name = "stats clause")]
pub enum StatsClause {
    Statistics(StringEqCI!("STATISTICS")),
    Stats(StringEqCI!("STATS")),
}

#[derive(Grammar)]
#[grammar(name = "stats scope")]
pub enum StatsScope {
    Schema {
        kw: (StringEqCI!("SCHEMA"), Space),
        schema: Schema,
    },
    Table {
        kw: (StringEqCI!("TABLE"), Space),
        table: TName,
    },
}

#[derive(Grammar)]
#[grammar(name = "stats property")]
pub enum StatsProperty {
    IgnoreFixedStats(StringEqCI!("IGNOREFIXEDSTATS")),
    SkipAutomaticStatsCollection(StringEqCI!("SKIPAUTOMATICSTATSCOLLECTION")),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_ok(cases: &[&str]) {
        let failed: Vec<_> = cases.iter().filter(|c| SQL::parse(c).is_err()).collect();
        assert!(failed.is_empty(), "failed to parse: {failed:#?}");
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
}
