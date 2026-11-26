//! The IRIS SQL grammar for `&sql(...)` blocks embedded in ObjectScript, reached from `cos.rs`.
//!
//! Written once under `#[levels]`, which makes it twice: `full`, every spelling IRIS accepts, is
//! re-exported here; `core` lacks the syntactic sugar, which `desugar` rewrites into it and the
//! checker reads.
//!
//! Its rules of placement: a rule describes its own text, the blank before or after it belonging to
//! the sequence that places it, and a keyword run is one tuple; one wrapper per bracket kind,
//! `Parenthesized<T>` for every pair of parentheses; tygr's combinators before a hand-rolled shape,
//! an alias only where it shortens; and two rules reading the same text are one.

use std::cell::{Cell, RefCell};
use std::ops::{Deref, Range};

use either::Either;
use tygr::*;

use crate::convert::VecSep;
use crate::cos;

/// A keyword, not the start of a longer word: IRIS reads `NOTBETWEEN`, `BETWEEN1`, `TOP1` and `%NOINDEXa` as names.
macro_rules! Kw {
    ($word:literal) => { (StringEqCI!($word), NotFollowedBy<WordEnd>) };
}

/// A token in its canonical spelling or another, `Spellings!(Kw!("$HOROLOG"), Kw!("$H"))`; the core
/// has the canonical one alone, which `Default` spells.
macro_rules! Spellings {
    ($canonical:ty, $other:ty $(,)?) => { Spellings<$canonical, $other> };
    ($canonical:ty, $($other:ty),+ $(,)?) => { Spellings<$canonical, Spellings!($($other),+)> };
}

#[derive(Grammar, Clone)]
#[tygr(inline)]
pub enum Spellings<Canonical: Grammar, Other: Grammar> {
    Canonical(Canonical),
    Other(Other),
}

impl<Canonical: Grammar + Default, Other: Grammar> Default for Spellings<Canonical, Other> {
    fn default() -> Self {
        Spellings::Canonical(Default::default())
    }
}

pub mod desugar;

pub use full::*;

char_class!(pub IsSpace, "simple_space", |ch| matches!(
    ch,
    ' ' | '\t' | '\n' | '\r'
));
pub type Blanks = Hidden<StringOf1<IsSpace>>;

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

pub type StringEndsWith<T> = Raw<(Vec<(NotFollowedBy<T>, IsAnyChar)>, T)>;

pub type SQLLineComment = Wrap<StringEq!("--"), StringOf<IsNotNL>, Option<StringEq!("\n")>>;

pub type SQLMultiLineComment = Prefix<StringEq!("/*"), StringEndsWith<StringEq!("*/")>>;

char_class!(pub IsExtFuncChar, "non left paren character", |ch| {
    !matches!(ch, '(' | '$')
});

char_class!(pub IsNotParen, "any tokens except \"(\" and \")\"", |ch| ch
    != '('
    && ch != ')');

#[derive(Grammar, Clone)]
#[tygr(hidden)]
pub enum SpaceUnit {
    Raw(Blanks),
    LC(SQLLineComment),
    MC(SQLMultiLineComment),
}

/// Blanks and comments, possibly none: around punctuation.
#[derive(Grammar, Clone, Default)]
#[tygr(hidden)]
pub struct Space(Vec<SpaceUnit>);

/// Blanks and comments, at least one: between two words, which would otherwise fuse.
#[derive(Grammar, Clone)]
#[tygr(hidden)]
pub struct Space1(Vec1<SpaceUnit>);

impl Default for Space1 {
    fn default() -> Self {
        Space1(
            Vec1::try_from(vec![SpaceUnit::Raw(Hidden(Raw::new(" ")))])
                .ok()
                .expect("one unit"),
        )
    }
}

impl Space {
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
pub enum IdentifierStart {
    SimpleLatin(IsSimpleLatinLetter),
    NonAscii(IsNonAsciiLetter),
    P(StringEq!("%")),
    U(StringEq!("_")),
}

#[derive(Grammar, Clone)]
pub enum IdentifierPart {
    Digit(IsDigit),
    SimpleLatin(IsSimpleLatinLetter),
    NonAscii(IsNonAsciiLetter),
    H(StringEq!("#")),
    A(StringEq!("@")),
    U(StringEq!("_")),
    D(StringEq!("$")),
}

/// What a keyword must not run into; hidden, so a keyword's EBNF is the word alone.
#[derive(Grammar, Clone)]
#[tygr(hidden)]
pub struct WordEnd(IdentifierPart);

#[derive(Grammar, Clone)]
#[tygr(inline)]
pub struct IdentifierStruct {
    first: IdentifierStart,
    rest: Vec<IdentifierPart>,
}

/// 2026.2 reads `a$b`, `ñ` and `Яа` as names and refuses `_` alone.
#[derive(Grammar, Clone)]
#[tygr(validated)]
pub struct Identifier(Raw<IdentifierStruct>);

impl Validate for Identifier {
    const REQUIREMENT: &'static str = "not be underscores alone";
    fn validate(&self) -> bool {
        self.0.as_str().chars().any(|ch| ch != '_')
    }
}

#[derive(Grammar, Clone)]
#[tygr(inline)]
pub struct SQLDelimitedIdentStruct {
    open: StringEq!("\""),
    head: DelimitedIdentifierPart,
    tail: Vec<DelimitedIdentifierPart>,
    close: StringEq!("\""),
}

#[derive(Grammar, Clone)]
pub enum DelimitedIdentifierPart {
    DDQ(StringEq!("\"\"")),
    Char(IsNotDQ),
}

#[derive(Grammar, Clone)]
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
pub enum StdCharacterRepresentation {
    DSQ(StringEq!("''")),
    Char(IsNotSQ),
}

#[derive(Grammar, Clone)]
pub enum StringLiteral {
    SQLEmpty(StringEq!("''")),
    Std(Raw<StdStringLiteral>),
}

#[derive(Grammar, Clone)]
pub enum Sign {
    Plus(StringEq!("+")),
    Minus(StringEq!("-")),
}

#[derive(Grammar, Clone)]
pub struct Integer {
    sign: Option<Sign>,
    integer: StringOf1<IsDigit>,
}

#[derive(Grammar, Clone)]
#[tygr(inline)]
pub struct SQLExponent {
    e: ExpoSymbol,
    sign: Option<Sign>,
    digits: StringOf1<IsDigit>,
}

#[derive(Grammar, Clone)]
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
pub struct ParameterMarker(pub StringEq!("?"));

/// 2026.2 refuses `:i%Prop()`, an instance variable with an empty subscript list, and takes `:arr()`.
#[derive(Grammar, Clone)]
#[tygr(validated)]
pub struct HostVar {
    pub host_ref: HostRef,
    pub subs: Option<SubList>,
}

impl Validate for HostVar {
    const REQUIREMENT: &'static str = "subscript an instance variable with something";
    fn validate(&self) -> bool {
        !(matches!(self.host_ref, HostRef::InstanceVariableHostRef(_))
            && self
                .subs
                .as_ref()
                .is_some_and(|subs| subs.wrapped.is_none()))
    }
}

pub type SubList = Parenthesized<Option<VecSep<Sub, ArgSep>>>;

#[derive(Grammar, Clone)]
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
pub enum HostRef {
    InstanceVariableHostRef(InstanceVariableHostRef),
    PropertyHostRef(PropertyHostRef),
    Plain(Option<StringEq!(":")>, HostIdentifier),
}

#[derive(Grammar, Clone)]
pub struct HostIdentifier(Raw<(HostIdentifierStart, Vec<HostIdentifierPart>)>);

/// A letter or `%`: 2026.2 refuses `:_x` and `:1x`.
#[derive(Grammar, Clone)]
pub enum HostIdentifierStart {
    SimpleLatin(IsSimpleLatinLetter),
    Unicode(IsNonAsciiLetter),
    P(StringEq!("%")),
}

#[derive(Grammar, Clone)]
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

/// The functions whose arguments both levels spell: the builtins, aggregates and window functions.
const OWN_SYNTAX: &[&str] = &[
    "CAST",
    "JSON_ARRAY",
    "JSON_OBJECT",
    "PREDICT",
    "PROBABILITY",
    "SEARCH_INDEX",
    "TIMESTAMPADD",
    "TIMESTAMPDIFF",
    "TO_VECTOR",
    "TRIM",
    "CURRENT_TIMESTAMP",
    "CURRENT_TIME",
    "XMLCONCAT",
    "XMLELEMENT",
    "XMLFOREST",
    "XMLATTRIBUTES",
    "DATEADD",
    "DATEDIFF",
    "DATENAME",
    "DATEPART",
    "DATE_TRUNC",
    "AVG",
    "COUNT",
    "SUM",
    "MAX",
    "MIN",
    "LIST",
    "XMLAGG",
    "%DLIST",
    "STDDEV",
    "STDDEV_POP",
    "STDDEV_SAMP",
    "VARIANCE",
    "VAR_POP",
    "VAR_SAMP",
    "JSON_ARRAYAGG",
    "APPROX_COUNT_DISTINCT",
    "ROW_NUMBER",
    "RANK",
    "DENSE_RANK",
    "PERCENT_RANK",
    "CUME_DIST",
    "NTILE",
    "FIRST_VALUE",
    "LAST_VALUE",
    "NTH_VALUE",
    "LEAD",
    "LAG",
];

/// The functions the core calls by name; the full level, where they have their own syntax, shadows this.
const SCALAR_FUNCTIONS: &[&str] = &[];

/// Whether a call by name spells an ObjectScript function, `$EXTRACT(x, 1)`: the core's way; the full level,
/// where each is a scalar function, shadows this.
const DOLLAR_CALLS: bool = true;

#[tygr_objectscript_macros::levels]
pub mod leveled {
    pub type Parenthesized<T> = Wrap<(StringEq!("("), Space), T, (Space, StringEq!(")"))>;

    use either::Either;
    use tygr::*;

    use super::*;
    use crate::convert::VecSep;

    #[sugar]
    use super::sugar::*;

    #[derive(Grammar)]
    pub struct TName(pub Src<DottedIdent>);

    /// A function's name, `$` before an ObjectScript function's (`$EXTRACT`, `$TSQL_SPID`).
    #[derive(Grammar)]
    pub struct SQLFunctionName {
        pub dollar: Option<StringEq!("$")>,
        pub name: Src<DottedIdent>,
    }

    #[derive(Grammar)]
    pub enum Colmn {
        Str(Src<Ident>, Space, StringEq!("."), Space, StringEq!("*")),
        Arr(
            Src<DottedIdent>,
            Vec<(Space, StringEq!("->"), Space, Ident)>,
        ),
    }

    #[derive(Grammar)]
    pub struct Precision(pub NonNegativeInteger);

    #[derive(Grammar)]
    pub struct NonNegativeInteger(pub StringOf1<IsDigit>);

    #[derive(Grammar)]
    pub enum OrderPredicate {
        Le(Spellings!(StringEq!("<="), StringEqCI!("NOT>"))),
        Ge(Spellings!(StringEq!(">="), StringEqCI!("NOT<"))),
        Gt(StringEq!(">")),
        Lt(StringEq!("<")),
    }

    #[derive(Grammar)]
    pub enum ContainsPredicate {
        LB(StringEq!("[")),
        NotLB(StringEqCI!("NOT[")),
    }

    #[derive(Grammar)]
    pub enum FollowsPredicate {
        Follows(StringEq!("]")),
        NotFollows(StringEqCI!("NOT]")),
    }

    #[derive(Grammar)]
    pub enum CmpOp {
        StartsWith(Kw!("%STARTSWITH")),
        Eq(StringEq!("=")),
        Ne(Spellings!(StringEq!("<>"), StringEq!("!="), StringEqCI!("NOT="))),
        Ord(OrderPredicate),
        Contains(ContainsPredicate),
        Follows(FollowsPredicate),
    }

    #[derive(Grammar)]
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
        Niladic(Niladic),
        Call(Call),
        Column(Colmn),
        SpecialSystemVariable(SpecialSystemVariable),
        HostVar(HostVar),
        Null(Kw!("NULL")),
    }

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

    fn scalar_function(function: &ScalarFunction) -> core::Term {
        calls!(ScalarFunction: function;
            PercentObject PercentOid Similarity Abs Ascii Ceiling CharacterLength CharIndex CharLength Char
            Coalesce Concat Convert DataLength Date Day Decode
            Embedding Floor GetDate GetUtcDate Greatest HashBytes IfNull Instr IsNull IsNumeric LastDay
            LastIdentity LCase Least Left Len Length Lower LPad LTrim Month Now NullIf Nvl PatIndex Power
            Repeat Replace Replicate Reverse Right Round RPad RTrim Sign Space Str String Stuff Substr
            Substring ToChar ToDate ToUSChar ToUSDate ToNumber ToTimestamp UCase UnixTimestamp
            VectorCosine VectorDotProduct Year DollarExtract DollarFind DollarFNumber DollarJustify
            DollarLength DollarList DollarListBuild DollarListData DollarListFind DollarListFromString
            DollarListGet DollarListLength DollarListSame DollarListToString DollarMvFmt DollarMvFmts
            DollarMvIconv DollarMvIconvs DollarMvOconv DollarMvOconvs DollarPiece DollarTranslate
            TSqlDbName TSqlError TSqlFetchStatus TSqlGetUtcDate TSqlHostName TSqlLockTimeout TSqlNestLevel
            TSqlServerName TSqlSpid TSqlSqlStatus TSqlTranCount TSqlVersion TSqlNewId TSqlObjectId
            TSqlObjectName;;
            ScalarFunction::Position(_, a) => call("CHARINDEX", args(a)),
        )
    }

