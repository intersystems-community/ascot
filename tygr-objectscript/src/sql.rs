//! The IRIS SQL grammar for `&sql(...)` blocks embedded in ObjectScript, reached from `cos.rs`.
//! Syntax only: `ascot` reports parse errors here but does not type-check SQL yet.
//!
//! Written once under `#[levels]`, which makes it twice: `full`, every spelling IRIS accepts, is
//! re-exported here; `core` lacks the syntactic sugar, which `desugar` rewrites into it.

use std::cell::{Cell, RefCell};
use std::ops::{Deref, Range};

use either::Either;
use tygr::*;

use crate::convert::VecSep;
use crate::cos;

pub mod desugar;

pub use full::*;

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

// tygr 0.2.0 dropped this helper; rebuilt from tygr's public building blocks.
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

#[derive(Grammar, Clone, Default)]
#[tygr(hidden)]
pub struct Space(Vec<SpaceUnit>);

impl Space {
    /// One blank.
    pub fn blank() -> Space {
        Space(vec![SpaceUnit::Raw(Hidden(Raw::new(" ")))])
    }
}

#[derive(Grammar, Clone)]
pub enum Ident {
    Simple(Identifier),
    Delimited(DelimitedIdentifier),
}

impl Ident {
    /// A plain identifier, as a rule spells a name.
    pub fn simple(name: &str) -> Ident {
        Ident::Simple(Identifier(Raw::new(name)))
    }
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

/// `T` with the range of the source text it comes from or, after a rewrite, stands for.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Parse)]
#[tygr(inline, try_from_infallible = Ranged<T>)]
pub struct Src<T> {
    pub range: Range<usize>,
    pub node: T,
}

impl<T> Src<T> {
    pub fn new(range: Range<usize>, node: T) -> Self {
        Src { range, node }
    }

    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// Apply `f` to the node, keeping the same range.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Src<U> {
        Src {
            range: self.range,
            node: f(self.node),
        }
    }

    /// A node a rule synthesizes takes the range of the node being rewritten (see `rewriting`).
    pub fn synthesized(node: T) -> Self {
        let (start, end) = REWRITING.get();
        Src::new(start..end, node)
    }

    /// `f()`, during which `synthesized` nodes take `self`'s range.
    pub fn rewriting<R>(&self, f: impl FnOnce() -> R) -> R {
        let outer = REWRITING.replace((self.range.start, self.range.end));
        let result = f();
        REWRITING.set(outer);
        result
    }
}

impl<T> From<Ranged<T>> for Src<T> {
    fn from(ranged: Ranged<T>) -> Self {
        Src {
            range: ranged.range(),
            node: ranged.ranged,
        }
    }
}

impl<T> Deref for Src<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.node
    }
}

/// Where a node printed, and the range of the text it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapping {
    pub printed: Range<usize>,
    pub origin: Range<usize>,
}

