//! The IRIS SQL grammar for `&sql(...)` blocks embedded in ObjectScript, reached from `cos.rs`.
//! Syntax only: `ascot` reports parse errors here but does not type-check SQL yet.
//!
//! Written once under `#[levels]`, which makes it twice: `full`, every spelling IRIS accepts, is
//! re-exported here; `core` lacks the syntactic sugar, which `desugar` rewrites into it.
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

/// A part in parentheses, blanks allowed within.
pub type Parenthesized<T> = Wrap<(StringEq!("("), Space), T, (Space, StringEq!(")"))>;

pub type SubList = Parenthesized<Option<VecSep<Sub, ArgSep>>>;

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

    #[sugar]
    use super::sugar::*;

    #[derive(Grammar)]
    pub struct TName(pub Src<DottedIdent>);

    #[derive(Grammar)]
    #[tygr(name = "SQL function name")]
    pub struct SQLFunctionName(pub Src<DottedIdent>);

    #[derive(Grammar)]
    #[tygr(name = "colmn")]
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
            Coalesce Convert DataLength Date Day Decode
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
            expr,
            as_kw: (Space::blank(), Default::default(), Default::default()),
            cast_type,
        };
        let args = Wrap::new(
            (Default::default(), Space::default()),
            Box::new(cast),
            (Space::default(), Default::default()),
        );
        core::Term::Builtin(Box::new(core::Builtin::Cast(
            Default::default(),
            (Space::default(), args),
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
            _ => core::Scalx(VecSep::one(term.desugar())),
        };
        call(&op.print(), vec![arg])
    }

    /// `%TRUNCATE(x, n)` is the call, as `%STRING`, `%SQLSTRING` and `%SQLUPPER` with a truncation are.
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
    pub struct Scalx(pub VecSep<Src<Term>, Wrap<Space, Scalop, Space>>);
    #[derive(Grammar)]
    #[tygr(name = "case")]
    pub struct Case {
        pub case_kw: StringEqCI!("CASE"),
        pub body: CaseBody,
        pub else_clause: Implied<SQLElseClause>,
        pub end_kw: (Space, StringEqCI!("END")),
    }

    /// Searched, each WHEN a condition, or simple, each WHEN a value the operand is compared to; 2026.2
    /// rejects a searched CASE without WHEN and takes a simple one.
    #[derive(Grammar)]
    pub enum CaseBody {
        Searched(Vec1<(Space, When<Prdct>)>),
        Simple(Space, Scalx, Vec<(Space, When<Scalx>)>),
    }

    pub type WhenKw = StringEqCI!("WHEN");

    pub type ThenKw = StringEqCI!("THEN");

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct When<T: Grammar> {
        pub when_kw: (WhenKw, Space),
        pub operand: T,
        pub then_kw: (Space, ThenKw, Space),
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
        pub as_kw: (Space, StringEqCI!("AS"), Space1),
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

    /// The functions a call by name cannot spell: the `$` functions, CURRENT_TIMESTAMP and
    /// CURRENT_TIME with a precision, which the niladic would shadow, and the ODBC-only functions.
    #[derive(Grammar)]
    #[tygr(name = "builtin")]
    pub enum Builtin {
        XmlConcat(XMLConcat),
        XmlElement(XMLElement),
        XmlForest(XMLForest),
        Cast(StringEqCI!("CAST"), Args<Box<SQLCastExpression>>),
        DateAdd(StringEqCI!("DATEADD"), Args<RRR<DatePart>>),
        DateDiff(StringEqCI!("DATEDIFF"), Args<RRR<DatePart>>),
        DateName(StringEqCI!("DATENAME"), Args<RR<DatePart>>),
        DatePart(StringEqCI!("DATEPART"), Args<RR<DatePartPart>>),
        DateTrunc(StringEqCI!("DATE_TRUNC"), Args<RR<DatePart>>),
        Convert(StringEqCI!("CONVERT"), Args<RR<Scalx, SQLConvertFunc>>),
        JSONArray(StringEqCI!("JSON_ARRAY"), SQLJSONArrayArgs),
        JSONObject(StringEqCI!("JSON_OBJECT"), SQLJSONObjectArgs),
        Predict(StringEqCI!("PREDICT"), Args<SQLPredictArgs>),
        Probability(StringEqCI!("PROBABILITY"), Args<SQLProbabilityArgs>),
        SearchIndex(StringEqCI!("SEARCH_INDEX"), Args<SQLSearchIndexArgs>),
        TimestampAdd(
            StringEqCI!("TIMESTAMPADD"),
            Args<RRR<DoubleQuotable<TSAddInterval>>>,
        ),
        TimestampDiff(
            StringEqCI!("TIMESTAMPDIFF"),
            Args<RRR<DoubleQuotable<TSDiffInterval>>>,
        ),
        ToVector(
            StringEqCI!("TO_VECTOR"),
            Args<ROO<Scalx, VectorDatyp, Integer>>,
        ),
        Trim(StringEqCI!("TRIM"), Args<TrimArgs>),
        Odbc(ODBCOnly),
        CurrentTimestamp(StringEqCI!("CURRENT_TIMESTAMP"), Args<Scalx>),
        CurrentTime(StringEqCI!("CURRENT_TIME"), Args<Scalx>),
        DollarExtract(StringEqCI!("$EXTRACT"), Args<ROO>),
        DollarFind(StringEqCI!("$FIND"), Args<ROO>),
        DollarFNumber(StringEqCI!("$FNUMBER"), Args<ROO>),
        DollarJustify(StringEqCI!("$JUSTIFY"), Args<RRO>),
        DollarLength(StringEqCI!("$LENGTH"), Args<RO>),
        DollarList(StringEqCI!("$LIST"), Args<ROO>),
        DollarListBuild(StringEqCI!("$LISTBUILD"), ArgsN),
        DollarListData(StringEqCI!("$LISTDATA"), Args<RO>),
        DollarListFind(StringEqCI!("$LISTFIND"), Args<RRO>),
        DollarListFromString(StringEqCI!("$LISTFROMSTRING"), Args<RO>),
        DollarListGet(StringEqCI!("$LISTGET"), Args<ROO>),
        DollarListLength(StringEqCI!("$LISTLENGTH"), Args<Scalx>),
        DollarListSame(StringEqCI!("$LISTSAME"), Args<RR>),
        DollarListToString(StringEqCI!("$LISTTOSTRING"), Args<RO>),
        DollarMvFmt(StringEqCI!("$MVFMT"), Args<RR>),
        DollarMvFmts(StringEqCI!("$MVFMTS"), Args<RR>),
        DollarMvIconv(StringEqCI!("$MVICONV"), Args<RR>),
        DollarMvIconvs(StringEqCI!("$MVICONVS"), Args<RR>),
        DollarMvOconv(StringEqCI!("$MVOCONV"), Args<RR>),
        DollarMvOconvs(StringEqCI!("$MVOCONVS"), Args<RR>),
        DollarPiece(StringEqCI!("$PIECE"), Args<RROO>),
        DollarTranslate(StringEqCI!("$TRANSLATE"), Args<RRO>),
        TSqlDbName(StringEqCI!("$TSQL_DB_NAME"), Args<()>),
        TSqlError(StringEqCI!("$TSQL_ERROR"), Args<()>),
        TSqlFetchStatus(StringEqCI!("$TSQL_FETCH_STATUS"), Args<()>),
        TSqlGetUtcDate(StringEqCI!("$TSQL_GETUTCDATE"), Args<()>),
        TSqlHostName(StringEqCI!("$TSQL_HOST_NAME"), Args<()>),
        TSqlLockTimeout(StringEqCI!("$TSQL_LOCK_TIMEOUT"), Args<()>),
        TSqlNestLevel(StringEqCI!("$TSQL_NESTLEVEL"), Args<()>),
        TSqlServerName(StringEqCI!("$TSQL_SERVERNAME"), Args<()>),
        TSqlSpid(StringEqCI!("$TSQL_SPID"), Args<()>),
        TSqlSqlStatus(StringEqCI!("$TSQL_SQLSTATUS"), Args<()>),
        TSqlTranCount(StringEqCI!("$TSQL_TRANCOUNT"), Args<()>),
        TSqlVersion(StringEqCI!("$TSQL_VERSION"), Args<()>),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum OptionallyWrapped<T: Grammar> {
        Wrapped(Parenthesized<T>),
        Scalar(T),
    }

    /// TRIM's arguments: what to trim, then the string.
    #[derive(Grammar)]
    pub struct TrimArgs {
        pub how: TrimHow,
        pub expr: Scalx,
    }

    /// `BOTH ' ' FROM` in the core: a lone spec trims blanks, lone chars both ends.
    #[derive(Grammar)]
    pub enum TrimHow {
        SpecChars(TrimSpec, Space, Scalx, (Space, StringEqCI!("FROM"), Space)),
        #[sugar]
        Spec(TrimSpec, Space, Option<(StringEqCI!("FROM"), Space)>),
        #[sugar]
        Chars(Option<(Scalx, Space, StringEqCI!("FROM"), Space)>),
    }

    /// `TRIM(LEADING FROM x)` and `TRIM(LEADING x)` are `TRIM(LEADING ' ' FROM x)`, `TRIM(c FROM x)` is
    /// `TRIM(BOTH c FROM x)` and `TRIM(x)` `TRIM(BOTH ' ' FROM x)`.
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
        Leading(StringEqCI!("LEADING")),
        Trailing(StringEqCI!("TRAILING")),
        Both(StringEqCI!("BOTH")),
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
        pub with_clause: Option<((Space, StringEqCI!("WITH"), Space), PredictWith)>,
    }

    pub type PredictProbabilityUse = Prefix<(StringEqCI!("USE"), Space), TrainedModelName>;

    #[derive(Grammar)]
    #[tygr(name = "trained-model-name")]
    pub struct TrainedModelName(pub Ident);

    #[derive(Grammar)]
    #[tygr(name = "predict-with")]
    pub enum PredictWith {
        Unnamed(Parenthesized<VecSep<Scalx, ArgSep>>),
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
        pub for_clause: Option<(Space, Prefix<(StringEqCI!("FOR"), Space), Scalx>)>,
        pub with_clause: Option<((Space, StringEqCI!("WITH"), Space), PredictWith)>,
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
    #[tygr(name = "JSON null clause")]
    pub struct JSONNullClause(
        pub Either<StringEqCI!("NULL"), StringEqCI!("ABSENT")>,
        pub Space1,
        pub StringEqCI!("ON"),
        pub Space1,
        pub StringEqCI!("NULL"),
    );

    #[derive(Grammar)]
    #[tygr(name = "xmlElement")]
    pub struct XMLElement {
        pub keyword: StringEqCI!("XMLELEMENT"),
        pub args: Args<(Option<SQLXmlElementName>, Space, VecSep<XMLValue, ArgSep>)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum SQLXmlElementName {
        Name(StringEqCI!("NAME"), Option<(Space, Ident)>),
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
        pub attrs: Args<VecSep<SQLXmlAttribute, ArgSep>>,
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
        pub items: Args<VecSep<SQLXmlAttribute, ArgSep>>,
    }

    #[derive(Grammar)]
    #[tygr(name = "xmlConcat")]
    pub struct XMLConcat {
        pub keyword: StringEqCI!("XMLCONCAT"),
        pub values: Args<VecSep<XMLValue2, ArgSep>>,
    }

    #[derive(Grammar)]
    pub struct XTFun {
        pub fun: Prefix<StringEqCI!("$$"), StringOf1<IsExtFuncChar>>,
        pub args: Parenthesized<Option<VecSep<Scalx, ArgSep>>>,
    }

    /// `{fn CONVERT(x, SQL_TYPE)}` and `{fn IFNULL(x, y)}`, which IRIS reads otherwise without the
    /// escape (CONVERT takes `SQL_TYPE` for a column, IFNULL(x, y) is NULL when x is not; measured on
    /// 2026.2), and `{fn USER}`, whose niladic `USER` has no parentheses for the braces' comments.
    pub type ODBCOnly = Wrap<
        (StringEq!("{"), Space, StringEqCI!("FN"), Space1),
        ODBCOnlyFunc,
        (Space, StringEq!("}")),
    >;

    #[derive(Grammar)]
    #[tygr(name = "odbc-only func")]
    pub enum ODBCOnlyFunc {
        Convert(StringEqCI!("CONVERT"), Args<RR<Scalx, SQLConvertFunc>>),
        IfNull(StringEqCI!("IFNULL"), Args<RR>),
        User(StringEqCI!("USER"), Option<StringEq!("()")>),
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

    /// The date part of DATEADD, DATEDIFF, DATENAME and DATE_TRUNC: a keyword, a host variable, or either in
    /// parentheses.
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
    #[sugar]
    pub enum DatePartPartRaw {
        SqlTimestamp(StringEqCI!("SQLTIMESTAMP")),
        #[sugar]
        Sts(StringEqCI!("STS")),
        Common(DatePartCommon),
    }

    /// The core spells SQLTIMESTAMP in upper case, as it does every date part.
    impl Desugar for DatePartPartRaw {
        type Core = core::DatePartPartRaw;
        fn desugar(&self) -> Self::Core {
            match self {
                Self::SqlTimestamp(_) | Self::Sts(_) => {
                    core::DatePartPartRaw::SqlTimestamp(Default::default())
                }
                Self::Common(common) => core::DatePartPartRaw::Common(common.desugar()),
            }
        }
    }

    /// The date part keywords, each full name before the abbreviations it prefixes.
    #[derive(Grammar)]
    #[sugar]
    pub enum DatePartCommon {
        Year(StringEqCI!("YEAR")),
        Quarter(StringEqCI!("QUARTER")),
        Month(StringEqCI!("MONTH")),
        Weekday(StringEqCI!("WEEKDAY")),
        Week(StringEqCI!("WEEK")),
        DayOfYear(StringEqCI!("DAYOFYEAR")),
        Day(StringEqCI!("DAY")),
        Hour(StringEqCI!("HOUR")),
        Minute(StringEqCI!("MINUTE")),
        Second(StringEqCI!("SECOND")),
        Millisecond(StringEqCI!("MILLISECOND")),
        Microsecond(StringEqCI!("MICROSECOND")),
        Nanosecond(StringEqCI!("NANOSECOND")),
        #[sugar]
        Yyyy(StringEqCI!("YYYY")),
        #[sugar]
        Yy(StringEqCI!("YY")),
        #[sugar]
        Qq(StringEqCI!("QQ")),
        #[sugar]
        Mm(StringEqCI!("MM")),
        #[sugar]
        Wk(StringEqCI!("WK")),
        #[sugar]
        Ww(StringEqCI!("WW")),
        #[sugar]
        Dw(StringEqCI!("DW")),
        #[sugar]
        Dy(StringEqCI!("DY")),
        #[sugar]
        Dd(StringEqCI!("DD")),
        #[sugar]
        Hh(StringEqCI!("HH")),
        #[sugar]
        Mi(StringEqCI!("MI")),
        #[sugar]
        Ss(StringEqCI!("SS")),
        #[sugar]
        Mcs(StringEqCI!("MCS")),
        #[sugar]
        Ms(StringEqCI!("MS")),
        #[sugar]
        Ns(StringEqCI!("NS")),
        #[sugar]
        Q(StringEqCI!("Q")),
        #[sugar]
        M(StringEqCI!("M")),
        #[sugar]
        W(StringEqCI!("W")),
        #[sugar]
        Y(StringEqCI!("Y")),
        #[sugar]
        D(StringEqCI!("D")),
        #[sugar]
        N(StringEqCI!("N")),
        #[sugar]
        S(StringEqCI!("S")),
    }

    /// A date part is its full name in upper case, whatever its spelling or abbreviation.
    impl Desugar for DatePartCommon {
        type Core = core::DatePartCommon;
        fn desugar(&self) -> Self::Core {
            use core::DatePartCommon::*;
            match self {
                Self::Year(_) | Self::Yyyy(_) | Self::Yy(_) => Year(Default::default()),
                Self::Quarter(_) | Self::Qq(_) | Self::Q(_) => Quarter(Default::default()),
                Self::Month(_) | Self::Mm(_) | Self::M(_) => Month(Default::default()),
                Self::Week(_) | Self::Wk(_) | Self::Ww(_) => Week(Default::default()),
                Self::Weekday(_) | Self::Dw(_) | Self::W(_) => Weekday(Default::default()),
                Self::DayOfYear(_) | Self::Dy(_) | Self::Y(_) => DayOfYear(Default::default()),
                Self::Day(_) | Self::Dd(_) | Self::D(_) => Day(Default::default()),
                Self::Hour(_) | Self::Hh(_) => Hour(Default::default()),
                Self::Minute(_) | Self::Mi(_) | Self::N(_) => Minute(Default::default()),
                Self::Second(_) | Self::Ss(_) | Self::S(_) => Second(Default::default()),
                Self::Millisecond(_) | Self::Ms(_) => Millisecond(Default::default()),
                Self::Microsecond(_) | Self::Mcs(_) => Microsecond(Default::default()),
                Self::Nanosecond(_) | Self::Ns(_) => Nanosecond(Default::default()),
            }
        }
    }

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

    #[derive(Grammar)]
    #[tygr(name = "dts val spec")]
    pub enum DTSValSpec {
        Literal(Literal),
        HostVar(HostVar),
    }

    #[derive(Grammar)]
    #[tygr(name = "agg")]
    pub enum Agg {
        AggFuncArgs { fun: AggFunc, arg: Args<AggArg> },
        CountStar(StringEqCI!("COUNT"), Args<StringEq!("*")>),
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
        #[sugar]
        pub all: Option<(AllKw, Space)>,
        pub distinct: Option<(Dist, Space)>,
        pub expr: Scalx,
        pub foreach: Option<(Space, Prefix<(StringEqCI!("%FOREACH"), Space), List>)>,
        pub afterhaving: Option<(Space, StringEqCI!("%AFTERHAVING"))>,
    }

    #[derive(Grammar)]
    #[tygr(name = "dist")]
    pub struct Dist {
        pub keyword: StringEqCI!("DISTINCT"),
        pub by: Option<(Space1, Prefix<(StringEqCI!("BY"), Space), List>)>,
    }

    pub type List = Parenthesized<VecSep<Scalx, ArgSep>>;

    #[derive(Grammar)]
    #[tygr(name = "window")]
    pub struct Window {
        pub fun: WindowFunc,
        pub arg: Parenthesized<Option<Either<StringEq!("*"), VecSep<Scalx, ArgSep>>>>,
        pub over_kw: (Space, StringEqCI!("OVER"), Space),
        pub spec: Parenthesized<(
            Option<WindowPartition>,
            Option<(Space, Order)>,
            Option<(Space, StringEqCI!("ROWS"), Space, WindowRows)>,
        )>,
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

    pub type WindowPartition =
        Prefix<(StringEqCI!("PARTITION"), Space1, StringEqCI!("BY"), Space), VecSep<Scalx, ArgSep>>;

    #[derive(Grammar)]
    #[tygr(name = "window rows")]
    pub enum WindowRows {
        #[sugar]
        Single(WindowRowsBound),
        Between {
            between_kw: StringEqCI!("BETWEEN"),
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
    #[tygr(name = "window rows bound")]
    pub enum WindowRowsBound {
        UnboundedPreceding(StringEqCI!("UNBOUNDED"), Space1, StringEqCI!("PRECEDING")),
        UnboundedFollowing(StringEqCI!("UNBOUNDED"), Space1, StringEqCI!("FOLLOWING")),
        Preceding(Number, Space, StringEqCI!("PRECEDING")),
        Following(Number, Space, StringEqCI!("FOLLOWING")),
        CurrentRow(StringEqCI!("CURRENT"), Space1, StringEqCI!("ROW")),
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
        Negated(NotKw, Space1, K),
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
            let Some(not_kw) = negation(predicate) else {
                return plain;
            };
            core::Cond::Not {
                not_kw: (not_kw, Space::blank()),
                condition: Box::new(Src::synthesized(core::Cond::Paren(Wrap::new(
                    (Default::default(), Space::default()),
                    Box::new(core::Prdct(VecSep::one(core::And(VecSep::one(
                        Src::synthesized(plain),
                    ))))),
                    (Space::default(), Default::default()),
                )))),
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
                Space1,
                StringEqCI!("SOME"),
                Space1,
                StringEqCI!("%ELEMENT"),
                Space,
            ),
            column: Parenthesized<Colmn>,
            label: Option<(Space, Option<(StringEqCI!("AS"), Space)>, Ident)>,
            predicate: (Space, Parenthesized<Box<Prdct>>),
        },
        ForSome {
            kw: (StringEqCI!("FOR"), Space1, StringEqCI!("SOME"), Space),
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
        GroupOnly(Group),
    }

    pub type AndClause<T> = Prefix<(StringEqCI!("AND"), Space), T>;

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
            is_kw: (StringEqCI!("IS"), Space1),
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
            escape: Option<(Space, EscapeScalx)>,
        },
        Matches {
            matches_kw: Negatable<(StringEqCI!("%MATCHES"), Space)>,
            pattern: Scalx,
            escape: Option<(Space, EscapeScalx)>,
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
            size_hint: Option<(Space, SizeHint)>,
        },
        Find {
            find_kw: (StringEqCI!("%FIND"), Space),
            expr: Scalx,
            size_hint: Option<(Space, SizeHint)>,
        },
        Pattern {
            pattern_kw: (StringEqCI!("%PATTERN"), Space),
            pattern: Scalx,
        },
        IsJSON {
            format: Option<(StringEqCI!("FORMAT"), Space1, JSONRepresentation, Space1)>,
            is_kw: (StringEqCI!("IS"), Space1),
            json_kw: Negatable<StringEqCI!("JSON")>,
            constraint: Option<(Space1, JSONPredicateTypeConstraint)>,
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

    pub type EscapeScalx = Prefix<(StringEqCI!("ESCAPE"), Space), Scalx>;

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
    pub struct SizeHint {
        pub kw: (StringEqCI!("SIZE"), Space),
        /// Doubled, as IRIS requires: 2026.2 rejects `SIZE (n)` and `SIZE n`, takes deeper nesting and blanks.
        pub size: Parenthesized<Parenthesized<PConst>>,
    }

    #[derive(Grammar)]
    pub enum PConst {
        #[sugar]
        Nested(Parenthesized<Box<PConst>>),
        Literal(Literal),
    }

    /// `SIZE ((n))` keeps one pair of parentheses: the ones within go.
    impl Desugar for PConstSugar<'_> {
        type Core = core::PConst;
        fn desugar(&self) -> Self::Core {
            let Self::Nested(inner) = self;
            (*inner.wrapped).desugar()
        }
    }

    #[derive(Grammar)]
    #[tygr(name = "JSON representation")]
    pub struct JSONRepresentation(pub StringEqCI!("JSON"));

    #[derive(Grammar)]
    #[tygr(name = "JSON predicate type constraint")]
    pub enum JSONPredicateTypeConstraint {
        Value(StringEqCI!("VALUE")),
        Array(StringEqCI!("ARRAY")),
        Object(StringEqCI!("OBJECT")),
        Scalar(StringEqCI!("SCALAR")),
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
        pub options: Vec<(SelectOption, Space)>,
        #[sugar]
        pub all: Option<(AllKw, Space)>,
        pub selst: Selst,
        pub from: Option<(Space, FromClause)>,
        pub where_clause: Option<(Space, WhereClause)>,
        pub group: Option<(Space, Group)>,
        pub having: Option<(Space, Having)>,
        pub order: Option<(Space, Order)>,
    }

    #[derive(Grammar)]
    #[tygr(name = "select option")]
    pub enum SelectOption {
        NoFPlan(StringEqCI!("%NOFPLAN")),
        NoLock(StringEqCI!("%NOLOCK")),
        Profile(StringEqCI!("%PROFILE")),
        ProfileAll(StringEqCI!("%PROFILE_ALL")),
        NoRuntime(StringEqCI!("%NORUNTIME")),
        DoRuntime(StringEqCI!("%DORUNTIME")),
        RowLimit(StringEqCI!("%ROWLIMIT"), Space, ValSpec),
        RowOffset(StringEqCI!("%ROWOFFSET"), Space, ValSpec),
    }

    #[derive(Grammar)]
    pub enum TopSpec {
        ValSpec(DTSValSpec),
        All(StringEqCI!("ALL")),
    }

    #[derive(Grammar)]
    #[tygr(name = "selst")]
    pub struct Selst {
        pub distinct: Option<(Dist, Space)>,
        pub top: Option<(Prefix<(StringEqCI!("TOP"), Space1), TopSpec>, Space)>,
        pub select_list: SelectList,
        pub targt: Option<(Space, Targt)>,
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
    pub struct Targt(pub TargtEnum);

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
        pub keyword: (StringEqCI!("FROM"), Space),
        pub options: Vec<(FromOpt, Space)>,
        pub tables: Tables,
    }

    #[derive(Grammar)]
    #[tygr(name = "from_opt")]
    pub enum FromOpt {
        InOrder(StringEqCI!("%inorder")),
        Full(StringEqCI!("%full")),
        DbugFull(StringEqCI!("%dbugfull")),
        Flatten(StringEqCI!("%flatten")),
        NoBSL(StringEqCI!("%nobsl")),
        NoFixedStats(StringEqCI!("%nofixedstats"), Space, TableRefs),
        NoFlatten(StringEqCI!("%noflatten")),
        NoMerge(StringEqCI!("%nomerge")),
        NoReduce(StringEqCI!("%noreduce")),
        NoSVSO(StringEqCI!("%nosvso")),
        NoTopOpt(StringEqCI!("%notopopt")),
        NoFactorOrderByOr(StringEqCI!("%nofactororderbyor")),
        NoFactorAndFromOr(StringEqCI!("%nofactorandfromor")),
        NoMergeEquiAgg(StringEqCI!("%nomergeequivagg")),
        IgnoreIndices(StringEqCI!("%ignoreindices"), Space, IndexRefs),
        IgnoreIndex(StringEqCI!("%ignoreindex"), Space, IndexRefs),
        StartTable(StringEqCI!("%starttable")),
        AllIndex(StringEqCI!("%allindex")),
        NoUnionOrOpt(StringEqCI!("%nounionoropt")),
        FirstTable(StringEqCI!("%firsttable"), Space, Ident),
        Parallel(StringEqCI!("%parallel")),
        NoParallel(StringEqCI!("%noparallel")),
    }

    #[derive(Grammar)]
    pub struct TableRefs(pub VecSep<TableRef, ArgSep>);

    #[derive(Grammar)]
    pub enum TableRef {
        All(StringEq!("*")),
        SchemaAll(Schema, Space, StringEq!(".*")),
        SchemaTable {
            schema: Option<(Schema, Space, StringEq!("."), Space)>,
            table: TName,
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
                            core::Frmit::JTab(Wrap::new(
                                (Default::default(), Space::default()),
                                Box::new(table),
                                (Space::default(), Default::default()),
                            )),
                        ),
                        joins: Vec::new(),
                    };
                }
                let cross = core::Join::Cross {
                    cross_kw: (
                        Default::default(),
                        Default::default(),
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
            cross_kw: (StringEqCI!("CROSS"), Space1, StringEqCI!("JOIN"), Space),
            table: Box<JTab>,
        },
        Natural {
            natural_kw: (StringEqCI!("NATURAL"), Space1),
            join_type: JOp,
            space: Space,
            table: Box<JTab>,
        },
        Qualified {
            join_type: JOp,
            space: Space,
            table: Box<JTab>,
            spec: (Space, JSpec),
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "jop")]
    pub enum JOp {
        Typed {
            jtype: JType,
            sp: Space1,
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
                sp: Default::default(),
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
            on_kw: (StringEqCI!("ON"), Space),
            condition: Prdct,
        },
        Using {
            using_kw: (StringEqCI!("USING"), Space),
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
            lateral: Option<(StringEqCI!("LATERAL"), Space)>,
            query: SQLSubquery,
            alias: Option<Alias>,
        },
        JTab(Parenthesized<Box<JTab>>),
    }

    #[derive(Grammar)]
    #[tygr(name = "json_table")]
    pub struct JSONTable {
        pub kw: StringEq!("JSON_TABLE"),
        pub args: Args<(JSONApiCommonSyntax, Space, JSONTableColumnsSpec)>,
    }

    #[derive(Grammar)]
    #[tygr(name = "json_api_common_syntax")]
    pub struct JSONApiCommonSyntax {
        pub context_item: StringLiteral,
        pub sep: ArgSep,
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
        pub columns: Args<VecSep<JSONTableRegularColumnDefinition, ArgSep>>,
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
        pub table: TName,
        pub args: Parenthesized<Option<VecSep<DTSValSpec, ArgSep>>>,
    }

    pub type WhereClause = Prefix<(StringEqCI!("WHERE"), Space), Prdct>;

    pub type Group =
        Prefix<(StringEqCI!("GROUP"), Space1, StringEqCI!("BY"), Space), VecSep<Scalx, ArgSep>>;

    pub type Having = Prefix<(StringEqCI!("HAVING"), Space), Prdct>;

    pub type Order = Prefix<
        (StringEqCI!("ORDER"), Space1, StringEqCI!("BY"), Space),
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
            limit: Prefix<(StringEqCI!("LIMIT"), Space), LimitSpec>,
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
                        || core::LimitSpec::All(Default::default(), Default::default()),
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
        pub kw: (StringEqCI!("OFFSET"), Space),
        pub count: RowCount,
        #[sugar]
        pub rows: Option<(Space, RowRowsInline)>,
    }

    #[derive(Grammar)]
    pub struct Query(pub VecSep<QTerm, (Space, QueryOp, Space)>);

    #[derive(Grammar)]
    #[tygr(name = "query op")]
    pub enum QueryOp {
        Union {
            union_kw: StringEqCI!("UNION"),
            all: Option<(Space1, StringEqCI!("ALL"))>,
            parallel: Option<(Space1, StringEqCI!("%parallel"))>,
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
        pub or_update: Option<(Space1, StringEqCI!("OR"), Space1, StringEqCI!("UPDATE"))>,
        pub options: Vec<(Space1, DmlOption)>,
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
                .any(|(_, option)| matches!(option, DmlOption::ILock(_)))
        }
    }

    /// One option list for INSERT, UPDATE, DELETE and TRUNCATE TABLE, each validated against its own set.
    #[derive(Grammar)]
    #[tygr(name = "dml option")]
    pub enum DmlOption {
        NoCheck(StringEqCI!("%NOCHECK")),
        NoFPlan(StringEqCI!("%NOFPLAN")),
        NoLock(StringEqCI!("%NOLOCK")),
        NoIndex(StringEqCI!("%NOINDEX")),
        NoTrigger(StringEqCI!("%NOTRIGGER")),
        NoStream(StringEqCI!("%NOSTREAM")),
        NoJourn(StringEqCI!("%NOJOURN")),
        ILock(StringEqCI!("%ILOCK")),
        ObjectFiler(StringEqCI!("%OBJECTFILER")),
        ProfileAll(StringEqCI!("%PROFILE_ALL")),
        Profile(StringEqCI!("%PROFILE")),
        NoRuntime(StringEqCI!("%NORUNTIME")),
        DoRuntime(StringEqCI!("%DORUNTIME")),
    }

    #[derive(Grammar)]
    #[tygr(name = "iud_target")]
    pub struct IUDTarget {
        pub target: TableOrSubquery,
        pub alias: Option<Alias>,
    }

    #[derive(Grammar)]
    pub enum TableOrSubquery {
        Table(TName),
        Subquery(SQLSubquery),
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
        DefaultValues(StringEqCI!("DEFAULT"), Space1, StringEqCI!("VALUES")),
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
                columns: Some((
                    Space::default(),
                    Wrap::new(
                        (Default::default(), Space::default()),
                        commas(columns),
                        (Space::default(), Default::default()),
                    ),
                )),
                values: (
                    Space::blank(),
                    core::ValueSource::ValSpec {
                        values_kw: Default::default(),
                        sp: Space::blank(),
                        spec: core::ValSpec::Scalx(Wrap::new(
                            (Default::default(), Space::default()),
                            commas(values),
                            (Space::default(), Default::default()),
                        )),
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
                                spec: ValSpec::Scalx(exprs),
                                ..
                            },
                        ),
                } if columns.1.wrapped.items.len() == exprs.wrapped.items.len() => {
                    let items = (columns.1.wrapped.items.iter().zip(&exprs.wrapped.items))
                        .map(|(column, value)| {
                            let Scalx(terms) = value;
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

    pub type SQLColumnListParen = Args<VecSep<Colmn, ArgSep>>;

    #[derive(Grammar)]
    #[tygr(name = "value source")]
    pub enum ValueSource {
        Query {
            query: Box<Query>,
            order: Option<(Space, Order)>,
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
        Scalx(Parenthesized<VecSep<Scalx, ArgSep>>),
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
    #[tygr(name = "updat")]
    pub struct Updat {
        pub update_kw: StringEqCI!("UPDATE"),
        pub body: (Space, UpdatBody),
    }

    #[derive(Grammar)]
    pub enum UpdatBody {
        UpdateStats {
            stats_kw: (StringEqCI!("STATISTICS"), Space),
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

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct WhereWerop {
        pub kw: (StringEqCI!("WHERE"), Space),
        pub werop: Werop,
    }

    #[derive(Grammar)]
    pub enum Werop {
        Prdct(Prdct),
        CurrentOf {
            current_of: (StringEqCI!("CURRENT"), Space1, StringEqCI!("OF"), Space),
            ident: Ident,
            prdct: Option<(Space, AndClause<Prdct>)>,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "delet", validated)]
    pub struct Delet {
        pub delete_kw: StringEqCI!("DELETE"),
        pub options: Vec<(Space1, DmlOption)>,
        pub from_kw: Implied<StringEqCI!("FROM")>,
        pub target: (Space, IUDTarget),
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
    #[tygr(name = "truncate", validated)]
    pub struct Truncate {
        pub kw: (StringEqCI!("TRUNCATE"), Space1, StringEqCI!("TABLE")),
        pub options: Vec<(Space1, DmlOption)>,
        pub table: (Space, TName),
    }

    impl Validate for Truncate {
        const REQUIREMENT: &'static str = "take only %NOCHECK, %NOLOCK and %NOJOURN";
        fn validate(&self) -> bool {
            self.options.iter().all(|(_, option)| {
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
            Space1,
            StringEqCI!("TRANSACTION"),
            Space,
        ),
        pub commit_mode: Option<(CommitModeSpec, Space)>,
        pub modes: VecSep<Option<TRMode>, ArgSep>,
    }

    #[derive(Grammar)]
    #[tygr(name = "settp")]
    pub struct SetTP {
        pub start_kw: Option<(StringEqCI!("START"), Space1)>,
        pub kw: (StringEqCI!("SET"), Space1, StringEqCI!("TRANSACTION")),
        pub commit_mode: Option<(Space1, CommitModeSpec)>,
        pub modes: VecSep<Option<(Space, TRMode)>, (Space, StringEq!(","))>,
    }

    pub type CommitModeSpec = Prefix<StringEqCI!("%COMMITMODE"), (Space1, CommitMode)>;

    #[derive(Grammar)]
    pub enum CommitMode {
        Implicit(StringEqCI!("IMPLICIT")),
        Explicit(StringEqCI!("EXPLICIT")),
        None(StringEqCI!("NONE")),
    }

    #[derive(Grammar)]
    pub enum TRMode {
        IsolationLevel {
            kw: (
                StringEqCI!("ISOLATION"),
                Space1,
                StringEqCI!("LEVEL"),
                Space1,
            ),
            level: GetLevel,
        },
        ReadOnly(StringEqCI!("READ"), Space1, StringEqCI!("ONLY")),
        ReadWrite(StringEqCI!("READ"), Space1, StringEqCI!("WRITE")),
        DiagnosticsSize {
            kw: (
                StringEqCI!("DIAGNOSTICS"),
                Space1,
                StringEqCI!("SIZE"),
                Space,
            ),
            size: Src<Term>,
        },
    }

    #[derive(Grammar)]
    pub enum GetLevel {
        ReadUncommitted(StringEqCI!("READ"), Space1, StringEqCI!("UNCOMMITTED")),
        ReadCommitted(StringEqCI!("READ"), Space1, StringEqCI!("COMMITTED")),
        ReadVerified(StringEqCI!("READ"), Space1, StringEqCI!("VERIFIED")),
        RepeatableRead(StringEqCI!("REPEATABLE"), Space1, StringEqCI!("READ")),
        Serializable(StringEqCI!("SERIALIZABLE")),
    }

    #[derive(Grammar)]
    #[tygr(name = "rollb")]
    pub struct RollB {
        pub rollback_kw: StringEqCI!("ROLLBACK"),
        #[sugar]
        pub work: Option<(Space1, StringEqCI!("WORK"))>,
    }

    /// `START [SET]`, which may precede a transaction option or GET.
    pub type StartSet = Option<(
        StringEqCI!("START"),
        Space,
        Option<(StringEqCI!("SET"), Space)>,
    )>;

    pub type TP = Prefix<StartSet, TPOption>;

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
            work: Option<(Space1, StringEqCI!("WORK"))>,
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
            targt: Option<(Space, Targt)>,
        },
        Close {
            close_kw: (StringEqCI!("CLOSE"), Space),
            cursor: Ident,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "decl")]
    pub struct Decl {
        pub declare_kw: StringEqCI!("DECLARE"),
        pub cursor_name: (Space, Ident),
        pub cursor_kw: (
            Space,
            StringEqCI!("CURSOR"),
            Space1,
            StringEqCI!("FOR"),
            Space,
        ),
        pub statement: DeclStatement,
        pub for_clause: Option<(Space, Prefix<(StringEqCI!("FOR"), Space1), UpdtCls>)>,
    }

    #[derive(Grammar)]
    pub enum DeclStatement {
        Explain(Explain),
        Selct(WithStat<Selct>),
    }

    #[derive(Grammar)]
    pub enum UpdtCls {
        ReadOnly(StringEqCI!("READ"), Space1, StringEqCI!("ONLY")),
        Update(StringEqCI!("UPDATE")),
    }

    #[derive(Grammar)]
    #[tygr(name = "get")]
    pub struct Get {
        pub start_set: StartSet,
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
        pub args: ArgsN,
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

    #[derive(Grammar)]
    #[tygr(name = "explain")]
    pub struct Explain {
        pub explain_kw: StringEqCI!("EXPLAIN"),
        pub options: Option<ExplainOptionList>,
        pub into: Option<(Space1, StringEqCI!("INTO"), Space, Param)>,
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
        pub order: Option<(Space, Order)>,
        pub row_limiting_clause: Option<(Space, RowLimitingClause)>,
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
                    .filter(|(_, clause)| !unlimited(clause)),
            }
        }
    }

    #[derive(Grammar)]
    #[tygr(name = "option")]
    pub struct SQLOption {
        pub kw: (StringEqCI!("SET"), Space1, StringEqCI!("OPTION"), Space1),
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
        pub kw: (
            StringEqCI!("SET"),
            Space1,
            StringEqCI!("ML"),
            Space1,
            StringEqCI!("CONFIGURATION"),
            Space,
        ),
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
            to_kw: (Space1, StringEqCI!("TO"), Space),
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
            on_kw: (Space1, StringEqCI!("ON"), Space1),
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
        Space1,
        StringEqCI!("OPTION"),
        Space1,
        StringEqCI!("FOR"),
        Space,
    )>;

    pub type Revok = Prefix<StringEqCI!("REVOKE"), (Space, RevokeCmd)>;

    #[derive(Grammar)]
    #[tygr(name = "revoke cmd")]
    pub enum RevokeCmd {
        AdminPrivileges {
            privileges: VecSep<AdminPriv, ArgSep>,
            from_kw: (Space1, StringEqCI!("FROM"), Space),
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
            on_kw: (Space1, StringEqCI!("ON"), Space1),
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
            privileges_kw: Option<(Space1, StringEqCI!("PRIVILEGES"))>,
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
            schema_kw: (StringEqCI!("SCHEMA"), Space),
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
        pub foreign_kw: Option<(StringEqCI!("FOREIGN"), Space1)>,
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
        pub grant_option: Option<(Space1, GrantOpt)>,
        pub command: (Space1, ChkPrvCmd),
    }

    #[derive(Grammar)]
    #[tygr(name = "grant opt")]
    pub struct GrantOpt {
        pub kind: Either<StringEqCI!("GRANT"), StringEqCI!("ADMIN")>,
        pub option_for: (Space1, StringEqCI!("OPTION"), Space1, StringEqCI!("FOR")),
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
        Binary(
            StringEqCI!("BINARY"),
            Option<(Space1, StringEqCI!("VARYING"))>,
            OptParenScalx,
        ),
        Bit(StringEqCI!("BIT")),
        Character(
            StringEqCI!("CHARACTER"),
            Option<(Space1, StringEqCI!("VARYING"))>,
            OptParenScalx,
        ),
        Char(
            StringEqCI!("CHAR"),
            Option<(Space1, StringEqCI!("VARYING"))>,
            OptParenScalx,
        ),
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

    pub type IfExists = Option<(StringEqCI!("IF"), Space1, StringEqCI!("EXISTS"), Space)>;
    pub type IfNotExists = Option<(
        StringEqCI!("IF"),
        Space1,
        StringEqCI!("NOT"),
        Space1,
        StringEqCI!("EXISTS"),
        Space,
    )>;
    pub type OrReplace = Option<(StringEqCI!("OR"), Space1, StringEqCI!("REPLACE"), Space)>;

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct UserSpec {
        pub name: Ident,
        pub password: Option<(Space, PWPrefix, Space, Password)>,
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
        pub foreign: Option<(StringEqCI!("FOREIGN"), Space1)>,
        pub server_kw: (StringEqCI!("SERVER"), Space),
    }

    #[derive(Grammar)]
    #[tygr(name = "ddl")]
    pub enum DDL {
        CreOrAltView {
            cre_or_alt: CreOrAlt,
            kw: (Space1, StringEqCI!("VIEW"), Space),
            tname: TName,
            col_list: Option<(Space, FList1)>,
            as_kw: (Space, StringEqCI!("AS"), Space),
            query: Query,
            with_clause: Option<((Space, StringEqCI!("WITH"), Space1), ViewWith)>,
        },
        AlterStatsDropFixed {
            kw: (StringEqCI!("ALTER"), Space1),
            scope: StatsScope,
            drop_fixed: (
                Space,
                StringEqCI!("DROP"),
                Space1,
                StringEqCI!("FIXED"),
                Space1,
            ),
            clause: StatsClause,
        },
        AlterStatsFix {
            kw: (StringEqCI!("ALTER"), Space1),
            scope: StatsScope,
            fix: (Space, StringEqCI!("FIX"), Space1),
            clause: StatsClause,
        },
        AlterStatsSetRuntime {
            kw: (StringEqCI!("ALTER"), Space1),
            scope: StatsScope,
            set_runtime: (
                Space,
                StringEqCI!("SET"),
                Space1,
                StringEqCI!("RUNTIME"),
                Space1,
            ),
            prop: StatsProperty,
            eq: (Space, StringEq!("="), Space),
            val: TrueFalse,
        },
        AlterForeignTable {
            kw: (
                StringEqCI!("ALTER"),
                Space1,
                StringEqCI!("FOREIGN"),
                Space1,
                StringEqCI!("TABLE"),
                Space,
            ),
            tname: TName,
            action: (Space, AlterFTAction),
        },
        AlterMlConfiguration {
            kw: (
                StringEqCI!("ALTER"),
                Space1,
                StringEqCI!("ML"),
                Space1,
                StringEqCI!("CONFIGURATION"),
                Space,
            ),
            name: MLConfigurationName,
            provider: Option<((Space, StringEqCI!("PROVIDER"), Space1), ProviderName)>,
            desc: Option<((Space, StringEqCI!("%DESCRIPTION"), Space), LiteralOrNull)>,
            using_clause: Option<(Space, MlconfigUsingClause)>,
            settings: Option<(Space, ProviderSpecificSettings)>,
        },
        AlterModel {
            kw: (StringEqCI!("ALTER"), Space1, StringEqCI!("MODEL"), Space),
            name: ModelName,
            action: (Space, AlterModelAction),
        },
        AlterTable {
            kw: (StringEqCI!("ALTER"), Space1, StringEqCI!("TABLE"), Space),
            tname: TName,
            with_check: Option<((Space, StringEqCI!("WITH"), Space1), CheckSpec)>,
            action: Option<(Space, AlterAction)>,
        },
        AlterUser {
            kw: (StringEqCI!("ALTER"), Space1, StringEqCI!("USER"), Space),
            user: UserSpec,
        },
        AlterServer {
            kw: (StringEqCI!("ALTER"), Space1),
            server_kw: ServerKw,
            tname: TName,
            action: (Space, AlterServerAction),
        },
        BuildIndex {
            kw: (StringEqCI!("BUILD"), Space1, StringEqCI!("INDEX"), Space1),
            options: Vec<(BuildOption, Space1)>,
            for_kw: (StringEqCI!("FOR"), Space1),
            scope: BuildScope,
        },
        CancelQuery {
            kw: (StringEqCI!("CANCEL"), Space1, StringEqCI!("QUERY"), Space),
            val: ValSpec,
            cancel_scope: Option<(Space, CancelQueryScope)>,
            timeout: Option<(Space, StringEqCI!("TIMEOUT"), Space, ValSpec)>,
        },
        Collect {
            kw: (StringEqCI!("COLLECT"), Space1),
            clause: StatsClause,
            target: (Space1, StringEqCI!("FOR"), Space1, StatsScope),
            sample: Option<((Space, StringEqCI!("%SAMPLE_PERCENT"), Space), PercentValue)>,
        },
        CreateBitmapextentIndex {
            kw: (
                StringEqCI!("CREATE"),
                Space1,
                StringEqCI!("BITMAPEXTENT"),
                Space1,
                StringEqCI!("INDEX"),
                Space,
            ),
            name: IndexName,
            on_kw: (Space, StringEqCI!("ON"), Space),
            table_kw: Option<(StringEqCI!("TABLE"), Space)>,
            tname: TName,
        },
        CreateDatabase {
            kw: (
                StringEqCI!("CREATE"),
                Space1,
                StringEqCI!("DATABASE"),
                Space1,
            ),
            file: Option<(StringEqCI!("FILE"), Space1)>,
            name: DatabaseName,
            on_dir: Option<(
                (Space1, StringEqCI!("ON"), Space1, StringEqCI!("DIRECTORY")),
                Option<(Space, StringEq!("="))>,
                (Space, DirectoryName),
            )>,
            with_opts: Option<((Space, StringEqCI!("WITH")), Vec1<(Space, DBOption)>)>,
        },
        CreateForeignTable {
            kw: (
                StringEqCI!("CREATE"),
                Space1,
                StringEqCI!("FOREIGN"),
                Space1,
                StringEqCI!("TABLE"),
                Space,
            ),
            if_not_exists: IfNotExists,
            tname: TName,
            columns: Option<Args<VecSep<(Ident, (Space, Datyp)), ArgSep>>>,
            server: (Space, StringEqCI!("SERVER"), Space),
            server_name: TName,
            ft_options: Option<(Space, FTOptions)>,
            values: Option<(Space, StringEqCI!("VALUES"), Space, FList1)>,
            using_json: Option<((Space, StringEqCI!("USING"), Space), JSONString)>,
            novalidate: Option<(Space, StringEqCI!("NOVALIDATE"))>,
        },
        CreateModel {
            kw: (StringEqCI!("CREATE"), Space1, StringEqCI!("MODEL"), Space),
            if_not_exists: IfNotExists,
            name: ModelName,
            predicting: (Space, StringEqCI!("PREDICTING"), Space),
            pred: Parenthesized<(Ident, Option<(Space, Datyp)>)>,
            with_clause: Option<(Space, WithClauseCreate)>,
            source: Option<(Space, ModelSource)>,
            using_clause: Option<(Space, UsingClause)>,
        },
        CreateRole {
            kw: (StringEqCI!("CREATE"), Space1, StringEqCI!("ROLE"), Space),
            name: Ident,
        },
        CreateSchema {
            kw: (StringEqCI!("CREATE"), Space1, StringEqCI!("SCHEMA"), Space),
            if_not_exists: IfNotExists,
            name: Ident,
        },
        CreateTableAs {
            kw: (StringEqCI!("CREATE"), Space1, StringEqCI!("TABLE"), Space),
            if_not_exists: IfNotExists,
            nojourn: Option<(StringEqCI!("%NOJOURN"), Space)>,
            tname: TName,
            as_kw: (Space, StringEqCI!("AS"), Space),
            with_clause: Option<(WithClause, Space)>,
            query: Query,
            options: Option<(Space, TableOptionList)>,
        },
        CreateUser {
            kw: (StringEqCI!("CREATE"), Space1, StringEqCI!("USER"), Space),
            user: UserSpec,
        },
        CreateServer {
            kw: (StringEqCI!("CREATE"), Space1),
            server_kw: ServerKw,
            tname: TName,
            type_clause: Option<((Space, StringEqCI!("TYPE"), Space), ServerType)>,
            fdw: (
                Space,
                StringEqCI!("FOREIGN"),
                Space1,
                StringEqCI!("DATA"),
                Space1,
                StringEqCI!("WRAPPER"),
                Space,
            ),
            fdw_name: FDWName,
            connection: (Space, CreateServerConnection),
            delimited: Option<(Space, CreateServerDelimitedIds)>,
            passthrough: Option<(Space, EnableDisable, Space1, StringEqCI!("PASSTHROUGH"))>,
            pushdown: Option<(Space, FTPushdownClause)>,
            using_json: Option<((Space, StringEqCI!("USING"), Space), JSONString)>,
        },
        CreateIndex {
            kw: (StringEqCI!("CREATE"), Space1),
            partitioned: Option<(CreateIndexPartitioned, Space1)>,
            qualifiers: Option<(IndexQualifiers, Space)>,
            index_kw: (StringEqCI!("INDEX"), Space),
            name: IndexName,
            on_kw: (Space, StringEqCI!("ON"), Space),
            table_kw: Option<(StringEqCI!("TABLE"), Space)>,
            tname: TName,
            comp_list: Args<VecSep<ComponentSpec, ArgSep>>,
            as_clause: Option<(
                (Space, StringEqCI!("AS"), Space),
                ClassName,
                Option<(Space, CreateIndexAsParams)>,
            )>,
            with_clause: Option<(Space, CreateIndexWithClause)>,
            in_on: Option<(Space, InOn, (Space, Token))>,
            defer_immediate: Option<(
                Space,
                ImmediateDefer,
                Option<(Space1, StringEqCI!("BUILD"))>,
            )>,
        },
        CreateAggregate {
            kw: (StringEqCI!("CREATE"), Space1),
            or_replace: OrReplace,
            agg_kw: (StringEqCI!("AGGREGATE"), Space),
            name: QualifiedSPName,
            params: Args<Option<ParameterList>>,
            returns: Option<((Space, StringEqCI!("RETURNS"), Space), Datyp)>,
            initialize: Option<(
                (
                    Space,
                    StringEqCI!("INITIALIZE"),
                    Space1,
                    StringEqCI!("WITH"),
                    Space,
                ),
                QualifiedSPName,
            )>,
            iterate: (
                (
                    Space,
                    StringEqCI!("ITERATE"),
                    Space1,
                    StringEqCI!("WITH"),
                    Space,
                ),
                QualifiedSPName,
            ),
            merge: Option<(
                (
                    Space,
                    StringEqCI!("MERGE"),
                    Space1,
                    StringEqCI!("WITH"),
                    Space,
                ),
                QualifiedSPName,
            )>,
            finalize: Option<(
                (
                    Space,
                    StringEqCI!("FINALIZE"),
                    Space1,
                    StringEqCI!("WITH"),
                    Space,
                ),
                QualifiedSPName,
            )>,
        },
        CreateFunction {
            kw: (StringEqCI!("CREATE"), Space1),
            or_replace: OrReplace,
            func_kw: (StringEqCI!("FUNCTION"), Space),
            name: QualifiedSPName,
            spec: (Space, CodeSpecification),
        },
        CreateMlConfiguration {
            kw: (StringEqCI!("CREATE"), Space1),
            or_replace: OrReplace,
            ml_kw: (
                StringEqCI!("ML"),
                Space1,
                StringEqCI!("CONFIGURATION"),
                Space,
            ),
            name: MLConfigurationName,
            provider: ((Space, StringEqCI!("PROVIDER"), Space1), ProviderName),
            desc: Option<((Space1, StringEqCI!("%DESCRIPTION"), Space), LiteralOrNull)>,
            using_clause: Option<(Space, MlconfigUsingClause)>,
            settings: Option<(Space, ProviderSpecificSettings)>,
        },
        CreateProcedure {
            kw: (StringEqCI!("CREATE"), Space1),
            or_replace: OrReplace,
            proc_kw: (StringEqCI!("PROC"), Option<StringEqCI!("EDURE")>, Space),
            name: QualifiedSPName,
            spec: (Space, CodeSpecification),
        },
        CreateTrigger {
            kw: (StringEqCI!("CREATE"), Space1),
            or_replace: OrReplace,
            trigger_kw: (StringEqCI!("TRIGGER"), Space),
            name: TName,
            time: (Space, TriggerTime, Space1),
            events: VecSep<TriggerEvent, ArgSep>,
            order: Option<(Space, StringEqCI!("ORDER"), Space1, NonNegativeInteger)>,
            on: (Space, StringEqCI!("ON"), Space, TName),
            referencing: Option<(
                Space,
                StringEqCI!("REFERENCING"),
                Vec1<(Space, TriggerReferencing)>,
            )>,
            for_each: Option<(
                Space,
                StringEqCI!("FOR"),
                Space1,
                StringEqCI!("EACH"),
                Space1,
                TriggerOrientation,
            )>,
            when: Option<(Space, StringEqCI!("WHEN"), Space, Parenthesized<Prdct>)>,
            body: (Space, TriggerBody),
        },
        DropTrigger {
            kw: (StringEqCI!("DROP"), Space1, StringEqCI!("TRIGGER"), Space),
            if_exists: IfExists,
            name: TName,
            from: Option<((Space, StringEqCI!("FROM"), Space), TName)>,
        },
        CreateQuery {
            kw: (StringEqCI!("CREATE"), Space1),
            or_replace: OrReplace,
            query_kw: (StringEqCI!("QUERY"), Space),
            name: QualifiedSPName,
            spec: (Space, CodeSpecification),
        },
        CreateMethod {
            kw: (StringEqCI!("CREATE"), Space1),
            static_kw: Option<(StringEqCI!("STATIC"), Space1)>,
            method_kw: (StringEqCI!("METHOD"), Space),
            name: QualifiedSPName,
            spec: (Space, CodeSpecification),
        },
        CreateTable {
            kw: (StringEqCI!("CREATE"), Space1),
            temp: Option<(CreateTableTemp, Space1)>,
            table_kw: (StringEqCI!("TABLE"), Space),
            if_not_exists: IfNotExists,
            tname: TName,
            elems: Args<VecSep<Telem, ArgSep>>,
            options: Option<(Space, TableOptionList)>,
        },
        DropDatabase {
            kw: (StringEqCI!("DROP"), Space1, StringEqCI!("DATABASE"), Space1),
            if_exists: IfExists,
            name: DatabaseName,
            retain: Option<(Space1, StringEqCI!("RETAIN_FILES"))>,
        },
        DropForeignTable {
            kw: (
                StringEqCI!("DROP"),
                Space1,
                StringEqCI!("FOREIGN"),
                Space1,
                StringEqCI!("TABLE"),
                Space,
            ),
            if_exists: IfExists,
            tname: TName,
            cascade: Option<(Space, CascadeOpt)>,
        },
        DropFunction {
            kw: (StringEqCI!("DROP"), Space1, StringEqCI!("FUNCTION"), Space),
            if_exists: IfExists,
            name: QualifiedSPName,
            from: Option<((Space, StringEqCI!("FROM"), Space), ClassName)>,
        },
        DropIndexDot {
            kw: (StringEqCI!("DROP"), Space1, StringEqCI!("INDEX"), Space),
            if_exists: IfExists,
            nojourn: Option<(StringEqCI!("%NOJOURN"), Space)>,
            tname: Ident,
            name: Vec1<((Space, StringEq!("."), Space), Ident)>,
        },
        DropIndexOn {
            kw: (StringEqCI!("DROP"), Space1, StringEqCI!("INDEX"), Space),
            if_exists: IfExists,
            name: IndexName,
            on: Option<(
                (Space, StringEqCI!("ON"), Space),
                Option<(StringEqCI!("TABLE"), Space)>,
                TName,
            )>,
        },
        DropMethod {
            kw: (StringEqCI!("DROP"), Space1, StringEqCI!("METHOD"), Space),
            if_exists: IfExists,
            name: QualifiedSPName,
            from: Option<((Space, StringEqCI!("FROM"), Space), ClassName)>,
        },
        DropMlConfiguration {
            kw: (
                StringEqCI!("DROP"),
                Space1,
                StringEqCI!("ML"),
                Space1,
                StringEqCI!("CONFIGURATION"),
                Space,
            ),
            if_exists: IfExists,
            name: MLConfigurationName,
        },
        DropModel {
            kw: (StringEqCI!("DROP"), Space1, StringEqCI!("MODEL"), Space),
            if_exists: IfExists,
            name: ModelName,
        },
        DropProcedure {
            kw: (
                StringEqCI!("DROP"),
                Space1,
                StringEqCI!("PROC"),
                Option<StringEqCI!("EDURE")>,
                Space,
            ),
            if_exists: IfExists,
            name: QualifiedSPName,
            from: Option<((Space, StringEqCI!("FROM"), Space), ClassName)>,
        },
        DropQuery {
            kw: (StringEqCI!("DROP"), Space1, StringEqCI!("QUERY"), Space),
            if_exists: IfExists,
            name: QualifiedSPName,
            from: Option<((Space, StringEqCI!("FROM"), Space), ClassName)>,
        },
        DropRole {
            kw: (StringEqCI!("DROP"), Space1, StringEqCI!("ROLE"), Space),
            if_exists: IfExists,
            name: Ident,
        },
        DropSchema {
            kw: (StringEqCI!("DROP"), Space1, StringEqCI!("SCHEMA"), Space),
            if_exists: IfExists,
            name: Ident,
            cascade_restrict: Option<(Space, CascadeOpt)>,
        },
        DropTable {
            kw: (StringEqCI!("DROP"), Space1, StringEqCI!("TABLE"), Space),
            if_exists: IfExists,
            nojourn: Option<(StringEqCI!("%NOJOURN"), Space)>,
            tname: TName,
            cascade: Option<(Space, CascadeOpt)>,
            data: Option<(Space, DataOption)>,
        },
        DropUser {
            kw: (StringEqCI!("DROP"), Space1, StringEqCI!("USER"), Space),
            if_exists: IfExists,
            name: Ident,
            cascade: Option<(Space, StringEqCI!("CASCADE"))>,
        },
        DropView {
            kw: (StringEqCI!("DROP"), Space1, StringEqCI!("VIEW"), Space),
            if_exists: IfExists,
            tname: TName,
            cascade: Option<(Space, CascadeOpt)>,
        },
        DropServer {
            kw: (StringEqCI!("DROP"), Space1),
            server_kw: ServerKw,
            tname: TName,
            cascade: Option<(Space, CascadeOpt)>,
        },
        DropAggregate {
            kw: (StringEqCI!("DROP"), Space1, StringEqCI!("AGGREGATE"), Space),
            if_exists: IfExists,
            name: QualifiedSPName,
        },
        FreezePlans {
            kw: (StringEqCI!("FREEZE"), Space1, StringEqCI!("PLANS")),
            by: Option<((Space1, StringEqCI!("BY"), Space1), FreezeScope)>,
        },
        Load {
            kw: (StringEqCI!("LOAD"), Space),
            spec: LoadSpec,
        },
        PurgeQueries {
            kw: (StringEqCI!("PURGE"), Space1),
            cached: Option<(StringEqCI!("CACHED"), Space1)>,
            queries: StringEqCI!("QUERIES"),
            by: Option<((Space1, StringEqCI!("BY"), Space1), PurgeScope)>,
        },
        TrainModel {
            kw: (StringEqCI!("TRAIN"), Space1, StringEqCI!("MODEL"), Space),
            name: ModelName,
            as_name: Option<((Space, StringEqCI!("AS"), Space), ModelName)>,
            not_default: Option<(Space, StringEqCI!("NOT"), Space1, StringEqCI!("DEFAULT"))>,
            for_col: Option<((Space, StringEqCI!("FOR"), Space), ColumnDefItem)>,
            with_clause: Option<(Space, WithClauseTrain)>,
            source: Option<(Space, ModelSource)>,
            using_clause: Option<(Space, UsingClause)>,
        },
        TuneTable {
            kw: (StringEqCI!("TUNE"), Space1, StringEqCI!("TABLE"), Space),
            tname: TName,
            options: Vec<(Space, TuneOption)>,
        },
        UnfreezePlans {
            kw: (StringEqCI!("UNFREEZE"), Space1, StringEqCI!("PLANS")),
            from_upgrade: Option<(
                Space1,
                Option<(StringEqCI!("FROM"), Space1)>,
                StringEqCI!("UPGRADE"),
            )>,
            by: Option<((Space1, StringEqCI!("BY"), Space1), FreezeScope)>,
        },
        UseDatabase {
            kw: (StringEqCI!("USE"), Space1),
            database: Option<(StringEqCI!("DATABASE"), Space1)>,
            name: DatabaseName,
        },
        ValidateModel {
            kw: (StringEqCI!("VALIDATE"), Space1, StringEqCI!("MODEL"), Space),
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
        pub space: Space1,
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
            options: Option<Args<DBServerOptions>>,
        },
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum CreateServerDelimitedIds {
        Delimited(StringEqCI!("DELIMITEDIDS")),
        NoDelimited(StringEqCI!("NODELIMITEDIDS")),
    }

    #[derive(Grammar)]
    #[tygr(name = "ENABLE/DISABLE")]
    pub enum EnableDisable {
        Enable(StringEqCI!("ENABLE")),
        Disable(StringEqCI!("DISABLE")),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum CreateIndexAsParams {
        Parens(Parenthesized<VecSep<IndexParameterSpec, ArgSep>>),
        With {
            kw: (StringEqCI!("WITH"), Space),
            params: VecSep<IndexParameterSpec, ArgSep>,
        },
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct CreateIndexWithClause {
        pub kw: (StringEqCI!("WITH"), Space),
        pub data: Option<(StringEqCI!("DATA"), Space, FList1)>,
        pub pad_index: IndexWithItem<StringEqCI!("PAD_INDEX")>,
        pub fillfactor: IndexWithItem<(
            StringEqCI!("FILLFACTOR"),
            (Space, StringEq!("="), Space),
            Literal,
        )>,
        pub ignore_dup: IndexWithItem<StringEqCI!("IGNORE_DUP_KEY")>,
        pub drop_existing: IndexWithItem<StringEqCI!("DROP_EXISTING")>,
        pub stats_norecompute: IndexWithItem<StringEqCI!("STATISTICS_NORECOMPUTE")>,
    }

    /// An item of an index's WITH clause, after a blank and an optional comma.
    pub type IndexWithItem<K> = Option<(Space, Option<(StringEq!(","), Space)>, K)>;

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

    #[derive(Grammar)]
    pub enum Password {
        Ident(Ident),
        Literal(Literal),
        NULL(StringEqCI!("NULL")),
    }

    #[derive(Grammar)]
    pub enum PWPrefix {
        IdentifiedBy(StringEqCI!("IDENTIFIED"), Space1, StringEqCI!("BY")),
        IdentifyBy(StringEqCI!("IDENTIFY"), Space1, StringEqCI!("BY")),
        WithPassword(
            Option<(StringEqCI!("WITH"), Space1)>,
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
            name: VecSep<Ident, (Space, StringEq!("."), Space)>,
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
            names: VecSep<VecSep<Ident, (Space, StringEq!("."), Space)>, ArgSep>,
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
        Prefix<(StringEqCI!("IDENTIFIED"), Space1, StringEqCI!("BY"), Space), ValSpec>;

    pub type WithClauseCreate = Prefix<(StringEqCI!("WITH"), Space), Parenthesized<ColumnDefList>>;

    #[derive(Grammar)]
    #[tygr(name = "model-source")]
    pub enum ModelSource {
        Subquery {
            kw: (StringEqCI!("FROM"), Space),
            selex: Parenthesized<Src<Selex>>,
        },
        Inline {
            from: FromClause,
            where_clause: Option<(Space, WhereClause)>,
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
        List(FList1),
        Assignment(WithValueAssignment),
    }

    #[derive(Grammar)]
    #[tygr(name = "with-value-assignment")]
    pub struct WithValueAssignment(
        pub VecSep<(Ident, (Space, StringEq!("="), Space), WithExpression), ArgSep>,
    );

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
        pub trained_model: Option<(Space1, StringEqCI!("TRAINED"), Space1, StringEqCI!("MODEL"))>,
        pub name: (Space, TrainedModelName),
    }

    #[derive(Grammar)]
    #[tygr(name = "scope")]
    pub enum Scope {
        Days(Integer, (Space1, StringEqCI!("DAYS"))),
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
    pub struct DROption {
        pub name: Either<StringEqCI!("APITOKEN"), StringEqCI!("URL")>,
        pub eq: Option<(Space, StringEq!("="))>,
        pub value: (Space, LiteralOrNull),
    }

    #[derive(Grammar)]
    #[tygr(name = "H2O option")]
    pub struct H2OOption {
        pub kw: StringEqCI!("JAVATLSCONFIGFILE"),
        pub eq: Option<(Space, StringEq!("="))>,
        pub val: (Space, LiteralOrNull),
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

    pub type FTPushdownClause = Prefix<StringEqCI!("PUSHDOWN"), (Space1, FTPushdownType)>;

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
            of: Option<(Space1, StringEqCI!("OF"), Space, VecSep<Ident, ArgSep>)>,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "trigger referencing")]
    pub struct TriggerReferencing {
        pub old_new: Either<StringEqCI!("OLD"), StringEqCI!("NEW")>,
        pub row: Option<(Space1, StringEqCI!("ROW"))>,
        pub as_kw: Option<(Space1, StringEqCI!("AS"))>,
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
            lang: Option<(StringEqCI!("LANGUAGE"), Space1, StringEqCI!("SQL"), Space)>,
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
        pub database: (Space1, DatabaseName),
        pub activate: Option<(Space1, SQLActivateSpec)>,
    }

    #[derive(Grammar)]
    #[tygr(name = "activate spec")]
    pub struct SQLActivateSpec {
        pub force_or_defer: Option<(Either<StringEqCI!("FORCE"), StringEqCI!("DEFER")>, Space1)>,
        pub activate_kw: StringEqCI!("ACTIVATE"),
        pub mappings_kw: Option<(Space1, StringEqCI!("MAPPINGS"))>,
    }

    #[derive(Grammar)]
    #[tygr(name = "cre or alt")]
    pub enum CreOrAlt {
        Alter(StringEqCI!("ALTER")),
        Create(
            StringEqCI!("CREATE"),
            Option<(Space1, StringEqCI!("OR"), Space1, StringEqCI!("REPLACE"))>,
        ),
    }

    #[derive(Grammar)]
    #[tygr(name = "view with")]
    pub enum ViewWith {
        ReadOnly(StringEqCI!("READ"), Space1, StringEqCI!("ONLY")),
        Check {
            levels: Option<(LevelsClause, Space1)>,
            kw: (StringEqCI!("CHECK"), Space1, StringEqCI!("OPTION")),
            constraint: Option<(Space1, StringEqCI!("CONSTRAINT"), Space, Ident)>,
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
        pub view_column_list: Option<(FList1, Option<(Space, StringEqCI!("AS"))>, Space)>,
        pub stat: Src<Stat>,
        pub space_after: Space,
    }

    #[derive(Grammar)]
    pub struct Thr {
        pub kw: (
            StringEqCI!("THROUGH"),
            Space1,
            StringEqCI!("FOREIGN"),
            Space1,
            StringEqCI!("SERVER"),
            Space,
        ),
        pub tname: TName,
        pub remote_statement: (Space, WithStat<Selct>),
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
    pub struct DatabaseName {
        pub head: DbIdentHead,
        pub rest: Vec<IsDbIdentPart>,
    }

    #[derive(Grammar)]
    pub enum DbIdentHead {
        Percent(StringEq!("%"), IsDbIdentPart),
        Normal(IsDbIdentStart),
    }

    #[derive(Grammar)]
    #[tygr(name = "code specification")]
    pub struct CodeSpecification {
        pub params: Parenthesized<ParameterList>,
        pub characteristics: Vec<(Space, MethodQueryCharacteristics)>,
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
    pub struct DefaultClause {
        pub default_kw: Option<(StringEqCI!("DEFAULT"), Space)>,
        pub value: Scalx,
    }

    #[derive(Grammar)]
    #[tygr(name = "method-query characteristics")]
    pub enum MethodQueryCharacteristics {
        ContainId {
            kw: (StringEqCI!("CONTAINID"), Space),
            value: Integer,
        },
        Final(StringEqCI!("FINAL")),
        For {
            kw: (StringEqCI!("FOR"), Space),
            classname: Classname,
        },
        Private(StringEqCI!("PRIVATE")),
        Proc {
            kw: StringEqCI!("PROC"),
            edure: Option<StringEqCI!("EDURE")>,
        },
        Results {
            kw: (StringEqCI!("RESULTS"), Space),
            clause: ResultSetClause,
        },
        Returns {
            kw: (StringEqCI!("RETURNS"), Space),
            datyp: Datyp,
        },
        SelectMode {
            kw: (StringEqCI!("SELECTMODE"), Space1),
            mode: SelectMode,
        },
        ResultSets {
            dynamic: Option<(StringEqCI!("DYNAMIC"), Space1)>,
            kw: (StringEqCI!("RESULT"), Space1, StringEqCI!("SETS")),
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
                Space1,
                Either<StringEqCI!("OBJECTSCRIPT"), StringEqCI!("COS")>,
                Space,
            ),
            body: Either<COSMethodBody, COSQueryBody>,
        },
        Python {
            kw: (
                StringEqCI!("LANGUAGE"),
                Space1,
                StringEqCI!("PYTHON"),
                Space,
            ),
            body: Prefix<StringEq!("{"), StringEndsWith<StringEq!("}")>>,
        },
        Sql {
            kw: Option<(StringEqCI!("LANGUAGE"), Space1, StringEqCI!("SQL"), Space1)>,
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
        pub value: Option<(Space, ReturnValue)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum ReturnValue {
        HostRef(HostRef),
        Literal(Literal),
    }

    #[derive(Grammar)]
    #[tygr(name = "COS query body")]
    pub struct COSQueryBody(pub VecSep<QueryMethod, Space>);

    #[derive(Grammar)]
    #[tygr(name = "query method")]
    pub struct QueryMethod {
        pub kind: QueryMethodKind,
        pub params: Args<ParameterList>,
        pub body: (Space, COSMethodBody),
    }

    #[derive(Grammar)]
    pub enum QueryMethodKind {
        Close(StringEqCI!("CLOSE")),
        Execute(StringEqCI!("EXECUTE")),
        FetchOdbc(StringEqCI!("FETCHODBC")),
        FetchRows(StringEqCI!("FETCHROWS")),
        Fetch(StringEqCI!("FETCH")),
        GetInfo(StringEqCI!("GETINFO")),
    }

    pub type COSMethodBody = crate::cos::Brace<crate::cos::OSText>;

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
        pub collate: Option<(Space, StringEqCI!("COLLATE"), Space1, SqlCollation)>,
        pub direction: Option<(Space, ComponentDirection)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum ComponentDirection {
        Asc(StringEqCI!("ASC")),
        Desc(StringEqCI!("DESC")),
    }

    pub type CollectionSpec = Parenthesized<Either<StringEqCI!("ELEMENTS"), StringEqCI!("KEYS")>>;

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
            kw: (
                Space1,
                StringEqCI!("CONSTRAINT"),
                Space1,
                StringEqCI!("ALL"),
            ),
        },
        DropAllPartitions {
            drop_kw: DropKeywd,
            kw: (
                Space1,
                StringEqCI!("ALL"),
                Space1,
                StringEqCI!("PARTITIONS"),
            ),
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
            kw: (StringEqCI!("ADD"), Space),
            actions: Either<Parenthesized<AddActionList>, AddActionList>,
        },
        Alter {
            kw: StringEqCI!("ALTER"),
            column_kw: Option<(Space1, StringEqCI!("COLUMN"))>,
            name: (Space, Ident),
            alter: (Space, AlterCol),
        },
        Cleanup(StringEqCI!("CLEANUP")),
        Convert {
            kw: (StringEqCI!("CONVERT"), Space1),
            clause: ConvertClause,
        },
        LockMode {
            kw: (StringEqCI!("LOCK"), Space1, StringEqCI!("MODE"), Space),
            spec: Parenthesized<LockSpec>,
        },
        Modify {
            kw: (StringEqCI!("MODIFY"), Space),
            specs: Either<Parenthesized<ModSpecList>, ModSpecList>,
        },
        Move {
            kw: (StringEqCI!("MOVE"), Space1),
            spec: MoveSpec,
        },
        Rename {
            kw: (StringEqCI!("RENAME"), Space),
            name: Ident,
        },
        Truncate {
            kw: (StringEqCI!("TRUNCATE"), Space1),
            partition: PartitionRangeSpec,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "convert clause")]
    pub enum ConvertClause {
        Partition(PartitionConvertClause),
        StorageDefault {
            clause: StorageDefaultConvertClause,
            del: Option<(Space1, DataOption)>,
        },
        Shard {
            to: Option<(StringEqCI!("TO"), Space1)>,
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
    pub struct PartitionConvertClause {
        pub kw: StringEqCI!("PARTITION"),
        pub spec: Option<(
            Space1,
            Either<Prefix<(StringEqCI!("BY"), Space1), PartitionSpecificationList>, PartitionOnOff>,
        )>,
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
            collate: Option<(Space, StringEqCI!("COLLATE"), Space1, SqlCollation)>,
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
        DropDefault(StringEqCI!("DROP"), Space1, StringEqCI!("DEFAULT")),
        NotNull(StringEqCI!("NOT"), Space1, StringEqCI!("NULL")),
        Null(StringEqCI!("NULL")),
        Rename {
            kw: (StringEqCI!("RENAME"), Space),
            name: Ident,
        },
        SetDefault {
            kw: (StringEqCI!("SET"), Space1, StringEqCI!("DEFAULT"), Space),
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
        pub to: Option<(StringEqCI!("TO"), Space1)>,
        pub kw: (StringEqCI!("STORAGETYPE"), Space, StringEq!("="), Space),
        pub typ: Storagetype,
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
            kw: StringEqCI!("GETDATE"),
            value: Parenthesized<Option<Literal>>,
        },
        GetUtcDate {
            kw: StringEqCI!("GETUTCDATE"),
            value: Parenthesized<Option<Literal>>,
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
    pub struct ModSpec {
        pub name: Ident,
        pub change: (Space, ModChange),
    }

    #[derive(Grammar)]
    pub enum ModChange {
        Rename {
            kw: (StringEqCI!("RENAME"), Space),
            new_name: Ident,
        },
        Modify {
            datyp: Option<(Datyp, Space)>,
            mod_col: ModCol,
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
        pub not: Option<(StringEqCI!("NOT"), Space1)>,
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
            kw: (StringEqCI!("FOREIGN"), Space1, StringEqCI!("KEY"), Space),
            name: Ident,
        },
        PrimaryKey(StringEqCI!("PRIMARY"), Space1, StringEqCI!("KEY")),
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
        pub kw: (StringEqCI!("PRIMARY"), Space1, StringEqCI!("KEY"), Space),
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
        pub kw: (StringEqCI!("FOREIGN"), Space1, StringEqCI!("KEY"), Space),
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

    pub type Identity = Prefix<StringEqCI!("IDENTITY"), Option<Args<(Literal, ArgSep, Literal)>>>;

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
        Space1,
        StringEqCI!("FILLFACTOR"),
        Space,
        StringEq!("="),
        Space,
        Literal,
    )>;

    pub type TblPrimaryKey =
        Prefix<(StringEqCI!("PRIMARY"), Space1, StringEqCI!("KEY"), Space), TblUniqueSpec>;

    #[derive(Grammar)]
    #[tygr(name = "extentsize clause")]
    pub struct ExtentsizeClause {
        pub kw: (
            Either<StringEqCI!("%EXTENTSIZE"), StringEqCI!("%NUMROWS")>,
            Space,
        ),
        pub value: Literal,
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
        pub with: Option<(Space1, StringEqCI!("WITH"))>,
        pub name: Either<Args<TName>, (Space, TName)>,
    }

    #[derive(Grammar)]
    #[tygr(name = "table_option_list")]
    pub enum TableOptionList {
        WithOptions {
            shard: Option<(Shard, Space)>,
            partition: Option<(PartitionOption, Space)>,
            with_kw: (StringEqCI!("WITH"), Space1),
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
            Space1,
            StringEqCI!("SIZE"),
            Space,
            Literal,
        )>,
        pub next_size: Option<(
            Space,
            StringEqCI!("NEXT"),
            Space1,
            StringEqCI!("SIZE"),
            Space,
            Literal,
        )>,
        pub lock_mode: Option<(
            Space,
            StringEqCI!("LOCK"),
            Space1,
            StringEqCI!("MODE"),
            Option<(Space1, LockSpec)>,
        )>,
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
    pub struct PartitionOption {
        pub kw: (StringEqCI!("PARTITION"), Space1),
        pub spec: Either<
            Prefix<(StringEqCI!("BY"), Space1), PartitionSpecificationList>,
            StringEqCI!("OFF"),
        >,
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
            kw: (StringEqCI!("HASH"), Space),
            column: Parenthesized<ColumnName>,
            in_kw: (Space, StringEqCI!("IN"), Space),
            count: PositiveInteger,
            partitions_kw: (Space, StringEqCI!("PARTITIONS")),
        },
        List {
            kw: (StringEqCI!("LIST"), Space),
            column: Parenthesized<ColumnName>,
        },
        Range {
            kw: (StringEqCI!("RANGE"), Space),
            column: Parenthesized<ColumnName>,
            interval_kw: (Space, StringEqCI!("INTERVAL"), Space),
            interval: PositiveInteger,
            datetime_interval: Option<(Space, PartitionDatetimeInterval, Option<StringEqCI!("S")>)>,
        },
    }

    #[derive(Grammar)]
    #[tygr(name = "partition datetime interval")]
    pub enum PartitionDatetimeInterval {
        Day(StringEqCI!("DAY")),
        Hour(StringEqCI!("HOUR")),
        Minute(StringEqCI!("MINUTE")),
        Month(StringEqCI!("MONTH")),
        Year(StringEqCI!("YEAR")),
    }

    #[derive(Grammar)]
    #[tygr(name = "partition range spec")]
    pub struct PartitionRangeSpec {
        pub kw: StringEqCI!("PARTITION"),
        pub range: PartitionRange,
    }

    #[derive(Grammar)]
    pub enum PartitionRange {
        Id {
            kw: (Space, StringEqCI!("ID"), Space),
            prefix: Literal,
        },
        IdBetween {
            kw: (Space, StringEqCI!("ID")),
            prefix: Option<(Space, Literal)>,
            between_kw: (Space, StringEqCI!("BETWEEN"), Space),
            from: Literal,
            and_kw: (Space, StringEqCI!("AND"), Space),
            to: Literal,
        },
        Between {
            prefix: Option<(Space, PartitionPrefix)>,
            between_kw: (Space, StringEqCI!("BETWEEN"), Space),
            from: Literal,
            and_kw: (Space, StringEqCI!("AND"), Space),
            to: Literal,
        },
        Values(Space, PartitionPrefix),
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
            kw: (StringEqCI!("CHARACTER"), Space1, StringEqCI!("SET"), Space),
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
            kw: (StringEqCI!("ON"), Space1, StringEqCI!("UPDATE"), Space),
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
            arg: Option<Args<Literal>>,
        },
        PctSqupper {
            kw: StringEqCI!("%SQLUPPER"),
            arg: Option<Args<Literal>>,
        },
        PctString {
            kw: StringEqCI!("%STRING"),
            arg: Option<Args<Literal>>,
        },
        PctTruncate {
            kw: StringEqCI!("%TRUNCATE"),
            arg: Option<Args<Literal>>,
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
            arg: Option<Args<Literal>>,
        },
        Sqlupper {
            kw: StringEqCI!("SQLUPPER"),
            arg: Option<Args<Literal>>,
        },
        StringColl {
            kw: StringEqCI!("STRING"),
            arg: Option<Args<Literal>>,
        },
        Truncate {
            kw: StringEqCI!("TRUNCATE"),
            arg: Option<Args<Literal>>,
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
        pub args: Option<Args<DatypArgs>>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct DatypArgs(pub StringOf<IsNotParen>);

    #[derive(Grammar)]
    #[tygr(name = "datyp spec")]
    pub enum DatypSpec {
        HashParen(Parenthesized<Vec<(StringEq!("#"), Space)>>),
        Binary {
            kw: StringEqCI!("BINARY"),
            varying: Option<(Space1, StringEqCI!("VARYING"))>,
        },
        Bit(StringEqCI!("BIT")),
        Character {
            kw: StringEqCI!("CHARACTER"),
            varying: Option<(Space1, StringEqCI!("VARYING"))>,
        },
        Char {
            kw: StringEqCI!("CHAR"),
            varying: Option<(Space1, StringEqCI!("VARYING"))>,
        },
        Datetime(StringEqCI!("DATETIME")),
        Date(StringEqCI!("DATE")),
        Decimal(StringEqCI!("DECIMAL")),
        Dec(StringEqCI!("DEC")),
        Double {
            kw: StringEqCI!("DOUBLE"),
            precision: Option<(Space1, StringEqCI!("PRECISION"))>,
        },
        Float(StringEqCI!("FLOAT")),
        Image(StringEqCI!("IMAGE")),
        Integer(StringEqCI!("INTEGER")),
        Int(StringEqCI!("INT")),
        LongVarchar(StringEqCI!("LONG"), Space1, StringEqCI!("VARCHAR")),
        Longvarbinary(StringEqCI!("LONGVARBINARY")),
        Longvarchar(StringEqCI!("LONGVARCHAR")),
        Long(StringEqCI!("LONG")),
        Money(StringEqCI!("MONEY")),
        NationalVarchar(StringEqCI!("NATIONAL"), Space1, StringEqCI!("VARCHAR")),
        NationalChar {
            kw: StringEqCI!("NATIONAL"),
            char_kw: Option<(
                Space1,
                Either<StringEqCI!("CHARACTER"), StringEqCI!("CHAR")>,
                Option<(Space1, StringEqCI!("VARYING"))>,
            )>,
        },
        Nchar(StringEqCI!("NCHAR")),
        Ntext(StringEqCI!("NTEXT")),
        Number(StringEqCI!("NUMBER")),
        NumericRaw(StringEqCI!("NUMERIC"), Space1, StringEqCI!("RAW")),
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
            args: Option<Args<(Literal, Option<(ArgSep, Literal)>)>>,
        },
        NotNull {
            kw: (StringEqCI!("NOT"), Space1, StringEqCI!("NULL")),
            informix: Informix,
        },
        Null(StringEqCI!("NULL")),
        Primary {
            kw: (
                StringEqCI!("PRIMARY"),
                Option<(Space1, StringEqCI!("KEY"))>,
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
        NotNull(StringEqCI!("NOT"), Space1, StringEqCI!("NULL")),
        PrimaryKey {
            kw: (StringEqCI!("PRIMARY"), Space1, StringEqCI!("KEY"), Space),
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
            kw: StringEqCI!("GETDATE"),
            prec: Parenthesized<Option<Precision>>,
        },
        GetUtcDate {
            kw: StringEqCI!("GETUTCDATE"),
            prec: Parenthesized<Option<Precision>>,
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
        pub match_spec: Option<(Space, StringEqCI!("MATCH"), Space1, MatchSpec)>,
        pub triggered_action: Option<(Space, TriggeredAction)>,
        pub nocheck: Option<(Space, StringEqCI!("NOCHECK"))>,
    }

    /// `<triggered action> ::= <delete rule> [ <update rule> ] | <update rule> [ <delete rule> ]`
    #[derive(Grammar)]
    #[tygr(name = "triggered action")]
    pub enum TriggeredAction {
        DeleteFirst {
            delete_rule: DeleteRule,
            update_rule: Option<(Space1, UpdateRule)>,
        },
        UpdateFirst {
            update_rule: UpdateRule,
            delete_rule: Option<(Space1, DeleteRule)>,
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
        Prefix<(StringEqCI!("ON"), Space1, StringEqCI!("UPDATE"), Space1), RefAction>;

    pub type DeleteRule =
        Prefix<(StringEqCI!("ON"), Space1, StringEqCI!("DELETE"), Space1), RefAction>;

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
        NoAction(StringEqCI!("NO"), Space1, StringEqCI!("ACTION")),
        SetDefault(StringEqCI!("SET"), Space1, StringEqCI!("DEFAULT")),
        SetNull(StringEqCI!("SET"), Space1, StringEqCI!("NULL")),
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
            kw: (StringEqCI!("SQL"), Space1),
            spec: LoadSqlSpec,
        },
        Data {
            hints: Option<(LoadHints, Space)>,
            kw: (StringEqCI!("DATA"), Space1),
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

    pub type LoadDataFrom = Prefix<(StringEqCI!("FROM"), Space1), DataSource>;

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
            name: VecSep<Ident, StringEq!(".")>,
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
        pub kw: (StringEqCI!("FROM"), Space1),
        pub source: SqlSource,
        pub dialect: Option<(Space, StringEqCI!("DIALECT"), Space, Literal)>,
        pub delimiter: Option<(Space, StringEqCI!("DELIMITER"), Space, Literal)>,
        pub verbose: Option<(Space, StringEqCI!("VERBOSE"))>,
        pub log_to_file: Option<(Space, LoadSqlLogToFile)>,
        pub ignore_errors: Option<(Space, StringEqCI!("IGNORE"), Space1, StringEqCI!("ERRORS"))>,
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

    pub type LoadSqlLogToFile = Prefix<
        (
            StringEqCI!("LOG"),
            Space1,
            StringEqCI!("TO"),
            Space1,
            StringEqCI!("FILE"),
        ),
        Option<(Space, Literal)>,
    >;

    #[derive(Grammar)]
    #[tygr(name = "alter-server-action")]
    pub enum AlterServerAction {
        Alter {
            kw: (StringEqCI!("ALTER"), Space1),
            action: SubAlterServerAction,
        },
        Modify {
            kw: (StringEqCI!("MODIFY"), Space1),
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

/// The spellings only the full level has, which `leveled` imports for its sugar variants.
#[allow(clippy::upper_case_acronyms)]
mod sugar {
    use either::Either;
    use tygr::*;

    use super::*;

    #[derive(Grammar, Clone)]
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

    #[derive(Grammar, Clone)]
    #[tygr(name = "%string operation")]
    pub struct StringOperation {
        pub op: StringOperator,
        pub args: Args<(Scalx, ArgSep, ColTrun)>,
    }

    #[derive(Grammar, Clone)]
    #[tygr(name = "%string operator")]
    pub enum StringOperator {
        String(StringEqCI!("%STRING")),
        SqlString(StringEqCI!("%SQLSTRING")),
        SqlUpper(StringEqCI!("%SQLUPPER")),
        Truncate(StringEqCI!("%TRUNCATE")),
    }

    #[derive(Grammar, Clone)]
    pub enum ColTrun {
        Const(PConst),
        HostVar(HostVar),
    }

    /// The functions a call by name spells, which the core reads as calls.
    #[derive(Grammar, Clone)]
    #[tygr(name = "scalar function")]
    pub enum ScalarFunction {
        PercentObject(StringEqCI!("%OBJECT"), Args<Scalx>),
        PercentOid(StringEqCI!("%OID"), Args<Scalx>),
        Similarity(StringEqCI!("%SIMILARITY"), Args<RR>),
        Abs(StringEqCI!("ABS"), Args<Scalx>),
        Ascii(StringEqCI!("ASCII"), Args<Scalx>),
        Ceiling(StringEqCI!("CEILING"), Args<Scalx>),
        CharacterLength(StringEqCI!("CHARACTER_LENGTH"), Args<Scalx>),
        CharIndex(StringEqCI!("CHARINDEX"), Args<RRO>),
        CharLength(StringEqCI!("CHAR_LENGTH"), Args<Scalx>),
        Coalesce(StringEqCI!("COALESCE"), Args1Plus),
        Convert(StringEqCI!("CONVERT"), Args<RRO>),
        DataLength(StringEqCI!("DATALENGTH"), Args<Scalx>),
        Date(StringEqCI!("DATE"), Args<Scalx>),
        Day(StringEqCI!("DAY"), Args<Scalx>),
        Decode(StringEqCI!("DECODE"), Args1Plus),
        Embedding(StringEqCI!("EMBEDDING"), Args<RO>),
        Floor(StringEqCI!("FLOOR"), Args<Scalx>),
        GetDate(StringEqCI!("GETDATE"), Args<Option<Scalx>>),
        GetUtcDate(StringEqCI!("GETUTCDATE"), Args<Option<Scalx>>),
        Greatest(StringEqCI!("GREATEST"), Args1Plus),
        HashBytes(StringEqCI!("HASHBYTES"), Args<RR>),
        IfNull(StringEqCI!("IFNULL"), Args<RRO>),
        Instr(StringEqCI!("INSTR"), Args<RROO>),
        IsNull(StringEqCI!("ISNULL"), Args<RR>),
        IsNumeric(StringEqCI!("ISNUMERIC"), Args<Scalx>),
        LastDay(StringEqCI!("LAST_DAY"), Args<Scalx>),
        LastIdentity(StringEqCI!("LAST_IDENTITY"), Args<()>),
        LCase(StringEqCI!("LCASE"), Args<Scalx>),
        Least(StringEqCI!("LEAST"), Args1Plus),
        Left(StringEqCI!("LEFT"), Args<RR>),
        Len(StringEqCI!("LEN"), Args<Scalx>),
        Length(StringEqCI!("LENGTH"), Args<Scalx>),
        Lower(StringEqCI!("LOWER"), Args<Scalx>),
        LPad(StringEqCI!("LPAD"), Args<RRO>),
        LTrim(StringEqCI!("LTRIM"), Args<Scalx>),
        Month(StringEqCI!("MONTH"), Args<Scalx>),
        Now(StringEqCI!("NOW"), Args<()>),
        NullIf(StringEqCI!("NULLIF"), Args<RR>),
        Nvl(StringEqCI!("NVL"), Args<RR>),
        PatIndex(StringEqCI!("PATINDEX"), Args<RR>),
        Position(StringEqCI!("POSITION"), Args<SQLPositionArgs>),
        Power(StringEqCI!("POWER"), Args<RR>),
        Repeat(StringEqCI!("REPEAT"), Args<RR>),
        Replace(StringEqCI!("REPLACE"), Args<RRR>),
        Replicate(StringEqCI!("REPLICATE"), Args<RR>),
        Reverse(StringEqCI!("REVERSE"), Args<Scalx>),
        Right(StringEqCI!("RIGHT"), Args<RR>),
        Round(StringEqCI!("ROUND"), Args<RR>),
        RPad(StringEqCI!("RPAD"), Args<RRO>),
        RTrim(StringEqCI!("RTRIM"), Args<Scalx>),
        Sign(StringEqCI!("SIGN"), Args<Scalx>),
        Space(StringEqCI!("SPACE"), Args<Scalx>),
        Str(StringEqCI!("STR"), Args<ROO>),
        String(StringEqCI!("STRING"), Args1Plus),
        Stuff(StringEqCI!("STUFF"), Args<RRRR>),
        Substr(StringEqCI!("SUBSTR"), Args<RRO>),
        Substring(StringEqCI!("SUBSTRING"), SubstringArgs),
        ToChar(StringEqCI!("TOCHAR"), Args<RO>),
        ToDate(StringEqCI!("TODATE"), Args<RO>),
        ToUSChar(StringEqCI!("TO_CHAR"), Args<RO>),
        ToUSDate(StringEqCI!("TO_DATE"), Args<RO>),
        ToNumber(StringEqCI!("TO_NUMBER"), Args<Src<Term>>),
        ToTimestamp(StringEqCI!("TO_TIMESTAMP"), Args<RO>),
        UCase(StringEqCI!("UCASE"), Args<Scalx>),
        UnixTimestamp(StringEqCI!("UNIX_TIMESTAMP"), Args<Option<Scalx>>),
        VectorCosine(StringEqCI!("VECTOR_COSINE"), Args<RR>),
        VectorDotProduct(StringEqCI!("VECTOR_DOT_PRODUCT"), Args<RR>),
        Year(StringEqCI!("YEAR"), Args<Scalx>),
    }

    #[derive(Grammar, Clone)]
    #[tygr(inline)]
    pub struct SQLPositionArgs {
        pub needle: Scalx,
        pub in_kw: (Space, StringEqCI!("IN"), Space),
        pub haystack: Scalx,
    }

    pub type SubstringArgs = Args<SubstringArgsInternal>;

    #[derive(Grammar, Clone)]
    #[tygr(name = "substring args")]
    pub struct SubstringArgsInternal {
        pub expr: Scalx,
        pub rest: Either<
            (
                (Space, StringEqCI!("FROM"), Space),
                Scalx,
                Option<((Space, StringEqCI!("FOR"), Space), Scalx)>,
            ),
            (ArgSep, Scalx, Option<(ArgSep, Scalx)>),
        >,
    }

    pub type ODBCEscape = Wrap<(StringEq!("{"), Space), DTS, (Space, StringEq!("}"))>;

    #[derive(Grammar, Clone)]
    pub enum DTS {
        Function(Prefix<(StringEqCI!("FN"), Space1), ODBCFunc>),
        DateTime(DTSVal),
    }

    #[derive(Grammar, Clone)]
    #[tygr(name = "odbc func")]
    pub enum ODBCFunc {
        // Similar to SQLScalarFunction but in ODBC escape syntax
        Abs(StringEqCI!("ABS"), Args<Scalx>),
        Acos(StringEqCI!("ACOS"), Args<Scalx>),
        Asin(StringEqCI!("ASIN"), Args<Scalx>),
        Atan(StringEqCI!("ATAN"), Args<Scalx>),
        Ceiling(StringEqCI!("CEILING"), Args<Scalx>),
        Cos(StringEqCI!("COS"), Args<Scalx>),
        Cot(StringEqCI!("COT"), Args<Scalx>),
        Exp(StringEqCI!("EXP"), Args<Scalx>),
        Floor(StringEqCI!("FLOOR"), Args<Scalx>),
        Log(StringEqCI!("LOG"), Args<Scalx>),
        Log10(StringEqCI!("LOG10"), Args<Scalx>),
        Mod(StringEqCI!("MOD"), Args<RR>),
        Pi(StringEqCI!("PI"), Option<StringEq!("()")>),
        Power(StringEqCI!("POWER"), Args<RR>),
        Sin(StringEqCI!("SIN"), Args<Scalx>),
        Sqrt(StringEqCI!("SQRT"), Args<Scalx>),
        Tan(StringEqCI!("TAN"), Args<Scalx>),
        Ascii(StringEqCI!("ASCII"), Args<Scalx>),
        Char(StringEqCI!("CHAR"), Args<Scalx>),
        Concat(StringEqCI!("CONCAT"), Args<RR>),
        CurDate(StringEqCI!("CURDATE"), Option<StringEq!("()")>),
        CurTime(StringEqCI!("CURTIME"), Option<StringEq!("()")>),
        Database(StringEqCI!("DATABASE"), Option<StringEq!("()")>),
        DayName(StringEqCI!("DAYNAME"), Args<Scalx>),
        DayOfMonth(StringEqCI!("DAYOFMONTH"), Args<Scalx>),
        DayOfWeek(StringEqCI!("DAYOFWEEK"), Args<Scalx>),
        DayOfYear(StringEqCI!("DAYOFYEAR"), Args<Scalx>),
        Hour(StringEqCI!("HOUR"), Args<Scalx>),
        LCase(StringEqCI!("LCASE"), Args<Scalx>),
        Left(StringEqCI!("LEFT"), Args<RR>),
        Length(StringEqCI!("LENGTH"), Args<Scalx>),
        LTrim(StringEqCI!("LTRIM"), Args<Scalx>),
        Minute(StringEqCI!("MINUTE"), Args<Scalx>),
        Month(StringEqCI!("MONTH"), Args<Scalx>),
        MonthName(StringEqCI!("MONTHNAME"), Args<Scalx>),
        Now(StringEqCI!("NOW"), Option<StringEq!("()")>),
        Quarter(StringEqCI!("QUARTER"), Args<Scalx>),
        Repeat(StringEqCI!("REPEAT"), Args<RR>),
        Right(StringEqCI!("RIGHT"), Args<RR>),
        Round(StringEqCI!("ROUND"), Args<RR>),
        RTrim(StringEqCI!("RTRIM"), Args<Scalx>),
        Second(StringEqCI!("SECOND"), Args<Scalx>),
        Sign(StringEqCI!("SIGN"), Args<Scalx>),
        Space(StringEqCI!("SPACE"), Args<Scalx>),
        Substring(StringEqCI!("SUBSTRING"), SubstringArgs),
        TimestampAdd(
            StringEqCI!("TIMESTAMPADD"),
            Args<RRR<DoubleQuotable<TSAddInterval>>>,
        ),
        TimestampDiff(
            StringEqCI!("TIMESTAMPDIFF"),
            Args<RRR<DoubleQuotable<TSDiffInterval>>>,
        ),
        Truncate(StringEqCI!("TRUNCATE"), Args<RR>),
        UCase(StringEqCI!("UCASE"), Args<Scalx>),
        Week(StringEqCI!("WEEK"), Args<Scalx>),
        Year(StringEqCI!("YEAR"), Args<Scalx>),
    }

    #[derive(Grammar, Clone)]
    #[tygr(name = "dts val")]
    pub struct DTSVal {
        pub dts_type: DTSType,
        pub sp: Space,
        pub val_spec: DTSValSpec,
    }

    #[derive(Grammar, Clone)]
    #[tygr(name = "dts type")]
    pub enum DTSType {
        Date(StringEqCI!("D")),
        Timestamp(StringEqCI!("TS")),
        Time(StringEqCI!("T")),
    }

    #[derive(Grammar, Clone)]
    pub struct Fetch {
        pub keyword: (StringEqCI!("FETCH"), Space),
        pub direction: Option<(Either<StringEqCI!("FIRST"), StringEqCI!("NEXT")>, Space)>,
        pub valspec: RowCount,
        pub row_keyword: Option<(Space, RowRowsInline)>,
        pub only: Option<(Space, StringEqCI!("ONLY"))>,
    }

    #[derive(Grammar, Clone)]
    #[tygr(inline)]
    pub enum RowRowsInline {
        Rows(StringEqCI!("ROWS")),
        Row(StringEqCI!("ROW")),
    }

    /// `ALL` where `DISTINCT` could stand: what a SELECT or an aggregate does without it.
    pub type AllKw = (StringEqCI!("ALL"), NotFollowedBy<IdentifierPart>);
}
