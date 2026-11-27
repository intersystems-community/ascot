//! The IRIS SQL grammar for `&sql(...)` blocks embedded in ObjectScript, reached from `cos.rs`.
//!
//! Written once under `#[levels]`, which makes it twice: `full`, every spelling IRIS accepts, is
//! re-exported here; `core` lacks the syntactic sugar, which `desugar` rewrites into it and the
//! checker reads.
//!
//! Its rules of placement: a rule describes its own text, the blank before or after it belonging to
//! the sequence that places it, and a keyword run is one `Kw!`; one wrapper per bracket kind,
//! `Parenthesized<T>` for every pair of parentheses; tygr's combinators before a hand-rolled shape,
//! an alias only where it shortens; and two rules reading the same text are one.

use either::Either;
use tygr::bnf::{Expr, Rules};
use tygr::*;
/// A keyword, `Kw!("READ ONLY", "READONLY")`: its words a blank apart, none the start of a longer word (IRIS
/// reads `NOTBETWEEN`, `BETWEEN1`, `TOP1` and `%NOINDEXa` as names), in its canonical spelling or others.
pub use tygr_objectscript_macros::Kw;

use crate::convert::VecSep;
use crate::cos;

/// A keyword's words, `Kw!("READ ONLY")`, one terminal in the EBNF.
#[derive(Clone, Default)]
pub struct Phrase<T>(pub T);

impl<T: Parse> Parse for Phrase<T> {
    type First = T::First;

    fn parse_at<S: Success, L: Level>(
        input: &str,
        pos: usize,
        state: State<L>,
    ) -> Option<(S::Output<Self>, usize)> {
        let (words, end) = T::parse_at::<S, L>(input, pos, state)?;
        Some((S::and_then(words, |words| S::output(|| Phrase(words))), end))
    }

    fn to_bnf() -> Expr {
        fn words(expr: &Expr, out: &mut Vec<String>) {
            match expr {
                Expr::Token { text, .. } => out.push(text.clone()),
                Expr::Sequence(exprs) => exprs.iter().for_each(|expr| words(expr, out)),
                _ => {}
            }
        }
        let mut text = Vec::new();
        words(&T::to_bnf(), &mut text);
        Expr::Token {
            text: text.join(" "),
            case_insensitive: true,
        }
    }

    fn visit_rules(rules: &mut Rules) {
        T::visit_rules(rules)
    }
}

impl<T: Print> Print for Phrase<T> {
    fn print_to(&self, buf: &mut String) {
        self.0.print_to(buf)
    }
}

/// A token in its canonical spelling or others of any shape, `Spellings!(Kw!("OR"), StringEq!("!"))`; the core
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

pub type SimpleComment = Wrap<StringEq!("--"), StringOf<IsNotNL>, Option<StringEq!("\n")>>;

pub type BracketedComment = Prefix<StringEq!("/*"), StringEndsWith<StringEq!("*/")>>;

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
    LC(SimpleComment),
    MC(BracketedComment),
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

/// A letter or `%`, or `_` with a second character: 2026.2 reads `a$b`, `ñ`, `Яа`, `_1` and `__` as names and `_`
/// alone as the concatenation operator.
#[derive(Grammar, Clone)]
pub enum IdentifierStart {
    SimpleLatin(IsSimpleLatinLetter),
    NonAscii(IsNonAsciiLetter),
    P(StringEq!("%")),
    Underscored(StringEq!("_"), IdentifierPart),
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
pub struct Identifier(Raw<(IdentifierStart, Vec<IdentifierPart>)>);

#[derive(Grammar, Clone)]
pub enum DelimitedIdentifierPart {
    DDQ(StringEq!("\"\"")),
    Char(IsNotDQ),
}

#[derive(Grammar, Clone)]
pub struct DelimitedIdentifier(
    Raw<(
        StringEq!("\""),
        Vec1<DelimitedIdentifierPart>,
        StringEq!("\""),
    )>,
);

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

pub type CharacterStringLiteral =
    Wrap<StringEq!("'"), Vec<CharacterRepresentation>, StringEq!("'")>;

#[derive(Grammar, Clone)]
pub enum CharacterRepresentation {
    DSQ(StringEq!("''")),
    Char(IsNotSQ),
}

#[derive(Grammar, Clone)]
pub struct StringLiteral(pub Raw<CharacterStringLiteral>);

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
pub struct Exponent {
    e_kw: StringEqCI!("E"),
    sign: Option<Sign>,
    digits: StringOf1<IsDigit>,
}

#[derive(Grammar, Clone)]
pub struct Number {
    base: NumberBase,
    exponent: Option<Exponent>,
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
pub struct HostVariable {
    pub colon: Option<StringEq!(":")>,
    pub host: SubscriptedHost,
}

/// 2026.2 refuses `:i%Prop()`, an instance variable with an empty subscript list, and takes `:arr()`.
#[derive(Grammar, Clone)]
pub enum SubscriptedHost {
    Instance {
        kind: Either<StringEq!("i%"), StringEq!("r%")>,
        name: HostIdentifier,
        subs: Option<Parenthesized<VecSep<Subscript, ArgSep>>>,
    },
    Variable {
        host: Host,
        subs: Option<Subscripts>,
    },
}

pub type Subscripts = Parenthesized<Option<VecSep<Subscript, ArgSep>>>;

#[derive(Grammar, Clone)]
pub enum VariableReference {
    HostReference(HostReference),
    Ident(Ident),
}

#[derive(Grammar, Clone)]
pub struct HostReference {
    pub colon: Option<StringEq!(":")>,
    pub host: Host,
}

#[derive(Grammar, Clone)]
pub enum Host {
    Property {
        name: HostIdentifier,
        parts: Vec1<(StringEq!("."), Either<HostIdentifier, DelimitedIdentifier>)>,
    },
    Plain(HostIdentifier),
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
pub enum TargetSpecification {
    HostVariable(HostVariable),
    Ident(Ident, Option<Subscripts>),
}

#[derive(Grammar, Clone)]
pub enum Subscript {
    Literal(Literal),
    VariableReference(VariableReference),
}

pub type ArgSep = (Space, StringEq!(","), Space);

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
    "POSITION",
    "CURRENT_TIMESTAMP",
    "CURRENT_TIME",
    "XMLCONCAT",
    "XMLELEMENT",
    "XMLFOREST",
    "XMLATTRIBUTES",
    "DATEADD",
    "DATEDIFF",
    "DATENAME",
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

/// The functions called by name, the ObjectScript `$` functions among them (2026.2 takes these and no other
/// `$` name): each its variant, keyword and argument shape, and the name the core calls where another.
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
    pub struct TableName(pub Ranged<DottedIdent>);

    /// A function's name, `$` before an ObjectScript function's (`$EXTRACT`, `$TSQL_SPID`).
    #[derive(Grammar)]
    pub struct FunctionName {
        pub dollar: Option<StringEq!("$")>,
        pub name: Ranged<DottedIdent>,
    }

    #[derive(Grammar)]
    pub enum ColumnReference {
        Str(Ranged<Ident>, Space, StringEq!("."), Space, StringEq!("*")),
        Arr(
            Ranged<DottedIdent>,
            Vec<(Space, StringEq!("->"), Space, Ident)>,
        ),
    }

    #[derive(Grammar)]
    pub struct UnsignedInteger(pub StringOf1<IsDigit>);

    #[derive(Grammar)]
    pub enum OrderingOperator {
        Le(Spellings!(StringEq!("<="), StringEqCI!("NOT>"))),
        Ge(Spellings!(StringEq!(">="), StringEqCI!("NOT<"))),
        Gt(StringEq!(">")),
        Lt(StringEq!("<")),
    }

    #[derive(Grammar)]
    pub enum CompOp {
        StartsWith(Kw!("%STARTSWITH")),
        Eq(StringEq!("=")),
        Ne(Spellings!(StringEq!("<>"), StringEq!("!="), StringEqCI!("NOT="))),
        Ord(OrderingOperator),
        Contains(StringEq!("[")),
        Follows(StringEq!("]")),
    }

    #[derive(Grammar)]
    pub enum ValueExpressionPrimary {
        Macro(
            StringEq!("$$$"),
            Identifier,
            Option<crate::cos::MacroArgList>,
        ),
        Literal(Literal),
        Builtin(Box<Builtin>),
        WindowFunction(Box<WindowFunction>),
        AggregateFunction(Box<AggregateFunction>),
        Unary {
            op: Sign,
            space: Space,
            term: Box<Operand>,
        },
        #[sugar(percent_call)]
        PercentUnary {
            op: PercentOperator,
            term: Box<Operand<Space1>>,
        },
        #[sugar(position)]
        Position(Kw!("POSITION"), Args<Box<PositionArgs>>),
        #[sugar(substring_from)]
        Substring(Kw!("SUBSTRING"), Args<Box<SubstringFrom>>),
        ExtrinsicFunction(Box<ExtrinsicFunction>),
        ParameterMarker(StringEq!("?")),
        Case(Box<Case>),
        #[sugar(odbc_escape)]
        OdbcEscape(Box<ODBCEscape>),
        Query(Subquery),
        Niladic(Niladic),
        Call(Call),
        Column(ColumnReference),
        SpecialSystemVariable(SpecialSystemVariable),
        HostVariable(HostVariable),
        Null(Kw!("NULL")),
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

    fn odbc_function(function: ODBCFunction) -> core::ValueExpressionPrimary {
        calls!(ODBCFunction: function;
            Abs Acos Asin Atan2 Atan Ceiling Cos Cot Exp Floor Log Log10 Mod Power Sin Sqrt Tan Ascii Char
            Concat DayName DayOfMonth DayOfWeek DayOfYear Degrees Hour LCase Left Length LTrim Minute Month
            MonthName Quarter Radians Repeat Right Round RTrim Second Sign Space Substring Truncate UCase Week
            Year;
            Pi CurDate CurTime Database Now;
            ODBCFunction::TimestampAdd(kw, a) => {
                core::ValueExpressionPrimary::Builtin(Box::new(core::Builtin::TimestampAdd(kw.desugar(), a.desugar())))
            }
            ODBCFunction::TimestampDiff(kw, a) => {
                core::ValueExpressionPrimary::Builtin(Box::new(core::Builtin::TimestampDiff(kw.desugar(), a.desugar())))
            }
        )
    }

    fn cast(value: ODBCDateTimeLiteral) -> core::ValueExpressionPrimary {
        let cast_type = match value.dts_type {
            ODBCDateTimeType::Date(_) => core::CastTarget::Date(Default::default()),
            ODBCDateTimeType::Time(_) => core::CastTarget::Time(Default::default()),
            ODBCDateTimeType::Timestamp(_) => core::CastTarget::Timestamp(Default::default()),
        };
        let expr = term(match value.val_spec {
            ODBCDateTimeValue::Literal(literal) => core::ValueExpressionPrimary::Literal(literal),
            ODBCDateTimeValue::HostVariable(host_var) => {
                core::ValueExpressionPrimary::HostVariable(host_var)
            }
        });
        let cast = core::CastArgs {
            expr,
            as_kw: Default::default(),
            cast_type,
        };
        core::ValueExpressionPrimary::Builtin(Box::new(core::Builtin::Cast(
            Default::default(),
            (Default::default(), parenthesized(Box::new(cast))),
        )))
    }

    fn odbc_escape(escape: Box<ODBCEscape>) -> core::ValueExpressionPrimary {
        match escape.wrapped {
            ODBCEscapeContent::DateTime(value) => cast(value),
            ODBCEscapeContent::Function(function) => odbc_function(function.prefixed),
        }
    }

    fn percent_call(
        op: PercentOperator,
        term: Box<Operand<Space1>>,
    ) -> core::ValueExpressionPrimary {
        let arg = match *term {
            Operand::Paren(_, parens) => (*parens.wrapped).desugar(),
            Operand::Plain(_, term) => core::ValueExpression::one(term.desugar()),
        };
        call(&op.print(), vec![arg])
    }

    fn position(_: Kw!("POSITION"), args: Args<Box<PositionArgs>>) -> core::ValueExpressionPrimary {
        call("CHARINDEX", desugar::args(args))
    }

    fn substring_from(
        kw: Kw!("SUBSTRING"),
        args: Args<Box<SubstringFrom>>,
    ) -> core::ValueExpressionPrimary {
        call(&kw.print(), desugar::args(args))
    }

    #[derive(Grammar)]
    pub enum SpecialSystemVariable {
        Horolog(Kw!("$HOROLOG", "$H")),
        Job(Kw!("$JOB", "$J")),
        Namespace(Kw!("$NAMESPACE", "$ZNSPACE")),
        TLevel(Kw!("$TLEVEL", "$TL")),
        Username(Kw!("$USERNAME")),
        ZHorolog(Kw!("$ZHOROLOG", "$ZH")),
        ZJob(Kw!("$ZJOB", "$ZJ")),
        ZPi(Kw!("$ZPI")),
        ZTimestamp(Kw!("$ZTIMESTAMP", "$ZTS")),
        ZTimezone(Kw!("$ZTIMEZONE", "$ZTZ")),
        ZVersion(Kw!("$ZVERSION", "$ZV")),
    }

    /// A function called by name: built-in, ObjectScript's (`$LENGTH`) or user-defined, in the default schema when
    /// unqualified (2026.2 takes `Thrice(2)` for SQLUser.Thrice); what a built-in takes is the check's (A6071). A
    /// function with a syntax of its own cannot be called otherwise (2026.2 rejects TIMESTAMPADD(:u, 1, a),
    /// POSITION(a, b) and COUNT(a, b)).
    #[derive(Grammar)]
    #[tygr(validated)]
    pub struct Call {
        pub name: FunctionName,
        pub args: ArgsN,
    }

    impl Validate for Call {
        const REQUIREMENT: &'static str = "not name a function with a syntax of its own";
        fn validate(&self) -> bool {
            !OWN_SYNTAX.contains(&self.name.print().to_uppercase().as_str())
        }
    }

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
    pub enum BinaryOperator {
        Concat(Spellings!(StringEq!("||"), StringEq!("_"))),
        Mod(StringEq!("#")),
        Mul(StringEq!("*")),
        Add(StringEq!("+")),
        Sub(StringEq!("-")),
        Div(StringEq!("/")),
        IntDiv(StringEq!("\\")),
    }

    /// Terms joined by operators, the first after the blank `B` its place gives it: `Space1` after a keyword,
    /// nothing inside parentheses or after an operator.
    #[derive(Grammar)]
    pub struct ValueExpression<B: Grammar + Default = ()> {
        pub first: Operand<B>,
        pub rest: Vec<(Space, BinaryOperator, Space, Operand)>,
    }

    impl<B: Grammar + Default> ValueExpression<B> {
        pub fn one(term: Ranged<ValueExpressionPrimary>) -> Self {
            ValueExpression {
                first: Operand::Plain(Default::default(), term),
                rest: Vec::new(),
            }
        }

        /// The expression after another blank, where a rewrite places it.
        pub fn after<C: Grammar + Default>(self) -> ValueExpression<C> {
            ValueExpression {
                first: self.first.after(),
                rest: self.rest,
            }
        }
    }

    /// An operand after the blank `B` its place gives it: `P`, parenthesized, which may do without the blank, or
    /// the plain `T`.
    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum Gapped<B: Grammar + Default, T: Grammar, P: Grammar> {
        Paren(Option<B>, P),
        Plain(B, T),
    }

    impl<B: Grammar + Default, T: Grammar, P: Grammar> Gapped<B, T, P> {
        /// The operand at another place, with that place's blank.
        pub fn after<C: Grammar + Default>(self) -> Gapped<C, T, P> {
            match self {
                Gapped::Paren(_, parens) => Gapped::Paren(None, parens),
                Gapped::Plain(_, plain) => Gapped::Plain(Default::default(), plain),
            }
        }
    }

    pub type Operand<B = ()> =
        Gapped<B, Ranged<ValueExpressionPrimary>, Parenthesized<Box<ValueExpression>>>;

    #[derive(Grammar)]
    pub struct Case {
        pub case_kw: Kw!("CASE"),
        pub body: CaseBody,
        pub else_clause: Option<(Space1, ElseClause)>,
        pub end_kw: (Space1, Kw!("END")),
    }