thread_local! {
    static RECORDER: RefCell<Option<Vec<Mapping>>> = const { RefCell::new(None) };
    static REWRITING: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

impl<T: Print> Print for Src<T> {
    fn print_to(&self, buf: &mut String) {
        let start = buf.len();
        self.node.print_to(buf);
        RECORDER.with_borrow_mut(|recorder| {
            if let Some(mappings) = recorder {
                mappings.push(Mapping {
                    printed: start..buf.len(),
                    origin: self.range.clone(),
                });
            }
        });
    }
}

/// `t`'s text, with where each `Src` node in it printed, innermost first.
pub fn print_mapped<T: Print>(t: &T) -> (String, Vec<Mapping>) {
    let outer = RECORDER.replace(Some(Vec::new()));
    let text = t.print();
    let mappings = RECORDER.replace(outer).unwrap_or_default();
    (text, mappings)
}

#[tygr_objectscript_macros::levels]
pub mod leveled {
    use either::Either;
    use tygr::*;

    use super::*;
    use crate::convert::VecSep;

    #[derive(Grammar)]
    pub struct TName(pub Src<DottedIdent>);

    pub type Table = TName;

    #[derive(Grammar)]
    #[tygr(name = "SQL function name")]
    pub struct SQLFunctionName(pub Src<DottedIdent>);

    #[derive(Grammar)]
    #[tygr(name = "colmn")]
    pub enum Colmn {
        Str(Src<Ident>, Space, StringEq!("."), Space, StringEq!("*")),
        Arr(Src<DottedIdent>, Vec<ArrowRef>),
    }

    /// Arrow reference for object property access (Cache/IRIS extension)
    pub type ArrowRef = Prefix<(Space, StringEq!("->"), Space), Ident>;

    #[derive(Grammar)]
    pub struct Precision(pub NonNegativeInteger);

    #[derive(Grammar)]
    #[tygr(name = "non-negative integer")]
    pub struct NonNegativeInteger(pub StringOf1<IsDigit>);

    #[derive(Grammar)]
    #[tygr(name = "positive numeric literal value")]
    pub struct PositivceNumericLiteralValue {
        pub sign: Option<StringEq!("+")>,
        pub integer: StringOf1<IsDigit>,
        pub fraction: Option<(StringEq!("."), StringOf1<IsDigit>)>,
        pub exponent: Option<SQLExponent>,
    }

    #[derive(Grammar)]
    #[tygr(name = "not equals operation")]
    pub enum NotEqualsOperation {
        Diamond(StringEq!("<>")),
        #[sugar]
        BangEq(StringEq!("!=")),
        #[sugar]
        NotEq(StringEqCI!("NOT=")),
    }

    /// `!=` and `NOT=` are `<>`.
    impl Desugar for NotEqualsOperationSugar<'_> {
        type Core = core::NotEqualsOperation;
        fn desugar(&self) -> Self::Core {
            core::NotEqualsOperation::Diamond(Default::default())
        }
    }

    #[derive(Grammar)]
    #[tygr(name = "order predicate")]
    pub enum OrderPredicate {
        Le(StringEq!("<=")),
        #[sugar]
        NotGt(StringEqCI!("NOT>")),
        Ge(StringEq!(">=")),
        #[sugar]
        NotLt(StringEqCI!("NOT<")),
        Gt(StringEq!(">")),
        Lt(StringEq!("<")),
    }

    /// `NOT>` is `<=` and `NOT<` is `>=`.
    impl Desugar for OrderPredicateSugar<'_> {
        type Core = core::OrderPredicate;
        fn desugar(&self) -> Self::Core {
            match self {
                Self::NotGt(_) => core::OrderPredicate::Le(Default::default()),
                Self::NotLt(_) => core::OrderPredicate::Ge(Default::default()),
            }
        }
    }

    #[derive(Grammar)]
    #[tygr(name = "contains predicate")]
    pub enum ContainsPredicate {
        LB(StringEq!("[")),
        NotLB(StringEqCI!("NOT[")),
    }

    #[derive(Grammar)]
    #[tygr(name = "follows predicate")]
    pub enum FollowsPredicate {
        Follows(StringEq!("]")),
        NotFollows(StringEqCI!("NOT]")),
    }

    #[derive(Grammar)]
    #[tygr(name = "cmp op")]
    pub enum CmpOp {
        StartsWith(StringEqCI!("%STARTSWITH")),
        Eq(StringEq!("=")),
        Ne(NotEqualsOperation),
        Ord(OrderPredicate),
        Contains(ContainsPredicate),
        Follows(FollowsPredicate),
    }

    #[derive(Grammar)]
    pub enum Sign {
        Plus(StringEq!("+")),
        Minus(StringEq!("-")),
    }

    #[sugar]
    #[derive(Grammar)]
    #[tygr(name = "percent op")]
    pub enum PercentOp {
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
        StringOperator(StringOperator),
    }

    #[derive(Grammar)]
    #[tygr(name = "term")]
    pub enum Term {
        Macro(
            StringEq!("$$$"),
            Identifier,
            Option<crate::cos::MacroArgList>,
        ),
        Literal(Literal),
        Builtin(Box<Builtin>),
        #[sugar]
        ScalarFunction(Box<ScalarFunction>),
        Window(Box<Window>),
        Agg(Box<Agg>),
        Unary {
            op: Sign,
            space: Space,
            term: Box<Src<Term>>,
        },
        #[sugar]
        PercentUnary {
            op: PercentOp,
            space: Space,
            term: Box<Src<Term>>,
        },
        #[sugar]
        StringOperation(Box<StringOperation>),
        XTFun(Box<XTFun>),
        ParameterMarker(ParameterMarker),
        Case(Box<Case>),
        #[sugar]
        OdbcEscape(Box<ODBCEscape>),
        Scalx(Parenthesized<Box<Scalx>>),
        Query(SQLSubquery),
        Niladic(Niladic, NotFollowedBy<IdentifierPart>),
        Call(Call),
        Column(Colmn),
        SpecialSystemVariable(SpecialSystemVariable),
        HostVar(HostVar),
        Null(StringEqCI!("NULL")),
    }

    /// A scalar function is the call by its name, `%SQLUPPER x` the call `%SQLUPPER(x)`, `%TRUNCATE(x, n)`
    /// the call, and an ODBC escape what it stands for (see the fns below).
    impl Desugar for TermSugar<'_> {
        type Core = core::Term;
        fn desugar(&self) -> Self::Core {
            match self {
                Self::ScalarFunction(f) => scalar_function(f),
                Self::PercentUnary { op, term, .. } => percent_call(op, term),
                Self::StringOperation(operation) => string_operation(operation),
                Self::OdbcEscape(escape) => odbc_escape(escape),
            }
        }
    }

    /// The call a function's keyword and argument list spell; a niladic-looking one gets `()`.
    macro_rules! calls {
        ($Enum:ident: $value:expr; $($Variant:ident)*; $($Niladic:ident)*; $($arm:tt)*) => {
            match $value {
                $($Enum::$Variant(kw, a) => call(&kw.print(), args(a)),)*
                $($Enum::$Niladic(kw, _) => call(&kw.print(), Vec::new()),)*
                $($arm)*
            }
        };
    }

    /// A scalar function is the call by its name, `POSITION(a IN b)` the call `CHARINDEX(a, b)`.
    fn scalar_function(function: &ScalarFunction) -> core::Term {
        calls!(ScalarFunction: function;
            PercentObject PercentOid Similarity Abs Ascii Ceiling CharacterLength CharIndex CharLength
            Coalesce Convert DataLength Date DateAdd DateDiff DateName DatePart DateTrunc Day Decode
            Embedding Floor GetDate GetUtcDate Greatest HashBytes IfNull Instr IsNull IsNumeric LastDay
            LastIdentity LCase Least Left Len Length Lower LPad LTrim Month Now NullIf Nvl PatIndex Power
            Repeat Replace Replicate Reverse Right Round RPad RTrim Sign Space Str String Stuff Substr
            Substring ToChar ToDate ToUSChar ToUSDate ToNumber ToTimestamp UCase UnixTimestamp
            VectorCosine VectorDotProduct Year;;
            ScalarFunction::Position(_, a) => call("CHARINDEX", args(a)),
        )
    }

    /// `{fn NAME(args)}` is `NAME(args)`, `{fn PI}` and its kin `PI()`, TIMESTAMPADD and TIMESTAMPDIFF builtins.
    fn odbc_function(function: &ODBCFunc) -> core::Term {
        calls!(ODBCFunc: function;
            Abs Acos Asin Atan Ceiling Cos Cot Exp Floor Log Log10 Mod Power Sin Sqrt Tan Ascii Char
            Concat DayName DayOfMonth DayOfWeek DayOfYear Hour LCase Left Length LTrim Minute Month
            MonthName Quarter Repeat Right Round RTrim Second Sign Space Substring Truncate UCase Week
            Year;
            Pi CurDate CurTime Database Now;
            ODBCFunc::TimestampAdd(kw, a) => {
                core::Term::Builtin(Box::new(core::Builtin::TimestampAdd(*kw, a.desugar())))
            }
            ODBCFunc::TimestampDiff(kw, a) => {
                core::Term::Builtin(Box::new(core::Builtin::TimestampDiff(*kw, a.desugar())))
            }
        )
    }

    /// `{d x}`, `{t x}` and `{ts x}` are `CAST(x AS DATE)`, `CAST(x AS TIME)` and `CAST(x AS TIMESTAMP)`.
    fn cast(value: &DTSVal) -> core::Term {
        let cast_type = match &value.dts_type {
            DTSType::Date(_) => core::CastType::Date(Default::default()),
            DTSType::Time(_) => core::CastType::Time(Default::default()),
            DTSType::Timestamp(_) => core::CastType::Timestamp(Default::default()),
        };
        let expr = term(match &value.val_spec {
            DTSValSpec::Literal(literal) => core::Term::Literal(literal.clone()),
            DTSValSpec::HostVar(host_var) => core::Term::HostVar(host_var.clone()),
        });
        let cast = core::SQLCastExpression {
            open: (Space::default(), Default::default(), Space::default()),
            expr,
            as_kw: (Space::blank(), Default::default(), Space::blank()),
            cast_type,
            close: (Space::default(), Default::default()),
        };
        core::Term::Builtin(Box::new(core::Builtin::Cast(
            Default::default(),
            Box::new(cast),
        )))
    }

    /// An ODBC escape is what it stands for: see `odbc_function` and `cast`.
    fn odbc_escape(escape: &ODBCEscape) -> core::Term {
        match &escape.wrapped {
            DTS::DateTime(value) => cast(value),
            DTS::Function(function) => odbc_function(&function.prefixed),
        }
    }

    /// `%SQLUPPER x` is the call `%SQLUPPER(x)`; an `x` in parentheses gives the call its own.
    fn percent_call(op: &PercentOp, term: &Src<Term>) -> core::Term {
        let arg = match &term.node {
            Term::Scalx(parens) => (*parens.wrapped).desugar(),
            _ => core::Scalx::Legacy(VecSep::one(term.desugar())),
        };
        call(&op.print(), vec![arg])
    }

    /// `%TRUNCATE(x, n)` is the call, as `%STRING`, `%SQLSTRING` and `%SQLUPPER` with a truncation are.
    fn string_operation(operation: &StringOperation) -> core::Term {
        let truncation = match &operation.truncation {
            ColTrun::Const(constant) => {
                let core::PConst::Literal(literal) = constant.desugar();
                core::Term::Literal(literal)
            }
            ColTrun::HostVar(host_var) => core::Term::HostVar(host_var.clone()),
        };
        let args = vec![operation.expr.desugar(), term(truncation)];
        call(&operation.op.print(), args)
    }

    #[sugar]
    #[derive(Grammar)]
    #[tygr(name = "%string operation")]
    pub struct StringOperation {
        pub op: StringOperator,
        pub open: (Space, StringEq!("("), Space),
        pub expr: Scalx,
        pub comma: ArgSep,
        pub truncation: ColTrun,
        pub close: (Space, StringEq!(")")),
    }

    #[sugar]
    #[derive(Grammar)]
    #[tygr(name = "%string operator")]
    pub enum StringOperator {
        String(StringEqCI!("%STRING")),
        SqlString(StringEqCI!("%SQLSTRING")),
        SqlUpper(StringEqCI!("%SQLUPPER")),
        Truncate(StringEqCI!("%TRUNCATE")),
    }

    #[sugar]
    #[derive(Grammar)]
    pub enum ColTrun {
        Const(PConst),
        HostVar(HostVar),
    }

    #[derive(Grammar)]
    #[tygr(name = "special system variable")]
    pub enum SpecialSystemVariable {
        Horolog(StringEqCI!("$HOROLOG")),
        #[sugar]
        H(StringEqCI!("$H")),
        Job(StringEqCI!("$JOB")),
        #[sugar]
        J(StringEqCI!("$J")),
        Namespace(StringEqCI!("$NAMESPACE")),
        TLevel(StringEqCI!("$TLEVEL")),
        #[sugar]
        TL(StringEqCI!("$TL")),
        Username(StringEqCI!("$USERNAME")),
        ZHorolog(StringEqCI!("$ZHOROLOG")),
        #[sugar]
        ZH(StringEqCI!("$ZH")),
        ZJob(StringEqCI!("$ZJOB")),
        #[sugar]
        ZJ(StringEqCI!("$ZJ")),
        #[sugar]
        ZNspace(StringEqCI!("$ZNSPACE")),
        ZPi(StringEqCI!("$ZPI")),
        ZTimestamp(StringEqCI!("$ZTIMESTAMP")),
        ZTimezone(StringEqCI!("$ZTIMEZONE")),
        #[sugar]
        ZTS(StringEqCI!("$ZTS")),
        #[sugar]
        ZTZ(StringEqCI!("$ZTZ")),
        ZVersion(StringEqCI!("$ZVERSION")),
        #[sugar]
        ZV(StringEqCI!("$ZV")),
    }

    /// `$H` is `$HOROLOG`, and so on: an alias is its full name.
    impl Desugar for SpecialSystemVariableSugar<'_> {
        type Core = core::SpecialSystemVariable;
        fn desugar(&self) -> Self::Core {
            use core::SpecialSystemVariable::*;
            match self {
                Self::H(_) => Horolog(Default::default()),
                Self::J(_) => Job(Default::default()),
                Self::TL(_) => TLevel(Default::default()),
                Self::ZH(_) => ZHorolog(Default::default()),
                Self::ZJ(_) => ZJob(Default::default()),
                Self::ZNspace(_) => Namespace(Default::default()),
                Self::ZTS(_) => ZTimestamp(Default::default()),
                Self::ZTZ(_) => ZTimezone(Default::default()),
                Self::ZV(_) => ZVersion(Default::default()),
            }
        }
    }

    /// A function called by name, built-in or user-defined.
    #[derive(Grammar)]
    #[tygr(name = "call")]
    pub struct Call {
        pub name: SQLFunctionName,
        pub args: ArgsN,
    }

    /// A niladic function, which stands without parentheses.
    #[derive(Grammar)]
    #[tygr(name = "niladic")]
    pub enum Niladic {
        CurrentDate(StringEqCI!("CURRENT_DATE")),
        CurrentTimestamp(StringEqCI!("CURRENT_TIMESTAMP")),
        CurrentTime(StringEqCI!("CURRENT_TIME")),
        SysDate(StringEqCI!("SYSDATE")),
        User(StringEqCI!("USER")),
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
        #[sugar]
        Underscore(StringEq!("_")),
    }

    /// `_` is `||`.
    impl Desugar for ScalopSugar<'_> {
        type Core = core::Scalop;
        fn desugar(&self) -> Self::Core {
            core::Scalop::Concat(Default::default())
        }
    }

    #[derive(Grammar)]
    pub enum Scalx {
        Legacy(VecSep<Src<Term>, Wrap<Space, Scalop, Space>>),
    }
    #[derive(Grammar)]
    #[tygr(name = "case")]
    pub enum Case {
        Searched(SearchedCase),
        Simple(SimpleCase),
    }

    #[derive(Grammar)]
    #[tygr(name = "searched case")]
    pub struct SearchedCase {
        pub case_kw: (StringEqCI!("CASE"), Space),
        pub when_clauses: Vec<SQLWhenClause>,
        pub else_clause: Implied<SQLElseClause>,
        pub end_kw: (Space, StringEqCI!("END")),
    }

    #[derive(Grammar)]
    #[tygr(name = "simple case")]
    pub struct SimpleCase {
        pub case_kw: (StringEqCI!("CASE"), Space),
        pub expr: Scalx,
        pub when_clauses: Vec<SQLSimpleCaseWhen>,
        pub else_clause: Implied<SQLElseClause>,
        pub end_kw: (Space, StringEqCI!("END")),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLWhenClause {
        pub when_kw: (Space, StringEqCI!("WHEN"), Space),
        pub condition: Prdct,
        pub then_kw: (Space, StringEqCI!("THEN"), Space),
        pub result: Scalx,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLSimpleCaseWhen {
        pub when_kw: (Space, StringEqCI!("WHEN"), Space),
        pub value: Scalx,
        pub then_kw: (Space, StringEqCI!("THEN"), Space),
        pub result: Scalx,
    }

    /// `ELSE result`; omitted, a CASE reads `ELSE NULL`, what it returns when no WHEN matches.
    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLElseClause {
        pub else_kw: (StringEqCI!("ELSE"), Space),
        pub result: Scalx,
    }

    impl Default for SQLElseClause {
        fn default() -> Self {
            SQLElseClause {
                else_kw: (Default::default(), Space::blank()),
                result: Scalx::Legacy(VecSep::one(Src::synthesized(
                    Term::Null(Default::default()),
                ))),
            }
        }
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLCastExpression {
        pub open: (Space, StringEq!("("), Space),
        pub expr: Scalx,
        pub as_kw: (Space, StringEqCI!("AS"), Space),
        pub cast_type: CastType,
        pub close: (Space, StringEq!(")")),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct Args0 {
        pub open: (Space, StringEq!("("), Space),
        pub close: (Space, StringEq!(")")),
    }

    pub type ArgsR<T = Scalx> = Wrap<(Space, StringEq!("("), Space), T, (Space, StringEq!(")"))>;

    // A derive rejects a literal in a generic type's field, so the generic argument lists alias theirs.
    type ArgsOpen = (Space, StringEq!("("), Space);
    type ArgsClose = (Space, StringEq!(")"));

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ArgsRR<T1: Grammar = Scalx, T2: Grammar = Scalx> {
        pub open: ArgsOpen,
        pub arg1: T1,
        pub sep1: ArgSep,
        pub arg2: T2,
        pub close: ArgsClose,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ArgsRRR<T1: Grammar = Scalx, T2: Grammar = Scalx, T3: Grammar = Scalx> {
        pub open: ArgsOpen,
        pub arg1: T1,
        pub sep1: ArgSep,
        pub arg2: T2,
        pub sep2: ArgSep,
        pub arg3: T3,
        pub close: ArgsClose,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ArgsRRRR<
        T1: Grammar = Scalx,
        T2: Grammar = Scalx,
        T3: Grammar = Scalx,
        T4: Grammar = Scalx,
    > {
        pub open: ArgsOpen,
        pub arg1: T1,
        pub sep1: ArgSep,
        pub arg2: T2,
        pub sep2: ArgSep,
        pub arg3: T3,
        pub sep3: ArgSep,
        pub arg4: T4,
        pub close: ArgsClose,
    }

    pub type ArgsO<T = Scalx> =
        Wrap<(Space, StringEq!("("), Space), Option<T>, (Space, StringEq!(")"))>;

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ArgsRO<T1: Grammar = Scalx, T2: Grammar = Scalx> {
        pub open: ArgsOpen,
        pub arg1: T1,
        pub arg2: Option<(ArgSep, T2)>,
        pub close: ArgsClose,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ArgsROO<T1: Grammar = Scalx, T2: Grammar = Scalx, T3: Grammar = Scalx> {
        pub open: ArgsOpen,
        pub arg1: T1,
        pub rest: Option<(ArgSep, T2, Option<(ArgSep, T3)>)>,
        pub close: ArgsClose,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ArgsRRO<T1: Grammar = Scalx, T2: Grammar = Scalx, T3: Grammar = Scalx> {
        pub open: ArgsOpen,
        pub arg1: T1,
        pub sep1: ArgSep,
        pub arg2: T2,
        pub arg3: Option<(ArgSep, T3)>,
        pub close: ArgsClose,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ArgsRROO<
        T1: Grammar = Scalx,
        T2: Grammar = Scalx,
        T3: Grammar = Scalx,
        T4: Grammar = Scalx,
    > {
        pub open: ArgsOpen,
        pub arg1: T1,
        pub sep1: ArgSep,
        pub arg2: T2,
        pub rest: Option<(ArgSep, T3, Option<(ArgSep, T4)>)>,
        pub close: ArgsClose,
    }

    pub type ArgsN = Wrap<
        (Space, StringEq!("("), Space),
        Option<VecSep<Scalx, ArgSep>>,
        (Space, StringEq!(")")),
    >;

    pub type Args1Plus =
        Wrap<(Space, StringEq!("("), Space), VecSep<Scalx, ArgSep>, (Space, StringEq!(")"))>;

    /// The functions a call by name cannot spell: the `$` functions, CURRENT_TIMESTAMP and
    /// CURRENT_TIME with a precision, which the niladic would shadow, and the ODBC-only functions.
    #[derive(Grammar)]
    #[tygr(name = "builtin")]
    pub enum Builtin {
        XmlConcat(XMLConcat),
        XmlElement(XMLElement),
        XmlForest(XMLForest),
        Cast(StringEqCI!("CAST"), Box<SQLCastExpression>),
        Convert(StringEqCI!("CONVERT"), ArgsRR<Scalx, SQLConvertFunc>),
        JSONArray(StringEqCI!("JSON_ARRAY"), SQLJSONArrayArgs),
        JSONObject(StringEqCI!("JSON_OBJECT"), SQLJSONObjectArgs),
        Predict(StringEqCI!("PREDICT"), SQLPredictArgs),
        Probability(StringEqCI!("PROBABILITY"), SQLProbabilityArgs),
        SearchIndex(StringEqCI!("SEARCH_INDEX"), SQLSearchIndexArgs),
        TimestampAdd(
            StringEqCI!("TIMESTAMPADD"),
            ArgsRRR<OptionallyQuoted<TSAddInterval>>,
        ),
        TimestampDiff(
            StringEqCI!("TIMESTAMPDIFF"),
            ArgsRRR<OptionallyQuoted<TSDiffInterval>>,
        ),
        ToVector(
            StringEqCI!("TO_VECTOR"),
            ArgsROO<Scalx, VectorDatyp, Integer>,
        ),
        Trim(StringEqCI!("TRIM"), Space, Parenthesized<TrimArgs>),
        Odbc(ODBCOnly),
        CurrentTimestamp(StringEqCI!("CURRENT_TIMESTAMP"), ArgsR),
        CurrentTime(StringEqCI!("CURRENT_TIME"), ArgsR),
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
    }

    /// The functions a call by name spells, which the core reads as calls.
    #[sugar]
    #[derive(Grammar)]
    #[tygr(name = "scalar function")]
    pub enum ScalarFunction {
        PercentObject(StringEqCI!("%OBJECT"), ArgsR),
        PercentOid(StringEqCI!("%OID"), ArgsR),
        Similarity(StringEqCI!("%SIMILARITY"), ArgsRR),
        Abs(StringEqCI!("ABS"), ArgsR),
        Ascii(StringEqCI!("ASCII"), ArgsR),
        Ceiling(StringEqCI!("CEILING"), ArgsR),
        CharacterLength(StringEqCI!("CHARACTER_LENGTH"), ArgsR),
        CharIndex(StringEqCI!("CHARINDEX"), ArgsRRO),
        CharLength(StringEqCI!("CHAR_LENGTH"), ArgsR),
        Coalesce(StringEqCI!("COALESCE"), Args1Plus),
        Convert(StringEqCI!("CONVERT"), ArgsRRO),
        DataLength(StringEqCI!("DATALENGTH"), ArgsR),
        Date(StringEqCI!("DATE"), ArgsR),
        DateAdd(StringEqCI!("DATEADD"), ArgsRRR<DatePart>),
        DateDiff(StringEqCI!("DATEDIFF"), ArgsRRR<DatePart>),
        DateName(StringEqCI!("DATENAME"), ArgsRR<DatePart>),
        DatePart(StringEqCI!("DATEPART"), ArgsRR<DatePartPart>),
        DateTrunc(StringEqCI!("DATE_TRUNC"), ArgsRR<DatePart>),
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
        Repeat(StringEqCI!("REPEAT"), ArgsRR),
        Replace(StringEqCI!("REPLACE"), ArgsRRR),
        Replicate(StringEqCI!("REPLICATE"), ArgsRR),
        Reverse(StringEqCI!("REVERSE"), ArgsR),
        Right(StringEqCI!("RIGHT"), ArgsRR),
        Round(StringEqCI!("ROUND"), ArgsRR),
        RPad(StringEqCI!("RPAD"), ArgsRRO),
        RTrim(StringEqCI!("RTRIM"), ArgsR),
        Sign(StringEqCI!("SIGN"), ArgsR),
        Space(StringEqCI!("SPACE"), ArgsR),
        Str(StringEqCI!("STR"), ArgsROO),
        String(StringEqCI!("STRING"), Args1Plus),
        Stuff(StringEqCI!("STUFF"), ArgsRRRR),
        Substr(StringEqCI!("SUBSTR"), ArgsRRO),
        Substring(StringEqCI!("SUBSTRING"), SubstringArgs),
        ToChar(StringEqCI!("TOCHAR"), ArgsRO),
        ToDate(StringEqCI!("TODATE"), ArgsRO),
        ToUSChar(StringEqCI!("TO_CHAR"), ArgsRO),
        ToUSDate(StringEqCI!("TO_DATE"), ArgsRO),
        ToNumber(StringEqCI!("TO_NUMBER"), ArgsR<Src<Term>>),
        ToTimestamp(StringEqCI!("TO_TIMESTAMP"), ArgsRO),
        UCase(StringEqCI!("UCASE"), ArgsR),
        UnixTimestamp(StringEqCI!("UNIX_TIMESTAMP"), ArgsO),
        VectorCosine(StringEqCI!("VECTOR_COSINE"), ArgsRR),
        VectorDotProduct(StringEqCI!("VECTOR_DOT_PRODUCT"), ArgsRR),
        Year(StringEqCI!("YEAR"), ArgsR),
    }

    #[sugar]
    pub type DatePart = OptionallyWrapped<DatePartCommon>;

    #[sugar]
    #[derive(Grammar)]
    #[tygr(name = "datepart_part")]
    pub struct DatePartPart(pub OptionallyWrapped<DatePartPartRaw>);

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum OptionallyWrapped<T: Grammar> {
        Wrapped(Parenthesized<T>),
        Scalar(T),
    }

    #[sugar]
    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum DatePartCommon {
        Year(Either<StringEqCI!("YEAR"), Either<StringEqCI!("YYYY"), StringEqCI!("YY")>>),
        Quarter(Either<StringEqCI!("QUARTER"), Either<StringEqCI!("QQ"), StringEqCI!("Q")>>),
        Month(Either<StringEqCI!("MONTH"), Either<StringEqCI!("MM"), StringEqCI!("M")>>),
        Week(Either<StringEqCI!("WEEK"), Either<StringEqCI!("WK"), StringEqCI!("WW")>>),
        Weekday(Either<StringEqCI!("WEEKDAY"), Either<StringEqCI!("DW"), StringEqCI!("W")>>),
        DayOfYear(Either<StringEqCI!("DAYOFYEAR"), Either<StringEqCI!("DY"), StringEqCI!("Y")>>),
        Day(Either<StringEqCI!("DAY"), Either<StringEqCI!("DD"), StringEqCI!("D")>>),
        Hour(Either<StringEqCI!("HOUR"), StringEqCI!("HH")>),
        Minute(Either<StringEqCI!("MINUTE"), Either<StringEqCI!("MI"), StringEqCI!("N")>>),
        Second(Either<StringEqCI!("SECOND"), Either<StringEqCI!("SS"), StringEqCI!("S")>>),
        Millisecond(Either<StringEqCI!("MILLISECOND"), StringEqCI!("MS")>),
        Microsecond(Either<StringEqCI!("MICROSECOND"), StringEqCI!("MCS")>),
        Nanosecond(Either<StringEqCI!("NANOSECOND"), StringEqCI!("NS")>),
        Literal(Literal),
        HostVar(HostVar),
    }

    #[sugar]
    #[derive(Grammar)]
    #[tygr(name = "datepart_part_raw")]
    pub enum DatePartPartRaw {
        SqlTimestamp(Either<StringEqCI!("SQLTIMESTAMP"), StringEqCI!("STS")>),
        Common(DatePartCommon),
    }

    #[sugar]
    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLPositionArgs {
        pub open: (Space, StringEq!("("), Space),
        pub needle: Scalx,
        pub in_kw: (Space, StringEqCI!("IN"), Space),
        pub haystack: Scalx,
        pub close: (Space, StringEq!(")")),
    }

    #[sugar]
    pub type SubstringArgs =
        Wrap<(Space, StringEq!("("), Space), SubstringArgsInternal, (Space, StringEq!(")"))>;

    #[sugar]
    #[derive(Grammar)]
    #[tygr(name = "substring args")]
    pub enum SubstringArgsInternal {
        Keyword {
            expr: Scalx,
            from_kw: (Space, StringEqCI!("FROM"), Space),
            start: Scalx,
            for_clause: Option<((Space, StringEqCI!("FOR"), Space), Scalx)>,
        },
        Position {
            expr: Scalx,
            comma: ArgSep,
            start: Scalx,
            length: Option<(ArgSep, Scalx)>,
        },
    }

    /// TRIM's arguments, `BOTH ' ' FROM x` in the core: a lone spec trims blanks, lone chars both ends.
    #[derive(Grammar)]
    pub enum TrimArgs {
        SpecScalxScalx(
            TrimSpec,
            Space,
            Scalx,
            Space,
            StringEqCI!("FROM"),
            Space,
            Scalx,
        ),
        #[sugar]
        SpecScalx(TrimSpec, Space, Option<(StringEqCI!("FROM"), Space)>, Scalx),
        #[sugar]
        ScalxScalx(Option<(Scalx, Space, StringEqCI!("FROM"), Space)>, Scalx),
    }

    /// `TRIM(LEADING FROM x)` and `TRIM(LEADING x)` are `TRIM(LEADING ' ' FROM x)`, `TRIM(c FROM x)` is
    /// `TRIM(BOTH c FROM x)` and `TRIM(x)` `TRIM(BOTH ' ' FROM x)`.
    impl Desugar for TrimArgsSugar<'_> {
        type Core = core::TrimArgs;
        fn desugar(&self) -> Self::Core {
            let (spec, chars, expr) = match self {
                Self::SpecScalx(spec, _, _, expr) => (spec.desugar(), string(" "), *expr),
                Self::ScalxScalx(chars, expr) => (
                    core::TrimSpec::Both(Default::default()),
                    chars
                        .as_ref()
                        .map_or_else(|| string(" "), |(chars, ..)| chars.desugar()),
                    *expr,
                ),
            };
            core::TrimArgs::SpecScalxScalx(
                spec,
                Space::blank(),
                chars,
                Space::blank(),
                Default::default(),
                Space::blank(),
                expr.desugar(),
            )
        }
    }

    #[derive(Grammar)]
    pub enum TrimSpec {
        Leading(StringEqCI!("LEADING")),
        Trailing(StringEqCI!("TRAILING")),
        Both(StringEqCI!("BOTH")),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLSearchIndexArgs {
        pub open: (Space, StringEq!("("), Space),
        pub table: Vec<(Ident, StringEq!("."))>,
        pub index_name: IndexName,
        pub scalx_list: Vec<(ArgSep, Scalx)>,
        pub close: (Space, StringEq!(")")),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLPredictArgs {
        pub open: (Space, StringEq!("("), Space),
        pub model: ModelName,
        pub use_clause: Option<PredictProbabilityUse>,
        pub with_clause: Option<PredictWith>,
        pub close: (Space, StringEq!(")")),
    }

    pub type PredictProbabilityUse = Prefix<(Space, StringEqCI!("USE"), Space), TrainedModelName>;

    #[derive(Grammar)]
    #[tygr(name = "trained-model-name")]
    pub struct TrainedModelName(pub Ident);

    #[derive(Grammar)]
    #[tygr(name = "predict-with")]
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
    #[tygr(inline)]
    pub struct PredictAssign {
        pub ident: Ident,
        pub eq: (Space, StringEq!("="), Space),
        pub scalx: Scalx,
    }

    pub type ProbabilityFor = Prefix<(Space, StringEqCI!("FOR"), Space), Scalx>;

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLProbabilityArgs {
        pub open: (Space, StringEq!("("), Space),
        pub model: ModelName,
        pub use_clause: Option<PredictProbabilityUse>,
        pub for_clause: Option<ProbabilityFor>,
        pub with_clause: Option<PredictWith>,
        pub close: (Space, StringEq!(")")),
    }

    pub type SQLJSONArrayArgs = ArgsO<(VecSep<Scalx, ArgSep>, Option<JSONNullClause>)>;

    pub type SQLJSONObjectArgs = ArgsO<(SQLJSONKeyValuePairs, Option<JSONNullClause>)>;

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLJSONKeyValuePairs(
        pub VecSep<(Scalx, (Space, StringEq!(":"), Space), Scalx), ArgSep>,
    );

    #[derive(Grammar)]
    #[tygr(name = "JSON null clause")]
    pub struct JSONNullClause(
        pub Space,
        pub Either<StringEqCI!("NULL"), StringEqCI!("ABSENT")>,
        pub Space,
        pub StringEqCI!("ON"),
        pub Space,
        pub StringEqCI!("NULL"),
    );

    #[derive(Grammar)]
    #[tygr(name = "xmlElement")]
    pub struct XMLElement {
        pub keyword: StringEqCI!("XMLELEMENT"),
        pub open: (Space, StringEq!("("), Space),
        pub name: Option<SQLXmlElementName>,
        pub space: Space,
        pub values: VecSep<XMLValue, ArgSep>,
        pub close: (Space, StringEq!(")")),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum SQLXmlElementName {
        NameIdent(StringEqCI!("NAME"), Space, Ident),
        NameOnly(StringEqCI!("NAME")),
        Ident(Ident),
    }

    #[derive(Grammar)]
    #[tygr(name = "xmlValue")]
    pub enum XMLValue {
        Concat(Box<XMLConcat>),
        Attributes(XMLAttributes),
        Expr(Scalx),
        Value2(XMLValue2),
    }

    #[derive(Grammar)]
    #[tygr(name = "xmlValue2")]
    pub enum XMLValue2 {
        Element(Box<XMLElement>),
        Forest(Box<XMLForest>),
        Comment(XMLComment),
    }

    #[derive(Grammar)]
    #[tygr(name = "xmlComment")]
    pub struct XMLComment(pub (StringEq!("<--"), StringEndsWith<StringEq!("-->")>));

    #[derive(Grammar)]
    #[tygr(name = "xmlAttributes")]
    pub struct XMLAttributes {
        pub keyword: StringEqCI!("XMLATTRIBUTES"),
        pub open: (Space, StringEq!("("), Space),
        pub attrs: VecSep<SQLXmlAttribute, ArgSep>,
        pub close: (Space, StringEq!(")")),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLXmlAttribute {
        pub expr: Scalx,
        pub alias: Option<((Space, StringEqCI!("AS"), Space), Ident)>,
    }

    #[derive(Grammar)]
    #[tygr(name = "xmlForest")]
    pub struct XMLForest {
        pub keyword: StringEqCI!("XMLFOREST"),
        pub open: (Space, StringEq!("("), Space),
        pub items: VecSep<SQLXmlAttribute, ArgSep>,
        pub close: (Space, StringEq!(")")),
    }

    #[derive(Grammar)]
    #[tygr(name = "xmlConcat")]
    pub struct XMLConcat {
        pub keyword: StringEqCI!("XMLCONCAT"),
        pub open: (Space, StringEq!("("), Space),
        pub values: VecSep<XMLValue2, ArgSep>,
        pub close: (Space, StringEq!(")")),
    }

    #[derive(Grammar)]
    pub struct XTFun {
        pub fun: XTFuncRef,
        pub args: XTFunArgs,
    }

    pub type XTFuncRef = Prefix<StringEqCI!("$$"), StringOf1<IsExtFuncChar>>;

    pub type XTFunArgs = Parenthesized<Option<VecSep<Scalx, ArgSep>>>;

    #[sugar]
    pub type ODBCEscape = Wrap<(StringEq!("{"), Space), DTS, (Space, StringEq!("}"))>;

    #[sugar]
    #[derive(Grammar)]
    pub enum DTS {
        Function(FunExt),
        DateTime(DTSVal),
    }

    #[sugar]
    pub type FunExt = Prefix<(StringEqCI!("FN"), Space), ODBCFunc>;

    #[sugar]
    #[derive(Grammar)]
    #[tygr(name = "odbc func")]
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
        CurDate(StringEqCI!("CURDATE"), Option<StringEq!("()")>),
        CurTime(StringEqCI!("CURTIME"), Option<StringEq!("()")>),
        Database(StringEqCI!("DATABASE"), Option<StringEq!("()")>),
        DayName(StringEqCI!("DAYNAME"), ArgsR),
        DayOfMonth(StringEqCI!("DAYOFMONTH"), ArgsR),
        DayOfWeek(StringEqCI!("DAYOFWEEK"), ArgsR),
        DayOfYear(StringEqCI!("DAYOFYEAR"), ArgsR),
        Hour(StringEqCI!("HOUR"), ArgsR),
        LCase(StringEqCI!("LCASE"), ArgsR),
        Left(StringEqCI!("LEFT"), ArgsRR),
        Length(StringEqCI!("LENGTH"), ArgsR),
        LTrim(StringEqCI!("LTRIM"), ArgsR),
        Minute(StringEqCI!("MINUTE"), ArgsR),
        Month(StringEqCI!("MONTH"), ArgsR),
        MonthName(StringEqCI!("MONTHNAME"), ArgsR),
        Now(StringEqCI!("NOW"), Option<StringEq!("()")>),
        Quarter(StringEqCI!("QUARTER"), ArgsR),
        Repeat(StringEqCI!("REPEAT"), ArgsRR),
        Right(StringEqCI!("RIGHT"), ArgsRR),
        Round(StringEqCI!("ROUND"), ArgsRR),
        RTrim(StringEqCI!("RTRIM"), ArgsR),
        Second(StringEqCI!("SECOND"), ArgsR),
        Sign(StringEqCI!("SIGN"), ArgsR),
        Space(StringEqCI!("SPACE"), ArgsR),
        Substring(StringEqCI!("SUBSTRING"), SubstringArgs),
        TimestampAdd(
            StringEqCI!("TIMESTAMPADD"),
            ArgsRRR<OptionallyQuoted<TSAddInterval>>,
        ),
        TimestampDiff(
            StringEqCI!("TIMESTAMPDIFF"),
            ArgsRRR<OptionallyQuoted<TSDiffInterval>>,
        ),
        Truncate(StringEqCI!("TRUNCATE"), ArgsRR),
        UCase(StringEqCI!("UCASE"), ArgsR),
        Week(StringEqCI!("WEEK"), ArgsR),
        Year(StringEqCI!("YEAR"), ArgsR),
    }

    /// `{fn CONVERT(x, SQL_TYPE)}` and `{fn IFNULL(x, y)}`, which IRIS reads otherwise without the
    /// escape (CONVERT takes `SQL_TYPE` for a column, IFNULL(x, y) is NULL when x is not; measured on
    /// 2026.2), and `{fn USER}`, whose niladic `USER` has no parentheses for the braces' comments.
    pub type ODBCOnly = Wrap<
        (StringEq!("{"), Space, StringEqCI!("FN"), Space),
        ODBCOnlyFunc,
        (Space, StringEq!("}")),
    >;

    #[derive(Grammar)]
    #[tygr(name = "odbc-only func")]
    pub enum ODBCOnlyFunc {
        Convert(StringEqCI!("CONVERT"), ArgsRR<Scalx, SQLConvertFunc>),
        IfNull(StringEqCI!("IFNULL"), ArgsRR),
        User(StringEqCI!("USER"), Option<StringEq!("()")>),
    }

    pub type OptionallyQuoted<T> = Either<Wrap<StringEq!("\""), T, StringEq!("\"")>, T>;

    #[derive(Grammar)]
    #[tygr(name = "tsaddinterval")]
    pub enum TSAddInterval {
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
    #[tygr(name = "tsdiffinterval")]
    pub enum TSDiffInterval {
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

    #[sugar]
    #[derive(Grammar)]
    #[tygr(name = "dts val")]
    pub struct DTSVal {
        pub dts_type: DTSType,
        pub sp: Space,
        pub val_spec: DTSValSpec,
    }

    #[sugar]
    #[derive(Grammar)]
    #[tygr(name = "dts type")]
    pub enum DTSType {
        Date(StringEqCI!("D")),
        Timestamp(StringEqCI!("TS")),
        Time(StringEqCI!("T")),
    }

    #[derive(Grammar)]
    #[tygr(name = "dts val spec")]
    pub enum DTSValSpec {
        Literal(Literal),
        HostVar(HostVar),
    }

    #[derive(Grammar)]
    #[tygr(name = "agg")]
    pub enum Agg {
        AggFuncArgs {
            fun: AggFunc,
            sp: Space,
            open: (StringEq!("("), Space),
            #[sugar]
            all: Option<AllKw>,
            arg: AggArg,
            close: (Space, StringEq!(")")),
        },
        CountStar(StringEqCI!("COUNT"), Space, Parenthesized<StringEq!("*")>),
    }

    #[derive(Grammar)]
    #[tygr(name = "agg func")]
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
    #[tygr(name = "agg arg")]
    pub struct AggArg {
        pub distinct: Option<Dist>,
        pub expr: Scalx,
        pub foreach: Option<SQLForEachClause>,
        pub afterhaving: Option<(Space, StringEqCI!("%AFTERHAVING"))>,
    }

    #[derive(Grammar)]
    #[tygr(name = "dist")]
    pub struct Dist {
        pub keyword: (StringEqCI!("DISTINCT"), Space),
        pub by: Option<SQLDistinctBy>,
    }

    pub type SQLDistinctBy = Wrap<(StringEqCI!("BY"), Space), List, Space>;

    pub type List = Parenthesized<VecSep<Scalx, ArgSep>>;

    pub type SQLForEachClause = Prefix<(Space, StringEqCI!("%FOREACH"), Space), List>;

    #[derive(Grammar)]
    #[tygr(name = "window")]
    pub struct Window {
        pub fun: WindowFunc,
        pub arg: Parenthesized<Option<Either<StringEq!("*"), VecSep<Scalx, ArgSep>>>>,
        pub over_kw: (Space, StringEqCI!("OVER"), Space),
        pub open: (StringEq!("("), Space),
        pub partition: Option<WindowPartition>,
        pub order: Option<Order>,
        pub frame: Option<WindowRows>,
        pub close: (Space, StringEq!(")")),
    }

    #[derive(Grammar)]
    #[tygr(name = "window func")]
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
        VecSep<Scalx, ArgSep>,
    >;

    #[derive(Grammar)]
    #[tygr(name = "window rows")]
    pub enum WindowRows {
        #[sugar]
        Single((Space, StringEqCI!("ROWS"), Space), WindowRowsBound),
        Between {
            rows_kw: (Space, StringEqCI!("ROWS"), Space),
            between_kw: StringEqCI!("BETWEEN"),
            start: (Space, WindowRowsBound),
            end: AndClause<WindowRowsBound>,
        },
    }

    /// `ROWS n PRECEDING` is `ROWS BETWEEN n PRECEDING AND CURRENT ROW`, the standard's lone-bound reading.
    impl Desugar for WindowRowsSugar<'_> {
        type Core = core::WindowRows;
        fn desugar(&self) -> Self::Core {
            let Self::Single(rows_kw, bound) = self;
            let current_row = core::WindowRowsBound::CurrentRow(
                Default::default(),
                Space::blank(),
                Default::default(),
            );
            core::WindowRows::Between {
                rows_kw: rows_kw.desugar(),
                between_kw: Default::default(),
                start: (Space::blank(), bound.desugar()),
                end: Prefix::new(
                    (Space::blank(), Default::default(), Space::blank()),
                    current_row,
                ),
            }
        }
    }

    #[derive(Grammar)]
    #[tygr(name = "window rows bound")]
    pub enum WindowRowsBound {
        UnboundedPreceding(StringEqCI!("UNBOUNDED"), Space, StringEqCI!("PRECEDING")),
        UnboundedFollowing(StringEqCI!("UNBOUNDED"), Space, StringEqCI!("FOLLOWING")),
        Preceding(Number, Space, StringEqCI!("PRECEDING")),
        Following(Number, Space, StringEqCI!("FOLLOWING")),
        CurrentRow(StringEqCI!("CURRENT"), Space, StringEqCI!("ROW")),
    }
    #[derive(Grammar)]
    #[tygr(name = "prdct")]
    pub struct Prdct(pub VecSep<And, (Space, OrOperator, Space)>);

    #[derive(Grammar)]
    #[tygr(name = "or operator")]
    pub enum OrOperator {
        Or(StringEqCI!("OR")),
        #[sugar]
        Exclamation(StringEq!("!")),
    }

    /// `!` is `OR`.
    impl Desugar for OrOperatorSugar<'_> {
        type Core = core::OrOperator;
        fn desugar(&self) -> Self::Core {
            core::OrOperator::Or(Default::default())
        }
    }

    #[derive(Grammar)]
    #[tygr(name = "and")]
    pub struct And(pub VecSep<Src<Cond>, (Space, AndOperator, Space)>);

    #[derive(Grammar)]
    #[tygr(name = "and operator")]
    pub enum AndOperator {
        And(StringEqCI!("AND")),
        #[sugar]
        Ampersand(StringEq!("&")),
    }

    /// `&` is `AND`.
    impl Desugar for AndOperatorSugar<'_> {
        type Core = core::AndOperator;
        fn desugar(&self) -> Self::Core {
            core::AndOperator::And(Default::default())
        }
    }

    pub type NoIndex = Option<(StringEqCI!("%NOINDEX"), Space)>;

    pub type NotKw = StringEqCI!("NOT");

    /// A predicate's keyword or its negation, the keyword under the `NOT` the condition's impl adds.
    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum Negatable<K: Grammar> {
        Plain(K),
        #[sugar]
        Negated(NotKw, Space, K),
    }

    /// A negated keyword is the keyword; the condition's impl puts the `NOT` around the predicate.
    impl<K: Grammar + Desugar> Desugar for NegatableSugar<'_, K>
    where
        K::Core: Grammar,
    {
        type Core = core::Negatable<K::Core>;
        fn desugar(&self) -> Self::Core {
            let Self::Negated(_, _, keyword) = self;
            core::Negatable::Plain(keyword.desugar())
        }
    }

    /// A keyword or clause read the same when omitted: spelled, or omitted and read as its default.
    #[derive(Grammar)]
    #[tygr(inline)]
    #[sugar]
    pub enum Implied<K: Grammar + Default> {
        Spelled(Space, K),
        #[sugar]
        Omitted(()),
    }

    /// An omitted keyword stands as spelled, after a blank.
    impl<K: Grammar + Default + Desugar> Desugar for Implied<K>
    where
        K::Core: Grammar + Default,
    {
        type Core = core::Implied<K::Core>;
        fn desugar(&self) -> Self::Core {
            match self {
                Implied::Spelled(sp, keyword) => {
                    core::Implied::Spelled(sp.desugar(), keyword.desugar())
                }
                Implied::Omitted(_) => {
                    core::Implied::Spelled(Space::blank(), <K::Core as Default>::default())
                }
            }
        }
    }

    /// An operator and a quantifier against a subquery; `IN` is `= ANY`, a `NOT IN` under the condition's NOT.
    #[derive(Grammar)]
    #[tygr(name = "quantified op")]
    pub enum QuantifiedOp {
        Op {
            op: CmpOp,
            sp: Space,
            quantifier: Quantifier,
        },
        #[sugar]
        In(Negatable<StringEqCI!("IN")>),
    }

    /// `IN (subquery)` is `= ANY (subquery)`; a `NOT IN` is `NOT (... = ANY ...)` by the condition's impl.
    impl Desugar for QuantifiedOpSugar<'_> {
        type Core = core::QuantifiedOp;
        fn desugar(&self) -> Self::Core {
            core::QuantifiedOp::Op {
                op: core::CmpOp::Eq(Default::default()),
                sp: Space::blank(),
                quantifier: core::Quantifier::Any(Default::default()),
            }
        }
    }

    #[derive(Grammar)]
    #[tygr(name = "cond")]
    #[sugar]
    pub enum Cond {
        Not {
            not_kw: (StringEqCI!("NOT"), Space),
            condition: Box<Src<Cond>>,
        },
        Paren {
            open: (StringEq!("("), Space),
            predicate: Box<Prdct>,
            close: (Space, StringEq!(")")),
        },
        Predicate {
            noindex: NoIndex,
            predicate: Predicate,
        },
    }

    /// A negated predicate is `NOT (predicate)`: NOT BETWEEN, NOT IN, NOT LIKE, NOT %MATCHES, NOT %INLIST,
    /// IS NOT NULL, IS NOT JSON and NOT IN (subquery), as NOT (= ANY), each measured on 2026.2 with NULL
    /// rows; NOT[ and NOT] stay as written, a quantified one being no NOT (... ALL ...) on IRIS.
    impl Desugar for Cond {
        type Core = core::Cond;
        fn desugar(&self) -> Self::Core {
            let (noindex, predicate) = match self {
                Cond::Not { not_kw, condition } => {
                    return core::Cond::Not {
                        not_kw: not_kw.desugar(),
                        condition: condition.desugar(),
                    };
                }
                Cond::Paren {
                    open,
                    predicate,
                    close,
                } => {
                    return core::Cond::Paren {
                        open: open.desugar(),
                        predicate: predicate.desugar(),
                        close: close.desugar(),
                    };
                }
                Cond::Predicate { noindex, predicate } => (noindex, predicate),
            };
            let plain = core::Cond::Predicate {
                noindex: noindex.desugar(),
                predicate: predicate.desugar(),
            };
            let Some(not_kw) = negation(predicate) else {
                return plain;
            };
            core::Cond::Not {
                not_kw: (not_kw, Space::blank()),
                condition: Box::new(Src::synthesized(core::Cond::Paren {
                    open: (Default::default(), Space::default()),
                    predicate: Box::new(core::Prdct(VecSep::one(core::And(VecSep::one(
                        Src::synthesized(plain),
                    ))))),
                    close: (Space::default(), Default::default()),
                })),
            }
        }
    }

    /// The `NOT` of the predicate's negated keyword.
    fn negation(predicate: &Predicate) -> Option<NotKw> {
        use Comparison::*;
        let Predicate::Comparison { tail, .. } = predicate else {
            return None;
        };
        match tail {
            IsNull { null_kw, .. } => negated(null_kw),
            Between { between_kw, .. } => negated(between_kw),
            Like { like_kw, .. } => negated(like_kw),
            Matches { matches_kw, .. } => negated(matches_kw),
            In { in_kw, .. } => negated(in_kw),
            InList { inlist_kw, .. } => negated(inlist_kw),
            IsJSON { json_kw, .. } => negated(json_kw),
            Query {
                op: QuantifiedOp::In(in_kw),
                ..
            } => negated(in_kw),
            _ => None,
        }
    }

    fn negated<K: Grammar>(keyword: &Negatable<K>) -> Option<NotKw> {
        match keyword {
            Negatable::Negated(not, ..) => Some(*not),
            Negatable::Plain(_) => None,
        }
    }

    /// What a `%NOINDEX` hint may precede (measured on 2026.2: not a `NOT` or a parenthesis).
    #[derive(Grammar)]
    #[tygr(name = "predicate")]
    pub enum Predicate {
        Exists {
            exists_kw: (StringEqCI!("EXISTS"), Space),
            query: SQLSubquery,
        },
        ForSomeElement {
            kw: (
                StringEqCI!("FOR"),
                Space,
                StringEqCI!("SOME"),
                Space,
                StringEqCI!("%ELEMENT"),
                Space,
            ),
            column: Parenthesized<Colmn>,
            label: Option<(Space, Option<(StringEqCI!("AS"), Space)>, Ident)>,
            predicate: (Space, Parenthesized<Box<Prdct>>),
        },
        ForSome {
            kw: (StringEqCI!("FOR"), Space, StringEqCI!("SOME"), Space),
            tables: Parenthesized<Tables>,
            condition: Option<(Space, Box<ForCnd>)>,
        },
        Comparison {
            lhs: Scalx,
            sp: Space,
            tail: Comparison,
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

    pub type AndClause<T> = Prefix<(Space, StringEqCI!("AND"), Space), T>;

    pub type Parenthesized<T> = Wrap<(StringEq!("("), Space), T, (Space, StringEq!(")"))>;

    #[doc(hidden)]
    #[derive(Grammar)]
    #[tygr(name = "comparison")]
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
            null_kw: Negatable<StringEqCI!("NULL")>,
        },
        Between {
            between_kw: Negatable<(StringEqCI!("BETWEEN"), Space)>,
            lower: Scalx,
            and_kw: (Space, StringEqCI!("AND"), Space),
            upper: Scalx,
        },
        Like {
            like_kw: Negatable<(StringEqCI!("LIKE"), Space)>,
            pattern: Scalx,
            escape: Option<EscapeScalx>,
        },
        Matches {
            matches_kw: Negatable<(StringEqCI!("%MATCHES"), Space)>,
            pattern: Scalx,
            escape: Option<EscapeScalx>,
        },
        Query {
            op: QuantifiedOp,
            sp: Space,
            expr: SQLSubquery,
        },
        In {
            in_kw: Negatable<(StringEqCI!("IN"), Space)>,
            list: Parenthesized<VecSep<Scalx, ArgSep>>,
        },
        InList {
            inlist_kw: Negatable<(StringEqCI!("%INLIST"), Space)>,
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
            json_kw: Negatable<StringEqCI!("JSON")>,
            constraint: Option<JSONPredicateTypeConstraint>,
        },
        Contains {
            kw: Either<StringEqCI!("%CONTAINSTERM"), StringEqCI!("%CONTAINS")>,
            list: Args1Plus,
        },
        Scalx {
            op: CmpOp,
            space: Space,
            expr: Scalx,
        },
    }

    pub type EscapeScalx = Prefix<(Space, StringEqCI!("ESCAPE"), Space), Scalx>;

    pub type SQLSubquery = Parenthesized<Box<Query>>;

    #[derive(Grammar)]
    #[tygr(name = "quantifier")]
    pub enum Quantifier {
        All(StringEqCI!("ALL")),
        Any(StringEqCI!("ANY")),
        #[sugar]
        Some(StringEqCI!("SOME")),
    }

    /// `SOME` is `ANY`.
    impl Desugar for QuantifierSugar<'_> {
        type Core = core::Quantifier;
        fn desugar(&self) -> Self::Core {
            core::Quantifier::Any(Default::default())
        }
    }

    #[derive(Grammar)]
    #[tygr(name = "size hint")]
    pub enum SizeHint {
        Paren {
            kw: (Space, StringEqCI!("SIZE"), Space),
            open: StringEq!("("),
            size: PConst,
            close: StringEq!(")"),
        },
        #[sugar]
        Bare {
            kw: (Space, StringEqCI!("SIZE"), Space),
            size: Literal,
        },
    }

    /// `SIZE n` is `SIZE (n)`.
    impl Desugar for SizeHintSugar<'_> {
        type Core = core::SizeHint;
        fn desugar(&self) -> Self::Core {
            let Self::Bare { kw, size } = self;
            core::SizeHint::Paren {
                kw: kw.desugar(),
                open: Default::default(),
                size: core::PConst::Literal(size.desugar()),
                close: Default::default(),
            }
        }
    }

    #[derive(Grammar)]
    pub enum PConst {
        #[sugar]
        Nested(StringEq!("("), Box<PConst>, StringEq!(")")),
        Literal(Literal),
    }

    /// `SIZE ((n))` keeps one pair of parentheses: the ones within go.
    impl Desugar for PConstSugar<'_> {
        type Core = core::PConst;
        fn desugar(&self) -> Self::Core {
            let Self::Nested(_, inner, _) = self;
            (***inner).desugar()
        }
    }

    #[derive(Grammar)]
    #[tygr(name = "JSON representation")]
    pub struct JSONRepresentation(pub StringEqCI!("JSON"));

    pub type JSONInputClause = Prefix<(StringEqCI!("FORMAT"), Space), (JSONRepresentation, Space)>;

    #[derive(Grammar)]
    #[tygr(name = "JSON predicate type constraint")]
    pub enum JSONPredicateTypeConstraint {
        Value((Space, StringEqCI!("VALUE"))),
        Array((Space, StringEqCI!("ARRAY"))),
        Object((Space, StringEqCI!("OBJECT"))),
        Scalar((Space, StringEqCI!("SCALAR"))),
    }

    #[derive(Grammar)]
    #[tygr(name = "with list")]
    pub struct WithList(pub VecSep<Src<WithListElement>, ArgSep>);

    pub type WithClause = Prefix<(StringEqCI!("WITH"), Space), WithList>;

    #[derive(Grammar)]
    #[tygr(name = "with list element")]
    pub struct WithListElement {
        pub name: Ident,
        pub columns: Option<(Space, FList1)>,
        pub as_kw: (Space, StringEqCI!("AS"), Space),
        pub query: SQLSubquery,
    }

    #[derive(Grammar)]
    #[tygr(name = "column name")]
    pub struct ColumnName(pub Ident);

    #[derive(Grammar)]
    pub struct Selex {
        pub kw: (StringEqCI!("SELECT"), Space),
        pub options: Vec<SelectOption>,
        #[sugar]
        pub all: Option<AllKw>,
        pub selst: Selst,
        pub from: Option<FromClause>,
        pub where_clause: Option<WhereClause>,
        pub group: Option<Group>,
        pub having: Option<Having>,
        pub order: Option<Order>,
    }

    #[derive(Grammar)]
    #[tygr(name = "select option")]
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

    /// `ALL` where `DISTINCT` could stand: what a SELECT or an aggregate does without it.
    pub type AllKw = (StringEqCI!("ALL"), NotFollowedBy<IdentifierPart>, Space);

    pub type SQLTopClause = Wrap<(StringEqCI!("TOP"), Space1), TopSpec, Space>;

    #[derive(Grammar)]
    pub enum TopSpec {
        ValSpec(DTSValSpec),
        All(StringEqCI!("ALL")),
    }

    #[derive(Grammar)]
    #[tygr(name = "selst")]
    pub struct Selst {
        pub distinct: Option<Dist>,
        pub top: Option<SQLTopClause>,
        pub select_list: SelectList,
        pub targt: Targt,
    }

    #[derive(Grammar)]
    #[tygr(name = "select list")]
    pub struct SelectList(pub VecSep<Src<SelectItem>, ArgSep>);

    #[derive(Grammar)]
    #[tygr(name = "select item")]
    pub enum SelectItem {
        Star(StringEq!("*")),
        Expr { expr: Scalx, alias: Option<Alias> },
    }

    // The blank stays inside the alias, so that a reference without one consumes nothing after it.
    #[derive(Grammar)]
    pub struct Alias {
        pub as_kw: Implied<StringEqCI!("AS")>,
        pub sp: Space,
        pub name: SQLIdentNonKeyword,
    }

    pub type Into = Prefix<(StringEqCI!("INTO"), Space), HostList>;

    #[derive(Grammar)]
    #[tygr(name = "targt")]
    pub struct Targt(pub Option<(Space, TargtEnum)>);

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum TargtEnum {
        Into(Into),
        IntoList(IntoList),
    }

    #[derive(Grammar)]
    #[tygr(name = "host list")]
    pub struct HostList(pub VecSep<Param, ArgSep>);

    pub type IntoList = Prefix<StringEqCI!("%INTOLIST"), (Space, Param)>;

    #[derive(Grammar)]
    #[tygr(name = "from")]
    pub struct FromClause {
        pub keyword: (Space, StringEqCI!("FROM"), Space),
        pub options: Vec<FromOpt>,
        pub tables: Tables,
    }

    #[derive(Grammar)]
    #[tygr(name = "from_opt")]
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
    pub struct TableRefs(pub VecSep<TableRef, ArgSep>);

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
    pub struct IndexRefs(pub VecSep<IndexRef, ArgSep>);

    #[derive(Grammar)]
    pub struct IndexRef {
        pub table: Vec<(Ident, Space, StringEq!("."), Space)>,
        pub index: IndexSpec,
    }

    #[derive(Grammar)]
    pub enum IndexSpec {
        Name(IndexName),
        All(StringEq!("*")),
    }

    #[derive(Grammar)]
    #[tygr(name = "tables")]
    #[sugar]
    pub struct Tables {
        pub first: Tref,
        #[sugar]
        pub rest: Vec<(ArgSep, Tref)>,
    }

    /// `FROM a, b` is `FROM a CROSS JOIN b`, each join at its reference's range, in parentheses when it has joins.
    impl Desugar for Tables {
        type Core = core::Tables;
        fn desugar(&self) -> Self::Core {
            let core::Tref::Join(mut first) = self.first.desugar();
            for (_, tref) in &self.rest {
                let (Tref::Join(jtab) | Tref::OJSyntax { table: jtab, .. }) = tref;
                let core::Tref::Join(mut table) = tref.desugar();
                let start = jtab.formit.range().start;
                let end = jtab
                    .joins
                    .last()
                    .map_or(jtab.formit.range().end, |(_, join)| join.range().end);
                if !table.joins.is_empty() {
                    table = core::JTab {
                        formit: Src::new(
                            start..end,
                            core::Frmit::JTab {
                                open: (Default::default(), Space::default()),
                                joined: Box::new(table),
                                close: (Space::default(), Default::default()),
                            },
                        ),
                        joins: Vec::new(),
                    };
                }
                let cross = core::Join::Cross {
                    cross_kw: (
                        Default::default(),
                        Space::blank(),
                        Default::default(),
                        Space::blank(),
                    ),
                    table: Box::new(table),
                };
                first
                    .joins
                    .push((Space::blank(), Src::new(start..end, cross)));
            }
            core::Tables {
                first: core::Tref::Join(first),
            }
        }
    }

    #[sugar]
    impl Tables {
        pub fn iter(&self) -> impl Iterator<Item = &Tref> {
            std::iter::once(&self.first).chain(self.rest.iter().map(|(_, tref)| tref))
        }
    }

    #[derive(Grammar)]
    #[tygr(name = "tref")]
    pub enum Tref {
        Join(JTab),
        #[sugar]
        OJSyntax {
            open: StringEq!("{"),
            oj: (Ident, Space),
            table: JTab,
            close: (Space, StringEq!("}")),
        },
    }

    /// `{oj ...}` is the join inside it.
    impl Desugar for TrefSugar<'_> {
        type Core = core::Tref;
        fn desugar(&self) -> Self::Core {
            let Self::OJSyntax { table, .. } = self;
            core::Tref::Join(table.desugar())
        }
    }

    #[derive(Grammar)]
    #[tygr(name = "jtab")]
    pub struct JTab {
        pub formit: Src<Frmit>,
        pub joins: Vec<(Space, Src<Join>)>,
    }

    #[derive(Grammar)]
    pub enum Join {
        Cross {
            cross_kw: (StringEqCI!("CROSS"), Space, StringEqCI!("JOIN"), Space),
            table: Box<JTab>,
        },
        Natural {
            natural_kw: (StringEqCI!("NATURAL"), Space),
            join_type: JOp,
            space: Space,
            table: Box<JTab>,
        },
        Qualified {
            join_type: JOp,
            space: Space,
            table: Box<JTab>,
            spec: JSpec,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "jop")]
    pub enum JOp {
        Typed {
            jtype: JType,
            sp: Space,
            join_kw: StringEqCI!("JOIN"),
        },
        #[sugar]
        Bare(StringEqCI!("JOIN")),
    }

    /// `JOIN` is `INNER JOIN`.
    impl Desugar for JOpSugar<'_> {
        type Core = core::JOp;
        fn desugar(&self) -> Self::Core {
            let Self::Bare(join_kw) = self;
            core::JOp::Typed {
                jtype: core::JType::Inner(Default::default()),
                sp: Space::blank(),
                join_kw: join_kw.desugar(),
            }
        }
    }

    #[derive(Grammar)]
    pub enum JType {
        Inner(StringEqCI!("INNER")),
        Outer(Side, Implied<StringEqCI!("OUTER")>),
    }

    #[derive(Grammar)]
    pub enum Side {
        Left(StringEqCI!("LEFT")),
        Right(StringEqCI!("RIGHT")),
        Full(StringEqCI!("FULL")),
    }

    #[derive(Grammar)]
    pub enum JSpec {
        On {
            on_kw: (Space, StringEqCI!("ON"), Space),
            condition: Prdct,
        },
        Using {
            using_kw: (Space, StringEqCI!("USING"), Space),
            flist1: FList1,
        },
    }

    #[derive(Grammar)]
    pub enum Frmit {
        JSONTable {
            json_table: JSONTable,
            alias: Option<Alias>,
        },
        TVF {
            function: TVF,
            alias: Option<Alias>,
        },
        Table {
            name: Table,
            alias: Option<Alias>,
        },
        Query {
            lateral: Option<(StringEqCI!("LATERAL"), Space)>,
            query: SQLSubquery,
            alias: Option<Alias>,
        },
        JTab {
            open: (StringEq!("("), Space),
            joined: Box<JTab>,
            close: (Space, StringEq!(")")),
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "json_table")]
    pub struct JSONTable {
        pub kw: StringEq!("JSON_TABLE"),
        pub sp1: Space,
        pub open: StringEq!("("),
        pub sp2: Space,
        pub common_syntax: JSONApiCommonSyntax,
        pub sp3: Space,
        pub columns_spec: JSONTableColumnsSpec,
        pub sp4: Space,
        pub close: StringEq!(")"),
    }

    #[derive(Grammar)]
    #[tygr(name = "json_api_common_syntax")]
    pub struct JSONApiCommonSyntax {
        pub context_item: StringLiteral,
        pub sp1: Space,
        pub comma: StringEq!(","),
        pub sp2: Space,
        pub path_spec: StringLiteral,
    }

    #[derive(Grammar)]
    #[tygr(name = "json_table_columns_spec")]
    pub enum JSONTableColumnsSpec {
        Columns(JSONTableColumnsClause),
        Type(JSONTableTypeClause),
    }

    #[derive(Grammar)]
    #[tygr(name = "json_table_columns_clause")]
    pub struct JSONTableColumnsClause {
        pub kw: StringEq!("COLUMNS"),
        pub sp1: Space,
        pub open: StringEq!("("),
        pub sp2: Space,
        pub columns: VecSep<JSONTableRegularColumnDefinition, ArgSep>,
        pub sp3: Space,
        pub close: StringEq!(")"),
    }

    #[derive(Grammar)]
    #[tygr(name = "json_table_regular_column_definition")]
    pub struct JSONTableRegularColumnDefinition {
        pub name: Ident,
        pub sp1: Space,
        pub data_type: Datyp,
        pub path_clause: Option<(Space, StringEq!("PATH"), Space, StringLiteral)>,
    }

    #[derive(Grammar)]
    #[tygr(name = "json_table_type_clause")]
    pub struct JSONTableTypeClause {
        pub kw: StringEq!("%TYPE"),
        pub sp1: Space,
        pub type_name: StringLiteral,
    }

    #[derive(Grammar)]
    pub struct TVF {
        pub table: Table,
        pub open: (StringEq!("("), Space),
        pub args: Option<VecSep<DTSValSpec, ArgSep>>,
        pub close: (Space, StringEq!(")")),
    }

    pub type WhereClause = Prefix<(Space, StringEqCI!("WHERE"), Space), Prdct>;

    pub type Group = Prefix<
        (Space, StringEqCI!("GROUP"), Space, StringEqCI!("BY"), Space),
        VecSep<Scalx, ArgSep>,
    >;

    pub type Having = Prefix<(Space, StringEqCI!("HAVING"), Space), Prdct>;

    pub type Order = Prefix<
        (Space, StringEqCI!("ORDER"), Space, StringEqCI!("BY"), Space),
        VecSep<Src<Ordir>, ArgSep>,
    >;

    #[derive(Grammar)]
    pub struct Ordir {
        pub expr: Ordit,
        #[sugar]
        pub asc: Option<(
            Space,
            StringEqCI!("ASC"),
            NotFollowedBy<(Space, StringEqCI!("DESC"))>,
        )>,
        pub desc: Option<(Space, StringEqCI!("DESC"))>,
    }

    #[derive(Grammar)]
    pub enum Ordit {
        Literal(Literal),
        Scalx(Scalx),
    }

    #[derive(Grammar)]
    #[tygr(name = "row limiting clause")]
    pub enum RowLimitingClause {
        LimitOffset {
            limit: Limit,
            offset: Option<Offset>,
        },
        #[sugar]
        OffsetFetch {
            offset: Offset,
            fetch: Option<Fetch>,
        },
        #[sugar]
        FetchOnly(Fetch),
    }

    /// `OFFSET m [ROWS] [FETCH … n …]` is `LIMIT n OFFSET m`, `LIMIT ALL OFFSET m` without the FETCH, and
    /// `FETCH [FIRST|NEXT] n [ROWS] [ONLY]` alone `LIMIT n`.
    impl Desugar for RowLimitingClauseSugar<'_> {
        type Core = core::RowLimitingClause;
        fn desugar(&self) -> Self::Core {
            let count = |fetch: &Fetch| core::LimitSpec::Val(fetch.valspec.desugar());
            let (limit, offset) = match self {
                Self::OffsetFetch { offset, fetch } => (
                    fetch.as_ref().map_or_else(
                        || core::LimitSpec::All(Default::default(), Default::default()),
                        count,
                    ),
                    Some(core::Offset {
                        kw: (Space::blank(), Default::default(), Space::blank()),
                        count: offset.count.desugar(),
                    }),
                ),
                Self::FetchOnly(fetch) => (count(fetch), None),
            };
            core::RowLimitingClause::LimitOffset {
                limit: Prefix::new((Space::blank(), Default::default(), Space::blank()), limit),
                offset,
            }
        }
    }

    pub type Limit = Prefix<(Space, StringEqCI!("LIMIT"), Space), LimitSpec>;

    #[derive(Grammar)]
    #[tygr(name = "limit spec")]
    pub enum LimitSpec {
        All(StringEqCI!("ALL"), NotFollowedBy<IdentifierPart>),
        Val(RowCount),
    }

    #[derive(Grammar)]
    #[tygr(name = "row count")]
    pub enum RowCount {
        Val(ValSpec),
        Scalx(Scalx),
    }

    #[derive(Grammar)]
    pub struct Offset {
        pub kw: (Space, StringEqCI!("OFFSET"), Space),
        pub count: RowCount,
        #[sugar]
        pub rows: Option<(Space, RowRowsInline)>,
    }

    #[sugar]
    #[derive(Grammar)]
    pub struct Fetch {
        pub keyword: (Space, StringEqCI!("FETCH"), Space),
        pub direction: Option<(Either<StringEqCI!("FIRST"), StringEqCI!("NEXT")>, Space)>,
        pub valspec: RowCount,
        pub row_keyword: Option<(Space, RowRowsInline)>,
        pub only: Option<(Space, StringEqCI!("ONLY"))>,
    }

    #[sugar]
    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum RowRowsInline {
        Rows(StringEqCI!("ROWS")),
        Row(StringEqCI!("ROW")),
    }

    #[derive(Grammar)]
    pub struct Query(pub VecSep<QTerm, (Space, QueryOp, Space)>);

    #[derive(Grammar)]
    #[tygr(name = "query op")]
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
        Query(SQLSubquery),
        Selex(Src<Selex>),
    }
    #[derive(Grammar)]
    #[tygr(validated)]
    pub struct Insrt {
        pub insert_kw: StringEqCI!("INSERT"),
        pub or_update: Option<(Space, StringEqCI!("OR"), Space, StringEqCI!("UPDATE"))>,
        pub options: Vec<DmlOption>,
        pub into_kw: Implied<StringEqCI!("INTO")>,
        pub target: (Space, IUDTarget),
        pub values: (Space, InsertValues),
    }

    impl Validate for Insrt {
        const REQUIREMENT: &'static str = "not take %ILOCK";
        fn validate(&self) -> bool {
            !self
                .options
                .iter()
                .any(|option| matches!(option, DmlOption::ILock(_)))
        }
    }

    /// One option list for INSERT, UPDATE, DELETE and TRUNCATE TABLE, each validated against its own set.
    #[derive(Grammar)]
    #[tygr(name = "dml option")]
    pub enum DmlOption {
        NoCheck((Space, StringEqCI!("%NOCHECK"))),
        NoFPlan((Space, StringEqCI!("%NOFPLAN"))),
        NoLock((Space, StringEqCI!("%NOLOCK"))),
        NoIndex((Space, StringEqCI!("%NOINDEX"))),
        NoTrigger((Space, StringEqCI!("%NOTRIGGER"))),
        NoStream((Space, StringEqCI!("%NOSTREAM"))),
        NoJourn((Space, StringEqCI!("%NOJOURN"))),
        ILock((Space, StringEqCI!("%ILOCK"))),
        ObjectFiler((Space, StringEqCI!("%OBJECTFILER"))),
        ProfileAll((Space, StringEqCI!("%PROFILE_ALL"))),
        Profile((Space, StringEqCI!("%PROFILE"))),
        NoRuntime((Space, StringEqCI!("%NORUNTIME"))),
        DoRuntime((Space, StringEqCI!("%DORUNTIME"))),
    }

    #[derive(Grammar)]
    #[tygr(name = "iud_target")]
    pub enum IUDTarget {
        Table {
            name: Table,
            alias: Option<Alias>,
        },
        Subquery {
            query: SQLSubquery,
            alias: Option<Alias>,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "insert values")]
    pub enum InsertValues {
        #[sugar]
        SetList {
            set_kw: StringEqCI!("SET"),
            assignments: (Space, VecSep<Src<SQLSetAssignment>, ArgSep>),
        },
        ColumnList {
            columns: Option<SQLColumnListParen>,
            values: (Space, ValueSource),
        },
        DefaultValues(StringEqCI!("DEFAULT"), Space, StringEqCI!("VALUES")),
    }

    /// `INSERT ... SET a = x, b = y` is `INSERT ... (a, b) VALUES (x, y)`.
    impl Desugar for InsertValuesSugar<'_> {
        type Core = core::InsertValues;
        fn desugar(&self) -> Self::Core {
            let Self::SetList {
                assignments: (_, list),
                ..
            } = self;
            let columns = list.items.iter().map(|a| a.column.desugar()).collect();
            let values = list.items.iter().map(|a| a.value.desugar()).collect();
            core::InsertValues::ColumnList {
                columns: Some(Wrap::new(
                    (Space::default(), Default::default(), Space::default()),
                    commas(columns),
                    (Space::default(), Default::default()),
                )),
                values: (
                    Space::blank(),
                    core::ValueSource::ValSpec {
                        values_kw: Default::default(),
                        sp: Space::blank(),
                        spec: core::ValSpec::Scalx {
                            open: (Default::default(), Space::default()),
                            exprs: commas(values),
                            close: (Space::default(), Default::default()),
                        },
                    },
                ),
            }
        }
    }

    #[derive(Grammar)]
    #[sugar]
    pub enum Asign {
        SetList {
            set_kw: StringEqCI!("SET"),
            assignments: (Space, VecSep<Src<SQLSetAssignment>, ArgSep>),
        },
        ColumnList {
            columns: Option<SQLColumnListParen>,
            values: (Space, ValueSource),
        },
    }

    /// `UPDATE t (a, b) VALUES (x, y)` is `UPDATE t SET a = x, b = y`; a query or host array as values stays.
    impl Desugar for Asign {
        type Core = core::Asign;
        fn desugar(&self) -> Self::Core {
            match self {
                Asign::SetList {
                    set_kw,
                    assignments,
                } => core::Asign::SetList {
                    set_kw: *set_kw,
                    assignments: assignments.desugar(),
                },
                Asign::ColumnList {
                    columns: Some(columns),
                    values:
                        (
                            _,
                            ValueSource::ValSpec {
                                spec: ValSpec::Scalx { exprs, .. },
                                ..
                            },
                        ),
                } if columns.wrapped.items.len() == exprs.items.len() => {
                    let items = (columns.wrapped.items.iter().zip(&exprs.items))
                        .map(|(column, value)| {
                            let Scalx::Legacy(terms) = value;
                            let last = terms.items.len() - 1;
                            let range = terms.items[0].range().start..terms.items[last].range().end;
                            Src::new(
                                range,
                                core::SQLSetAssignment {
                                    column: column.desugar(),
                                    eq: (Space::blank(), Default::default(), Space::blank()),
                                    value: value.desugar(),
                                },
                            )
                        })
                        .collect();
                    core::Asign::SetList {
                        set_kw: Default::default(),
                        assignments: (Space::blank(), commas(items)),
                    }
                }
                Asign::ColumnList { columns, values } => core::Asign::ColumnList {
                    columns: columns.desugar(),
                    values: values.desugar(),
                },
            }
        }
    }

    pub type FList1 = Parenthesized<VecSep<Ident, ArgSep>>;

    pub type SQLColumnListParen =
        Wrap<(Space, StringEq!("("), Space), VecSep<Colmn, ArgSep>, (Space, StringEq!(")"))>;

    #[derive(Grammar)]
    #[tygr(name = "value source")]
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
    #[tygr(name = "val spec")]
    pub enum ValSpec {
        Scalx {
            open: (StringEq!("("), Space),
            exprs: VecSep<Scalx, ArgSep>,
            close: (Space, StringEq!(")")),
        },
        Var {
            var: Var,
            subs: SubList,
        },
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLSetAssignment {
        pub column: Colmn,
        pub eq: (Space, StringEq!("="), Space),
        pub value: Scalx,
    }

    #[derive(Grammar)]
    #[tygr(name = "updat")]
    pub enum Updat {
        UpdateStats {
            update_kw: StringEqCI!("UPDATE"),
            stats_kw: (Space, StringEqCI!("STATISTICS"), Space),
            table: TName,
        },
        Update {
            update_kw: StringEqCI!("UPDATE"),
            options: Vec<DmlOption>,
            target: (Space, IUDTarget),
            assignment: (Space, Asign),
            from: Option<FromClause>,
            where_clause: Option<WhereWerop>,
        },
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct WhereWerop {
        pub sp1: Space,
        pub kw: StringEqCI!("WHERE"),
        pub sp2: Space,
        pub werop: Werop,
    }

    #[derive(Grammar)]
    pub enum Werop {
        Prdct(Prdct),
        CurrentOf {
            current_of: (StringEqCI!("CURRENT"), Space, StringEqCI!("OF"), Space),
            ident: Ident,
            prdct: Option<AndClause<Prdct>>,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "delet", validated)]
    pub struct Delet {
        pub delete_kw: StringEqCI!("DELETE"),
        pub options: Vec<DmlOption>,
        pub from_kw: Implied<StringEqCI!("FROM")>,
        pub target: (Space, IUDTarget),
        pub where_clause: Option<WhereWerop>,
    }

    impl Validate for Delet {
        const REQUIREMENT: &'static str = "not take %NOSTREAM or %ILOCK";
        fn validate(&self) -> bool {
            !self
                .options
                .iter()
                .any(|option| matches!(option, DmlOption::NoStream(_) | DmlOption::ILock(_)))
        }
    }

    #[derive(Grammar)]
    #[tygr(name = "truncate", validated)]
    pub struct Truncate {
        pub truncate_kw: StringEqCI!("TRUNCATE"),
        pub table_kw: (Space, StringEqCI!("TABLE"), Space),
        pub options: Vec<DmlOption>,
        pub table: (Space, TName),
    }

    impl Validate for Truncate {
        const REQUIREMENT: &'static str = "take only %NOCHECK, %NOLOCK and %NOJOURN";
        fn validate(&self) -> bool {
            self.options.iter().all(|option| {
                matches!(
                    option,
                    DmlOption::NoCheck(_) | DmlOption::NoLock(_) | DmlOption::NoJourn(_)
                )
            })
        }
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum TpRelated {
        StartTransaction(StartTP),
        SetTransaction(SetTP),
        Rollback(RollB),
        TPOption(TP),
    }

    #[derive(Grammar)]
    #[tygr(name = "starttp")]
    pub struct StartTP {
        pub kw: (
            StringEqCI!("START"),
            Space,
            StringEqCI!("TRANSACTION"),
            Space,
        ),
        pub commit_mode: Option<(CommitModeSpec, Space)>,
        pub modes: VecSep<Option<TRMode>, ArgSep>,
    }

    #[derive(Grammar)]
    #[tygr(name = "settp")]
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
            size: Src<Term>,
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
    #[tygr(name = "rollb")]
    pub struct RollB {
        pub rollback_kw: StringEqCI!("ROLLBACK"),
        #[sugar]
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
    #[tygr(name = "tp option")]
    pub enum TPOption {
        InTransaction(StringEqCI!("%INTRANSACTION")),
        BegTrans(StringEqCI!("%BEGTRANS")),
        #[sugar]
        InTrans(StringEqCI!("%INTRANS")),
        Commit {
            commit_kw: StringEqCI!("COMMIT"),
            #[sugar]
            work: Option<(Space, StringEqCI!("WORK"))>,
        },
    }

    /// `%INTRANS` is `%INTRANSACTION`.
    impl Desugar for TPOptionSugar<'_> {
        type Core = core::TPOption;
        fn desugar(&self) -> Self::Core {
            core::TPOption::InTransaction(Default::default())
        }
    }

    #[derive(Grammar)]
    #[tygr(name = "ofc")]
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
    #[tygr(name = "decl")]
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
            statement: WithStat<Selct>,
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
    #[tygr(name = "get")]
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
    #[tygr(name = "call statement")]
    pub struct CallStatement {
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
    #[tygr(name = "routine name")]
    pub struct RoutineName(pub SQLFunctionName);

    pub type SQLArgumentList = ArgsO<VecSep<Scalx, ArgSep>>;

    #[derive(Grammar)]
    #[tygr(name = "explain")]
    pub struct Explain {
        pub explain_kw: StringEqCI!("EXPLAIN"),
        pub options: Option<ExplainOptionList>,
        pub into: Option<(Space, StringEqCI!("INTO"), Space, Param)>,
        pub statement: (Space, Box<ExplainSQLStatement>),
    }

    #[derive(Grammar)]
    #[tygr(name = "explain option list")]
    pub struct ExplainOptionList {
        pub alt: Option<(Space, AltOption)>,
        pub stat: Option<(Space, StatOption)>,
    }

    #[derive(Grammar)]
    #[tygr(name = "alt option")]
    pub enum AltOption {
        Alt(StringEqCI!("ALT")),
        All(StringEqCI!("ALL")),
    }

    #[derive(Grammar)]
    #[tygr(name = "stat option")]
    pub enum StatOption {
        Stat(StringEqCI!("STAT")),
        Stats(StringEqCI!("STATS")),
    }

    #[derive(Grammar)]
    #[tygr(name = "explain sql statement")]
    pub enum ExplainSQLStatement {
        Select(WithStat<Selct>),
        Update(WithStat<Updat>),
        Delete(WithStat<Delet>),
    }

    #[derive(Grammar)]
    #[tygr(name = "selct")]
    #[sugar]
    pub struct Selct {
        pub query: Query,
        pub order: Option<Order>,
        pub row_limiting_clause: Option<RowLimitingClause>,
    }

    /// `LIMIT ALL` alone, which limits nothing, goes.
    impl Desugar for Selct {
        type Core = core::Selct;
        fn desugar(&self) -> Self::Core {
            let unlimited = |clause: &core::RowLimitingClause| {
                matches!(clause, core::RowLimitingClause::LimitOffset { limit, offset: None }
                    if matches!(limit.prefixed, core::LimitSpec::All(..)))
            };
            core::Selct {
                query: self.query.desugar(),
                order: self.order.desugar(),
                row_limiting_clause: (self.row_limiting_clause.desugar())
                    .filter(|clause| !unlimited(clause)),
            }
        }
    }

    #[derive(Grammar)]
    #[tygr(name = "option")]
    pub struct SQLOption {
        pub set_kw: StringEqCI!("SET"),
        pub option_kw: (Space, StringEqCI!("OPTION"), Space),
        pub setting: OptionSetting,
    }

    #[derive(Grammar)]
    #[tygr(name = "compile option")]
    pub struct CompileOption {
        pub compilemode_kw: StringEqCI!("COMPILEMODE"),
        pub eq: (Space, StringEq!("="), Space),
        pub mode: CompileMode,
    }

    #[derive(Grammar)]
    #[tygr(name = "option setting")]
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
    #[tygr(name = "compile mode")]
    pub enum CompileMode {
        Deferred(StringEqCI!("DEFERRED")),
        Immediate(StringEqCI!("IMMEDIATE")),
        Install(StringEqCI!("INSTALL")),
        NoCheck(StringEqCI!("NOCHECK")),
    }

    #[derive(Grammar)]
    #[tygr(name = "TRUE/FALSE")]
    pub enum TrueFalse {
        False(StringEqCI!("FALSE")),
        True(StringEqCI!("TRUE")),
    }

    #[derive(Grammar)]
    #[tygr(name = "locale option")]
    pub struct LocaleOption {
        pub setting: LocaleSetting,
        pub eq: (Space, StringEq!("="), Space),
        pub value: Literal,
    }

    #[derive(Grammar)]
    #[tygr(name = "locale setting")]
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
    #[tygr(name = "setml")]
    pub struct Setml {
        pub set_kw: StringEqCI!("SET"),
        pub ml_kw: (Space, StringEqCI!("ML"), Space),
        pub configuration_kw: (StringEqCI!("CONFIGURATION"), Space),
        pub name: MLConfigurationName,
    }

    #[derive(Grammar)]
    #[tygr(name = "ml_configuration_name")]
    pub struct MLConfigurationName(pub Ident);

    pub type Grant = Prefix<StringEqCI!("GRANT"), (Space, GrantCmd)>;

    #[derive(Grammar)]
    #[tygr(name = "grant cmd")]
    pub enum GrantCmd {
        AdminPrivileges {
            privileges: VecSep<AdminPriv, ArgSep>,
            to_kw: (Space, StringEqCI!("TO"), Space),
            grantees: Grntee,
            with_admin: WithOption<StringEqCI!("ADMIN")>,
        },
        Roles {
            roles: VecSep<Role, ArgSep>,
            to_kw: (Space, StringEqCI!("TO"), Space),
            grantees: Grntee,
            with_admin: WithOption<StringEqCI!("ADMIN")>,
        },
        ObjectPrivileges {
            privileges: SimplePrivs,
            on_kw: (Space, StringEqCI!("ON"), Space),
            objects: Objcts,
            to_kw: (Space, StringEqCI!("TO"), Space),
            grantees: Grntee,
            with_grant: WithOption<StringEqCI!("GRANT")>,
        },
        Actions {
            actions: VecSep<Action, ArgSep>,
            on_kw: (Space, StringEqCI!("ON"), Space),
            object: Object,
            to_kw: (Space, StringEqCI!("TO"), Space),
            grantees: Grntee,
            with_grant: WithOption<StringEqCI!("GRANT")>,
        },
        CubeSelect {
            select_kw: StringEqCI!("SELECT"),
            on_kw: (Space, StringEqCI!("ON"), Space),
            cube_kw: StringEqCI!("CUBE"),
            cubes_suffix: Option<StringEqCI!("S")>,
            cubes: (Space, CubeList),
            to_kw: (Space, StringEqCI!("TO"), Space),
            grantees: Grntee,
            with_grant: WithOption<StringEqCI!("GRANT")>,
        },
    }

    pub type WithOption<K> = Option<(
        Space,
        StringEqCI!("WITH"),
        Space,
        K,
        Space,
        StringEqCI!("OPTION"),
    )>;

    pub type GrantOptionFor = Option<(
        StringEqCI!("GRANT"),
        Space,
        StringEqCI!("OPTION"),
        Space,
        StringEqCI!("FOR"),
        Space,
    )>;

    pub type Revok = Prefix<StringEqCI!("REVOKE"), (Space, RevokeCmd)>;

    #[derive(Grammar)]
    #[tygr(name = "revoke cmd")]
    pub enum RevokeCmd {
        AdminPrivileges {
            privileges: VecSep<AdminPriv, ArgSep>,
            from_kw: (Space, StringEqCI!("FROM"), Space),
            grantees: Grntee,
        },
        Roles {
            roles: VecSep<Role, ArgSep>,
            from_kw: (Space, StringEqCI!("FROM"), Space),
            grantees: Grntee,
        },
        ObjectPrivileges {
            grant_option: GrantOptionFor,
            privileges: SimplePrivs,
            on_kw: (Space, StringEqCI!("ON"), Space),
            objects: Objcts,
            from_kw: (Space, StringEqCI!("FROM"), Space),
            grantees: Grntee,
            behavior: Option<(Space, CascadeOpt)>,
            as_clause: Option<(Space, StringEqCI!("AS"), Space, Grntee)>,
        },
        Actions {
            grant_option: GrantOptionFor,
            actions: VecSep<Action, ArgSep>,
            on_kw: (Space, StringEqCI!("ON"), Space),
            object: Object,
            from_kw: (Space, StringEqCI!("FROM"), Space),
            grantees: Grntee,
            behavior: Option<(Space, CascadeOpt)>,
            as_clause: Option<(Space, StringEqCI!("AS"), Space, Grntee)>,
        },
        CubeSelect {
            grant_option: GrantOptionFor,
            select_kw: StringEqCI!("SELECT"),
            on_kw: (Space, StringEqCI!("ON"), Space),
            cube_kw: StringEqCI!("CUBE"),
            cubes_suffix: Option<StringEqCI!("S")>,
            cubes: (Space, CubeList),
            from_kw: (Space, StringEqCI!("FROM"), Space),
            grantees: Grntee,
            behavior: Option<(Space, CascadeOpt)>,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "simple privs")]
    pub enum SimplePrivs {
        All {
            all_kw: StringEqCI!("ALL"),
            privileges_kw: Option<(Space, StringEqCI!("PRIVILEGES"))>,
        },
        Star(StringEq!("*")),
        List(VecSep<SimplePriv, ArgSep>),
        Execute(StringEqCI!("EXECUTE")),
        Use(StringEqCI!("USE")),
    }

    #[derive(Grammar)]
    #[tygr(name = "simple priv")]
    pub enum SimplePriv {
        Alter(StringEqCI!("%ALTER")),
        Delete(StringEqCI!("DELETE")),
        OptCol(OptColPriv),
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
    #[tygr(name = "opt col priv")]
    pub enum OptColPriv {
        Select(StringEqCI!("SELECT")),
        Insert(StringEqCI!("INSERT")),
        Update(StringEqCI!("UPDATE")),
        References(StringEqCI!("REFERENCES")),
    }

    #[derive(Grammar)]
    #[tygr(name = "admin priv")]
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
        List(VecSep<User, ArgSep>),
    }

    #[derive(Grammar)]
    pub enum User {
        Public(StringEqCI!("PUBLIC")),
        Ident(Option<StringEq!("_")>, Ident),
    }

    #[derive(Grammar)]
    pub struct Role(pub Ident);

    #[derive(Grammar)]
    pub enum Objcts {
        Star(StringEq!("*")),
        Schemas {
            schema_kw: StringEqCI!("SCHEMA"),
            space: Space,
            schemas: VecSep<Schema, ArgSep>,
        },
        ForeignServers(ForeignServerPrivilegeClause),
        Tables(VecSep<Object, ArgSep>),
    }

    #[derive(Grammar)]
    pub struct Object(pub TName);

    #[derive(Grammar)]
    pub struct Schema(pub Ident);

    #[derive(Grammar)]
    #[tygr(name = "cubelist")]
    pub struct CubeList(pub VecSep<CubeName, ArgSep>);

    #[derive(Grammar)]
    #[tygr(name = "cubename")]
    pub struct CubeName(pub Ident);

    #[derive(Grammar)]
    #[tygr(name = "foreign-server-privilege-list")]
    pub struct ForeignServerPrivilegeList(pub VecSep<TName, ArgSep>);

    #[derive(Grammar)]
    #[tygr(name = "foreign-server-privilege-clause")]
    pub struct ForeignServerPrivilegeClause {
        pub foreign_kw: Option<(StringEqCI!("FOREIGN"), Space)>,
        pub server_kw: ServersServer,
        // A table named Servers: GRANT SELECT ON Servers TO u1 is a table grant on IRIS.
        pub servers: (
            Space1,
            NotFollowedBy<(Either<StringEqCI!("TO"), StringEqCI!("FROM")>, Space1)>,
            ForeignServerPrivilegeList,
        ),
    }

    #[derive(Grammar)]
    #[tygr(name = "SERVERS/SERVER")]
    pub enum ServersServer {
        Servers(StringEqCI!("SERVERS")),
        Server(StringEqCI!("SERVER")),
    }

    #[derive(Grammar)]
    #[tygr(name = "chkprv")]
    pub struct ChkPrv {
        pub checkpriv_kw: StringEqCI!("%CHECKPRIV"),
        pub grant_option: Option<(Space, GrantOpt)>,
        pub command: (Space, ChkPrvCmd),
    }

    #[derive(Grammar)]
    #[tygr(name = "grant opt")]
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
    #[tygr(name = "chkprv cmd")]
    pub enum ChkPrvCmd {
        Admin(VecSep<AdminPriv, ArgSep>),
        PrivSpec {
            priv_spec: PrivSpec,
            on_kw: (Space, StringEqCI!("ON"), Space),
            object: Object,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "priv spec")]
    pub enum PrivSpec {
        Action(Action),
        Threshold(StringEqCI!("%THRESHOLD")),
    }

    type OptParenScalx = Option<Parenthesized<Scalx>>;
    type OptParenScalx2 = Option<Parenthesized<(Scalx, Option<(ArgSep, Scalx)>)>>;

    #[derive(Grammar)]
    #[tygr(name = "cast type")]
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

    pub type IfExists = Option<(StringEqCI!("IF"), Space, StringEqCI!("EXISTS"), Space)>;
    pub type IfNotExists = Option<(
        StringEqCI!("IF"),
        Space,
        StringEqCI!("NOT"),
        Space,
        StringEqCI!("EXISTS"),
        Space,
    )>;
    pub type OrReplace = Option<(StringEqCI!("OR"), Space, StringEqCI!("REPLACE"), Space)>;

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct UserSpec {
        pub name: Ident,
        pub password: Option<(Space, PasswordClause)>,
        pub default: Option<(
            (Space, StringEqCI!("DEFAULT"), Space),
            Ident,
            (Space, Ident),
        )>,
        pub temporary: Option<(
            (Space, StringEqCI!("TEMPORARY"), Space),
            Ident,
            (Space, Ident),
        )>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ServerKw {
        pub foreign: Option<(StringEqCI!("FOREIGN"), Space)>,
        pub server_kw: (StringEqCI!("SERVER"), Space),
    }

    #[derive(Grammar)]
    #[tygr(name = "ddl")]
    pub enum DDL {
        CreOrAltView {
            cre_or_alt: CreOrAlt,
            kw: (Space, StringEqCI!("VIEW"), Space),
            tname: TName,
            col_list: Option<(
                (Space, StringEq!("("), Space),
                VecSep<Ident, ArgSep>,
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
            desc: Option<((Space, StringEqCI!("%DESCRIPTION"), Space), LiteralOrNull)>,
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
            user: UserSpec,
        },
        AlterServer {
            kw: (StringEqCI!("ALTER"), Space),
            server_kw: ServerKw,
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
            if_not_exists: IfNotExists,
            tname: TName,
            columns: Option<(
                (Space, StringEq!("("), Space),
                VecSep<(Ident, (Space, Datyp)), ArgSep>,
                (Space, StringEq!(")")),
            )>,
            server: (Space, StringEqCI!("SERVER"), Space),
            server_name: TName,
            ft_options: Option<(Space, FTOptions)>,
            values: Option<(
                (Space, StringEqCI!("VALUES"), Space, StringEq!("("), Space),
                VecSep<Ident, ArgSep>,
                (Space, StringEq!(")")),
            )>,
            using_json: Option<((Space, StringEqCI!("USING"), Space), JSONString)>,
            novalidate: Option<(Space, StringEqCI!("NOVALIDATE"))>,
        },
        CreateModel {
            kw: (StringEqCI!("CREATE"), Space, StringEqCI!("MODEL"), Space),
            if_not_exists: IfNotExists,
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
            if_not_exists: IfNotExists,
            name: Ident,
        },
        CreateTableAs {
            kw: (StringEqCI!("CREATE"), Space, StringEqCI!("TABLE"), Space),
            if_not_exists: IfNotExists,
            nojourn: Option<(StringEqCI!("%NOJOURN"), Space)>,
            tname: TName,
            as_kw: (Space, StringEqCI!("AS"), Space),
            with_clause: Option<(WithClause, Space)>,
            query: Query,
            options: Option<(Space, TableOptionList)>,
        },
        CreateUser {
            kw: (StringEqCI!("CREATE"), Space, StringEqCI!("USER"), Space),
            user: UserSpec,
        },
        CreateServer {
            kw: (StringEqCI!("CREATE"), Space),
            server_kw: ServerKw,
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
            comp_list: VecSep<ComponentSpec, ArgSep>,
            comp_close: (Space, StringEq!(")")),
            as_clause: Option<(
                (Space, StringEqCI!("AS"), Space),
                ClassName,
                Option<(Space, CreateIndexAsParams)>,
            )>,
            with_clause: Option<(Space, CreateIndexWithClause)>,
            in_on: Option<(Space, InOn, (Space, Token))>,
            defer_immediate: Option<(Space, CreateIndexDeferImmediate)>,
        },
        CreateAggregate {
            kw: (StringEqCI!("CREATE"), Space),
            or_replace: OrReplace,
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
            or_replace: OrReplace,
            func_kw: (StringEqCI!("FUNCTION"), Space),
            name: QualifiedSPName,
            spec: (Space, CodeSpecification),
        },
        CreateMlConfiguration {
            kw: (StringEqCI!("CREATE"), Space),
            or_replace: OrReplace,
            ml_kw: (
                StringEqCI!("ML"),
                Space,
                StringEqCI!("CONFIGURATION"),
                Space,
            ),
            name: MLConfigurationName,
            provider: (Space, StringEqCI!("PROVIDER"), Space),
            provider_name: ProviderName,
            desc: Option<((Space, StringEqCI!("%DESCRIPTION"), Space), LiteralOrNull)>,
            using_clause: Option<(Space, MlconfigUsingClause)>,
            settings: Option<(Space, ProviderSpecificSettings)>,
        },
        CreateProcedure {
            kw: (StringEqCI!("CREATE"), Space),
            or_replace: OrReplace,
            proc_kw: (StringEqCI!("PROC"), Option<StringEqCI!("EDURE")>, Space),
            name: QualifiedSPName,
            spec: (Space, CodeSpecification),
        },
        CreateTrigger {
            kw: (StringEqCI!("CREATE"), Space),
            or_replace: OrReplace,
            trigger_kw: (StringEqCI!("TRIGGER"), Space),
            name: TName,
            time: (Space, TriggerTime, Space),
            events: VecSep<TriggerEvent, ArgSep>,
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
            if_exists: IfExists,
            name: TName,
            from: Option<((Space, StringEqCI!("FROM"), Space), TName)>,
        },
        CreateQuery {
            kw: (StringEqCI!("CREATE"), Space),
            or_replace: OrReplace,
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
            if_not_exists: IfNotExists,
            tname: TName,
            open: (Space, StringEq!("("), Space),
            elems: VecSep<Telem, ArgSep>,
            close: (Space, StringEq!(")")),
            options: Option<(Space, TableOptionList)>,
        },
        DropDatabase {
            kw: (StringEqCI!("DROP"), Space, StringEqCI!("DATABASE"), Space),
            if_exists: IfExists,
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
            if_exists: IfExists,
            tname: TName,
            cascade: Option<(Space, CascadeOpt)>,
        },
        DropFunction {
            kw: (StringEqCI!("DROP"), Space, StringEqCI!("FUNCTION"), Space),
            if_exists: IfExists,
            name: QualifiedSPName,
            from: Option<((Space, StringEqCI!("FROM"), Space), ClassName)>,
        },
        DropIndexDot {
            kw: (StringEqCI!("DROP"), Space, StringEqCI!("INDEX"), Space),
            if_exists: IfExists,
            nojourn: Option<(StringEqCI!("%NOJOURN"), Space)>,
            tname: Ident,
            name: Vec1<((Space, StringEq!("."), Space), Ident)>,
        },
        DropIndexOn {
            kw: (StringEqCI!("DROP"), Space, StringEqCI!("INDEX"), Space),
            if_exists: IfExists,
            name: IndexName,
            on: Option<(
                (Space, StringEqCI!("ON"), Space),
                Option<(StringEqCI!("TABLE"), Space)>,
                TName,
            )>,
        },
        DropMethod {
            kw: (StringEqCI!("DROP"), Space, StringEqCI!("METHOD"), Space),
            if_exists: IfExists,
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
            if_exists: IfExists,
            name: MLConfigurationName,
        },
        DropModel {
            kw: (StringEqCI!("DROP"), Space, StringEqCI!("MODEL"), Space),
            if_exists: IfExists,
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
            if_exists: IfExists,
            name: QualifiedSPName,
            from: Option<((Space, StringEqCI!("FROM"), Space), ClassName)>,
        },
        DropQuery {
            kw: (StringEqCI!("DROP"), Space, StringEqCI!("QUERY"), Space),
            if_exists: IfExists,
            name: QualifiedSPName,
            from: Option<((Space, StringEqCI!("FROM"), Space), ClassName)>,
        },
        DropRole {
            kw: (StringEqCI!("DROP"), Space, StringEqCI!("ROLE"), Space),
            if_exists: IfExists,
            name: Ident,
        },
        DropSchema {
            kw: (StringEqCI!("DROP"), Space, StringEqCI!("SCHEMA"), Space),
            if_exists: IfExists,
            name: Ident,
            cascade_restrict: Option<(Space, CascadeOpt)>,
        },
        DropTable {
            kw: (StringEqCI!("DROP"), Space, StringEqCI!("TABLE"), Space),
            if_exists: IfExists,
            nojourn: Option<(StringEqCI!("%NOJOURN"), Space)>,
            tname: TName,
            cascade: Option<(Space, CascadeOpt)>,
            data: Option<(Space, DataOption)>,
        },
        DropUser {
            kw: (StringEqCI!("DROP"), Space, StringEqCI!("USER"), Space),
            if_exists: IfExists,
            name: Ident,
            cascade: Option<(Space, StringEqCI!("CASCADE"))>,
        },
        DropView {
            kw: (StringEqCI!("DROP"), Space, StringEqCI!("VIEW"), Space),
            if_exists: IfExists,
            tname: TName,
            cascade: Option<(Space, CascadeOpt)>,
        },
        DropServer {
            kw: (StringEqCI!("DROP"), Space),
            server_kw: ServerKw,
            tname: TName,
            cascade: Option<(Space, CascadeOpt)>,
        },
        DropAggregate {
            kw: (StringEqCI!("DROP"), Space, StringEqCI!("AGGREGATE"), Space),
            if_exists: IfExists,
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
            for_col: Option<((Space, StringEqCI!("FOR"), Space), ColumnDefItem)>,
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
    #[tygr(inline)]
    pub struct CreateTableTemp {
        pub private_global: PrivateGlobal,
        pub space: Space,
        pub temporary: StringEqCI!("TEMPORARY"),
    }

    #[derive(Grammar)]
    #[tygr(name = "PRIVATE/GLOBAL")]
    pub enum PrivateGlobal {
        Global(StringEqCI!("GLOBAL")),
        Private(StringEqCI!("PRIVATE")),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum CreateIndexPartitioned {
        NonPartitioned(StringEqCI!("NONPARTITIONED")),
        Partitioned(StringEqCI!("PARTITIONED")),
    }

    #[derive(Grammar)]
    #[tygr(name = "create server connection")]
    pub enum CreateServerConnection {
        Connection((StringEqCI!("CONNECTION"), Space), Literal),
        Host {
            kw: (StringEqCI!("HOST"), Space),
            host: Literal,
            options: Option<(
                (Space, StringEq!("("), Space),
                DBServerOptions,
                (Space, StringEq!(")")),
            )>,
        },
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum CreateServerDelimitedIds {
        Delimited(StringEqCI!("DELIMITEDIDS")),
        NoDelimited(StringEqCI!("NODELIMITEDIDS")),
    }

    pub type CreateServerPassthrough = Suffix<EnableDisable, (Space, StringEqCI!("PASSTHROUGH"))>;

    #[derive(Grammar)]
    #[tygr(name = "ENABLE/DISABLE")]
    pub enum EnableDisable {
        Enable(StringEqCI!("ENABLE")),
        Disable(StringEqCI!("DISABLE")),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum CreateIndexAsParams {
        Parens {
            open: (StringEq!("("), Space),
            params: VecSep<IndexParameterSpec, ArgSep>,
            close: (Space, StringEq!(")")),
        },
        With {
            kw: (StringEqCI!("WITH"), Space),
            params: VecSep<IndexParameterSpec, ArgSep>,
        },
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct CreateIndexWithClause {
        pub kw: (StringEqCI!("WITH"), Space),
        pub data: Option<(
            (StringEqCI!("DATA"), Space, StringEq!("("), Space),
            VecSep<Ident, ArgSep>,
            (Space, StringEq!(")")),
        )>,
        pub pad_index: Option<(
            Space,
            Option<(StringEq!(","), Space)>,
            StringEqCI!("PAD_INDEX"),
        )>,
        pub fillfactor: Option<(
            Space,
            Option<(StringEq!(","), Space)>,
            StringEqCI!("FILLFACTOR"),
            (Space, StringEq!("="), Space),
            Literal,
        )>,
        pub ignore_dup: Option<(
            Space,
            Option<(StringEq!(","), Space)>,
            StringEqCI!("IGNORE_DUP_KEY"),
        )>,
        pub drop_existing: Option<(
            Space,
            Option<(StringEq!(","), Space)>,
            StringEqCI!("DROP_EXISTING"),
        )>,
        pub stats_norecompute: Option<(
            Space,
            Option<(StringEq!(","), Space)>,
            StringEqCI!("STATISTICS_NORECOMPUTE"),
        )>,
    }

    pub type CreateIndexDeferImmediate =
        Suffix<ImmediateDefer, Option<(Space, StringEqCI!("BUILD"))>>;

    #[derive(Grammar)]
    #[tygr(name = "IMMEDIATE/DEFER")]
    pub enum ImmediateDefer {
        Immediate(StringEqCI!("IMMEDIATE")),
        Defer(StringEqCI!("DEFER")),
    }

    #[derive(Grammar)]
    #[tygr(name = "alter-ft-action")]
    pub struct AlterFTAction(pub Ident);

    #[derive(Grammar)]
    #[tygr(name = "model-name")]
    pub struct ModelName(pub Ident);

    pub type PasswordClause = Prefix<(PWPrefix, Space), Password>;

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
    #[tygr(name = "build scope")]
    pub enum BuildScope {
        All(StringEqCI!("ALL")),
        Schema((StringEqCI!("SCHEMA"), Space), Ident),
        Table {
            table_kw: (StringEqCI!("TABLE"), Space),
            first: Ident,
            rest: Vec<((Space, StringEq!("."), Space), Ident)>,
            index: Option<((Space, StringEqCI!("INDEX"), Space), VecSep<Ident, ArgSep>)>,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "build option")]
    pub enum BuildOption {
        NoJourn(StringEqCI!("%NOJOURN")),
        NoLock(StringEqCI!("%NOLOCK")),
    }

    #[derive(Grammar)]
    #[tygr(name = "purge scope")]
    pub enum PurgeScope {
        Age((StringEqCI!("AGE"), Space), Literal),
        Name {
            name_kw: (StringEqCI!("NAME"), Space),
            first: VecSep<Ident, (Space, StringEq!("."), Space)>,
            rest: Vec<(ArgSep, VecSep<Ident, (Space, StringEq!("."), Space)>)>,
        },
        Table((StringEqCI!("TABLE"), Space), TName),
    }

    #[derive(Grammar)]
    #[tygr(name = "freeze scope")]
    pub enum FreezeScope {
        Id((StringEqCI!("ID"), Space), Literal),
        Schema((StringEqCI!("SCHEMA"), Space), Schema),
        Table((StringEqCI!("TABLE"), Space), TName),
    }

    pub type CancelQueryScope =
        Prefix<(StringEqCI!("IDENTIFIED"), Space, StringEqCI!("BY"), Space), ValSpec>;

    pub type TimeoutScope = Prefix<(StringEqCI!("TIMEOUT"), Space), ValSpec>;

    pub type WithClauseCreate = Prefix<(StringEqCI!("WITH"), Space), Parenthesized<ColumnDefList>>;

    #[derive(Grammar)]
    #[tygr(name = "model-source")]
    pub enum ModelSource {
        Subquery {
            open: (StringEqCI!("FROM"), Space, StringEq!("("), Space),
            selex: Src<Selex>,
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
    #[tygr(name = "alg description")]
    pub struct AlgDescription(pub JSONObject);

    pub type WithClauseTrain = Prefix<(StringEqCI!("WITH"), Space), TrainWith>;

    #[derive(Grammar)]
    #[tygr(name = "train-with")]
    pub enum TrainWith {
        List {
            open: (StringEq!("("), Space),
            idents: VecSep<Ident, ArgSep>,
            close: (Space, StringEq!(")")),
        },
        Assignment(WithValueAssignment),
    }

    #[derive(Grammar)]
    #[tygr(name = "with-value-assignment")]
    pub struct WithValueAssignment {
        pub first_name: Ident,
        pub first_eq: (Space, StringEq!("="), Space),
        pub first_val: WithExpression,
        pub rest: Vec<(
            ArgSep,
            Ident,
            (Space, StringEq!("="), Space),
            WithExpression,
        )>,
    }

    #[derive(Grammar)]
    #[tygr(name = "with-expression")]
    pub enum WithExpression {
        Ident(Ident),
        Literal(Literal),
        Scalx(Scalx),
    }

    #[derive(Grammar)]
    #[tygr(name = "alter-model-action")]
    pub enum AlterModelAction {
        Default(DefaultTrainedModel),
        Purge(StringEqCI!("PURGE"), Option<(Space, Scope)>),
    }

    #[derive(Grammar)]
    #[tygr(name = "DefaultTrainedModel")]
    pub struct DefaultTrainedModel {
        pub kw: StringEqCI!("DEFAULT"),
        pub trained_model: Option<(Space, StringEqCI!("TRAINED"), Space, StringEqCI!("MODEL"))>,
        pub name: (Space, TrainedModelName),
    }

    #[derive(Grammar)]
    #[tygr(name = "scope")]
    pub enum Scope {
        Days(Integer, (Space, StringEqCI!("DAYS"))),
        All(StringEqCI!("ALL")),
    }

    #[derive(Grammar)]
    #[tygr(name = "provider_name")]
    pub enum ProviderName {
        AutoML(StringEqCI!("AutoML")),
        DataRobot(StringEqCI!("DataRobot")),
        H2O(StringEqCI!("H2O")),
        PMML(StringEqCI!("PMML")),
    }

    #[derive(Grammar)]
    #[tygr(name = "literal or NULL")]
    pub enum LiteralOrNull {
        Literal(Literal),
        Null(StringEqCI!("NULL")),
    }

    pub type MlconfigUsingClause = Prefix<(StringEqCI!("USING"), Space), MlconfigUsing>;

    #[derive(Grammar)]
    #[tygr(name = "mlconfig-using")]
    pub enum MlconfigUsing {
        JSONString(JSONObject),
        Null(StringEqCI!("NULL")),
    }

    #[derive(Grammar)]
    #[tygr(name = "provider_specific_settings")]
    pub enum ProviderSpecificSettings {
        DR(VecSep<DROption, ArgSep>),
        H2O(VecSep<H2OOption, ArgSep>),
    }

    #[derive(Grammar)]
    #[tygr(name = "tune option")]
    pub enum TuneOption {
        ClearValues(StringEqCI!("%CLEAR_VALUES")),
        KeepUpToDate(StringEqCI!("%KEEP_UP_TO_DATE")),
        RecompileCQ(StringEqCI!("%RECOMPILE_CQ")),
        SamplePercent(StringEqCI!("%SAMPLE_PERCENT"), (Space, PercentValue)),
    }

    #[derive(Grammar)]
    #[tygr(name = "DR option")]
    pub enum DROption {
        ApiToken(
            StringEqCI!("APITOKEN"),
            Option<(Space, StringEq!("="))>,
            (Space, LiteralOrNull),
        ),
        Url(
            StringEqCI!("URL"),
            Option<(Space, StringEq!("="))>,
            (Space, LiteralOrNull),
        ),
    }

    #[derive(Grammar)]
    #[tygr(name = "H2O option")]
    pub struct H2OOption {
        pub kw: StringEqCI!("JAVATLSCONFIGFILE"),
        pub eq: Option<(Space, StringEq!("="))>,
        pub val: (Space, LiteralOrNull),
    }

    #[derive(Grammar)]
    #[tygr(name = "collect target")]
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
    #[tygr(name = "db_option")]
    pub struct DBOption(pub Ident);

    #[derive(Grammar)]
    #[tygr(name = "ft-options")]
    pub enum FTOptions {
        Table((StringEqCI!("TABLE"), Space), Literal),
        Query((StringEqCI!("QUERY"), Space), Literal),
        File(FileSource),
    }

    pub type JSONObject = crate::cos::Brace<crate::cos::OSText>;

    #[derive(Grammar)]
    #[tygr(name = "json-string")]
    pub struct JSONString(pub Either<JSONObject, StringLiteral>);

    #[derive(Grammar)]
    #[tygr(name = "server-type")]
    pub enum ServerType {
        DB(StringEqCI!("'DB'")),
        FILE(StringEqCI!("'FILE'")),
    }

    #[derive(Grammar)]
    #[tygr(name = "fdw-name")]
    pub struct FDWName(pub Ident);

    #[derive(Grammar)]
    #[tygr(name = "db-server-options")]
    pub struct DBServerOptions(pub Ident);

    pub type FTPushdownClause = Prefix<StringEqCI!("PUSHDOWN"), (Space, FTPushdownType)>;

    #[derive(Grammar)]
    #[tygr(name = "ft-pushdown-type")]
    pub enum FTPushdownType {
        Ansi92(StringEqCI!("ANSI92")),
        Extended(StringEqCI!("EXTENDED")),
        Iris(StringEqCI!("IRIS")),
        Off(StringEqCI!("OFF")),
    }

    #[derive(Grammar)]
    #[tygr(name = "qualified SPname")]
    pub struct QualifiedSPName(pub DottedIdent);

    #[derive(Grammar)]
    #[tygr(name = "classname")]
    pub struct ClassName(pub DottedIdent);

    #[derive(Grammar)]
    #[tygr(name = "trigger time")]
    pub enum TriggerTime {
        Before(StringEqCI!("BEFORE")),
        After(StringEqCI!("AFTER")),
    }

    #[derive(Grammar)]
    #[tygr(name = "trigger event")]
    pub enum TriggerEvent {
        Insert(StringEqCI!("INSERT")),
        Delete(StringEqCI!("DELETE")),
        Update {
            kw: StringEqCI!("UPDATE"),
            of: Option<(Space, StringEqCI!("OF"), Space, VecSep<Ident, ArgSep>)>,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "trigger referencing")]
    pub struct TriggerReferencing {
        pub old_new: Either<StringEqCI!("OLD"), StringEqCI!("NEW")>,
        pub row: Option<(Space, StringEqCI!("ROW"))>,
        pub as_kw: Option<(Space, StringEqCI!("AS"))>,
        pub alias: (Space, Ident),
    }

    #[derive(Grammar)]
    #[tygr(name = "trigger orientation")]
    pub enum TriggerOrientation {
        RowAndObject(StringEqCI!("ROW_AND_OBJECT")),
        Row(StringEqCI!("ROW")),
        Statement(StringEqCI!("STATEMENT")),
    }

    #[derive(Grammar)]
    #[tygr(name = "trigger body")]
    pub enum TriggerBody {
        Code(CodeBody),
        Stat {
            lang: Option<(StringEqCI!("LANGUAGE"), Space, StringEqCI!("SQL"), Space)>,
            stat: Box<Src<Stat>>,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "lock spec")]
    pub enum LockSpec {
        Row(StringEqCI!("ROW")),
        Page(StringEqCI!("PAGE")),
    }

    #[derive(Grammar)]
    #[tygr(name = "cascade opt")]
    pub enum CascadeOpt {
        Restrict(StringEqCI!("RESTRICT")),
        Cascade(StringEqCI!("CASCADE")),
    }

    #[derive(Grammar)]
    #[tygr(name = "data option")]
    pub enum DataOption {
        DelData(StringEqCI!("%DELDATA")),
        NoDelData(StringEqCI!("%NODELDATA")),
    }

    #[derive(Grammar)]
    #[tygr(name = "move spec")]
    pub struct MoveSpec {
        pub partition_range: Option<(PartitionRangeSpec, Space)>,
        pub to_kw: StringEqCI!("TO"),
        pub database: (Space, DatabaseName),
        pub activate: Option<(Space, SQLActivateSpec)>,
    }

    #[derive(Grammar)]
    #[tygr(name = "activate spec")]
    pub struct SQLActivateSpec {
        pub force_or_defer: Option<(Either<StringEqCI!("FORCE"), StringEqCI!("DEFER")>, Space)>,
        pub activate_kw: StringEqCI!("ACTIVATE"),
        pub mappings_kw: Option<(Space, StringEqCI!("MAPPINGS"))>,
    }

    #[derive(Grammar)]
    #[tygr(name = "cre or alt")]
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
    #[tygr(name = "view with")]
    pub enum ViewWith {
        ReadOnly(StringEqCI!("READ"), Space, StringEqCI!("ONLY")),
        Check {
            levels: Option<(LevelsClause, Space)>,
            check_kw: StringEqCI!("CHECK"),
            option_kw: (Space, StringEqCI!("OPTION")),
            constraint: Option<(Space, StringEqCI!("CONSTRAINT"), Space, Ident)>,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "levels clause")]
    pub enum LevelsClause {
        Cascaded(StringEqCI!("CASCADED")),
        Cascade(StringEqCI!("CASCADE")),
        Local(StringEqCI!("LOCAL")),
    }

    #[derive(Grammar)]
    #[tygr(name = "sql")]
    pub struct SQL {
        pub space_before: Space,
        pub view_column_list: Option<(ViewColumnList, Space)>,
        pub stat: Src<Stat>,
        pub space_after: Space,
    }

    #[derive(Grammar)]
    pub struct Thr {
        pub through: StringEqCI!("THROUGH"),
        pub sp1: Space,
        pub foreigh: StringEqCI!("FOREIGN"),
        pub sp2: Space,
        pub server: StringEqCI!("SERVER"),
        pub sp3: Space,
        pub tname: TName,
        pub sp4: Space,
        pub remote_statement: WithStat<Selct>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct WithStat<T: Grammar> {
        pub with_clause: Option<(WithClause, Space)>,
        pub body: T,
    }

    #[derive(Grammar)]
    #[tygr(name = "stat")]
    pub enum Stat {
        Selct(WithStat<Selct>),
        Insert(WithStat<Insrt>),
        Update(WithStat<Updat>),
        Delete(WithStat<Delet>),
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
        Call(CallStatement),
        Explain(Explain),
        Setml(Setml),
        ChkPrv(ChkPrv),
    }

    #[derive(Grammar)]
    #[tygr(name = "percent_value", validated)]
    pub struct PercentValue(pub Number);

    impl Validate for PercentValue {
        const REQUIREMENT: &'static str = "be in the range (0, 1]";
        fn validate(&self) -> bool {
            self.0
                .print()
                .parse::<f64>()
                .is_ok_and(|v| v > 0.0 && v <= 1.0)
        }
    }

    #[derive(Grammar)]
    #[tygr(name = "directory_name")]
    pub struct DirectoryName(pub Literal);

    #[derive(Grammar)]
    #[tygr(name = "database_name")]
    pub enum DatabaseName {
        Percent {
            prefix: StringEq!("%"),
            first: IsDbIdentPart,
            rest: Vec<IsDbIdentPart>,
        },
        Normal {
            first: IsDbIdentStart,
            rest: Vec<IsDbIdentPart>,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "code specification")]
    pub struct CodeSpecification {
        pub open: (StringEq!("("), Space),
        pub params: ParameterList,
        pub close: (Space, StringEq!(")")),
        pub characteristics: Vec<MethodQueryCharacteristics>,
        pub body: (Space, CodeBody),
    }

    #[derive(Grammar)]
    #[tygr(name = "parameter list")]
    pub struct ParameterList(pub Option<VecSep<ParameterDeclaration, ArgSep>>);

    #[derive(Grammar)]
    #[tygr(name = "parameter declaration")]
    pub struct ParameterDeclaration {
        pub direction: Option<(ParameterDirection, Space)>,
        pub name: Ident,
        pub datyp: (Space, Datyp),
        pub default: Option<(Space, DefaultClause)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum ParameterDirection {
        In(StringEqCI!("IN")),
        InOut(StringEqCI!("INOUT")),
        Out(StringEqCI!("OUT")),
    }

    #[derive(Grammar)]
    #[tygr(name = "default clause")]
    pub enum DefaultClause {
        Default {
            kw: (StringEqCI!("DEFAULT"), Space),
            value: Scalx,
        },
        Expr(Scalx),
    }

    #[derive(Grammar)]
    #[tygr(name = "method-query characteristics")]
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
    #[tygr(name = "result set clause")]
    pub struct ResultSetClause(pub Parenthesized<ColumnDefList>);

    #[derive(Grammar)]
    #[tygr(name = "select mode")]
    pub enum SelectMode {
        Display(StringEqCI!("DISPLAY")),
        Logical(StringEqCI!("LOGICAL")),
        Odbc(StringEqCI!("ODBC")),
        Runtime(StringEqCI!("RUNTIME")),
    }

    #[derive(Grammar)]
    #[tygr(name = "code body")]
    pub enum CodeBody {
        Cos {
            kw: (
                StringEqCI!("LANGUAGE"),
                Space,
                Either<StringEqCI!("OBJECTSCRIPT"), StringEqCI!("COS")>,
                Space,
            ),
            body: Either<COSMethodBody, COSQueryBody>,
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
            Either<ReturnStatement, Src<Stat>>,
            Space,
            StringEq!(";"),
        )>,
        (Space, StringEqCI!("END")),
    >;

    #[derive(Grammar)]
    #[tygr(name = "return statement")]
    pub struct ReturnStatement {
        pub kw: StringEqCI!("RETURN"),
        pub space: Space,
        pub value: Option<ReturnValue>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum ReturnValue {
        HostRef(HostRef),
        Literal(Literal),
    }

    #[derive(Grammar)]
    #[tygr(name = "COS query body")]
    pub struct COSQueryBody {
        pub first: QueryMethod,
        pub rest: Vec<(Space, QueryMethod)>,
    }

    #[derive(Grammar)]
    #[tygr(name = "query method")]
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
    #[tygr(name = "indexname")]
    pub struct IndexName(pub Ident);

    #[derive(Grammar)]
    #[tygr(name = "index qualifiers")]
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
    #[tygr(name = "component spec")]
    pub struct ComponentSpec {
        pub name: Ident,
        pub collection: Option<(Space, CollectionSpec)>,
        pub collate: Option<(Space, StringEqCI!("COLLATE"), Space, SqlCollation)>,
        pub direction: Option<(Space, ComponentDirection)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum ComponentDirection {
        Asc(StringEqCI!("ASC")),
        Desc(StringEqCI!("DESC")),
    }

    #[derive(Grammar)]
    #[tygr(name = "collection spec")]
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
    #[tygr(name = "classname")]
    pub struct Classname {
        pub first: DottedIdent,
        pub rest: Vec<(ArgSep, Ident)>,
    }

    #[derive(Grammar)]
    #[tygr(name = "index parameter spec")]
    pub struct IndexParameterSpec {
        pub name: Ident,
        pub eq: (Space, StringEq!("="), Space),
        pub value: Literal,
    }

    #[derive(Grammar)]
    #[tygr(name = "alter action")]
    pub enum AlterAction {
        CheckConstraintAll {
            check: CheckSpec,
            kw: (Space, StringEqCI!("CONSTRAINT"), Space, StringEqCI!("ALL")),
        },
        DropAllPartitions {
            drop_kw: DropKeywd,
            kw: (Space, StringEqCI!("ALL"), Space, StringEqCI!("PARTITIONS")),
        },
        DropPartition {
            drop_kw: DropKeywd,
            partition: (Space, PartitionRangeSpec),
        },
        DropAction {
            drop_kw: DropKeywd,
            action: (Space, DropAction),
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
            partition: PartitionRangeSpec,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "convert clause")]
    pub enum ConvertClause {
        Partition(PartitionConvertClause),
        StorageDefault {
            clause: StorageDefaultConvertClause,
            del: Option<(Space, DataOption)>,
        },
        Shard {
            to: Option<(StringEqCI!("TO"), Space)>,
            clause: ShardConvertClause,
            timing: Option<(Space, ImmediateDefer)>,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "shard convert clause")]
    pub enum ShardConvertClause {
        Sharded(Sharded),
        Shard(Shard),
        NonSharded(StringEqCI!("NONSHARDED")),
    }

    #[derive(Grammar)]
    #[tygr(name = "partition convert clause")]
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
    #[tygr(inline)]
    pub enum PartitionOnOff {
        On(StringEqCI!("ON")),
        Off(StringEqCI!("OFF")),
    }

    #[derive(Grammar)]
    #[tygr(name = "alter col")]
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
    #[tygr(name = "storagedefault convert clause")]
    pub enum StorageDefaultConvertClause {
        Column(ColumnStorageDefaultConversion),
        Type(StorageDefaultConversionTypeSpec),
    }

    #[derive(Grammar)]
    #[tygr(name = "storagedefault conversion type spec")]
    pub struct StorageDefaultConversionTypeSpec {
        pub to: Option<(StringEqCI!("TO"), Space)>,
        pub kw: (StringEqCI!("STORAGETYPE"), Space, StringEq!("="), Space),
        pub typ: StorageDefaultConversionType,
    }

    #[derive(Grammar)]
    #[tygr(name = "storagedefault conversion type")]
    pub enum StorageDefaultConversionType {
        Columnar(StringEqCI!("COLUMNAR")),
        Row(StringEqCI!("ROW")),
    }

    #[derive(Grammar)]
    #[tygr(name = "column storagedefault conversion")]
    pub struct ColumnStorageDefaultConversion {
        pub kw: StringEqCI!("COLUMN"),
        pub s: Option<Raw<StringEqCI!("S")>>,
        pub list: (Space, VecSep<Ident, ArgSep>),
        pub spec: (Space, StorageDefaultConversionTypeSpec),
    }

    pub type Setdef = OptionallyWrapped<DefSpec>;

    #[derive(Grammar)]
    #[tygr(name = "def spec")]
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
        CurrentTimestamp {
            kw: StringEqCI!("CURRENT_TIMESTAMP"),
            precision: Option<Parenthesized<Literal>>,
        },
        CurrentTime {
            kw: StringEqCI!("CURRENT_TIME"),
            precision: Option<Parenthesized<Literal>>,
        },
        CurrentUser(StringEqCI!("CURRENT_USER")),
        GetDate {
            kw: (StringEqCI!("GETDATE"), StringEq!("("), Space),
            value: Option<Literal>,
            close: (Space, StringEq!(")")),
        },
        GetUtcDate {
            kw: (StringEqCI!("GETUTCDATE"), StringEq!("("), Space),
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
            arg2: Option<(ArgSep, Literal)>,
            close: (Space, StringEq!(")")),
        },
        ToTimestamp {
            kw: (StringEqCI!("TO_TIMESTAMP("), Space),
            arg1: Literal,
            arg2: Option<(ArgSep, Literal)>,
            close: (Space, StringEq!(")")),
        },
        User(StringEqCI!("USER")),
    }

    #[derive(Grammar)]
    #[tygr(name = "mod spec")]
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

    pub type ModSpecList = VecSep<ModSpec, ArgSep>;

    #[derive(Grammar)]
    #[tygr(name = "mod col")]
    pub struct ModCol {
        pub default: Option<(StringEqCI!("DEFAULT"), Space, Setdef, Space)>,
        pub constraint: Option<(StringEqCI!("CONSTRAINT"), Space, Ident, Space)>,
        pub null: Option<NullSpec>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct NullSpec {
        pub not: Option<(StringEqCI!("NOT"), Space)>,
        pub null: StringEqCI!("NULL"),
    }

    #[derive(Grammar)]
    #[tygr(name = "drop keywd")]
    pub enum DropKeywd {
        Delete(StringEqCI!("DELETE")),
        Drop(StringEqCI!("DROP")),
    }

    #[derive(Grammar)]
    #[tygr(name = "drop action")]
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
            items: VecSep<DropColumnItem, ArgSep>,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "drop column item", inline)]
    pub struct DropColumnItem {
        pub name: Ident,
        pub cascade: Option<(Space, CascadeOpt)>,
        pub data: Option<(Space, DataOption)>,
    }

    #[derive(Grammar)]
    #[tygr(name = "add action")]
    pub struct AddAction {
        pub constraint: Option<(StringEqCI!("CONSTRAINT"), Space, TName, Space)>,
        pub body: AddBody,
    }

    pub type AddActionList = VecSep<AddAction, ArgSep>;

    #[derive(Grammar)]
    #[tygr(name = "add body")]
    pub enum AddBody {
        ForeignKey {
            spec: OptionallyWrapped<AltFkeySpec>,
            on: Option<(Space, StringEqCI!("ON"), Space, Token)>,
        },
        PrimaryKey {
            spec: OptionallyWrapped<AltPkey>,
            on: Option<(Space, StringEqCI!("ON"), Space, Token)>,
        },
        Unique {
            spec: OptionallyWrapped<AltUnique>,
            fillfactor: FillFactor,
            on: Option<(Space, StringEqCI!("ON"), Space, Token)>,
        },
        ColDef(OptionallyWrapped<Coldef>),
        Check {
            kw: (StringEqCI!("CHECK"), Space),
            parse: Chkparse,
            informix: Informix,
        },
        Default {
            kw: (StringEqCI!("DEFAULT"), Space),
            value: Setdef,
            for_kw: (Space, StringEqCI!("FOR"), Space),
            name: Ident,
        },
    }

    pub type Coldef = Prefix<Option<(StringEqCI!("COLUMN"), Space)>, OptionallyWrapped<Tecdef>>;

    #[derive(Grammar)]
    #[tygr(name = "check spec")]
    pub enum CheckSpec {
        Check(StringEqCI!("CHECK")),
        NoCheck(StringEqCI!("NOCHECK")),
    }

    #[derive(Grammar)]
    #[tygr(name = "alt pkey")]
    pub struct AltPkey {
        pub kw: (StringEqCI!("PRIMARY"), Space, StringEqCI!("KEY"), Space),
        pub spec: PkeySpec,
        pub informix: Informix,
    }

    #[derive(Grammar)]
    #[tygr(name = "pkey spec")]
    pub enum PkeySpec {
        FList1(FList1),
        IdentFList2 { name: Ident, flist: (Space, FList2) },
    }

    #[derive(Grammar)]
    #[tygr(name = "alt fkey spec")]
    pub struct AltFkeySpec {
        pub kw: (StringEqCI!("FOREIGN"), Space, StringEqCI!("KEY"), Space),
        pub ident: Option<(Ident, Space)>,
        pub flist2: FList2,
        pub references_kw: (Space, StringEqCI!("REFERENCES"), Space),
        pub ref_: Ref,
        pub informix: Informix,
    }

    #[derive(Grammar)]
    #[tygr(name = "alt unique")]
    pub struct AltUnique {
        pub kw: (StringEqCI!("UNIQUE"), Space),
        pub spec: UniqueSpec,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct UniqueSpec {
        pub cluster: Cluster,
        pub flist: (Space, FList2),
        pub informix: Informix,
    }

    #[derive(Grammar)]
    #[tygr(name = "in_on")]
    pub enum InOn {
        In(StringEqCI!("IN")),
        On(StringEqCI!("ON")),
    }

    #[derive(Grammar)]
    #[tygr(name = "token")]
    pub struct Token(pub Ident);

    #[derive(Grammar)]
    #[tygr(name = "telem")]
    pub struct Telem {
        pub constraint: Option<(StringEqCI!("CONSTRAINT"), Space, Ident, Space)>,
        pub option: TelemOption,
    }

    #[derive(Grammar)]
    #[tygr(name = "telem option")]
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
        TblForeign(AltFkeySpec),
        TblPrimaryKey(TblPrimaryKey),
        TblUnique(TblUnique),
        Tecdef(Tecdef),
    }

    #[derive(Grammar)]
    #[tygr(name = "tecdef")]
    pub struct Tecdef {
        pub name: Ident,
        pub typ: (Space, Tectyp),
        pub qualifiers: Vec<(Space, FldQualifier)>,
    }

    #[derive(Grammar)]
    #[tygr(name = "tectyp")]
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
    #[tygr(name = "tbl unique spec")]
    pub struct TblUniqueSpec {
        pub spec: UniqueSpec,
        pub fillfactor: FillFactor,
    }

    pub type FillFactor = Option<(
        Space,
        StringEqCI!("WITH"),
        Space,
        StringEqCI!("FILLFACTOR"),
        Space,
        StringEq!("="),
        Space,
        Literal,
    )>;

    pub type TblPrimaryKey =
        Prefix<(StringEqCI!("PRIMARY"), Space, StringEqCI!("KEY"), Space), TblUniqueSpec>;

    #[derive(Grammar)]
    #[tygr(name = "extentsize clause")]
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
    #[tygr(name = "class parameter")]
    pub struct ClassParameter {
        pub kw: (StringEqCI!("%CLASSPARAMETER"), Space),
        pub name: Ident,
        pub eq: Option<(Space, StringEq!("="))>,
        pub value: (Space, Literal),
    }

    pub type Shard = Prefix<StringEqCI!("SHARD"), ShardSpec>;
    pub type Sharded = Prefix<StringEqCI!("SHARDED"), ShardSpec>;

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ShardSpec {
        pub key: Option<(Space, StringEqCI!("KEY"), Space, FList1)>,
        pub coshard: Option<(Space, CoshardClause)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct CoshardClause {
        pub kw: StringEqCI!("COSHARD"),
        pub with: Option<(Space, StringEqCI!("WITH"))>,
        pub open: Option<(Space, StringEq!("("))>,
        pub name: (Space, TName),
        pub close: Option<(Space, StringEq!(")"))>,
    }

    #[derive(Grammar)]
    #[tygr(name = "table_option_list")]
    pub enum TableOptionList {
        WithOptions {
            shard: Option<(Shard, Space)>,
            partition: Option<(PartitionOption, Space)>,
            with_kw: (StringEqCI!("WITH"), Space),
            options: VecSep<TableOption, ArgSep>,
        },
        Shard {
            shard: Shard,
            partition: Option<(Space, PartitionOption)>,
        },
        Partition(PartitionOption),
        InformixCompat(InformixCompatability),
    }

    #[derive(Grammar)]
    #[tygr(name = "informix compatability")]
    pub struct InformixCompatability {
        pub in_on: Option<(InOn, Space, Token)>,
        pub extent_size: Option<(
            Space,
            StringEqCI!("EXTENT"),
            Space,
            StringEqCI!("SIZE"),
            Space,
            Literal,
        )>,
        pub next_size: Option<(
            Space,
            StringEqCI!("NEXT"),
            Space,
            StringEqCI!("SIZE"),
            Space,
            Literal,
        )>,
        pub lock_mode: Option<(
            Space,
            StringEqCI!("LOCK"),
            Space,
            StringEqCI!("MODE"),
            Option<(Space, LockModeType)>,
        )>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum LockModeType {
        Page(StringEqCI!("PAGE")),
        Row(StringEqCI!("ROW")),
    }

    #[derive(Grammar)]
    #[tygr(name = "table_option")]
    pub enum TableOption {
        ClassParam(ClassParameter),
        StorageType {
            kw: StringEqCI!("STORAGETYPE"),
            eq: Option<(Space, StringEq!("="))>,
            value: (Space, Storagetype),
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "partition option")]
    pub enum PartitionOption {
        PartitionBy {
            kw: (StringEqCI!("PARTITION"), Space, StringEqCI!("BY"), Space),
            list: PartitionSpecificationList,
        },
        PartitionOff(StringEqCI!("PARTITION"), Space, StringEqCI!("OFF")),
    }

    #[derive(Grammar)]
    #[tygr(name = "partition specification list")]
    pub struct PartitionSpecificationList {
        pub first: PartitionSpecification,
        pub second: Option<(ArgSep, PartitionSpecification)>,
    }

    #[derive(Grammar)]
    #[tygr(name = "partition specification")]
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
    #[tygr(name = "partition datetime interval")]
    pub enum PartitionDatetimeInterval {
        Day(StringEqCI!("DAY"), Option<StringEqCI!("S")>),
        Hour(StringEqCI!("HOUR"), Option<StringEqCI!("S")>),
        Minute(StringEqCI!("MINUTE"), Option<StringEqCI!("S")>),
        Month(StringEqCI!("MONTH"), Option<StringEqCI!("S")>),
        Year(StringEqCI!("YEAR"), Option<StringEqCI!("S")>),
    }

    #[derive(Grammar)]
    #[tygr(name = "partition range spec")]
    pub enum PartitionRangeSpec {
        PartitionId {
            kw: (StringEqCI!("PARTITION"), Space, StringEqCI!("ID"), Space),
            prefix: Literal,
        },
        PartitionIdBetween {
            kw: (StringEqCI!("PARTITION"), Space, StringEqCI!("ID")),
            prefix: Option<(Space, Literal)>,
            between_kw: (Space, StringEqCI!("BETWEEN"), Space),
            from: Literal,
            and_kw: (Space, StringEqCI!("AND"), Space),
            to: Literal,
        },
        PartitionBetween {
            kw: StringEqCI!("PARTITION"),
            prefix: Option<(Space, PartitionPrefix)>,
            between_kw: (Space, StringEqCI!("BETWEEN"), Space),
            from: Literal,
            and_kw: (Space, StringEqCI!("AND"), Space),
            to: Literal,
        },
        PartitionValues {
            kw: (StringEqCI!("PARTITION"), Space),
            prefix: PartitionPrefix,
        },
    }

    pub type PartitionPrefix = Parenthesized<VecSep<Literal, ArgSep>>;

    #[derive(Grammar)]
    #[tygr(name = "fld qualifier")]
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
            kw: (StringEqCI!("COMPUTEONCHANGE"), Space),
            list: FList1,
        },
        Default {
            kw: (StringEqCI!("DEFAULT"), Space),
            value: Setdef,
        },
        OnUpdate {
            kw: (StringEqCI!("ON"), Space, StringEqCI!("UPDATE"), Space),
            value: UpdateSpec,
        },
        Private(StringEqCI!("PRIVATE")),
        StorageType {
            kw: (StringEqCI!("STORAGETYPE"), Space, StringEq!("="), Space),
            value: Storagetype,
        },
        Transient(StringEqCI!("TRANSIENT")),
    }

    #[derive(Grammar)]
    #[tygr(name = "collate spec")]
    pub enum CollateSpec {
        SqlCollation(SqlCollation),
        Ident(Ident),
    }

    #[derive(Grammar)]
    #[tygr(name = "sqlcollation")]
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
            alphaup: Option<(Space, StringEqCI!("ALPHAUP"))>,
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
    #[tygr(name = "storagetype")]
    pub enum Storagetype {
        Columnar(StringEqCI!("COLUMNAR")),
        Row(StringEqCI!("ROW")),
    }

    #[derive(Grammar)]
    #[tygr(name = "datyp")]
    pub struct Datyp {
        pub spec: DatypSpec,
        pub args: Option<(Space, StringEq!("("), DatypArgs, StringEq!(")"))>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct DatypArgs(pub StringOf<IsNotParen>);

    #[derive(Grammar)]
    #[tygr(name = "datyp spec")]
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
        AnyToken {
            head: DatypName,
            rest: Vec<(StringEq!("."), Ident)>,
        },
    }

    #[derive(Grammar)]
    #[tygr(validated, inline)]
    pub struct DatypName(pub Ident);

    impl Validate for DatypName {
        const REQUIREMENT: &'static str = "not be a column clause keyword";
        fn validate(&self) -> bool {
            !matches!(
                self.0.print().to_uppercase().as_str(),
                "COLLATE" | "CONSTRAINT" | "DEFAULT" | "DROP" | "NOT" | "NULL" | "RENAME" | "SET"
            )
        }
    }

    #[derive(Grammar)]
    #[tygr(name = "fcon")]
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
                Option<(ArgSep, Literal)>,
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
    #[tygr(name = "check cnstr")]
    pub struct CheckCnstr {
        pub kw: (StringEqCI!("CHECK"), Space),
        pub parse: Chkparse,
        pub informix: Informix,
    }

    #[derive(Grammar)]
    #[tygr(name = "named fcon spec")]
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
    #[tygr(name = "update spec")]
    pub enum UpdateSpec {
        Negative {
            minus: (StringEq!("-"), Space),
            value: Number,
        },
        Literal(Literal),
        CurrentDate(StringEqCI!("CURRENT_DATE")),
        CurrentTimestamp {
            kw: StringEqCI!("CURRENT_TIMESTAMP"),
            precision: Option<(Space, Parenthesized<Precision>)>,
        },
        CurrentTime {
            kw: StringEqCI!("CURRENT_TIME"),
            precision: Option<(Space, Parenthesized<Precision>)>,
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
    #[tygr(name = "cluster")]
    pub struct Cluster(pub Option<ClusterType>);

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum ClusterType {
        Clustered(StringEqCI!("CLUSTERED")),
        NonClustered(StringEqCI!("NONCLUSTERED")),
    }

    #[derive(Grammar)]
    #[tygr(name = "ref")]
    pub struct Ref {
        pub name: TName,
        pub flist: Option<(Space, FList1)>,
        pub match_spec: Option<(Space, StringEqCI!("MATCH"), Space, MatchSpec)>,
        pub triggered_action: Option<(Space, TriggeredAction)>,
        pub nocheck: Option<(Space, StringEqCI!("NOCHECK"))>,
    }

    /// `<triggered action> ::= <delete rule> [ <update rule> ] | <update rule> [ <delete rule> ]`
    #[derive(Grammar)]
    #[tygr(name = "triggered action")]
    pub enum TriggeredAction {
        DeleteFirst {
            delete_rule: DeleteRule,
            update_rule: Option<(Space, UpdateRule)>,
        },
        UpdateFirst {
            update_rule: UpdateRule,
            delete_rule: Option<(Space, DeleteRule)>,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "positive integer", validated)]
    pub struct PositiveInteger(pub Number);

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

    pub type UpdateRule =
        Prefix<(StringEqCI!("ON"), Space, StringEqCI!("UPDATE"), Space), RefAction>;

    pub type DeleteRule =
        Prefix<(StringEqCI!("ON"), Space, StringEqCI!("DELETE"), Space), RefAction>;

    #[derive(Grammar)]
    #[tygr(name = "match spec")]
    pub enum MatchSpec {
        Full(StringEqCI!("FULL")),
        Partial(StringEqCI!("PARTIAL")),
    }

    #[derive(Grammar)]
    #[tygr(name = "ref action")]
    pub enum RefAction {
        Cascade(StringEqCI!("CASCADE")),
        NoAction(StringEqCI!("NO"), Space, StringEqCI!("ACTION")),
        SetDefault(StringEqCI!("SET"), Space, StringEqCI!("DEFAULT")),
        SetNull(StringEqCI!("SET"), Space, StringEqCI!("NULL")),
    }

    #[derive(Grammar)]
    #[tygr(name = "informix")]
    pub struct Informix(pub Option<(Space, StringEqCI!("CONSTRAINT"), Space, TName)>);

    pub type Chkparse = Prefix<StringEq!("("), StringEndsWith<StringEq!(")")>>;

    #[derive(Grammar)]
    #[tygr(name = "flist 2")]
    pub enum FList2 {
        FList1(FList1),
        IdentList {
            idents: VecSep<Ident, ArgSep>,
            flist: Option<(Space, FList1)>,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "load-spec")]
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
    #[tygr(name = "load-hints")]
    pub struct LoadHints(pub Vec<(LoadHint, Space)>);

    #[derive(Grammar)]
    #[tygr(name = "load-hint")]
    pub enum LoadHint {
        NoCheck(StringEqCI!("%NOCHECK")),
        NoIndex(StringEqCI!("%NOINDEX")),
        NoJourn(StringEqCI!("%NOJOURN")),
        NoLock(StringEqCI!("%NOLOCK")),
        Bulk(StringEqCI!("BULK")),
    }

    #[derive(Grammar)]
    #[tygr(name = "load-data-spec")]
    pub enum LoadDataSpec {
        Inline(LoadDataSpecInline),
        WithUsing(LoadDataSpecWithUsing),
    }

    #[derive(Grammar)]
    #[tygr(name = "load-data-spec-inline")]
    pub struct LoadDataSpecInline {
        pub from: LoadDataFrom,
        pub into: (Space, LoadDataInto),
        pub using: Option<(Space, LoadDataUsing)>,
    }

    #[derive(Grammar)]
    #[tygr(name = "load-data-spec-with-using")]
    pub struct LoadDataSpecWithUsing {
        pub from: Option<(LoadDataFrom, Space)>,
        pub into: Option<(LoadDataInto, Space)>,
        pub using: LoadDataUsing,
    }

    pub type LoadDataFrom = Prefix<(StringEqCI!("FROM"), Space), DataSource>;

    #[derive(Grammar)]
    #[tygr(name = "data-source")]
    pub enum DataSource {
        File(FileSource),
        Jdbc {
            kw: (StringEqCI!("JDBC"), Space),
            cnxn: JdbcCnxnSpec,
            data: (Space, JdbcDataSpec),
        },
    }

    pub type ColumnDefList = VecSep<ColumnDefItem, ArgSep>;

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct FileSource {
        pub kw: (StringEqCI!("FILE"), Space),
        pub path: Literal,
        pub columns: Option<(
            Space,
            StringEqCI!("COLUMNS"),
            Space,
            Parenthesized<ColumnDefList>,
        )>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ColumnDefItem {
        pub name: Ident,
        pub datyp: (Space, Datyp),
    }

    #[derive(Grammar)]
    #[tygr(name = "jdbc-cnxn-spec")]
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
    #[tygr(name = "jdbc-data-spec")]
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
    #[tygr(name = "cnxn-spec")]
    pub enum CnxnSpec {
        Ident(Ident),
        Literal(Literal),
    }

    #[derive(Grammar)]
    #[tygr(name = "load-data-into")]
    pub struct LoadDataInto {
        pub kw: (StringEqCI!("INTO"), Space),
        pub table: TName,
        pub columns: Option<(Space, FList1)>,
        pub values: Option<(Space, StringEqCI!("VALUES"), Space, FList1)>,
    }

    pub type LoadDataUsing = Prefix<(StringEqCI!("USING"), Space), LoadOptions>;

    #[derive(Grammar)]
    #[tygr(name = "load-options")]
    pub enum LoadOptions {
        Json(JSONString),
        Name(Ident),
    }

    #[derive(Grammar)]
    #[tygr(name = "load-sql-spec")]
    pub struct LoadSqlSpec {
        pub kw: (StringEqCI!("FROM"), Space),
        pub source: SqlSource,
        pub dialect: Option<(Space, LoadSqlDialect)>,
        pub delimiter: Option<(Space, LoadSqlDelimiter)>,
        pub verbose: Option<(Space, StringEqCI!("VERBOSE"))>,
        pub log_to_file: Option<(Space, LoadSqlLogToFile)>,
        pub ignore_errors: Option<(Space, StringEqCI!("IGNORE"), Space, StringEqCI!("ERRORS"))>,
    }

    #[derive(Grammar)]
    #[tygr(name = "sql-source")]
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
    #[tygr(name = "alter-server-action")]
    pub enum AlterServerAction {
        Alter {
            kw: (StringEqCI!("ALTER"), Space),
            action: SubAlterServerAction,
        },
        Modify {
            kw: (StringEqCI!("MODIFY"), Space),
            actions: VecSep<SubAlterServerAction, ArgSep>,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "sub-alter-server-action")]
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
    #[tygr(name = "vector-datyp")]
    pub enum VectorDatyp {
        Decimal(StringEqCI!("DECIMAL")),
        Double(StringEqCI!("DOUBLE")),
        Float(StringEqCI!("FLOAT")),
        Int(StringEqCI!("INT")),
        Integer(StringEqCI!("INTEGER")),
        String(StringEqCI!("STRING")),
    }

    #[derive(Grammar)]
    #[tygr(name = "stats clause")]
    pub enum StatsClause {
        Statistics(StringEqCI!("STATISTICS")),
        Stats(StringEqCI!("STATS")),
    }

    #[derive(Grammar)]
    #[tygr(name = "stats scope")]
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
    #[tygr(name = "stats property")]
    pub enum StatsProperty {
        IgnoreFixedStats(StringEqCI!("IGNOREFIXEDSTATS")),
        SkipAutomaticStatsCollection(StringEqCI!("SKIPAUTOMATICSTATSCOLLECTION")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tygr::{Parse, Print};

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
             TIMESTAMPADD(:unit, 1, d), CONVERT(a, SQL_VARCHAR), CONVERT(VARCHAR, a), f(a) FROM t",
        )
        .node
        .selst
        .select_list
        .0
        .items
        .iter()
        .map(|item| match &item.node {
            SelectItem::Expr {
                expr: Scalx::Legacy(terms),
                ..
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
                "call",
                "builtin",
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
        let shapes: Vec<&str> = selex.node.where_clause.as_ref().unwrap().prefixed.0.items[0]
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
    fn one_order_direction() {
        assert!(SQL::parse("SELECT a FROM t ORDER BY a ASC DESC").is_err());
    }

    #[test]
    fn dml_options_of_each_statement() {
        all_ok(&[
            "INSERT %NOSTREAM INTO t (a) VALUES (1)",
            "UPDATE %ILOCK %NOSTREAM t SET a = 1",
            "DELETE %NOJOURN FROM t",
            "TRUNCATE TABLE %NOCHECK %NOLOCK %NOJOURN t",
        ]);
        for rejected in [
            "INSERT %ILOCK INTO t (a) VALUES (1)",
            "DELETE %NOSTREAM FROM t",
            "DELETE %ILOCK FROM t",
            "TRUNCATE TABLE %NOINDEX t",
            "TRUNCATE TABLE %PROFILE t",
        ] {
            assert!(SQL::parse(rejected).is_err(), "{rejected}");
        }
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
            "SELECT SYSDATE() FROM t",
        ] {
            assert!(SQL::parse(rejected).is_err(), "{rejected}");
        }
    }
}