    fn odbc_function(function: &ODBCFunc) -> core::Term {
        calls!(ODBCFunc: function;
            Abs Acos Asin Atan2 Atan Ceiling Cos Cot Exp Floor Log Log10 Mod Power Sin Sqrt Tan Ascii Char
            Concat DayName DayOfMonth DayOfWeek DayOfYear Degrees Hour LCase Left Length LTrim Minute Month
            MonthName Quarter Radians Repeat Right Round RTrim Second Sign Space Substring Truncate UCase Week
            Year;
            Pi CurDate CurTime Database Now;
            ODBCFunc::TimestampAdd(kw, a) => {
                core::Term::Builtin(Box::new(core::Builtin::TimestampAdd(kw.desugar(), a.desugar())))
            }
            ODBCFunc::TimestampDiff(kw, a) => {
                core::Term::Builtin(Box::new(core::Builtin::TimestampDiff(kw.desugar(), a.desugar())))
            }
        )
    }

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
            expr,
            as_kw: (Space::blank(), Default::default(), Default::default()),
            cast_type,
        };
        core::Term::Builtin(Box::new(core::Builtin::Cast(
            Default::default(),
            (Space::default(), parenthesized(Box::new(cast))),
        )))
    }

    fn odbc_escape(escape: &ODBCEscape) -> core::Term {
        match &escape.wrapped {
            DTS::DateTime(value) => cast(value),
            DTS::Function(function) => odbc_function(&function.prefixed),
        }
    }

    fn percent_call(op: &PercentOp, term: &Src<Term>) -> core::Term {
        let arg = match &term.node {
            Term::Scalx(parens) => (*parens.wrapped).desugar(),
            _ => core::Scalx(VecSep::one(term.desugar())),
        };
        call(&op.print(), vec![arg])
    }

    fn string_operation(operation: &StringOperation) -> core::Term {
        let (expr, _, truncation) = &operation.args.1.wrapped;
        let truncation = match truncation {
            ColTrun::Const(constant) => {
                let core::PConst::Literal(literal) = constant.desugar();
                core::Term::Literal(literal)
            }
            ColTrun::HostVar(host_var) => core::Term::HostVar(host_var.clone()),
        };
        let args = vec![expr.desugar(), term(truncation)];
        call(&operation.op.print(), args)
    }

    #[derive(Grammar)]
    pub enum SpecialSystemVariable {
        Horolog(Spellings!(Kw!("$HOROLOG"), Kw!("$H"))),
        Job(Spellings!(Kw!("$JOB"), Kw!("$J"))),
        Namespace(Spellings!(Kw!("$NAMESPACE"), Kw!("$ZNSPACE"))),
        TLevel(Spellings!(Kw!("$TLEVEL"), Kw!("$TL"))),
        Username(Kw!("$USERNAME")),
        ZHorolog(Spellings!(Kw!("$ZHOROLOG"), Kw!("$ZH"))),
        ZJob(Spellings!(Kw!("$ZJOB"), Kw!("$ZJ"))),
        ZPi(Kw!("$ZPI")),
        ZTimestamp(Spellings!(Kw!("$ZTIMESTAMP"), Kw!("$ZTS"))),
        ZTimezone(Spellings!(Kw!("$ZTIMEZONE"), Kw!("$ZTZ"))),
        ZVersion(Spellings!(Kw!("$ZVERSION"), Kw!("$ZV"))),
    }

    /// A function called by name, built-in or user-defined; a function with a syntax of its own cannot be
    /// called otherwise (2026.2 rejects TIMESTAMPADD(:u, 1, a), CONCAT(b, b, b), ROUND(a) and COUNT(a, b)),
    /// and a `$` name is a scalar function's or nothing (2026.2 rejects $ASCII(x)).
    #[derive(Grammar)]
    #[tygr(validated)]
    pub struct Call {
        pub name: SQLFunctionName,
        pub args: ArgsN,
    }

    impl Validate for Call {
        const REQUIREMENT: &'static str =
            "not name a function with a syntax of its own or an unknown $ function";
        fn validate(&self) -> bool {
            let name = self.name.print().to_uppercase();
            (DOLLAR_CALLS || self.name.dollar.is_none())
                && !OWN_SYNTAX.contains(&name.as_str())
                && !SCALAR_FUNCTIONS.contains(&name.as_str())
        }
    }

    /// The names the full level spells; a const stays in the full level alone, so the core sees the list above.
    const DOLLAR_CALLS: bool = false;
    const SCALAR_FUNCTIONS: &[&str] = &[
        "%OBJECT",
        "%OID",
        "%SIMILARITY",
        "ABS",
        "ASCII",
        "CEILING",
        "CHARACTER_LENGTH",
        "CHARINDEX",
        "CHAR_LENGTH",
        "CHAR",
        "COALESCE",
        "CONCAT",
        "CONVERT",
        "DATALENGTH",
        "DATE",
        "DAY",
        "DECODE",
        "EMBEDDING",
        "FLOOR",
        "GETDATE",
        "GETUTCDATE",
        "GREATEST",
        "HASHBYTES",
        "IFNULL",
        "INSTR",
        "ISNULL",
        "ISNUMERIC",
        "LAST_DAY",
        "LAST_IDENTITY",
        "LCASE",
        "LEAST",
        "LEFT",
        "LEN",
        "LENGTH",
        "LOWER",
        "LPAD",
        "LTRIM",
        "MONTH",
        "NOW",
        "NULLIF",
        "NVL",
        "PATINDEX",
        "POSITION",
        "POWER",
        "REPEAT",
        "REPLACE",
        "REPLICATE",
        "REVERSE",
        "RIGHT",
        "ROUND",
        "RPAD",
        "RTRIM",
        "SIGN",
        "SPACE",
        "STR",
        "STRING",
        "STUFF",
        "SUBSTR",
        "SUBSTRING",
        "TOCHAR",
        "TODATE",
        "TO_CHAR",
        "TO_DATE",
        "TO_NUMBER",
        "TO_TIMESTAMP",
        "UCASE",
        "UNIX_TIMESTAMP",
        "VECTOR_COSINE",
        "VECTOR_DOT_PRODUCT",
        "YEAR",
    ];

    /// A function without parentheses; `USER` takes an empty pair too (2026.2).
    #[derive(Grammar)]
    pub enum Niladic {
        CurrentDate(Kw!("CURRENT_DATE")),
        CurrentTimestamp(Kw!("CURRENT_TIMESTAMP")),
        CurrentTime(Kw!("CURRENT_TIME")),
        SysDate(Kw!("SYSDATE")),
        User(Kw!("USER"), Option<Args<()>>),
    }

    #[derive(Grammar)]
    pub enum Scalop {
        Concat(Spellings!(StringEq!("||"), StringEq!("_"))),
        Mod(StringEq!("#")),
        Mul(StringEq!("*")),
        Add(StringEq!("+")),
        Sub(StringEq!("-")),
        Div(StringEq!("/")),
        IntDiv(StringEq!("\\")),
    }

    #[derive(Grammar)]
    pub struct Scalx(pub VecSep<Src<Term>, (Space, Scalop, Space)>);
    #[derive(Grammar)]
    pub struct Case {
        pub case_kw: Kw!("CASE"),
        pub body: CaseBody,
        pub else_clause: Implied<SQLElseClause>,
        pub end_kw: (Space, Kw!("END")),
    }

    /// Searched, each WHEN a condition, or simple, each WHEN a value the operand is compared to; 2026.2
    /// rejects a searched CASE without WHEN and takes a simple one.
    #[derive(Grammar)]
    pub enum CaseBody {
        Searched(Vec1<(Space, When<Prdct>)>),
        Simple(Space, Scalx, Vec<(Space, When<Scalx>)>),
    }

    pub type WhenKw = Kw!("WHEN");

    pub type ThenKw = Kw!("THEN");

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct When<T: Grammar> {
        pub when_kw: (WhenKw, Space),
        pub operand: T,
        pub then_kw: (Space, ThenKw, Space),
        pub result: Scalx,
    }

    /// Omitted, a CASE reads `ELSE NULL`.
    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLElseClause {
        pub else_kw: (Kw!("ELSE"), Space),
        pub result: Scalx,
    }

    impl Default for SQLElseClause {
        fn default() -> Self {
            SQLElseClause {
                else_kw: (Default::default(), Space::blank()),
                result: Scalx(VecSep::one(Src::synthesized(
                    Term::Null(Default::default()),
                ))),
            }
        }
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLCastExpression {
        pub expr: Scalx,
        pub as_kw: (Space, Kw!("AS"), Space1),
        pub cast_type: CastType,
    }

    /// A blank and the parenthesized arguments of a function; in a shape, R is a required argument
    /// and O an optional one.
    pub type Args<T> = (Space, Parenthesized<T>);

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct RR<T1: Grammar = Scalx, T2: Grammar = Scalx> {
        pub arg1: T1,
        pub sep1: ArgSep,
        pub arg2: T2,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct RRR<T1: Grammar = Scalx, T2: Grammar = Scalx, T3: Grammar = Scalx> {
        pub arg1: T1,
        pub sep1: ArgSep,
        pub arg2: T2,
        pub sep2: ArgSep,
        pub arg3: T3,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct RRRR<
        T1: Grammar = Scalx,
        T2: Grammar = Scalx,
        T3: Grammar = Scalx,
        T4: Grammar = Scalx,
    > {
        pub arg1: T1,
        pub sep1: ArgSep,
        pub arg2: T2,
        pub sep2: ArgSep,
        pub arg3: T3,
        pub sep3: ArgSep,
        pub arg4: T4,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct RO<T1: Grammar = Scalx, T2: Grammar = Scalx> {
        pub arg1: T1,
        pub arg2: Option<(ArgSep, T2)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ROO<T1: Grammar = Scalx, T2: Grammar = Scalx, T3: Grammar = Scalx> {
        pub arg1: T1,
        pub rest: Option<(ArgSep, T2, Option<(ArgSep, T3)>)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct RRO<T1: Grammar = Scalx, T2: Grammar = Scalx, T3: Grammar = Scalx> {
        pub arg1: T1,
        pub sep1: ArgSep,
        pub arg2: T2,
        pub arg3: Option<(ArgSep, T3)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct RROO<
        T1: Grammar = Scalx,
        T2: Grammar = Scalx,
        T3: Grammar = Scalx,
        T4: Grammar = Scalx,
    > {
        pub arg1: T1,
        pub sep1: ArgSep,
        pub arg2: T2,
        pub rest: Option<(ArgSep, T3, Option<(ArgSep, T4)>)>,
    }

    pub type ArgsN = Args<Option<VecSep<Scalx, ArgSep>>>;

    pub type Args1Plus = Args<VecSep<Scalx, ArgSep>>;

    /// The functions with a syntax of their own, CURRENT_TIMESTAMP and CURRENT_TIME with a precision,
    /// which the niladic would shadow, and the ODBC-only functions.
    #[derive(Grammar)]
    pub enum Builtin {
        XmlConcat(XMLConcat),
        XmlElement(XMLElement),
        XmlForest(XMLForest),
        Cast(Kw!("CAST"), Args<Box<SQLCastExpression>>),
        DateAdd(Kw!("DATEADD"), Args<RRR<DatePart>>),
        DateDiff(Kw!("DATEDIFF"), Args<RRR<DatePart>>),
        DateName(Kw!("DATENAME"), Args<RR<DatePart>>),
        DatePart(Kw!("DATEPART"), Args<RR<DatePartPart>>),
        DateTrunc(Kw!("DATE_TRUNC"), Args<RR<DatePart>>),
        JSONArray(Kw!("JSON_ARRAY"), SQLJSONArrayArgs),
        JSONObject(Kw!("JSON_OBJECT"), SQLJSONObjectArgs),
        Predict(Kw!("PREDICT"), Args<SQLPredictArgs>),
        Probability(Kw!("PROBABILITY"), Args<SQLProbabilityArgs>),
        SearchIndex(Kw!("SEARCH_INDEX"), Args<SQLSearchIndexArgs>),
        TimestampAdd(
            Kw!("TIMESTAMPADD"),
            Args<RRR<DoubleQuotable<TSAddInterval>>>,
        ),
        TimestampDiff(
            Kw!("TIMESTAMPDIFF"),
            Args<RRR<DoubleQuotable<TSDiffInterval>>>,
        ),
        ToVector(Kw!("TO_VECTOR"), Args<ROO<Scalx, VectorDatyp, Integer>>),
        Trim(Kw!("TRIM"), Args<TrimArgs>),
        Odbc(ODBCOnly),
        CurrentTimestamp(Kw!("CURRENT_TIMESTAMP"), Args<Scalx>),
        CurrentTime(Kw!("CURRENT_TIME"), Args<Scalx>),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum OptionallyWrapped<T: Grammar> {
        Wrapped(Parenthesized<T>),
        Scalar(T),
    }

    #[derive(Grammar)]
    pub struct TrimArgs {
        pub how: TrimHow,
        pub expr: Scalx,
    }

    /// `BOTH ' ' FROM` in the core: a lone spec trims blanks, lone chars both ends.
    #[derive(Grammar)]
    pub enum TrimHow {
        SpecChars(TrimSpec, Space, Scalx, (Space, Kw!("FROM"), Space)),
        #[sugar]
        Spec(TrimSpec, Space, Option<(Kw!("FROM"), Space)>),
        #[sugar]
        Chars(Option<(Scalx, Space, Kw!("FROM"), Space)>),
    }

    impl Desugar for TrimHowSugar<'_> {
        type Core = core::TrimHow;
        fn desugar(&self) -> Self::Core {
            let (spec, chars) = match self {
                Self::Spec(spec, ..) => (spec.desugar(), string(" ")),
                Self::Chars(chars) => (
                    core::TrimSpec::Both(Default::default()),
                    chars
                        .as_ref()
                        .map_or_else(|| string(" "), |(chars, ..)| chars.desugar()),
                ),
            };
            core::TrimHow::SpecChars(
                spec,
                Space::blank(),
                chars,
                (Space::blank(), Default::default(), Space::blank()),
            )
        }
    }

    #[derive(Grammar)]
    pub enum TrimSpec {
        Leading(Kw!("LEADING")),
        Trailing(Kw!("TRAILING")),
        Both(Kw!("BOTH")),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLSearchIndexArgs {
        pub table: Vec<(Ident, StringEq!("."))>,
        pub index_name: IndexName,
        pub scalx_list: Vec<(ArgSep, Scalx)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLPredictArgs {
        pub model: ModelName,
        pub use_clause: Option<(Space, PredictProbabilityUse)>,
        pub with_clause: Option<((Space, Kw!("WITH"), Space), PredictWith)>,
    }

    pub type PredictProbabilityUse = Prefix<(Kw!("USE"), Space), TrainedModelName>;

    #[derive(Grammar)]
    pub struct TrainedModelName(pub Ident);

    #[derive(Grammar)]
    pub enum PredictWith {
        Unnamed(List),
        Named(VecSep<PredictAssign, ArgSep>),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct PredictAssign {
        pub ident: Ident,
        pub eq: (Space, StringEq!("="), Space),
        pub scalx: Scalx,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLProbabilityArgs {
        pub model: ModelName,
        pub use_clause: Option<(Space, PredictProbabilityUse)>,
        pub for_clause: Option<(Space, Prefix<(Kw!("FOR"), Space), Scalx>)>,
        pub with_clause: Option<((Space, Kw!("WITH"), Space), PredictWith)>,
    }

    pub type SQLJSONArrayArgs =
        Args<Option<(VecSep<Scalx, ArgSep>, Option<(Space, JSONNullClause)>)>>;

    pub type SQLJSONObjectArgs =
        Args<Option<(SQLJSONKeyValuePairs, Option<(Space, JSONNullClause)>)>>;

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLJSONKeyValuePairs(
        pub VecSep<(Scalx, (Space, StringEq!(":"), Space), Scalx), ArgSep>,
    );

    #[derive(Grammar)]
    pub struct JSONNullClause(
        pub Either<Kw!("NULL"), Kw!("ABSENT")>,
        pub Space1,
        pub Kw!("ON"),
        pub Space1,
        pub Kw!("NULL"),
    );

    #[derive(Grammar)]
    pub struct XMLElement {
        pub keyword: Kw!("XMLELEMENT"),
        pub args: Args<(SQLXmlElementName, Vec<(ArgSep, XMLValue)>)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    /// `NAME "x"`, `NAME` or `"x"`: 2026.2 rejects a plain identifier, `XMLELEMENT(a, b)` and `NAME foo`.
    pub enum SQLXmlElementName {
        Name(Kw!("NAME"), Option<(Space, DelimitedIdentifier)>),
        Delimited(DelimitedIdentifier),
    }

    #[derive(Grammar)]
    pub enum XMLValue {
        Concat(Box<XMLConcat>),
        Attributes(XMLAttributes),
        Expr(Scalx),
        Value2(XMLValue2),
    }

    #[derive(Grammar)]
    pub enum XMLValue2 {
        Element(Box<XMLElement>),
        Forest(Box<XMLForest>),
        Comment(XMLComment),
    }

    #[derive(Grammar)]
    pub struct XMLComment(pub (StringEq!("<--"), StringEndsWith<StringEq!("-->")>));

    #[derive(Grammar)]
    pub struct XMLAttributes {
        pub keyword: Kw!("XMLATTRIBUTES"),
        pub attrs: Args<VecSep<SQLXmlAttribute, ArgSep>>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLXmlAttribute {
        pub expr: Scalx,
        pub alias: Option<((Space, Kw!("AS"), Space), Ident)>,
    }

    #[derive(Grammar)]
    pub struct XMLForest {
        pub keyword: Kw!("XMLFOREST"),
        pub items: Args<VecSep<SQLXmlAttribute, ArgSep>>,
    }

    #[derive(Grammar)]
    pub struct XMLConcat {
        pub keyword: Kw!("XMLCONCAT"),
        pub values: Args<VecSep<XMLValue2, ArgSep>>,
    }

    #[derive(Grammar)]
    pub struct XTFun {
        pub fun: Prefix<StringEqCI!("$$"), StringOf1<IsExtFuncChar>>,
        pub args: Parenthesized<Option<VecSep<Scalx, ArgSep>>>,
    }

    /// `{fn CONVERT(x, SQL_TYPE)}` and `{fn IFNULL(x, y)}`, which IRIS reads otherwise without the escape (2026.2:
    /// CONVERT takes `SQL_TYPE` for a column, IFNULL(x, y) is NULL when x is not), and `{fn USER}`; `fn` is declared
    /// as ODBC spells it, so the core prints `{fn`.
    pub type ODBCOnly =
        Wrap<(StringEq!("{"), Space, Kw!("fn"), Space1), ODBCOnlyFunc, (Space, StringEq!("}"))>;

    #[derive(Grammar)]
    pub enum ODBCOnlyFunc {
        Convert(Kw!("CONVERT"), Args<RR<Scalx, SQLConvertFunc>>),
        IfNull(Kw!("IFNULL"), Args<RR>),
        User(Kw!("USER"), Option<Args<()>>),
    }

    pub type DQuote = StringEq!("\"");

    pub type SQuote = StringEq!("'");

    /// A keyword bare or in double quotes, as TIMESTAMPADD's interval (2026.2 rejects `'SQL_TSI_DAY'`).
    #[derive(Grammar)]
    pub enum DoubleQuotable<K: Grammar> {
        Bare(K),
        #[sugar]
        Quoted(Wrap<DQuote, K, DQuote>),
    }

    impl<K: Grammar + Desugar> Desugar for DoubleQuotableSugar<'_, K>
    where
        K::Core: Grammar,
    {
        type Core = core::DoubleQuotable<K::Core>;
        fn desugar(&self) -> Self::Core {
            let Self::Quoted(quoted) = self;
            core::DoubleQuotable::Bare(quoted.wrapped.desugar())
        }
    }

    /// A keyword bare or in single or double quotes, as a date part (2026.2 takes `'day'` and `"day"`,
    /// not `' day'` or any other literal).
    #[derive(Grammar)]
    pub enum Quotable<K: Grammar> {
        Bare(K),
        #[sugar]
        Single(Wrap<SQuote, K, SQuote>),
        #[sugar]
        Double(Wrap<DQuote, K, DQuote>),
    }

    impl<K: Grammar + Desugar> Desugar for QuotableSugar<'_, K>
    where
        K::Core: Grammar,
    {
        type Core = core::Quotable<K::Core>;
        fn desugar(&self) -> Self::Core {
            match self {
                Self::Single(quoted) => core::Quotable::Bare(quoted.wrapped.desugar()),
                Self::Double(quoted) => core::Quotable::Bare(quoted.wrapped.desugar()),
            }
        }
    }

    #[derive(Grammar)]
    pub enum DatePart {
        Keyword(Quotable<DatePartCommon>),
        HostVar(HostVar),
        #[sugar]
        Parenthesized(Parenthesized<Box<DatePart>>),
    }

    impl Desugar for DatePartSugar<'_> {
        type Core = core::DatePart;
        fn desugar(&self) -> Self::Core {
            let Self::Parenthesized(parens) = self;
            (*parens.wrapped).desugar()
        }
    }

    /// DATEPART's date part, which SQLTIMESTAMP joins.
    #[derive(Grammar)]
    pub enum DatePartPart {
        Keyword(Quotable<DatePartPartRaw>),
        HostVar(HostVar),
        #[sugar]
        Parenthesized(Parenthesized<Box<DatePartPart>>),
    }

    impl Desugar for DatePartPartSugar<'_> {
        type Core = core::DatePartPart;
        fn desugar(&self) -> Self::Core {
            let Self::Parenthesized(parens) = self;
            (*parens.wrapped).desugar()
        }
    }

    #[derive(Grammar)]
    pub enum DatePartPartRaw {
        SqlTimestamp(Spellings!(Kw!("SQLTIMESTAMP"), Kw!("STS"))),
        Common(DatePartCommon),
    }

    #[derive(Grammar)]
    pub enum DatePartCommon {
        Year(Spellings!(Kw!("YEAR"), Kw!("YYYY"), Kw!("YY"))),
        Quarter(Spellings!(Kw!("QUARTER"), Kw!("QQ"), Kw!("Q"))),
        Month(Spellings!(Kw!("MONTH"), Kw!("MM"), Kw!("M"))),
        Weekday(Spellings!(Kw!("WEEKDAY"), Kw!("DW"), Kw!("W"))),
        Week(Spellings!(Kw!("WEEK"), Kw!("WK"), Kw!("WW"))),
        DayOfYear(Spellings!(Kw!("DAYOFYEAR"), Kw!("DY"), Kw!("Y"))),
        Day(Spellings!(Kw!("DAY"), Kw!("DD"), Kw!("D"))),
        Hour(Spellings!(Kw!("HOUR"), Kw!("HH"))),
        Minute(Spellings!(Kw!("MINUTE"), Kw!("MI"), Kw!("N"))),
        Second(Spellings!(Kw!("SECOND"), Kw!("SS"), Kw!("S"))),
        Millisecond(Spellings!(Kw!("MILLISECOND"), Kw!("MS"))),
        Microsecond(Spellings!(Kw!("MICROSECOND"), Kw!("MCS"))),
        Nanosecond(Spellings!(Kw!("NANOSECOND"), Kw!("NS"))),
    }

    #[derive(Grammar)]
    pub enum TSAddInterval {
        Year(Kw!("SQL_TSI_YEAR")),
        Quarter(Kw!("SQL_TSI_QUARTER")),
        Month(Kw!("SQL_TSI_MONTH")),
        Week(Kw!("SQL_TSI_WEEK")),
        Day(Kw!("SQL_TSI_DAY")),
        Hour(Kw!("SQL_TSI_HOUR")),
        Minute(Kw!("SQL_TSI_MINUTE")),
        Second(Kw!("SQL_TSI_SECOND")),
        FracSecond(Kw!("SQL_TSI_FRAC_SECOND")),
    }

    #[derive(Grammar)]
    pub enum TSDiffInterval {
        Year(Kw!("SQL_TSI_YEAR")),
        Month(Kw!("SQL_TSI_MONTH")),
        Week(Kw!("SQL_TSI_WEEK")),
        Day(Kw!("SQL_TSI_DAY")),
        Hour(Kw!("SQL_TSI_HOUR")),
        Minute(Kw!("SQL_TSI_MINUTE")),
        Second(Kw!("SQL_TSI_SECOND")),
        FracSecond(Kw!("SQL_TSI_FRAC_SECOND")),
    }

    #[derive(Grammar)]
    pub enum SQLConvertFunc {
        SqlTimestamp(Kw!("SQL_TIMESTAMP")),
        SqlInteger(Kw!("SQL_INTEGER")),
        SqlBigInt(Kw!("SQL_BIGINT")),
        SqlSmallInt(Kw!("SQL_SMALLINT")),
        SqlTinyInt(Kw!("SQL_TINYINT")),
        SqlDate(Kw!("SQL_DATE")),
        SqlTime(Kw!("SQL_TIME")),
        SqlVarChar(Kw!("SQL_VARCHAR")),
        SqlDouble(Kw!("SQL_DOUBLE")),
        SqlPosixtime(Kw!("SQL_POSIXTIME")),
    }

    #[derive(Grammar)]
    pub enum DTSValSpec {
        Literal(Literal),
        HostVar(HostVar),
    }

    #[derive(Grammar)]
    pub enum Agg {
        AggFuncArgs { fun: AggFunc, arg: Args<AggArg> },
        CountStar(Kw!("COUNT"), Args<StringEq!("*")>),
    }

    #[derive(Grammar)]
    pub enum AggFunc {
        Avg(Kw!("AVG")),
        Count(Kw!("COUNT")),
        Sum(Kw!("SUM")),
        Max(Kw!("MAX")),
        Min(Kw!("MIN")),
        List(Kw!("LIST")),
        XmlAgg(Kw!("XMLAGG")),
        DList(Kw!("%DLIST")),
        StdDev(Kw!("STDDEV")),
        StdDevPop(Kw!("STDDEV_POP")),
        StdDevSamp(Kw!("STDDEV_SAMP")),
        Variance(Kw!("VARIANCE")),
        VarPop(Kw!("VAR_POP")),
        VarSamp(Kw!("VAR_SAMP")),
        JSONArrayAgg(Kw!("JSON_ARRAYAGG")),
        ApproxCountDistinct(Kw!("APPROX_COUNT_DISTINCT")),
    }

    #[derive(Grammar)]
    pub struct AggArg {
        #[sugar]
        pub all: Option<(AllKw, Space)>,
        pub distinct: Option<(Dist, Space)>,
        pub expr: Scalx,
        pub foreach: Option<(Space, Prefix<(Kw!("%FOREACH"), Space), List>)>,
        pub afterhaving: Option<(Space, Kw!("%AFTERHAVING"))>,
    }

    #[derive(Grammar)]
    pub struct Dist {
        pub keyword: Kw!("DISTINCT"),
        pub by: Option<(Space1, Prefix<(Kw!("BY"), Space), List>)>,
    }

    pub type List = Parenthesized<VecSep<Scalx, ArgSep>>;

    #[derive(Grammar)]
    pub struct Window {
        pub call: WindowCall,
        pub over_kw: (Space, Kw!("OVER"), Space),
        pub spec: Parenthesized<(
            Option<WindowPartition>,
            Option<(Space, Order)>,
            Option<(Space, Kw!("ROWS"), Space, WindowRows)>,
        )>,
    }

    /// Each window function with its arguments: 2026.2 rejects ROW_NUMBER(a), SUM(*), SUM(a, b), NTILE(),
    /// LEAD(a, 1, 0, 1) and FIRST_VALUE(a, 1), and COUNT takes * or an aggregate's argument.
    #[derive(Grammar)]
    pub enum WindowCall {
        RowNumber(Kw!("ROW_NUMBER"), Args<()>),
        Rank(Kw!("RANK"), Args<()>),
        DenseRank(Kw!("DENSE_RANK"), Args<()>),
        PercentRank(Kw!("PERCENT_RANK"), Args<()>),
        CumeDist(Kw!("CUME_DIST"), Args<()>),
        NTile(Kw!("NTILE"), Args<Scalx>),
        FirstValue(Kw!("FIRST_VALUE"), Args<Scalx>),
        LastValue(Kw!("LAST_VALUE"), Args<Scalx>),
        NthValue(Kw!("NTH_VALUE"), Args<RO>),
        Lead(Kw!("LEAD"), Args<ROO>),
        Lag(Kw!("LAG"), Args<ROO>),
        Sum(Kw!("SUM"), Args<Scalx>),
        Avg(Kw!("AVG"), Args<Scalx>),
        Count(Kw!("COUNT"), Args<Either<StringEq!("*"), AggArg>>),
        Min(Kw!("MIN"), Args<Scalx>),
        Max(Kw!("MAX"), Args<Scalx>),
    }

    pub type WindowPartition =
        Prefix<(Kw!("PARTITION"), Space1, Kw!("BY"), Space), VecSep<Scalx, ArgSep>>;

    #[derive(Grammar)]
    pub enum WindowRows {
        #[sugar]
        Single(WindowRowsBound),
        Between {
            between_kw: Kw!("BETWEEN"),
            start: (Space, WindowRowsBound),
            end: (Space1, AndClause<WindowRowsBound>),
        },
    }

    /// `ROWS n PRECEDING` is `ROWS BETWEEN n PRECEDING AND CURRENT ROW`, the standard's lone-bound reading.
    impl Desugar for WindowRowsSugar<'_> {
        type Core = core::WindowRows;
        fn desugar(&self) -> Self::Core {
            let Self::Single(bound) = self;
            let current_row = core::WindowRowsBound::CurrentRow(
                Default::default(),
                Default::default(),
                Default::default(),
            );
            core::WindowRows::Between {
                between_kw: Default::default(),
                start: (Space::blank(), bound.desugar()),
                end: (
                    Default::default(),
                    Prefix::new((Default::default(), Space::blank()), current_row),
                ),
            }
        }
    }

    #[derive(Grammar)]
    pub enum WindowRowsBound {
        UnboundedPreceding(Kw!("UNBOUNDED"), Space1, Kw!("PRECEDING")),
        UnboundedFollowing(Kw!("UNBOUNDED"), Space1, Kw!("FOLLOWING")),
        Preceding(
            Either<NonNegativeInteger, StringLiteral>,
            Space,
            Kw!("PRECEDING"),
        ),
        Following(
            Either<NonNegativeInteger, StringLiteral>,
            Space,
            Kw!("FOLLOWING"),
        ),
        CurrentRow(Kw!("CURRENT"), Space1, Spellings!(Kw!("ROW"), Kw!("ROWS"))),
    }

    #[derive(Grammar)]
    pub struct Prdct(pub VecSep<And, (Space, Spellings!(Kw!("OR"), StringEq!("!")), Space)>);

    #[derive(Grammar)]
    pub struct And(pub VecSep<Src<Cond>, (Space, Spellings!(Kw!("AND"), StringEq!("&")), Space)>);

    pub type NoIndex = Option<(Kw!("%NOINDEX"), Space)>;

    /// `NOT` before a predicate's keyword, which the condition's impl puts around the predicate in the core.
    pub type Not = Option<(Kw!("NOT"), Space1)>;

    /// A keyword or clause read the same when omitted: spelled, with the blank before or after it
    /// that a synthesized one must bring, or omitted and read as its default.
    #[derive(Grammar)]
    #[tygr(inline)]
    #[sugar]
    pub enum Implied<
        K: Grammar + Default,
        Before: Grammar + Default = Space1,
        After: Grammar + Default = (),
    > {
        Spelled(Before, K, After),
        #[sugar]
        Omitted(()),
    }

    impl<K, Before, After> Desugar for Implied<K, Before, After>
    where
        K: Grammar + Default + Desugar,
        K::Core: Grammar + Default,
        Before: Grammar + Default + Desugar,
        Before::Core: Grammar + Default,
        After: Grammar + Default + Desugar,
        After::Core: Grammar + Default,
    {
        type Core = core::Implied<K::Core, Before::Core, After::Core>;
        fn desugar(&self) -> Self::Core {
            match self {
                Implied::Spelled(before, keyword, after) => {
                    core::Implied::Spelled(before.desugar(), keyword.desugar(), after.desugar())
                }
                Implied::Omitted(_) => core::Implied::Spelled(
                    Default::default(),
                    Default::default(),
                    Default::default(),
                ),
            }
        }
    }

    /// An operator and a quantifier against a subquery; `IN` is `= ANY`, a `NOT IN` under the condition's NOT.
    #[derive(Grammar)]
    pub enum QuantifiedOp {
        Op {
            op: CmpOp,
            sp: Space,
            quantifier: Quantifier,
        },
        #[sugar]
        In { not: Not, in_kw: Kw!("IN") },
    }

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
    #[sugar]
    pub enum Cond {
        Not {
            not_kw: (Kw!("NOT"), Space),
            condition: Box<Src<Cond>>,
        },
        Paren(Parenthesized<Box<Prdct>>),
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
                Cond::Paren(predicate) => return core::Cond::Paren(predicate.desugar()),
                Cond::Predicate { noindex, predicate } => (noindex, predicate),
            };
            let plain = core::Cond::Predicate {
                noindex: noindex.desugar(),
                predicate: predicate.desugar(),
            };
            if !negation(predicate) {
                return plain;
            }
            core::Cond::Not {
                not_kw: (Default::default(), Space::blank()),
                condition: Box::new(Src::synthesized(core::Cond::Paren(parenthesized(
                    Box::new(core::Prdct(VecSep::one(core::And(VecSep::one(
                        Src::synthesized(plain),
                    ))))),
                )))),
            }
        }
    }

    fn negation(predicate: &Predicate) -> bool {
        use Comparison::*;
        let Predicate::Comparison { tail, .. } = predicate else {
            return false;
        };
        match tail {
            IsNull { not, .. }
            | Between { not, .. }
            | Like { not, .. }
            | Matches { not, .. }
            | In { not, .. }
            | InList { not, .. }
            | IsJSON { not, .. }
            | Query {
                op: QuantifiedOp::In { not, .. },
                ..
            } => not.is_some(),
            _ => false,
        }
    }

    /// What a `%NOINDEX` hint may precede (measured on 2026.2: not a `NOT` or a parenthesis).
    #[derive(Grammar)]
    pub enum Predicate {
        Exists {
            exists_kw: (Kw!("EXISTS"), Space),
            query: SQLSubquery,
        },
        ForSomeElement {
            kw: (
                Kw!("FOR"),
                Space1,
                Kw!("SOME"),
                Space1,
                Kw!("%ELEMENT"),
                Space,
            ),
            column: Parenthesized<Colmn>,
            label: Option<Alias>,
            predicate: (Space, Parenthesized<Box<Prdct>>),
        },
        ForSome {
            kw: (Kw!("FOR"), Space1, Kw!("SOME"), Space),
            tables: Parenthesized<Tables>,
            condition: Option<(Space, Parenthesized<Box<ForCnd>>)>,
        },
        Comparison {
            lhs: Scalx,
            sp: Space,
            tail: Comparison,
        },
    }

    #[derive(Grammar)]
    pub enum ForCnd {
        Prdct(Prdct),
        GroupHaving(Option<(Group, Space)>, Having),
    }

    pub type AndClause<T> = Prefix<(Kw!("AND"), Space), T>;

    #[doc(hidden)]
    #[derive(Grammar)]
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
            is_kw: (Kw!("IS"), Space1),
            #[sugar]
            not: Not,
            null_kw: Kw!("NULL"),
        },
        Between {
            #[sugar]
            not: Not,
            between_kw: (Kw!("BETWEEN"), Space),
            lower: Scalx,
            and_kw: (Space, Kw!("AND"), Space),
            upper: Scalx,
        },
        Like {
            #[sugar]
            not: Not,
            like_kw: (Kw!("LIKE"), Space),
            pattern: Scalx,
            escape: Option<(Space, EscapeScalx)>,
        },
        Matches {
            #[sugar]
            not: Not,
            matches_kw: (Kw!("%MATCHES"), Space),
            pattern: Scalx,
            escape: Option<(Space, EscapeScalx)>,
        },
        Query {
            op: QuantifiedOp,
            sp: Space,
            expr: SQLSubquery,
        },
        In {
            #[sugar]
            not: Not,
            in_kw: (Kw!("IN"), Space),
            list: List,
        },
        InList {
            #[sugar]
            not: Not,
            inlist_kw: (Kw!("%INLIST"), Space),
            list_expr: Scalx,
            size_hint: Option<(Space, SizeHint)>,
        },
        Find {
            find_kw: (Kw!("%FIND"), Space),
            expr: Scalx,
            size_hint: Option<(Space, SizeHint)>,
        },
        Pattern {
            pattern_kw: (Kw!("%PATTERN"), Space),
            pattern: Scalx,
        },
        IsJSON {
            format: Option<(Kw!("FORMAT"), Space1, JSONRepresentation, Space1)>,
            is_kw: (Kw!("IS"), Space1),
            #[sugar]
            not: Not,
            json_kw: Kw!("JSON"),
            constraint: Option<(Space1, JSONPredicateTypeConstraint)>,
        },
        Contains {
            kw: Either<Kw!("%CONTAINSTERM"), Kw!("%CONTAINS")>,
            list: Args1Plus,
        },
        Scalx {
            op: CmpOp,
            space: Space,
            expr: Scalx,
        },
    }

    pub type EscapeScalx = Prefix<(Kw!("ESCAPE"), Space), Scalx>;

    pub type SQLSubquery = Parenthesized<QueryExpr>;

    #[derive(Grammar)]
    pub enum Quantifier {
        All(Kw!("ALL")),
        Any(Spellings!(Kw!("ANY"), Kw!("SOME"))),
    }

    #[derive(Grammar)]
    pub struct SizeHint {
        pub kw: (Kw!("SIZE"), Space),
        /// Doubled, as IRIS requires: 2026.2 rejects `SIZE (n)` and `SIZE n`, takes deeper nesting and blanks.
        pub size: Parenthesized<Parenthesized<PConst>>,
    }

    #[derive(Grammar)]
    pub enum PConst {
        #[sugar]
        Nested(Parenthesized<Box<PConst>>),
        Literal(Literal),
    }

    impl Desugar for PConstSugar<'_> {
        type Core = core::PConst;
        fn desugar(&self) -> Self::Core {
            let Self::Nested(inner) = self;
            (*inner.wrapped).desugar()
        }
    }

    #[derive(Grammar)]
    pub struct JSONRepresentation(pub Kw!("JSON"));

    #[derive(Grammar)]
    pub enum JSONPredicateTypeConstraint {
        Value(Kw!("VALUE")),
        Array(Kw!("ARRAY")),
        Object(Kw!("OBJECT")),
        Scalar(Kw!("SCALAR")),
    }

    #[derive(Grammar)]
    pub struct WithList(pub VecSep<Src<WithListElement>, ArgSep>);

    pub type WithClause = Prefix<(Kw!("WITH"), Space), WithList>;

    #[derive(Grammar)]
    pub struct WithListElement {
        pub name: Ident,
        pub columns: Option<(Space, FList1)>,
        pub as_kw: (Space, Kw!("AS"), Space),
        pub query: SQLSubquery,
    }

    #[derive(Grammar)]
    pub struct ColumnName(pub Ident);

    #[derive(Grammar)]
    pub struct Selex {
        pub kw: (Kw!("SELECT"), Space),
        pub options: Vec<(SelectOption, Space)>,
        #[sugar]
        pub all: Option<(AllKw, Space)>,
        pub selst: Selst,
        pub from: Option<(Space, FromClause)>,
        pub where_clause: Option<(Space, WhereClause)>,
        pub group: Option<(Space, Group)>,
        pub having: Option<(Space, Having)>,
        pub tail: Option<(Space, Either<Order, RowLimitingClause>)>,
    }

    /// ORDER BY and a row limit after the query, as a SELECT takes one of the two (ORDER BY with TOP alone, see
    /// QueryExpr): 2026.2 reads LIMIT 1 ORDER BY a LIMIT 2 and LIMIT 1 LIMIT 2, the first limit the SELECT's and
    /// the second the whole's, not ORDER BY a LIMIT 1 LIMIT 2, LIMIT 1 LIMIT 2 ORDER BY a or LIMIT 1 ORDER BY a UNION.
    #[derive(Grammar)]
    pub struct Tail {
        pub order: Option<(Space, Order)>,
        pub limit: Option<(Space, RowLimitingClause)>,
    }

    impl Query {
        fn ordered_with_top(terms: &[QTerm]) -> bool {
            terms.iter().all(|term| match term {
                QTerm::Selex(selex) => {
                    !matches!(selex.node.tail, Some((_, Either::Left(_))))
                        || selex.node.selst.top.is_some()
                }
                QTerm::Query(_) => true,
            })
        }

        /// Whether the last operand's ORDER BY is the enclosing statement's: a SELECT with TOP orders its own rows
        /// (2026.2 takes a second ORDER BY after SELECT TOP 1 a FROM t ORDER BY a DESC, and after a UNION's last
        /// operand with TOP, whose ORDER BY sees that operand's aliases alone), one without TOP the whole's.
        pub(super) fn ends_with_statement_order(&self) -> bool {
            matches!(self.0.items.last(), Some(QTerm::Selex(selex))
                if selex.node.selst.top.is_none() && matches!(selex.node.tail, Some((_, Either::Left(_)))))
        }

        /// Whether the operands order their rows as 2026.2 allows before a statement's own ORDER BY (`ordered`):
        /// each but the last only with TOP (ORDER BY a UNION is refused, SQLCODE -25), the last also without, as the
        /// statement's, which then orders no second time (ORDER BY a DESC ORDER BY a is refused, SQLCODE -25).
        fn orders_once(&self, ordered: bool) -> bool {
            let (_, rest) = self.0.items.split_last().expect("one term");
            Self::ordered_with_top(rest) && !(self.ends_with_statement_order() && ordered)
        }
    }

    fn take_statement_order(query: &mut core::Query) -> Option<(Space, core::Order)> {
        let is_statement = query.ends_with_statement_order();
        let Some(core::QTerm::Selex(last)) = query.0.items.last_mut() else {
            return None;
        };
        let Some((space, Either::Left(order))) = last.node.tail.take_if(|_| is_statement) else {
            return None;
        };
        Some((space, order))
    }

    /// The query an INSERT or a CREATE TABLE AS reads: 2026.2 takes a row limit in its operands and ORDER BY once,
    /// last (LIMIT 1 ORDER BY a, not ORDER BY a LIMIT 1 or LIMIT 1 LIMIT 2, SQLCODE -25).
    #[derive(Grammar)]
    #[tygr(validated)]
    #[sugar]
    pub struct SourceQuery {
        pub query: Query,
        pub order: Option<(Space, Order)>,
    }

    impl Validate for SourceQuery {
        const REQUIREMENT: &'static str = "order an operand only with TOP and its rows once";
        fn validate(&self) -> bool {
            self.query.orders_once(self.order.is_some())
        }
    }

    impl Desugar for SourceQuery {
        type Core = core::SourceQuery;
        fn desugar(&self) -> Self::Core {
            let mut query = self.query.desugar();
            let order = take_statement_order(&mut query).or_else(|| self.order.desugar());
            core::SourceQuery { query, order }
        }
    }

    /// A query inside a statement, as a subquery, a CTE body or a view: 2026.2 takes a row limit in every operand
    /// and ORDER BY only with TOP (SQLCODE -143 otherwise).
    #[derive(Grammar)]
    #[tygr(validated)]
    pub struct QueryExpr(pub Box<Query>);

    impl Validate for QueryExpr {
        const REQUIREMENT: &'static str = "order an operand only with TOP";
        fn validate(&self) -> bool {
            Query::ordered_with_top(&self.0.0.items)
        }
    }

    #[derive(Grammar)]
    pub enum SelectOption {
        NoFPlan(Kw!("%NOFPLAN")),
        NoLock(Kw!("%NOLOCK")),
        Profile(Kw!("%PROFILE")),
        ProfileAll(Kw!("%PROFILE_ALL")),
        NoRuntime(Kw!("%NORUNTIME")),
        DoRuntime(Kw!("%DORUNTIME")),
        RowLimit(Kw!("%ROWLIMIT"), Space, Count),
        RowOffset(Kw!("%ROWOFFSET"), Space, Count),
    }

    #[derive(Grammar)]
    pub enum TopSpec {
        All(Kw!("ALL")),
        Count(Count),
    }

    /// A count: a literal, a host variable or `?`, in any parentheses; 2026.2 takes `TOP (1)`, `TOP(1)`,
    /// `LIMIT ((10))`, `OFFSET (:n)`, `FETCH NEXT 'x' ROWS ONLY`, `DIAGNOSTICS SIZE (1)` and `%ROWLIMIT ((5))`, not
    /// `LIMIT 1 + 1`, `LIMIT a`, `LIMIT -1`, `%ROWLIMIT (1, 2)` or `TOP (ALL)`.
    #[derive(Grammar)]
    pub enum Count {
        Paren(Parenthesized<Box<Count>>),
        Literal(Literal),
        HostVar(HostVar),
        Marker(ParameterMarker),
    }

    #[derive(Grammar)]
    pub struct Selst {
        pub distinct: Option<(Dist, Space)>,
        pub top: Option<(Kw!("TOP"), Space, TopSpec, Space)>,
        pub select_list: SelectList,
        pub targt: Option<(Space, Targt)>,
    }

    #[derive(Grammar)]
    pub struct SelectList(pub VecSep<Src<SelectItem>, ArgSep>);

    #[derive(Grammar)]
    pub enum SelectItem {
        Star(StringEq!("*")),
        Expr { expr: Scalx, alias: Option<Alias> },
    }

    // The blank stays inside the alias, so that a reference without one consumes nothing after it.
    #[derive(Grammar)]
    pub struct Alias {
        pub as_kw: Implied<Kw!("AS")>,
        pub sp: Space,
        pub name: SQLIdentNonKeyword,
    }

    pub type Into = Prefix<(Kw!("INTO"), Space), HostList>;

    #[derive(Grammar)]
    pub struct Targt(pub TargtEnum);

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum TargtEnum {
        Into(Into),
        IntoList(IntoList),
    }

    #[derive(Grammar)]
    pub struct HostList(pub VecSep<Param, ArgSep>);

    pub type IntoList = Prefix<Kw!("%INTOLIST"), (Space, Param)>;

    #[derive(Grammar)]
    pub struct FromClause {
        pub keyword: (Kw!("FROM"), Space),
        pub options: Vec<(FromOpt, Space)>,
        pub tables: Tables,
    }

    #[derive(Grammar)]
    pub enum FromOpt {
        InOrder(Kw!("%INORDER")),
        Full(Kw!("%FULL")),
        DbugFull(Kw!("%DBUGFULL")),
        Flatten(Kw!("%FLATTEN")),
        NoBSL(Kw!("%NOBSL")),
        NoFixedStats(Kw!("%NOFIXEDSTATS"), Space, TableRefs),
        NoFlatten(Kw!("%NOFLATTEN")),
        NoMerge(Kw!("%NOMERGE")),
        NoReduce(Kw!("%NOREDUCE")),
        NoSVSO(Kw!("%NOSVSO")),
        NoTopOpt(Kw!("%NOTOPOPT")),
        IgnoreIndices(Kw!("%IGNOREINDICES"), Space, IndexRefs),
        IgnoreIndex(Kw!("%IGNOREINDEX"), Space, IndexRefs),
        StartTable(Kw!("%STARTTABLE")),
        AllIndex(Kw!("%ALLINDEX")),
        NoUnionOrOpt(Kw!("%NOUNIONOROPT")),
        FirstTable(Kw!("%FIRSTTABLE"), Space, Ident),
        Parallel(Kw!("%PARALLEL")),
        NoParallel(Kw!("%NOPARALLEL")),
    }

    #[derive(Grammar)]
    pub struct TableRefs(pub VecSep<TableRef, ArgSep>);

    /// `[schema.]table` or `[schema.]*`; 2026.2 takes blanks around the dot and refuses a third part.
    #[derive(Grammar)]
    pub struct TableRef {
        pub schema: Option<(Schema, Space, StringEq!("."), Space)>,
        pub table: Either<StringEq!("*"), Ident>,
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
                let core::Tref::Join(mut table) = tref.desugar();
                let start = table.formit.range().start;
                let end = (table.joins.last())
                    .map_or(table.formit.range().end, |(_, join)| join.range().end);
                if !table.joins.is_empty() {
                    table = core::JTab {
                        formit: Src::new(
                            start..end,
                            core::Frmit::JTab(parenthesized(Box::new(table))),
                        ),
                        joins: Vec::new(),
                    };
                }
                let cross = core::Join::Cross {
                    cross_kw: Default::default(),
                    table: (Space::blank(), Box::new(table)),
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

    #[derive(Grammar)]
    pub enum Tref {
        Join(JTab),
        #[sugar]
        OJSyntax {
            open: (StringEq!("{"), Space),
            oj: (Kw!("OJ"), Space1),
            table: JTab,
            close: (Space, StringEq!("}")),
        },
    }

    impl Desugar for TrefSugar<'_> {
        type Core = core::Tref;
        fn desugar(&self) -> Self::Core {
            let Self::OJSyntax { table, .. } = self;
            core::Tref::Join(table.desugar())
        }
    }

    #[derive(Grammar)]
    pub struct JTab {
        pub formit: Src<Frmit>,
        pub joins: Vec<(Space, Src<Join>)>,
    }

    #[derive(Grammar)]
    pub enum Join {
        Cross {
            cross_kw: (Kw!("CROSS"), Space1, Kw!("JOIN")),
            table: (Space, Box<JTab>),
        },
        Natural {
            natural_kw: (Kw!("NATURAL"), Space1),
            jtype: Implied<JType, (), Space1>,
            join_kw: Kw!("JOIN"),
            space: Space,
            table: Box<JTab>,
        },
        Qualified {
            jtype: Implied<JType, (), Space1>,
            join_kw: Kw!("JOIN"),
            space: Space,
            table: Box<JTab>,
            spec: (Space, JSpec),
        },
    }

    #[derive(Grammar)]
    pub enum JType {
        Inner(Kw!("INNER")),
        Outer(Side, Implied<Kw!("OUTER")>),
    }

    impl Default for JType {
        fn default() -> Self {
            JType::Inner(Default::default())
        }
    }

    #[derive(Grammar)]
    pub enum Side {
        Left(Kw!("LEFT")),
        Right(Kw!("RIGHT")),
        Full(Kw!("FULL")),
    }

    #[derive(Grammar)]
    pub enum JSpec {
        On {
            on_kw: (Kw!("ON"), Space),
            condition: Prdct,
        },
        Using {
            using_kw: (Kw!("USING"), Space),
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
            name: TName,
            alias: Option<Alias>,
        },
        Query {
            lateral: Option<(Kw!("LATERAL"), Space)>,
            query: SQLSubquery,
            alias: Option<Alias>,
        },
        JTab(Parenthesized<Box<JTab>>),
    }

    #[derive(Grammar)]
    pub struct JSONTable {
        pub kw: Kw!("JSON_TABLE"),
        pub args: Args<(JSONApiCommonSyntax, Option<(Space, JSONTableColumnsSpec)>)>,
    }

    #[derive(Grammar)]
    pub struct JSONApiCommonSyntax {
        pub context_item: Scalx,
        pub path: Option<(ArgSep, StringLiteral)>,
    }

    #[derive(Grammar)]
    pub enum JSONTableColumnsSpec {
        Columns(JSONTableColumnsClause),
        Type(JSONTableTypeClause),
    }

    #[derive(Grammar)]
    pub struct JSONTableColumnsClause {
        pub kw: Kw!("COLUMNS"),
        pub columns: Args<VecSep<JSONTableRegularColumnDefinition, ArgSep>>,
    }

    #[derive(Grammar)]
    pub struct JSONTableRegularColumnDefinition {
        pub name: Ident,
        pub sp1: Space,
        pub data_type: Datyp,
        pub path_clause: Option<(Space, Kw!("PATH"), Space, StringLiteral)>,
    }

    #[derive(Grammar)]
    pub struct JSONTableTypeClause {
        pub kw: Kw!("%TYPE"),
        pub sp1: Space,
        pub type_name: StringLiteral,
    }

    #[derive(Grammar)]
    pub struct TVF {
        pub table: TName,
        pub args: Parenthesized<Option<(VecSep<Scalx, ArgSep>, Option<(Space, StringEq!(","))>)>>,
    }

    pub type WhereClause = Prefix<(Kw!("WHERE"), Space), Prdct>;

    pub type Group = Prefix<(Kw!("GROUP"), Space1, Kw!("BY"), Space), VecSep<Scalx, ArgSep>>;

    pub type Having = Prefix<(Kw!("HAVING"), Space), Prdct>;

    pub type Order = Prefix<(Kw!("ORDER"), Space1, Kw!("BY"), Space), VecSep<Src<Ordir>, ArgSep>>;

    #[derive(Grammar)]
    pub struct Ordir {
        pub expr: Ordit,
        #[sugar]
        pub asc: Option<(Space, Kw!("ASC"), NotFollowedBy<(Space, Kw!("DESC"))>)>,
        pub desc: Option<(Space, Kw!("DESC"))>,
    }

    #[derive(Grammar)]
    pub enum Ordit {
        Literal(Literal),
        Scalx(Scalx),
    }

    #[derive(Grammar)]
    pub enum RowLimitingClause {
        LimitOffset {
            limit: Prefix<(Kw!("LIMIT"), Space), LimitSpec>,
            offset: Option<(Space, Offset)>,
        },
        #[sugar]
        OffsetFetch {
            offset: Offset,
            fetch: Option<(Space, Fetch)>,
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
                        || core::LimitSpec::All(Default::default()),
                        |(_, fetch)| count(fetch),
                    ),
                    Some((
                        Space::blank(),
                        core::Offset {
                            kw: (Default::default(), Space::blank()),
                            count: offset.count.desugar(),
                        },
                    )),
                ),
                Self::FetchOnly(fetch) => (count(fetch), None),
            };
            core::RowLimitingClause::LimitOffset {
                limit: Prefix::new((Default::default(), Space::blank()), limit),
                offset,
            }
        }
    }

    #[derive(Grammar)]
    pub enum LimitSpec {
        All(Kw!("ALL")),
        Val(Count),
    }

    #[derive(Grammar)]
    pub struct Offset {
        pub kw: (Kw!("OFFSET"), Space),
        pub count: Count,
        #[sugar]
        pub rows: Option<(Space, RowRowsInline)>,
    }

    #[derive(Grammar)]
    pub struct Query(pub VecSep<QTerm, (Space, QueryOp, Space)>);

    #[derive(Grammar)]
    pub enum QueryOp {
        Union {
            union_kw: Kw!("UNION"),
            all: Option<(Space1, Kw!("ALL"))>,
            parallel: Option<(Space1, Kw!("%PARALLEL"))>,
        },
        Intersect(Kw!("INTERSECT")),
        Except(Kw!("EXCEPT")),
    }

    #[derive(Grammar)]
    pub enum QTerm {
        Query(SQLSubquery),
        Selex(Src<Selex>),
    }
    #[derive(Grammar)]
    #[tygr(validated)]
    pub struct Insrt {
        pub insert_kw: Kw!("INSERT"),
        pub or_update: Option<(Space1, Kw!("OR"), Space1, Kw!("UPDATE"))>,
        pub options: Vec<(Space1, DmlOption)>,
        pub into_kw: Implied<Kw!("INTO")>,
        pub target: (Space, IUDTarget),
        pub values: (Space, InsertValues),
    }

    impl Validate for Insrt {
        const REQUIREMENT: &'static str = "not take %ILOCK";
        fn validate(&self) -> bool {
            !self
                .options
                .iter()
                .any(|(_, option)| matches!(option, DmlOption::ILock(_)))
        }
    }

    /// One option list for INSERT, UPDATE, DELETE and TRUNCATE TABLE, each validated against its own set.
    #[derive(Grammar)]
    pub enum DmlOption {
        NoCheck(Kw!("%NOCHECK")),
        NoFPlan(Kw!("%NOFPLAN")),
        NoLock(Kw!("%NOLOCK")),
        NoIndex(Kw!("%NOINDEX")),
        NoTrigger(Kw!("%NOTRIGGER")),
        NoStream(Kw!("%NOSTREAM")),
        NoJourn(Kw!("%NOJOURN")),
        ILock(Kw!("%ILOCK")),
        ObjectFiler(Kw!("%OBJECTFILER")),
        ProfileAll(Kw!("%PROFILE_ALL")),
        Profile(Kw!("%PROFILE")),
        NoRuntime(Kw!("%NORUNTIME")),
        DoRuntime(Kw!("%DORUNTIME")),
    }

    #[derive(Grammar)]
    pub struct IUDTarget {
        pub target: TableOrSubquery,
        pub alias: Option<Alias>,
    }

    #[derive(Grammar)]
    pub enum TableOrSubquery {
        Table(TName),
        Subquery(SQLSubquery),
    }

    /// An INSERT's values, or DEFAULT VALUES after an optional column list (2026.2 takes both).
    #[derive(Grammar)]
    pub enum InsertValues {
        Asign(Asign),
        DefaultValues {
            columns: Option<SQLColumnListParen>,
            kw: (Space, Kw!("DEFAULT"), Space1, Kw!("VALUES")),
        },
    }

    /// The values of an INSERT or UPDATE: a column list with VALUES, a query or a host array, or SET assignments,
    /// which read as the column list with VALUES.
    #[derive(Grammar)]
    pub enum Asign {
        #[sugar]
        SetList {
            set_kw: Kw!("SET"),
            assignments: (Space, VecSep<Src<SQLSetAssignment>, ArgSep>),
        },
        ColumnList {
            columns: Option<SQLColumnListParen>,
            values: (Space, ValueSource),
        },
    }

    impl Desugar for AsignSugar<'_> {
        type Core = core::Asign;
        fn desugar(&self) -> Self::Core {
            let Self::SetList {
                assignments: (_, list),
                ..
            } = self;
            let columns = list.items.iter().map(|a| a.column.desugar()).collect();
            let values = list.items.iter().map(|a| a.value.desugar()).collect();
            core::Asign::ColumnList {
                columns: Some((Space::default(), parenthesized(commas(columns)))),
                values: (
                    Space::blank(),
                    core::ValueSource::ValSpec {
                        values_kw: Default::default(),
                        sp: Space::blank(),
                        spec: core::ValSpec::Scalx(parenthesized(commas(values))),
                    },
                ),
            }
        }
    }

    pub type FList1 = Parenthesized<VecSep<Ident, ArgSep>>;

    pub type SQLColumnListParen = Args<VecSep<Colmn, ArgSep>>;

    #[derive(Grammar)]
    pub enum ValueSource {
        Query(SourceQuery),
        ValSpec {
            values_kw: Kw!("VALUES"),
            sp: Space,
            spec: ValSpec,
        },
    }

    #[derive(Grammar)]
    pub enum ValSpec {
        Scalx(List),
        Var { var: Var, subs: SubList },
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SQLSetAssignment {
        pub column: Colmn,
        pub eq: (Space, StringEq!("="), Space),
        pub value: Scalx,
    }

    #[derive(Grammar)]
    #[tygr(validated)]
    pub struct Updat {
        pub update_kw: Kw!("UPDATE"),
        pub body: (Space, UpdatBody),
    }

    impl Validate for Updat {
        const REQUIREMENT: &'static str = "take VALUES, not a query";
        fn validate(&self) -> bool {
            !matches!(
                &self.body.1,
                UpdatBody::Update {
                    assignment: (
                        _,
                        Asign::ColumnList {
                            values: (_, ValueSource::Query(_)),
                            ..
                        }
                    ),
                    ..
                }
            )
        }
    }

    #[derive(Grammar)]
    pub enum UpdatBody {
        UpdateStats {
            stats_kw: (Kw!("STATISTICS"), Space),
            table: TName,
        },
        Update {
            options: Vec<(DmlOption, Space)>,
            target: IUDTarget,
            assignment: (Space, Asign),
            from: Option<(Space, FromClause)>,
            where_clause: Option<(Space, WhereWerop)>,
        },
    }

    pub type WhereWerop = Prefix<(Kw!("WHERE"), Space), Werop>;

    #[derive(Grammar)]
    pub enum Werop {
        Prdct(Prdct),
        CurrentOf {
            current_of: (Kw!("CURRENT"), Space1, Kw!("OF"), Space),
            ident: Ident,
            prdct: Option<(Space, AndClause<Prdct>)>,
        },
    }

    #[derive(Grammar)]
    #[tygr(validated)]
    pub struct Delet {
        pub delete_kw: Kw!("DELETE"),
        pub options: Vec<(Space1, DmlOption)>,
        pub from_kw: Implied<Kw!("FROM")>,
        pub target: (Space, IUDTarget),
        pub from: Option<(Space, FromClause)>,
        pub where_clause: Option<(Space, WhereWerop)>,
    }

    impl Validate for Delet {
        const REQUIREMENT: &'static str = "not take %NOSTREAM or %ILOCK";
        fn validate(&self) -> bool {
            !self
                .options
                .iter()
                .any(|(_, option)| matches!(option, DmlOption::NoStream(_) | DmlOption::ILock(_)))
        }
    }

    #[derive(Grammar)]
    #[tygr(validated)]
    pub struct Truncate {
        pub kw: (Kw!("TRUNCATE"), Space1, Kw!("TABLE")),
        pub options: Vec<(Space1, DmlOption)>,
        pub table: (Space, TName),
        pub alias: Option<Alias>,
    }

    impl Validate for Truncate {
        const REQUIREMENT: &'static str = "take only %NOCHECK, %NOLOCK and %NOJOURN, each once";
        fn validate(&self) -> bool {
            let names: Vec<String> = (self.options.iter())
                .map(|(_, option)| option.print().to_uppercase())
                .collect();
            self.options.iter().all(|(_, option)| {
                matches!(
                    option,
                    DmlOption::NoCheck(_) | DmlOption::NoLock(_) | DmlOption::NoJourn(_)
                )
            }) && names
                .iter()
                .enumerate()
                .all(|(i, name)| !names[..i].contains(name))
        }
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum TpRelated {
        StartTransaction(StartTP),
        SetTransaction(SetTP),
        Rollback(RollB),
        TPOption(TPOption),
    }

    #[derive(Grammar)]
    pub struct StartTP {
        pub kw: (Kw!("START"), Space1, Kw!("TRANSACTION"), Space),
        pub commit_mode: Option<(CommitModeSpec, Option<(Space, StringEq!(","))>, Space)>,
        pub modes: VecSep<Option<TRMode>, ArgSep>,
    }

    #[derive(Grammar)]
    pub struct SetTP {
        pub start_kw: Option<(Kw!("START"), Space1)>,
        pub kw: (Kw!("SET"), Space1, Kw!("TRANSACTION")),
        pub commit_mode: Option<(Space1, CommitModeSpec)>,
        pub modes: VecSep<Option<(Space, TRMode)>, (Space, StringEq!(","))>,
    }

    pub type CommitModeSpec = Prefix<Kw!("%COMMITMODE"), (Space1, CommitMode)>;

    #[derive(Grammar)]
    pub enum CommitMode {
        Implicit(Kw!("IMPLICIT")),
        Explicit(Kw!("EXPLICIT")),
        None(Kw!("NONE")),
    }

    #[derive(Grammar)]
    pub enum TRMode {
        IsolationLevel {
            kw: (Kw!("ISOLATION"), Space1, Kw!("LEVEL"), Space1),
            level: GetLevel,
        },
        ReadOnly(Kw!("READ"), Space1, Kw!("ONLY")),
        ReadWrite(Kw!("READ"), Space1, Kw!("WRITE")),
        DiagnosticsSize {
            kw: (Kw!("DIAGNOSTICS"), Space1, Kw!("SIZE"), Space),
            size: Count,
        },
    }

    #[derive(Grammar)]
    pub enum GetLevel {
        ReadUncommitted(Kw!("READ"), Space1, Kw!("UNCOMMITTED")),
        ReadCommitted(Kw!("READ"), Space1, Kw!("COMMITTED")),
        ReadVerified(Kw!("READ"), Space1, Kw!("VERIFIED")),
        RepeatableRead(Kw!("REPEATABLE"), Space1, Kw!("READ")),
    }

    /// ROLLBACK [WORK] or ROLLBACK TO SAVEPOINT name; 2026.2 rejects ROLLBACK WORK TO SAVEPOINT.
    #[derive(Grammar)]
    pub enum RollB {
        Savepoint(
            Kw!("ROLLBACK"),
            Space1,
            Kw!("TO"),
            Space1,
            Kw!("SAVEPOINT"),
            Space,
            Ident,
        ),
        Bare(Kw!("ROLLBACK"), #[sugar] Option<(Space1, Kw!("WORK"))>),
    }

    #[derive(Grammar)]
    pub struct Savepoint {
        pub kw: (Kw!("SAVEPOINT"), Space),
        pub name: Ident,
    }

    /// LOCK [TABLE] t IN SHARE|EXCLUSIVE MODE [WAIT n] and UNLOCK [TABLE] t IN SHARE|EXCLUSIVE MODE [IMMEDIATE].
    #[derive(Grammar)]
    #[tygr(validated)]
    pub struct LockTable {
        pub kw: Either<Kw!("UNLOCK"), Kw!("LOCK")>,
        pub table_kw: Option<(Space1, Kw!("TABLE"))>,
        pub table: (Space, TName),
        pub mode: (
            Space,
            Kw!("IN"),
            Space1,
            Either<Kw!("SHARE"), Kw!("EXCLUSIVE")>,
            Space1,
            Kw!("MODE"),
        ),
        pub tail: Option<(Space1, LockTail)>,
    }

    #[derive(Grammar)]
    pub enum LockTail {
        Wait(Kw!("WAIT"), Space, Literal),
        Immediate(Kw!("IMMEDIATE")),
    }

    impl Validate for LockTable {
        const REQUIREMENT: &'static str = "take WAIT with LOCK and IMMEDIATE with UNLOCK";
        fn validate(&self) -> bool {
            !matches!(
                (&self.kw, &self.tail),
                (Either::Left(_), Some((_, LockTail::Wait(..))))
                    | (Either::Right(_), Some((_, LockTail::Immediate(_))))
            )
        }
    }

    #[derive(Grammar)]
    pub enum TPOption {
        InTransaction(Spellings!(Kw!("%INTRANSACTION"), Kw!("%INTRANS"))),
        BegTrans(Kw!("%BEGTRANS")),
        Commit {
            commit_kw: Kw!("COMMIT"),
            #[sugar]
            work: Option<(Space1, Kw!("WORK"))>,
        },
    }

    #[derive(Grammar)]
    pub enum OFC {
        Open {
            open_kw: (Kw!("OPEN"), Space),
            cursor: Ident,
        },
        Fetch {
            fetch_kw: (Kw!("FETCH"), Space),
            cursor: Ident,
            targt: Option<(Space, Targt)>,
        },
        Close {
            close_kw: (Kw!("CLOSE"), Space),
            cursor: Ident,
        },
    }

    #[derive(Grammar)]
    pub struct Decl {
        pub declare_kw: Kw!("DECLARE"),
        pub cursor_name: (Space, Ident),
        pub cursor_kw: (Space, Kw!("CURSOR"), Space1, Kw!("FOR"), Space),
        pub statement: DeclStatement,
        pub for_clause: Option<(Space, Prefix<(Kw!("FOR"), Space1), UpdtCls>)>,
    }

    #[derive(Grammar)]
    pub enum DeclStatement {
        Explain(Explain),
        Selct(Selct),
    }

    #[derive(Grammar)]
    pub enum UpdtCls {
        ReadOnly(Kw!("READ"), Space1, Kw!("ONLY")),
        Update(Kw!("UPDATE")),
    }

    #[derive(Grammar)]
    #[tygr(validated)]
    pub struct CallStatement {
        pub output: Option<(
            Either<HostVar, ParameterMarker>,
            Space,
            StringEq!("="),
            Space,
        )>,
        pub call_kw: Kw!("CALL"),
        pub routine: (Space, RoutineName),
        pub args: ArgsN,
        pub using: Option<(
            Space1,
            Kw!("USING"),
            Space,
            VecSep<Either<HostVar, ParameterMarker>, ArgSep>,
        )>,
    }

    impl Validate for CallStatement {
        const REQUIREMENT: &'static str = "take no column as an argument";
        fn validate(&self) -> bool {
            (self
                .args
                .1
                .wrapped
                .iter()
                .flat_map(|args| args.items.iter()))
            .all(|Scalx(terms)| {
                terms
                    .items
                    .iter()
                    .all(|term| !matches!(term.node, Term::Column(_)))
            })
        }
    }

    #[derive(Grammar)]
    pub struct RoutineName(pub Src<DottedIdent>);

    #[derive(Grammar)]
    pub struct Explain {
        pub explain_kw: Kw!("EXPLAIN"),
        pub options: Vec<(Space1, ExplainOption)>,
        pub statement: (Space, Box<Src<Stat>>),
    }

    /// EXPLAIN's options, in any order (2026.2 takes EXPLAIN STAT ALT, EXPLAIN INTO :x ALT and INTO :x, :y).
    #[derive(Grammar)]
    pub enum ExplainOption {
        Alt(Kw!("ALT")),
        All(Kw!("ALL")),
        Stat(Spellings!(Kw!("STAT"), Kw!("STATS"))),
        Into(Kw!("INTO"), Space, VecSep<Param, ArgSep>),
    }

    #[derive(Grammar)]
    #[tygr(validated)]
    #[sugar]
    pub struct Selct {
        pub with_clause: Option<(WithClause, Space)>,
        pub query: Query,
        pub tail: Tail,
    }

    /// 2026.2 refuses a parenthesized first operand after WITH (SQLCODE -383), not a parenthesized later one, and
    /// orders the rows as `Query::orders_once` says.
    impl Validate for Selct {
        const REQUIREMENT: &'static str =
            "begin with SELECT after WITH, order an operand only with TOP and its rows once";
        fn validate(&self) -> bool {
            (self.with_clause.is_none() || matches!(self.query.0.items[0], QTerm::Selex(_)))
                && self.query.orders_once(self.tail.order.is_some())
        }
    }

    /// The statement's ORDER BY, parsed as its last operand's, stands in the Tail, and a lone SELECT's row limit
    /// before ORDER BY after it, reading the same (LIMIT 2 ORDER BY a DESC gives the rows ORDER BY a DESC LIMIT 2
    /// gives); in a union a row limit limits its operand and stays.
    impl Desugar for Selct {
        type Core = core::Selct;
        fn desugar(&self) -> Self::Core {
            let mut query = self.query.desugar();
            let mut tail = self.tail.desugar();
            if let Some(order) = take_statement_order(&mut query) {
                tail.order = Some(order);
            }
            if let [core::QTerm::Selex(selex)] = &mut query.0.items[..]
                && tail.order.is_some()
                && tail.limit.is_none()
                && let Some((space, Either::Right(limit))) =
                    selex.node.tail.take_if(|(_, tail)| tail.is_right())
            {
                tail.limit = Some((space, limit));
            }
            core::Selct {
                with_clause: self.with_clause.desugar(),
                query,
                tail,
            }
        }
    }

    #[derive(Grammar)]
    pub struct SQLOption {
        pub kw: (Kw!("SET"), Space1, Kw!("OPTION"), Space1),
        pub setting: OptionSetting,
    }

    #[derive(Grammar)]
    pub struct CompileOption {
        pub compilemode_kw: Kw!("COMPILEMODE"),
        pub eq: (Space, StringEq!("="), Space),
        pub mode: CompileMode,
    }

    #[derive(Grammar)]
    pub enum OptionSetting {
        CompileOption(CompileOption),
        LockTimeout {
            lock_kw: Kw!("LOCK_TIMEOUT"),
            eq: (Space, StringEq!("="), Space),
            value: Number,
        },
        PkeyIsIdkey {
            pkey_kw: Kw!("PKEY_IS_IDKEY"),
            eq: (Space, StringEq!("="), Space),
            value: TrueFalse,
        },
        SupportDelimitedIdentifiers {
            support_kw: Kw!("SUPPORT_DELIMITED_IDENTIFIERS"),
            eq: (Space, StringEq!("="), Space),
            value: TrueFalse,
        },
        AutoParallelThreshold {
            auto_kw: Kw!("AUTO_PARALLEL_THRESHOLD"),
            eq: (Space, StringEq!("="), Space),
            value: Number,
        },
        DefaultSchema {
            default_kw: Kw!("DEFAULT_SCHEMA"),
            eq: (Space, StringEq!("="), Space),
            schema: Schema,
        },
        ExactDistinct {
            exact_kw: Kw!("EXACT_DISTINCT"),
            eq: (Space, StringEq!("="), Space),
            value: TrueFalse,
        },
        LockEscalationThreshold {
            lock_kw: Kw!("LOCK_ESCALATION_THRESHOLD"),
            eq: (Space, StringEq!("="), Space),
            value: Number,
        },
        LocaleOption(LocaleOption),
    }

    #[derive(Grammar)]
    pub enum CompileMode {
        Deferred(Kw!("DEFERRED")),
        Immediate(Kw!("IMMEDIATE")),
        Install(Kw!("INSTALL")),
        NoCheck(Kw!("NOCHECK")),
    }

    #[derive(Grammar)]
    pub enum TrueFalse {
        False(Kw!("FALSE")),
        True(Kw!("TRUE")),
    }

    #[derive(Grammar)]
    pub struct LocaleOption {
        pub setting: LocaleSetting,
        pub eq: (Space, StringEq!("="), Space),
        pub value: Literal,
    }

    #[derive(Grammar)]
    pub enum LocaleSetting {
        Am(Kw!("AM")),
        DateFormat(Kw!("DATE_FORMAT")),
        DateMaximum(Kw!("DATE_MAXIMUM")),
        DateMinimum(Kw!("DATE_MINIMUM")),
        DateSeparator(Kw!("DATE_SEPARATOR")),
        DecimalSeparator(Kw!("DECIMAL_SEPARATOR")),
        Midnight(Kw!("MIDNIGHT")),
        MinusSign(Kw!("MINUS_SIGN")),
        MonthAbbr(Kw!("MONTH_ABBR")),
        MonthName(Kw!("MONTH_NAME")),
        Noon(Kw!("NOON")),
        NumericGroupSeparator(Kw!("NUMERIC_GROUP_SEPARATOR")),
        NumericGroupSize(Kw!("NUMERIC_GROUP_SIZE")),
        Pm(Kw!("PM")),
        PlusSign(Kw!("PLUS_SIGN")),
        TimeFormat(Kw!("TIME_FORMAT")),
        TimePrecision(Kw!("TIME_PRECISION")),
        TimeSeparator(Kw!("TIME_SEPARATOR")),
        WeekdayAbbr(Kw!("WEEKDAY_ABBR")),
        WeekdayName(Kw!("WEEKDAY_NAME")),
        YearOption(Kw!("YEAR_OPTION")),
    }

    #[derive(Grammar)]
    pub struct Setml {
        pub kw: (
            Kw!("SET"),
            Space1,
            Kw!("ML"),
            Space1,
            Kw!("CONFIGURATION"),
            Space,
        ),
        pub name: MLConfigurationName,
    }

    #[derive(Grammar)]
    pub struct MLConfigurationName(pub Ident);

    pub type Grant = Prefix<Kw!("GRANT"), (Space, GrantCmd)>;

    /// WITH ADMIN OPTION goes with roles and admin privileges, WITH GRANT OPTION with objects.
    #[derive(Grammar)]
    #[tygr(validated)]
    pub struct GrantCmd {
        pub what: Granted,
        pub to_kw: (Space, Kw!("TO"), Space),
        pub grantees: Grntee,
        pub with: WithOption<Either<Kw!("ADMIN"), Kw!("GRANT")>>,
    }

    impl Validate for GrantCmd {
        const REQUIREMENT: &'static str =
            "take WITH ADMIN OPTION on roles and admin privileges, WITH GRANT OPTION on objects";
        fn validate(&self) -> bool {
            match &self.with {
                None => true,
                Some((.., Either::Left(_), _, _)) => self.what.is_admin(),
                Some((.., Either::Right(_), _, _)) => !self.what.is_admin(),
            }
        }
    }

    /// What GRANT gives and REVOKE takes back; the roles last, since a privilege keyword is an identifier too.
    #[derive(Grammar)]
    pub enum Granted {
        AdminPrivileges(VecSep<AdminPriv, ArgSep>),
        CubeSelect {
            select_kw: Kw!("SELECT"),
            columns: Option<(Space, FList1)>,
            on_kw: (Space, Kw!("ON"), Space1),
            cube_kw: Spellings!(Kw!("CUBE"), Kw!("CUBES")),
            cubes: (Space, CubeList),
        },
        ObjectPrivileges {
            privileges: SimplePrivs,
            on_kw: (Space, Kw!("ON"), Space),
            objects: Objcts,
        },
        Actions {
            actions: VecSep<Action, ArgSep>,
            on_kw: (Space, Kw!("ON"), Space),
            object: Object,
        },
        Roles(VecSep<Role, ArgSep>),
    }

    impl Granted {
        fn is_admin(&self) -> bool {
            matches!(self, Self::AdminPrivileges(_) | Self::Roles(_))
        }
    }

    pub type WithOption<K> = Option<(Space, Kw!("WITH"), Space, K, Space, Kw!("OPTION"))>;

    pub type GrantOptionFor = Option<(
        Kw!("GRANT"),
        Space1,
        Kw!("OPTION"),
        Space1,
        Kw!("FOR"),
        Space,
    )>;

    pub type Revok = Prefix<Kw!("REVOKE"), (Space, RevokeCmd)>;

    /// GRANT OPTION FOR, CASCADE or RESTRICT and AS go with objects, AS not with cubes.
    #[derive(Grammar)]
    #[tygr(validated)]
    pub struct RevokeCmd {
        pub grant_option: GrantOptionFor,
        pub what: Granted,
        pub from_kw: (Space, Kw!("FROM"), Space),
        pub grantees: Grntee,
        pub behavior: Option<(Space, CascadeOpt)>,
        pub as_clause: Option<(Space, Kw!("AS"), Space, Grntee)>,
    }

    impl Validate for RevokeCmd {
        const REQUIREMENT: &'static str =
            "take GRANT OPTION FOR, CASCADE, RESTRICT and AS on objects alone, AS not on cubes";
        fn validate(&self) -> bool {
            let object = !self.what.is_admin();
            let plain =
                self.grant_option.is_none() && self.behavior.is_none() && self.as_clause.is_none();
            (object || plain)
                && !(matches!(self.what, Granted::CubeSelect { .. }) && self.as_clause.is_some())
        }
    }

    #[derive(Grammar)]
    pub enum SimplePrivs {
        All {
            all_kw: Kw!("ALL"),
            privileges_kw: Option<(Space1, Kw!("PRIVILEGES"))>,
        },
        Star(StringEq!("*")),
        List(VecSep<SimplePriv, ArgSep>),
        Execute(Kw!("EXECUTE")),
        Use(Kw!("USE")),
    }

    #[derive(Grammar)]
    pub enum SimplePriv {
        Alter(Kw!("%ALTER")),
        Delete(Kw!("DELETE")),
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
    pub enum OptColPriv {
        Select(Kw!("SELECT")),
        Insert(Kw!("INSERT")),
        Update(Kw!("UPDATE")),
        References(Kw!("REFERENCES")),
    }

    #[derive(Grammar)]
    pub enum AdminPriv {
        CreateMethod(Kw!("%CREATE_METHOD")),
        DropMethod(Kw!("%DROP_METHOD")),
        CreateFunction(Kw!("%CREATE_FUNCTION")),
        DropFunction(Kw!("%DROP_FUNCTION")),
        CreateProcedure(Kw!("%CREATE_PROCEDURE")),
        DropProcedure(Kw!("%DROP_PROCEDURE")),
        CreateQuery(Kw!("%CREATE_QUERY")),
        DropQuery(Kw!("%DROP_QUERY")),
        CreateTable(Kw!("%CREATE_TABLE")),
        AlterTable(Kw!("%ALTER_TABLE")),
        DropTable(Kw!("%DROP_TABLE")),
        CreateView(Kw!("%CREATE_VIEW")),
        AlterView(Kw!("%ALTER_VIEW")),
        DropView(Kw!("%DROP_VIEW")),
        CreateTrigger(Kw!("%CREATE_TRIGGER")),
        DropTrigger(Kw!("%DROP_TRIGGER")),
        DBObjectDefinition(Kw!("%DB_OBJECT_DEFINITION")),
        NoCheck(Kw!("%NOCHECK")),
        NoTrigger(Kw!("%NOTRIGGER")),
        NoIndex(Kw!("%NOINDEX")),
        NoLock(Kw!("%NOLOCK")),
        CreateMLConfiguration(Kw!("%CREATE_ML_CONFIGURATION")),
        AlterMLConfiguration(Kw!("%ALTER_ML_CONFIGURATION")),
        DropMLConfiguration(Kw!("%DROP_ML_CONFIGURATION")),
        ManageModel(Kw!("%MANAGE_MODEL")),
        UseModel(Kw!("%USE_MODEL")),
        NoJourn(Kw!("%NOJOURN")),
        BuildIndex(Kw!("%BUILD_INDEX")),
        DropUnowned(Kw!("%DROP_UNOWNED")),
        ManageForeignServer(Kw!("%MANAGE_FOREIGN_SERVER")),
        CancelQuery(Kw!("%CANCEL_QUERY")),
    }

    #[derive(Grammar)]
    pub enum Grntee {
        Star(StringEq!("*")),
        List(VecSep<User, ArgSep>),
    }

    #[derive(Grammar)]
    pub enum User {
        Public(Kw!("PUBLIC")),
        Ident(Option<StringEq!("_")>, Ident),
    }

    #[derive(Grammar)]
    pub struct Role(pub Ident);

    #[derive(Grammar)]
    pub enum Objcts {
        Star(StringEq!("*")),
        Schemas {
            schema_kw: (Kw!("SCHEMA"), Space),
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
    pub struct CubeList(pub VecSep<CubeName, ArgSep>);

    #[derive(Grammar)]
    pub struct CubeName(pub Ident);

    #[derive(Grammar)]
    pub enum ForeignServerPrivilegeList {
        Star(StringEq!("*")),
        List(VecSep<TName, ArgSep>),
    }

    #[derive(Grammar)]
    pub struct ForeignServerPrivilegeClause {
        pub foreign_kw: Option<(Kw!("FOREIGN"), Space1)>,
        pub server_kw: Spellings!(Kw!("SERVER"), Kw!("SERVERS")),
        // A table named Servers: GRANT SELECT ON Servers TO u1 is a table grant on IRIS.
        pub servers: (
            Space1,
            NotFollowedBy<(Either<Kw!("TO"), Kw!("FROM")>, Space1)>,
            ForeignServerPrivilegeList,
        ),
    }

    #[derive(Grammar)]
    pub struct ChkPrv {
        pub checkpriv_kw: Kw!("%CHECKPRIV"),
        pub grant_option: Option<(Space1, GrantOpt)>,
        pub command: (Space1, ChkPrvCmd),
    }

    #[derive(Grammar)]
    pub struct GrantOpt {
        pub kind: Either<Kw!("GRANT"), Kw!("ADMIN")>,
        pub option_for: (Space1, Kw!("OPTION"), Space1, Kw!("FOR")),
    }

    #[derive(Grammar)]
    pub enum ChkPrvCmd {
        Admin(VecSep<AdminPriv, ArgSep>),
        PrivSpec {
            priv_spec: PrivSpec,
            on_kw: (Space, Kw!("ON"), Space),
            object: Object,
        },
    }

    #[derive(Grammar)]
    pub enum PrivSpec {
        Action(Action),
        Execute(Kw!("EXECUTE")),
        Use(Kw!("USE")),
    }

    type OptParenScalx = Option<Parenthesized<Scalx>>;
    type OptParenScalx2 = Option<Parenthesized<(Scalx, Option<(ArgSep, Scalx)>)>>;

    /// CAST's types, the varying ones before the types they begin with; 2026.2 takes NCHAR VARYING and refuses
    /// DOUBLE PRECISION, INT(10) and VARCHAR(10, 2).
    #[derive(Grammar)]
    pub enum CastType {
        Bigint(Kw!("BIGINT")),
        Varbinary(
            Spellings!(Kw!("VARBINARY"), (Kw!("BINARY"), Space1, Kw!("VARYING"))),
            OptParenScalx,
        ),
        Binary(Kw!("BINARY"), OptParenScalx),
        Bit(Kw!("BIT")),
        Varchar(
            Spellings!(
                Kw!("VARCHAR"),
                (
                    Spellings!(Kw!("CHARACTER"), Kw!("CHAR")),
                    Space1,
                    Kw!("VARYING")
                )
            ),
            OptParenScalx,
        ),
        Char(Spellings!(Kw!("CHAR"), Kw!("CHARACTER")), OptParenScalx),
        Datetime(Kw!("DATETIME")),
        Date(Kw!("DATE")),
        Decimal(Spellings!(Kw!("DECIMAL"), Kw!("DEC")), OptParenScalx2),
        Double(Kw!("DOUBLE")),
        Float(Kw!("FLOAT")),
        Guid(Kw!("GUID")),
        Integer(Spellings!(Kw!("INTEGER"), Kw!("INT"))),
        Money(Kw!("MONEY")),
        Nvarchar(
            Spellings!(Kw!("NVARCHAR"), (Kw!("NCHAR"), Space1, Kw!("VARYING"))),
            OptParenScalx,
        ),
        Nchar(Kw!("NCHAR"), OptParenScalx),
        Numeric(Kw!("NUMERIC"), OptParenScalx2),
        Posixtime(Kw!("POSIXTIME")),
        Real(Kw!("REAL")),
        Smalldatetime(Kw!("SMALLDATETIME")),
        Smallint(Kw!("SMALLINT")),
        Smallmoney(Kw!("SMALLMONEY")),
        Timestamp(Kw!("TIMESTAMP")),
        Time(Kw!("TIME")),
        Tinyint(Kw!("TINYINT")),
    }

    pub type IfExists = Option<(Kw!("IF"), Space1, Kw!("EXISTS"), Space)>;
    pub type IfNotExists = Option<(Kw!("IF"), Space1, Kw!("NOT"), Space1, Kw!("EXISTS"), Space)>;
    pub type OrReplace = Option<(Kw!("OR"), Space1, Kw!("REPLACE"), Space)>;

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct UserSpec {
        pub name: Ident,
        pub password: Option<(Space, PWPrefix, Space, Password)>,
        pub default: Option<((Space, Kw!("DEFAULT"), Space), Ident, (Space, Ident))>,
        pub temporary: Option<((Space, Kw!("TEMPORARY"), Space), Ident, (Space, Ident))>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ServerKw {
        pub foreign: Option<(Kw!("FOREIGN"), Space1)>,
        pub server_kw: (Kw!("SERVER"), Space),
    }

    #[derive(Grammar)]
    pub enum DDL {
        Create {
            kw: (Kw!("CREATE"), Space1),
            what: Created,
        },
        Alter {
            kw: (Kw!("ALTER"), Space1),
            what: Altered,
        },
        Drop {
            kw: (Kw!("DROP"), Space1),
            what: Dropped,
        },
        BuildIndex {
            kw: (Kw!("BUILD"), Space1, Kw!("INDEX"), Space1),
            options: Vec<(BuildOption, Space1)>,
            for_kw: (Kw!("FOR"), Space1),
            scope: BuildScope,
        },
        CancelQuery {
            kw: (Kw!("CANCEL"), Space1, Kw!("QUERY"), Space),
            val: Count,
            cancel_scope: Option<(Space, CancelQueryScope)>,
            timeout: Option<(Space, Kw!("TIMEOUT"), Space, Count)>,
        },
        Collect {
            kw: (Kw!("COLLECT"), Space1),
            clause: StatsClause,
            target: (Space1, Kw!("FOR"), Space1, StatsScope),
            sample: Option<((Space, Kw!("%SAMPLE_PERCENT"), Space), PercentValue)>,
        },
        FreezePlans {
            kw: (Kw!("FREEZE"), Space1, Kw!("PLANS")),
            by: Option<((Space1, Kw!("BY"), Space1), FreezeScope)>,
        },
        Load {
            kw: (Kw!("LOAD"), Space),
            spec: LoadSpec,
        },
        PurgeQueries {
            kw: (Kw!("PURGE"), Space1),
            cached: Option<(Kw!("CACHED"), Space1)>,
            queries: Kw!("QUERIES"),
            by: Option<((Space1, Kw!("BY"), Space1), PurgeScope)>,
        },
        TrainModel {
            kw: (Kw!("TRAIN"), Space1, Kw!("MODEL"), Space),
            name: ModelName,
            as_name: Option<((Space, Kw!("AS"), Space), ModelName)>,
            not_default: Option<(Space, Kw!("NOT"), Space1, Kw!("DEFAULT"))>,
            for_col: Option<((Space, Kw!("FOR"), Space), DottedIdent)>,
            with_clause: Option<(Space, WithClauseTrain)>,
            source: Option<(Space, ModelSource)>,
            using_clause: Option<(Space, UsingClause)>,
        },
        TuneTable {
            kw: (Kw!("TUNE"), Space1, Kw!("TABLE"), Space),
            tname: TName,
            options: Vec<(Space, TuneOption)>,
        },
        UnfreezePlans {
            kw: (Kw!("UNFREEZE"), Space1, Kw!("PLANS")),
            from_upgrade: Option<(Space1, Option<(Kw!("FROM"), Space1)>, Kw!("UPGRADE"))>,
            by: Option<((Space1, Kw!("BY"), Space1), FreezeScope)>,
        },
        UseDatabase {
            kw: (Kw!("USE"), Space1),
            database: Option<(Kw!("DATABASE"), Space1)>,
            name: DatabaseName,
        },
        ValidateModel {
            kw: (Kw!("VALIDATE"), Space1, Kw!("MODEL"), Space),
            name: ModelName,
            as_name: Option<((Space, Kw!("AS"), Space), ModelName)>,
            use_name: Option<((Space, Kw!("USE"), Space), ModelName)>,
            with_clause: Option<(Space, WithClauseTrain)>,
            source: Option<(Space, ModelSource)>,
        },
    }

    /// What CREATE makes; OR REPLACE where 2026.2 takes it (VIEW, AGGREGATE, ML CONFIGURATION, TRIGGER and the
    /// routines), since CREATE OR REPLACE TABLE, INDEX, USER, ROLE, SCHEMA, DATABASE, MODEL and SERVER fail (-51).
    #[derive(Grammar)]
    pub enum Created {
        BitmapextentIndex {
            kw: (Kw!("BITMAPEXTENT"), Space1, Kw!("INDEX"), Space),
            name: IndexName,
            on_kw: (Space, Kw!("ON"), Space),
            table_kw: Option<(Kw!("TABLE"), Space)>,
            tname: TName,
        },
        Database {
            kw: (Kw!("DATABASE"), Space1),
            file: Option<(Kw!("FILE"), Space1)>,
            name: DatabaseName,
            on_dir: Option<OnDirectory>,
        },
        ForeignTable {
            kw: (Kw!("FOREIGN"), Space1, Kw!("TABLE"), Space),
            if_not_exists: IfNotExists,
            tname: TName,
            columns: Option<Args<VecSep<(Ident, (Space, Datyp)), ArgSep>>>,
            server: (Space, Kw!("SERVER"), Space),
            server_name: TName,
            ft_options: Option<(Space, FTOptions)>,
            values: Option<(Space, Kw!("VALUES"), Space, FList1)>,
            using_json: Option<((Space, Kw!("USING"), Space), JSONString)>,
            novalidate: Option<(Space, Kw!("NOVALIDATE"))>,
        },
        Model {
            kw: (Kw!("MODEL"), Space),
            if_not_exists: IfNotExists,
            name: ModelName,
            predicting: (Space, Kw!("PREDICTING"), Space),
            pred: Parenthesized<(Ident, Option<(Space, Datyp)>)>,
            source: Option<(Space, ModelSource)>,
            with_clause: Option<(Space, WithClauseCreate)>,
            using_clause: Option<(Space, UsingClause)>,
        },
        Role {
            kw: (Kw!("ROLE"), Space),
            name: Ident,
        },
        Schema {
            kw: (Kw!("SCHEMA"), Space),
            if_not_exists: IfNotExists,
            name: Ident,
        },
        TableAs {
            kw: (Kw!("TABLE"), Space),
            if_not_exists: IfNotExists,
            nojourn: Option<(Kw!("%NOJOURN"), Space)>,
            tname: TName,
            as_kw: (Space, Kw!("AS"), Space),
            with_clause: Option<(WithClause, Space)>,
            query: SourceQuery,
            options: Option<(Space, TableOptionList)>,
        },
        User {
            kw: (Kw!("USER"), Space),
            user: UserSpec,
        },
        Server {
            server_kw: ServerKw,
            if_not_exists: IfNotExists,
            tname: TName,
            type_clause: Option<((Space, Kw!("TYPE"), Space), ServerType)>,
            fdw: (
                Space,
                Kw!("FOREIGN"),
                Space1,
                Kw!("DATA"),
                Space1,
                Kw!("WRAPPER"),
                Space,
            ),
            fdw_name: FDWName,
            connection: (Space, CreateServerConnection),
            delimited: Option<(Space, CreateServerDelimitedIds)>,
            passthrough: Option<(Space, EnableDisable, Space1, Kw!("PASSTHROUGH"))>,
            pushdown: Option<(Space, FTPushdownClause)>,
            using_json: Option<((Space, Kw!("USING"), Space), JSONString)>,
        },
        Index {
            partitioned: Option<(CreateIndexPartitioned, Space1)>,
            qualifiers: Option<(IndexQualifiers, Space)>,
            index_kw: (Kw!("INDEX"), Space),
            name: IndexName,
            on_kw: (Space, Kw!("ON"), Space),
            table_kw: Option<(Kw!("TABLE"), Space)>,
            tname: TName,
            comp_list: Args<VecSep<ComponentSpec, ArgSep>>,
            as_clause: Option<(
                (Space, Kw!("AS"), Space),
                ClassName,
                Option<(Space, CreateIndexAsParams)>,
            )>,
            with_clause: Option<(Space, CreateIndexWithClause)>,
            in_on: Option<(Space, InOn, (Space, Token))>,
            defer_immediate: Option<(Space, ImmediateDefer, Option<(Space1, Kw!("BUILD"))>)>,
        },
        View {
            or_replace: OrReplace,
            def: ViewDef,
        },
        Routine(CreateRoutine),
        Aggregate {
            or_replace: OrReplace,
            agg_kw: (Kw!("AGGREGATE"), Space),
            name: QualifiedSPName,
            params: Args<VecSep<(Ident, Space1, Datyp), ArgSep>>,
            returns: Option<((Space, Kw!("RETURNS"), Space), Datyp)>,
            initialize: Option<(
                (Space, Kw!("INITIALIZE"), Space1, Kw!("WITH"), Space),
                QualifiedSPName,
            )>,
            iterate: (
                (Space, Kw!("ITERATE"), Space1, Kw!("WITH"), Space),
                QualifiedSPName,
            ),
            merge: Option<(
                (Space, Kw!("MERGE"), Space1, Kw!("WITH"), Space),
                QualifiedSPName,
            )>,
            finalize: Option<(
                (Space, Kw!("FINALIZE"), Space1, Kw!("WITH"), Space),
                QualifiedSPName,
            )>,
        },
        MlConfiguration {
            or_replace: OrReplace,
            ml_kw: (Kw!("ML"), Space1, Kw!("CONFIGURATION"), Space),
            name: MLConfigurationName,
            provider: ((Space, Kw!("PROVIDER"), Space1), ProviderName),
            desc: Option<((Space1, Kw!("%DESCRIPTION"), Space), LiteralOrNull)>,
            using_clause: Option<(Space, MlconfigUsingClause)>,
            settings: Option<(Space, ProviderSpecificSettings)>,
        },
        Trigger {
            or_replace: OrReplace,
            trigger_kw: (Kw!("TRIGGER"), Space),
            name: TName,
            time: (Space, TriggerTime, Space1),
            events: VecSep<TriggerEvent, ArgSep>,
            order: Option<(Space1, Kw!("ORDER"), Space, Literal)>,
            on: (Space, Kw!("ON"), Space, TName),
            referencing: Option<(
                Space1,
                Kw!("REFERENCING"),
                Vec1<(Space1, TriggerReferencing)>,
            )>,
            for_each: Option<(
                Space1,
                Kw!("FOR"),
                Space1,
                Kw!("EACH"),
                Space1,
                TriggerOrientation,
            )>,
            when: Option<(Space, Kw!("WHEN"), Space, Parenthesized<Prdct>)>,
            body: (Space, TriggerBody),
        },
        Table {
            temp: Option<(CreateTableTemp, Space1)>,
            table_kw: (Kw!("TABLE"), Space),
            if_not_exists: IfNotExists,
            tname: TName,
            elems: Args<VecSep<Telem, ArgSep>>,
            options: Option<(Space, TableOptionList)>,
        },
    }

    /// What ALTER changes; the statistics forms first, since ALTER TABLE t FIX STATISTICS is one of them.
    #[derive(Grammar)]
    pub enum Altered {
        Stats {
            scope: StatsScope,
            action: (Space, StatsAction),
        },
        ForeignTable {
            kw: (Kw!("FOREIGN"), Space1, Kw!("TABLE"), Space),
            tname: TName,
        },
        MlConfiguration {
            kw: (Kw!("ML"), Space1, Kw!("CONFIGURATION"), Space),
            name: MLConfigurationName,
            clauses: Vec1<(Space, MlConfigClause)>,
        },
        Model {
            kw: (Kw!("MODEL"), Space),
            name: ModelName,
            action: (Space, AlterModelAction),
        },
        Table {
            kw: (Kw!("TABLE"), Space),
            tname: TName,
            with_check: Option<((Space, Kw!("WITH"), Space1), CheckSpec)>,
            action: Option<(Space, AlterAction)>,
        },
        User {
            kw: (Kw!("USER"), Space),
            user: UserSpec,
        },
        Server {
            server_kw: ServerKw,
            tname: TName,
            action: (Space, AlterServerAction),
        },
        View(ViewDef),
    }

    #[derive(Grammar)]
    pub enum StatsAction {
        DropFixed {
            kw: (Kw!("DROP"), Space1, Kw!("FIXED"), Space1),
            clause: StatsClause,
        },
        Fix {
            kw: (Kw!("FIX"), Space1),
            clause: StatsClause,
        },
        SetRuntime {
            kw: (Kw!("SET"), Space1, Kw!("RUNTIME"), Space1),
            prop: StatsProperty,
            eq: (Space, StringEq!("="), Space),
            val: TrueFalse,
        },
    }

    /// What DROP removes; an index by its dotted name, ON its table, or both (2026.2 takes DROP INDEX t.i ON t).
    #[derive(Grammar)]
    pub enum Dropped {
        Trigger {
            kw: (Kw!("TRIGGER"), Space),
            if_exists: IfExists,
            name: TName,
            from: Option<((Space, Kw!("FROM"), Space), TName)>,
        },
        Database {
            kw: (Kw!("DATABASE"), Space1),
            if_exists: IfExists,
            name: DatabaseName,
            retain: Option<(Space1, Kw!("RETAIN_FILES"))>,
        },
        ForeignTable {
            kw: (Kw!("FOREIGN"), Space1, Kw!("TABLE"), Space),
            if_exists: IfExists,
            tname: TName,
            cascade: Option<(Space, CascadeOpt)>,
        },
        Routine {
            kind: (RoutineKind, Space),
            if_exists: IfExists,
            name: QualifiedSPName,
            from: Option<((Space, Kw!("FROM"), Space), ClassName)>,
        },
        Index {
            kw: (Kw!("INDEX"), Space),
            if_exists: IfExists,
            nojourn: Option<(Kw!("%NOJOURN"), Space)>,
            name: VecSep<Ident, (Space, StringEq!("."), Space)>,
            on: Option<(
                (Space, Kw!("ON"), Space),
                Option<(Kw!("TABLE"), Space)>,
                TName,
            )>,
        },
        MlConfiguration {
            kw: (Kw!("ML"), Space1, Kw!("CONFIGURATION"), Space),
            if_exists: IfExists,
            name: MLConfigurationName,
        },
        Model {
            kw: (Kw!("MODEL"), Space),
            if_exists: IfExists,
            name: ModelName,
        },
        Role {
            kw: (Kw!("ROLE"), Space),
            if_exists: IfExists,
            name: Ident,
        },
        Schema {
            kw: (Kw!("SCHEMA"), Space),
            if_exists: IfExists,
            name: Ident,
            cascade_restrict: Option<(Space, CascadeOpt)>,
        },
        Table {
            kw: (Kw!("TABLE"), Space),
            if_exists: IfExists,
            nojourn: Option<(Kw!("%NOJOURN"), Space)>,
            tname: TName,
            cascade: Option<(Space, CascadeOpt)>,
            data: Option<(Space, DataOption)>,
        },
        User {
            kw: (Kw!("USER"), Space),
            if_exists: IfExists,
            name: Ident,
            cascade: Option<(Space, Kw!("CASCADE"))>,
        },
        View {
            kw: (Kw!("VIEW"), Space),
            if_exists: IfExists,
            tname: TName,
            cascade: Option<(Space, CascadeOpt)>,
        },
        Server {
            server_kw: ServerKw,
            if_exists: IfExists,
            tname: TName,
            cascade: Option<(Space, CascadeOpt)>,
        },
        Aggregate {
            kw: (Kw!("AGGREGATE"), Space),
            if_exists: IfExists,
            name: QualifiedSPName,
        },
    }

    #[derive(Grammar)]
    pub struct OnDirectory {
        pub kw: (Space1, Kw!("ON"), Space1, Kw!("DIRECTORY")),
        #[sugar]
        pub eq: Option<(Space, StringEq!("="))>,
        pub dir: (Space, DirectoryName),
    }

    #[derive(Grammar)]
    pub struct ViewDef {
        pub kw: (Kw!("VIEW"), Space),
        pub tname: TName,
        pub col_list: Option<(Space, FList1)>,
        pub as_kw: (Space, Kw!("AS"), Space),
        pub with_clause: Option<(WithClause, Space)>,
        pub query: QueryExpr,
        pub view_with: Option<((Space, Kw!("WITH"), Space1), ViewWith)>,
    }

    /// 2026.2 takes STATIC before METHOD alone ("METHOD expected" after CREATE STATIC PROCEDURE) and not with OR
    /// REPLACE (-51), an external language for a FUNCTION or PROCEDURE alone ("'SQL', 'OBJECTSCRIPT', or 'PYTHON'
    /// expected" for a METHOD) and SQL or OBJECTSCRIPT for a QUERY ("'SQL', or 'OBJECTSCRIPT' expected").
    #[derive(Grammar)]
    #[tygr(validated)]
    pub struct CreateRoutine {
        pub or_replace: OrReplace,
        pub static_kw: Option<(Kw!("STATIC"), Space1)>,
        pub kind: (RoutineKind, Space),
        pub name: QualifiedSPName,
        pub spec: (Space, CodeSpecification),
    }

    impl Validate for CreateRoutine {
        const REQUIREMENT: &'static str =
            "take STATIC on a METHOD without OR REPLACE, and a language its kind allows";
        fn validate(&self) -> bool {
            let (kind, body) = (&self.kind.0, &self.spec.1.body.1);
            let static_ok = self.static_kw.is_none()
                || (matches!(kind, RoutineKind::Method(_)) && self.or_replace.is_none());
            let language_ok = match kind {
                RoutineKind::Method(_) => !matches!(body, CodeBody::External { .. }),
                RoutineKind::Query(_) => {
                    matches!(body, CodeBody::Sql { .. } | CodeBody::Cos { .. })
                }
                _ => true,
            };
            static_ok && language_ok
        }
    }

    #[derive(Grammar)]
    pub enum RoutineKind {
        Function(Kw!("FUNCTION")),
        Method(Kw!("METHOD")),
        Procedure(ProcedureKw),
        Query(Kw!("QUERY")),
    }

    pub type ProcedureKw = Spellings!(Kw!("PROCEDURE"), Kw!("PROC"));

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct CreateTableTemp {
        pub private_global: PrivateGlobal,
        pub space: Space1,
        pub temporary: Kw!("TEMPORARY"),
    }

    #[derive(Grammar)]
    pub enum PrivateGlobal {
        Global(Kw!("GLOBAL")),
        Private(Kw!("PRIVATE")),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum CreateIndexPartitioned {
        NonPartitioned(Kw!("NONPARTITIONED")),
        Partitioned(Kw!("PARTITIONED")),
    }

    #[derive(Grammar)]
    pub struct CreateServerConnection {
        pub kind: (Either<Kw!("CONNECTION"), Kw!("HOST")>, Space),
        pub value: Literal,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum CreateServerDelimitedIds {
        Delimited(Kw!("DELIMITEDIDS")),
        NoDelimited(Kw!("NODELIMITEDIDS")),
    }

    #[derive(Grammar)]
    pub enum EnableDisable {
        Enable(Kw!("ENABLE")),
        Disable(Kw!("DISABLE")),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum CreateIndexAsParams {
        Parens(Parenthesized<Option<VecSep<IndexParameterSpec, ArgSep>>>),
        With {
            kw: (Kw!("WITH"), Space),
            params: VecSep<IndexParameterSpec, ArgSep>,
        },
    }

    /// 2026.2 takes `WITH DATA (b) PAD_INDEX, FILLFACTOR = 80`, not `WITH DATA (b), PAD_INDEX`.
    #[derive(Grammar)]
    #[tygr(inline, validated)]
    pub struct CreateIndexWithClause {
        pub kw: (Kw!("WITH"), Space),
        pub data: Option<(Kw!("DATA"), Space, FList1)>,
        pub pad_index: IndexWithItem<Kw!("PAD_INDEX")>,
        pub fillfactor: IndexWithItem<(Kw!("FILLFACTOR"), (Space, StringEq!("="), Space), Literal)>,
        pub ignore_dup: IndexWithItem<Kw!("IGNORE_DUP_KEY")>,
        pub drop_existing: IndexWithItem<Kw!("DROP_EXISTING")>,
        pub stats_norecompute: IndexWithItem<Kw!("STATISTICS_NORECOMPUTE")>,
    }

    impl Validate for CreateIndexWithClause {
        const REQUIREMENT: &'static str = "put no comma before the first option";
        fn validate(&self) -> bool {
            let commas = [
                self.pad_index.as_ref().map(|item| item.1.is_some()),
                self.fillfactor.as_ref().map(|item| item.1.is_some()),
                self.ignore_dup.as_ref().map(|item| item.1.is_some()),
                self.drop_existing.as_ref().map(|item| item.1.is_some()),
                self.stats_norecompute.as_ref().map(|item| item.1.is_some()),
            ];
            commas.iter().flatten().next() != Some(&true)
        }
    }

    pub type IndexWithItem<K> = Option<(Space, Option<(StringEq!(","), Space)>, K)>;

    #[derive(Grammar)]
    pub enum ImmediateDefer {
        Immediate(Kw!("IMMEDIATE")),
        Defer(Kw!("DEFER")),
    }

    #[derive(Grammar)]
    pub struct ModelName(pub Ident);

    #[derive(Grammar)]
    pub enum Password {
        HostVar(HostVar),
        Ident(Ident),
        Literal(Literal),
        NULL(Kw!("NULL")),
    }

    pub type PWPrefix = Spellings!(
        (Kw!("IDENTIFY"), Space1, Kw!("BY")),
        (Kw!("IDENTIFIED"), Space1, Kw!("BY")),
        (Option<(Kw!("WITH"), Space1)>, Kw!("PASSWORD"))
    );

    #[derive(Grammar)]
    pub enum BuildScope {
        All(Kw!("ALL")),
        Schema((Kw!("SCHEMA"), Space), Ident),
        Table {
            table_kw: (Kw!("TABLE"), Space),
            name: VecSep<Ident, (Space, StringEq!("."), Space)>,
            index: Option<((Space, Kw!("INDEX"), Space), VecSep<Ident, ArgSep>)>,
        },
    }

    #[derive(Grammar)]
    pub enum BuildOption {
        NoJourn(Kw!("%NOJOURN")),
        NoLock(Kw!("%NOLOCK")),
    }

    #[derive(Grammar)]
    pub enum PurgeScope {
        Age((Kw!("AGE"), Space), Literal),
        Name {
            name_kw: (Kw!("NAME"), Space),
            names: VecSep<PurgeName, ArgSep>,
        },
        Table((Kw!("TABLE"), Space), TName),
    }

    #[derive(Grammar)]
    pub enum PurgeName {
        Literal(Literal),
        Dotted(VecSep<Either<StringEq!("*"), Ident>, (Space, StringEq!("."), Space)>),
    }

    #[derive(Grammar)]
    pub enum FreezeScope {
        Id((Kw!("ID"), Space), Literal),
        Scope(StatsScope),
    }

    pub type CancelQueryScope = Prefix<(Kw!("IDENTIFIED"), Space1, Kw!("BY"), Space), Count>;

    pub type WithClauseCreate = Prefix<(Kw!("WITH"), Space), Parenthesized<ColumnDefList>>;

    #[derive(Grammar)]
    pub enum ModelSource {
        Subquery {
            kw: (Kw!("FROM"), Space),
            selex: Parenthesized<Src<Selex>>,
        },
        Inline {
            from: FromClause,
            where_clause: Option<(Space, WhereClause)>,
            group: Option<(Space, Group)>,
            having: Option<(Space, Having)>,
            tail: Tail,
        },
    }

    pub type UsingClause = Prefix<(Kw!("USING"), Space), AlgDescription>;

    #[derive(Grammar)]
    pub struct AlgDescription(pub JSONObject);

    pub type WithClauseTrain = Prefix<(Kw!("WITH"), Space), TrainWith>;

    #[derive(Grammar)]
    pub enum TrainWith {
        List(FList1),
        Assignment(WithValueAssignment),
    }

    #[derive(Grammar)]
    pub struct WithValueAssignment(
        pub VecSep<(Ident, (Space, StringEq!("="), Space), WithExpression), ArgSep>,
    );

    #[derive(Grammar)]
    pub enum WithExpression {
        Ident(Ident),
        Literal(Literal),
        Scalx(Scalx),
    }

    #[derive(Grammar)]
    pub enum AlterModelAction {
        Default(DefaultTrainedModel),
        Purge(Kw!("PURGE"), Option<(Space, Scope)>),
    }

    #[derive(Grammar)]
    pub struct DefaultTrainedModel {
        pub kw: Kw!("DEFAULT"),
        pub trained_model: Option<(Space1, Kw!("TRAINED"), Space1, Kw!("MODEL"))>,
        pub name: (Space, TrainedModelName),
    }

    #[derive(Grammar)]
    pub enum Scope {
        Days(NonNegativeInteger, Space1, Kw!("DAYS")),
        All(Kw!("ALL")),
    }

    #[derive(Grammar)]
    pub enum ProviderName {
        AutoML(Kw!("AutoML")),
        DataRobot(Kw!("DataRobot")),
        H2O(Kw!("H2O")),
        PMML(Kw!("PMML")),
    }

    #[derive(Grammar)]
    pub enum LiteralOrNull {
        Literal(Literal),
        Null(Kw!("NULL")),
    }

    pub type MlconfigUsingClause = Prefix<(Kw!("USING"), Space), MlconfigUsing>;

    /// What ALTER ML CONFIGURATION changes, in any order; 2026.2 rejects the statement without a clause.
    #[derive(Grammar)]
    pub enum MlConfigClause {
        Provider(Kw!("PROVIDER"), Space1, ProviderName),
        Description(Kw!("%DESCRIPTION"), Space, LiteralOrNull),
        Using(MlconfigUsingClause),
        Settings(ProviderSpecificSettings),
    }

    #[derive(Grammar)]
    pub enum MlconfigUsing {
        JSONString(JSONObject),
        Null(Kw!("NULL")),
    }

    #[derive(Grammar)]
    pub enum ProviderSpecificSettings {
        DR(VecSep<DROption, Space>),
        H2O(VecSep<H2OOption, Space>),
    }

    #[derive(Grammar)]
    pub enum TuneOption {
        ClearValues(Kw!("%CLEAR_VALUES")),
        KeepUpToDate(Kw!("%KEEP_UP_TO_DATE")),
        RecompileCQ(Kw!("%RECOMPILE_CQ")),
        SamplePercent(Kw!("%SAMPLE_PERCENT"), (Space, PercentValue)),
    }

    #[derive(Grammar)]
    pub struct DROption {
        pub name: Either<Kw!("APITOKEN"), Kw!("URL")>,
        pub value: (Space, LiteralOrNull),
    }

    #[derive(Grammar)]
    pub struct H2OOption {
        pub kw: Kw!("JAVATLSCONFIGFILE"),
        pub val: (Space, LiteralOrNull),
    }

    #[derive(Grammar)]
    pub enum FTOptions {
        Table((Kw!("TABLE"), Space), Literal),
        Query((Kw!("QUERY"), Space), Literal),
        File(FileSource),
    }

    pub type JSONObject = crate::cos::Brace<crate::cos::OSText>;

    #[derive(Grammar)]
    pub struct JSONString(pub Either<JSONObject, StringLiteral>);

    #[derive(Grammar)]
    pub enum ServerType {
        DB(StringEqCI!("'DB'")),
        FILE(StringEqCI!("'FILE'")),
    }

    #[derive(Grammar)]
    pub struct FDWName(pub Ident);

    pub type FTPushdownClause = Prefix<Kw!("PUSHDOWN"), (Space1, FTPushdownType)>;

    #[derive(Grammar)]
    pub enum FTPushdownType {
        Ansi92(Kw!("ANSI92")),
        Basic(Kw!("BASIC")),
        Extended(Kw!("EXTENDED")),
        Iris(Kw!("IRIS")),
        Off(Kw!("OFF")),
    }

    #[derive(Grammar)]
    pub struct QualifiedSPName(pub DottedIdent);

    #[derive(Grammar)]
    pub struct ClassName(pub DottedIdent);

    #[derive(Grammar)]
    pub enum TriggerTime {
        Before(Kw!("BEFORE")),
        After(Kw!("AFTER")),
    }

    #[derive(Grammar)]
    pub enum TriggerEvent {
        Insert(Kw!("INSERT")),
        Delete(Kw!("DELETE")),
        Update {
            kw: Kw!("UPDATE"),
            of: Option<(Space1, Kw!("OF"), Space, VecSep<Ident, ArgSep>)>,
        },
    }

    /// OLD|NEW [ROW|TABLE [AS]] alias: 2026.2 takes AS after ROW or TABLE alone.
    #[derive(Grammar)]
    pub struct TriggerReferencing {
        pub old_new: Either<Kw!("OLD"), Kw!("NEW")>,
        pub kind: Option<(
            Space1,
            Either<Kw!("ROW"), Kw!("TABLE")>,
            Option<(Space1, Kw!("AS"))>,
        )>,
        pub alias: (Space, Ident),
    }

    #[derive(Grammar)]
    pub enum TriggerOrientation {
        RowAndObject(Kw!("ROW_AND_OBJECT")),
        RowAndObjectWords(Kw!("ROW"), Space1, Kw!("AND"), Space1, Kw!("OBJECT")),
        Row(Kw!("ROW")),
        Statement(Kw!("STATEMENT")),
    }

    #[derive(Grammar)]
    pub enum TriggerBody {
        Code(CodeBody),
        Stat {
            lang: Option<(Kw!("LANGUAGE"), Space1, Kw!("SQL"), Space)>,
            stat: Box<Src<Stat>>,
        },
    }

    #[derive(Grammar)]
    pub enum LockSpec {
        Row(Kw!("ROW")),
        Page(Kw!("PAGE")),
    }

    #[derive(Grammar)]
    pub enum CascadeOpt {
        Restrict(Kw!("RESTRICT")),
        Cascade(Kw!("CASCADE")),
    }

    #[derive(Grammar)]
    pub enum DataOption {
        DelData(Kw!("%DELDATA")),
        NoDelData(Kw!("%NODELDATA")),
    }

    #[derive(Grammar)]
    pub struct MoveSpec {
        pub partition_range: Option<(PartitionRangeSpec, Space)>,
        pub to_kw: Kw!("TO"),
        pub database: (Space1, DatabaseName),
        pub activate: Option<(Space1, SQLActivateSpec)>,
    }

    #[derive(Grammar)]
    pub struct SQLActivateSpec {
        pub force_or_defer: Option<(Either<Kw!("FORCE"), Kw!("DEFER")>, Space1)>,
        pub activate_kw: Kw!("ACTIVATE"),
        pub mappings_kw: Option<(Space1, Kw!("MAPPINGS"))>,
    }

    #[derive(Grammar)]
    pub enum ViewWith {
        ReadOnly(Kw!("READ"), Space1, Kw!("ONLY")),
        LeveledCheck {
            levels: (LevelsClause, Space1),
            kw: (Kw!("CHECK"), Space1, Kw!("OPTION")),
        },
        Check {
            kw: (Kw!("CHECK"), Space1, Kw!("OPTION")),
            constraint: Option<(Space1, Kw!("CONSTRAINT"), Space, Ident)>,
        },
    }

    #[derive(Grammar)]
    pub enum LevelsClause {
        Cascaded(Spellings!(Kw!("CASCADED"), Kw!("CASCADE"))),
        Local(Kw!("LOCAL")),
    }

    #[derive(Grammar)]
    pub struct SQL {
        pub space_before: Space,
        pub view_column_list: Option<(FList1, Option<(Space, Kw!("AS"))>, Space)>,
        pub stat: Src<Stat>,
        pub space_after: Space,
    }

    #[derive(Grammar)]
    pub struct Thr {
        pub kw: (
            Kw!("THROUGH"),
            Space1,
            Option<(Kw!("FOREIGN"), Space1)>,
            Kw!("SERVER"),
            Space,
        ),
        pub tname: TName,
        pub remote_statement: (Space, Box<Src<Stat>>),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct WithStat<T: Grammar> {
        pub with_clause: Option<(WithClause, Space)>,
        pub body: T,
    }

    #[derive(Grammar)]
    pub enum Stat {
        Selct(Selct),
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
        Savepoint(Savepoint),
        Lock(LockTable),
        Call(CallStatement),
        Explain(Explain),
        Setml(Setml),
        ChkPrv(ChkPrv),
    }

    #[derive(Grammar)]
    #[tygr(validated)]
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
    pub struct DirectoryName(pub Literal);

    #[derive(Grammar)]
    pub struct DatabaseName(pub Ident);

    #[derive(Grammar)]
    pub struct CodeSpecification {
        pub params: Parenthesized<ParameterList>,
        pub characteristics: Vec<(Space, MethodQueryCharacteristics)>,
        pub body: (Space, CodeBody),
    }

    #[derive(Grammar)]
    pub struct ParameterList(pub Option<VecSep<ParameterDeclaration, ArgSep>>);

    #[derive(Grammar)]
    pub struct ParameterDeclaration {
        pub direction: Option<(ParameterDirection, Space)>,
        pub name: Ident,
        pub datyp: (Space, Datyp),
        pub default: Option<(Space, DefaultClause)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum ParameterDirection {
        In(Kw!("IN")),
        InOut(Kw!("INOUT")),
        Out(Kw!("OUT")),
    }

    #[derive(Grammar)]
    pub struct DefaultClause {
        pub default_kw: Option<(Kw!("DEFAULT"), Space)>,
        pub value: ParameterDefault,
    }

    #[derive(Grammar)]
    pub enum ParameterDefault {
        Literal(Option<Sign>, Literal),
        Null(Kw!("NULL")),
        Niladic(Niladic),
    }

    #[derive(Grammar)]
    pub enum MethodQueryCharacteristics {
        ContainId {
            kw: (Kw!("CONTAINID"), Space),
            value: Literal,
        },
        Final(Kw!("FINAL")),
        For {
            kw: (Kw!("FOR"), Space),
            classname: ClassName,
        },
        Private(Kw!("PRIVATE")),
        Proc(ProcedureKw),
        Results {
            kw: (Kw!("RESULTS"), Space),
            clause: ResultSetClause,
        },
        Returns {
            kw: (Kw!("RETURNS"), Space),
            datyp: Datyp,
        },
        SelectMode {
            kw: (Kw!("SELECTMODE"), Space1),
            mode: SelectMode,
        },
        DynamicResultSets {
            kw: (Kw!("DYNAMIC"), Space1, Kw!("RESULT"), Space1, Kw!("SETS")),
            count: Option<(Space1, Literal)>,
        },
        ResultSets(Kw!("RESULT"), Space1, Kw!("SETS")),
    }

    #[derive(Grammar)]
    pub struct ResultSetClause(pub Parenthesized<ColumnDefList>);

    #[derive(Grammar)]
    pub enum SelectMode {
        Display(Kw!("DISPLAY")),
        Logical(Kw!("LOGICAL")),
        Odbc(Kw!("ODBC")),
        Runtime(Kw!("RUNTIME")),
    }

    #[derive(Grammar)]
    pub enum CodeBody {
        Cos {
            kw: (
                Kw!("LANGUAGE"),
                Space1,
                Spellings!(Kw!("OBJECTSCRIPT"), Kw!("COS")),
                Space,
            ),
            body: Either<COSMethodBody, COSQueryBody>,
        },
        External {
            kw: (
                Kw!("LANGUAGE"),
                Space1,
                ExternalLanguage,
                Space1,
                Kw!("EXTERNAL"),
                Space1,
                Kw!("NAME"),
                Space,
            ),
            name: StringLiteral,
        },
        Python {
            kw: (Kw!("LANGUAGE"), Space1, Kw!("PYTHON"), Space),
            body: Prefix<StringEq!("{"), StringEndsWith<StringEq!("}")>>,
        },
        Sql {
            kw: Option<(Kw!("LANGUAGE"), Space1, Kw!("SQL"), Space1)>,
            body: SqlMethodBody,
        },
    }

    #[derive(Grammar)]
    pub enum ExternalLanguage {
        Java(Kw!("JAVA")),
        DotNet(Kw!("DOTNET")),
        Python(Kw!("PYTHON")),
    }

    pub type SqlMethodBody = Wrap<
        Kw!("BEGIN"),
        Vec1<(
            Space,
            Either<ReturnStatement, Src<Stat>>,
            Space,
            StringEq!(";"),
        )>,
        (Space, Kw!("END")),
    >;

    #[derive(Grammar)]
    pub struct ReturnStatement {
        pub kw: Kw!("RETURN"),
        pub value: (Space, ReturnValue),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum ReturnValue {
        Paren(Parenthesized<Box<ReturnValue>>),
        Literal(Option<Sign>, Literal),
        HostVar(HostVar),
        Marker(ParameterMarker),
    }

    #[derive(Grammar)]
    pub struct COSQueryBody(pub VecSep<QueryMethod, Space>);

    #[derive(Grammar)]
    pub struct QueryMethod {
        pub kind: QueryMethodKind,
        pub params: Args<ParameterList>,
        pub body: (Space, COSMethodBody),
    }

    #[derive(Grammar)]
    pub enum QueryMethodKind {
        Close(Kw!("CLOSE")),
        Execute(Kw!("EXECUTE")),
        FetchOdbc(Kw!("FETCHODBC")),
        FetchRows(Kw!("FETCHROWS")),
        Fetch(Kw!("FETCH")),
        GetInfo(Kw!("GETINFO")),
    }

    pub type COSMethodBody = crate::cos::Brace<crate::cos::OSText>;

    #[derive(Grammar)]
    pub struct IndexName(pub Ident);

    #[derive(Grammar)]
    pub enum IndexQualifiers {
        Bitmap(Kw!("BITMAP")),
        Bitslice(Kw!("BITSLICE")),
        Columnar(Kw!("COLUMNAR")),
        Unique { kw: Kw!("UNIQUE"), cluster: Cluster },
        Cluster(ClusterType),
    }

    #[derive(Grammar)]
    pub struct ComponentSpec {
        pub name: Ident,
        pub collection: Option<(Space, CollectionSpec)>,
        pub collate: Option<(Space, Kw!("COLLATE"), Space1, SqlCollation)>,
        pub direction: Option<(Space, ComponentDirection)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum ComponentDirection {
        Asc(Kw!("ASC")),
        Desc(Kw!("DESC")),
    }

    pub type CollectionSpec = Parenthesized<Either<Kw!("ELEMENTS"), Kw!("KEYS")>>;

    #[derive(Grammar)]
    pub struct IndexParameterSpec {
        pub name: Ident,
        pub eq: (Space, StringEq!("="), Space),
        pub value: Literal,
    }

    #[derive(Grammar)]
    pub enum AlterAction {
        CheckConstraintAll {
            check: CheckSpec,
            kw: (Space1, Kw!("CONSTRAINT"), Space1, Kw!("ALL")),
        },
        DropAllPartitions {
            drop_kw: DropKeywd,
            kw: (Space1, Kw!("ALL"), Space1, Kw!("PARTITIONS")),
        },
        DropPartition {
            drop_kw: DropKeywd,
            partition: (Space1, PartitionRangeSpec),
        },
        DropAction {
            drop_kw: DropKeywd,
            action: (Space, DropAction),
        },
        Add {
            kw: (Kw!("ADD"), Space),
            actions: Either<Parenthesized<AddActionList>, AddActionList>,
        },
        Alter {
            kw: Kw!("ALTER"),
            column_kw: Option<(Space1, Kw!("COLUMN"))>,
            name: (Space, Ident),
            alter: (Space, AlterCol),
        },
        Cleanup(Kw!("CLEANUP")),
        Convert {
            kw: (Kw!("CONVERT"), Space1),
            clause: ConvertClause,
        },
        LockMode {
            kw: (Kw!("LOCK"), Space1, Kw!("MODE"), Space),
            spec: Parenthesized<LockSpec>,
        },
        Modify {
            kw: (Kw!("MODIFY"), Space),
            specs: Either<Parenthesized<ModSpecList>, ModSpecList>,
        },
        Move {
            kw: (Kw!("MOVE"), Space1),
            spec: MoveSpec,
        },
        Rename {
            kw: (Kw!("RENAME"), Space),
            name: Ident,
        },
        Truncate {
            kw: (Kw!("TRUNCATE"), Space1),
            partition: PartitionRangeSpec,
        },
    }

    #[derive(Grammar)]
    pub enum ConvertClause {
        Partition {
            to: Option<(Kw!("TO"), Space1)>,
            clause: PartitionConvertClause,
        },
        StorageDefault {
            clause: StorageDefaultConvertClause,
            del: Option<(Space1, DataOption)>,
        },
        Shard {
            to: Option<(Kw!("TO"), Space1)>,
            clause: ShardConvertClause,
            timing: Option<(Space, ImmediateDefer)>,
        },
    }

    #[derive(Grammar)]
    pub enum ShardConvertClause {
        Sharded(Sharded),
        Shard(Shard),
        NonSharded(Kw!("NONSHARDED")),
    }

    #[derive(Grammar)]
    pub struct PartitionConvertClause {
        pub kw: Kw!("PARTITION"),
        pub spec: Option<(
            Space1,
            Either<Prefix<(Kw!("BY"), Space1), PartitionSpecificationList>, PartitionOnOff>,
        )>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum PartitionOnOff {
        On(Kw!("ON")),
        Off(Kw!("OFF")),
    }

    #[derive(Grammar)]
    pub enum AlterCol {
        DatypCollate {
            datyp: Datyp,
            collate: Option<(Space, Kw!("COLLATE"), Space1, SqlCollation)>,
        },
        SqlCollation(SqlCollation),
        Collate {
            kw: (Kw!("COLLATE"), Space),
            name: SqlCollation,
        },
        Default {
            kw: (Kw!("DEFAULT"), Space),
            value: Setdef,
        },
        DropDefault(Kw!("DROP"), Space1, Kw!("DEFAULT")),
        NotNull(Kw!("NOT"), Space1, Kw!("NULL")),
        Null(Kw!("NULL")),
        Rename {
            kw: (Kw!("RENAME"), Space),
            name: Ident,
        },
        SetDefault {
            kw: (Kw!("SET"), Space1, Kw!("DEFAULT"), Space),
            value: Setdef,
        },
    }

    #[derive(Grammar)]
    pub enum StorageDefaultConvertClause {
        Column(ColumnStorageDefaultConversion),
        Type(StorageDefaultConversionTypeSpec),
    }

    #[derive(Grammar)]
    pub struct StorageDefaultConversionTypeSpec {
        pub to: Option<(Kw!("TO"), Space1)>,
        pub kw: (Kw!("STORAGETYPE"), Space, StringEq!("="), Space),
        pub typ: Storagetype,
    }

    #[derive(Grammar)]
    pub struct ColumnStorageDefaultConversion {
        pub kw: Spellings!(Kw!("COLUMNS"), Kw!("COLUMN")),
        pub list: (Space, VecSep<Ident, ArgSep>),
        pub spec: (Space, StorageDefaultConversionTypeSpec),
    }

    /// A DEFAULT value, in parentheses to any depth (2026.2 takes ((1))).
    #[derive(Grammar)]
    pub enum Setdef {
        Spec(DefSpec),
        Paren(Parenthesized<Box<Setdef>>),
    }

    #[derive(Grammar)]
    pub enum DefSpec {
        Cos {
            kw: (Kw!("%COS"), Space),
            value: Literal,
        },
        MCode {
            kw: (Kw!("%MCODE"), Space),
            value: Literal,
        },
        Negative {
            minus: (StringEq!("-"), Space),
            value: Number,
        },
        Literal(Literal),
        CurrentDate(Kw!("CURRENT_DATE")),
        CurrentTimestamp {
            kw: Kw!("CURRENT_TIMESTAMP"),
            precision: Option<(Space, Parenthesized<Precision>)>,
        },
        CurrentTime {
            kw: Kw!("CURRENT_TIME"),
            precision: Option<(Space, Parenthesized<Precision>)>,
        },
        CurrentUser(Kw!("CURRENT_USER")),
        GetDate {
            kw: Kw!("GETDATE"),
            prec: Args<Option<Precision>>,
        },
        GetUtcDate {
            kw: Kw!("GETUTCDATE"),
            prec: Args<Option<Precision>>,
        },
        Null(Kw!("NULL")),
        ObjectScript {
            kw: (Kw!("OBJECTSCRIPT"), Space),
            value: Literal,
        },
        SessionUser(Kw!("SESSION_USER")),
        SysDate(Kw!("SYSDATE")),
        SystemUser(Kw!("SYSTEM_USER")),
        ToDate(Kw!("TO_DATE"), Args<(Literal, Option<(ArgSep, Literal)>)>),
        ToTimestamp(
            Kw!("TO_TIMESTAMP"),
            Args<(Literal, Option<(ArgSep, Literal)>)>,
        ),
        User(Kw!("USER")),
    }

    #[derive(Grammar)]
    #[tygr(validated)]
    pub struct ModSpec {
        pub name: Ident,
        pub change: (Space, ModChange),
    }

    impl Validate for ModSpec {
        const REQUIREMENT: &'static str = "give a type or a change";
        fn validate(&self) -> bool {
            !matches!(
                &self.change.1,
                ModChange::Modify {
                    datyp: None,
                    mod_col: ModCol {
                        default: None,
                        constraint: None,
                        null: None
                    },
                }
            )
        }
    }

    #[derive(Grammar)]
    pub enum ModChange {
        Rename {
            kw: (Kw!("RENAME"), Space),
            new_name: Ident,
        },
        Modify {
            datyp: Option<(Datyp, Space)>,
            mod_col: ModCol,
        },
    }

    pub type ModSpecList = VecSep<ModSpec, ArgSep>;

    #[derive(Grammar)]
    pub struct ModCol {
        pub default: Option<(Kw!("DEFAULT"), Space, Setdef, Space)>,
        pub constraint: Option<(Kw!("CONSTRAINT"), Space, Ident, Space)>,
        pub null: Option<NullSpec>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct NullSpec {
        pub not: Option<(Kw!("NOT"), Space1)>,
        pub null: Kw!("NULL"),
    }

    pub type DropKeywd = Spellings!(Kw!("DROP"), Kw!("DELETE"));

    #[derive(Grammar)]
    pub enum DropAction {
        Constraint {
            kw: (Kw!("CONSTRAINT"), Space),
            name: Ident,
        },
        ForeignKey {
            kw: (Kw!("FOREIGN"), Space1, Kw!("KEY"), Space),
            name: Ident,
        },
        PrimaryKey(Kw!("PRIMARY"), Space1, Kw!("KEY")),
        Column {
            column_kw: Option<(Kw!("COLUMN"), Space)>,
            items: VecSep<DropColumnItem, ArgSep>,
        },
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct DropColumnItem {
        pub name: Ident,
        pub cascade: Option<(Space, CascadeOpt)>,
        pub data: Option<(Space, DataOption)>,
    }

    #[derive(Grammar)]
    pub struct AddAction {
        pub constraint: Option<(Kw!("CONSTRAINT"), Space, TName, Space)>,
        pub body: AddBody,
    }

    pub type AddActionList = VecSep<AddAction, ArgSep>;

    #[derive(Grammar)]
    pub enum AddBody {
        ForeignKey(OptionallyWrapped<AltFkeySpec>),
        PrimaryKey {
            spec: OptionallyWrapped<AltPkey>,
            on: Option<(Space, Kw!("ON"), Space, Token)>,
        },
        Unique {
            spec: OptionallyWrapped<AltUnique>,
            fillfactor: FillFactor,
            on: Option<(Space, Kw!("ON"), Space, Token)>,
        },
        ColDef(OptionallyWrapped<Coldef>),
        Index(TblIndex),
        Default {
            kw: (Kw!("DEFAULT"), Space),
            value: Setdef,
            for_kw: (Space, Kw!("FOR"), Space),
            name: Ident,
        },
    }

    pub type Coldef = Prefix<Option<(Kw!("COLUMN"), Space)>, OptionallyWrapped<Tecdef>>;

    #[derive(Grammar)]
    pub enum CheckSpec {
        Check(Kw!("CHECK")),
        NoCheck(Kw!("NOCHECK")),
    }

    #[derive(Grammar)]
    pub struct AltPkey {
        pub kw: (Kw!("PRIMARY"), Space1, Kw!("KEY"), Space),
        pub spec: FList2,
    }

    /// FOREIGN KEY [name] (columns) REFERENCES ...: 2026.2 takes the name in ALTER TABLE ADD, not in CREATE TABLE.
    #[derive(Grammar)]
    pub struct AltFkeySpec {
        pub kw: (Kw!("FOREIGN"), Space1, Kw!("KEY"), Space),
        pub flist2: FList2,
        pub references_kw: (Space, Kw!("REFERENCES"), Space),
        pub ref_: Ref,
    }

    #[derive(Grammar)]
    pub struct TblForeign {
        pub kw: (Kw!("FOREIGN"), Space1, Kw!("KEY"), Space),
        pub columns: TblKeyColumns,
        pub references_kw: (Space, Kw!("REFERENCES"), Space),
        pub ref_: Ref,
    }

    /// `[CONSTRAINT name] INDEX|KEY name (components)`, a table element and an ALTER TABLE ADD action on 2026.2.
    #[derive(Grammar)]
    pub struct TblIndex {
        pub kw: (Spellings!(Kw!("INDEX"), Kw!("KEY")), Space),
        pub name: DottedIdent,
        pub components: Args<VecSep<ComponentSpec, ArgSep>>,
    }

    #[derive(Grammar)]
    pub struct AltUnique {
        pub kw: Kw!("UNIQUE"),
        pub spec: UniqueSpec,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct UniqueSpec {
        pub cluster: Cluster,
        pub flist: (Space, FList2),
    }

    #[derive(Grammar)]
    pub enum InOn {
        In(Kw!("IN")),
        On(Kw!("ON")),
    }

    #[derive(Grammar)]
    pub struct Token(pub Either<Ident, Literal>);

    #[derive(Grammar)]
    pub struct Telem {
        pub constraint: Option<(Kw!("CONSTRAINT"), Space, Ident, Space)>,
        pub option: TelemOption,
    }

    #[derive(Grammar)]
    pub enum TelemOption {
        Description {
            kw: (Kw!("%DESCRIPTION"), Space),
            value: Literal,
        },
        File {
            kw: (Kw!("%FILE"), Space),
            value: Literal,
        },
        PublicRowId(Kw!("%PUBLICROWID")),
        Routine {
            kw: (Kw!("%ROUTINE"), Space),
            value: Literal,
        },
        ClassParameter(ClassParameter),
        ExtentsizeClause(ExtentsizeClause),
        Shard(Shard),
        TblForeign(TblForeign),
        TblPrimaryKey(TblPrimaryKey),
        TblUnique(TblUnique),
        Index(TblIndex),
        Tecdef(Tecdef),
    }

    #[derive(Grammar)]
    pub struct Tecdef {
        pub name: Ident,
        pub typ: (Space, Tectyp),
        pub qualifiers: Vec<(Space, FldQualifier)>,
    }

    #[derive(Grammar)]
    pub enum Tectyp {
        Datyp(Datyp),
        Identity {
            datyp: Option<Datyp>,
            identity: (Space, Identity),
        },
    }

    pub type Identity = Prefix<Kw!("IDENTITY"), Option<Args<(Literal, ArgSep, Literal)>>>;

    pub type TblUnique = Prefix<Kw!("UNIQUE"), TblUniqueSpec>;

    #[derive(Grammar)]
    pub struct TblUniqueSpec {
        pub cluster: Cluster,
        pub columns: (Space, TblKeyColumns),
    }

    #[derive(Grammar)]
    pub enum TblKeyColumns {
        Parens(FList1),
        Bare(VecSep<Ident, ArgSep>),
    }

    pub type FillFactor = Option<(
        Space,
        Kw!("WITH"),
        Space1,
        Kw!("FILLFACTOR"),
        Space,
        StringEq!("="),
        Space,
        Literal,
    )>;

    pub type TblPrimaryKey = Prefix<(Kw!("PRIMARY"), Space1, Kw!("KEY"), Space), TblUniqueSpec>;

    #[derive(Grammar)]
    pub struct ExtentsizeClause {
        pub kw: (Spellings!(Kw!("%EXTENTSIZE"), Kw!("%NUMROWS")), Space),
        pub value: Literal,
    }

    #[derive(Grammar)]
    pub struct ClassParameter {
        pub kw: (Kw!("%CLASSPARAMETER"), Space),
        pub name: Ident,
        #[sugar]
        pub eq: Option<(Space, StringEq!("="))>,
        pub value: (Space, Literal),
    }

    pub type Shard = Prefix<Kw!("SHARD"), ShardSpec>;
    pub type Sharded = Prefix<Kw!("SHARDED"), ShardSpec>;

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ShardSpec {
        pub key: Option<(
            Space,
            Kw!("KEY"),
            Space,
            FList1,
            Option<(Space, CoshardClause)>,
        )>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct CoshardClause {
        pub kw: Kw!("COSHARD"),
        pub with: Option<(Space1, Kw!("WITH"))>,
        pub name: Either<Args<TName>, (Space, TName)>,
    }

    #[derive(Grammar)]
    pub enum TableOptionList {
        WithOptions {
            shard: Option<(Shard, Space)>,
            partition: Option<(PartitionOption, Space)>,
            with_kw: (Kw!("WITH"), Space1),
            options: VecSep<TableOption, ArgSep>,
            informix: Option<(Space, InformixCompatability)>,
        },
        Shard {
            shard: Shard,
            partition: Option<(Space, PartitionOption)>,
        },
        Partition(PartitionOption),
        InformixCompat(InformixCompatability),
    }

    #[derive(Grammar)]
    pub struct InformixCompatability {
        pub in_on: Option<(InOn, Space, Token)>,
        pub extent_size: Option<(Space, Kw!("EXTENT"), Space1, Kw!("SIZE"), Space, Literal)>,
        pub next_size: Option<(Space, Kw!("NEXT"), Space1, Kw!("SIZE"), Space, Literal)>,
    }

    #[derive(Grammar)]
    pub enum TableOption {
        ClassParam(ClassParameter),
        StorageType {
            kw: Kw!("STORAGETYPE"),
            #[sugar]
            eq: Option<(Space, StringEq!("="))>,
            value: (Space, Storagetype),
        },
    }

    /// PARTITION [BY ... | OFF] as a table option; 2026.2 refuses PARTITION ON here and takes it after CONVERT.
    #[derive(Grammar)]
    pub struct PartitionOption {
        pub kw: Kw!("PARTITION"),
        pub spec: Option<(
            Space1,
            Either<Prefix<(Kw!("BY"), Space1), PartitionSpecificationList>, Kw!("OFF")>,
        )>,
    }

    #[derive(Grammar)]
    pub struct PartitionSpecificationList(pub VecSep<PartitionSpecification, ArgSep>);

    #[derive(Grammar)]
    pub enum PartitionSpecification {
        Hash {
            kw: (Kw!("HASH"), Space),
            column: Parenthesized<ColumnName>,
            in_kw: (Space, Kw!("IN"), Space),
            count: Literal,
            partitions_kw: (Space, Kw!("PARTITIONS")),
        },
        List {
            kw: (Kw!("LIST"), Space),
            column: Parenthesized<ColumnName>,
        },
        Range {
            kw: (Kw!("RANGE"), Space),
            column: Parenthesized<ColumnName>,
            interval_kw: (Space, Kw!("INTERVAL"), Space),
            interval: Literal,
            datetime_interval: Option<(Space, PartitionDatetimeInterval)>,
        },
    }

    #[derive(Grammar)]
    pub enum PartitionDatetimeInterval {
        Day(Spellings!(Kw!("DAY"), Kw!("DAYS"))),
        Hour(Spellings!(Kw!("HOUR"), Kw!("HOURS"))),
        Minute(Spellings!(Kw!("MINUTE"), Kw!("MINUTES"))),
        Month(Spellings!(Kw!("MONTH"), Kw!("MONTHS"))),
        Year(Spellings!(Kw!("YEAR"), Kw!("YEARS"))),
    }

    #[derive(Grammar)]
    pub struct PartitionRangeSpec {
        pub kw: Kw!("PARTITION"),
        pub range: PartitionRange,
    }

    #[derive(Grammar)]
    pub enum PartitionRange {
        Id {
            kw: (Space, Kw!("ID"), Space),
            prefix: Literal,
        },
        IdBetween {
            kw: (Space, Kw!("ID")),
            prefix: Option<(Space, Literal)>,
            between_kw: (Space, Kw!("BETWEEN"), Space),
            from: Literal,
            and_kw: (Space, Kw!("AND"), Space),
            to: Literal,
        },
        Between {
            prefix: Option<(Space, PartitionPrefix)>,
            between_kw: (Space, Kw!("BETWEEN"), Space),
            from: Literal,
            and_kw: (Space, Kw!("AND"), Space),
            to: Literal,
        },
        Values(Space, PartitionPrefix),
    }

    pub type PartitionPrefix = Parenthesized<VecSep<Literal, ArgSep>>;

    #[derive(Grammar)]
    pub enum FldQualifier {
        Description {
            kw: (Kw!("%DESCRIPTION"), Space),
            value: Literal,
        },
        Fcon(Fcon),
        SqlCollation(SqlCollation),
        Calculated(Kw!("CALCULATED")),
        CharacterSet {
            kw: (Kw!("CHARACTER"), Space1, Kw!("SET"), Space),
            name: Ident,
        },
        CollateIdent {
            kw: (Kw!("COLLATE"), Space),
            value: SqlCollation,
        },
        ComputeCode {
            kw: (Kw!("COMPUTECODE"), Space),
            body: COSMethodBody,
        },
        ComputeOnChange {
            kw: (Kw!("COMPUTEONCHANGE"), Space),
            list: FList1,
        },
        Default {
            kw: (Kw!("DEFAULT"), Space),
            value: Setdef,
        },
        OnUpdate {
            kw: (Kw!("ON"), Space1, Kw!("UPDATE"), Space),
            value: UpdateSpec,
        },
        Private(Kw!("PRIVATE")),
        StorageType {
            kw: (
                Kw!("WITH"),
                Space1,
                Kw!("STORAGETYPE"),
                Space,
                StringEq!("="),
                Space,
            ),
            value: Storagetype,
        },
        AutoIncrement(Kw!("AUTO_INCREMENT")),
        Transient(Kw!("TRANSIENT")),
    }

    #[derive(Grammar)]
    pub enum SqlCollation {
        PctAlphaup(Kw!("%ALPHAUP")),
        PctExact(Kw!("%EXACT")),
        PctMinus(Kw!("%MINUS")),
        PctMvr(Kw!("%MVR")),
        PctPlus(Kw!("%PLUS")),
        PctSpace(Kw!("%SPACE")),
        PctSqlstring {
            kw: Kw!("%SQLSTRING"),
            arg: Option<Args<Literal>>,
        },
        PctSqupper {
            kw: Kw!("%SQLUPPER"),
            arg: Option<Args<Literal>>,
        },
        PctString {
            kw: Kw!("%STRING"),
            arg: Option<Args<Literal>>,
        },
        PctTruncate {
            kw: Kw!("%TRUNCATE"),
            arg: Option<Args<Literal>>,
        },
        PctUpper(Kw!("%UPPER")),
        Alphaup(Kw!("ALPHAUP")),
        Exact(Kw!("EXACT")),
        Minus(Kw!("MINUS")),
        Mvr(Kw!("MVR")),
        Plus(Kw!("PLUS")),
        Space(Kw!("SPACE")),
        Sqlstring {
            kw: Kw!("SQLSTRING"),
            arg: Option<Args<Literal>>,
        },
        Sqlupper {
            kw: Kw!("SQLUPPER"),
            arg: Option<Args<Literal>>,
        },
        StringColl {
            kw: Kw!("STRING"),
            arg: Option<Args<Literal>>,
        },
        Truncate {
            kw: Kw!("TRUNCATE"),
            arg: Option<Args<Literal>>,
        },
        Upper(Kw!("UPPER")),
    }

    #[derive(Grammar)]
    pub enum Storagetype {
        Columnar(Kw!("COLUMNAR")),
        Row(Kw!("ROW")),
    }

    #[derive(Grammar)]
    pub struct Datyp {
        pub spec: DatypSpec,
        pub args: Option<Args<DatypArgs>>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct DatypArgs(pub Vec<Either<Parenthesized<Box<DatypArgs>>, StringOf1<IsNotParen>>>);

    #[derive(Grammar)]
    pub enum DatypSpec {
        HashParen(Parenthesized<Vec<(StringEq!("#"), Space)>>),
        Binary {
            kw: Kw!("BINARY"),
            varying: Option<(Space1, Kw!("VARYING"))>,
        },
        Bit(Kw!("BIT")),
        Character {
            kw: Kw!("CHARACTER"),
            varying: Option<(Space1, Kw!("VARYING"))>,
        },
        Char {
            kw: Kw!("CHAR"),
            varying: Option<(Space1, Kw!("VARYING"))>,
        },
        Datetime(Kw!("DATETIME")),
        Date(Kw!("DATE")),
        Decimal(Kw!("DECIMAL")),
        Dec(Kw!("DEC")),
        Double {
            kw: Kw!("DOUBLE"),
            precision: Option<(Space1, Kw!("PRECISION"))>,
        },
        Float(Kw!("FLOAT")),
        Image(Kw!("IMAGE")),
        Integer(Kw!("INTEGER")),
        Int(Kw!("INT")),
        LongVarchar(Kw!("LONG"), Space1, Kw!("VARCHAR")),
        Longvarbinary(Kw!("LONGVARBINARY")),
        Longvarchar(Kw!("LONGVARCHAR")),
        Long(Kw!("LONG")),
        Money(Kw!("MONEY")),
        NationalVarchar(Kw!("NATIONAL"), Space1, Kw!("VARCHAR")),
        NationalChar {
            kw: Kw!("NATIONAL"),
            char_kw: Option<(
                Space1,
                Spellings!(Kw!("CHARACTER"), Kw!("CHAR")),
                Option<(Space1, Kw!("VARYING"))>,
            )>,
        },
        Nchar(Kw!("NCHAR")),
        Ntext(Kw!("NTEXT")),
        Number(Kw!("NUMBER")),
        Real(Kw!("REAL")),
        Smallint(Kw!("SMALLINT")),
        Smallmoney(Kw!("SMALLMONEY")),
        Text(Kw!("TEXT")),
        Timestamp(Kw!("TIMESTAMP")),
        Time(Kw!("TIME")),
        Tinyint(Kw!("TINYINT")),
        Varbinary(Kw!("VARBINARY")),
        Varchar2(Kw!("VARCHAR2")),
        Varchar(Kw!("VARCHAR")),
        Vector(Kw!("VECTOR")),
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
    pub enum Fcon {
        Constraint {
            kw: (Kw!("CONSTRAINT"), Space),
            name: TName,
            spec: (Space, NamedFconSpec),
        },
        Identity {
            kw: Kw!("IDENTITY"),
            args: Option<Args<(Literal, Option<(ArgSep, Literal)>)>>,
        },
        NotNull(Kw!("NOT"), Space1, Kw!("NULL")),
        Null(Kw!("NULL")),
        Primary {
            kw: (Kw!("PRIMARY"), Option<(Space1, Kw!("KEY"))>),
            cluster: Cluster,
        },
        References {
            kw: (Kw!("REFERENCES"), Space1),
            reference: Ref,
        },
        Unique {
            kw: Kw!("UNIQUE"),
            cluster: Cluster,
        },
    }

    #[derive(Grammar)]
    pub enum NamedFconSpec {
        Default {
            kw: (Kw!("DEFAULT"), Space),
            value: Setdef,
        },
        NotNull(Kw!("NOT"), Space1, Kw!("NULL")),
        PrimaryKey {
            kw: (Kw!("PRIMARY"), Space1, Kw!("KEY")),
            cluster: Cluster,
        },
        References {
            kw: (Kw!("REFERENCES"), Space1),
            reference: Ref,
        },
        Unique {
            kw: Kw!("UNIQUE"),
            cluster: Cluster,
        },
    }

    #[derive(Grammar)]
    pub enum UpdateSpec {
        Negative {
            minus: (StringEq!("-"), Space),
            value: Number,
        },
        Literal(Literal),
        CurrentDate(Kw!("CURRENT_DATE")),
        CurrentTimestamp {
            kw: Kw!("CURRENT_TIMESTAMP"),
            precision: Option<(Space, Parenthesized<Precision>)>,
        },
        CurrentTime {
            kw: Kw!("CURRENT_TIME"),
            precision: Option<(Space, Parenthesized<Precision>)>,
        },
        CurrentUser(Kw!("CURRENT_USER")),
        GetDate {
            kw: Kw!("GETDATE"),
            prec: Parenthesized<Option<Precision>>,
        },
        GetUtcDate {
            kw: Kw!("GETUTCDATE"),
            prec: Parenthesized<Option<Precision>>,
        },
        Null(Kw!("NULL")),
        SessionUser(Kw!("SESSION_USER")),
        SysDate(Kw!("SYSDATE")),
        SystemUser(Kw!("SYSTEM_USER")),
        User(Kw!("USER")),
    }

    pub type Cluster = Option<(Space1, ClusterType)>;

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum ClusterType {
        Clustered(Kw!("CLUSTERED")),
        NonClustered(Kw!("NONCLUSTERED")),
    }

    #[derive(Grammar)]
    pub struct Ref {
        pub name: TName,
        pub flist: Option<(Space, FList1)>,
        pub match_spec: Option<(Space, Kw!("MATCH"), Space1, MatchSpec)>,
        pub action: Option<(Space, Either<TriggeredAction, Kw!("NOCHECK")>)>,
    }

    #[derive(Grammar)]
    #[tygr(validated)]
    pub struct TriggeredAction {
        pub first: RefRule,
        pub second: Option<(Space1, RefRule, Option<(Space1, Kw!("NOCHECK"))>)>,
    }

    impl Validate for TriggeredAction {
        const REQUIREMENT: &'static str = "name ON UPDATE and ON DELETE once each";
        fn validate(&self) -> bool {
            let event = |rule: &RefRule| matches!(rule.prefix.2, RefEvent::Update(_));
            (self.second.as_ref()).is_none_or(|(_, rule, _)| event(rule) != event(&self.first))
        }
    }

    pub type RefRule = Prefix<(Kw!("ON"), Space1, RefEvent, Space1), RefAction>;

    #[derive(Grammar)]
    pub enum RefEvent {
        Update(Kw!("UPDATE")),
        Delete(Kw!("DELETE")),
    }

    #[derive(Grammar)]
    pub enum MatchSpec {
        Full(Kw!("FULL")),
        Partial(Kw!("PARTIAL")),
    }

    #[derive(Grammar)]
    pub enum RefAction {
        Cascade(Kw!("CASCADE")),
        NoAction(Kw!("NO"), Space1, Kw!("ACTION")),
        SetDefault(Kw!("SET"), Space1, Kw!("DEFAULT")),
        SetNull(Kw!("SET"), Space1, Kw!("NULL")),
    }

    #[derive(Grammar)]
    pub enum FList2 {
        Named { name: Ident, flist: (Space, FList1) },
        Columns(TblKeyColumns),
    }

    #[derive(Grammar)]
    pub enum LoadSpec {
        Sql {
            kw: (Kw!("SQL"), Space1),
            spec: LoadSqlSpec,
        },
        Data {
            hints: Option<LoadHints>,
            kw: (Kw!("DATA"), Space1),
            spec: LoadDataSpec,
        },
    }

    #[derive(Grammar)]
    pub enum LoadHints {
        Bulk(
            Kw!("BULK"),
            Space1,
            Option<(Kw!("%NOINDEX"), Space1)>,
            Option<(Kw!("%NOJOURN"), Space1)>,
        ),
        Set(LoadHintSet),
    }

    /// The % hints in any order, each once: 2026.2 rejects %NOJOURN %NOJOURN.
    #[derive(Grammar)]
    #[tygr(validated)]
    pub struct LoadHintSet(pub Vec1<(LoadHint, Space1)>);

    impl Validate for LoadHintSet {
        const REQUIREMENT: &'static str = "give each hint once";
        fn validate(&self) -> bool {
            let names: Vec<String> = self
                .0
                .iter()
                .map(|(hint, _)| hint.print().to_uppercase())
                .collect();
            names
                .iter()
                .enumerate()
                .all(|(i, name)| !names[..i].contains(name))
        }
    }

    #[derive(Grammar)]
    pub enum LoadHint {
        NoCheck(Kw!("%NOCHECK")),
        NoIndex(Kw!("%NOINDEX")),
        NoJourn(Kw!("%NOJOURN")),
        NoLock(Kw!("%NOLOCK")),
    }

    #[derive(Grammar)]
    pub enum LoadDataSpec {
        Inline(LoadDataSpecInline),
        WithUsing(LoadDataSpecWithUsing),
    }

    #[derive(Grammar)]
    pub struct LoadDataSpecInline {
        pub from: LoadDataFrom,
        pub into: (Space, LoadDataInto),
        pub using: Option<(Space, LoadDataUsing)>,
    }

    #[derive(Grammar)]
    pub struct LoadDataSpecWithUsing {
        pub from: Option<(LoadDataFrom, Space)>,
        pub into: Option<(LoadDataInto, Space)>,
        pub using: LoadDataUsing,
    }

    pub type LoadDataFrom = Prefix<(Kw!("FROM"), Space1), DataSource>;

    #[derive(Grammar)]
    pub enum DataSource {
        File(FileSource),
        Jdbc {
            kw: (Kw!("JDBC"), Space),
            cnxn: JdbcCnxnSpec,
            data: (Space, JdbcDataSpec),
        },
    }

    pub type ColumnDefList = VecSep<ColumnDefItem, ArgSep>;

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct FileSource {
        pub kw: (Kw!("FILE"), Space),
        pub path: Literal,
        pub columns: Option<(Space, Kw!("COLUMNS"), Space, Parenthesized<ColumnDefList>)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ColumnDefItem {
        pub name: Ident,
        pub datyp: (Space, Datyp),
    }

    #[derive(Grammar)]
    pub enum JdbcCnxnSpec {
        Connection {
            kw: Option<(Kw!("CONNECTION"), Space)>,
            spec: CnxnSpec,
        },
        Url {
            kw: Option<(Kw!("URL"), Space)>,
            value: Literal,
        },
    }

    #[derive(Grammar)]
    pub enum JdbcDataSpec {
        Query {
            kw: (Kw!("QUERY"), Space),
            value: Literal,
        },
        Table {
            kw: (Kw!("TABLE"), Space),
            name: VecSep<Ident, StringEq!(".")>,
        },
    }

    #[derive(Grammar)]
    pub enum CnxnSpec {
        Ident(Ident),
        Literal(Literal),
    }

    #[derive(Grammar)]
    pub struct LoadDataInto {
        pub kw: (Kw!("INTO"), Space),
        pub table: TName,
        pub columns: Option<(Space, FList1)>,
        pub values: Option<(Space, Kw!("VALUES"), Space, FList1)>,
    }

    pub type LoadDataUsing = Prefix<(Kw!("USING"), Space), LoadOptions>;

    #[derive(Grammar)]
    pub enum LoadOptions {
        Json(JSONString),
        Name(Ident),
    }

    #[derive(Grammar)]
    pub struct LoadSqlSpec {
        pub kw: (Kw!("FROM"), Space1),
        pub source: SqlSource,
        pub dialect: Option<(Space, Kw!("DIALECT"), Space, Literal)>,
        pub delimiter: Option<(Space, Kw!("DELIMITER"), Space, Literal)>,
        pub verbose: Option<(Space, Kw!("VERBOSE"))>,
        pub log_to_file: Option<(Space, LoadSqlLogToFile)>,
        pub ignore_errors: Option<(Space, Kw!("IGNORE"), Space1, Kw!("ERRORS"))>,
    }

    #[derive(Grammar)]
    pub enum SqlSource {
        Directory {
            kw: (Kw!("DIRECTORY"), Space),
            path: Literal,
            recurse: Option<(Space, Kw!("RECURSE"))>,
        },
        File {
            kw: (Kw!("FILE"), Space),
            path: Literal,
        },
    }

    pub type LoadSqlLogToFile =
        Prefix<(Kw!("LOG"), Space1, Kw!("TO"), Space1, Kw!("FILE")), Option<(Space, Literal)>>;

    #[derive(Grammar)]
    pub struct AlterServerAction {
        pub kw: (Spellings!(Kw!("ALTER"), Kw!("MODIFY")), Space1),
        pub action: SubAlterServerAction,
    }

    #[derive(Grammar)]
    pub enum SubAlterServerAction {
        FtPushdown(FTPushdownClause),
        Connection {
            kw: (Kw!("CONNECTION"), Space),
            value: Literal,
        },
        DelimitedIds(Kw!("DELIMITEDIDS")),
        Host {
            kw: (Kw!("HOST"), Space),
            value: Literal,
        },
        NoDelimitedIds(Kw!("NODELIMITEDIDS")),
    }

    #[derive(Grammar)]
    pub enum VectorDatyp {
        Decimal(Kw!("DECIMAL")),
        Double(Kw!("DOUBLE")),
        Float(Kw!("FLOAT")),
        Integer(Spellings!(Kw!("INTEGER"), Kw!("INT"))),
        String(Kw!("STRING")),
    }

    pub type StatsClause = Spellings!(Kw!("STATISTICS"), Kw!("STATS"));

    #[derive(Grammar)]
    pub enum StatsScope {
        Schema {
            kw: (Kw!("SCHEMA"), Space),
            schema: Schema,
        },
        Table {
            kw: (Kw!("TABLE"), Space),
            table: TName,
        },
    }

    #[derive(Grammar)]
    pub enum StatsProperty {
        IgnoreFixedStats(Kw!("IGNOREFIXEDSTATS")),
        SkipAutomaticStatsCollection(Kw!("SKIPAUTOMATICSTATSCOLLECTION")),
    }
}

/// The spellings only the full level has, which `leveled` imports for its sugar variants.
#[allow(clippy::upper_case_acronyms)]
mod sugar {
    use either::Either;

    use super::*;

    #[derive(Grammar, Clone)]
    pub enum PercentOp {
        AlphaUp(Kw!("%ALPHAUP")),
        Exact(Kw!("%EXACT")),
        External(Kw!("%EXTERNAL")),
        Internal(Kw!("%INTERNAL")),
        IntExt(Kw!("%INTEXT")),
        MinusOp(Kw!("%MINUS")),
        Mvr(Kw!("%MVR")),
        OdbcIn(Kw!("%ODBCIN")),
        OdbcOut(Kw!("%ODBCOUT")),
        PlusOp(Kw!("%PLUS")),
        RuntimeIn(Kw!("%RUNTIMEIN")),
        RuntimeOut(Kw!("%RUNTIMEOUT")),
        Upper(Either<Kw!("%UPPER"), Kw!("UPPER")>),
        StringOperator(StringOperator),
    }

    #[derive(Grammar, Clone)]
    pub struct StringOperation {
        pub op: StringOperator,
        pub args: Args<(Scalx, ArgSep, ColTrun)>,
    }

    #[derive(Grammar, Clone)]
    pub enum StringOperator {
        String(Kw!("%STRING")),
        SqlString(Kw!("%SQLSTRING")),
        SqlUpper(Kw!("%SQLUPPER")),
        Truncate(Kw!("%TRUNCATE")),
    }

    #[derive(Grammar, Clone)]
    pub enum ColTrun {
        Const(PConst),
        HostVar(HostVar),
    }

    /// The functions a call by name spells, which the core reads as calls, the ObjectScript `$` functions
    /// among them (2026.2 takes these and no other `$` name).
    #[derive(Grammar, Clone)]
    pub enum ScalarFunction {
        PercentObject(Kw!("%OBJECT"), Args<Scalx>),
        PercentOid(Kw!("%OID"), Args<Scalx>),
        Similarity(Kw!("%SIMILARITY"), Args<RR>),
        Abs(Kw!("ABS"), Args<Scalx>),
        Ascii(Kw!("ASCII"), Args<Scalx>),
        Ceiling(Kw!("CEILING"), Args<Scalx>),
        CharacterLength(Kw!("CHARACTER_LENGTH"), Args<Scalx>),
        CharIndex(Kw!("CHARINDEX"), Args<RRO>),
        CharLength(Kw!("CHAR_LENGTH"), Args<Scalx>),
        Char(Kw!("CHAR"), Args<Scalx>),
        Coalesce(Kw!("COALESCE"), Args1Plus),
        Concat(Kw!("CONCAT"), Args<RR>),
        Convert(Kw!("CONVERT"), Args<RRO>),
        DataLength(Kw!("DATALENGTH"), Args<Scalx>),
        Date(Kw!("DATE"), Args<Scalx>),
        Day(Kw!("DAY"), Args<Scalx>),
        Decode(Kw!("DECODE"), Args1Plus),
        Embedding(Kw!("EMBEDDING"), Args<RO>),
        Floor(Kw!("FLOOR"), Args<Scalx>),
        GetDate(Kw!("GETDATE"), Args<Option<Scalx>>),
        GetUtcDate(Kw!("GETUTCDATE"), Args<Option<Scalx>>),
        Greatest(Kw!("GREATEST"), Args1Plus),
        HashBytes(Kw!("HASHBYTES"), Args<RR>),
        IfNull(Kw!("IFNULL"), Args<RRO>),
        Instr(Kw!("INSTR"), Args<RROO>),
        IsNull(Kw!("ISNULL"), Args<RR>),
        IsNumeric(Kw!("ISNUMERIC"), Args<Scalx>),
        LastDay(Kw!("LAST_DAY"), Args<Scalx>),
        LastIdentity(Kw!("LAST_IDENTITY"), Args<()>),
        LCase(Kw!("LCASE"), Args<Scalx>),
        Least(Kw!("LEAST"), Args1Plus),
        Left(Kw!("LEFT"), Args<RR>),
        Len(Kw!("LEN"), Args<Scalx>),
        Length(Kw!("LENGTH"), Args<Scalx>),
        Lower(Kw!("LOWER"), Args<Scalx>),
        LPad(Kw!("LPAD"), Args<RRO>),
        LTrim(Kw!("LTRIM"), Args<Scalx>),
        Month(Kw!("MONTH"), Args<Scalx>),
        Now(Kw!("NOW"), Args<()>),
        NullIf(Kw!("NULLIF"), Args<RR>),
        Nvl(Kw!("NVL"), Args<RR>),
        PatIndex(Kw!("PATINDEX"), Args<RR>),
        Position(Kw!("POSITION"), Args<SQLPositionArgs>),
        Power(Kw!("POWER"), Args<RR>),
        Repeat(Kw!("REPEAT"), Args<RR>),
        Replace(Kw!("REPLACE"), Args<RRR>),
        Replicate(Kw!("REPLICATE"), Args<RR>),
        Reverse(Kw!("REVERSE"), Args<Scalx>),
        Right(Kw!("RIGHT"), Args<RR>),
        Round(Kw!("ROUND"), Args<RRO>),
        RPad(Kw!("RPAD"), Args<RRO>),
        RTrim(Kw!("RTRIM"), Args<Scalx>),
        Sign(Kw!("SIGN"), Args<Scalx>),
        Space(Kw!("SPACE"), Args<Scalx>),
        Str(Kw!("STR"), Args<ROO>),
        String(Kw!("STRING"), Args1Plus),
        Stuff(Kw!("STUFF"), Args<RRRR>),
        Substr(Kw!("SUBSTR"), Args<RRO>),
        Substring(Kw!("SUBSTRING"), SubstringArgs),
        ToChar(Kw!("TOCHAR"), Args<RO>),
        ToDate(Kw!("TODATE"), Args<RO>),
        ToUSChar(Kw!("TO_CHAR"), Args<RO>),
        ToUSDate(Kw!("TO_DATE"), Args<RO>),
        ToNumber(Kw!("TO_NUMBER"), Args<Src<Term>>),
        ToTimestamp(Kw!("TO_TIMESTAMP"), Args<RO>),
        UCase(Kw!("UCASE"), Args<Scalx>),
        UnixTimestamp(Kw!("UNIX_TIMESTAMP"), Args<Option<Scalx>>),
        VectorCosine(Kw!("VECTOR_COSINE"), Args<RR>),
        VectorDotProduct(Kw!("VECTOR_DOT_PRODUCT"), Args<RR>),
        Year(Kw!("YEAR"), Args<Scalx>),
        DollarExtract(Kw!("$EXTRACT"), Args<ROO>),
        DollarFind(Kw!("$FIND"), Args<RRO>),
        DollarFNumber(Kw!("$FNUMBER"), Args<ROO>),
        DollarJustify(Kw!("$JUSTIFY"), Args<RRO>),
        DollarLength(Kw!("$LENGTH"), Args<RO>),
        DollarList(Kw!("$LIST"), Args<ROO>),
        DollarListBuild(Kw!("$LISTBUILD"), ArgsN),
        DollarListData(Kw!("$LISTDATA"), Args<RO>),
        DollarListFind(Kw!("$LISTFIND"), Args<RRO>),
        DollarListFromString(Kw!("$LISTFROMSTRING"), Args<RO>),
        DollarListGet(Kw!("$LISTGET"), Args<ROO>),
        DollarListLength(Kw!("$LISTLENGTH"), Args<Scalx>),
        DollarListSame(Kw!("$LISTSAME"), Args<RR>),
        DollarListToString(Kw!("$LISTTOSTRING"), Args<RO>),
        DollarMvFmt(Kw!("$MVFMT"), Args<RR>),
        DollarMvFmts(Kw!("$MVFMTS"), Args<RR>),
        DollarMvIconv(Kw!("$MVICONV"), Args<RR>),
        DollarMvIconvs(Kw!("$MVICONVS"), Args<RR>),
        DollarMvOconv(Kw!("$MVOCONV"), Args<RR>),
        DollarMvOconvs(Kw!("$MVOCONVS"), Args<RR>),
        DollarPiece(Kw!("$PIECE"), Args<RROO>),
        DollarTranslate(Kw!("$TRANSLATE"), Args<RRO>),
        TSqlDbName(Kw!("$TSQL_DB_NAME"), Args<()>),
        TSqlError(Kw!("$TSQL_ERROR"), Args<()>),
        TSqlFetchStatus(Kw!("$TSQL_FETCH_STATUS"), Args<()>),
        TSqlGetUtcDate(Kw!("$TSQL_GETUTCDATE"), Args<()>),
        TSqlHostName(Kw!("$TSQL_HOST_NAME"), Args<()>),
        TSqlLockTimeout(Kw!("$TSQL_LOCK_TIMEOUT"), Args<()>),
        TSqlNestLevel(Kw!("$TSQL_NESTLEVEL"), Args<()>),
        TSqlServerName(Kw!("$TSQL_SERVERNAME"), Args<()>),
        TSqlSpid(Kw!("$TSQL_SPID"), Args<()>),
        TSqlSqlStatus(Kw!("$TSQL_SQLSTATUS"), Args<()>),
        TSqlTranCount(Kw!("$TSQL_TRANCOUNT"), Args<()>),
        TSqlVersion(Kw!("$TSQL_VERSION"), Args<()>),
        TSqlNewId(Kw!("$TSQL_NEWID"), Args<()>),
        TSqlObjectId(Kw!("$TSQL_OBJECT_ID"), Args<Scalx>),
        TSqlObjectName(Kw!("$TSQL_OBJECT_NAME"), Args<Scalx>),
    }

    #[derive(Grammar, Clone)]
    #[tygr(inline)]
    pub struct SQLPositionArgs {
        pub needle: Scalx,
        pub in_kw: (Space, Kw!("IN"), Space),
        pub haystack: Scalx,
    }

    pub type SubstringArgs = Args<SubstringArgsInternal>;

    #[derive(Grammar, Clone)]
    pub struct SubstringArgsInternal {
        pub expr: Scalx,
        pub rest: Either<
            (
                (Space, Kw!("FROM"), Space),
                Scalx,
                Option<((Space, Kw!("FOR"), Space), Scalx)>,
            ),
            (ArgSep, Scalx, Option<(ArgSep, Scalx)>),
        >,
    }

    pub type ODBCEscape = Wrap<(StringEq!("{"), Space), DTS, (Space, StringEq!("}"))>;

    #[derive(Grammar, Clone)]
    pub enum DTS {
        Function(Prefix<(Kw!("FN"), Space1), ODBCFunc>),
        DateTime(DTSVal),
    }

    #[derive(Grammar, Clone)]
    pub enum ODBCFunc {
        Abs(Kw!("ABS"), Args<Scalx>),
        Acos(Kw!("ACOS"), Args<Scalx>),
        Asin(Kw!("ASIN"), Args<Scalx>),
        Atan2(Kw!("ATAN2"), Args<RR>),
        Atan(Kw!("ATAN"), Args<Scalx>),
        Ceiling(Kw!("CEILING"), Args<Scalx>),
        Cos(Kw!("COS"), Args<Scalx>),
        Cot(Kw!("COT"), Args<Scalx>),
        Exp(Kw!("EXP"), Args<Scalx>),
        Floor(Kw!("FLOOR"), Args<Scalx>),
        Log(Kw!("LOG"), Args<Scalx>),
        Log10(Kw!("LOG10"), Args<Scalx>),
        Mod(Kw!("MOD"), Args<RR>),
        Pi(Kw!("PI"), Option<Args<()>>),
        Power(Kw!("POWER"), Args<RR>),
        Sin(Kw!("SIN"), Args<Scalx>),
        Sqrt(Kw!("SQRT"), Args<Scalx>),
        Tan(Kw!("TAN"), Args<Scalx>),
        Ascii(Kw!("ASCII"), Args<Scalx>),
        Char(Kw!("CHAR"), Args<Scalx>),
        Concat(Kw!("CONCAT"), Args<RR>),
        CurDate(Kw!("CURDATE"), Option<Args<()>>),
        CurTime(Kw!("CURTIME"), Option<Args<()>>),
        Database(Kw!("DATABASE"), Option<Args<()>>),
        DayName(Kw!("DAYNAME"), Args<Scalx>),
        DayOfMonth(Kw!("DAYOFMONTH"), Args<Scalx>),
        DayOfWeek(Kw!("DAYOFWEEK"), Args<Scalx>),
        DayOfYear(Kw!("DAYOFYEAR"), Args<Scalx>),
        Degrees(Kw!("DEGREES"), Args<Scalx>),
        Hour(Kw!("HOUR"), Args<Scalx>),
        LCase(Kw!("LCASE"), Args<Scalx>),
        Left(Kw!("LEFT"), Args<RR>),
        Length(Kw!("LENGTH"), Args<Scalx>),
        LTrim(Kw!("LTRIM"), Args<Scalx>),
        Minute(Kw!("MINUTE"), Args<Scalx>),
        Month(Kw!("MONTH"), Args<Scalx>),
        MonthName(Kw!("MONTHNAME"), Args<Scalx>),
        Now(Kw!("NOW"), Option<Args<()>>),
        Quarter(Kw!("QUARTER"), Args<Scalx>),
        Radians(Kw!("RADIANS"), Args<Scalx>),
        Repeat(Kw!("REPEAT"), Args<RR>),
        Right(Kw!("RIGHT"), Args<RR>),
        Round(Kw!("ROUND"), Args<RR>),
        RTrim(Kw!("RTRIM"), Args<Scalx>),
        Second(Kw!("SECOND"), Args<Scalx>),
        Sign(Kw!("SIGN"), Args<Scalx>),
        Space(Kw!("SPACE"), Args<Scalx>),
        Substring(Kw!("SUBSTRING"), SubstringArgs),
        TimestampAdd(
            Kw!("TIMESTAMPADD"),
            Args<RRR<DoubleQuotable<TSAddInterval>>>,
        ),
        TimestampDiff(
            Kw!("TIMESTAMPDIFF"),
            Args<RRR<DoubleQuotable<TSDiffInterval>>>,
        ),
        Truncate(Kw!("TRUNCATE"), Args<RR>),
        UCase(Kw!("UCASE"), Args<Scalx>),
        Week(Kw!("WEEK"), Args<Scalx>),
        Year(Kw!("YEAR"), Args<Scalx>),
    }

    #[derive(Grammar, Clone)]
    pub struct DTSVal {
        pub dts_type: DTSType,
        pub sp: Space,
        pub val_spec: DTSValSpec,
    }

    #[derive(Grammar, Clone)]
    pub enum DTSType {
        Date(Kw!("D")),
        Timestamp(Kw!("TS")),
        Time(Kw!("T")),
    }

    #[derive(Grammar, Clone)]
    pub struct Fetch {
        pub keyword: (Kw!("FETCH"), Space),
        pub direction: Option<(Either<Kw!("FIRST"), Kw!("NEXT")>, Space)>,
        pub valspec: Count,
        pub row_keyword: Option<(Space, RowRowsInline)>,
        pub only: Option<(Space, Kw!("ONLY"))>,
    }

    #[derive(Grammar, Clone)]
    #[tygr(inline)]
    pub enum RowRowsInline {
        Rows(Kw!("ROWS")),
        Row(Kw!("ROW")),
    }

    /// `ALL` where `DISTINCT` could stand: what a SELECT or an aggregate does without it.
    pub type AllKw = Kw!("ALL");
}