    /// Searched, each WHEN a condition, or simple, each WHEN a value the operand is compared to; 2026.2
    /// rejects a searched CASE without WHEN and takes a simple one.
    #[derive(Grammar)]
    pub enum CaseBody {
        Searched(Vec1<(Space1, When<BooleanValueExpression<Space1>>)>),
        Simple(
            ValueExpression<Space1>,
            Vec<(Space1, When<ValueExpression<Space1>>)>,
        ),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct When<T: Grammar> {
        pub when_kw: Kw!("WHEN"),
        pub operand: T,
        pub then_kw: (Space1, Kw!("THEN")),
        pub result: ValueExpression<Space1>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ElseClause {
        pub else_kw: Kw!("ELSE"),
        pub result: ValueExpression<Space1>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct CastArgs {
        pub expr: ValueExpression,
        pub as_kw: (Space1, Kw!("AS"), Space1),
        pub cast_type: CastTarget,
    }

    /// A blank and the parenthesized arguments of a function; in a shape, R is a required argument
    /// and O an optional one.
    pub type Args<T> = (Space, Parenthesized<T>);

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct RR<T1: Grammar = ValueExpression, T2: Grammar = ValueExpression> {
        pub arg1: T1,
        pub sep1: ArgSep,
        pub arg2: T2,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct RRR<
        T1: Grammar = ValueExpression,
        T2: Grammar = ValueExpression,
        T3: Grammar = ValueExpression,
    > {
        pub arg1: T1,
        pub sep1: ArgSep,
        pub arg2: T2,
        pub sep2: ArgSep,
        pub arg3: T3,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct RO<T1: Grammar = ValueExpression, T2: Grammar = ValueExpression> {
        pub arg1: T1,
        pub arg2: Option<(ArgSep, T2)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ROO<
        T1: Grammar = ValueExpression,
        T2: Grammar = ValueExpression,
        T3: Grammar = ValueExpression,
    > {
        pub arg1: T1,
        pub rest: Option<(ArgSep, T2, Option<(ArgSep, T3)>)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct RRO<
        T1: Grammar = ValueExpression,
        T2: Grammar = ValueExpression,
        T3: Grammar = ValueExpression,
    > {
        pub arg1: T1,
        pub sep1: ArgSep,
        pub arg2: T2,
        pub arg3: Option<(ArgSep, T3)>,
    }

    pub type ArgsN = Args<Option<VecSep<ValueExpression, ArgSep>>>;

    pub type Args1Plus = Args<VecSep<ValueExpression, ArgSep>>;

    /// The functions with a syntax of their own, CURRENT_TIMESTAMP and CURRENT_TIME with a precision,
    /// which the niladic would shadow, and the ODBC-only functions.
    #[derive(Grammar)]
    pub enum Builtin {
        XmlConcat(XMLConcat),
        XmlElement(XMLElement),
        XmlForest(XMLForest),
        Cast(Kw!("CAST"), Args<Box<CastArgs>>),
        DateAdd(Kw!("DATEADD"), Args<RRR<Nested<DatePart>>>),
        DateDiff(Kw!("DATEDIFF"), Args<RRR<Nested<DatePart>>>),
        DateName(Kw!("DATENAME"), Args<RR<Nested<DatePart>>>),
        DatePart(Kw!("DATEPART"), Args<RR<Nested<ExtendedDatePart>>>),
        DateTrunc(Kw!("DATE_TRUNC"), Args<RR<Nested<DatePart>>>),
        JSONArray(Kw!("JSON_ARRAY"), JSONArrayArgs),
        JSONObject(Kw!("JSON_OBJECT"), JSONObjectArgs),
        Predict(Kw!("PREDICT"), Args<PredictArgs>),
        Probability(Kw!("PROBABILITY"), Args<ProbabilityArgs>),
        SearchIndex(Kw!("SEARCH_INDEX"), Args<SearchIndexArgs>),
        TimestampAdd(
            Kw!("TIMESTAMPADD"),
            Args<RRR<DoubleQuotable<TimestampAddInterval>>>,
        ),
        TimestampDiff(
            Kw!("TIMESTAMPDIFF"),
            Args<RRR<DoubleQuotable<TimestampDiffInterval>>>,
        ),
        ToVector(
            Kw!("TO_VECTOR"),
            Args<ROO<ValueExpression, VectorDataType, Integer>>,
        ),
        Trim(Kw!("TRIM"), Args<TrimArgs>),
        Odbc(ODBCOnlyFunctionEscape),
        CurrentTimestamp(Kw!("CURRENT_TIMESTAMP"), Args<ValueExpression>),
        CurrentTime(Kw!("CURRENT_TIME"), Args<ValueExpression>),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum OptionallyWrapped<T: Grammar> {
        Wrapped(Parenthesized<T>),
        Scalar(T),
    }

    /// `BOTH ' ' FROM expr` in the core: a lone spec trims blanks, lone chars both ends.
    #[derive(Grammar)]
    pub enum TrimArgs {
        Full(
            TrimSpecification,
            ValueExpression<Space1>,
            Space1,
            Kw!("FROM"),
            ValueExpression<Space1>,
        ),
        #[sugar(trim_space)]
        Space(
            TrimSpecification,
            Option<(Space1, Kw!("FROM"))>,
            ValueExpression<Space1>,
        ),
        #[sugar(trim_both)]
        Both(
            ValueExpression,
            Space1,
            Kw!("FROM"),
            ValueExpression<Space1>,
        ),
        #[sugar(trim_both_space)]
        BothSpace(ValueExpression),
    }

    fn trim_space(
        spec: TrimSpecification,
        _: Option<(Space1, Kw!("FROM"))>,
        expr: ValueExpression<Space1>,
    ) -> core::TrimArgs {
        trim(spec.desugar(), string(" "), expr.desugar())
    }

    fn trim_both(
        chars: ValueExpression,
        _: Space1,
        _: Kw!("FROM"),
        expr: ValueExpression<Space1>,
    ) -> core::TrimArgs {
        let both = core::TrimSpecification::Both(Default::default());
        trim(both, chars.desugar().after(), expr.desugar())
    }

    fn trim_both_space(expr: ValueExpression) -> core::TrimArgs {
        let both = core::TrimSpecification::Both(Default::default());
        trim(both, string(" "), expr.desugar().after())
    }

    fn trim(
        spec: core::TrimSpecification,
        chars: core::ValueExpression<Space1>,
        expr: core::ValueExpression<Space1>,
    ) -> core::TrimArgs {
        core::TrimArgs::Full(spec, chars, Default::default(), Default::default(), expr)
    }

    #[derive(Grammar)]
    pub enum TrimSpecification {
        Both(Kw!("BOTH")),
        Leading(Kw!("LEADING")),
        Trailing(Kw!("TRAILING")),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SearchIndexArgs {
        pub table: Vec<(Ident, StringEq!("."))>,
        pub index_name: Ident,
        pub scalx_list: Vec<(ArgSep, ValueExpression)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct PredictArgs {
        pub model: Ident,
        pub use_clause: Option<(Space, PredictProbabilityUse)>,
        pub with_clause: Option<((Space, Kw!("WITH"), Space), PredictWith)>,
    }

    pub type PredictProbabilityUse = Prefix<(Kw!("USE"), Space1), Ident>;

    #[derive(Grammar)]
    pub enum PredictWith {
        Unnamed(List),
        Named(VecSep<PredictAssignment, ArgSep>),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct PredictAssignment {
        pub ident: Ident,
        pub eq: (Space, StringEq!("="), Space),
        pub expression: ValueExpression,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ProbabilityArgs {
        pub model: Ident,
        pub use_clause: Option<(Space, PredictProbabilityUse)>,
        pub for_clause: Option<(Space1, Prefix<Kw!("FOR"), ValueExpression<Space1>>)>,
        pub with_clause: Option<((Space, Kw!("WITH"), Space), PredictWith)>,
    }

    pub type JSONArrayArgs = Args<
        Option<(
            VecSep<ValueExpression, ArgSep>,
            Option<(Space, JSONNullClause)>,
        )>,
    >;

    pub type JSONObjectArgs = Args<Option<(JSONKeyValuePairs, Option<(Space, JSONNullClause)>)>>;

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct JSONKeyValuePairs(
        pub  VecSep<
            (
                ValueExpression,
                (Space, StringEq!(":"), Space),
                ValueExpression,
            ),
            ArgSep,
        >,
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
        pub args: Args<(XMLElementName, Vec<(ArgSep, XMLValue)>)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    /// `NAME "x"`, `NAME` or `"x"`: 2026.2 rejects a plain identifier, `XMLELEMENT(a, b)` and `NAME foo`.
    pub enum XMLElementName {
        Name(Kw!("NAME"), Option<(Space, DelimitedIdentifier)>),
        Delimited(DelimitedIdentifier),
    }

    #[derive(Grammar)]
    pub enum XMLValue {
        Concat(Box<XMLConcat>),
        Attributes(XMLAttributes),
        Expr(ValueExpression),
        Value2(XMLContent),
    }

    #[derive(Grammar)]
    pub enum XMLContent {
        Element(Box<XMLElement>),
        Forest(Box<XMLForest>),
        Comment(StringEq!("<--"), StringEndsWith<StringEq!("-->")>),
    }

    #[derive(Grammar)]
    pub struct XMLAttributes {
        pub keyword: Kw!("XMLATTRIBUTES"),
        pub attrs: Args<VecSep<XMLAttribute, ArgSep>>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct XMLAttribute {
        pub expr: ValueExpression,
        pub alias: Option<((Space1, Kw!("AS"), Space1), Ident)>,
    }

    #[derive(Grammar)]
    pub struct XMLForest {
        pub keyword: Kw!("XMLFOREST"),
        pub items: Args<VecSep<XMLAttribute, ArgSep>>,
    }

    #[derive(Grammar)]
    pub struct XMLConcat {
        pub keyword: Kw!("XMLCONCAT"),
        pub values: Args<VecSep<XMLContent, ArgSep>>,
    }

    #[derive(Grammar)]
    pub struct ExtrinsicFunction {
        pub function: Prefix<StringEqCI!("$$"), StringOf1<IsExtFuncChar>>,
        pub args: Parenthesized<Option<VecSep<ValueExpression, ArgSep>>>,
    }

    /// `{fn CONVERT(x, SQL_TYPE)}` and `{fn IFNULL(x, y)}`, which IRIS reads otherwise without the escape (2026.2:
    /// CONVERT takes `SQL_TYPE` for a column, IFNULL(x, y) is NULL when x is not), and `{fn USER}`; `fn` is declared
    /// as ODBC spells it, so the core prints `{fn`.
    pub type ODBCOnlyFunctionEscape =
        Wrap<(StringEq!("{"), Space, Kw!("fn"), Space1), ODBCOnlyFunction, (Space, StringEq!("}"))>;

    #[derive(Grammar)]
    pub enum ODBCOnlyFunction {
        Convert(Kw!("CONVERT"), Args<RR<ValueExpression, ODBCConvertType>>),
        IfNull(Kw!("IFNULL"), Args<RR>),
        User(Kw!("USER"), Option<Args<()>>),
    }

    pub type DoubleQuote = StringEq!("\"");

    pub type SingleQuote = StringEq!("'");

    /// A keyword bare or in double quotes, as TIMESTAMPADD's interval (2026.2 rejects `'SQL_TSI_DAY'`).
    #[derive(Grammar)]
    pub enum DoubleQuotable<K: Grammar> {
        Bare(K),
        #[sugar(unquoted_double)]
        Quoted(Wrap<DoubleQuote, K, DoubleQuote>),
    }

    fn unquoted_double<K: Grammar + Desugar>(
        quoted: Wrap<DoubleQuote, K, DoubleQuote>,
    ) -> core::DoubleQuotable<K::Core>
    where
        K::Core: Grammar,
    {
        core::DoubleQuotable::Bare(quoted.wrapped.desugar())
    }

    /// A keyword bare or in single or double quotes, as a date part (2026.2 takes `'day'` and `"day"`,
    /// not `' day'` or any other literal).
    #[derive(Grammar)]
    pub enum Quotable<K: Grammar> {
        Bare(K),
        #[sugar(unquoted)]
        Single(Wrap<SingleQuote, K, SingleQuote>),
        #[sugar(unquoted)]
        Double(Wrap<DoubleQuote, K, DoubleQuote>),
    }

    fn unquoted<Q: Grammar, K: Grammar + Desugar>(quoted: Wrap<Q, K, Q>) -> core::Quotable<K::Core>
    where
        K::Core: Grammar,
    {
        core::Quotable::Bare(quoted.wrapped.desugar())
    }

    /// `T` in parentheses to any depth: 2026.2 takes `TOP (1)`, `LIMIT ((10))`, `DATEADD((dd), 1, d)` and
    /// `SIZE (((10)))`.
    #[derive(Grammar)]
    pub enum Nested<T: Grammar, B: Grammar + Default = ()> {
        Bare(B, T),
        #[sugar(unnested)]
        Parens(Option<B>, Parenthesized<Box<Nested<T>>>),
    }

    fn unnested<T: Grammar + Desugar, B: Grammar + Default + Desugar>(
        _: Option<B>,
        parens: Parenthesized<Box<Nested<T>>>,
    ) -> core::Nested<T::Core, B::Core>
    where
        T::Core: Grammar,
        B::Core: Grammar + Default,
    {
        let core::Nested::Bare(_, bare) = (*parens.wrapped).desugar();
        core::Nested::Bare(Default::default(), bare)
    }

    #[derive(Grammar)]
    pub enum DatePart {
        Keyword(Quotable<DatePartCommon>),
        HostVariable(HostVariable),
    }

    /// DATEPART's date part, which SQLTIMESTAMP joins.
    #[derive(Grammar)]
    pub enum ExtendedDatePart {
        Keyword(Quotable<ExtendedDatePartKeyword>),
        HostVariable(HostVariable),
    }

    #[derive(Grammar)]
    pub enum ExtendedDatePartKeyword {
        SqlTimestamp(Kw!("SQLTIMESTAMP", "STS")),
        Common(DatePartCommon),
    }

    #[derive(Grammar)]
    pub enum DatePartCommon {
        Year(Kw!("YEAR", "YYYY", "YY")),
        Quarter(Kw!("QUARTER", "QQ", "Q")),
        Month(Kw!("MONTH", "MM", "M")),
        Weekday(Kw!("WEEKDAY", "DW", "W")),
        Week(Kw!("WEEK", "WK", "WW")),
        DayOfYear(Kw!("DAYOFYEAR", "DY", "Y")),
        Day(Kw!("DAY", "DD", "D")),
        Hour(Kw!("HOUR", "HH")),
        Minute(Kw!("MINUTE", "MI", "N")),
        Second(Kw!("SECOND", "SS", "S")),
        Millisecond(Kw!("MILLISECOND", "MS")),
        Microsecond(Kw!("MICROSECOND", "MCS")),
        Nanosecond(Kw!("NANOSECOND", "NS")),
    }

    #[derive(Grammar)]
    pub enum TimestampAddInterval {
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
    pub enum TimestampDiffInterval {
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
    pub enum ODBCConvertType {
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
    pub enum ODBCDateTimeValue {
        Literal(Literal),
        HostVariable(HostVariable),
    }

    #[derive(Grammar)]
    pub enum AggregateFunction {
        AggFuncArgs {
            function: SetFunctionType,
            arg: Args<AggregateFunctionArgs>,
        },
        CountStar(Kw!("COUNT"), Args<StringEq!("*")>),
    }

    #[derive(Grammar)]
    pub enum SetFunctionType {
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
    pub struct AggregateFunctionArgs {
        pub quantifier: SetQuantifier,
        pub expr: ValueExpression,
        pub foreach: Option<(Space1, Prefix<(Kw!("%FOREACH"), Space), List>)>,
        pub afterhaving: Option<(Space1, Kw!("%AFTERHAVING"))>,
    }

    /// DISTINCT, DISTINCT BY (…), or ALL, what a SELECT or an aggregate does without a word; 2026.2 refuses
    /// ALL DISTINCT and DISTINCT ALL.
    #[derive(Grammar)]
    pub enum SetQuantifier {
        Distinct {
            kw: Kw!("DISTINCT"),
            by: Option<(Space1, Prefix<(Kw!("BY"), Space), List>)>,
            space: Space1,
        },
        All(#[sugar] Option<(Kw!("ALL"), Space1)>),
    }

    pub type List = Parenthesized<VecSep<ValueExpression, ArgSep>>;

    #[derive(Grammar)]
    pub struct WindowFunction {
        pub call: WindowFunctionType,
        pub over_kw: (Space1, Kw!("OVER"), Space),
        pub spec: Parenthesized<(
            Option<WindowPartitionClause>,
            Option<(Space, OrderByClause)>,
            Option<(Space, Kw!("ROWS"), Space1, WindowFrameExtent)>,
        )>,
    }

    /// Each window function with its arguments: 2026.2 rejects ROW_NUMBER(a), SUM(*), SUM(a, b), NTILE(),
    /// LEAD(a, 1, 0, 1) and FIRST_VALUE(a, 1), and COUNT takes * or an aggregate's argument.
    #[derive(Grammar)]
    pub enum WindowFunctionType {
        RowNumber(Kw!("ROW_NUMBER"), Args<()>),
        Rank(Kw!("RANK"), Args<()>),
        DenseRank(Kw!("DENSE_RANK"), Args<()>),
        PercentRank(Kw!("PERCENT_RANK"), Args<()>),
        CumeDist(Kw!("CUME_DIST"), Args<()>),
        NTile(Kw!("NTILE"), Args<ValueExpression>),
        FirstValue(Kw!("FIRST_VALUE"), Args<ValueExpression>),
        LastValue(Kw!("LAST_VALUE"), Args<ValueExpression>),
        NthValue(Kw!("NTH_VALUE"), Args<RO>),
        Lead(Kw!("LEAD"), Args<ROO>),
        Lag(Kw!("LAG"), Args<ROO>),
        Sum(Kw!("SUM"), Args<ValueExpression>),
        Avg(Kw!("AVG"), Args<ValueExpression>),
        Count(
            Kw!("COUNT"),
            Args<Either<StringEq!("*"), AggregateFunctionArgs>>,
        ),
        Min(Kw!("MIN"), Args<ValueExpression>),
        Max(Kw!("MAX"), Args<ValueExpression>),
    }

    pub type WindowPartitionClause =
        Prefix<(Kw!("PARTITION BY"), Space1), VecSep<ValueExpression, ArgSep>>;

    #[derive(Grammar)]
    pub enum WindowFrameExtent {
        #[sugar(single_bound)]
        Single(WindowFrameBound),
        Between {
            between_kw: Kw!("BETWEEN"),
            start: (Space1, WindowFrameBound),
            end: (Space1, Kw!("AND"), Space1, WindowFrameBound),
        },
    }

    /// `ROWS n PRECEDING` is `ROWS BETWEEN n PRECEDING AND CURRENT ROW`, the standard's lone-bound reading.
    fn single_bound(bound: WindowFrameBound) -> core::WindowFrameExtent {
        let current_row = core::WindowFrameBound::CurrentRow(Default::default());
        core::WindowFrameExtent::Between {
            between_kw: Default::default(),
            start: (Default::default(), bound.desugar()),
            end: (
                Default::default(),
                Default::default(),
                Default::default(),
                current_row,
            ),
        }
    }

    #[derive(Grammar)]
    pub enum WindowFrameBound {
        UnboundedPreceding(Kw!("UNBOUNDED PRECEDING")),
        UnboundedFollowing(Kw!("UNBOUNDED FOLLOWING")),
        Preceding(
            Either<UnsignedInteger, StringLiteral>,
            Space1,
            Kw!("PRECEDING"),
        ),
        Following(
            Either<UnsignedInteger, StringLiteral>,
            Space1,
            Kw!("FOLLOWING"),
        ),
        CurrentRow(Kw!("CURRENT ROW", "CURRENT ROWS")),
    }

    /// Conditions joined by OR, the first after the blank `B` its place gives it.
    #[derive(Grammar)]
    pub struct BooleanValueExpression<B: Grammar + Default = ()> {
        pub first: BooleanTerm<B>,
        pub rest: Vec<OrBooleanTerm>,
    }

    impl<B: Grammar + Default> BooleanValueExpression<B> {
        pub fn one(cond: BooleanFactor<B>) -> Self {
            BooleanValueExpression {
                first: BooleanTerm {
                    first: cond,
                    rest: Vec::new(),
                },
                rest: Vec::new(),
            }
        }
    }

    /// `OR` and the conditions it joins, or `!`, which needs no blanks.
    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum OrBooleanTerm {
        Or(Space1, Kw!("OR"), BooleanTerm<Space1>),
        #[sugar(bang)]
        Bang(Space, StringEq!("!"), BooleanTerm<Space>),
    }

    fn bang(_: Space, _: StringEq!("!"), term: BooleanTerm<Space>) -> core::OrBooleanTerm {
        core::OrBooleanTerm::Or(
            Default::default(),
            Default::default(),
            term.desugar().after(),
        )
    }

    #[derive(Grammar)]
    pub struct BooleanTerm<B: Grammar + Default = ()> {
        pub first: BooleanFactor<B>,
        pub rest: Vec<AndBooleanFactor>,
    }

    impl<B: Grammar + Default> BooleanTerm<B> {
        pub fn after<C: Grammar + Default>(self) -> BooleanTerm<C> {
            BooleanTerm {
                first: self.first.after(),
                rest: self.rest,
            }
        }
    }

    /// `AND` and the condition it joins, or `&`, which needs no blanks.
    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum AndBooleanFactor {
        And(Space1, Kw!("AND"), BooleanFactor<Space1>),
        #[sugar(ampersand)]
        Ampersand(Space, StringEq!("&"), BooleanFactor<Space>),
    }

    fn ampersand(
        _: Space,
        _: StringEq!("&"),
        factor: BooleanFactor<Space>,
    ) -> core::AndBooleanFactor {
        core::AndBooleanFactor::And(
            Default::default(),
            Default::default(),
            factor.desugar().after(),
        )
    }

    pub type NoIndex = Option<(Kw!("%NOINDEX"), Space1)>;

    pub type BooleanFactor<B = ()> =
        Gapped<B, Ranged<Factor>, Parenthesized<Box<BooleanValueExpression>>>;

    /// NOT, or a predicate after its `%NOINDEX` hint (2026.2 takes the hint before no NOT or parenthesis). A
    /// predicate's own NOT, where IRIS writes it, is dropped and the predicate stands in `NOT(…)` in the core, each
    /// form measured on 2026.2 with NULL rows; under a quantifier it negates each comparison, so the quantifier flips.
    #[derive(Grammar)]
    pub enum Factor {
        Not(Kw!("NOT"), Box<BooleanFactor<Space1>>),
        Exists(NoIndex, Kw!("EXISTS"), Space, Subquery),
        ForSomeElement {
            noindex: NoIndex,
            kw: (Kw!("FOR SOME %ELEMENT"), Space),
            column: Parenthesized<ColumnReference>,
            label: Option<Alias>,
            predicate: (Space, Parenthesized<Box<BooleanValueExpression>>),
        },
        ForSome {
            noindex: NoIndex,
            kw: (Kw!("FOR SOME"), Space),
            tables: Parenthesized<TableReferenceList>,
            condition: Option<(Space, Parenthesized<Box<ForSomeCondition>>)>,
        },
        StarEq(
            NoIndex,
            ValueExpression,
            Space,
            StringEqCI!("*="),
            Space,
            ValueExpression,
        ),
        EqStar(
            NoIndex,
            ValueExpression,
            Space,
            StringEqCI!("=*"),
            Space,
            ValueExpression,
        ),
        #[sugar(negated)]
        IsNull(
            NoIndex,
            ValueExpression,
            Space1,
            Kw!("IS"),
            Space1,
            #[sugar] Not,
            Kw!("NULL"),
        ),
        #[sugar(negated)]
        Between(
            NoIndex,
            ValueExpression,
            Space1,
            #[sugar] Not,
            Kw!("BETWEEN"),
            ValueExpression<Space1>,
            Space1,
            Kw!("AND"),
            ValueExpression<Space1>,
        ),
        #[sugar(negated)]
        Like(
            NoIndex,
            ValueExpression,
            Space1,
            #[sugar] Not,
            LikeOperator,
            ValueExpression<Space1>,
            Option<(Space1, EscapeClause)>,
        ),
        #[sugar(in_query)]
        InQuery(
            NoIndex,
            ValueExpression,
            Space1,
            Not,
            Kw!("IN"),
            Space,
            Subquery,
        ),
        #[sugar(negated)]
        In(
            NoIndex,
            ValueExpression,
            Space1,
            #[sugar] Not,
            Kw!("IN"),
            Space,
            List,
        ),
        #[sugar(negated)]
        InList(
            NoIndex,
            ValueExpression,
            Space1,
            #[sugar] Not,
            Kw!("%INLIST"),
            ValueExpression<Space1>,
            Option<(Space1, SizeHint)>,
        ),
        Find(
            NoIndex,
            ValueExpression,
            Space1,
            Kw!("%FIND"),
            ValueExpression<Space1>,
            Option<(Space1, SizeHint)>,
        ),
        Pattern(
            NoIndex,
            ValueExpression,
            Space1,
            Kw!("%PATTERN"),
            ValueExpression<Space1>,
        ),
        #[sugar(negated)]
        IsJSON(
            NoIndex,
            ValueExpression,
            Space1,
            Option<(Kw!("FORMAT JSON"), Space1)>,
            Kw!("IS"),
            Space1,
            #[sugar] Not,
            Kw!("JSON"),
            Option<(Space1, JSONPredicateTypeConstraint)>,
        ),
        Contains(
            NoIndex,
            ValueExpression,
            Space1,
            Either<Kw!("%CONTAINSTERM"), Kw!("%CONTAINS")>,
            Args1Plus,
        ),
        #[sugar(negated_quantified)]
        Quantified(
            NoIndex,
            ValueExpression,
            Space,
            #[sugar] BracketNot,
            CompOp,
            Space,
            Quantifier,
            Space,
            Subquery,
        ),
        #[sugar(negated)]
        Compare(
            NoIndex,
            ValueExpression,
            Space,
            #[sugar] BracketNot,
            CompOp,
            Space,
            ValueExpression,
        ),
    }

    /// `NOT` before a predicate's keyword.
    pub type Not = Option<(Kw!("NOT"), Space1)>;

    #[derive(Grammar)]
    pub enum LikeOperator {
        Like(Kw!("LIKE")),
        Matches(Kw!("%MATCHES")),
    }

    /// `NOT` against a bracket, `NOT[` and `NOT]`, which 2026.2 takes with no blank between; `NOT<`, `NOT>` and
    /// `NOT=` are spellings of the operators themselves.
    pub type BracketNot = Option<(
        Kw!("NOT"),
        FollowedBy<Either<StringEq!("["), StringEq!("]")>>,
    )>;

    fn negated<N>(not: Option<N>, plain: core::Factor) -> core::Factor {
        match not {
            Some(_) => negation(plain),
            None => plain,
        }
    }

    fn negated_quantified(not: BracketNot, plain: core::Factor) -> core::Factor {
        match (not, plain) {
            (
                Some(_),
                core::Factor::Quantified(noindex, lhs, sp, op, sp2, quantifier, sp3, query),
            ) => {
                let quantifier = match quantifier {
                    core::Quantifier::All(_) => core::Quantifier::Any(Default::default()),
                    core::Quantifier::Any(_) => core::Quantifier::All(Default::default()),
                };
                negation(core::Factor::Quantified(
                    noindex, lhs, sp, op, sp2, quantifier, sp3, query,
                ))
            }
            (_, plain) => plain,
        }
    }

    /// `IN (q)` is `= ANY (q)`.
    fn in_query(
        noindex: NoIndex,
        lhs: ValueExpression,
        _: Space1,
        not: Not,
        _: Kw!("IN"),
        sp: Space,
        query: Subquery,
    ) -> core::Factor {
        let (eq, any) = (
            core::CompOp::Eq(Default::default()),
            core::Quantifier::Any(Default::default()),
        );
        let plain = core::Factor::Quantified(
            noindex.desugar(),
            lhs.desugar(),
            Default::default(),
            eq,
            Default::default(),
            any,
            sp.desugar(),
            query.desugar(),
        );
        negated(not, plain)
    }

    /// `NOT(…)` around a factor, standing at the negated predicate's range.
    fn negation(factor: core::Factor) -> core::Factor {
        let factor = core::BooleanFactor::Plain((), desugar::synthesized(factor));
        let condition = parenthesized(Box::new(core::BooleanValueExpression::one(factor)));
        core::Factor::Not(
            Default::default(),
            Box::new(core::BooleanFactor::Paren(None, condition)),
        )
    }

    #[derive(Grammar)]
    pub enum ForSomeCondition {
        BooleanValueExpression(BooleanValueExpression),
        GroupHaving(Option<(GroupByClause, Space1)>, HavingClause),
    }

    pub type EscapeClause = Prefix<Kw!("ESCAPE"), ValueExpression<Space1>>;

    pub type Subquery = Parenthesized<QueryExpression>;

    #[derive(Grammar)]
    pub enum Quantifier {
        All(Kw!("ALL")),
        Any(Kw!("ANY", "SOME")),
    }

    #[derive(Grammar)]
    pub struct SizeHint {
        pub kw: (Kw!("SIZE"), Space),
        /// Doubled, as IRIS requires: 2026.2 rejects `SIZE (n)` and `SIZE n`, takes deeper nesting and blanks.
        pub size: Parenthesized<Parenthesized<Nested<Literal>>>,
    }

    #[derive(Grammar)]
    pub enum JSONPredicateTypeConstraint {
        Value(Kw!("VALUE")),
        Array(Kw!("ARRAY")),
        Object(Kw!("OBJECT")),
        Scalar(Kw!("SCALAR")),
    }

    pub type WithClause = Prefix<(Kw!("WITH"), Space1), VecSep<Ranged<WithListElement>, ArgSep>>;

    #[derive(Grammar)]
    pub struct WithListElement {
        pub name: Ident,
        pub columns: Option<(Space, ColumnNameList)>,
        pub as_kw: (Space1, Kw!("AS"), Space),
        pub query: Subquery,
    }

    #[derive(Grammar)]
    pub struct QuerySpecification {
        pub kw: Kw!("SELECT"),
        pub options: Vec<(Space1, SelectOption)>,
        pub space: Space1,
        pub quantifier: SetQuantifier,
        pub body: SelectBody,
    }

    /// With TOP a SELECT orders its own rows and takes no row limit (SQLCODE -386 with one); without TOP it takes a
    /// row limit, and an ORDER BY after it is the statement's (-25 before UNION, -143 in a subquery).
    #[derive(Grammar)]
    pub enum SelectBody {
        Top(
            Kw!("TOP"),
            TopSpecification,
            Space1,
            SelectClauses,
            Option<(Space1, OrderByClause)>,
        ),
        Plain(SelectClauses, Option<(Space1, RowLimitingClause)>),
    }

    #[derive(Grammar)]
    pub struct SelectClauses {
        pub select_list: VecSep<Ranged<SelectItem>, ArgSep>,
        pub into: Option<(Space1, IntoClause)>,
        pub from: Option<(Space1, FromClause)>,
        pub where_clause: Option<(Space1, WhereClause)>,
        pub group: Option<(Space1, GroupByClause)>,
        pub having: Option<(Space1, HavingClause)>,
    }

    /// The statement's ORDER BY and the row limit after it, the only place a LIMIT is the whole's: 2026.2 reads
    /// LIMIT 1 ORDER BY a LIMIT 2 as the SELECT's limit, then the whole's order and limit, and refuses a numeric
    /// LIMIT right after a lone SELECT with TOP (-386); LIMIT 1 LIMIT 2 without ORDER BY, which it takes, is not read.
    #[derive(Grammar)]
    pub struct Tail {
        pub order: OrderByClause,
        pub limit: Option<(Space1, RowLimitingClause)>,
    }

    /// The query an INSERT or a CREATE TABLE AS reads, ordered once, last (2026.2 takes LIMIT 1 ORDER BY a, not
    /// ORDER BY a LIMIT 1 or LIMIT 1 LIMIT 2, SQLCODE -25).
    #[derive(Grammar)]
    pub struct SourceQuery {
        pub query: Query,
        pub order: Option<(Space1, OrderByClause)>,
    }

    /// A query inside a statement, as a subquery, a CTE body or a view.
    #[derive(Grammar)]
    pub struct QueryExpression(pub Box<Query>);

    #[derive(Grammar)]
    pub enum SelectOption {
        NoFPlan(Kw!("%NOFPLAN")),
        NoLock(Kw!("%NOLOCK")),
        Profile(Kw!("%PROFILE")),
        ProfileAll(Kw!("%PROFILE_ALL")),
        NoRuntime(Kw!("%NORUNTIME")),
        DoRuntime(Kw!("%DORUNTIME")),
        RowLimit(Kw!("%ROWLIMIT"), Nested<Count, Space1>),
        RowOffset(Kw!("%ROWOFFSET"), Nested<Count, Space1>),
    }

    #[derive(Grammar)]
    pub enum TopSpecification {
        All(Space1, Kw!("ALL")),
        Count(Nested<Count, Space1>),
    }

    /// A count: a literal, a host variable or `?`; 2026.2 takes `OFFSET (:n)`, `FETCH NEXT 'x' ROWS ONLY`,
    /// `DIAGNOSTICS SIZE (1)` and `%ROWLIMIT ((5))`, not `LIMIT 1 + 1`, `LIMIT a`, `LIMIT -1`, `%ROWLIMIT (1, 2)`
    /// or `TOP (ALL)`.
    #[derive(Grammar)]
    pub enum Count {
        Literal(Literal),
        HostVariable(HostVariable),
        Marker(StringEq!("?")),
    }

    #[derive(Grammar)]
    pub enum SelectItem {
        Star(StringEq!("*")),
        Expr {
            expr: ValueExpression,
            alias: Option<Alias>,
        },
    }

    // The blank stays inside the alias, so that a reference without one consumes nothing after it.
    #[derive(Grammar)]
    pub struct Alias {
        #[sugar]
        pub as_kw: Option<(Space1, Kw!("AS"))>,
        pub space: Space1,
        pub name: SQLIdentNonKeyword,
    }

    pub type Into = Prefix<(Kw!("INTO"), Space), VecSep<TargetSpecification, ArgSep>>;

    #[derive(Grammar)]
    pub enum IntoClause {
        Into(Into),
        IntoList(IntoList),
    }

    pub type IntoList = Prefix<Kw!("%INTOLIST"), (Space, TargetSpecification)>;

    #[derive(Grammar)]
    pub struct FromClause {
        pub keyword: Kw!("FROM"),
        pub options: Vec<(Space1, FromOption)>,
        pub tables: TableReferenceList<Space1>,
    }

    #[derive(Grammar)]
    pub enum FromOption {
        InOrder(Kw!("%INORDER")),
        Full(Kw!("%FULL")),
        DbugFull(Kw!("%DBUGFULL")),
        Flatten(Kw!("%FLATTEN")),
        NoBSL(Kw!("%NOBSL")),
        NoFixedStats(Kw!("%NOFIXEDSTATS"), Space, VecSep<TableRef, ArgSep>),
        NoFlatten(Kw!("%NOFLATTEN")),
        NoMerge(Kw!("%NOMERGE")),
        NoReduce(Kw!("%NOREDUCE")),
        NoSVSO(Kw!("%NOSVSO")),
        NoTopOpt(Kw!("%NOTOPOPT")),
        IgnoreIndices(Kw!("%IGNOREINDICES"), Space, VecSep<IndexRef, ArgSep>),
        IgnoreIndex(Kw!("%IGNOREINDEX"), Space, VecSep<IndexRef, ArgSep>),
        StartTable(Kw!("%STARTTABLE")),
        AllIndex(Kw!("%ALLINDEX")),
        NoUnionOrOpt(Kw!("%NOUNIONOROPT")),
        FirstTable(Kw!("%FIRSTTABLE"), Space1, Ident),
        Parallel(Kw!("%PARALLEL")),
        NoParallel(Kw!("%NOPARALLEL")),
    }

    /// `[schema.]table` or `[schema.]*`; 2026.2 takes blanks around the dot and refuses a third part.
    #[derive(Grammar)]
    pub struct TableRef {
        pub schema: Option<(Ident, Space, StringEq!("."), Space)>,
        pub table: Either<StringEq!("*"), Ident>,
    }

    #[derive(Grammar)]
    pub struct IndexRef {
        pub table: Vec<(Ident, Space, StringEq!("."), Space)>,
        pub index: IndexSpecification,
    }

    #[derive(Grammar)]
    pub enum IndexSpecification {
        Name(Ident),
        All(StringEq!("*")),
    }

    #[derive(Grammar)]
    #[sugar]
    pub struct TableReferenceList<B: Grammar + Default = ()> {
        pub first: TableReference<B>,
        #[sugar]
        pub rest: Vec<(ArgSep, Ranged<TableReference>)>,
    }

    /// `FROM a, b` is `FROM a CROSS JOIN b`, b's own joins after it: 2026.2 reads `t, u LEFT JOIN v ON …` as
    /// `t CROSS JOIN u LEFT JOIN v ON …`, the ON seeing u and v alone, the rows of `t CROSS JOIN (u LEFT JOIN v …)`.
    impl<B: Grammar + Default + Desugar> Desugar for TableReferenceList<B>
    where
        B::Core: Grammar + Default,
    {
        type Core = core::TableReferenceList<B::Core>;
        fn desugar(self) -> Self::Core {
            let core::TableReference::Join(mut first) = self.first.desugar();
            for (_, entry) in self.rest {
                let core::TableReference::Join(core::JoinedTable { primary, joins }) =
                    entry.ranged.desugar();
                let table = Box::new(core::JoinedTable {
                    primary: primary.after(),
                    joins: Vec::new(),
                });
                let cross = core::Join::Cross {
                    cross_kw: Default::default(),
                    table,
                };
                first.joins.push((
                    Default::default(),
                    Ranged::new(entry.start, cross, entry.end),
                ));
                first.joins.extend(joins);
            }
            core::TableReferenceList {
                first: core::TableReference::Join(first),
            }
        }
    }

    /// A joined table, or the ODBC `{oj …}` escape around one, after the blank `B` its place gives it.
    #[derive(Grammar)]
    pub enum TableReference<B: Grammar + Default = ()> {
        Join(JoinedTable<B>),
        #[sugar(outer_join_escape)]
        OJSyntax(
            Option<B>,
            OuterJoinEscapeOpen,
            JoinedTable<Space1>,
            OuterJoinEscapeClose,
        ),
    }

    pub type OuterJoinEscapeOpen = (StringEq!("{"), Space, Kw!("OJ"));

    pub type OuterJoinEscapeClose = (Space, StringEq!("}"));

    fn outer_join_escape<B: Grammar + Default + Desugar>(
        _: Option<B>,
        _: OuterJoinEscapeOpen,
        table: JoinedTable<Space1>,
        _: OuterJoinEscapeClose,
    ) -> core::TableReference<B::Core>
    where
        B::Core: Grammar + Default,
    {
        core::TableReference::Join(table.desugar().after())
    }

    #[derive(Grammar)]
    pub struct JoinedTable<B: Grammar + Default = ()> {
        pub primary: TablePrimary<B>,
        pub joins: Vec<(Space1, Ranged<Join>)>,
    }

    impl<B: Grammar + Default> JoinedTable<B> {
        pub fn after<C: Grammar + Default>(self) -> JoinedTable<C> {
            JoinedTable {
                primary: self.primary.after(),
                joins: self.joins,
            }
        }
    }

    #[derive(Grammar)]
    pub enum Join {
        Cross {
            cross_kw: Kw!("CROSS JOIN"),
            table: Box<JoinedTable<Space1>>,
        },
        Natural {
            natural_kw: (Kw!("NATURAL"), Space1),
            join_type: JoinType,
            join_kw: Kw!("JOIN"),
            table: Box<JoinedTable<Space1>>,
        },
        Qualified {
            join_type: JoinType,
            join_kw: Kw!("JOIN"),
            table: Box<JoinedTable<Space1>>,
            spec: (Space1, JoinSpecification),
        },
    }

    /// LEFT, RIGHT or FULL, OUTER implied, or INNER, the join without a word.
    #[derive(Grammar)]
    pub enum JoinType {
        Outer(
            OuterJoinType,
            #[sugar] Option<(Space1, Kw!("OUTER"))>,
            Space1,
        ),
        Inner(#[sugar] Option<(Kw!("INNER"), Space1)>),
    }

    #[derive(Grammar)]
    pub enum OuterJoinType {
        Left(Kw!("LEFT")),
        Right(Kw!("RIGHT")),
        Full(Kw!("FULL")),
    }

    #[derive(Grammar)]
    pub enum JoinSpecification {
        On {
            on_kw: Kw!("ON"),
            condition: BooleanValueExpression<Space1>,
        },
        Using {
            using_kw: (Kw!("USING"), Space),
            columns: ColumnNameList,
        },
    }

    pub type TablePrimary<B = ()> = Gapped<B, NamedTable, ParenthesizedTable>;

    #[derive(Grammar)]
    pub enum NamedTable {
        JSONTable(JSONTable, Option<Alias>),
        TableValuedFunction(TableValuedFunction, Option<Alias>),
        Table(TableName, Option<Alias>),
        Lateral(Kw!("LATERAL"), Space, Subquery, Option<Alias>),
    }

    #[derive(Grammar)]
    pub enum ParenthesizedTable {
        Query(Subquery, Option<Alias>),
        JoinedTable(Parenthesized<Box<JoinedTable>>),
    }

    #[derive(Grammar)]
    pub struct JSONTable {
        pub kw: Kw!("JSON_TABLE"),
        pub args: Args<(
            JSONApiCommonSyntax,
            Option<(Space, JSONTableColumnsSpecification)>,
        )>,
    }

    #[derive(Grammar)]
    pub struct JSONApiCommonSyntax {
        pub context_item: ValueExpression,
        pub path: Option<(ArgSep, StringLiteral)>,
    }

    #[derive(Grammar)]
    pub enum JSONTableColumnsSpecification {
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
        pub sp1: Space1,
        pub data_type: DataType,
        pub path_clause: Option<(Space1, Kw!("PATH"), Space, StringLiteral)>,
    }

    #[derive(Grammar)]
    pub struct JSONTableTypeClause {
        pub kw: Kw!("%TYPE"),
        pub sp1: Space,
        pub type_name: StringLiteral,
    }

    #[derive(Grammar)]
    pub struct TableValuedFunction {
        pub table: TableName,
        pub args: Parenthesized<
            Option<(
                VecSep<ValueExpression, ArgSep>,
                Option<(Space, StringEq!(","))>,
            )>,
        >,
    }

    pub type WhereClause = Prefix<Kw!("WHERE"), BooleanValueExpression<Space1>>;

    pub type GroupByClause = Prefix<(Kw!("GROUP BY"), Space1), VecSep<ValueExpression, ArgSep>>;

    pub type HavingClause = Prefix<Kw!("HAVING"), BooleanValueExpression<Space1>>;

    pub type OrderByClause =
        Prefix<(Kw!("ORDER BY"), Space1), VecSep<Ranged<SortSpecification>, ArgSep>>;

    #[derive(Grammar)]
    pub struct SortSpecification {
        pub expr: SortKey,
        pub direction: OrderingSpecification,
    }

    /// DESC, or ASC, the order without a word.
    #[derive(Grammar)]
    pub enum OrderingSpecification {
        Desc(Space1, Kw!("DESC")),
        Asc(#[sugar] Option<(Space1, Kw!("ASC"))>),
    }

    #[derive(Grammar)]
    pub enum SortKey {
        Literal(Literal),
        ValueExpression(ValueExpression),
    }

    #[derive(Grammar)]
    pub enum RowLimitingClause {
        LimitOffset {
            limit: Prefix<Kw!("LIMIT"), LimitSpecification>,
            offset: Option<(Space1, ResultOffsetClause)>,
        },
        #[sugar(offset_fetch)]
        OffsetFetch {
            offset: ResultOffsetClause,
            fetch: Option<(Space1, Fetch)>,
        },
        #[sugar(fetch_only)]
        FetchOnly(Fetch),
    }

    /// `OFFSET m [ROWS] [FETCH … n …]` is `LIMIT n OFFSET m`, `LIMIT ALL OFFSET m` without the FETCH.
    fn offset_fetch(
        offset: ResultOffsetClause,
        fetch: Option<(Space1, Fetch)>,
    ) -> core::RowLimitingClause {
        let offset = core::ResultOffsetClause {
            kw: Default::default(),
            count: offset.count.desugar(),
        };
        let all = || core::LimitSpecification::All(Default::default(), Default::default());
        let limit = fetch.map_or_else(all, |(_, fetch)| fetch_count(fetch));
        limit_offset(limit, Some((Default::default(), offset)))
    }

    /// `FETCH [FIRST|NEXT] n [ROWS] [ONLY]` alone is `LIMIT n`.
    fn fetch_only(fetch: Fetch) -> core::RowLimitingClause {
        limit_offset(fetch_count(fetch), None)
    }

    fn fetch_count(fetch: Fetch) -> core::LimitSpecification {
        core::LimitSpecification::Val(fetch.valspec.desugar())
    }

    fn limit_offset(
        limit: core::LimitSpecification,
        offset: Option<(Space1, core::ResultOffsetClause)>,
    ) -> core::RowLimitingClause {
        core::RowLimitingClause::LimitOffset {
            limit: Prefix::new(Default::default(), limit),
            offset,
        }
    }

    #[derive(Grammar)]
    pub enum LimitSpecification {
        All(Space1, Kw!("ALL")),
        Val(Nested<Count, Space1>),
    }

    #[derive(Grammar)]
    pub struct ResultOffsetClause {
        pub kw: Kw!("OFFSET"),
        pub count: Nested<Count, Space1>,
        #[sugar]
        pub rows: Option<(Space1, RowOrRows)>,
    }

    #[derive(Grammar)]
    pub struct Query(pub VecSep<QueryTerm, (Space, QueryOperator, Space)>);

    #[derive(Grammar)]
    pub enum QueryOperator {
        Union {
            union_kw: Kw!("UNION"),
            all: Option<(Space1, Kw!("ALL"))>,
            parallel: Option<(Space1, Kw!("%PARALLEL"))>,
        },
        Intersect(Kw!("INTERSECT")),
        Except(Kw!("EXCEPT")),
    }

    #[derive(Grammar)]
    pub enum QueryTerm {
        Query(Subquery),
        QuerySpecification(Ranged<QuerySpecification>),
    }
    #[derive(Grammar)]
    pub struct InsertStatement {
        pub insert_kw: Kw!("INSERT"),
        pub or_update: Option<(Space1, Kw!("OR UPDATE"))>,
        pub options: Vec<(Space1, InsertOption)>,
        #[sugar]
        pub into_kw: Option<(Space1, Kw!("INTO"))>,
        pub target: (Space, TargetTable),
        pub values: (Space, InsertColumnsAndSource),
    }

    /// The options INSERT, UPDATE and DELETE share, a repeated one allowed; INSERT adds %NOSTREAM, UPDATE %NOSTREAM
    /// and %ILOCK.
    #[derive(Grammar)]
    pub enum DmlOption {
        NoCheck(Kw!("%NOCHECK")),
        NoFPlan(Kw!("%NOFPLAN")),
        NoLock(Kw!("%NOLOCK")),
        NoIndex(Kw!("%NOINDEX")),
        NoTrigger(Kw!("%NOTRIGGER")),
        NoJourn(Kw!("%NOJOURN")),
        ObjectFiler(Kw!("%OBJECTFILER")),
        ProfileAll(Kw!("%PROFILE_ALL")),
        Profile(Kw!("%PROFILE")),
        NoRuntime(Kw!("%NORUNTIME")),
        DoRuntime(Kw!("%DORUNTIME")),
    }

    #[derive(Grammar)]
    pub enum InsertOption {
        Dml(DmlOption),
        NoStream(Kw!("%NOSTREAM")),
    }

    #[derive(Grammar)]
    pub enum UpdateOption {
        Dml(DmlOption),
        NoStream(Kw!("%NOSTREAM")),
        ILock(Kw!("%ILOCK")),
    }

    #[derive(Grammar)]
    pub enum TruncateOption {
        NoCheck(Kw!("%NOCHECK")),
        NoLock(Kw!("%NOLOCK")),
        NoJourn(Kw!("%NOJOURN")),
    }

    /// Every option word, UPDATE's set; IRIS reserves them, so none names a table (`DELETE %ILOCK FROM t` is refused,
    /// not a DELETE from %ILOCK).
    pub type OptionWord = UpdateOption;

    #[derive(Grammar)]
    pub struct TargetTable {
        pub not_option: NotFollowedBy<OptionWord>,
        pub target: TableOrSubquery,
        pub alias: Option<Alias>,
    }

    #[derive(Grammar)]
    pub enum TableOrSubquery {
        Table(TableName),
        Subquery(Subquery),
    }

    /// An INSERT's columns and source: a column list with VALUES, or with a query, or DEFAULT VALUES (2026.2 takes
    /// it after a column list too).
    #[derive(Grammar)]
    pub enum InsertColumnsAndSource {
        ColumnList(ColumnList),
        Query {
            columns: Option<ColumnReferenceList>,
            query: (Space, SourceQuery),
        },
        DefaultValues {
            columns: Option<ColumnReferenceList>,
            kw: (Space, Kw!("DEFAULT VALUES")),
        },
    }

    /// A column list with its VALUES, as INSERT and UPDATE take it, or SET assignments, which read the same.
    #[derive(Grammar)]
    pub enum ColumnList {
        #[sugar(set_clause_list)]
        SetClauseList {
            set_kw: Kw!("SET"),
            assignments: (Space1, VecSep<Ranged<SetClause>, ArgSep>),
        },
        Columns {
            columns: Option<ColumnReferenceList>,
            values: (Space, ValuesClause),
        },
    }

    /// `SET a = 1, b = 2` reads as `(a,b)VALUES(1,2)`.
    fn set_clause_list(
        _: Kw!("SET"),
        (_, list): (Space1, VecSep<Ranged<SetClause>, ArgSep>),
    ) -> core::ColumnList {
        let (columns, values) = (list.items.into_iter())
            .map(|a| (a.ranged.column.desugar(), a.ranged.value.desugar()))
            .unzip();
        let values = core::ValuesClause {
            values_kw: Default::default(),
            space: Default::default(),
            spec: core::ValuesSpecification::ValueExpression(parenthesized(commas(values))),
        };
        core::ColumnList::Columns {
            columns: Some((Default::default(), parenthesized(commas(columns)))),
            values: (Default::default(), values),
        }
    }

    pub type ColumnNameList = Parenthesized<VecSep<Ident, ArgSep>>;

    pub type ColumnReferenceList = Args<VecSep<ColumnReference, ArgSep>>;

    #[derive(Grammar)]
    pub struct ValuesClause {
        pub values_kw: Kw!("VALUES"),
        pub space: Space,
        pub spec: ValuesSpecification,
    }

    #[derive(Grammar)]
    pub enum ValuesSpecification {
        ValueExpression(List),
        VariableReference {
            var: VariableReference,
            subs: Subscripts,
        },
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct SetClause {
        pub column: ColumnReference,
        pub eq: (Space, StringEq!("="), Space),
        pub value: ValueExpression,
    }

    #[derive(Grammar)]
    pub struct UpdateStatement {
        pub update_kw: Kw!("UPDATE"),
        pub body: UpdateStatementBody,
    }

    #[derive(Grammar)]
    pub enum UpdateStatementBody {
        UpdateStats {
            stats_kw: (Space1, Kw!("STATISTICS"), Space1),
            table: TableName,
        },
        Update {
            options: Vec<(Space1, UpdateOption)>,
            target: (Space, TargetTable),
            assignment: (Space, ColumnList),
            from: Option<(Space1, FromClause)>,
            where_clause: Option<(Space1, DmlWhereClause)>,
        },
    }

    pub type DmlWhereClause = Prefix<Kw!("WHERE"), WhereCondition>;

    #[derive(Grammar)]
    pub enum WhereCondition {
        BooleanValueExpression(BooleanValueExpression<Space1>),
        CurrentOf {
            current_of: (Space1, Kw!("CURRENT OF"), Space1),
            ident: Ident,
            condition: Option<(Space1, Prefix<Kw!("AND"), BooleanValueExpression<Space1>>)>,
        },
    }

    #[derive(Grammar)]
    pub struct DeleteStatement {
        pub delete_kw: Kw!("DELETE"),
        pub options: Vec<(Space1, DmlOption)>,
        #[sugar]
        pub from_kw: Option<(Space1, Kw!("FROM"))>,
        pub target: (Space, TargetTable),
        pub from: Option<(Space1, FromClause)>,
        pub where_clause: Option<(Space1, DmlWhereClause)>,
    }

    /// The options in any order; each once is the check's rule (A3825).
    #[derive(Grammar)]
    pub struct Truncate {
        pub kw: Kw!("TRUNCATE TABLE"),
        pub options: Vec<(Space1, Ranged<TruncateOption>)>,
        pub table: (Space1, NotFollowedBy<OptionWord>, TableName),
        pub alias: Option<Alias>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum TransactionStatement {
        StartTransaction(StartTransactionStatement),
        SetTransaction(SetTransactionStatement),
        Rollback(RollbackStatement),
        TransactionOption(TransactionOption),
    }

    /// `START TRANSACTION [%COMMITMODE mode] [[,] modes]`: 2026.2 takes the comma before the modes with and
    /// without a commit mode, and between two modes alone; omitted, %COMMITMODE keeps the process's.
    #[derive(Grammar)]
    pub struct StartTransactionStatement {
        pub kw: Kw!("START TRANSACTION"),
        pub commit_mode: Option<(Space1, CommitModeClause)>,
        pub modes: TransactionModes,
    }

    /// `[START] SET TRANSACTION`, as START TRANSACTION (2026.2 takes START SET TRANSACTION).
    #[derive(Grammar)]
    pub struct SetTransactionStatement {
        pub start_kw: Option<(Kw!("START"), Space1)>,
        pub kw: Kw!("SET TRANSACTION"),
        pub commit_mode: Option<(Space1, CommitModeClause)>,
        pub modes: TransactionModes,
    }

    pub type CommitModeClause = Prefix<Kw!("%COMMITMODE"), (Space1, CommitMode)>;

    pub type TransactionModes = Option<(Either<ArgSep, Space1>, VecSep<TransactionMode, ArgSep>)>;

    #[derive(Grammar)]
    pub enum CommitMode {
        Implicit(Kw!("IMPLICIT")),
        Explicit(Kw!("EXPLICIT")),
        None(Kw!("NONE")),
    }

    #[derive(Grammar)]
    pub enum TransactionMode {
        IsolationLevel {
            kw: (Kw!("ISOLATION LEVEL"), Space1),
            level: LevelOfIsolation,
        },
        ReadOnly(Kw!("READ ONLY")),
        ReadWrite(Kw!("READ WRITE")),
        DiagnosticsSize {
            kw: Kw!("DIAGNOSTICS SIZE"),
            size: Nested<Count, Space1>,
        },
    }

    #[derive(Grammar)]
    pub enum LevelOfIsolation {
        ReadUncommitted(Kw!("READ UNCOMMITTED")),
        ReadCommitted(Kw!("READ COMMITTED")),
        ReadVerified(Kw!("READ VERIFIED")),
        RepeatableRead(Kw!("REPEATABLE READ")),
    }

    /// ROLLBACK [WORK] or ROLLBACK TO SAVEPOINT name; 2026.2 rejects ROLLBACK WORK TO SAVEPOINT.
    #[derive(Grammar)]
    pub enum RollbackStatement {
        Savepoint(Kw!("ROLLBACK TO SAVEPOINT"), Space1, Ident),
        Bare(Kw!("ROLLBACK"), #[sugar] Option<(Space1, Kw!("WORK"))>),
    }

    #[derive(Grammar)]
    pub struct Savepoint {
        pub kw: (Kw!("SAVEPOINT"), Space1),
        pub name: Ident,
    }

    /// LOCK [TABLE] t IN SHARE|EXCLUSIVE MODE [WAIT n] and UNLOCK [TABLE] t IN SHARE|EXCLUSIVE MODE [IMMEDIATE].
    #[derive(Grammar)]
    pub struct LockTable<Verb: Grammar, Tail: Grammar> {
        pub kw: Verb,
        pub table_kw: Option<(Space1, Kw!("TABLE"))>,
        pub table: (Space, TableName),
        pub mode: (
            Space,
            Kw!("IN"),
            Space1,
            Either<Kw!("SHARE"), Kw!("EXCLUSIVE")>,
            Space1,
            Kw!("MODE"),
        ),
        pub tail: Option<(Space1, Tail)>,
    }

    pub type Wait = (Kw!("WAIT"), Space, Literal);

    #[derive(Grammar)]
    pub enum TransactionOption {
        InTransaction(Kw!("%INTRANSACTION", "%INTRANS")),
        BegTrans(Kw!("%BEGTRANS")),
        Commit {
            commit_kw: Kw!("COMMIT"),
            #[sugar]
            work: Option<(Space1, Kw!("WORK"))>,
        },
    }

    #[derive(Grammar)]
    pub enum CursorStatement {
        Open {
            open_kw: (Kw!("OPEN"), Space1),
            cursor: Ident,
        },
        Fetch {
            fetch_kw: (Kw!("FETCH"), Space1),
            cursor: Ident,
            into: Option<(Space, IntoClause)>,
        },
        Close {
            close_kw: (Kw!("CLOSE"), Space1),
            cursor: Ident,
        },
    }

    #[derive(Grammar)]
    pub struct DeclareCursor {
        pub declare_kw: Kw!("DECLARE"),
        pub cursor_name: (Space1, Ident),
        pub cursor_kw: (Space1, Kw!("CURSOR FOR"), Space1),
        pub statement: DeclaredStatement,
        pub for_clause: Option<(Space1, Prefix<(Kw!("FOR"), Space1), UpdatabilityClause>)>,
    }

    #[derive(Grammar)]
    pub enum DeclaredStatement {
        ExplainStatement(ExplainStatement),
        SelectStatement(SelectStatement),
    }

    #[derive(Grammar)]
    pub enum UpdatabilityClause {
        ReadOnly(Kw!("READ ONLY")),
        Update(Kw!("UPDATE")),
    }

    #[derive(Grammar)]
    #[tygr(validated)]
    pub struct CallStatement {
        pub output: Option<(
            Either<HostVariable, StringEq!("?")>,
            Space,
            StringEq!("="),
            Space,
        )>,
        pub call_kw: Kw!("CALL"),
        pub routine: (Space1, Ranged<DottedIdent>),
        pub args: ArgsN,
        pub using: Option<(
            Space1,
            Kw!("USING"),
            Space,
            VecSep<Either<HostVariable, StringEq!("?")>, ArgSep>,
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
            .all(|expression| {
                let rest = expression.rest.iter().map(|(.., operand)| operand);
                std::iter::once(&expression.first).chain(rest).all(|operand| {
                    !matches!(operand, Operand::Plain(_, term) if matches!(term.ranged, ValueExpressionPrimary::Column(_)))
                })
            })
        }
    }

    #[derive(Grammar)]
    pub struct ExplainStatement {
        pub explain_kw: Kw!("EXPLAIN"),
        pub options: Vec<(Space1, ExplainOption)>,
        pub statement: (Space, Box<Ranged<Statement>>),
    }

    /// EXPLAIN's options, in any order (2026.2 takes EXPLAIN STAT ALT, EXPLAIN INTO :x ALT and INTO :x, :y).
    #[derive(Grammar)]
    pub enum ExplainOption {
        Alt(Kw!("ALT")),
        All(Kw!("ALL")),
        Stat(Kw!("STAT", "STATS")),
        Into(Kw!("INTO"), Space, VecSep<TargetSpecification, ArgSep>),
    }

    /// 2026.2 refuses a parenthesized first operand after WITH (SQLCODE -383, "CTE is only supported for SELECT…")
    /// and takes a parenthesized later one; the grammar takes both.
    #[derive(Grammar)]
    pub struct SelectStatement {
        pub with_clause: Option<(WithClause, Space)>,
        pub query: Query,
        pub tail: Option<(Space1, Tail)>,
    }

    #[derive(Grammar)]
    pub struct SetOptionStatement {
        pub kw: (Kw!("SET OPTION"), Space1),
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
            kw: Kw!("PKEY_IS_IDKEY"),
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
            schema: Ident,
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
    pub struct SetMlConfigurationStatement {
        pub kw: (Kw!("SET ML CONFIGURATION"), Space1),
        pub name: Ident,
    }

    pub type GrantStatement = Prefix<Kw!("GRANT"), (Space, GrantCommand)>;

    /// WITH GRANT OPTION goes with cubes and objects, WITH ADMIN OPTION with roles and admin privileges.
    #[derive(Grammar)]
    pub enum GrantCommand {
        Cube(Granting<CubeGranted, Kw!("GRANT")>),
        Object(Granting<ObjectGranted, Kw!("GRANT")>),
        Admin(Granting<AdminGranted, Kw!("ADMIN")>),
    }

    #[derive(Grammar)]
    pub struct Granting<What: Grammar, With: Grammar> {
        pub what: What,
        pub to_kw: (Space1, Kw!("TO"), Space),
        pub grantees: GranteeList,
        pub with: WithOption<With>,
    }

    #[derive(Grammar)]
    pub struct CubeGranted {
        pub select_kw: Kw!("SELECT"),
        pub columns: Option<(Space, ColumnNameList)>,
        pub on_kw: (Space, Kw!("ON"), Space1),
        pub cube_kw: Kw!("CUBE", "CUBES"),
        pub cubes: (Space, VecSep<Ident, ArgSep>),
    }

    #[derive(Grammar)]
    pub enum ObjectGranted {
        Privileges {
            privileges: ObjectPrivileges,
            on_kw: (Space1, Kw!("ON"), Space),
            objects: Objects,
        },
        Actions {
            actions: VecSep<Action, ArgSep>,
            on_kw: (Space1, Kw!("ON"), Space1),
            object: TableName,
        },
    }

    /// The roles last, since a privilege keyword is an identifier too.
    #[derive(Grammar)]
    pub enum AdminGranted {
        Privileges(VecSep<AdminPrivilege, ArgSep>),
        Roles(VecSep<Ident, ArgSep>),
    }

    pub type WithOption<K> = Option<(Space1, Kw!("WITH"), Space1, K, Space1, Kw!("OPTION"))>;

    pub type GrantOptionFor = Option<(Kw!("GRANT OPTION FOR"), Space)>;

    pub type RevokeStatement = Prefix<Kw!("REVOKE"), (Space, RevokeCommand)>;

    /// GRANT OPTION FOR, CASCADE or RESTRICT and AS go with objects, AS not with cubes, none with roles and admin
    /// privileges (2026.2 refuses `REVOKE role FROM u CASCADE` and `REVOKE GRANT OPTION FOR role FROM u`).
    #[derive(Grammar)]
    pub enum RevokeCommand {
        Cube(Revoking<GrantOptionFor, CubeGranted, Option<(Space, DropBehavior)>>),
        Object(Revoking<GrantOptionFor, ObjectGranted, (Option<(Space, DropBehavior)>, AsGrantor)>),
        Admin(Revoking<(), AdminGranted, ()>),
    }

    #[derive(Grammar)]
    pub struct Revoking<Before: Grammar, What: Grammar, After: Grammar> {
        pub grant_option: Before,
        pub what: What,
        pub from_kw: (Space1, Kw!("FROM"), Space),
        pub grantees: GranteeList,
        pub tail: After,
    }

    pub type AsGrantor = Option<(Space1, Kw!("AS"), Space, GranteeList)>;

    #[derive(Grammar)]
    pub enum ObjectPrivileges {
        All {
            all_kw: Kw!("ALL"),
            privileges_kw: Option<(Space1, Kw!("PRIVILEGES"))>,
        },
        Star(StringEq!("*")),
        List(VecSep<SimplePrivilege, ArgSep>),
        Execute(Kw!("EXECUTE")),
        Use(Kw!("USE")),
    }

    #[derive(Grammar)]
    pub enum SimplePrivilege {
        Alter(Kw!("%ALTER")),
        Delete(Kw!("DELETE")),
        OptCol(ColumnPrivilege),
    }

    #[derive(Grammar)]
    pub enum Action {
        Complex {
            privilege: ColumnPrivilege,
            space: Space,
            columns: ColumnNameList,
        },
        Simple(SimplePrivilege),
    }

    #[derive(Grammar)]
    pub enum ColumnPrivilege {
        Select(Kw!("SELECT")),
        Insert(Kw!("INSERT")),
        Update(Kw!("UPDATE")),
        References(Kw!("REFERENCES")),
    }

    #[derive(Grammar)]
    pub enum AdminPrivilege {
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
    pub enum GranteeList {
        Star(StringEq!("*")),
        List(VecSep<User, ArgSep>),
    }

    #[derive(Grammar)]
    pub enum User {
        Public(Kw!("PUBLIC")),
        Ident(Option<StringEq!("_")>, Ident),
    }

    #[derive(Grammar)]
    pub enum Objects {
        Star(StringEq!("*")),
        Schemas {
            schema_kw: (Kw!("SCHEMA"), Space1),
            schemas: VecSep<Ident, ArgSep>,
        },
        ForeignServers(ForeignServerPrivilegeClause),
        Tables(VecSep<TableName, ArgSep>),
    }

    #[derive(Grammar)]
    pub enum ForeignServerPrivilegeList {
        Star(StringEq!("*")),
        List(VecSep<TableName, ArgSep>),
    }

    #[derive(Grammar)]
    pub struct ForeignServerPrivilegeClause {
        pub foreign_kw: Option<(Kw!("FOREIGN"), Space1)>,
        pub server_kw: Kw!("SERVER", "SERVERS"),
        // A table named Servers: GRANT SELECT ON Servers TO u1 is a table grant on IRIS.
        pub servers: (
            Space1,
            NotFollowedBy<(Either<Kw!("TO"), Kw!("FROM")>, Space1)>,
            ForeignServerPrivilegeList,
        ),
    }

    #[derive(Grammar)]
    pub struct CheckPrivilegeStatement {
        pub checkpriv_kw: Kw!("%CHECKPRIV"),
        pub grant_option: Option<(Space1, GrantOrAdminOptionFor)>,
        pub command: (Space1, CheckPrivilegeCommand),
    }

    #[derive(Grammar)]
    pub struct GrantOrAdminOptionFor {
        pub kind: Either<Kw!("GRANT"), Kw!("ADMIN")>,
        pub option_for: (Space1, Kw!("OPTION FOR")),
    }

    #[derive(Grammar)]
    pub enum CheckPrivilegeCommand {
        Admin(VecSep<AdminPrivilege, ArgSep>),
        PrivilegeSpecification {
            privilege: PrivilegeSpecification,
            on_kw: (Space1, Kw!("ON"), Space1),
            object: TableName,
        },
    }

    #[derive(Grammar)]
    pub enum PrivilegeSpecification {
        Action(Action),
        Execute(Kw!("EXECUTE")),
        Use(Kw!("USE")),
    }

    type OptionalLength = Option<Parenthesized<ValueExpression>>;
    type OptionalPrecisionScale =
        Option<Parenthesized<(ValueExpression, Option<(ArgSep, ValueExpression)>)>>;

    /// CAST's types, the varying ones before the types they begin with; 2026.2 takes NCHAR VARYING and refuses
    /// DOUBLE PRECISION, INT(10) and VARCHAR(10, 2).
    #[derive(Grammar)]
    pub enum CastTarget {
        Bigint(Kw!("BIGINT")),
        Varbinary(Kw!("VARBINARY", "BINARY VARYING"), OptionalLength),
        Binary(Kw!("BINARY"), OptionalLength),
        Bit(Kw!("BIT")),
        Varchar(
            Kw!("VARCHAR", "CHARACTER VARYING", "CHAR VARYING"),
            OptionalLength,
        ),
        Char(Kw!("CHAR", "CHARACTER"), OptionalLength),
        Datetime(Kw!("DATETIME")),
        Date(Kw!("DATE")),
        Decimal(Kw!("DECIMAL", "DEC"), OptionalPrecisionScale),
        Double(Kw!("DOUBLE")),
        Float(Kw!("FLOAT")),
        Guid(Kw!("GUID")),
        Integer(Kw!("INTEGER", "INT")),
        Money(Kw!("MONEY")),
        Nvarchar(Kw!("NVARCHAR", "NCHAR VARYING"), OptionalLength),
        Nchar(Kw!("NCHAR"), OptionalLength),
        Numeric(Kw!("NUMERIC"), OptionalPrecisionScale),
        Posixtime(Kw!("POSIXTIME")),
        Real(Kw!("REAL")),
        Smalldatetime(Kw!("SMALLDATETIME")),
        Smallint(Kw!("SMALLINT")),
        Smallmoney(Kw!("SMALLMONEY")),
        Timestamp(Kw!("TIMESTAMP")),
        Time(Kw!("TIME")),
        Tinyint(Kw!("TINYINT")),
    }

    pub type IfExists = Option<(Kw!("IF EXISTS"), Space1)>;
    pub type IfNotExists = Option<(Kw!("IF NOT EXISTS"), Space1)>;
    pub type OrReplace = Option<(Kw!("OR REPLACE"), Space1)>;

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct UserSpecification {
        pub name: Ident,
        pub password: Option<(Space, PasswordKeyword, Space, Password)>,
        pub default: Option<((Space1, Kw!("DEFAULT"), Space1), Ident, (Space1, Ident))>,
        pub temporary: Option<((Space1, Kw!("TEMPORARY"), Space1), Ident, (Space1, Ident))>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ServerKw {
        pub foreign: Option<(Kw!("FOREIGN"), Space1)>,
        pub server_kw: (Kw!("SERVER"), Space1),
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
            kw: (Kw!("BUILD INDEX"), Space1),
            options: Vec<(BuildOption, Space1)>,
            for_kw: (Kw!("FOR"), Space1),
            scope: BuildScope,
        },
        CancelQuery {
            kw: Kw!("CANCEL QUERY"),
            id: Nested<Count, Space1>,
            cancel_scope: Option<(Space1, CancelQueryScope)>,
            timeout: Option<(Space1, Kw!("TIMEOUT"), Nested<Count, Space1>)>,
        },
        Collect {
            kw: (Kw!("COLLECT"), Space1),
            clause: StatisticsKeyword,
            target: (Space1, Kw!("FOR"), Space1, StatisticsScope),
            sample: Option<((Space1, Kw!("%SAMPLE_PERCENT"), Space1), PercentValue)>,
        },
        FreezePlans {
            kw: Kw!("FREEZE PLANS"),
            by: Option<((Space1, Kw!("BY"), Space1), FreezeScope)>,
        },
        Load {
            kw: (Kw!("LOAD"), Space1),
            spec: LoadSpecification,
        },
        PurgeQueries {
            kw: (Kw!("PURGE"), Space1),
            cached: Option<(Kw!("CACHED"), Space1)>,
            queries: Kw!("QUERIES"),
            by: Option<((Space1, Kw!("BY"), Space1), PurgeScope)>,
        },
        TrainModel {
            kw: (Kw!("TRAIN MODEL"), Space1),
            name: Ident,
            as_name: Option<((Space1, Kw!("AS"), Space1), Ident)>,
            not_default: Option<(Space1, Kw!("NOT DEFAULT"))>,
            for_col: Option<((Space1, Kw!("FOR"), Space1), DottedIdent)>,
            with_clause: Option<(Space1, TrainWithClause)>,
            source: Option<(Space1, ModelSource)>,
            using_clause: Option<(Space1, UsingClause)>,
        },
        TuneTable {
            kw: (Kw!("TUNE TABLE"), Space1),
            table_name: TableName,
            options: Vec<(Space1, TuneOption)>,
        },
        UnfreezePlans {
            kw: Kw!("UNFREEZE PLANS"),
            from_upgrade: Option<(Space1, Option<(Kw!("FROM"), Space1)>, Kw!("UPGRADE"))>,
            by: Option<((Space1, Kw!("BY"), Space1), FreezeScope)>,
        },
        UseDatabase {
            kw: (Kw!("USE"), Space1),
            database: Option<(Kw!("DATABASE"), Space1)>,
            name: Ident,
        },
        ValidateModel {
            kw: (Kw!("VALIDATE MODEL"), Space1),
            name: Ident,
            as_name: Option<((Space1, Kw!("AS"), Space1), Ident)>,
            use_name: Option<((Space1, Kw!("USE"), Space1), Ident)>,
            with_clause: Option<(Space1, TrainWithClause)>,
            source: Option<(Space1, ModelSource)>,
        },
    }

    /// What CREATE makes; OR REPLACE where 2026.2 takes it (VIEW, AGGREGATE, ML CONFIGURATION, TRIGGER and the
    /// routines), since CREATE OR REPLACE TABLE, INDEX, USER, ROLE, SCHEMA, DATABASE, MODEL and SERVER fail (-51).
    #[derive(Grammar)]
    pub enum Created {
        BitmapextentIndex {
            kw: (Kw!("BITMAPEXTENT INDEX"), Space1),
            name: Ident,
            on_kw: (Space1, Kw!("ON"), Space1),
            table_kw: Option<(Kw!("TABLE"), Space1)>,
            table_name: TableName,
        },
        Database {
            kw: (Kw!("DATABASE"), Space1),
            file: Option<(Kw!("FILE"), Space1)>,
            name: Ident,
            on_dir: Option<OnDirectory>,
        },
        ForeignTable {
            kw: (Kw!("FOREIGN TABLE"), Space1),
            if_not_exists: IfNotExists,
            table_name: TableName,
            columns: Option<Args<VecSep<(Ident, (Space, DataType)), ArgSep>>>,
            server: (Space1, Kw!("SERVER"), Space1),
            server_name: TableName,
            options: Option<(Space, ForeignTableOptions)>,
            values: Option<(Space1, Kw!("VALUES"), Space, ColumnNameList)>,
            using_json: Option<((Space1, Kw!("USING"), Space), JSONString)>,
            novalidate: Option<(Space1, Kw!("NOVALIDATE"))>,
        },
        Model {
            kw: (Kw!("MODEL"), Space1),
            if_not_exists: IfNotExists,
            name: Ident,
            predicting: (Space1, Kw!("PREDICTING"), Space),
            predicted: Parenthesized<(Ident, Option<(Space1, DataType)>)>,
            source: Option<(Space1, ModelSource)>,
            with_clause: Option<(Space, ModelWithClause)>,
            using_clause: Option<(Space1, UsingClause)>,
        },
        Role {
            kw: (Kw!("ROLE"), Space1),
            name: Ident,
        },
        Schema {
            kw: (Kw!("SCHEMA"), Space1),
            if_not_exists: IfNotExists,
            name: Ident,
        },
        TableAs {
            kw: (Kw!("TABLE"), Space1),
            if_not_exists: IfNotExists,
            nojourn: Option<(Kw!("%NOJOURN"), Space1)>,
            table_name: TableName,
            as_kw: (Space1, Kw!("AS"), Space1),
            with_clause: Option<(WithClause, Space1)>,
            query: SourceQuery,
            options: Option<(Space1, TableOptionList)>,
        },
        User {
            kw: (Kw!("USER"), Space1),
            user: UserSpecification,
        },
        Server {
            server_kw: ServerKw,
            if_not_exists: IfNotExists,
            table_name: TableName,
            type_clause: Option<((Space1, Kw!("TYPE"), Space), ServerType)>,
            fdw: (Space1, Kw!("FOREIGN DATA WRAPPER"), Space1),
            fdw_name: Ident,
            connection: (Space1, CreateServerConnection),
            delimited: Option<(Space1, CreateServerDelimitedIds)>,
            passthrough: Option<(Space1, EnableDisable, Space1, Kw!("PASSTHROUGH"))>,
            pushdown: Option<(Space1, PushdownClause)>,
            using_json: Option<((Space1, Kw!("USING"), Space), JSONString)>,
        },
        Index {
            partitioned: Option<(CreateIndexPartitioned, Space1)>,
            qualifiers: Option<(IndexQualifiers, Space)>,
            index_kw: (Kw!("INDEX"), Space1),
            name: Ident,
            on_kw: (Space1, Kw!("ON"), Space1),
            table_kw: Option<(Kw!("TABLE"), Space1)>,
            table_name: TableName,
            components: Args<VecSep<ComponentSpecification, ArgSep>>,
            as_clause: Option<(
                (Space1, Kw!("AS"), Space1),
                DottedIdent,
                Option<(Space, CreateIndexAsParameters)>,
            )>,
            with_clause: Option<(Space1, CreateIndexWithClause)>,
            in_on: Option<(Space1, InOrOn, (Space, NameOrLiteral))>,
            defer_immediate: Option<(Space1, ImmediateOrDefer, Option<(Space1, Kw!("BUILD"))>)>,
        },
        View {
            or_replace: OrReplace,
            definition: ViewDefinition,
        },
        Method(CreateRoutine<MethodPrefix, Kw!("METHOD"), MethodBody>),
        Query(CreateRoutine<OrReplace, Kw!("QUERY"), QueryBody>),
        Routine(CreateRoutine<OrReplace, Either<Kw!("FUNCTION"), ProcedureKw>, CodeBody>),
        Aggregate {
            or_replace: OrReplace,
            agg_kw: (Kw!("AGGREGATE"), Space1),
            name: DottedIdent,
            params: Args<VecSep<(Ident, Space1, DataType), ArgSep>>,
            returns: Option<((Space1, Kw!("RETURNS"), Space1), DataType)>,
            initialize: Option<((Space1, Kw!("INITIALIZE WITH"), Space1), DottedIdent)>,
            iterate: ((Space1, Kw!("ITERATE WITH"), Space1), DottedIdent),
            merge: Option<((Space1, Kw!("MERGE WITH"), Space1), DottedIdent)>,
            finalize: Option<((Space1, Kw!("FINALIZE WITH"), Space1), DottedIdent)>,
        },
        MlConfiguration {
            or_replace: OrReplace,
            ml_kw: (Kw!("ML CONFIGURATION"), Space1),
            name: Ident,
            provider: ((Space1, Kw!("PROVIDER"), Space1), ProviderName),
            desc: Option<((Space1, Kw!("%DESCRIPTION"), Space), LiteralOrNull)>,
            using_clause: Option<(Space1, MlConfigUsingClause)>,
            settings: Option<(Space1, ProviderSpecificSettings)>,
        },
        Trigger {
            or_replace: OrReplace,
            trigger_kw: (Kw!("TRIGGER"), Space1),
            name: TableName,
            time: (Space1, TriggerActionTime, Space1),
            events: VecSep<TriggerEvent, ArgSep>,
            order: Option<(Space1, Kw!("ORDER"), Space, Literal)>,
            on: (Space1, Kw!("ON"), Space1, TableName),
            referencing: Option<(
                Space1,
                Kw!("REFERENCING"),
                Vec1<(Space1, TriggerReferencing)>,
            )>,
            for_each: Option<(Space1, Kw!("FOR EACH"), Space1, TriggerOrientation)>,
            when: Option<(
                Space1,
                Kw!("WHEN"),
                Space,
                Parenthesized<BooleanValueExpression>,
            )>,
            body: (Space1, TriggerBody),
        },
        Table {
            scope: Option<(TableScope, Space1)>,
            table_kw: (Kw!("TABLE"), Space1),
            if_not_exists: IfNotExists,
            table_name: TableName,
            elements: Args<VecSep<TableElement, ArgSep>>,
            options: Option<(Space1, TableOptionList)>,
        },
    }

    /// What ALTER changes; the statistics forms first, since ALTER TABLE t FIX STATISTICS is one of them.
    #[derive(Grammar)]
    pub enum Altered {
        Stats {
            scope: StatisticsScope,
            action: (Space1, StatisticsAction),
        },
        ForeignTable {
            kw: (Kw!("FOREIGN TABLE"), Space1),
            table_name: TableName,
        },
        MlConfiguration {
            kw: (Kw!("ML CONFIGURATION"), Space1),
            name: Ident,
            clauses: Vec1<(Space1, MlConfigClause)>,
        },
        Model {
            kw: (Kw!("MODEL"), Space1),
            name: Ident,
            action: (Space1, AlterModelAction),
        },
        Table {
            kw: (Kw!("TABLE"), Space1),
            table_name: TableName,
            with_check: Option<((Space1, Kw!("WITH"), Space1), CheckOrNoCheck)>,
            action: Option<(Space1, AlterAction)>,
        },
        User {
            kw: (Kw!("USER"), Space1),
            user: UserSpecification,
        },
        Server {
            server_kw: ServerKw,
            table_name: TableName,
            action: (Space1, AlterServerAction),
        },
        View(ViewDefinition),
    }

    #[derive(Grammar)]
    pub enum StatisticsAction {
        DropFixed {
            kw: (Kw!("DROP FIXED"), Space1),
            clause: StatisticsKeyword,
        },
        Fix {
            kw: (Kw!("FIX"), Space1),
            clause: StatisticsKeyword,
        },
        SetRuntime {
            kw: (Kw!("SET RUNTIME"), Space1),
            property: StatisticsProperty,
            eq: (Space, StringEq!("="), Space),
            value: TrueFalse,
        },
    }

    /// What DROP removes; an index by its dotted name, ON its table, or both (2026.2 takes DROP INDEX t.i ON t).
    #[derive(Grammar)]
    pub enum Dropped {
        Trigger {
            kw: (Kw!("TRIGGER"), Space1),
            if_exists: IfExists,
            name: TableName,
            from: Option<((Space1, Kw!("FROM"), Space1), TableName)>,
        },
        Database {
            kw: (Kw!("DATABASE"), Space1),
            if_exists: IfExists,
            name: Ident,
            retain: Option<(Space1, Kw!("RETAIN_FILES"))>,
        },
        ForeignTable {
            kw: (Kw!("FOREIGN TABLE"), Space1),
            if_exists: IfExists,
            table_name: TableName,
            cascade: Option<(Space1, DropBehavior)>,
        },
        Routine {
            kind: (RoutineKind, Space),
            if_exists: IfExists,
            name: DottedIdent,
            from: Option<((Space1, Kw!("FROM"), Space1), DottedIdent)>,
        },
        Index {
            kw: (Kw!("INDEX"), Space1),
            if_exists: IfExists,
            nojourn: Option<(Kw!("%NOJOURN"), Space1)>,
            name: VecSep<Ident, (Space, StringEq!("."), Space)>,
            on: Option<(
                (Space1, Kw!("ON"), Space1),
                Option<(Kw!("TABLE"), Space1)>,
                TableName,
            )>,
        },
        MlConfiguration {
            kw: (Kw!("ML CONFIGURATION"), Space1),
            if_exists: IfExists,
            name: Ident,
        },
        Model {
            kw: (Kw!("MODEL"), Space1),
            if_exists: IfExists,
            name: Ident,
        },
        Role {
            kw: (Kw!("ROLE"), Space1),
            if_exists: IfExists,
            name: Ident,
        },
        Schema {
            kw: (Kw!("SCHEMA"), Space1),
            if_exists: IfExists,
            name: Ident,
            behavior: Option<(Space1, DropBehavior)>,
        },
        Table {
            kw: (Kw!("TABLE"), Space1),
            if_exists: IfExists,
            nojourn: Option<(Kw!("%NOJOURN"), Space1)>,
            table_name: TableName,
            cascade: Option<(Space1, DropBehavior)>,
            data: Option<(Space1, DataOption)>,
        },
        User {
            kw: (Kw!("USER"), Space1),
            if_exists: IfExists,
            name: Ident,
            cascade: Option<(Space1, Kw!("CASCADE"))>,
        },
        View {
            kw: (Kw!("VIEW"), Space1),
            if_exists: IfExists,
            table_name: TableName,
            cascade: Option<(Space1, DropBehavior)>,
        },
        Server {
            server_kw: ServerKw,
            if_exists: IfExists,
            table_name: TableName,
            cascade: Option<(Space1, DropBehavior)>,
        },
        Aggregate {
            kw: (Kw!("AGGREGATE"), Space1),
            if_exists: IfExists,
            name: DottedIdent,
        },
    }

    #[derive(Grammar)]
    pub struct OnDirectory {
        pub kw: (Space1, Kw!("ON DIRECTORY")),
        #[sugar]
        pub eq: Option<(Space, StringEq!("="))>,
        pub dir: (Space, Literal),
    }

    #[derive(Grammar)]
    pub struct ViewDefinition {
        pub kw: (Kw!("VIEW"), Space1),
        pub table_name: TableName,
        pub col_list: Option<(Space, ColumnNameList)>,
        pub as_kw: (Space1, Kw!("AS"), Space),
        pub with_clause: Option<(WithClause, Space1)>,
        pub query: QueryExpression,
        pub view_with: Option<((Space1, Kw!("WITH"), Space1), ViewWithOption)>,
    }

    /// 2026.2 takes STATIC before METHOD alone and not with OR REPLACE (-51), an external language for a FUNCTION
    /// or PROCEDURE alone, and SQL or OBJECTSCRIPT for a QUERY.
    #[derive(Grammar)]
    pub struct CreateRoutine<Before: Grammar, Kind: Grammar, Body: Grammar> {
        pub before: Before,
        pub kind: (Kind, Space),
        pub name: DottedIdent,
        pub spec: (Space, CodeSpecification<Body>),
    }

    pub type MethodPrefix = Option<Either<(Kw!("OR REPLACE"), Space), (Kw!("STATIC"), Space1)>>;

    #[derive(Grammar)]
    pub enum RoutineKind {
        Function(Kw!("FUNCTION")),
        Method(Kw!("METHOD")),
        Procedure(ProcedureKw),
        Query(Kw!("QUERY")),
    }

    pub type ProcedureKw = Kw!("PROCEDURE", "PROC");

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct TableScope {
        pub private_global: GlobalOrPrivate,
        pub space: Space1,
        pub temporary: Kw!("TEMPORARY"),
    }

    #[derive(Grammar)]
    pub enum GlobalOrPrivate {
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
    pub enum CreateIndexAsParameters {
        Parens(Parenthesized<Option<VecSep<IndexParameterSpecification, ArgSep>>>),
        With {
            kw: (Kw!("WITH"), Space1),
            params: VecSep<IndexParameterSpecification, ArgSep>,
        },
    }

    /// 2026.2 takes `WITH DATA (b) PAD_INDEX, FILLFACTOR = 80` and `WITH PAD_INDEX FILLFACTOR = 80`, not
    /// `WITH DATA (b), PAD_INDEX`, `WITH FILLFACTOR = 80, PAD_INDEX` or `WITH PAD_INDEX PAD_INDEX`.
    #[derive(Grammar)]
    #[tygr(inline, validated)]
    pub struct CreateIndexWithClause {
        pub kw: Kw!("WITH"),
        pub data: Option<(Space1, Kw!("DATA"), Space, ColumnNameList)>,
        pub options: Option<(Space1, VecSep<IndexOption, Either<ArgSep, Space1>>)>,
    }

    impl Validate for CreateIndexWithClause {
        const REQUIREMENT: &'static str = "list the index options in order, each once";
        fn validate(&self) -> bool {
            (self.options.iter()).all(|(_, options)| {
                let ranks = options.items.iter().map(IndexOption::rank);
                ranks.clone().zip(ranks.skip(1)).all(|(a, b)| a < b)
            })
        }
    }

    #[derive(Grammar)]
    pub enum IndexOption {
        PadIndex(Kw!("PAD_INDEX")),
        FillFactor(Kw!("FILLFACTOR"), (Space, StringEq!("="), Space), Literal),
        IgnoreDupKey(Kw!("IGNORE_DUP_KEY")),
        DropExisting(Kw!("DROP_EXISTING")),
        StatisticsNoRecompute(Kw!("STATISTICS_NORECOMPUTE")),
    }

    impl IndexOption {
        fn rank(&self) -> u8 {
            match self {
                Self::PadIndex(_) => 0,
                Self::FillFactor(..) => 1,
                Self::IgnoreDupKey(_) => 2,
                Self::DropExisting(_) => 3,
                Self::StatisticsNoRecompute(_) => 4,
            }
        }
    }

    #[derive(Grammar)]
    pub enum ImmediateOrDefer {
        Immediate(Kw!("IMMEDIATE")),
        Defer(Kw!("DEFER")),
    }

    #[derive(Grammar)]
    pub enum Password {
        HostVariable(HostVariable),
        Ident(Ident),
        Literal(Literal),
        NULL(Kw!("NULL")),
    }

    pub type PasswordKeyword = Kw!("IDENTIFY BY", "IDENTIFIED BY", "WITH PASSWORD", "PASSWORD");

    #[derive(Grammar)]
    pub enum BuildScope {
        All(Kw!("ALL")),
        Schema((Kw!("SCHEMA"), Space1), Ident),
        Table {
            table_kw: (Kw!("TABLE"), Space1),
            name: VecSep<Ident, (Space, StringEq!("."), Space)>,
            index: Option<((Space1, Kw!("INDEX"), Space1), VecSep<Ident, ArgSep>)>,
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
        Table((Kw!("TABLE"), Space1), TableName),
    }

    #[derive(Grammar)]
    pub enum PurgeName {
        Literal(Literal),
        Dotted(VecSep<Either<StringEq!("*"), Ident>, (Space, StringEq!("."), Space)>),
    }

    #[derive(Grammar)]
    pub enum FreezeScope {
        Id((Kw!("ID"), Space), Literal),
        Scope(StatisticsScope),
    }

    pub type CancelQueryScope = Prefix<Kw!("IDENTIFIED BY"), Nested<Count, Space1>>;

    pub type ModelWithClause = Prefix<(Kw!("WITH"), Space), Parenthesized<ColumnNameAndTypeList>>;

    #[derive(Grammar)]
    pub enum ModelSource {
        Subquery {
            kw: (Kw!("FROM"), Space),
            query: Parenthesized<Ranged<QuerySpecification>>,
        },
        Inline {
            from: FromClause,
            where_clause: Option<(Space, WhereClause)>,
            group: Option<(Space, GroupByClause)>,
            having: Option<(Space, HavingClause)>,
            tail: Option<(Space, Tail)>,
        },
    }

    pub type UsingClause = Prefix<(Kw!("USING"), Space), JSONObject>;

    pub type TrainWithClause = Prefix<(Kw!("WITH"), Space), TrainWith>;

    #[derive(Grammar)]
    pub enum TrainWith {
        List(ColumnNameList),
        Assignment(VecSep<(Ident, (Space, StringEq!("="), Space), WithExpression), ArgSep>),
    }

    #[derive(Grammar)]
    pub enum WithExpression {
        Ident(Ident),
        Literal(Literal),
        ValueExpression(ValueExpression),
    }

    #[derive(Grammar)]
    pub enum AlterModelAction {
        Default(DefaultTrainedModel),
        Purge(Kw!("PURGE"), Option<(Space, Scope)>),
    }

    #[derive(Grammar)]
    pub struct DefaultTrainedModel {
        pub kw: Kw!("DEFAULT"),
        pub trained_model: Option<(Space1, Kw!("TRAINED MODEL"))>,
        pub name: (Space, Ident),
    }

    #[derive(Grammar)]
    pub enum Scope {
        Days(UnsignedInteger, Space1, Kw!("DAYS")),
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

    pub type MlConfigUsingClause = Prefix<(Kw!("USING"), Space), MlConfigUsing>;

    /// What ALTER ML CONFIGURATION changes, in any order; 2026.2 rejects the statement without a clause.
    #[derive(Grammar)]
    pub enum MlConfigClause {
        Provider(Kw!("PROVIDER"), Space1, ProviderName),
        Description(Kw!("%DESCRIPTION"), Space, LiteralOrNull),
        Using(MlConfigUsingClause),
        Settings(ProviderSpecificSettings),
    }

    #[derive(Grammar)]
    pub enum MlConfigUsing {
        JSONString(JSONObject),
        Null(Kw!("NULL")),
    }

    #[derive(Grammar)]
    pub enum ProviderSpecificSettings {
        DR(VecSep<DataRobotOption, Space>),
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
    pub struct DataRobotOption {
        pub name: Either<Kw!("APITOKEN"), Kw!("URL")>,
        pub value: (Space, LiteralOrNull),
    }

    #[derive(Grammar)]
    pub struct H2OOption {
        pub kw: Kw!("JAVATLSCONFIGFILE"),
        pub value: (Space, LiteralOrNull),
    }

    #[derive(Grammar)]
    pub enum ForeignTableOptions {
        Table((Kw!("TABLE"), Space), Literal),
        Query((Kw!("QUERY"), Space), Literal),
        File(FileSource),
    }

    pub type JSONObject = crate::cos::Brace<crate::cos::OSText>;

    pub type JSONString = Either<JSONObject, StringLiteral>;

    #[derive(Grammar)]
    pub enum ServerType {
        DB(StringEqCI!("'DB'")),
        FILE(StringEqCI!("'FILE'")),
    }

    pub type PushdownClause = Prefix<Kw!("PUSHDOWN"), (Space1, PushdownType)>;

    #[derive(Grammar)]
    pub enum PushdownType {
        Ansi92(Kw!("ANSI92")),
        Basic(Kw!("BASIC")),
        Extended(Kw!("EXTENDED")),
        Iris(Kw!("IRIS")),
        Off(Kw!("OFF")),
    }

    #[derive(Grammar)]
    pub enum TriggerActionTime {
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
        RowAndObject(Kw!("ROW_AND_OBJECT", "ROW AND OBJECT")),
        Row(Kw!("ROW")),
        Statement(Kw!("STATEMENT")),
    }

    #[derive(Grammar)]
    pub enum TriggerBody {
        Code(CodeBody),
        Statement {
            lang: Option<(Kw!("LANGUAGE SQL"), Space1)>,
            statement: Box<Ranged<Statement>>,
        },
    }

    #[derive(Grammar)]
    pub enum LockMode {
        Row(Kw!("ROW")),
        Page(Kw!("PAGE")),
    }

    #[derive(Grammar)]
    pub enum DropBehavior {
        Restrict(Kw!("RESTRICT")),
        Cascade(Kw!("CASCADE")),
    }

    #[derive(Grammar)]
    pub enum DataOption {
        DelData(Kw!("%DELDATA")),
        NoDelData(Kw!("%NODELDATA")),
    }

    #[derive(Grammar)]
    pub struct MoveSpecification {
        pub partition_range: Option<(PartitionRangeSpecification, Space)>,
        pub to_kw: Kw!("TO"),
        pub database: (Space1, Ident),
        pub activate: Option<(Space1, ActivateSpecification)>,
    }

    #[derive(Grammar)]
    pub struct ActivateSpecification {
        pub force_or_defer: Option<(Either<Kw!("FORCE"), Kw!("DEFER")>, Space1)>,
        pub activate_kw: Kw!("ACTIVATE"),
        pub mappings_kw: Option<(Space1, Kw!("MAPPINGS"))>,
    }

    #[derive(Grammar)]
    pub enum ViewWithOption {
        ReadOnly(Kw!("READ ONLY")),
        LeveledCheck {
            levels: (LevelsClause, Space1),
            kw: Kw!("CHECK OPTION"),
        },
        Check {
            kw: Kw!("CHECK OPTION"),
            constraint: Option<(Space1, Kw!("CONSTRAINT"), Space, Ident)>,
        },
    }

    #[derive(Grammar)]
    pub enum LevelsClause {
        Cascaded(Kw!("CASCADED", "CASCADE")),
        Local(Kw!("LOCAL")),
    }

    #[derive(Grammar)]
    pub struct SQL {
        pub space_before: Space,
        pub view_column_list: Option<(ColumnNameList, Option<(Space, Kw!("AS"))>, Space)>,
        pub statement: Ranged<Statement>,
        pub space_after: Space,
    }

    #[derive(Grammar)]
    pub struct ThroughStatement {
        pub kw: (
            Kw!("THROUGH"),
            Space1,
            Option<(Kw!("FOREIGN"), Space1)>,
            Kw!("SERVER"),
            Space,
        ),
        pub table_name: TableName,
        pub remote_statement: (Space, Box<Ranged<Statement>>),
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct WithStatement<T: Grammar> {
        pub with_clause: Option<(WithClause, Space)>,
        pub body: T,
    }

    #[derive(Grammar)]
    pub enum Statement {
        SelectStatement(SelectStatement),
        Insert(WithStatement<InsertStatement>),
        Update(WithStatement<UpdateStatement>),
        Delete(WithStatement<DeleteStatement>),
        ThroughStatement(ThroughStatement),
        Truncate(Truncate),
        DDL(DDL),
        Declare(DeclareCursor),
        CursorStatement(CursorStatement),
        SetOption(SetOptionStatement),
        GrantStatement(GrantStatement),
        RevokeStatement(RevokeStatement),
        TransactionStatement(TransactionStatement),
        Savepoint(Savepoint),
        Lock(LockTable<Kw!("LOCK"), Wait>),
        Unlock(LockTable<Kw!("UNLOCK"), Kw!("IMMEDIATE")>),
        Call(CallStatement),
        ExplainStatement(ExplainStatement),
        SetMlConfigurationStatement(SetMlConfigurationStatement),
        CheckPrivilegeStatement(CheckPrivilegeStatement),
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
    pub struct CodeSpecification<Body: Grammar = CodeBody> {
        pub params: Parenthesized<Option<VecSep<ParameterDeclaration, ArgSep>>>,
        pub characteristics: Vec<(Space, RoutineCharacteristic)>,
        pub body: (Space, Body),
    }

    #[derive(Grammar)]
    pub struct ParameterDeclaration {
        pub direction: Option<(ParameterMode, Space)>,
        pub name: Ident,
        pub data_type: (Space1, DataType),
        pub default: Option<(Space, DefaultClause)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum ParameterMode {
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
    pub enum RoutineCharacteristic {
        ContainId {
            kw: (Kw!("CONTAINID"), Space),
            value: Literal,
        },
        Final(Kw!("FINAL")),
        For {
            kw: (Kw!("FOR"), Space1),
            classname: DottedIdent,
        },
        Private(Kw!("PRIVATE")),
        Proc(ProcedureKw),
        Results {
            kw: (Kw!("RESULTS"), Space),
            clause: Parenthesized<ColumnNameAndTypeList>,
        },
        Returns {
            kw: (Kw!("RETURNS"), Space1),
            data_type: DataType,
        },
        SelectMode {
            kw: (Kw!("SELECTMODE"), Space1),
            mode: SelectMode,
        },
        DynamicResultSets {
            kw: Kw!("DYNAMIC RESULT SETS"),
            count: Option<(Space1, Literal)>,
        },
        ResultSets(Kw!("RESULT SETS")),
    }

    #[derive(Grammar)]
    pub enum SelectMode {
        Display(Kw!("DISPLAY")),
        Logical(Kw!("LOGICAL")),
        Odbc(Kw!("ODBC")),
        Runtime(Kw!("RUNTIME")),
    }

    #[derive(Grammar)]
    pub enum CodeBody {
        Cos(CosBody),
        External(ExternalBody),
        Python(PythonBody),
        Sql(SqlBody),
    }

    #[derive(Grammar)]
    pub enum MethodBody {
        Cos(CosBody),
        Python(PythonBody),
        Sql(SqlBody),
    }

    #[derive(Grammar)]
    pub enum QueryBody {
        Cos(CosBody),
        Sql(SqlBody),
    }

    #[derive(Grammar)]
    pub struct CosBody {
        pub kw: (Kw!("LANGUAGE OBJECTSCRIPT", "LANGUAGE COS"), Space),
        pub body: Either<COSMethodBody, VecSep<QueryMethod, Space>>,
    }

    #[derive(Grammar)]
    pub struct ExternalBody {
        pub kw: (
            Kw!("LANGUAGE"),
            Space1,
            ExternalLanguage,
            Space1,
            Kw!("EXTERNAL NAME"),
            Space,
        ),
        pub name: StringLiteral,
    }

    #[derive(Grammar)]
    pub struct PythonBody {
        pub kw: (Kw!("LANGUAGE PYTHON"), Space),
        pub body: Prefix<StringEq!("{"), StringEndsWith<StringEq!("}")>>,
    }

    #[derive(Grammar)]
    pub struct SqlBody {
        pub kw: Option<(Kw!("LANGUAGE SQL"), Space1)>,
        pub body: SqlMethodBody,
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
            Either<ReturnStatement, Ranged<Statement>>,
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
        HostVariable(HostVariable),
        Marker(StringEq!("?")),
    }

    #[derive(Grammar)]
    pub struct QueryMethod {
        pub kind: QueryMethodKind,
        pub params: Args<Option<VecSep<ParameterDeclaration, ArgSep>>>,
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
    pub enum IndexQualifiers {
        Bitmap(Kw!("BITMAP")),
        Bitslice(Kw!("BITSLICE")),
        Columnar(Kw!("COLUMNAR")),
        Unique { kw: Kw!("UNIQUE"), cluster: Cluster },
        Cluster(ClusterType),
    }

    #[derive(Grammar)]
    pub struct ComponentSpecification {
        pub name: Ident,
        pub collection: Option<(Space, CollectionSpecification)>,
        pub collate: Option<(Space1, Kw!("COLLATE"), Space1, SqlCollation)>,
        pub direction: Option<(Space, ComponentDirection)>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub enum ComponentDirection {
        Asc(Kw!("ASC")),
        Desc(Kw!("DESC")),
    }

    pub type CollectionSpecification = Parenthesized<Either<Kw!("ELEMENTS"), Kw!("KEYS")>>;

    #[derive(Grammar)]
    pub struct IndexParameterSpecification {
        pub name: Ident,
        pub eq: (Space, StringEq!("="), Space),
        pub value: Literal,
    }

    #[derive(Grammar)]
    pub enum AlterAction {
        CheckConstraintAll {
            check: CheckOrNoCheck,
            kw: (Space1, Kw!("CONSTRAINT ALL")),
        },
        DropAllPartitions {
            drop_kw: DropKeyword,
            kw: (Space1, Kw!("ALL PARTITIONS")),
        },
        DropPartition {
            drop_kw: DropKeyword,
            partition: (Space1, PartitionRangeSpecification),
        },
        DropAction {
            drop_kw: DropKeyword,
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
            alter: (Space, AlterColumnAction),
        },
        Cleanup(Kw!("CLEANUP")),
        Convert {
            kw: (Kw!("CONVERT"), Space1),
            clause: ConvertClause,
        },
        LockMode {
            kw: (Kw!("LOCK MODE"), Space),
            spec: Parenthesized<LockMode>,
        },
        Modify {
            kw: (Kw!("MODIFY"), Space),
            specs: Either<Parenthesized<ModifySpecificationList>, ModifySpecificationList>,
        },
        Move {
            kw: (Kw!("MOVE"), Space1),
            spec: MoveSpecification,
        },
        Rename {
            kw: (Kw!("RENAME"), Space1),
            name: Ident,
        },
        Truncate {
            kw: (Kw!("TRUNCATE"), Space1),
            partition: PartitionRangeSpecification,
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
            data: Option<(Space1, DataOption)>,
        },
        Shard {
            to: Option<(Kw!("TO"), Space1)>,
            clause: ShardConvertClause,
            timing: Option<(Space, ImmediateOrDefer)>,
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
    pub enum AlterColumnAction {
        DatypCollate {
            data_type: DataType,
            collate: Option<(Space1, Kw!("COLLATE"), Space1, SqlCollation)>,
        },
        SqlCollation(SqlCollation),
        Collate {
            kw: (Kw!("COLLATE"), Space),
            name: SqlCollation,
        },
        Default {
            kw: (Kw!("DEFAULT"), Space),
            value: DefaultOption,
        },
        DropDefault(Kw!("DROP DEFAULT")),
        NotNull(Kw!("NOT NULL")),
        Null(Kw!("NULL")),
        Rename {
            kw: (Kw!("RENAME"), Space1),
            name: Ident,
        },
        SetDefault {
            kw: (Kw!("SET DEFAULT"), Space),
            value: DefaultOption,
        },
    }

    #[derive(Grammar)]
    pub enum StorageDefaultConvertClause {
        Column(ColumnStorageDefaultConversion),
        Type(StorageDefaultConversionTypeSpecification),
    }

    #[derive(Grammar)]
    pub struct StorageDefaultConversionTypeSpecification {
        pub to: Option<(Kw!("TO"), Space1)>,
        pub kw: (Kw!("STORAGETYPE"), Space, StringEq!("="), Space),
        pub typ: StorageType,
    }

    #[derive(Grammar)]
    pub struct ColumnStorageDefaultConversion {
        pub kw: Kw!("COLUMNS", "COLUMN"),
        pub list: (Space, VecSep<Ident, ArgSep>),
        pub spec: (Space, StorageDefaultConversionTypeSpecification),
    }

    /// A DEFAULT value, in parentheses to any depth (2026.2 takes ((1))).
    #[derive(Grammar)]
    pub enum DefaultOption {
        Spec(BareDefaultOption),
        Paren(Parenthesized<Box<DefaultOption>>),
    }

    #[derive(Grammar)]
    pub enum BareDefaultOption {
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
            precision: Option<(Space, Parenthesized<UnsignedInteger>)>,
        },
        CurrentTime {
            kw: Kw!("CURRENT_TIME"),
            precision: Option<(Space, Parenthesized<UnsignedInteger>)>,
        },
        CurrentUser(Kw!("CURRENT_USER")),
        GetDate {
            kw: Kw!("GETDATE"),
            precision: Args<Option<UnsignedInteger>>,
        },
        GetUtcDate {
            kw: Kw!("GETUTCDATE"),
            precision: Args<Option<UnsignedInteger>>,
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
    pub struct ModifySpecification {
        pub name: Ident,
        pub change: (Space1, ModifyChange),
    }

    /// 2026.2 takes `a INT DEFAULT 1 CONSTRAINT nn NOT NULL` in this order, at least one part, `a RENAME b` and
    /// `a SET DEFAULT 1`, not `a NOT NULL DEFAULT 1`, `a COLLATE SQLUPPER` or `a` alone.
    #[derive(Grammar)]
    pub enum ModifyChange {
        Rename {
            kw: (Kw!("RENAME"), Space1),
            new_name: Ident,
        },
        SetDefault {
            kw: (Kw!("SET DEFAULT"), Space),
            value: DefaultOption,
        },
        Clauses(ModifyColumnClauses),
        Typed {
            data_type: DataType,
            clauses: Option<(Space1, ModifyColumnClauses)>,
        },
    }

    pub type ModifySpecificationList = VecSep<ModifySpecification, ArgSep>;

    #[derive(Grammar)]
    #[tygr(validated)]
    pub struct ModifyColumnClauses {
        pub default: Option<(Kw!("DEFAULT"), Space, DefaultOption)>,
        pub constraint: Option<(Space, Kw!("CONSTRAINT"), Space1, Ident)>,
        pub null: Option<(Space, Nullability)>,
    }

    impl Validate for ModifyColumnClauses {
        const REQUIREMENT: &'static str = "name a default, a constraint or nullability";
        fn validate(&self) -> bool {
            self.default.is_some() || self.constraint.is_some() || self.null.is_some()
        }
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct Nullability {
        pub not: Option<(Kw!("NOT"), Space1)>,
        pub null: Kw!("NULL"),
    }

    pub type DropKeyword = Kw!("DROP", "DELETE");

    #[derive(Grammar)]
    pub enum DropAction {
        Constraint {
            kw: (Kw!("CONSTRAINT"), Space),
            name: Ident,
        },
        ForeignKey {
            kw: (Kw!("FOREIGN KEY"), Space),
            name: Ident,
        },
        PrimaryKey(Kw!("PRIMARY KEY")),
        Column {
            column_kw: Option<(Kw!("COLUMN"), Space)>,
            items: VecSep<DropColumnItem, ArgSep>,
        },
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct DropColumnItem {
        pub name: Ident,
        pub cascade: Option<(Space1, DropBehavior)>,
        pub data: Option<(Space1, DataOption)>,
    }

    #[derive(Grammar)]
    pub struct AddAction {
        pub constraint: Option<(Kw!("CONSTRAINT"), Space, TableName, Space)>,
        pub body: AddBody,
    }

    pub type AddActionList = VecSep<AddAction, ArgSep>;

    #[derive(Grammar)]
    pub enum AddBody {
        ForeignKey(OptionallyWrapped<AddForeignKey>),
        PrimaryKey {
            spec: OptionallyWrapped<AddPrimaryKey>,
            on: Option<(Space1, Kw!("ON"), Space, NameOrLiteral)>,
        },
        Unique {
            spec: OptionallyWrapped<AddUnique>,
            fillfactor: FillFactor,
            on: Option<(Space1, Kw!("ON"), Space, NameOrLiteral)>,
        },
        ColDef(OptionallyWrapped<AddColumnDefinition>),
        Index(TableIndex),
        Default {
            kw: (Kw!("DEFAULT"), Space),
            value: DefaultOption,
            for_kw: (Space1, Kw!("FOR"), Space1),
            name: Ident,
        },
    }

    pub type AddColumnDefinition =
        Prefix<Option<(Kw!("COLUMN"), Space)>, OptionallyWrapped<ColumnDefinition>>;

    #[derive(Grammar)]
    pub enum CheckOrNoCheck {
        Check(Kw!("CHECK")),
        NoCheck(Kw!("NOCHECK")),
    }

    #[derive(Grammar)]
    pub struct AddPrimaryKey {
        pub kw: (Kw!("PRIMARY KEY"), Space),
        pub spec: NamedKeyColumns,
    }

    /// FOREIGN KEY [name] (columns) REFERENCES ...: 2026.2 takes the name in ALTER TABLE ADD, not in CREATE TABLE.
    #[derive(Grammar)]
    pub struct AddForeignKey {
        pub kw: (Kw!("FOREIGN KEY"), Space),
        pub flist2: NamedKeyColumns,
        pub references_kw: (Space1, Kw!("REFERENCES"), Space1),
        pub ref_: ReferencesSpecification,
    }

    #[derive(Grammar)]
    pub struct TableForeignKey {
        pub kw: (Kw!("FOREIGN KEY"), Space),
        pub columns: KeyColumns,
        pub references_kw: (Space1, Kw!("REFERENCES"), Space1),
        pub ref_: ReferencesSpecification,
    }

    /// `[CONSTRAINT name] INDEX|KEY name (components)`, a table element and an ALTER TABLE ADD action on 2026.2.
    #[derive(Grammar)]
    pub struct TableIndex {
        pub kw: (Kw!("INDEX", "KEY"), Space),
        pub name: DottedIdent,
        pub components: Args<VecSep<ComponentSpecification, ArgSep>>,
    }

    #[derive(Grammar)]
    pub struct AddUnique {
        pub kw: Kw!("UNIQUE"),
        pub spec: UniqueSpecification,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct UniqueSpecification {
        pub cluster: Cluster,
        pub columns: (Space, NamedKeyColumns),
    }

    #[derive(Grammar)]
    pub enum InOrOn {
        In(Kw!("IN")),
        On(Kw!("ON")),
    }

    pub type NameOrLiteral = Either<Ident, Literal>;

    #[derive(Grammar)]
    pub struct TableElement {
        pub constraint: Option<(Kw!("CONSTRAINT"), Space, Ident, Space)>,
        pub option: TableElementOption,
    }

    #[derive(Grammar)]
    pub enum TableElementOption {
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
        ExtentSizeClause(ExtentSizeClause),
        Shard(Shard),
        TableForeignKey(TableForeignKey),
        TablePrimaryKey(TablePrimaryKey),
        TableUnique(TableUnique),
        Index(TableIndex),
        ColumnDefinition(ColumnDefinition),
    }

    #[derive(Grammar)]
    pub struct ColumnDefinition {
        pub name: Ident,
        pub typ: (Space1, ColumnDataType),
        pub qualifiers: Vec<(Space, ColumnQualifier)>,
    }

    #[derive(Grammar)]
    pub enum ColumnDataType {
        DataType(DataType),
        Identity {
            data_type: Option<DataType>,
            identity: (Space, Identity),
        },
    }

    pub type Identity = Prefix<Kw!("IDENTITY"), Option<Args<(Literal, ArgSep, Literal)>>>;

    pub type TableUnique = Prefix<Kw!("UNIQUE"), TableUniqueSpecification>;

    #[derive(Grammar)]
    pub struct TableUniqueSpecification {
        pub cluster: Cluster,
        pub columns: (Space, KeyColumns),
    }

    #[derive(Grammar)]
    pub enum KeyColumns {
        Parens(ColumnNameList),
        Bare(VecSep<Ident, ArgSep>),
    }

    pub type FillFactor = Option<(
        Space,
        Kw!("WITH FILLFACTOR"),
        Space,
        StringEq!("="),
        Space,
        Literal,
    )>;

    pub type TablePrimaryKey = Prefix<(Kw!("PRIMARY KEY"), Space), TableUniqueSpecification>;

    #[derive(Grammar)]
    pub struct ExtentSizeClause {
        pub kw: (Kw!("%EXTENTSIZE", "%NUMROWS"), Space),
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

    pub type Shard = Prefix<Kw!("SHARD"), ShardSpecification>;
    pub type Sharded = Prefix<Kw!("SHARDED"), ShardSpecification>;

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ShardSpecification {
        pub key: Option<(
            Space,
            Kw!("KEY"),
            Space,
            ColumnNameList,
            Option<(Space, CoshardClause)>,
        )>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct CoshardClause {
        pub kw: Kw!("COSHARD"),
        pub with: Option<(Space1, Kw!("WITH"))>,
        pub name: Either<Args<TableName>, (Space, TableName)>,
    }

    #[derive(Grammar)]
    pub enum TableOptionList {
        WithOptions {
            shard: Option<(Shard, Space)>,
            partition: Option<(PartitionOption, Space)>,
            with_kw: (Kw!("WITH"), Space1),
            options: VecSep<TableOption, ArgSep>,
            informix: Option<(Space, InformixCompatibility)>,
        },
        Shard {
            shard: Shard,
            partition: Option<(Space, PartitionOption)>,
        },
        Partition(PartitionOption),
        InformixCompat(InformixCompatibility),
    }

    #[derive(Grammar)]
    pub struct InformixCompatibility {
        pub in_on: Option<(InOrOn, Space, NameOrLiteral)>,
        pub extent_size: Option<(Space, Kw!("EXTENT SIZE"), Space, Literal)>,
        pub next_size: Option<(Space, Kw!("NEXT SIZE"), Space, Literal)>,
    }

    #[derive(Grammar)]
    pub enum TableOption {
        ClassParam(ClassParameter),
        StorageType {
            kw: Kw!("STORAGETYPE"),
            #[sugar]
            eq: Option<(Space, StringEq!("="))>,
            value: (Space, StorageType),
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

    pub type PartitionSpecificationList = VecSep<PartitionSpecification, ArgSep>;

    #[derive(Grammar)]
    pub enum PartitionSpecification {
        Hash {
            kw: (Kw!("HASH"), Space),
            column: Parenthesized<Ident>,
            in_kw: (Space1, Kw!("IN"), Space),
            count: Literal,
            partitions_kw: (Space1, Kw!("PARTITIONS")),
        },
        List {
            kw: (Kw!("LIST"), Space),
            column: Parenthesized<Ident>,
        },
        Range {
            kw: (Kw!("RANGE"), Space),
            column: Parenthesized<Ident>,
            interval_kw: (Space1, Kw!("INTERVAL"), Space),
            interval: Literal,
            datetime_interval: Option<(Space1, PartitionDatetimeInterval)>,
        },
    }

    #[derive(Grammar)]
    pub enum PartitionDatetimeInterval {
        Day(Kw!("DAY", "DAYS")),
        Hour(Kw!("HOUR", "HOURS")),
        Minute(Kw!("MINUTE", "MINUTES")),
        Month(Kw!("MONTH", "MONTHS")),
        Year(Kw!("YEAR", "YEARS")),
    }

    #[derive(Grammar)]
    pub struct PartitionRangeSpecification {
        pub kw: Kw!("PARTITION"),
        pub range: PartitionRange,
    }

    #[derive(Grammar)]
    pub enum PartitionRange {
        Id {
            kw: (Space1, Kw!("ID"), Space),
            prefix: Literal,
        },
        IdBetween {
            kw: (Space1, Kw!("ID")),
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
    pub enum ColumnQualifier {
        Description {
            kw: (Kw!("%DESCRIPTION"), Space),
            value: Literal,
        },
        ColumnConstraintDefinition(ColumnConstraintDefinition),
        SqlCollation(SqlCollation),
        Calculated(Kw!("CALCULATED")),
        CharacterSet {
            kw: (Kw!("CHARACTER SET"), Space),
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
            list: ColumnNameList,
        },
        Default {
            kw: (Kw!("DEFAULT"), Space),
            value: DefaultOption,
        },
        OnUpdate {
            kw: (Kw!("ON UPDATE"), Space),
            value: OnUpdateValue,
        },
        Private(Kw!("PRIVATE")),
        StorageType {
            kw: (Kw!("WITH STORAGETYPE"), Space, StringEq!("="), Space),
            value: StorageType,
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
    pub enum StorageType {
        Columnar(Kw!("COLUMNAR")),
        Row(Kw!("ROW")),
    }

    #[derive(Grammar)]
    pub struct DataType {
        pub spec: DataTypeSpecification,
        pub args: Option<Args<DataTypeArgs>>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct DataTypeArgs(
        pub Vec<Either<Parenthesized<Box<DataTypeArgs>>, StringOf1<IsNotParen>>>,
    );

    #[derive(Grammar)]
    pub enum DataTypeSpecification {
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
        LongVarchar(Kw!("LONG VARCHAR")),
        Longvarbinary(Kw!("LONGVARBINARY")),
        Longvarchar(Kw!("LONGVARCHAR")),
        Long(Kw!("LONG")),
        Money(Kw!("MONEY")),
        NationalVarchar(Kw!("NATIONAL VARCHAR")),
        NationalChar {
            kw: Kw!("NATIONAL"),
            char_kw: Option<(
                Space1,
                Kw!("CHARACTER", "CHAR"),
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
            head: DataTypeName,
            rest: Vec<(StringEq!("."), Ident)>,
        },
    }

    #[derive(Grammar)]
    #[tygr(validated, inline)]
    pub struct DataTypeName(pub Ident);

    impl Validate for DataTypeName {
        const REQUIREMENT: &'static str = "not be a column clause keyword";
        fn validate(&self) -> bool {
            !matches!(
                self.0.print().to_uppercase().as_str(),
                "COLLATE" | "CONSTRAINT" | "DEFAULT" | "DROP" | "NOT" | "NULL" | "RENAME" | "SET"
            )
        }
    }

    #[derive(Grammar)]
    pub enum ColumnConstraintDefinition {
        Constraint {
            kw: (Kw!("CONSTRAINT"), Space),
            name: TableName,
            spec: (Space, NamedColumnConstraint),
        },
        Identity {
            kw: Kw!("IDENTITY"),
            args: Option<Args<(Literal, Option<(ArgSep, Literal)>)>>,
        },
        NotNull(Kw!("NOT NULL")),
        Null(Kw!("NULL")),
        Primary {
            kw: (Kw!("PRIMARY"), Option<(Space1, Kw!("KEY"))>),
            cluster: Cluster,
        },
        References {
            kw: (Kw!("REFERENCES"), Space1),
            reference: ReferencesSpecification,
        },
        Unique {
            kw: Kw!("UNIQUE"),
            cluster: Cluster,
        },
    }

    #[derive(Grammar)]
    pub enum NamedColumnConstraint {
        Default {
            kw: (Kw!("DEFAULT"), Space),
            value: DefaultOption,
        },
        NotNull(Kw!("NOT NULL")),
        PrimaryKey {
            kw: Kw!("PRIMARY KEY"),
            cluster: Cluster,
        },
        References {
            kw: (Kw!("REFERENCES"), Space1),
            reference: ReferencesSpecification,
        },
        Unique {
            kw: Kw!("UNIQUE"),
            cluster: Cluster,
        },
    }

    #[derive(Grammar)]
    pub enum OnUpdateValue {
        Negative {
            minus: (StringEq!("-"), Space),
            value: Number,
        },
        Literal(Literal),
        CurrentDate(Kw!("CURRENT_DATE")),
        CurrentTimestamp {
            kw: Kw!("CURRENT_TIMESTAMP"),
            precision: Option<(Space, Parenthesized<UnsignedInteger>)>,
        },
        CurrentTime {
            kw: Kw!("CURRENT_TIME"),
            precision: Option<(Space, Parenthesized<UnsignedInteger>)>,
        },
        CurrentUser(Kw!("CURRENT_USER")),
        GetDate {
            kw: Kw!("GETDATE"),
            precision: Parenthesized<Option<UnsignedInteger>>,
        },
        GetUtcDate {
            kw: Kw!("GETUTCDATE"),
            precision: Parenthesized<Option<UnsignedInteger>>,
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
    pub struct ReferencesSpecification {
        pub name: TableName,
        pub columns: Option<(Space, ColumnNameList)>,
        pub match_spec: Option<(Space, Kw!("MATCH"), Space1, MatchType)>,
        pub action: Option<(Space, Either<ReferentialTriggeredAction, Kw!("NOCHECK")>)>,
    }

    /// ON UPDATE and ON DELETE in either order, NOCHECK after both: 2026.2 refuses two ON DELETE rules,
    /// `ON DELETE CASCADE NOCHECK` and `NOCHECK ON DELETE CASCADE`.
    #[derive(Grammar)]
    pub enum ReferentialTriggeredAction {
        UpdateFirst(TriggeredRules<Kw!("UPDATE"), Kw!("DELETE")>),
        DeleteFirst(TriggeredRules<Kw!("DELETE"), Kw!("UPDATE")>),
    }

    #[derive(Grammar)]
    pub struct TriggeredRules<First: Grammar, Second: Grammar> {
        pub first: ReferentialRule<First>,
        pub second: Option<(
            Space1,
            ReferentialRule<Second>,
            Option<(Space1, Kw!("NOCHECK"))>,
        )>,
    }

    pub type ReferentialRule<Event> = Prefix<(Kw!("ON"), Space1, Event, Space1), ReferentialAction>;

    #[derive(Grammar)]
    pub enum MatchType {
        Full(Kw!("FULL")),
        Partial(Kw!("PARTIAL")),
    }

    #[derive(Grammar)]
    pub enum ReferentialAction {
        Cascade(Kw!("CASCADE")),
        NoAction(Kw!("NO ACTION")),
        SetDefault(Kw!("SET DEFAULT")),
        SetNull(Kw!("SET NULL")),
    }

    #[derive(Grammar)]
    pub enum NamedKeyColumns {
        Named {
            name: Ident,
            columns: (Space, ColumnNameList),
        },
        Columns(KeyColumns),
    }

    #[derive(Grammar)]
    pub enum LoadSpecification {
        Sql {
            kw: (Kw!("SQL"), Space1),
            spec: LoadSqlSpecification,
        },
        Data {
            hints: Option<LoadHints>,
            kw: (Kw!("DATA"), Space1),
            spec: LoadDataSpecification,
        },
    }

    /// BULK takes %NOJOURN then %NOINDEX: 2026.2 refuses `BULK %NOINDEX %NOJOURN`.
    #[derive(Grammar)]
    pub enum LoadHints {
        Bulk(
            Kw!("BULK"),
            Space1,
            Option<(Kw!("%NOJOURN"), Space1)>,
            Option<(Kw!("%NOINDEX"), Space1)>,
        ),
        Set(LoadHintSet),
    }

    /// The % hints in any order; each once is the check's rule (A3825).
    #[derive(Grammar)]
    pub struct LoadHintSet(pub Vec1<(Ranged<LoadHint>, Space1)>);

    #[derive(Grammar)]
    pub enum LoadHint {
        NoCheck(Kw!("%NOCHECK")),
        NoIndex(Kw!("%NOINDEX")),
        NoJourn(Kw!("%NOJOURN")),
        NoLock(Kw!("%NOLOCK")),
    }

    #[derive(Grammar)]
    pub enum LoadDataSpecification {
        Inline(LoadDataSpecificationInline),
        WithUsing(LoadDataSpecificationWithUsing),
    }

    #[derive(Grammar)]
    pub struct LoadDataSpecificationInline {
        pub from: LoadDataFrom,
        pub into: (Space, LoadDataInto),
        pub using: Option<(Space, LoadDataUsing)>,
    }

    #[derive(Grammar)]
    pub struct LoadDataSpecificationWithUsing {
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
            connection: JdbcConnectionSpecification,
            data: (Space, JdbcDataSpecification),
        },
    }

    pub type ColumnNameAndTypeList = VecSep<ColumnNameAndType, ArgSep>;

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct FileSource {
        pub kw: (Kw!("FILE"), Space),
        pub path: Literal,
        pub columns: Option<(
            Space,
            Kw!("COLUMNS"),
            Space,
            Parenthesized<ColumnNameAndTypeList>,
        )>,
    }

    #[derive(Grammar)]
    #[tygr(inline)]
    pub struct ColumnNameAndType {
        pub name: Ident,
        pub data_type: (Space1, DataType),
    }

    #[derive(Grammar)]
    pub enum JdbcConnectionSpecification {
        Connection {
            kw: Option<(Kw!("CONNECTION"), Space)>,
            spec: ConnectionSpecification,
        },
        Url {
            kw: Option<(Kw!("URL"), Space)>,
            value: Literal,
        },
    }

    #[derive(Grammar)]
    pub enum JdbcDataSpecification {
        Query {
            kw: (Kw!("QUERY"), Space),
            value: Literal,
        },
        Table {
            kw: (Kw!("TABLE"), Space1),
            name: VecSep<Ident, StringEq!(".")>,
        },
    }

    #[derive(Grammar)]
    pub enum ConnectionSpecification {
        Ident(Ident),
        Literal(Literal),
    }

    #[derive(Grammar)]
    pub struct LoadDataInto {
        pub kw: (Kw!("INTO"), Space),
        pub table: TableName,
        pub columns: Option<(Space, ColumnNameList)>,
        pub values: Option<(Space1, Kw!("VALUES"), Space, ColumnNameList)>,
    }

    pub type LoadDataUsing = Prefix<(Kw!("USING"), Space), LoadOptions>;

    #[derive(Grammar)]
    pub enum LoadOptions {
        Json(JSONString),
        Name(Ident),
    }

    #[derive(Grammar)]
    pub struct LoadSqlSpecification {
        pub kw: (Kw!("FROM"), Space1),
        pub source: SqlSource,
        pub dialect: Option<(Space, Kw!("DIALECT"), Space, Literal)>,
        pub delimiter: Option<(Space, Kw!("DELIMITER"), Space, Literal)>,
        pub verbose: Option<(Space, Kw!("VERBOSE"))>,
        pub log_to_file: Option<(Space, LoadSqlLogToFile)>,
        pub ignore_errors: Option<(Space, Kw!("IGNORE ERRORS"))>,
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

    pub type LoadSqlLogToFile = Prefix<Kw!("LOG TO FILE"), Option<(Space, Literal)>>;

    #[derive(Grammar)]
    pub struct AlterServerAction {
        pub kw: (Kw!("ALTER", "MODIFY"), Space1),
        pub action: SubAlterServerAction,
    }

    #[derive(Grammar)]
    pub enum SubAlterServerAction {
        FtPushdown(PushdownClause),
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
    pub enum VectorDataType {
        Decimal(Kw!("DECIMAL")),
        Double(Kw!("DOUBLE")),
        Float(Kw!("FLOAT")),
        Integer(Kw!("INTEGER", "INT")),
        String(Kw!("STRING")),
    }

    pub type StatisticsKeyword = Kw!("STATISTICS", "STATS");

    #[derive(Grammar)]
    pub enum StatisticsScope {
        Schema {
            kw: (Kw!("SCHEMA"), Space1),
            schema: Ident,
        },
        Table {
            kw: (Kw!("TABLE"), Space1),
            table: TableName,
        },
    }

    #[derive(Grammar)]
    pub enum StatisticsProperty {
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
    pub enum PercentOperator {
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
        String(Kw!("%STRING")),
        SqlString(Kw!("%SQLSTRING")),
        SqlUpper(Kw!("%SQLUPPER")),
        Truncate(Kw!("%TRUNCATE")),
    }

    #[derive(Grammar, Clone)]
    #[tygr(inline)]
    pub struct PositionArgs {
        pub needle: ValueExpression,
        pub in_kw: (Space1, Kw!("IN"), Space),
        pub haystack: ValueExpression,
    }

    /// The escape's SUBSTRING takes the FROM form or the comma form; called by name, the comma form is a Call.
    pub type SubstringArgs = Args<Either<SubstringFrom, RRO>>;

    /// `SUBSTRING(a FROM 1 FOR 2)`, the arguments of `SUBSTRING(a, 1, 2)`.
    #[derive(Grammar, Clone)]
    pub struct SubstringFrom {
        pub expr: ValueExpression,
        pub from: ((Space, Kw!("FROM"), Space), ValueExpression),
        pub length: Option<((Space, Kw!("FOR"), Space), ValueExpression)>,
    }

    pub type ODBCEscape = Wrap<(StringEq!("{"), Space), ODBCEscapeContent, (Space, StringEq!("}"))>;

    #[derive(Grammar, Clone)]
    pub enum ODBCEscapeContent {
        Function(Prefix<(Kw!("FN"), Space1), ODBCFunction>),
        DateTime(ODBCDateTimeLiteral),
    }

    #[derive(Grammar, Clone)]
    pub enum ODBCFunction {
        Abs(Kw!("ABS"), Args<ValueExpression>),
        Acos(Kw!("ACOS"), Args<ValueExpression>),
        Asin(Kw!("ASIN"), Args<ValueExpression>),
        Atan2(Kw!("ATAN2"), Args<RR>),
        Atan(Kw!("ATAN"), Args<ValueExpression>),
        Ceiling(Kw!("CEILING"), Args<ValueExpression>),
        Cos(Kw!("COS"), Args<ValueExpression>),
        Cot(Kw!("COT"), Args<ValueExpression>),
        Exp(Kw!("EXP"), Args<ValueExpression>),
        Floor(Kw!("FLOOR"), Args<ValueExpression>),
        Log(Kw!("LOG"), Args<ValueExpression>),
        Log10(Kw!("LOG10"), Args<ValueExpression>),
        Mod(Kw!("MOD"), Args<RR>),
        Pi(Kw!("PI"), Option<Args<()>>),
        Power(Kw!("POWER"), Args<RR>),
        Sin(Kw!("SIN"), Args<ValueExpression>),
        Sqrt(Kw!("SQRT"), Args<ValueExpression>),
        Tan(Kw!("TAN"), Args<ValueExpression>),
        Ascii(Kw!("ASCII"), Args<ValueExpression>),
        Char(Kw!("CHAR"), Args<ValueExpression>),
        Concat(Kw!("CONCAT"), Args<RR>),
        CurDate(Kw!("CURDATE"), Option<Args<()>>),
        CurTime(Kw!("CURTIME"), Option<Args<()>>),
        Database(Kw!("DATABASE"), Option<Args<()>>),
        DayName(Kw!("DAYNAME"), Args<ValueExpression>),
        DayOfMonth(Kw!("DAYOFMONTH"), Args<ValueExpression>),
        DayOfWeek(Kw!("DAYOFWEEK"), Args<ValueExpression>),
        DayOfYear(Kw!("DAYOFYEAR"), Args<ValueExpression>),
        Degrees(Kw!("DEGREES"), Args<ValueExpression>),
        Hour(Kw!("HOUR"), Args<ValueExpression>),
        LCase(Kw!("LCASE"), Args<ValueExpression>),
        Left(Kw!("LEFT"), Args<RR>),
        Length(Kw!("LENGTH"), Args<ValueExpression>),
        LTrim(Kw!("LTRIM"), Args<ValueExpression>),
        Minute(Kw!("MINUTE"), Args<ValueExpression>),
        Month(Kw!("MONTH"), Args<ValueExpression>),
        MonthName(Kw!("MONTHNAME"), Args<ValueExpression>),
        Now(Kw!("NOW"), Option<Args<()>>),
        Quarter(Kw!("QUARTER"), Args<ValueExpression>),
        Radians(Kw!("RADIANS"), Args<ValueExpression>),
        Repeat(Kw!("REPEAT"), Args<RR>),
        Right(Kw!("RIGHT"), Args<RR>),
        Round(Kw!("ROUND"), Args<RR>),
        RTrim(Kw!("RTRIM"), Args<ValueExpression>),
        Second(Kw!("SECOND"), Args<ValueExpression>),
        Sign(Kw!("SIGN"), Args<ValueExpression>),
        Space(Kw!("SPACE"), Args<ValueExpression>),
        Substring(Kw!("SUBSTRING"), SubstringArgs),
        TimestampAdd(
            Kw!("TIMESTAMPADD"),
            Args<RRR<DoubleQuotable<TimestampAddInterval>>>,
        ),
        TimestampDiff(
            Kw!("TIMESTAMPDIFF"),
            Args<RRR<DoubleQuotable<TimestampDiffInterval>>>,
        ),
        Truncate(Kw!("TRUNCATE"), Args<RR>),
        UCase(Kw!("UCASE"), Args<ValueExpression>),
        Week(Kw!("WEEK"), Args<ValueExpression>),
        Year(Kw!("YEAR"), Args<ValueExpression>),
    }

    #[derive(Grammar, Clone)]
    pub struct ODBCDateTimeLiteral {
        pub dts_type: ODBCDateTimeType,
        pub space: Space,
        pub val_spec: ODBCDateTimeValue,
    }

    #[derive(Grammar, Clone)]
    pub enum ODBCDateTimeType {
        Date(Kw!("D")),
        Timestamp(Kw!("TS")),
        Time(Kw!("T")),
    }

    #[derive(Grammar, Clone)]
    pub struct Fetch {
        pub keyword: Kw!("FETCH"),
        pub direction: Option<(Space1, Either<Kw!("FIRST"), Kw!("NEXT")>)>,
        pub valspec: Nested<Count, Space1>,
        pub row_keyword: Option<(Space1, RowOrRows)>,
        pub only: Option<(Space1, Kw!("ONLY"))>,
    }

    #[derive(Grammar, Clone)]
    #[tygr(inline)]
    pub enum RowOrRows {
        Rows(Kw!("ROWS")),
        Row(Kw!("ROW")),
    }
}
