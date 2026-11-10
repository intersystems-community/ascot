//! The ObjectScript grammar: class definitions (`ClassDefinition`, members, keywords), routines
//! (`Routine`, labels, `#` preprocessor lines and `$$$` macros) and the command and expression
//! language inside method bodies. Each `#[derive(Grammar)]` type is both the parser and the AST
//! node; `ascot`'s `details.rs`, `macro_expand.rs` and `check/` all pattern match on these types.
//! Embedded `&sql(...)` hands off to `sql.rs`.

use std::ops::Deref;
use std::str::FromStr;

use either::Either;
// Make key types public for use in corpus tests
pub use tygr::*;

use crate::sql;

// tygr 0.2.0 dropped these (previously public) helpers; reimplemented locally
// against tygr's still-public building blocks.
char_class!(pub AnyChar, "any", |_ch| true);
type StringEndsWith<T> = Raw<(Vec<(NotFollowedBy<T>, CharOf<AnyChar>)>, T)>;

char_class!(pub IsNumeric, "numeric", |ch| ch.is_numeric());

char_class!(
    pub IsNotLineBreakingWhitespace,
    "not line breaking whitespace",
    |ch| ch.is_whitespace() && ch != '\n'
);

#[derive(Grammar)]
pub struct Digits(StringOf1<IsNumeric>);

#[derive(Grammar)]
pub struct Exponential {
    sign: StringEqCI!("E"),
    plus_minus: Option<PlusMinus>,
    exponent: Digits,
}

#[derive(Grammar)]
pub enum Number {
    // `0.` is `0`: the point may end the number.
    FromDigit(
        Digits,
        Option<(StringEq!("."), Option<Digits>)>,
        Option<Exponential>,
    ),
    FromDot(StringEq!("."), Digits, Option<Exponential>),
}

/*

There are three kinds of "gap" between elements:

- Small Gap: simple white space (no NL), which is used
  - before infix operators
- Medium Gap: +NL, which is used
  - after infix operators,
  - within DAO, and
  - for padding parenthesises and brackets
- Large Gap: +Label and Macro directives, requiring the next line starting with some whitespaces
  - Before and between commands
 */

#[derive(Grammar, Clone)]
#[grammar(inline)]
pub enum SGapUnit {
    RangeComment(RangeComment),
    Space(StringOf1<IsNotLineBreakingWhitespace>),
    HMacro(HMacro),
}

#[derive(Grammar, Clone)]
#[grammar(inline)]
pub struct SGap(Raw<Vec<SGapUnit>>);

#[derive(Grammar, Clone)]
#[grammar(inline)]
pub struct SGap1(Raw<Vec1<SGapUnit>>);

#[derive(Grammar, Clone)]
pub struct LineComment {
    start: LineCommentStart,
    content: StringOf<IsNotNL>,
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct NL {
    gap: SGap,
    comment: Option<LineComment>,
    newline: StringEq!("\n"),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct MGap(Raw<Vec<MGapUnit>>);

#[derive(Grammar)]
#[grammar(inline)]
pub enum MGapUnit {
    SGap(SGapUnit),
    NL(NL),
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum LGapNL {
    NL(NL),
    HMacro(HMacro),
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum LGap1Enum {
    MultiLine {
        line_i: Vec1<(SGap, Periods, LGapNL)>,
        line_z: (Option<SubroutineHeader>, SGap1, Periods),
    },
    Inline(SGap1),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct LGap1(Raw<LGap1Enum>);

#[derive(Grammar)]
#[grammar(inline)]
pub struct LGap(Option<LGap1>);

#[derive(Grammar, Clone)]
#[grammar(inline)]
pub struct HMacro {
    // some old routines have a bare "#"
    pub command: Prefix<StringEq!("#"), HMacroCommand>,
    pub input: MacroInput,
}

#[derive(Grammar, Clone)]
pub enum HMacroCommand {
    // `#define F()` declares no parameter, which the call may then write as `$$$F` or `$$$F()`.
    Define {
        kw: (StringEqCI!("define"), SGap1),
        name: MacroName,
        args: Option<
            Wrap<
                (StringEq!("("), SGap),
                Option<VecSep<Prefix<StringEq!("%"), MacroName>, Wrap<SGap, StringEq!(","), SGap>>>,
                (SGap, StringEq!(")")),
            >,
        >,
    },
    Def1arg {
        kw: (StringEqCI!("def1arg"), SGap1),
        name: MacroName,
        arg: Option<
            Wrap<(StringEq!("("), SGap), Prefix<StringEq!("%"), MacroName>, (SGap, StringEq!(")"))>,
        >,
    },
    // Each bare keyword ends at a word boundary: `..#Undefined` is a parameter reference, not
    // `#undef ined`.
    IfDef(StringEqCI!("ifDef"), HMacroKeywordEnd),
    IfNDef(StringEqCI!("ifNDef"), HMacroKeywordEnd),
    If(StringEqCI!("if"), HMacroKeywordEnd),
    Elseif(StringEqCI!("ElseIf"), HMacroKeywordEnd),
    Else(StringEqCI!("Else"), HMacroKeywordEnd),
    EndIf(StringEqCI!("EndIf"), HMacroKeywordEnd),
    Execute(StringEqCI!("execute"), HMacroKeywordEnd),
    Import(StringEqCI!("import"), HMacroKeywordEnd),
    Include(Prefix<(StringEqCI!("include"), SGap1), RoutineName>),
    Noshow(StringEqCI!("noshow"), HMacroKeywordEnd),
    Show(StringEqCI!("show"), HMacroKeywordEnd),
    SqlCompile(StringEqCI!("sqlcompile"), HMacroKeywordEnd),
    Undef(StringEqCI!("undef"), HMacroKeywordEnd),
}

type HMacroKeywordEnd = NotFollowedBy<CharOf<IsLetterOrNumber>>;

pub type SGapXSGap<T> = Wrap<SGap, T, SGap>;
pub type SGapXMGap<T> = Wrap<SGap, T, MGap>;
pub type MGapXMGap<T> = Wrap<MGap, T, MGap>;
pub type LGapXLGap<T> = Wrap<LGap, T, LGap>;

// A list's comma, with the whitespace a list allows on each side kept as ranges, for the style.
pub type CommaSep<Before, After = Before> = Wrap<Range<Before>, StringEq!(","), Range<After>>;

// A comma between two present items: its offset, and the whitespace on each side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommaAt {
    pub start: usize,
    pub before: usize,
    pub after: usize,
}

impl<B, A> From<&CommaSep<B, A>> for CommaAt {
    fn from(comma: &CommaSep<B, A>) -> Self {
        CommaAt {
            start: comma.before.end,
            before: comma.before.end - comma.before.start,
            after: comma.after.end - comma.after.start,
        }
    }
}

// A list's commas, leaving out those next to an omitted item (`$lb(1,,3)`), which no spacing fits.
pub fn commas_of<T, B, A>(list: &VecSep<T, CommaSep<B, A>>) -> Vec<CommaAt> {
    let empty_item = |i: usize| {
        list.seps.get(i).map(|next| next.before.start)
            == list.seps.get(i.wrapping_sub(1)).map(|prev| prev.after.end)
    };
    (list.seps.iter().enumerate())
        .filter(|&(i, _)| !empty_item(i) && !empty_item(i + 1))
        .map(|(_, comma)| comma.into())
        .collect()
}

type LGapX<T> = Prefix<LGap, T>;

#[derive(Grammar)]
#[grammar(inline)]
pub struct SGap1X<T> {
    before: SGap1,
    it: T,
}

impl<T> Deref for SGap1X<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.it
    }
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct XMGap<T> {
    pub it: T,
    after: MGap,
}

impl<T> Deref for XMGap<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.it
    }
}

pub type Paren<T> = Wrap<StringEq!("("), T, StringEq!(")")>;

pub type Brace<T> = Wrap<StringEq!("{"), T, StringEq!("}")>;

#[derive(Grammar)]
pub enum MthArg {
    ByRef {
        dot: StringEq!("."),
        target: SetTarget,
    },
    Expr {
        expr: Range<Expr>,
        expand: Option<StringEq!("...")>,
    },
}

#[derive(Grammar)]
// The `:` may start a line of its own: `$select(cond ##continue\n : value, ...)`, and the same in
// a method body.
pub struct ExpColExp {
    pub e1: Range<Expr>,
    kw: MGapXMGap<StringEq!(":")>,
    pub e2: Range<Expr>,
}

pub type ColExp = Prefix<MGapXMGap<StringEq!(":")>, Range<Expr>>;

// The expression is parsed once and the `:expr` / `...` suffix decides the `FunArg` variant;
// trying `ExpColExp` and then `Expr` as separate alternatives re-parsed every argument, doubling
// the work at each nesting level.
#[derive(Grammar)]
pub enum FunArgSource {
    ColonDefault(ColExp),
    Star {
        kw: StringEq!("*"),
        md: Option<StarMod>,
    },
    Expr {
        expr: Range<Expr>,
        tail: Option<Either<(MGapXMGap<StringEq!(":")>, Range<Expr>), StringEq!("...")>>,
    },
    ByRef {
        dot: StringEq!("."),
        target: SetTarget,
    },
}

#[derive(GrammarFromOther)]
pub enum FunArg {
    ColonNormal(ExpColExp),
    ColonDefault(ColExp),
    Star {
        kw: StringEq!("*"),
        md: Option<StarMod>,
    },
    Normal {
        expr: Expr,
        // `args...`, meaningful only where a method argument is expected.
        expand: Option<StringEq!("...")>,
    },
    ByRef {
        dot: StringEq!("."),
        target: SetTarget,
    },
}

impl GrammarFrom for FunArg {
    type Source = FunArgSource;
    fn print_to(&self, buf: &mut String) {
        match self {
            FunArg::ColonNormal(x) => x.print_to(buf),
            FunArg::ColonDefault(x) => x.print_to(buf),
            FunArg::Star { kw, md } => {
                kw.print_to(buf);
                md.print_to(buf);
            }
            FunArg::Normal { expr, expand } => {
                expr.print_to(buf);
                expand.print_to(buf);
            }
            FunArg::ByRef { dot, target } => {
                dot.print_to(buf);
                Grammar::print_to(target, buf);
            }
        }
    }
}

impl From<FunArgSource> for FunArg {
    fn from(value: FunArgSource) -> Self {
        match value {
            FunArgSource::ColonDefault(x) => FunArg::ColonDefault(x),
            FunArgSource::Star { kw, md } => FunArg::Star { kw, md },
            FunArgSource::Expr {
                expr,
                tail: Some(Either::Left((kw, e2))),
            } => FunArg::ColonNormal(ExpColExp { e1: expr, kw, e2 }),
            FunArgSource::Expr { expr, tail } => FunArg::Normal {
                expr: expr.into_inner(),
                expand: tail.and_then(Either::right),
            },
            FunArgSource::ByRef { dot, target } => FunArg::ByRef { dot, target },
        }
    }
}

#[derive(Grammar)]
pub struct StarMod {
    pub op: MGapXMGap<StarModOp>,
    pub e: Expr,
}

#[derive(Grammar)]
pub enum StarModOp {
    Add(StringEq!("+")),
    Sub(StringEq!("-")),
}

#[derive(GrammarTryFromOther)]
pub enum GetTarget {
    Var(Variable),
    Member {
        member: Prefix<StringEq!(".."), Range<MemberRef>>,
        args: Option<Range<MthArgList>>,
    },
    Indirection(Range<Indirection>),
    Sub {
        base: Range<ExprOps<()>>,
        op: SufOp,
    },
    // A `$z...` system function standing for a reference, e.g. `$order($zobjval(...))`.
    Fun(Box<Prefix<StringEq!("$"), FunctionCall>>),
}

impl GrammarFrom for GetTarget {
    type Source = Range<ExprOps<()>>;
    fn print_to(&self, buf: &mut String) {
        match self {
            GetTarget::Var(base) => Grammar::print_to(base, buf),
            GetTarget::Member { member, args } => {
                member.print_to(buf);
                args.print_to(buf);
            }
            GetTarget::Indirection(ind) => ind.print_to(buf),
            GetTarget::Sub { base, op } => {
                Grammar::print_to(base, buf);
                op.print_to(buf);
            }
            GetTarget::Fun(call) => call.print_to(buf),
        }
    }
}

impl TryFrom<Range<ExprOps<()>>> for GetTarget {
    type Error = &'static str;

    fn try_from(value: Range<ExprOps<()>>) -> Result<Self, Self::Error> {
        let err = "this must name a variable, a property or an indirection";
        match value.into_inner() {
            ExprOps::Then { base, op } => Ok(GetTarget::Sub { base: *base, op }),
            ExprOps::Base(base) => match *base {
                ParenedExpr::Normal(prefix) => match prefix.into_inner().into_inner() {
                    AtomicExpr::Variable(variable) => Ok(GetTarget::Var(variable)),
                    AtomicExpr::Member { member, args } => Ok(GetTarget::Member { member, args }),
                    AtomicExpr::Indirection(ind) => Ok(GetTarget::Indirection(ind)),
                    AtomicExpr::FunctionCall(call)
                        if call.print().to_ascii_lowercase().starts_with("$z") =>
                    {
                        Ok(GetTarget::Fun(Box::new(call)))
                    }
                    _ => Err(err),
                },
                ParenedExpr::Paren(..) => Err(err),
            },
        }
    }
}

/// `_name` or `_form!control`: an object of a Caché Direct client, the VisM control of a Visual
/// Basic form, called back to. IRIS compiles the form and, having no client, raises
/// `<_CALLBACK SYNTAX>` where it runs.
#[derive(Grammar)]
pub struct CacheDirectCallback {
    u: StringEq!("_"),
    obj: Identifier,
    sub: Option<Prefix<StringEq!("!"), Identifier>>,
}

#[derive(GrammarTryFromOther)]
pub enum SetTarget {
    Var(Variable),
    Member {
        member: Prefix<StringEq!(".."), Range<MemberRef>>,
        args: Option<Range<MthArgList>>,
    },
    Indirection(Range<Indirection>),
    Fun(SetTargetFun),
    Special(SpecialVariable),
    Sub {
        obj: Range<ExprOps<()>>,
        op: SufOp,
    },
    CacheDirectCallback(CacheDirectCallback),
}

pub enum SetTargetFun {
    Piece(PieceCall),
    Extract(ExtractCall),
    List(ListPlainCall),
    ListGet(ListGetForSet),
    ListBuild(ListBuildForSet),
    Bit(BitCall),
    Vector(VectorCall),
    WExtract(WExtractCall),
    Property(PropertyCall),
    Sequence(SequenceCall),
    MV(MVCall),
    // `$zobjval(...)`, `$zobjmod(...)`: the undocumented object internals IRIS's own classes set.
    Undocumented(UnknownFunctionCall),
    // `$mvv(n)`, a MultiValue system value: IRIS's own classes assign it, undocumented as a target.
    Simple(SimpleFunctionCall),
}

// A SET target is an expression of a settable shape -- except `$LISTGET(var1,var2,...)`, which
// is a variable list of any length (like `SET $LISTBUILD(...)=`), nothing like the function's
// own arguments, so it's read as that first.
#[derive(Grammar)]
pub enum SetTargetSource {
    ListGet(Prefix<StringEq!("$"), ListGetForSet>),
    Expr(Range<ExprOps<()>>),
}

impl GrammarFrom for SetTarget {
    type Source = SetTargetSource;
    fn print_to(&self, buf: &mut String) {
        match self {
            SetTarget::Var(base) => Grammar::print_to(base, buf),
            SetTarget::Member { member, args } => {
                member.print_to(buf);
                args.print_to(buf);
            }
            SetTarget::Indirection(ind) => ind.print_to(buf),
            SetTarget::Fun(fun) => fun.print_to(buf),
            SetTarget::Special(var) => {
                buf.push('$');
                Grammar::print_to(var, buf);
            }
            SetTarget::CacheDirectCallback(callback) => Grammar::print_to(callback, buf),
            SetTarget::Sub { obj: base, op } => {
                base.print_to(buf);
                op.print_to(buf);
            }
        }
    }
}

impl TryFrom<SetTargetSource> for SetTarget {
    type Error = &'static str;

    fn try_from(value: SetTargetSource) -> Result<Self, Self::Error> {
        let err = "the left of `=` must be a variable, a property, an indirection or a settable function such as $PIECE";
        let value = match value {
            SetTargetSource::ListGet(call) => {
                return Ok(SetTarget::Fun(SetTargetFun::ListGet(call.prefixed)));
            }
            SetTargetSource::Expr(value) => value,
        };
        match value.into_inner() {
            ExprOps::Then { base, op } => Ok(SetTarget::Sub { obj: *base, op }),
            ExprOps::Base(base) => match *base {
                ParenedExpr::Normal(prefix) => match prefix.into_inner().into_inner() {
                    AtomicExpr::Variable(variable) => Ok(SetTarget::Var(variable)),
                    AtomicExpr::Member { member, args } => Ok(SetTarget::Member { member, args }),
                    AtomicExpr::Indirection(ind) => Ok(SetTarget::Indirection(ind)),
                    AtomicExpr::SpecialVariable { var, .. } => Ok(SetTarget::Special(var)),
                    AtomicExpr::CacheDirectCallback(callback) => {
                        Ok(SetTarget::CacheDirectCallback(callback))
                    }
                    AtomicExpr::FunctionCall(call) => {
                        match SetTargetFun::try_from(call.into_inner()) {
                            Ok(fun) => Ok(SetTarget::Fun(fun)),
                            Err(_) => Err(err),
                        }
                    }
                    _ => Err(err),
                },
                ParenedExpr::Paren(..) => Err(err),
            },
        }
    }
}

impl TryFrom<FunctionCall> for SetTargetFun {
    type Error = ();

    fn try_from(call: FunctionCall) -> Result<Self, ()> {
        match call {
            FunctionCall::Piece(c) => Ok(SetTargetFun::Piece(c)),
            FunctionCall::Extract(c) => Ok(SetTargetFun::Extract(c)),
            // Only the plain form is settable; ListRange is read-only.
            FunctionCall::ListPlain(c) => Ok(SetTargetFun::List(c)),
            FunctionCall::Bit(c) => Ok(SetTargetFun::Bit(c)),
            FunctionCall::Vector(c) => Ok(SetTargetFun::Vector(c)),
            FunctionCall::WExtract(c) => Ok(SetTargetFun::WExtract(c)),
            FunctionCall::Property(c) => Ok(SetTargetFun::Property(c)),
            FunctionCall::Sequence(c) => Ok(SetTargetFun::Sequence(c)),
            FunctionCall::ListBuild(c) => Ok(SetTargetFun::ListBuild(c.try_into()?)),
            FunctionCall::MV(c) => Ok(SetTargetFun::MV(c)),
            FunctionCall::Unknown(c) if c.is_settable() => Ok(SetTargetFun::Undocumented(c)),
            FunctionCall::Simple(c) if c.is_settable() => Ok(SetTargetFun::Simple(c)),
            _ => Err(()),
        }
    }
}

impl TryFrom<ListBuild> for ListBuildForSet {
    type Error = ();

    fn try_from(
        ListBuild {
            kw,
            lp,
            args: ListBuildArgs { first, rest, .. },
            rp,
        }: ListBuild,
    ) -> Result<Self, Self::Error> {
        let first = exp_arg_to_var_arg(first)?;
        let rest = rest
            .into_iter()
            .map(|rest| rest.map(exp_arg_to_var_arg).transpose())
            .collect::<Result<Vec<_>, ()>>()?;
        Ok(ListBuildForSet {
            kw,
            lp,
            first,
            rest,
            rp,
        })
    }
}

fn exp_arg_to_var_arg(
    arg: Wrap<MGap, Range<Option<Expr>>, MGap>,
) -> Result<Wrap<MGap, Range<Option<Variable>>, MGap>, ()> {
    let arg = arg
        .map(|arg| {
            arg.map(|arg| {
                arg.map(|arg| {
                    arg.as_variable()
                        .map(|arg| Variable::parse(&arg.print()).unwrap())
                        .ok_or(())
                })
                .transpose()
            })
            .transpose()
        })
        .transpose()?;
    Ok(arg)
}

impl SetTargetFun {
    fn print_to(&self, buf: &mut String) {
        buf.push('$');
        match self {
            SetTargetFun::Piece(c) => c.print_to(buf),
            SetTargetFun::Extract(c) => c.print_to(buf),
            SetTargetFun::List(c) => c.print_to(buf),
            SetTargetFun::ListGet(c) => c.print_to(buf),
            SetTargetFun::Bit(c) => c.print_to(buf),
            SetTargetFun::Vector(c) => c.print_to(buf),
            SetTargetFun::WExtract(c) => c.print_to(buf),
            SetTargetFun::Property(c) => c.print_to(buf),
            SetTargetFun::Sequence(c) => c.print_to(buf),
            SetTargetFun::MV(c) => c.print_to(buf),
            SetTargetFun::ListBuild(c) => c.print_to(buf),
            SetTargetFun::Undocumented(c) => c.print_to(buf),
            SetTargetFun::Simple(c) => Grammar::print_to(c, buf),
        }
    }
}

impl SetTarget {
    pub fn as_variable(&self) -> Option<&Variable> {
        match self {
            SetTarget::Var(variable) => Some(variable),
            _ => None,
        }
    }
}

pub type PostCondition = Range<Prefix<StringEq!(":"), ExpressionAfterColon>>;

#[derive(Grammar)]
pub struct GlobalRef {
    caret: StringEq!("^"),
    extension: Option<GlobalRefExtension>,
    name: Option<AllowMacro<GlobalVariableName>>,
}

#[derive(Grammar)]
pub struct ExtendedRoutineReference {
    caret: StringEq!("^"),
    pub extension: Option<GlobalRefExtension>,
    pub name: AllowIndirection<RoutineName>,
}

type BracketSafeExpr = Box<VecSep<OpsExpr, StringEq!("_")>>;

#[derive(Grammar)]
pub enum GlobalRefExtension {
    Normal(Wrap<StringEq!("|"), Option<VecSep<Expr, StringEq!(",")>>, StringEq!("|")>),
    Bracket(Wrap<StringEq!("["), Option<VecSep<BracketSafeExpr, StringEq!(",")>>, StringEq!("]")>),
}

type SubscriptList = ParenList<Range<Expr>>;

#[derive(Grammar)]
pub struct GlobalReference {
    pub global: GlobalRef,
    pub subs: Option<SubscriptList>,
}

#[derive(Grammar)]
#[grammar(inline)]
// The name may be a property or a method's value: `@obj.Name@(sub)`.
pub struct Indirection {
    pub at: StringEq!("@"),
    pub expr: Box<Range<ExprOps<()>>>,
    pub subscript_indirection: Option<Prefix<StringEq!("@"), SubscriptList>>,
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum AllowIndirection<T> {
    Plain(T),
    Indirection(Range<Box<Indirection>>),
}

// Dynamic Abstract Object
#[derive(Grammar)]
pub enum DAO {
    Obj {
        open: StringEq!("{"),
        content: MGapXMGap<Option<VecSep<DAOBind, CommaSep<MGap>>>>,
        close: StringEq!("}"),
    },
    Arr {
        open: StringEq!("["),
        content: MGapXMGap<Option<VecSep<Range<DAOElem>, CommaSep<MGap>>>>,
        close: StringEq!("]"),
    },
}

#[derive(Grammar)]
pub enum DAOElem {
    DAO(Box<Range<DAO>>),
    True(StringEq!("true")),
    False(StringEq!("false")),
    Null(StringEq!("null")),
    Str(JSONString),
    Num(Option<StringEq!("-")>, Number),
    OS {
        open: StringEq!("("),
        content: MGapXMGap<Range<Expr>>,
        close: StringEq!(")"),
    },
}

#[derive(Grammar)]
pub struct DAOBind {
    pub key: JSONString,
    pub colon: MGapXMGap<StringEq!(":")>,
    pub value: Range<DAOElem>,
}

// A JSON string: backslash escapes and no doubled quotes, as JSON spells it ("any valid JSON
// literal string is also a valid ObjectScript expression").
#[derive(Grammar)]
pub struct JSONString(Wrap<DQ, Raw<Vec<JSONStringUnit>>, DQ>);

#[derive(Grammar)]
#[grammar(inline)]
pub enum JSONStringUnit {
    Escape(BS, CharOf<IsNotNL>),
    Plain(CharOf<IsNotDQOrBSOrNL>),
}

char_class!(pub IsNotDQOrBSOrNL, "not_doublequote_backslash_or_newline", |ch| !matches!(
    ch,
    '"' | '\\' | '\n' | '\r'
));

#[derive(Grammar)]
#[grammar(inline)]
pub enum AllowMacro<T> {
    Plain(T),
    Macro(Box<MacroCall>),
}

#[derive(Grammar)]
pub enum MemberRef {
    Parameter {
        hash: StringEqCI!("#"),
        id: ClassMemberName,
    },
    Normal {
        id: QuotableClassMemberName,
    },
}

impl MemberRef {
    pub fn id(&self) -> String {
        match self {
            MemberRef::Parameter { id, .. } => id.print(),
            MemberRef::Normal { id } => id.name(),
        }
    }

    // A `#`-prefixed reference (`..#Foo`, `##class(X).#Foo`) targets a class `Parameter`,
    // which lives in a separate namespace from properties/methods/etc.
    pub fn is_parameter(&self) -> bool {
        matches!(self, MemberRef::Parameter { .. })
    }
}

pub type Field = Prefix<StringEq!("."), Range<MemberRef>>;

// A type's class: a class name (`This` among them, for the receiver's class) or a class
// parameter's value `..#NAME`. IRIS takes only the first; ascot's declarations use the others.
#[derive(Grammar)]
pub enum SignatureType {
    Class(ClassName),
    Parameter(StringEq!("..#"), ClassMemberName),
}

#[derive(Grammar)]
pub enum CastReceiver {
    This(StringEq!("$"), StringEqCI!("this")),
    Local(LocalVariableName),
    // `##class(C)($this).M()`: IRIS compiles the parenthesized receiver too.
    Parenthesized(Range<Paren<Box<CastReceiver>>>),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct SufOp {
    pub mem: Field,
    pub args: Option<Range<MthArgList>>,
}

type Pattern = Vec<AllowMacro<AllowIndirection<PatternElement>>>;

#[derive(Grammar)]
pub struct PatternElement {
    pub count: PatternCount,
    pub item: PatternItem,
}

#[derive(Grammar)]
pub enum PatternCount {
    Range {
        min: Option<Digits>,
        dot: StringEq!("."),
        max: Option<Digits>,
    },
    Exact(Digits),
}

#[derive(Grammar)]
pub enum PatternItem {
    Literal(OSString),
    Codes(Vec1<PatternCode>),
    Alter(Box<Paren<VecSep<Pattern, StringEq!(",")>>>),
}

#[derive(Grammar)]
pub enum PatternCode {
    Alpha(StringEqCI!("A")),                    // Alphabetic (upper + lower)
    Uppercase(StringEqCI!("U")),                // Uppercase
    Lowercase(StringEqCI!("L")),                // Lowercase
    Numeric(StringEqCI!("N")),                  // Numeric (0-9)
    Punctuation(StringEqCI!("P")),              // Punctuation
    Control(StringEqCI!("C")),                  // Control characters
    Everything(StringEqCI!("E")),               // Any character
    Cyrillic(StringEqCI!("R")),                 // Cyrillic alphabetic (Russian locale)
    CyrillicUpper(StringEqCI!("B")),            // Cyrillic uppercase (Russian locale)
    CyrillicLower(StringEqCI!("M")),            // Cyrillic lowercase (Russian locale)
    FullWidth(StringEqCI!("ZFWCHARZ")),         // Japanese ZENKAKU full-width characters
    HalfWidthKatakana(StringEqCI!("ZHWKATAZ")), // Japanese half-width Katakana characters
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum PrefixOperator {
    Not(StringEq!("'")),
    Minus(StringEq!("-")),
    Plus(StringEq!("+")),
}

pub struct NotFirstNonWhitespace;

impl Grammar for NotFirstNonWhitespace {
    type First = <() as Grammar>::First;

    fn parse_at(input: &str, pos: usize, state: State) -> Option<(Self, usize)> {
        let pos = Self::scan_at(input, pos, state)?;
        Some((Self, pos))
    }

    fn scan_at(input: &str, pos: usize, _state: State) -> Option<usize> {
        let before = input.get(..pos)?;
        let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
        if input.get(line_start..pos)?.chars().all(char::is_whitespace) {
            None
        } else {
            Some(pos)
        }
    }

    fn print_to(&self, _buf: &mut String) {}

    fn to_bnf() -> bnf::Expr {
        bnf::Expr::empty()
    }

    fn fail_at(_pos: usize, _state: State) -> bool {
        false
    }
}

#[derive(Grammar)]
pub enum Operator {
    Eq(StringEq!("=")),
    Ne(StringEq!("'=")),
    Le(StringEq!("<=")),
    Lt(StringEq!("<")),
    Ge(StringEq!(">=")),
    Gt(StringEq!(">")),
    Ng(StringEq!("'>")),
    Nl(StringEq!("'<")),
    LO(StringEq!("||")),
    LA(StringEq!("&&")),
    // `&` not starting an embedded SQL/HTML/JS block.
    And(
        StringEq!("&"),
        NotFollowedBy<
            Either<
                StringEqCI!("html<"),
                Either<StringEqCI!("js<"), Either<StringEqCI!("javascript<"), StringEqCI!("sql(")>>,
            >,
        >,
    ),
    NAnd(StringEq!("'&")),
    Or(StringEq!("!")),
    NOr(StringEq!("'!")),
    Add(StringEq!("+")),
    Sub(StringEq!("-")),
    Pow(StringEq!("**")),
    Mul(StringEq!("*")),
    // `/` not starting a `//` line comment.
    Div(StringEq!("/"), NotFollowedBy<StringEq!("/")>),
    IntDiv(StringEq!("\\")),
    Mod(NotFirstNonWhitespace, StringEq!("#")),
    Concat(StringEq!("_")),
    Contains(StringEq!("[")),
    NotContains(StringEq!("'[")),
    SortsAfter(StringEq!("]]")),
    Follows(StringEq!("]")),
    NotSortsAfter(StringEq!("']]")),
    NotFollows(StringEq!("']")),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct MatchOp {
    not: Option<StringEq!("'")>,
    sym: StringEq!("?"),
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum InfixOperation<BetweenInfix> {
    Other {
        op: Wrap<BetweenInfix, Operator, MGap>,
        rhs: Range<OpsExpr<()>>,
    },
    Match {
        op: Wrap<BetweenInfix, MatchOp, MGap>,
        rhs: Pattern,
    },
}

#[derive(Grammar)]
pub struct MultiSetTarget {
    pub open: StringEq!("("),
    pub targets: MGapXMGap<VecSep<SetTarget, CommaSep<SGap, MGap>>>,
    pub close: StringEq!(")"),
}

#[derive(Grammar)]
pub enum AssignTarget {
    Single(SetTarget),
    Multi(MultiSetTarget),
}

#[derive(Grammar)]
pub struct SetArgument {
    pub target: Range<AssignTarget>,
    pub eq: LGapXLGap<StringEq!("=")>,
    pub value: Range<Expr>,
}

// A command or function name: its documented spelling and abbreviation.
pub trait Keyword: Grammar {
    const KIND: KeywordKind;
    const FULL: &'static str;
    const ABBR: Option<&'static str>;
}

macro_rules! keywords {
    ($kind:ident, $names:ident [$($extra:literal),*]; $($name:ident: $full:literal $(/ $abbr:literal)?;)*) => {
        $(
            #[derive(Grammar)]
            #[grammar(inline)]
            pub enum $name {
                Full(StringEqCI!($full)),
                $(Abbr(StringEqCI!($abbr)),)?
            }

            impl Keyword for $name {
                const KIND: KeywordKind = KeywordKind::$kind;
                const FULL: &'static str = $full;
                const ABBR: Option<&'static str> = keywords!(@abbr $($abbr)?);
            }
        )*

        // Each name with its abbreviation.
        pub const $names: &[(&str, Option<&str>)] =
            &[$(($full, keywords!(@abbr $($abbr)?)),)* $(($extra, None)),*];
    };
    (@abbr) => { None };
    (@abbr $abbr:literal) => { Some($abbr) };
}

// `COMMAND_NAMES` is every command's full name, for `UnknownCommand`'s error message. `#Dim` is
// included even though it itself never reaches `UnknownCommand` (it starts with `#`, not a
// letter): a user who drops the `#` and just writes `Dim` produces a plain alphabetic word that
// *does* land here, and `#Dim` is the useful suggestion for that typo. The
// `&sql(`/`##sql`/`&html<`/`&js<` markers are left out for now.
keywords! {
    Command, COMMAND_NAMES ["H", "#Dim"];
    KwDo: "Do" / "D";
    KwIf: "If" / "I";
    KwElseIf: "ElseIf";
    KwElse: "Else" / "E";
    KwWhile: "While";
    KwFor: "For" / "F";
    KwGoto: "GoTo" / "G";
    KwBreak: "Break" / "B";
    KwQuit: "Quit" / "Q";
    KwZQuit: "ZQuit" / "Zq";
    KwReturn: "Return" / "Ret";
    KwContinue: "Continue";
    KwSet: "Set" / "S";
    KwKill: "Kill" / "K";
    KwNew: "New" / "N";
    KwMerge: "Merge" / "M";
    KwTry: "Try";
    KwCatch: "Catch";
    KwThrow: "Throw";
    KwZTrap: "ZTrap" / "Zt";
    KwZZDump: "ZZDump";
    KwWrite: "Write" / "W";
    KwRead: "Read" / "R";
    KwOpen: "Open" / "O";
    KwClose: "Close" / "C";
    KwUse: "Use" / "U";
    KwLock: "Lock" / "L";
    KwHalt: "Halt";
    KwHang: "Hang";
    KwJob: "Job" / "J";
    KwTStart: "TStart" / "Ts";
    KwTCommit: "TCommit" / "Tc";
    KwTRollback: "TRollback" / "Tro";
    KwZWrite: "ZWrite" / "Zw";
    KwZKill: "ZKill" / "Zk";
    KwZNspace: "ZNspace" / "Zn";
    KwXecute: "Xecute" / "X";
    KwView: "View" / "V";
    KwZPrint: "ZPrint" / "Zp";
    KwPrint: "Print" / "P";
    KwZBreak: "ZBreak" / "Zb";
    KwMVPrint: "MVPrint" / "MVP";
    KwMVCrt: "MVCrt" / "MVC";
}

// Every hand-written function's name (those in `SIMPLE_FUNCTIONS` are spelled there).
keywords! {
    Function, FUNCTION_KEYWORDS [];
    FnBit: "Bit";
    FnBitCount: "BitCount";
    FnBitFind: "BitFind";
    FnBitLogic: "BitLogic";
    FnCase: "Case";
    FnChange: "Change";
    FnChar: "Char" / "C";
    FnClassMethod: "ClassMethod";
    FnClassName: "ClassName";
    FnCompile: "Compile";
    FnData: "Data" / "D";
    FnDecimal: "Decimal";
    FnDouble: "Double";
    FnExtract: "Extract" / "E";
    FnFNumber: "FNumber" / "FN";
    FnGet: "Get" / "G";
    FnIncrement: "Increment" / "I";
    FnIsObject: "IsObject" / "ISO";
    FnLength: "Length" / "L";
    FnList: "List" / "LI";
    FnListBuild: "ListBuild" / "LB";
    FnListData: "ListData" / "LD";
    FnListGet: "ListGet" / "LG";
    FnListNext: "ListNext";
    FnListUpdate: "ListUpdate" / "LU";
    FnLocate: "Locate";
    FnMethod: "Method";
    FnMV: "MV";
    FnName: "Name" / "NA";
    FnNext: "Next" / "N";
    FnOrder: "Order" / "O";
    FnParameter: "Parameter";
    FnPiece: "Piece" / "P";
    FnPrefetchOff: "PrefetchOff";
    FnPrefetchOn: "PrefetchOn";
    FnProperty: "Property";
    FnQuery: "Query" / "Q";
    FnSelect: "Select" / "S";
    FnSequence: "Sequence" / "SEQ";
    FnSortBegin: "SortBegin";
    FnSortEnd: "SortEnd";
    FnText: "Text" / "T";
    FnVector: "Vector" / "VE";
    FnVectorDefined: "VectorDefined" / "VD";
    FnVectorOp: "VectorOp" / "VOP";
    FnWChar: "WChar" / "WC";
    FnWExtract: "WExtract" / "WE";
    FnXecute: "Xecute";
    FnZDate: "ZDate" / "ZD";
    FnZDateH: "ZDateH" / "ZDH";
    FnZDateTime: "ZDateTime" / "ZDT";
    FnZDateTimeH: "ZDateTimeH" / "ZDTH";
    FnZF: "ZF";
    FnZIncrement: "ZIncrement" / "ZINCR";
    FnZObjClass: "ZObjClass";
    FnZObjClassMethod: "ZObjClassMethod";
    FnZObjMethod: "ZObjMethod";
    FnZObjProperty: "ZObjProperty";
    FnZObjRef: "ZObjRef";
    FnZOrder: "ZOrder" / "ZO";
    FnZPrevious: "ZPrevious" / "ZP";
    FnZStrip: "ZStrip";
    FnZTime: "ZTime" / "ZT";
    FnZTimeH: "ZTimeH" / "ZTH";
    FnZUtil: "ZUtil" / "ZU";
}

// `H` alone halts; with an argument it hangs -- so only `HCommand` knows its full name.
#[derive(Grammar)]
#[grammar(inline)]
pub enum KwH {
    Abbr(StringEqCI!("H")),
}

// One command keyword as written in the source.
pub struct KeywordUse {
    pub kind: KeywordKind,
    pub start: usize,
    pub text: String,
    pub full: &'static str,
    pub abbr: Option<&'static str>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeywordKind {
    Command,
    // An intrinsic function's name, without its `$`.
    Function,
}

impl std::fmt::Display for KeywordKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            KeywordKind::Command => "Command keyword",
            KeywordKind::Function => "Function",
        })
    }
}

impl KeywordUse {
    fn of<K: Keyword>(kw: &K, start: usize) -> Self {
        KeywordUse {
            kind: K::KIND,
            start,
            text: kw.print(),
            full: K::FULL,
            abbr: K::ABBR,
        }
    }
}

// A keyword as written, with where it starts.
pub trait Spelled {
    fn spelled(&self) -> KeywordUse;
}

impl<K: Keyword> Spelled for Range<K> {
    fn spelled(&self) -> KeywordUse {
        KeywordUse::of(&**self, self.start)
    }
}

impl<A: Spelled, B: Spelled> Spelled for Either<A, B> {
    fn spelled(&self) -> KeywordUse {
        match self {
            Either::Left(a) => a.spelled(),
            Either::Right(b) => b.spelled(),
        }
    }
}

// What every kind of command exposes, so a check over commands as such (keyword style,
// postconditionals, ...) needs no knowledge of the individual kinds.
pub trait CommandLike {
    // The keywords this command spells out, given where the command itself starts.
    fn keywords(&self, start: usize) -> Vec<KeywordUse>;

    fn postcondition(&self) -> Option<&PostCondition> {
        None
    }
}

#[derive(Grammar)]
pub enum CatchTarget {
    NoWrap(Variable),
    Wrap(Paren<Variable>),
}

impl Deref for CatchTarget {
    type Target = Variable;

    fn deref(&self) -> &Self::Target {
        match self {
            CatchTarget::NoWrap(macro_or_variable) => macro_or_variable,
            CatchTarget::Wrap(wrap) => wrap.deref(),
        }
    }
}

#[derive(Grammar)]
pub enum MergeArgument {
    Pair {
        target: SetTarget,
        eq: LGapXLGap<StringEq!("=")>,
        source: Range<Expr>,
    },
    // `Merge @x`: argument indirection, `x` holding the whole `target=source` (documented).
    Indirect(Range<Box<Indirection>>),
}

#[derive(Grammar)]
pub struct DoArgument {
    pub entry_ref: DoTarget,
    pub pc: Option<PostCondition>,
}

// What `Do` runs: an entry reference, or `$Case(x, value:entry, ..., :entry)` choosing one, each
// value a `Do` argument (documented for `$CASE`).
#[derive(Grammar)]
pub enum DoTarget {
    Plain(Range<EntryRefAndArgs>),
    Case(DoCase),
}

#[derive(Grammar)]
pub struct DoCase {
    pub dollar: StringEq!("$"),
    pub kw: Range<FnCase>,
    pub open: StringEq!("("),
    pub target: ArgOf<Expr>,
    pub arms: Vec<More<ArgOf<DoCaseArm>>>,
    pub close: StringEq!(")"),
}

#[derive(Grammar)]
pub struct DoCaseArm {
    pub case: Option<Range<Expr>>,
    pub colon: StringEq!(":"),
    pub entry_ref: Range<EntryRefAndArgs>,
}

#[derive(Grammar)]
pub struct GoToArg {
    pub location: AllowCase<Location>,
    pub pc: Option<PostCondition>,
}

#[derive(Grammar)]
pub enum AllowCase<T> {
    Plain(T),
    Case(StringEq!("$"), Range<FnCase>, FunArgList),
}

#[derive(Grammar)]
pub struct Location(pub AllowIndirection<Range<LegacyEntryRef>>);

#[derive(GrammarTryFromOther)]
pub enum MethodOrFuncCallExpr {
    /// A receiver expression whose last suffix op is a call.
    Method {
        obj: Range<ExprOps<()>>,
        mem: Field,
        args: Range<MthArgList>,
    },
    /// A `..member(args)` self-call — unlike `Method`, there's no separate receiver: `..member`
    /// is itself a complete atom, with the call args attaching directly to it.
    SelfMethod {
        member: Prefix<StringEq!(".."), Range<MemberRef>>,
        args: Range<MthArgList>,
    },
    /// A `##super(args)` call — same shape as `SelfMethod`, but calling the superclass's
    /// version of the enclosing method instead of a named member.
    Super(Prefix<StringEqCI!("##super"), Range<MthArgList>>),
    /// A bare `$`-prefixed function call atom.
    Function(FunctionCall),
}

impl GrammarFrom for MethodOrFuncCallExpr {
    type Source = ExprOps<()>;
    fn print_to(&self, buf: &mut String) {
        match self {
            Self::Method { obj, mem, args } => {
                obj.print_to(buf);
                mem.print_to(buf);
                args.print_to(buf);
            }
            Self::SelfMethod { member, args } => {
                member.print_to(buf);
                args.print_to(buf);
            }
            Self::Super(sup) => sup.print_to(buf),
            Self::Function(call) => call.print_to(buf),
        }
    }
}

impl TryFrom<ExprOps<()>> for MethodOrFuncCallExpr {
    type Error = &'static str;

    fn try_from(value: ExprOps<()>) -> Result<Self, Self::Error> {
        let err = "Do takes a method or function call, not a value";
        match value {
            ExprOps::Then {
                base,
                op: SufOp { mem, args },
            } => {
                let args = args.ok_or(err)?;
                Ok(Self::Method {
                    obj: *base,
                    mem,
                    args,
                })
            }
            ExprOps::Base(base) => match *base {
                ParenedExpr::Normal(prefix) => match prefix.into_inner().into_inner() {
                    AtomicExpr::FunctionCall(call) => Ok(Self::Function(call.into_inner())),
                    AtomicExpr::Member { member, args } => Ok(Self::SelfMethod {
                        member,
                        args: args.ok_or(err)?,
                    }),
                    AtomicExpr::Super(sup) => Ok(Self::Super(sup)),
                    _ => Err(err),
                },
                ParenedExpr::Paren(..) => Err(err),
            },
        }
    }
}

#[derive(Grammar)]
pub enum EntryRefAndArgs {
    Expression(MethodOrFuncCallExpr),
    Legacy {
        proc: Range<LegacyEntryRef>,
        args: Option<MthArgList>,
    },
}

#[derive(Grammar)]
pub enum LegacyEntryRef {
    Label(LabelOffsetRoutine),
    Routine(ExtendedRoutineReference),
}

#[derive(Grammar)]
pub struct LabelOffsetRoutine {
    pub label: Range<AllowIndirection<Label>>,
    pub offset: Option<Prefix<StringEq!("+"), Range<Expr>>>,
    pub routine: Option<ExtendedRoutineReference>,
}

pub type MacroCall = Prefix<StringEq!("$$$"), MacroCallStruct>;

// `$$$Name (args)` is tolerated: a parameterless macro that keeps its argument list hands them
// on to its body, and a body defined with a trailing space puts one there.
#[derive(Grammar, Clone)]
pub struct MacroCallStruct {
    pub name: MacroName,
    pub args: Option<Prefix<StringOf<IsSpaceOrTab>, MacroArgList>>,
}

char_class!(pub IsMacroArgSafe, "any but comma or paren", |ch| !matches!(
    ch,
    ',' | '(' | ')' | '"'
));

pub type MacroArgList = Paren<VecSep<Range<MacroArg>, StringEq!(",")>>;
pub type MacroArg = Vec<MacroArgUnit>;

#[derive(Grammar, Clone)]
pub enum MacroArgUnit {
    Any(StringOf1<IsMacroArgSafe>),
    Str(OSString),
    List(MacroArgList),
}

#[derive(Grammar)]
pub struct ZKillTarget {
    pub var: AllowIndirection<Variable>,
}

#[derive(Grammar)]
pub enum ViewArgument {
    Full {
        offset: Expr,
        mode: Prefix<SGapXMGap<StringEq!(":")>, Expr>,
        length: Prefix<SGapXMGap<StringEq!(":")>, Expr>,
        new_value: Option<Prefix<SGapXMGap<StringEq!(":")>, Expr>>,
    },
    Block(Expr),
}

#[derive(Grammar)]
pub enum NewArgument {
    Single(AllowMacro<AllowIndirection<NewTarget>>),
    Except(Paren<VecSep<AllowMacro<AllowIndirection<NewTarget>>, CommaSep<MGap>>>),
}

#[derive(Grammar)]
pub enum NewTarget {
    Normal(LocalVariableName),
    Special(Prefix<StringEq!("$"), SpecialVariable>),
}

#[derive(Grammar)]
pub enum KillArgument {
    Single(Range<SetTarget>),
    Except(Paren<VecSep<Range<SetTarget>, CommaSep<MGap>>>),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct IfBranches {
    pub thn: Range<Block>,
    pub eifs: Vec<LGapX<ElseIfBranch>>,
    pub els: Option<LGapX<ElseBranch>>,
}

#[derive(Grammar)]
pub enum JobArgument {
    ClassMethod(
        StringEqCI!("##class"),
        Paren<QuotableClassName>,
        Field,
        MthArgList,
        Option<LocalJobAnnotation>,
    ),
    InstanceMethod(
        StringEq!(".."),
        ClassMemberName,
        MthArgList,
        Option<LocalJobAnnotation>,
    ),
    DollarClassMethod(
        StringEqCI!("$CLASSMETHOD"),
        MthArgList,
        Option<LocalJobAnnotation>,
    ),
    RoutineWithLocation(
        LegacyEntryRef,
        Option<MthArgList>,
        JobLocation,
        Option<LocalJobAnnotation>,
    ),
    RoutineLocal(
        LegacyEntryRef,
        Option<MthArgList>,
        Option<LocalJobAnnotation>,
    ),
}

#[derive(Grammar)]
pub enum JobLocation {
    Brackets(
        StringEq!("["),
        VecSep<Option<BracketSafeExpr>, StringEq!(",")>,
        StringEq!("]"),
    ),
    Bars(StringEq!("|"), Expr, StringEq!("|")),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct LocalJobAnnotation {
    // The document (RCOS_cjob) requires the parentheses, but %SYS.Audit passes a bare namespace
    // and IRIS takes it, so that parses too and is reported as undocumented syntax.
    pub process_params:
        Prefix<StringEq!(":"), Option<AllowMacro<Either<Paren<JobProcessParams>, Range<Expr>>>>>,
    pub timeout: Option<Prefix<StringEq!(":"), Expr>>,
}

type JobProcessParams = VecSep<Option<Expr>, StringEq!(":")>;

#[derive(Grammar)]
pub enum DimType {
    ListOf(StringEqCI!("List"), Space1, StringEqCI!("Of"), Space1, Type),
    ArrayOf(
        StringEqCI!("Array"),
        Space1,
        StringEqCI!("Of"),
        Space1,
        Type,
    ),
    Simple(Type),
}

#[derive(Grammar)]
pub struct ForIterator {
    pub var: Variable,
    pub eq: SGapXMGap<StringEq!("=")>,
    pub specs: VecSep<ForSpec, CommaSep<SGap, MGap>>,
}

#[derive(Grammar)]
pub enum ForSpec {
    Range {
        start: Range<Expr>,
        increment: Prefix<SGapXMGap<StringEq!(":")>, Range<Expr>>,
        end: Option<Prefix<SGapXMGap<StringEq!(":")>, Range<Expr>>>,
    },
    Value(Range<Expr>),
}

#[derive(Grammar)]
pub enum WriteArg {
    Expr(Range<Expr>),
    Fmts(Vec1<WriteFChar>),
    Dev {
        sym: StringEq!("*-"),
        val: Range<Expr>,
    },
    Char {
        sym: StringEq!("*"),
        val: Range<Expr>,
    },
}

#[derive(Grammar)]
pub enum WriteFChar {
    NewLine(StringEq!("!")),
    FormFeed(StringEq!("#")),
    // `write ? 5`: IRIS takes a space before the column.
    Tab(StringEq!("?"), SGap, Range<Expr>),
    Mnemonic {
        slash: StringEq!("/"),
        name: LocalVariableName,
        params: Option<ParenList<Range<Expr>>>,
    },
}

#[derive(Grammar)]
pub enum ReadFChar {
    NewLine(StringEq!("!")),
    FormFeed(StringEq!("#")),
    Tab(StringEq!("?"), Range<Expr>),
}

#[derive(Grammar)]
pub enum ReadArg {
    Prompt(OSString),
    Var(
        Option<StringEq!("*")>,
        Range<SetTarget>,
        Option<ReadMaxLen>,
        Option<ReadTimeout>,
    ),
    FChars(Vec1<ReadFChar>),
}

#[derive(Grammar)]
pub struct OpenArg {
    pub device: Expr,
    pub parameters: Option<Prefix<StringEq!(":"), Either<Paren<IOParameters>, IOParameter>>>,
    pub timeout: Option<Prefix<StringEq!(":"), Option<Expr>>>,
    pub mnespace: Option<Prefix<StringEq!(":"), Expr>>,
}

pub type IOParameters = VecSep<IOParameter, StringEq!(":")>;

#[derive(Grammar)]
pub struct IOParameter {
    pub key: Option<(StringEq!("/"), Identifier)>,
    pub eq: Option<StringEq!("=")>,
    // `/args="-c","%U:%G",File`: a list for a parameter that takes one.
    pub val: Option<VecSep<Expr, StringEq!(",")>>,
    pub ddd: Option<StringEq!("...")>,
}

#[derive(Grammar)]
pub struct UseArg {
    pub device: Expr,
    pub parameters:
        Option<Prefix<StringEq!(":"), Option<Either<Paren<IOParameters>, IOParameter>>>>,
    pub mnespace: Option<Prefix<StringEq!(":"), Expr>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn use_arg() {
        let _ = UseArg::parse("IO:/MAR=IOMS").expect("Ok");
    }
}

#[derive(Grammar)]
pub struct CloseArg {
    pub device: Expr,
    pub parameters: Option<Prefix<StringEq!(":"), CloseParameters>>,
}

#[derive(Grammar)]
pub enum CloseParameters {
    One(OSString),
    Many(Paren<VecSep<IOParameter, StringEq!(":")>>),
}

#[derive(Grammar)]
pub struct ReadMaxLen {
    hash: StringEq!("#"),
    pub len: Expr,
}

#[derive(Grammar)]
pub struct ReadTimeout {
    pub colon: StringEq!(":"),
    pub timeout: Expr,
}

#[derive(Grammar)]
pub enum PlusMinus {
    Plus(StringEq!("+")),
    Minus(StringEq!("-")),
}

#[derive(Grammar)]
pub struct LockArg {
    plus_minus: Option<PlusMinus>,
    pub lock_entry: AllowMacro<LockEntry>,
}

#[derive(Grammar)]
pub enum LockEntry {
    One(AllowIndirection<LockNameType>),
    Many {
        open: StringEq!("("),
        entries: VecSep<AllowIndirection<LockNameType>, CommaSep<SGap, MGap>>,
        close: StringEq!(")"),
    },
}

type LockName = AllowMacro<Variable>;

#[derive(Grammar)]
pub struct LockNameType {
    pub n: AllowIndirection<LockName>,
    pub t: Option<Prefix<StringEq!("#"), Expr>>,
}

// A braced block after a command, `Gap` the whitespace the command allows before the `{`.
#[derive(Grammar)]
pub struct CurryBlock<Gap = LGap> {
    // It starts where the command's own text ended.
    pub gap: Range<Gap>,
    pub block: Box<Range<Brace<Box<ProcedureBody>>>>,
}

impl<Gap> Deref for CurryBlock<Gap> {
    type Target = Range<Brace<Box<ProcedureBody>>>;

    fn deref(&self) -> &Self::Target {
        &self.block
    }
}

#[derive(Grammar)]
pub enum Block {
    Curry(CurryBlock),
    Command(SGap1X<VecSep<Range<Command>, SGap1>>),
}

char_class!(pub IsNotSpaceChar, "not_space", |ch| ch != ' ');

#[derive(Grammar)]
pub struct ElseIfBranch {
    pub kw: Range<KwElseIf>,
    // `ElseIf  (x)` with extra spaces: every space but the last is the keyword's, the last
    // belongs to the condition (as after `If`). Unlike `If`, `ElseIf` is never argumentless.
    kw_spaces: Raw<Vec<(StringEq!(" "), NotFollowedBy<CharOf<IsNotSpaceChar>>)>>,
    pub cnds: IfConditions,
    pub thn: Range<Block>,
}

#[derive(Grammar)]
pub struct ElseBranch {
    pub kw: Range<KwElse>,
    pub body: Option<Range<Block>>,
}

#[derive(Grammar)]
pub enum SpecialVariable {
    DEVICE(Either<StringEqCI!("DEVICE"), StringEqCI!("D")>),

    ESTACK(Either<StringEqCI!("ESTACK"), StringEqCI!("ES")>),
    ETRAP(Either<StringEqCI!("ETRAP"), StringEqCI!("ET")>),
    ECODE(Either<StringEqCI!("ECODE"), StringEqCI!("EC")>),

    HALT(StringEqCI!("HALT")),
    HOROLOG(Either<StringEqCI!("HOROLOG"), StringEqCI!("H")>),

    IO(Either<StringEqCI!("IO"), StringEqCI!("I")>),

    JOB(Either<StringEqCI!("JOB"), StringEqCI!("J")>),

    KEY(Either<StringEqCI!("KEY"), StringEqCI!("K")>),

    MvSysRetCode(StringEqCI!("mvsysretcode")),
    MVAM(StringEqCI!("MVAM")),
    MVVM(StringEqCI!("MVVM")),
    MVSM(StringEqCI!("MVSM")),

    NAMESPACE(StringEqCI!("NAMESPACE")),

    PRINCIPAL(Either<StringEqCI!("PRINCIPAL"), StringEqCI!("P")>),

    QUIT(Either<StringEqCI!("QUIT"), StringEqCI!("Q")>),

    ROLES(StringEqCI!("ROLES")),

    // `ST` followed by a letter is `$STORAGE`, below.
    STACK(Either<StringEqCI!("STACK"), (StringEqCI!("ST"), NotFollowedBy<CharOf<IsAlphabetic>>)>),
    SYSTEM(Either<StringEqCI!("SYSTEM"), StringEqCI!("SY")>),
    STORAGE(Either<StringEqCI!("STORAGE"), StringEqCI!("S")>),

    THROWOBJ(StringEqCI!("THROWOBJ")),
    TLEVEL(Either<StringEqCI!("TLEVEL"), StringEqCI!("TL")>),
    THIS(StringEqCI!("THIS")),
    TRS(StringEqCI!("TRS")),
    TID(StringEqCI!("TID")),
    TEST(Either<StringEqCI!("TEST"), StringEqCI!("T")>),

    USERNAME(StringEqCI!("USERNAME")),

    X(StringEqCI!("X")),
    Y(StringEqCI!("Y")),

    ZCHILD(Either<StringEqCI!("ZCHILD"), StringEqCI!("ZC")>),
    ZEOF(StringEqCI!("ZEOF")),
    ZEOS(StringEqCI!("ZEOS")),
    ZERROR(Either<StringEqCI!("ZERROR"), StringEqCI!("ZE")>),
    ZHOROLOG(Either<StringEqCI!("ZHOROLOG"), StringEqCI!("ZH")>),
    ZIO(StringEqCI!("ZIO")),
    ZJOB(Either<StringEqCI!("ZJOB"), StringEqCI!("ZJ")>),
    ZMODE(Either<StringEqCI!("ZMODE"), StringEqCI!("ZM")>),
    ZNAME(StringEqCI!("ZNAME")),
    ZNSPACE(Either<StringEqCI!("ZNSPACE"), StringEqCI!("ZN")>),
    ZORDER(Either<StringEqCI!("ZORDER"), StringEqCI!("ZO")>),
    // `ZP` followed by a letter is `$ZPI` or `$ZPOS`, below.
    ZPARENT(
        Either<StringEqCI!("ZPARENT"), (StringEqCI!("ZP"), NotFollowedBy<CharOf<IsAlphabetic>>)>,
    ),
    ZPI(StringEqCI!("ZPI")),
    ZPOS(Either<StringEqCI!("ZPOSITION"), StringEqCI!("ZPOS")>),
    ZREFERENCE(Either<StringEqCI!("ZREFERENCE"), StringEqCI!("ZR")>),
    ZSTORAGE(Either<StringEqCI!("ZSTORAGE"), StringEqCI!("ZS")>),
    ZTIMESTAMP(Either<StringEqCI!("ZTIMESTAMP"), StringEqCI!("ZTS")>),
    ZTIMEZONE(Either<StringEqCI!("ZTIMEZONE"), StringEqCI!("ZTZ")>),
    ZTRAP(Either<StringEqCI!("ZTRAP"), StringEqCI!("ZT")>),
    ZVERSION(Either<StringEqCI!("ZVERSION"), StringEqCI!("ZV")>),
    ZA(StringEqCI!("ZA")),
    ZB(StringEqCI!("ZB")),
    // Anything else spelled like one: accepted here, reported by the checker.
    Unknown(StringOf1<IsAlphabetic>),
}

// Every full name above (i.e. not its short abbreviation) -- kept in sync by hand alongside the
// enum, for `UnknownSpecialVariable::from_str`'s error message.
pub const SPECIAL_VARIABLE_NAMES: &[&str] = &[
    "DEVICE",
    "ESTACK",
    "ETRAP",
    "ECODE",
    "HALT",
    "HOROLOG",
    "IO",
    "JOB",
    "KEY",
    "MVSYSRETCODE",
    "MVAM",
    "MVVM",
    "MVSM",
    "NAMESPACE",
    "PRINCIPAL",
    "QUIT",
    "ROLES",
    "STACK",
    "SYSTEM",
    "STORAGE",
    "THROWOBJ",
    "TLEVEL",
    "THIS",
    "TRS",
    "TID",
    "TEST",
    "USERNAME",
    "X",
    "Y",
    "ZCHILD",
    "ZEOF",
    "ZEOS",
    "ZERROR",
    "ZHOROLOG",
    "ZIO",
    "ZJOB",
    "ZMODE",
    "ZNAME",
    "ZNSPACE",
    "ZORDER",
    "ZPARENT",
    "ZPI",
    "ZPOSITION",
    "ZREFERENCE",
    "ZSTORAGE",
    "ZTIMESTAMP",
    "ZTIMEZONE",
    "ZTRAP",
    "ZVERSION",
    "ZA",
    "ZB",
];

char_class!(pub IsAlphabetic, "alphabetic", |ch| ch.is_alphabetic());

#[derive(Grammar)]
pub struct Variable {
    pub base: AllowMacro<VariableBase>,
    pub subs: Option<SubscriptList>,
}

// https://docs.intersystems.com/iris20262/csp/docbook/DocBook.UI.Page.cls?KEY=RCOS_appx_identifiers#RCOS_appx_identifiers_cos_localvars
#[derive(Grammar, Clone)]
#[grammar(inline)]
pub struct LocalVariableName(Raw<LocalVariableNameStruct>);

#[derive(Grammar)]
#[grammar(inline)]
pub struct LocalVariableNameStruct {
    first: CharOf<IsLocalVariableNameFirst>,
    rest: StringOf<IsLocalVariableNameRemaining>,
}
char_class!(pub IsLocalVariableNameFirst, "letter or percent sign", |c| {
    c.is_alphabetic() || c == '%'
});
type IsLocalVariableNameRemaining = IsLetterOrNumberIncludingUnicode;

// https://docs.intersystems.com/iris20262/csp/docbook/DocBook.UI.Page.cls?KEY=RCOS_appx_identifiers#RCOS_appx_identifiers_cos_globals
// -- the definition here excludes '^' because there can be other thins between ^ and this part.
#[derive(Grammar)]
pub struct GlobalVariableName(Raw<GlobalVariableNameStruct>);
#[derive(Grammar)]
pub struct GlobalVariableNameStruct {
    first: CharOf<IsGlobalVariableNameFirst>,
    remaining: Option<(
        Vec<(StringOf<IsLetterOrNumberIncludingUnicode>, StringEq!("."))>,
        StringOf1<IsLetterOrNumberIncludingUnicode>,
    )>,
}
char_class!(pub IsGlobalVariableNameFirst, "letter or percent sign", |c| c
    .is_alphabetic()
    || c == '%');
char_class!(pub IsLetterOrNumber, "letter or number", |c| c
    .is_ascii_alphanumeric());

// https://docs.intersystems.com/iris20262/csp/docbook/DocBook.UI.Page.cls?KEY=RCOS_appx_identifiers#RCOS_appx_identifiers_class_members
#[derive(Grammar)]
pub struct ClassMemberName(Raw<ClassMemberNameStruct>);
#[derive(Grammar)]
pub struct ClassMemberNameStruct {
    first: CharOf<IsClassMemberNameFirst>,
    remaining: StringOf<IsClassMemberNameRemaining>,
}
char_class!(pub IsClassMemberNameFirst, "letter or percent sign", |c| c
    .is_ascii_alphabetic()
    || c == '%');
char_class!(pub IsClassMemberNameRemaining, "letter or number", |c| c
    .is_alphanumeric());

#[derive(Grammar)]
#[grammar(inline)]
pub enum VariableBase {
    Instance(Prefix<StringEq!("i%"), Range<QuotableClassMemberName>>),
    RowInstance(Prefix<StringEq!("r%"), Range<ClassMemberName>>),
    // `m%Prop`, a property's modified flag, and `s%Prop`: two more prefixes of IRIS's own classes,
    // undocumented; the quotes of `i%"%%OID"` go here too.
    ModifiedInstance(Range<Prefix<StringEq!("m%"), Range<QuotableClassMemberName>>>),
    SInstance(Range<Prefix<StringEq!("s%"), Range<QuotableClassMemberName>>>),
    Public(Prefix<StringEq!("%"), LocalVariableName>),
    Local(LocalVariableName),
    StructureSystemVariable {
        prefix: StringEq!("^$"),
        extension: Option<GlobalRefExtension>,
        var: StructureSystemVariable,
    },
    Global(GlobalRef),
}

#[derive(Grammar)]
pub enum StructureSystemVariable {
    GLOBAL(Either<StringEqCI!("GLOBAL"), StringEqCI!("G")>),
    JOB(Either<StringEqCI!("JOB"), StringEqCI!("J")>),
    LOCK(Either<StringEqCI!("LOCK"), StringEqCI!("L")>),
    ROUTINE(Either<StringEqCI!("ROUTINE"), StringEqCI!("R")>),
}

pub type Periods = Vec<(Vec1<StringEq!(".")>, SGap1)>;

pub type ExprEntry = MGapXMGap<Range<Expr>>;
// A `CodeMode = expression` body: IRIS compiles an empty one, comments alone included.
pub type ExprBody = MGapXMGap<Option<Range<Expr>>>;

#[derive(Grammar)]
pub enum BitLogicOp {
    And(StringEq!("&")),
    Or(StringEq!("|")),
    Xor(StringEq!("^")),
}

#[derive(Grammar)]
pub enum BitLogicOperand {
    Paren(Paren<MGapXMGap<Box<BitLogicExpr>>>),
    Variable(AllowIndirection<Variable>),
    Empty(StringEq!("\"\"")),
}

#[derive(Grammar)]
pub struct BitLogicAtom {
    pub nots: Vec<StringEq!("~")>,
    pub operand: Range<BitLogicOperand>,
}

pub type BitLogicExpr = VecSep<Range<BitLogicAtom>, MGapXMGap<BitLogicOp>>;

char_class!(pub IsAny, "any", |_c| true);

char_class!(pub
    IsSymbolFirst,
    "ObjectScript symbols (i.e., letters or %)",
    |ch| ch.is_alphabetic() || ch == '%'
);
char_class!(pub IsSymbolRest, "letter/digit/%", |ch| {
    ch.is_alphabetic() || ch.is_ascii_digit() || ch == '%'
});
char_class!(pub
    IsKeywordValue,
    "keyword name (i.e., letters, digits, or one of \"_%./-\")",
    |ch| ch.is_alphabetic() || ch.is_ascii_digit() || matches!(ch, '_' | '/' | '-' | '%' | '.')
);
char_class!(pub IsWhitespace, "whitespace", |ch| ch.is_whitespace());
char_class!(pub IsNotNL, "not_newline", |ch| ch != '\n');
char_class!(pub IsNotStar, "not_star", |ch| ch != '*');

pub type Space = Hidden<StringOf<IsWhitespace>>;
pub type Space1 = Hidden<StringOf1<IsWhitespace>>;

#[derive(Grammar, Clone)]
pub struct RangeComment(Raw<(StringEq!("/*"), StringEndsWith<StringEq!("*/")>)>);

#[derive(Grammar, Clone)]
pub struct BareNL(StringEq!("\n"));

#[derive(Grammar)]
#[grammar(inline)]
pub struct GenericLineComment<Prefix: Grammar> {
    head: Prefix,
    rest: StringEndsWith<BareNL>,
}

char_class!(pub IsNotDQOrNL, "not_doublequote_or_newline", |ch| {
    ch != '"' && ch != '\n'
});

#[derive(Grammar)]
pub struct OSStringContent(Vec<Either<StringEq!("\"\""), CharOf<IsNotDQOrNL>>>);

pub type OSString = Wrap<DQ, Raw<OSStringContent>, DQ>;

type BS = StringEq!("\\");
type DQ = StringEq!("\"");
type SQ = StringEq!("'");
type BT = StringEq!("`");

/// Double-quoted string with "" escape for internal quotes
#[derive(Grammar)]
pub struct JSDQStringStruct<Embeded: Grammar> {
    open: DQ,
    content: Vec<Either<(BS, CharOf<IsAny>), Either<Embeded, CharOf<IsJSDQStringSafe>>>>,
    close: DQ,
}

#[derive(Grammar)]
pub struct JSSQStringStruct<Embeded: Grammar> {
    open: SQ,
    content: Vec<Either<(BS, CharOf<IsAny>), Either<Embeded, CharOf<IsJSSQStringSafe>>>>,
    close: SQ,
}

#[derive(Grammar)]
pub struct JSBTStringStruct<Embeded: Grammar> {
    open: BT,
    content: Vec<Either<(BS, CharOf<IsAny>), Either<Embeded, CharOf<IsJSBTStringSafe>>>>,
    close: BT,
}

char_class!(pub IsJSDQStringSafe, "js_dq_string", |ch| {
    ch != '"' && ch != '\\'
});

char_class!(pub IsJSSQStringSafe, "js_sq_string", |ch| {
    ch != '\'' && ch != '\\'
});

char_class!(pub IsJSBTStringSafe, "js_bt_string", |ch| {
    ch != '`' && ch != '\\'
});

type JSSQString<Embeded> = Raw<JSSQStringStruct<Embeded>>;

type JSDQString<Embeded> = Raw<JSDQStringStruct<Embeded>>;

type JSBTString<Embeded> = Raw<JSBTStringStruct<Embeded>>;

#[derive(Grammar)]
pub enum JSString<Embeded: Grammar> {
    S(JSSQString<Embeded>),
    D(JSDQString<Embeded>),
    B(JSBTString<Embeded>),
}

// Impossible, which is useful for essentially ignoring embeded language

#[derive(Grammar)]
#[grammar(validated)]
pub struct Impossible;

impl Validate for Impossible {
    const REQUIREMENT: &'static str = "never match";
    fn validate(&self) -> bool {
        false
    }
}

#[derive(Grammar)]
pub struct Identifier(pub Range<OSSymbol>);

#[derive(Grammar)]
pub struct QuotableClassMemberName(pub Range<QuotableClassMemberNameEnum>);

impl Deref for QuotableClassMemberName {
    type Target = Range<QuotableClassMemberNameEnum>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl QuotableClassMemberName {
    // `"%%OID"` names the member `%%OID`; a doubled quote inside is one quote.
    pub fn name(&self) -> String {
        match self.0.deref() {
            QuotableClassMemberNameEnum::Symbol(symbol) => symbol.print(),
            QuotableClassMemberNameEnum::String(string) => {
                string.print().trim_matches('"').replace("\"\"", "\"")
            }
        }
    }
}

#[derive(Grammar)]
pub enum QuotableClassMemberNameEnum {
    Symbol(ClassMemberName),
    String(OSString),
}

#[derive(Grammar)]
pub struct OSSymbolStruct {
    first: CharOf<IsSymbolFirst>,
    rest: StringOf<IsSymbolRest>,
}

#[derive(Grammar)]
pub struct OSSymbol(Raw<OSSymbolStruct>);

// https://docs.intersystems.com/iris20262/csp/docbook/DocBook.UI.Page.cls?KEY=RCOS_appx_identifiers#RCOS_appx_identifiers_classes
#[derive(Grammar)]
#[grammar(validated)]
pub struct PackageAndClassName(VecSep<ShortPackageAndClassName, StringEq!(".")>);
// All non-first short package names must start with a letter
impl Validate for PackageAndClassName {
    const REQUIREMENT: &'static str = "have all non-first short package names start with a letter";
    fn validate(&self) -> bool {
        self.items
            .iter()
            .skip(1)
            .all(|name| name.print().chars().next().unwrap().is_ascii_alphabetic())
    }
}

impl Deref for PackageAndClassName {
    type Target = VecSep<ShortPackageAndClassName, StringEq!(".")>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub type PackageName = PackageAndClassName;
pub type ClassName = PackageAndClassName;

#[derive(Grammar)]
pub enum QuotableClassName {
    Plain(ClassName),
    Quoted(OSString),
}

#[derive(Grammar)]
pub struct ShortPackageAndClassName {
    first: CharOf<IsShortPackageAndClassNameFirst>,
    remaining: StringOf<IsPackageAndClassNameRemaining>,
}

type IsShortPackageAndClassNameFirst = IsLetterOrPercent;
char_class!(pub IsLetterOrPercent, "letter or percent sign", |c| c
    .is_ascii_alphabetic()
    || c == '%');

type IsPackageAndClassNameRemaining = IsLetterOrNumberIncludingUnicode;
char_class!(
    pub IsLetterOrNumberIncludingUnicode,
    "letter or number",
    |c| c.is_alphanumeric()
);

/*
There are two entry points for ObjectScript:

- OSExpr: ObjectScript as class parameter, keyword value, method parameter default values, etc.
- OSText: ObjectScript in {...}

The key differences are:

- OSExpr should not contain any comments.
- OSText may contain comments, which requires more detailed handling.
*/

#[derive(Grammar)]
#[grammar(name = "ObjectScript")]
pub struct OSText(Raw<Vec<OSTextUnit>>);

#[derive(Grammar)]
pub enum OSExpr {
    String(OSString),
    Text(StringOf1<IsOSExprSafe>),
    Paren {
        open: StringEq!("("),
        content: OSText,
        close: StringEq!(")"),
    },
    Brace {
        open: StringEq!("{"),
        content: Range<OSText>,
        close: StringEq!("}"),
    },
}

type Line = (StringOf<IsNotNL>, BareNL);

#[derive(Grammar, Clone)]
#[grammar(validated)]
pub struct ContinueLine(Line);

impl Validate for ContinueLine {
    const REQUIREMENT: &'static str = "contain ##continue";
    fn validate(&self) -> bool {
        self.0.0.print().to_lowercase().contains("##continue")
    }
}

#[derive(Grammar, Clone)]
pub struct MacroInput {
    pub line_i: Vec<ContinueLine>,
    pub line_z: StringOf<IsNotNL>,
}

#[derive(Grammar)]
pub enum OSTextUnitNoHash {
    T(StringOf1<IsOSTextSafe>),
    NL(StringEq!("\n"), Option<SubroutineHeader>),
    S(OSString),
    // A `"` that doesn't open a string closed on the same line (e.g. inside embedded SQL), so it
    // must not derail brace matching.
    DQ(StringEq!("\"")),
    Brace(Brace<OSText>),
    // A `;` comment: a brace inside it doesn't count.
    EComment(StringEq!(";"), StringOf<IsNotNL>),
    LC(GenericLineComment<LineCommentStart>),
    RC(RangeComment),
    SL(StringEq!("/")),
    HTML(StringEqCI!("&html<"), HTML, StringEq!(">")),
    JS(JsMarker, EmbeddedJS, StringEq!(">")),
    And(StringEqCI!("&"), AndWhatever),
}

#[derive(Grammar)]
pub enum OSTextUnitHash {
    // A `#;` comment: stripped by the preprocessor, so a brace inside it doesn't count.
    HComment(StringEq!("#;"), StringOf<IsNotNL>),
    MLit(StringEq!("##lit("), StringEndsWith<StringEq!(")")>),
    MDefine(StringEq!("#define"), MacroInput),
    MDef1arg(StringEq!("#def1arg"), MacroInput),
    HS(StringEq!("#")),
}

#[derive(Grammar)]
pub enum OSTextUnit {
    OSTextUnitNoHash(OSTextUnitNoHash),
    OSTextUnitHash(OSTextUnitHash),
}

#[derive(Grammar)]
pub struct EmbeddedJS(Vec<EmbeddedJSUnit>);

#[derive(Grammar)]
pub enum EmbeddedJSUnit {
    S(JSString<HashEmbededOS>),
    NGT(StringOf1<IsEJGSafe>),
}

char_class!(pub IsEJGSafe, "embeded_js_safe", |ch| !matches!(
    ch,
    '>' | '"' | '\''
));

#[derive(Grammar)]
#[grammar(validated)]
pub struct AndWhatever(StringOf<IsAlphabetic>);
// I could use IsOSExprSafe instead of IsAlphabetic, but filter is expensive so better to filter a shorter string.

impl Validate for AndWhatever {
    const REQUIREMENT: &'static str = "not be \"html\", \"js\" or \"javascript\"";
    fn validate(&self) -> bool {
        let raw = self.0.to_lowercase();
        !matches!(raw.as_str(), "html" | "js" | "javascript")
    }
}

char_class!(pub IsOSExprSafe, "expr_char", |ch| ch.is_alphanumeric()
|| matches!(
    ch,
    '$' | '.'
        | '='
        | '%'
        | '\''
        | ':'
        | '['
        | ']'
        | '_'
        | '+'
        | '-'
        | '*'
        | '|'
        | '>'
        | '<'
        | '!'
        | '^'
        | '?'
        | '@'
        | '\\'
        | '~'
));

// Like expr safe but add parentheses and whitespace
char_class!(pub IsOSTextSafe, "objectscript_text", |ch| ch.is_alphanumeric()
|| (ch.is_whitespace() && ch != '\n')
|| matches!(
    ch,
    '$' | '.'
        | '='
        | '%'
        | '\''
        | ':'
        | '['
        | ']'
        | '_'
        | '+'
        | '-'
        | '*'
        | '|'
        | '>'
        | '<'
        | '!'
        | '^'
        | '?'
        | '@'
        | '\\'
        | '~'
        | '('
        | ')'
        | ','
));

#[derive(Grammar)]
pub struct HashEmbededOS(StringEq!("#("), StringEndsWith<StringEq!(")#")>);

#[derive(Grammar)]
pub struct AngularEmbededOS(StringEq!("<%"), StringEndsWith<StringEq!("%>")>);

// `&html<...>` ends at the `>` that balances its `<`: IRIS counts every `<` and `>` of the text,
// in a quoted attribute, a comment or a `#()#` expression as much as in a tag, and nothing else,
// except that from any `<script` on the text is raw up to the next `</script>`, and the block
// fails without one.
#[derive(Grammar)]
pub struct HTML(Raw<Vec<HTMLUnit>>);

#[derive(Grammar)]
pub enum HTMLUnit {
    Script(
        Raw<(StringEqCI!("<script"), Vec<HTMLUnit>, StringEq!(">"))>,
        StringEndsWith<StringEqCI!("</script>")>,
    ),
    Nested(
        StringEq!("<"),
        NotFollowedBy<StringEqCI!("script")>,
        Vec<HTMLUnit>,
        StringEq!(">"),
    ),
    Other(CharOf<IsNotAngleBracket>),
}

char_class!(pub IsNotAngleBracket, "not an angle bracket", |ch| !matches!(ch, '<' | '>'));

#[derive(Grammar)]
pub enum HTMLTagContentUnit {
    Embeded1(HashEmbededOS),
    Embeded2(AngularEmbededOS),
    // A quote is plain text: IRIS ends the block at the first balancing `>`, quoted or not.
    Quote(Either<DQ, SQ>),
    Sym(CharOf<IsHTMLContentSafe>),
}

type HTMLTagContent = Vec<HTMLTagContentUnit>;

// A `\` or `~` is as plain as the rest: a Windows path in an XData's attribute.
char_class!(pub IsHTMLContentSafe, "html safe characters", |ch| ch
.is_alphanumeric()
|| ch.is_whitespace()
|| matches!(
    ch,
    '=' | ':'
        | '\\'
        | '~'
        | '&'
        | ';'
        | '['
        | ']'
        | '.'
        | '/'
        | '('
        | ')'
        | '-'
        | '#'
        | '|'
        | '{'
        | '}'
        | ','
        | '^'
        | '%'
        | '_'
        | '?'
        | '*'
        | '+'
        | '!'
        | '$'
));

/// Any data (for XData with non-HTML/XML MIME types)
#[derive(Grammar)]
pub struct Data(Vec<DataUnit>);

#[derive(Grammar)]
#[grammar(hidden)]
pub enum DataUnit {
    Txt(StringOf1<IsDataSafe>),
    Str(JSDQString<Impossible>),
    Brs(StringEq!("{"), Vec<DataUnit>, StringEq!("}")),
}

char_class!(pub IsDataSafe, "xdata_safe", |ch| !matches!(
    ch,
    '"' | '{' | '}'
));

#[derive(Grammar)]
pub struct Type {
    pub class_name: SignatureType,
    pub params: Option<TypeParamList>,
}

#[derive(Grammar)]
pub struct TypeParamList {
    open: Raw<(Space, StringEq!("("), Space)>,
    params: VecSep<TypeParam, Raw<(Space, StringEq!(","), Space)>>,
    close: Raw<(Space, StringEq!(")"))>,
}

impl Deref for TypeParamList {
    type Target = VecSep<TypeParam, Raw<(Space, StringEq!(","), Space)>>;
    fn deref(&self) -> &Self::Target {
        &self.params
    }
}

#[derive(Grammar)]
pub struct TypeParam {
    pub name: ClassName,
    eq: Raw<(Space, StringEq!("="), Space)>,
    pub value: OSExpr,
}

type WithSpace<X> = (X, Space);

#[derive(Grammar)]
pub enum KeywordValueEnum {
    // `Data = (Tags(ELEMENTS))` names a collection's subvalues as an index key does, but IRIS
    // reads this form case-sensitively, unlike the `On` clause's.
    Atom(
        StringOf1<IsKeywordValue>,
        Option<Either<StringEq!("(KEYS)"), StringEq!("(ELEMENTS)")>>,
    ),
    String(OSString),
    OS(StringEq!("{"), OSText, StringEq!("}")),
    Paren(
        WithSpace<StringEq!("(")>,
        VecSep<WithSpace<Box<KeywordValue>>, WithSpace<StringEq!(",")>>,
        StringEq!(")"),
    ),
}

#[derive(Grammar)]
pub struct KeywordValue(Raw<KeywordValueEnum>);

// The class may be `..#NAME`, a class parameter's value, in ascot's declarations (see
// `SignatureType`).
#[derive(Grammar)]
pub struct PropertyType {
    pub collection: Option<CollectionKind>,
    pub class_name: SignatureType,
}

#[derive(Grammar)]
pub enum CollectionKind {
    ListOf(Raw<(StringEqCI!("List"), Space1, StringEqCI!("Of"), Space1)>),
    ArrayOf(Raw<(StringEqCI!("Array"), Space1, StringEqCI!("Of"), Space1)>),
}

#[derive(Grammar)]
pub struct KeywordList<Entry = KeywordEntry> {
    open: Raw<(Space, StringEq!("["), Space)>,
    pub entries: Option<VecSep<KeywordEntryWithSpace<Entry>, CommaWithSpace>>,
    close: Raw<(Space, StringEq!("]"))>,
}

impl<Entry: Grammar> KeywordList<Entry> {
    pub fn items(&self) -> impl Iterator<Item = &KeywordEntryWithSpace<Entry>> {
        self.entries.iter().flat_map(|entries| entries.items.iter())
    }
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct CommaWithSpace(Raw<(StringEq!(","), Space)>);

// Ranged so a diagnostic about one keyword (`[ CompileAfter = , ... ]`) can point at it.
pub type KeywordEntryWithSpace<Entry = KeywordEntry> = Range<Suffix<Entry, Space>>;

/// An entry read as one of the declaration's keywords `K`, or kept as written when it is none.
/// IRIS parses any entry here; what it does with one it doesn't know is the checker's to report.
pub type FailableKeyword<K> = Either<K, KeywordEntry>;

#[derive(Grammar)]
pub enum KeywordEntry {
    Assign {
        keyword: Identifier,
        eq: Raw<(Space, StringEq!("="), Space)>,
        // Empty in IRIS's own exports (`CompileAfter = ,`).
        value: Option<KeywordValue>,
    },
    Not {
        not: Raw<(StringEqCI!("Not"), Space1)>,
        keyword: Identifier,
    },
    Flag(Identifier),
}

impl KeywordEntry {
    pub fn keyword(&self) -> &Identifier {
        match self {
            KeywordEntry::Assign { keyword, .. } => keyword,
            KeywordEntry::Not { keyword, .. } => keyword,
            KeywordEntry::Flag(keyword) => keyword,
        }
    }
}

// Where a keyword's name or value ends: before anything but another name character, so that
// `Final` doesn't match the start of `FinalFoo`, nor `persistent` of `persistentfoo`.
#[derive(Grammar)]
#[grammar(hidden)]
pub struct KeywordEnd(NotFollowedBy<CharOf<IsKeywordValue>>);

/// A flag keyword: `Name` sets it, `Not Name` clears it. `Name = 1` is no flag: IRIS refuses it on a
/// member, and takes it on a class as a bare `Name` whatever the value.
#[derive(Grammar)]
#[grammar(inline)]
pub enum Flag<Name> {
    Not {
        not: Raw<(StringEqCI!("Not"), Space1)>,
        name: Name,
        end: KeywordEnd,
    },
    Set {
        name: Name,
        end: KeywordEnd,
        // Fail at `Abstract = 1` rather than leave `= 1` behind.
        no_assign: NoAssign,
    },
}

impl<Name: Grammar> Flag<Name> {
    pub fn is_set(&self) -> bool {
        matches!(self, Flag::Set { .. })
    }
}

#[derive(Grammar)]
#[grammar(hidden)]
pub struct NoAssign(NotFollowedBy<Raw<(Space, StringEq!("="))>>);

/// The `0` or `1` of `ProcedureBlock = 1` and `ServerOnly = 0`.
#[derive(Grammar)]
#[grammar(inline)]
pub enum Bit {
    Off(StringEq!("0")),
    On(StringEq!("1")),
}

impl Bit {
    pub fn is_set(&self) -> bool {
        matches!(self, Bit::On(_))
    }
}

/// A keyword taking a value of the shape `Value`: `Name = value`.
#[derive(Grammar)]
#[grammar(inline)]
pub struct Valued<Name, Value> {
    name: Name,
    end: KeywordEnd,
    eq: Raw<(Space, StringEq!("="), Space)>,
    pub value: Value,
    after: KeywordEnd,
}

// Spelled as IRIS accepts them: `ClassType` in lowercase only, the rest in any case.

#[derive(Grammar)]
#[grammar(inline)]
pub enum ClassType {
    Datatype(StringEq!("datatype")),
    Dynamic(StringEq!("dynamic")),
    Index(StringEq!("index")),
    Literal(StringEq!("literal")),
    Persistent(StringEq!("persistent")),
    Serial(StringEq!("serial")),
    Stream(StringEq!("stream")),
    View(StringEq!("view")),
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum Inheritance {
    Left(StringEqCI!("left")),
    Right(StringEqCI!("right")),
}

/// A class's keywords (`https://docs.intersystems.com/iris20262/csp/docbook/DocBook.UI.Page.cls?KEY=ROBJ_class`):
/// a value the checker reads has its own shape, any other is taken as written.
#[derive(Grammar)]
pub enum ClassKeyword {
    Abstract(Flag<StringEqCI!("Abstract")>),
    // `""` is how IRIS writes "no class type", on every non-persistent class it exports.
    ClassType(Valued<StringEqCI!("ClassType"), Either<ClassType, StringEq!("\"\"")>>),
    ClientDataType(Valued<StringEqCI!("ClientDataType"), KeywordValue>),
    ClientName(Valued<StringEqCI!("ClientName"), KeywordValue>),
    CompileAfter(Valued<StringEqCI!("CompileAfter"), KeywordValue>),
    DdlAllowed(Flag<StringEqCI!("DdlAllowed")>),
    DependsOn(Valued<StringEqCI!("DependsOn"), KeywordValue>),
    Deprecated(Flag<StringEqCI!("Deprecated")>),
    Final(Flag<StringEqCI!("Final")>),
    GeneratedBy(Valued<StringEqCI!("GeneratedBy"), KeywordValue>),
    Hidden(Flag<StringEqCI!("Hidden")>),
    Inheritance(Valued<StringEqCI!("Inheritance"), Inheritance>),
    Language(Valued<StringEqCI!("Language"), MethodLanguage>),
    LegacyInstanceContext(Flag<StringEqCI!("LegacyInstanceContext")>),
    NoExtent(Flag<StringEqCI!("NoExtent")>),
    OdbcType(Valued<StringEqCI!("OdbcType"), KeywordValue>),
    Owner(Valued<StringEqCI!("Owner"), KeywordValue>),
    ProcedureBlock(Flag<StringEqCI!("ProcedureBlock")>),
    PropertyClass(Valued<StringEqCI!("PropertyClass"), ClassList>),
    ServerOnly(Valued<StringEqCI!("ServerOnly"), Bit>),
    Sharded(Valued<StringEqCI!("Sharded"), Bit>),
    SoapBindingStyle(Valued<StringEqCI!("SoapBindingStyle"), KeywordValue>),
    SoapBodyUse(Valued<StringEqCI!("SoapBodyUse"), KeywordValue>),
    SqlCategory(Valued<StringEqCI!("SqlCategory"), KeywordValue>),
    SqlRowIdName(Valued<StringEqCI!("SqlRowIdName"), KeywordValue>),
    SqlRowIdPrivate(Flag<StringEqCI!("SqlRowIdPrivate")>),
    SqlTableName(Valued<StringEqCI!("SqlTableName"), KeywordValue>),
    StorageStrategy(Valued<StringEqCI!("StorageStrategy"), KeywordValue>),
    System(Valued<StringEqCI!("System"), KeywordValue>),
    ViewQuery(Valued<StringEqCI!("ViewQuery"), KeywordValue>),
}

/// The keywords every member takes, besides its kind's own.
#[derive(Grammar)]
#[grammar(inline)]
pub enum MemberKeyword {
    Deprecated(Flag<StringEqCI!("Deprecated")>),
    Internal(Flag<StringEqCI!("Internal")>),
}

/// A member kind's keywords, which include every `MemberKeyword`.
pub trait HasMemberKeyword {
    fn member(&self) -> Option<&MemberKeyword>;
}

macro_rules! has_member_keyword {
    ($($kind:ident),* $(,)?) => {$(
        impl HasMemberKeyword for $kind {
            fn member(&self) -> Option<&MemberKeyword> {
                match self {
                    Self::Member(keyword) => Some(keyword),
                    _ => None,
                }
            }
        }
    )*};
}

has_member_keyword!(
    MethodKeyword,
    PropertyKeyword,
    ParameterKeyword,
    IndexKeyword,
    QueryKeyword,
    TriggerKeyword,
    ForeignKeyKeyword,
    XDataKeyword,
);

impl HasMemberKeyword for ProjectionKeyword {
    fn member(&self) -> Option<&MemberKeyword> {
        let Self::Member(keyword) = self;
        Some(keyword)
    }
}

/// KEY=ROBJ_method, plus `SqlRoutine` and `ZenMethod`, which IRIS still accepts.
#[derive(Grammar)]
pub enum MethodKeyword {
    Member(MemberKeyword),
    Abstract(Flag<StringEqCI!("Abstract")>),
    ClientName(Valued<StringEqCI!("ClientName"), KeywordValue>),
    CodeMode(Valued<StringEqCI!("CodeMode"), CodeMode>),
    ExternalProcName(Valued<StringEqCI!("ExternalProcName"), KeywordValue>),
    Final(Flag<StringEqCI!("Final")>),
    ForceGenerate(Flag<StringEqCI!("ForceGenerate")>),
    GenerateAfter(Valued<StringEqCI!("GenerateAfter"), KeywordValue>),
    Language(Valued<StringEqCI!("Language"), MethodLanguage>),
    NotInheritable(Flag<StringEqCI!("NotInheritable")>),
    PlaceAfter(Valued<StringEqCI!("PlaceAfter"), KeywordValue>),
    Private(Flag<StringEqCI!("Private")>),
    ProcedureBlock(Valued<StringEqCI!("ProcedureBlock"), Bit>),
    PublicList(Valued<StringEqCI!("PublicList"), KeywordValue>),
    Requires(Valued<StringEqCI!("Requires"), KeywordValue>),
    ReturnResultsets(Flag<StringEqCI!("ReturnResultsets")>),
    ServerOnly(Valued<StringEqCI!("ServerOnly"), Bit>),
    SoapAction(Valued<StringEqCI!("SoapAction"), KeywordValue>),
    SoapBindingStyle(Valued<StringEqCI!("SoapBindingStyle"), KeywordValue>),
    SoapBodyUse(Valued<StringEqCI!("SoapBodyUse"), KeywordValue>),
    SoapMessageName(Valued<StringEqCI!("SoapMessageName"), KeywordValue>),
    SoapNameSpace(Valued<StringEqCI!("SoapNameSpace"), KeywordValue>),
    SoapRequestMessage(Valued<StringEqCI!("SoapRequestMessage"), KeywordValue>),
    SoapTypeNameSpace(Valued<StringEqCI!("SoapTypeNameSpace"), KeywordValue>),
    SqlName(Valued<StringEqCI!("SqlName"), KeywordValue>),
    SqlProc(Flag<StringEqCI!("SqlProc")>),
    SqlRoutine(Valued<StringEqCI!("SqlRoutine"), KeywordValue>),
    WebMethod(Flag<StringEqCI!("WebMethod")>),
    ZenMethod(Flag<StringEqCI!("ZenMethod")>),
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum Cardinality {
    One(StringEqCI!("one")),
    Many(StringEqCI!("many")),
    Parent(StringEqCI!("parent")),
    Children(StringEqCI!("children")),
}

/// KEY=ROBJ_property; a relationship takes the same.
#[derive(Grammar)]
pub enum PropertyKeyword {
    Member(MemberKeyword),
    Aliases(Valued<StringEqCI!("Aliases"), KeywordValue>),
    Calculated(Flag<StringEqCI!("Calculated")>),
    Cardinality(Valued<StringEqCI!("Cardinality"), Cardinality>),
    ClientName(Valued<StringEqCI!("ClientName"), KeywordValue>),
    Collection(Valued<StringEqCI!("Collection"), KeywordValue>),
    ComputeLocalOnly(Flag<StringEqCI!("ComputeLocalOnly")>),
    Deferred(Flag<StringEqCI!("Deferred")>),
    Final(Flag<StringEqCI!("Final")>),
    Identity(Flag<StringEqCI!("Identity")>),
    InitialExpression(Valued<StringEqCI!("InitialExpression"), KeywordValue>),
    Inverse(Valued<StringEqCI!("Inverse"), KeywordValue>),
    MultiDimensional(Flag<StringEqCI!("MultiDimensional")>),
    OnDelete(Valued<StringEqCI!("OnDelete"), KeywordValue>),
    Private(Flag<StringEqCI!("Private")>),
    ReadOnly(Flag<StringEqCI!("ReadOnly")>),
    Required(Flag<StringEqCI!("Required")>),
    ServerOnly(Valued<StringEqCI!("ServerOnly"), Bit>),
    SqlColumnNumber(Valued<StringEqCI!("SqlColumnNumber"), KeywordValue>),
    SqlComputeCode(Valued<StringEqCI!("SqlComputeCode"), KeywordValue>),
    SqlComputed(Flag<StringEqCI!("SqlComputed")>),
    SqlComputeOnChange(Valued<StringEqCI!("SqlComputeOnChange"), KeywordValue>),
    SqlFieldName(Valued<StringEqCI!("SqlFieldName"), KeywordValue>),
    SqlListDelimiter(Valued<StringEqCI!("SqlListDelimiter"), KeywordValue>),
    SqlListType(Valued<StringEqCI!("SqlListType"), KeywordValue>),
    Transient(Flag<StringEqCI!("Transient")>),
}

/// KEY=ROBJ_parameter, plus `Encoded`, which IRIS still accepts.
#[derive(Grammar)]
pub enum ParameterKeyword {
    Member(MemberKeyword),
    Abstract(Flag<StringEqCI!("Abstract")>),
    Constraint(Valued<StringEqCI!("Constraint"), KeywordValue>),
    Encoded(Flag<StringEqCI!("Encoded")>),
    Final(Flag<StringEqCI!("Final")>),
    Flags(Valued<StringEqCI!("Flags"), KeywordValue>),
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum IndexType {
    Bitmap(StringEqCI!("bitmap")),
    Bitslice(StringEqCI!("bitslice")),
    CollatedKey(StringEqCI!("collatedkey")),
    Columnar(StringEqCI!("columnar")),
    Index(StringEqCI!("index")),
    Key(StringEqCI!("key")),
}

/// KEY=ROBJ_index.
#[derive(Grammar)]
pub enum IndexKeyword {
    Member(MemberKeyword),
    Abstract(Flag<StringEqCI!("Abstract")>),
    Condition(Valued<StringEqCI!("Condition"), KeywordValue>),
    CoshardWith(Valued<StringEqCI!("CoshardWith"), KeywordValue>),
    Data(Valued<StringEqCI!("Data"), KeywordValue>),
    Deferred(Flag<StringEqCI!("Deferred")>),
    Extent(Flag<StringEqCI!("Extent")>),
    IdKey(Flag<StringEqCI!("IdKey")>),
    PrimaryKey(Flag<StringEqCI!("PrimaryKey")>),
    ShardKey(Flag<StringEqCI!("ShardKey")>),
    SqlName(Valued<StringEqCI!("SqlName"), KeywordValue>),
    Type(Valued<StringEqCI!("Type"), IndexType>),
    Unique(Flag<StringEqCI!("Unique")>),
}

/// KEY=ROBJ_query.
#[derive(Grammar)]
pub enum QueryKeyword {
    Member(MemberKeyword),
    ClientName(Valued<StringEqCI!("ClientName"), KeywordValue>),
    Final(Flag<StringEqCI!("Final")>),
    Private(Flag<StringEqCI!("Private")>),
    Requires(Valued<StringEqCI!("Requires"), KeywordValue>),
    SoapBindingStyle(Valued<StringEqCI!("SoapBindingStyle"), KeywordValue>),
    SoapBodyUse(Valued<StringEqCI!("SoapBodyUse"), KeywordValue>),
    SoapNameSpace(Valued<StringEqCI!("SoapNameSpace"), KeywordValue>),
    SqlName(Valued<StringEqCI!("SqlName"), KeywordValue>),
    SqlProc(Flag<StringEqCI!("SqlProc")>),
    SqlView(Flag<StringEqCI!("SqlView")>),
    SqlViewName(Valued<StringEqCI!("SqlViewName"), KeywordValue>),
    WebMethod(Flag<StringEqCI!("WebMethod")>),
}

/// KEY=ROBJ_trigger.
#[derive(Grammar)]
pub enum TriggerKeyword {
    Member(MemberKeyword),
    CodeMode(Valued<StringEqCI!("CodeMode"), KeywordValue>),
    Event(Valued<StringEqCI!("Event"), KeywordValue>),
    Final(Flag<StringEqCI!("Final")>),
    Foreach(Valued<StringEqCI!("Foreach"), KeywordValue>),
    Language(Valued<StringEqCI!("Language"), KeywordValue>),
    NewTable(Valued<StringEqCI!("NewTable"), KeywordValue>),
    OldTable(Valued<StringEqCI!("OldTable"), KeywordValue>),
    Order(Valued<StringEqCI!("Order"), KeywordValue>),
    SqlName(Valued<StringEqCI!("SqlName"), KeywordValue>),
    Time(Valued<StringEqCI!("Time"), KeywordValue>),
    UpdateColumnList(Valued<StringEqCI!("UpdateColumnList"), KeywordValue>),
}

/// KEY=ROBJ_foreignkey.
#[derive(Grammar)]
pub enum ForeignKeyKeyword {
    Member(MemberKeyword),
    NoCheck(Flag<StringEqCI!("NoCheck")>),
    OnDelete(Valued<StringEqCI!("OnDelete"), KeywordValue>),
    OnUpdate(Valued<StringEqCI!("OnUpdate"), KeywordValue>),
    SqlName(Valued<StringEqCI!("SqlName"), KeywordValue>),
}

/// KEY=ROBJ_xdata.
#[derive(Grammar)]
pub enum XDataKeyword {
    Member(MemberKeyword),
    MimeType(Valued<StringEqCI!("MimeType"), KeywordValue>),
    SchemaSpec(Valued<StringEqCI!("SchemaSpec"), KeywordValue>),
    XMLNamespace(Valued<StringEqCI!("XMLNamespace"), KeywordValue>),
}

/// KEY=ROBJ_projection.
#[derive(Grammar)]
pub enum ProjectionKeyword {
    Member(MemberKeyword),
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum ImportIncludeLCPrefix {
    SS(StringEq!("//")),
}

#[derive(Grammar)]
pub enum DocumentLevelWhitespace {
    S(Space1),
    L(GenericLineComment<ImportIncludeLCPrefix>),
    R(RangeComment),
}

#[derive(Grammar)]
#[grammar(hidden)]
pub struct ImportIncludeGap(pub Raw<Vec<DocumentLevelWhitespace>>);

#[derive(Grammar)]
pub struct Import {
    pub keyword: Raw<(StringEqCI!("Import"), Space1)>,
    pub packages: ClassList,
    pub gap: ImportIncludeGap,
}

#[derive(Grammar)]
pub enum ClassList {
    Single(Range<ClassName>),
    // An entry may be empty, `(, A)` or `(A, , B)`: IRIS compiles the list with it.
    Multiple {
        open: Range<Raw<(StringEq!("("), Space)>>,
        first: Option<Range<ClassName>>,
        rest: Vec<(
            Range<Raw<(Space, StringEq!(","), Space)>>,
            Option<Range<ClassName>>,
        )>,
        close: Raw<(Space, StringEq!(")"))>,
    },
}

impl ClassList {
    pub fn classes(&self) -> Vec<&Range<ClassName>> {
        match self {
            ClassList::Single(id) => vec![id],
            ClassList::Multiple { first, rest, .. } => (first.iter())
                .chain(rest.iter().filter_map(|(_, id)| id.as_ref()))
                .collect(),
        }
    }

    /// The byte span of the `(` or `,` before each empty entry.
    pub fn empty_entries(&self) -> Vec<(usize, usize)> {
        match self {
            ClassList::Single(_) => vec![],
            ClassList::Multiple {
                open, first, rest, ..
            } => (first
                .is_none()
                .then_some((open.start, open.end))
                .into_iter())
            .chain(
                (rest.iter().filter(|(_, id)| id.is_none()))
                    .map(|(comma, _)| (comma.start, comma.end)),
            )
            .collect(),
        }
    }
}

#[derive(Grammar)]
pub struct IncludeCode {
    pub keyword: Raw<(StringEqCI!("Include"), Space1)>,
    pub files: Range<ClassList>,
    pub gap: ImportIncludeGap,
}

#[derive(Grammar)]
pub struct IncludeGeneratorCode {
    pub keyword: Raw<(StringEqCI!("IncludeGenerator"), Space1)>,
    pub files: Range<ClassList>,
    pub gap: ImportIncludeGap,
}

#[derive(Grammar)]
pub struct Extends {
    keyword: Raw<(Space1, StringEqCI!("Extends"), Space1)>,
    pub class_list: ClassList,
}

#[derive(Grammar)]
pub struct ForeignKey {
    keyword: Raw<(StringEqCI!("ForeignKey"), Space1)>,
    pub name: QuotableClassMemberName,
    key_props: FKKeyProps,
    refs_keyword: Raw<(Space1, StringEqCI!("References"), Space1)>,
    ref_class: ClassName,
    ref_index: Option<RefIndex>,
    pub keywords: Option<KeywordList<FailableKeyword<ForeignKeyKeyword>>>,
    semicolon: Raw<(Space, StringEq!(";"))>,
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct FKKeyProps {
    open: Raw<(StringEq!("("), Space)>,
    first: Option<ClassName>,
    rest: Vec<(Raw<(Space, StringEq!(","), Space)>, ClassName)>,
    close: Raw<(Space, StringEq!(")"))>,
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct RefIndex {
    open: Raw<(StringEq!("("), Space)>,
    name: Option<Identifier>,
    close: Raw<(Space, StringEq!(")"))>,
}

#[derive(Grammar)]
pub struct Index {
    keyword: Raw<(StringEqCI!("Index"), Space1)>,
    pub name: QuotableClassMemberName,
    on_clause: Option<IndexOn>,
    pub keywords: Option<KeywordList<FailableKeyword<IndexKeyword>>>,
    semicolon: Raw<(Space, StringEq!(";"))>,
}

impl Index {
    // The keys this index is defined `On`, in order; `None` without an `On` clause.
    pub fn on_properties(&self) -> Option<Vec<&IndexProperty>> {
        Some(match &self.on_clause.as_ref()?.properties {
            IndexPropertyList::Single(property) => vec![property],
            IndexPropertyList::Multiple(properties) => properties
                .first
                .iter()
                .chain(properties.rest.iter().map(|(_, property)| property))
                .collect(),
        })
    }
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct IndexOn {
    keyword: Raw<(Space1, StringEqCI!("On"), Space1)>,
    properties: IndexPropertyList,
    // A functional index's class, with its parameters: `As %iFind.Index.Basic(LANGUAGE = "en")`.
    index_class: Option<As<Type>>,
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum IndexPropertyList {
    Single(IndexProperty),
    Multiple(IndexPropertyMultiple),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct IndexPropertyMultiple {
    open: Raw<(StringEq!("("), Space)>,
    first: Option<IndexProperty>,
    rest: Vec<(Raw<(Space, StringEq!(","), Space)>, IndexProperty)>,
    close: Raw<(Space, StringEq!(")"))>,
}

#[derive(Grammar)]
pub struct IndexProperty {
    // Quoted when it has a character outside the identifier set, like any member name.
    pub name: QuotableClassMemberName,
    // `Home.State`: a property of the embedded object `Home` holds.
    path: Vec<(StringEq!("."), ClassMemberName)>,
    pub kind: Option<IndexPropertyKind>,
    as_collation: Option<AsCollation>,
}

impl IndexProperty {
    // The property indexed, `Home.State` for one of an embedded object.
    pub fn property(&self) -> String {
        std::iter::once(self.name.name())
            .chain(self.path.iter().map(|(_, segment)| segment.print()))
            .collect::<Vec<_>>()
            .join(".")
    }

    // The `As collation`, as written.
    pub fn collation(&self) -> Option<String> {
        self.as_collation.as_ref().map(|c| c.collation.print())
    }
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum IndexPropertyKind {
    Keys(StringEqCI!("(KEYS)")),
    Elements(StringEqCI!("(ELEMENTS)")),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct AsCollation {
    keyword: Raw<(Space1, StringEqCI!("As"), Space1)>,
    collation: CollationType,
}

#[derive(Grammar)]
pub enum CollationType {
    Exact(StringEqCI!("EXACT")),
    SqlString(Truncating<StringEqCI!("SQLSTRING")>),
    SqlUpper(Truncating<StringEqCI!("SQLUPPER")>),
    Truncate(Truncating<StringEqCI!("TRUNCATE")>),
    Plus(StringEqCI!("PLUS")),
    Minus(StringEqCI!("MINUS")),
    // The legacy collations: of these only `STRING` takes a length (`UPPER(50)` is IRIS #5177).
    Upper(StringEqCI!("UPPER")),
    Space(StringEqCI!("SPACE")),
    AlphaUp(StringEqCI!("ALPHAUP")),
    Mvr(StringEqCI!("MVR")),
    String(Truncating<StringEqCI!("STRING")>),
}

// A collation that may truncate to a length: `SQLUPPER(250)`.
#[derive(Grammar)]
#[grammar(inline)]
pub struct Truncating<K> {
    keyword: K,
    length: Option<(StringEq!("("), StringOf1<IsDigit>, StringEq!(")"))>,
}

char_class!(pub IsDigit, "digit", |c| c.is_ascii_digit());

#[derive(Grammar)]
pub struct MethodHeader {
    pub keyword: MethodKind,
    pub name: QuotableClassMemberName,
    pub formal_spec: Option<FormalSpec>,
    pub return_type: Range<Option<As<Type>>>,
    pub keywords: Option<KeywordList<FailableKeyword<MethodKeyword>>>,
}

#[derive(Grammar, Clone)]
pub enum CodeMode {
    Call(StringEqCI!("Call")),
    Code(StringEqCI!("Code")),
    Expression(StringEqCI!("Expression")),
    ObjectGenerator(StringEqCI!("ObjectGenerator")),
    Generator(StringEqCI!("Generator")),
}

impl Default for CodeMode {
    fn default() -> Self {
        Self::parse("Code").unwrap()
    }
}

#[derive(Grammar, Clone)]
pub enum MethodLanguage {
    Python(StringEqCI!("Python")),
    JavaScript(StringEqCI!("JavaScript")),
    ObjectScript(StringEqCI!("ObjectScript")),
    // The legacy name IRIS still compiles as ObjectScript.
    Cache(StringEqCI!("Cache")),
    TSQL(StringEqCI!("TSQL")),
    ISPL(StringEqCI!("ISPL")),
    // Not among the document's values; IRIS's own `%MV` classes compile with it.
    MVBasic(StringEqCI!("MVBasic")),
}

impl Default for MethodLanguage {
    fn default() -> Self {
        Self::parse("objectscript").unwrap()
    }
}

pub struct Method {
    pub header: MethodHeader,
    pub body: Prefix<Space, Brace<Range<OSText>>>,
}

type OSBody = Prefix<Space, Brace<Range<OSText>>>;

// What may follow a member's closing `}`: another member, a `///` doc comment, or the class's own
// closing `}` ending the document (a bare `}` may just as well be the enclosing method's).
#[derive(Grammar)]
pub enum AfterMember {
    Method(MethodKind),
    Property(Raw<(StringEqCI!("Property"), Space1)>),
    Relationship(Raw<(StringEqCI!("Relationship"), Space1)>),
    Parameter(Raw<(StringEqCI!("Parameter"), Space1)>),
    Index(Raw<(StringEqCI!("Index"), Space1)>),
    ForeignKey(Raw<(StringEqCI!("ForeignKey"), Space1)>),
    Query(Raw<(StringEqCI!("Query"), Space1)>),
    Trigger(Raw<(StringEqCI!("Trigger"), Space1)>),
    XData(Raw<(StringEqCI!("XData"), Space1)>),
    Projection(Raw<(StringEqCI!("Projection"), Space1)>),
    Storage(Raw<(StringEqCI!("Storage"), Space1)>),
    Doc(StringEq!("///")),
    Close(
        (
            StringEq!("}"),
            ImportIncludeGap,
            NotFollowedBy<CharOf<AnyChar>>,
        ),
    ),
}

// A body in another language (`[ Language = python ]`) can't be brace-matched with ObjectScript
// string/comment rules. It ends at a `}` starting a line that is followed by what may follow a
// member -- not at the first such `}`, which is routinely a `switch`'s or block's in JavaScript.
pub type ForeignBodyEnd = (StringEq!("\n}"), ImportIncludeGap, AfterMember);

pub type ForeignBody = Raw<(
    Vec<(NotFollowedBy<ForeignBodyEnd>, CharOf<AnyChar>)>,
    StringEq!("\n"),
)>;

// What must follow a member's closing `}` for an ObjectScript brace-matching of its body to be
// believed -- a `'}'` in embedded SQL or Python would otherwise end the body early, unnoticed.
pub type MemberEnd = NotFollowedBy<NotFollowedBy<(ImportIncludeGap, AfterMember)>>;

// A `Query`/`Trigger` body: SQL, TSQL or ObjectScript, with no keyword to tell which reliably, so
// brace-matched as ObjectScript when that ends where a member may end, else like a foreign body.
#[derive(Grammar)]
pub enum MemberBody {
    OS(
        Raw<(Space, StringEq!("{"))>,
        OSText,
        StringEq!("}"),
        MemberEnd,
    ),
    Foreign(Raw<(Space, StringEq!("{"))>, ForeignBody, StringEq!("}")),
}

fn foreign_body(input: &str, pos: usize, mut state: State) -> Option<(OSBody, usize)> {
    let (space, pos) = Space::parse_at(input, pos, state.reborrow())?;
    let (open, start) = <StringEq!("{")>::parse_at(input, pos, state.reborrow())?;
    let end = ForeignBody::scan_at(input, start, state.reborrow())?;
    let text = Range::new(start, OSText(input[start..end].into()), end);
    let (close, pos) = <StringEq!("}")>::parse_at(input, end, state)?;
    Some((Prefix::new(space, Wrap::new(open, text, close)), pos))
}

impl MethodHeader {
    fn is_objectscript(&self) -> bool {
        let language = self
            .keywords
            .iter()
            .flat_map(KeywordList::items)
            .find_map(|entry| match &***entry {
                Either::Left(MethodKeyword::Language(language)) => Some(&language.value),
                _ => None,
            });
        matches!(
            language,
            None | Some(MethodLanguage::ObjectScript(..) | MethodLanguage::Cache(..))
        )
    }
}

// Hand-written so the body scanner can depend on the header's `Language` keyword. Without one
// the body is brace-matched as ObjectScript -- but only believed if that ends where a member may
// end, since the class-level `[ Language = python ]` default isn't visible here.
impl Grammar for Method {
    type First = <MethodHeader as Grammar>::First;

    fn parse_at(input: &str, pos: usize, mut state: State) -> Option<(Self, usize)> {
        let (header, pos) = MethodHeader::parse_at(input, pos, state.reborrow())?;
        let os_body = |mut state: State| {
            let (body, end) = OSBody::parse_at(input, pos, state.reborrow())?;
            MemberEnd::scan_at(input, end, state)?;
            Some((body, end))
        };
        let (body, pos) = if header.is_objectscript() {
            os_body(state.reborrow())
                .or_else(|| foreign_body(input, pos, state.reborrow()))
                .or_else(|| OSBody::parse_at(input, pos, state))?
        } else {
            foreign_body(input, pos, state.reborrow())
                .or_else(|| OSBody::parse_at(input, pos, state))?
        };
        Some((Method { header, body }, pos))
    }

    fn scan_at(input: &str, pos: usize, state: State) -> Option<usize> {
        Self::parse_at(input, pos, state).map(|(_, end)| end)
    }

    fn print_to(&self, buf: &mut String) {
        self.header.print_to(buf);
        self.body.print_to(buf);
    }

    fn to_bnf() -> bnf::Expr {
        bnf::Expr::sequence(vec![MethodHeader::to_bnf(), OSBody::to_bnf()])
    }

    fn fail_at(pos: usize, state: State) -> bool {
        MethodHeader::fail_at(pos, state)
    }
}

impl Deref for Method {
    type Target = MethodHeader;

    fn deref(&self) -> &Self::Target {
        &self.header
    }
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum MethodKind {
    Method(Raw<(StringEqCI!("Method"), Space1)>),
    ClassMethod(Raw<(StringEqCI!("ClassMethod"), Space1)>),
    ClientMethod(Raw<(StringEqCI!("ClientMethod"), Space1)>),
}

#[derive(Grammar)]
pub struct FormalSpec {
    open: Raw<(StringEq!("("), Space)>,
    formals: Option<FormalList>,
    close: Raw<(Space, StringEq!(")"))>,
}

impl FormalSpec {
    pub fn formals(&self) -> Vec<&Range<Formal>> {
        self.formals
            .iter()
            .flat_map(|formals| formals.0.items.iter())
            .collect()
    }

    pub fn into_formals(self) -> Vec<Range<Formal>> {
        self.formals
            .into_iter()
            .flat_map(|formals| formals.0.items)
            .collect()
    }
}

#[derive(Grammar)]
pub struct FormalList(pub VecSep<Range<Formal>, Raw<(Space, StringEq!(","), Space)>>);

#[derive(Grammar)]
pub struct Formal {
    pub call_type: Option<Suffix<CallType, Space1>>,
    pub name: Range<LocalVariableName>,
    pub variadic: Option<StringEq!("...")>,
    pub as_type: Option<As<Type>>,
    pub default: Option<FormalDefault>,
}

pub type FormalDefault = Prefix<Raw<(Space, StringEq!("="), Space)>, Range<FormalDefaultValue>>;

// `= literal`, or `= {expr}`: code of the method, run on entry when the argument wasn't passed.
#[derive(Grammar)]
pub enum FormalDefaultValue {
    Brace {
        open: Raw<(StringEq!("{"), Space)>,
        expr: Range<Expr>,
        close: Raw<(Space, StringEq!("}"))>,
    },
    // Including braces whose text is no expression, for the checker to report.
    Other(OSExpr),
}

#[derive(Grammar)]
pub enum CallType {
    Output(StringEqCI!("Output")),
    ByRef(StringEqCI!("ByRef")),
    ByVal(StringEqCI!("ByVal")),
}

impl Default for CallType {
    fn default() -> Self {
        CallType::parse("ByVal").unwrap()
    }
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct As<Type: Grammar> {
    as_kw: Raw<(Space1, StringEqCI!("As"), Space1)>,
    pub ann: Type,
}

impl<T: Grammar> Deref for As<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.ann
    }
}

type DefaultValue = Prefix<Raw<(Space, StringEq!("="), Space)>, Range<OSExpr>>;

#[derive(Grammar)]
pub struct Parameter {
    keyword: Raw<(StringEqCI!("Parameter"), Space1)>,
    pub name: QuotableClassMemberName,
    pub as_type: Option<As<Type>>,
    pub keywords: Option<KeywordList<FailableKeyword<ParameterKeyword>>>,
    pub default: Option<DefaultValue>,
    semicolon: Raw<(Space, StringEq!(";"))>,
}

#[derive(Grammar)]
pub struct Projection {
    keyword: Raw<(StringEqCI!("Projection"), Space1)>,
    pub name: QuotableClassMemberName,
    as_type: Option<As<Type>>,
    pub keywords: Option<KeywordList<FailableKeyword<ProjectionKeyword>>>,
    semicolon: Raw<(Space, StringEq!(";"))>,
}

#[derive(Grammar)]
pub enum Property {
    Simple {
        keyword: Raw<(StringEqCI!("Property"), Space1)>,
        name: QuotableClassMemberName,
        as_type: Option<As<PropertyType>>,
        type_params: Option<TypeParamList>,
        keywords: Option<KeywordList<FailableKeyword<PropertyKeyword>>>,
        semicolon: Raw<(Space, StringEq!(";"))>,
    },
    // IRIS compiles `As list Of C`, a property's collection, and ignores the collection.
    Relationship {
        keyword: Raw<(StringEqCI!("Relationship"), Space1)>,
        name: QuotableClassMemberName,
        as_type: Option<As<PropertyType>>,
        type_params: Option<TypeParamList>,
        keywords: Option<KeywordList<FailableKeyword<PropertyKeyword>>>,
        semicolon: Raw<(Space, StringEq!(";"))>,
    },
}

impl Property {
    pub fn name(&self) -> &QuotableClassMemberName {
        match &self {
            Property::Simple { name, .. } => name,
            Property::Relationship { name, .. } => name,
        }
    }
    pub fn keywords(&self) -> &Option<KeywordList<FailableKeyword<PropertyKeyword>>> {
        match &self {
            Property::Simple { keywords, .. } => keywords,
            Property::Relationship { keywords, .. } => keywords,
        }
    }
}

#[derive(Grammar)]
pub struct Query {
    keyword: Raw<(StringEqCI!("Query"), Space1)>,
    pub name: QuotableClassMemberName,
    pub formal_spec: Option<FormalSpec>,
    pub return_type: Option<As<Type>>,
    pub keywords: Option<KeywordList<FailableKeyword<QueryKeyword>>>,
    body: MemberBody,
}

#[derive(Grammar)]
pub struct Storage {
    keyword: Raw<(StringEqCI!("Storage"), Space1)>,
    pub name: QuotableClassMemberName,
    pub keywords: Option<KeywordList>,
    open: Raw<(Space, StringEq!("{"), Space)>,
    content: XML,
    close: Raw<(Space, StringEq!("}"))>,
}

#[derive(Grammar)]
pub struct Trigger {
    keyword: Raw<(StringEqCI!("Trigger"), Space1)>,
    pub name: QuotableClassMemberName,
    pub keywords: Option<KeywordList<FailableKeyword<TriggerKeyword>>>,
    body: MemberBody,
}

#[derive(Grammar)]
pub enum XData {
    XML(XDataOfXML),
    Other(XDataOfOther),
}

impl XData {
    pub fn name(&self) -> &QuotableClassMemberName {
        match &self {
            XData::XML(xdata_of_xml) => &xdata_of_xml.name,
            XData::Other(xdata_of_other) => &xdata_of_other.name,
        }
    }

    pub fn keywords(&self) -> Option<&KeywordList<FailableKeyword<XDataKeyword>>> {
        match &self {
            XData::XML(xdata_of_xml) => xdata_of_xml.keywords.as_ref().map(|list| &list.0),
            XData::Other(xdata_of_other) => xdata_of_other.keywords.as_ref(),
        }
    }
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct XDataOfXML {
    keyword: Raw<(StringEqCI!("XData"), Space1)>,
    name: QuotableClassMemberName,
    keywords: Option<KeywordListWithMimeTypeIsXML>,
    open: Raw<(Space, StringEq!("{"), Space)>,
    content: XML,
    close: Raw<(Space, StringEq!("}"))>,
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct XDataOfOther {
    keyword: Raw<(StringEqCI!("XData"), Space1)>,
    name: QuotableClassMemberName,
    pub keywords: Option<KeywordList<FailableKeyword<XDataKeyword>>>,
    open: Raw<(Space, StringEq!("{"), Space)>,
    content: Data,
    close: Raw<(Space, StringEq!("}"))>,
}

#[derive(Grammar)]
#[grammar(validated)]
pub struct KeywordListWithMimeTypeIsXML(KeywordList<FailableKeyword<XDataKeyword>>);

impl Validate for KeywordListWithMimeTypeIsXML {
    const REQUIREMENT: &'static str = "have MimeType end with /xml";
    fn validate(&self) -> bool {
        self.0.items().all(|entry| match &***entry {
            Either::Left(XDataKeyword::MimeType(mime_type)) => mime_type.value.0.ends_with("/xml"),
            _ => true,
        })
    }
}

#[derive(Grammar)]
pub enum XMLUnit {
    S(Space1),
    C(XMLComment),
    // `<?xml version="1.0"?>`, which an XData may open with.
    PI(StringEq!("<?"), StringEndsWith<StringEq!("?>")>),
    JS(
        Raw<(StringEqCI!("<script"), HTMLTagContent, StringEq!(">"))>,
        StringEndsWith<StringEqCI!("</script>")>,
    ),
    CSS(
        Raw<(StringEqCI!("<style"), HTMLTagContent, StringEq!(">"))>,
        StringEndsWith<StringEqCI!("</style>")>,
    ),
    Single(XMLSingleTag),
    OpenClose(
        XMLOpenTag,
        Vec<Either<XMLUnit, StringOf1<IsXMLSafe>>>,
        XMLCloseTag,
    ),
    CDATA(StringEqCI!("<![CDATA["), StringEndsWith<StringEqCI!("]]>")>),
}

#[derive(Grammar)]
pub struct XMLCommentOpen(StringEq!("<!--"));
#[derive(Grammar)]
pub struct XMLCommentClose(StringEq!("-->"));
#[derive(Grammar)]
pub struct XMLComment(XMLCommentOpen, StringEndsWith<XMLCommentClose>);

char_class!(pub IsXMLSafe, "xml_safe", |ch| !matches!(ch, '<' | '>'));
char_class!(pub IsXMLTagSafe, "xml_tag_safe", |ch| !matches!(
    ch,
    '<' | '>' | '/' | '"' | '\'' | '!'
));

#[derive(Grammar)]
pub struct XMLSingleTag(StringEqCI!("<"), XMLTagContent, StringEqCI!("/>"));

#[derive(Grammar)]
pub struct XMLOpenTag(StringEqCI!("<"), XMLTagContent, StringEqCI!(">"));

#[derive(Grammar)]
pub struct XMLCloseTag(StringEqCI!("</"), XMLTagContent, StringEqCI!(">"));

#[derive(Grammar)]
pub enum XMLTagContentUnit {
    Txt(StringOf1<IsXMLTagSafe>),
    Str(XMLString),
    // A quote with no closing one is text: IRIS doesn't hold an XData to well-formedness.
    Quote(Either<DQ, SQ>),
}

// An attribute value runs to its closing quote, a `#()#` expression inside allowed: no `\`
// escapes, as a `C:\dir\` path ends in one.
#[derive(Grammar)]
pub enum XMLString {
    DQ(DQ, Vec<Either<HashEmbededOS, CharOf<IsNotDQ>>>, DQ),
    SQ(SQ, Vec<Either<HashEmbededOS, CharOf<IsNotSQ>>>, SQ),
}

char_class!(pub IsNotDQ, "not a double quote", |ch| ch != '"');
char_class!(pub IsNotSQ, "not a single quote", |ch| ch != '\'');
type XMLTagContent = Vec<XMLTagContentUnit>;

#[derive(Grammar)]
pub struct XML(Raw<Vec<XMLUnit>>);

#[derive(Grammar)]
pub enum Member {
    ForeignKey(ForeignKey),
    Index(Index),
    Method(Method),
    Parameter(Parameter),
    Projection(Projection),
    Property(Property),
    Query(Query),
    Storage(Storage),
    Trigger(Trigger),
    XData(XData),
}

#[derive(Grammar)]
pub struct MemberList {
    open: (Space, StringEq!("{")),
    pub members: VecSep<ImportIncludeGap, Range<Member>>,
    close: StringEq!("}"),
}

// IRIS compiles the three in any order, one of each (a repeat fails the load).
#[derive(Grammar)]
pub enum ClassDirective {
    Import(Import),
    Include(IncludeCode),
    IncludeGenerator(IncludeGeneratorCode),
}

impl ClassDirective {
    pub fn gap(&self) -> &ImportIncludeGap {
        match self {
            ClassDirective::Import(d) => &d.gap,
            ClassDirective::Include(d) => &d.gap,
            ClassDirective::IncludeGenerator(d) => &d.gap,
        }
    }
}

#[derive(Grammar)]
#[grammar(validated)]
pub struct ClassDefinitionHeader {
    pub gap_start: ImportIncludeGap,
    pub directives: Vec<ClassDirective>,
    pub class_keyword: (StringEqCI!("Class"), Space1),
    pub name: Range<ClassName>,
}

impl Validate for ClassDefinitionHeader {
    const REQUIREMENT: &'static str = "name Import, Include and IncludeGenerator at most once each";
    fn validate(&self) -> bool {
        let count =
            |is: fn(&ClassDirective) -> bool| self.directives.iter().filter(|d| is(d)).count();
        count(|d| matches!(d, ClassDirective::Import(_))) <= 1
            && count(|d| matches!(d, ClassDirective::Include(_))) <= 1
            && count(|d| matches!(d, ClassDirective::IncludeGenerator(_))) <= 1
    }
}

impl ClassDefinitionHeader {
    pub fn import(&self) -> Option<&Import> {
        self.directives.iter().find_map(|d| match d {
            ClassDirective::Import(import) => Some(import),
            _ => None,
        })
    }

    pub fn include(&self) -> Option<&IncludeCode> {
        self.directives.iter().find_map(|d| match d {
            ClassDirective::Include(include) => Some(include),
            _ => None,
        })
    }

    pub fn include_gen(&self) -> Option<&IncludeGeneratorCode> {
        self.directives.iter().find_map(|d| match d {
            ClassDirective::IncludeGenerator(include) => Some(include),
            _ => None,
        })
    }
}

#[derive(Grammar)]
pub struct ClassDefinition {
    header: ClassDefinitionHeader,
    pub extends: Option<Extends>,
    pub keyword_list: Option<KeywordList<FailableKeyword<ClassKeyword>>>,
    pub member_list: MemberList,
    pub gap_end: ImportIncludeGap,
}

impl Deref for ClassDefinition {
    type Target = ClassDefinitionHeader;

    fn deref(&self) -> &Self::Target {
        &self.header
    }
}

type CommandArgument<T> = Prefix<StringEq!(" "), Box<Range<T>>>;

#[derive(Grammar)]
pub struct IfConditions {
    pub first: Range<ExpressionAfterIf>,
    pub remaining: Vec<Conjunct>,
}

// A condition after the first, behind its comma.
#[derive(Grammar)]
pub struct Conjunct {
    pub comma: CommaSep<LGap>,
    pub expr: Range<Expr>,
}

impl Deref for Conjunct {
    type Target = Range<Expr>;

    fn deref(&self) -> &Self::Target {
        &self.expr
    }
}

#[derive(Grammar)]
pub struct WhileConditions {
    pub first: Range<ExpressionAfterWhile>,
    pub remaining: Vec<Conjunct>,
}

#[derive(Grammar)]
pub struct DoWhileCommand {
    pub kw: KwDo,
    // Not a `Block`: the body must have braces.
    pub iter: CurryBlock<MGap>,
    gap: MGap,
    pub kw_while: Range<KwWhile>,
    pub cnds: Option<WhileConditions>,
}

#[derive(Grammar)]
pub struct DoCommand {
    pub kw: KwDo,
    pub pc: Option<PostCondition>,
    pub args: Option<CommandArgument<VecSep<DoArgument, CommaSep<LGap>>>>,
}

#[derive(Grammar)]
pub struct IfCommand {
    pub kw: KwIf,
    pub cnds: Option<IfConditions>,
    pub branches: Option<IfBranches>,
}

#[derive(Grammar)]
pub struct WhileCommand {
    pub kw: KwWhile,
    pub cnds: Option<WhileConditions>,
    pub bdy: CurryBlock,
}

#[derive(Grammar)]
pub struct ForCommand {
    pub kw: KwFor,
    pub iter: Option<SGap1X<ForIterator>>,
    // Some older code does not have an assoicated block
    pub bdy: Option<Range<Block>>,
}

#[derive(Grammar)]
pub struct GotoCommand {
    pub kw: KwGoto,
    pub pc: Option<PostCondition>,
    pub args: Option<CommandArgument<VecSep<GoToArg, CommaSep<LGap>>>>,
}

#[derive(Grammar)]
pub struct BreakCommand {
    pub kw: KwBreak,
    pub pc: Option<PostCondition>,
    pub exp: Option<CommandArgument<Expr>>,
}

#[derive(Grammar)]
pub struct QuitCommand {
    pub kw: KwQuit,
    pub pc: Option<PostCondition>,
    pub exp: Range<Option<CommandArgument<Expr>>>,
}

#[derive(Grammar)]
pub struct ZQuitCommand {
    pub kw: KwZQuit,
    pub pc: Option<PostCondition>,
    pub exp: Option<CommandArgument<Expr>>,
}

#[derive(Grammar)]
pub struct ReturnCommand {
    pub kw: KwReturn,
    pub pc: Option<PostCondition>,
    pub exp: Range<Option<CommandArgument<Expr>>>,
}

#[derive(Grammar)]
pub struct ContinueCommand {
    pub kw: KwContinue,
    pub pc: Option<PostCondition>,
}

#[derive(Grammar)]
pub struct DimCommand {
    pub kw: StringEqCI!("#dim"),
    pub g: SGap1,
    // `#dim a, b As T, c As U`: each group of names has its type; one without takes the next's.
    pub declarations: VecSep<DimDeclaration, CommaSep<SGap>>,
    pub default: Option<Prefix<SGapXSGap<StringEq!("=")>, Range<GenericExpr<(), SGap>>>>,
}

#[derive(Grammar)]
pub struct DimDeclaration {
    pub name: VecSep<LocalVariableName, CommaSep<SGap>>,
    // The reference makes `As <Type>` mandatory; it is accepted as optional here.
    pub as_type: Range<Option<As<DimType>>>,
}

#[derive(Grammar)]
pub struct SetCommand {
    pub kw: KwSet,
    pub pc: Option<PostCondition>,
    pub assignments: CommandArgument<VecSep<Range<AllowIndirection<SetArgument>>, CommaSep<LGap>>>,
}

#[derive(Grammar)]
pub struct KillCommand {
    pub kw: KwKill,
    pub pc: Option<PostCondition>,
    pub targets: Option<CommandArgument<VecSep<Range<AllowMacro<KillArgument>>, CommaSep<LGap>>>>,
}

#[derive(Grammar)]
pub struct NewCommand {
    pub kw: KwNew,
    pub pc: Option<PostCondition>,
    pub targets: Option<CommandArgument<VecSep<Range<NewArgument>, CommaSep<LGap>>>>,
}

#[derive(Grammar)]
pub struct MergeCommand {
    pub kw: KwMerge,
    pub pc: Option<PostCondition>,
    pub merge_arguments: CommandArgument<VecSep<Range<MergeArgument>, CommaSep<LGap>>>,
}

#[derive(Grammar)]
pub struct TryCommand {
    pub kw_try: KwTry,
    pub b1: XMGap<Range<Block>>,
    pub kw_catch: Range<KwCatch>,
    pub g3: SGap,
    pub catch_target: Option<CatchTarget>,
    pub b2: Range<Block>,
}

#[derive(Grammar)]
pub struct ThrowCommand {
    pub kw: KwThrow,
    pub pc: Option<PostCondition>,
    pub e: Option<CommandArgument<Expr>>,
}

#[derive(Grammar)]
pub struct ZTrapCommand {
    pub kw: KwZTrap,
    pub pc: Option<PostCondition>,
    pub exp: Option<CommandArgument<Expr>>,
}

#[derive(Grammar)]
pub struct ZZDumpCommand {
    pub kw: KwZZDump,
    pub pc: Option<PostCondition>,
    pub exp: CommandArgument<VecSep<Expr, CommaSep<LGap>>>,
}

#[derive(Grammar)]
pub struct WriteCommand {
    pub kw: KwWrite,
    pub pc: Option<PostCondition>,
    pub args: Option<CommandArgument<VecSep<WriteArg, CommaSep<LGap>>>>,
}

#[derive(Grammar)]
pub struct ReadCommand {
    pub kw: KwRead,
    pub pc: Option<PostCondition>,
    pub args: CommandArgument<VecSep<ReadArg, CommaSep<LGap>>>,
}

#[derive(Grammar)]
pub struct OpenCommand {
    pub kw: KwOpen,
    pub pc: Option<PostCondition>,
    pub args: CommandArgument<VecSep<OpenArg, CommaSep<LGap>>>,
}

#[derive(Grammar)]
pub struct CloseCommand {
    pub kw: KwClose,
    pub pc: Option<PostCondition>,
    pub args: CommandArgument<VecSep<CloseArg, CommaSep<LGap>>>,
}

#[derive(Grammar)]
pub struct UseCommand {
    pub kw: KwUse,
    pub pc: Option<PostCondition>,
    pub args: CommandArgument<VecSep<UseArg, CommaSep<LGap>>>,
}

#[derive(Grammar)]
pub struct LockCommand {
    pub kw: KwLock,
    pub pc: Option<PostCondition>,
    pub args: Option<
        CommandArgument<
            VecSep<AllowMacro<(AllowMacro<LockArg>, Option<ReadTimeout>)>, CommaSep<LGap>>,
        >,
    >,
}

#[derive(Grammar)]
pub struct HaltCommand {
    pub kw: KwHalt,
    pub pc: Option<PostCondition>,
}

#[derive(Grammar)]
pub struct HangCommand {
    pub kw: KwHang,
    pub pc: Option<PostCondition>,
    pub e: CommandArgument<Expr>,
}

#[derive(Grammar)]
pub struct HCommand {
    pub kw: KwH,
    pub pc: Option<PostCondition>,
    pub e: Option<CommandArgument<Expr>>,
}

#[derive(Grammar)]
pub struct JobCommand {
    pub kw: KwJob,
    pub pc: Option<PostCondition>,
    pub args: CommandArgument<VecSep<JobArgument, CommaSep<LGap>>>,
}

#[derive(Grammar)]
pub struct TStartCommand {
    pub kw: KwTStart,
    pub pc: Option<PostCondition>,
}

#[derive(Grammar)]
pub struct TCommitCommand {
    pub kw: KwTCommit,
    pub pc: Option<PostCondition>,
}

#[derive(Grammar)]
pub struct TRollbackCommand {
    pub kw: KwTRollback,
    pub pc: Option<PostCondition>,
    pub one: Option<CommandArgument<StringEq!("1")>>,
}

#[derive(Grammar)]
pub struct ZWriteCommand {
    pub kw: KwZWrite,
    pub pc: Option<PostCondition>,
    pub exp: Option<CommandArgument<VecSep<Expr, CommaSep<LGap>>>>,
}

#[derive(Grammar)]
pub struct ZKillCommand {
    pub kw: KwZKill,
    pub pc: Option<PostCondition>,
    pub targets: CommandArgument<VecSep<Range<ZKillTarget>, CommaSep<LGap>>>,
}

#[derive(Grammar)]
pub struct ZNspaceCommand {
    pub kw: KwZNspace,
    pub pc: Option<PostCondition>,
    pub e: CommandArgument<Expr>,
}

#[derive(Grammar)]
pub struct XecuteCommand {
    pub kw: KwXecute,
    pub pc: Option<PostCondition>,
    pub args: CommandArgument<VecSep<XecuteArg, CommaSep<LGap>>>,
}

#[derive(Grammar)]
pub struct ViewCommand {
    pub kw: KwView,
    pub pc: Option<PostCondition>,
    pub exp: Option<CommandArgument<VecSep<ViewArgument, CommaSep<LGap>>>>,
}

#[derive(Grammar)]
pub struct ZPrintCommand {
    pub kw: KwZPrint,
    pub pc: Option<PostCondition>,
    pub args: Option<CommandArgument<PrintArgs>>,
}

#[derive(Grammar)]
pub struct PrintCommand {
    pub kw: KwPrint,
    pub pc: Option<PostCondition>,
    pub args: Option<CommandArgument<PrintArgs>>,
}

#[derive(Grammar)]
pub struct ZBreakCommand {
    pub kw: KwZBreak,
    pub pc: Option<PostCondition>,
    pub args: Option<CommandArgument<VecSep<ZBreakArg, CommaSep<LGap>>>>,
}

// MVP and MVC are undocumented commands
#[derive(Grammar)]
pub struct MVPCommand {
    pub kw: KwMVPrint,
    pub pc: Option<PostCondition>,
    pub args: CommandArgument<VecSep<WriteArg, CommaSep<LGap>>>,
}

#[derive(Grammar)]
pub struct MVCCommand {
    pub kw: KwMVCrt,
    pub pc: Option<PostCondition>,
    pub args: CommandArgument<VecSep<WriteArg, CommaSep<LGap>>>,
}

#[derive(Grammar)]
pub struct JSCommand {
    pub before: JsMarker,
    pub content: EmbeddedJS,
    pub after: StringEq!(">"),
}

#[derive(Grammar)]
pub enum Command {
    Unknown(UnknownCommand),
    DoWhile(DoWhileCommand),
    Do(DoCommand),
    If(IfCommand),
    // Stand-alone ElseIf and Else are needed to process dotted do block, modern tooling should ignore them.
    ElseIf(ElseIfBranch),
    Else(ElseBranch),
    While(WhileCommand),
    For(ForCommand),
    Goto(GotoCommand),
    Break(BreakCommand),
    Quit(QuitCommand),
    ZQuit(ZQuitCommand),
    Return(ReturnCommand),
    Continue(ContinueCommand),

    Dim(DimCommand),
    Set(SetCommand),
    Kill(KillCommand),
    New(NewCommand),
    Merge(MergeCommand),

    Try(TryCommand),
    Throw(ThrowCommand),
    ZTrap(ZTrapCommand),
    ZZDump(ZZDumpCommand),
    Write(WriteCommand),
    Read(ReadCommand),
    Open(OpenCommand),
    Close(CloseCommand),
    Use(UseCommand),

    Lock(LockCommand),
    Halt(HaltCommand),
    Hang(HangCommand),
    H(HCommand),
    Job(JobCommand),

    TStart(TStartCommand),
    TCommit(TCommitCommand),
    TRollback(TRollbackCommand),

    ZWrite(ZWriteCommand),
    ZKill(ZKillCommand),
    ZNspace(ZNspaceCommand),
    Xecute(XecuteCommand),
    View(ViewCommand),

    ZPrint(ZPrintCommand),
    Print(PrintCommand),

    ZBreak(ZBreakCommand),

    MVP(MVPCommand),
    MVC(MVCCommand),

    CallMacro(MacroCall),

    ESQL(
        Either<StringEqCI!("&sql("), StringEqCI!("##sql(")>,
        Box<sql::SQL>,
        StringEq!(")"),
    ),
    HTML(StringEqCI!("&html<"), HTML, StringEq!(">")),
    JS(JSCommand),
    // `ZNAME args`: a user-defined command (`^%ZLANGC00`), documented, unknown to this checker.
    User(UserCommand),
}

#[derive(Grammar)]
pub struct UserCommand {
    pub name: Range<UserCommandName>,
    pub pc: Option<PostCondition>,
    pub args: Option<CommandArgument<VecSep<Range<Expr>, CommaSep<LGap>>>>,
}

// `Z` and letters, and not a command IRIS has. Validated rather than converted: a word that
// isn't one is `UnknownCommand`'s to report, and a conversion failure's own message would be
// appended to that report.
#[derive(Grammar)]
#[grammar(validated)]
pub struct UserCommandName(pub StringOf1<IsAlphabetic>);

impl Validate for UserCommandName {
    const REQUIREMENT: &'static str = "be Z and letters, and not a command IRIS has";
    fn validate(&self) -> bool {
        let s = self.0.print();
        s.len() > 1
            && s.starts_with(['Z', 'z'])
            && !COMMAND_NAMES.iter().any(|(name, abbr)| {
                name.eq_ignore_ascii_case(&s) || abbr.is_some_and(|a| a.eq_ignore_ascii_case(&s))
            })
    }
}

macro_rules! keyword_command {
    ($($name:ident),* $(,)?) => { $(
        impl CommandLike for $name {
            fn keywords(&self, start: usize) -> Vec<KeywordUse> {
                vec![KeywordUse::of(&self.kw, start)]
            }

            fn postcondition(&self) -> Option<&PostCondition> {
                self.pc.as_ref()
            }
        }
    )* };
}

keyword_command! {
    DoCommand,
    GotoCommand,
    BreakCommand,
    QuitCommand,
    ZQuitCommand,
    ReturnCommand,
    ContinueCommand,
    SetCommand,
    KillCommand,
    NewCommand,
    MergeCommand,
    ThrowCommand,
    ZTrapCommand,
    ZZDumpCommand,
    WriteCommand,
    ReadCommand,
    OpenCommand,
    CloseCommand,
    UseCommand,
    LockCommand,
    HaltCommand,
    HangCommand,
    JobCommand,
    TStartCommand,
    TCommitCommand,
    TRollbackCommand,
    ZWriteCommand,
    ZKillCommand,
    ZNspaceCommand,
    XecuteCommand,
    ViewCommand,
    ZPrintCommand,
    PrintCommand,
    ZBreakCommand,
    MVPCommand,
    MVCCommand,
}

impl CommandLike for WhileCommand {
    fn keywords(&self, start: usize) -> Vec<KeywordUse> {
        vec![KeywordUse::of(&self.kw, start)]
    }
}

impl CommandLike for ForCommand {
    fn keywords(&self, start: usize) -> Vec<KeywordUse> {
        vec![KeywordUse::of(&self.kw, start)]
    }
}

impl CommandLike for DoWhileCommand {
    fn keywords(&self, start: usize) -> Vec<KeywordUse> {
        vec![
            KeywordUse::of(&self.kw, start),
            KeywordUse::of(&*self.kw_while, self.kw_while.start),
        ]
    }
}

impl CommandLike for IfCommand {
    fn keywords(&self, start: usize) -> Vec<KeywordUse> {
        let mut keywords = vec![KeywordUse::of(&self.kw, start)];
        if let Some(branches) = &self.branches {
            for eif in &branches.eifs {
                keywords.extend(eif.prefixed.keywords(eif.prefixed.kw.start));
            }
            if let Some(els) = &branches.els {
                keywords.extend(els.prefixed.keywords(els.prefixed.kw.start));
            }
        }
        keywords
    }
}

impl CommandLike for ElseIfBranch {
    fn keywords(&self, _start: usize) -> Vec<KeywordUse> {
        vec![KeywordUse::of(&*self.kw, self.kw.start)]
    }
}

impl CommandLike for ElseBranch {
    fn keywords(&self, _start: usize) -> Vec<KeywordUse> {
        vec![KeywordUse::of(&*self.kw, self.kw.start)]
    }
}

impl CommandLike for TryCommand {
    fn keywords(&self, start: usize) -> Vec<KeywordUse> {
        vec![
            KeywordUse::of(&self.kw_try, start),
            KeywordUse::of(&*self.kw_catch, self.kw_catch.start),
        ]
    }
}

impl CommandLike for HCommand {
    fn keywords(&self, start: usize) -> Vec<KeywordUse> {
        vec![KeywordUse {
            kind: KeywordKind::Command,
            start,
            text: self.kw.print(),
            full: if self.e.is_some() { "Hang" } else { "Halt" },
            abbr: Some("H"),
        }]
    }

    fn postcondition(&self) -> Option<&PostCondition> {
        self.pc.as_ref()
    }
}

// `#Dim` is a preprocessor directive, not a command keyword.
impl CommandLike for DimCommand {
    fn keywords(&self, _start: usize) -> Vec<KeywordUse> {
        vec![]
    }
}

impl Command {
    // `None` for what isn't a keyword command: a macro call or embedded SQL/HTML/JS.
    pub fn as_command_like(&self) -> Option<&dyn CommandLike> {
        Some(match self {
            Command::Unknown(unknown) => match unknown.0 {},
            Command::DoWhile(command) => command,
            Command::Do(command) => command,
            Command::If(command) => command,
            Command::While(command) => command,
            Command::For(command) => command,
            Command::Goto(command) => command,
            Command::Break(command) => command,
            Command::Quit(command) => command,
            Command::ZQuit(command) => command,
            Command::Return(command) => command,
            Command::Continue(command) => command,
            Command::Dim(command) => command,
            Command::Set(command) => command,
            Command::Kill(command) => command,
            Command::New(command) => command,
            Command::Merge(command) => command,
            Command::Try(command) => command,
            Command::Throw(command) => command,
            Command::ZTrap(command) => command,
            Command::ZZDump(command) => command,
            Command::Write(command) => command,
            Command::Read(command) => command,
            Command::Open(command) => command,
            Command::Close(command) => command,
            Command::Use(command) => command,
            Command::Lock(command) => command,
            Command::Halt(command) => command,
            Command::Hang(command) => command,
            Command::H(command) => command,
            Command::Job(command) => command,
            Command::TStart(command) => command,
            Command::TCommit(command) => command,
            Command::TRollback(command) => command,
            Command::ZWrite(command) => command,
            Command::ZKill(command) => command,
            Command::ZNspace(command) => command,
            Command::Xecute(command) => command,
            Command::View(command) => command,
            Command::ZPrint(command) => command,
            Command::Print(command) => command,
            Command::ZBreak(command) => command,
            Command::MVP(command) => command,
            Command::MVC(command) => command,
            Command::ElseIf(branch) => branch,
            Command::Else(branch) => branch,
            Command::CallMacro(_)
            | Command::ESQL(..)
            | Command::HTML(..)
            | Command::JS(_)
            | Command::User(_) => {
                return None;
            }
        })
    }
}

impl CommandLike for Command {
    fn keywords(&self, start: usize) -> Vec<KeywordUse> {
        self.as_command_like()
            .map_or_else(Vec::new, |command| command.keywords(start))
    }

    fn postcondition(&self) -> Option<&PostCondition> {
        self.as_command_like()?.postcondition()
    }
}

#[derive(GrammarFromStr)]
pub struct UnknownCommand(pub std::convert::Infallible);

impl GrammarFrom for UnknownCommand {
    type Source = StringOf1<IsAlphabetic>;

    fn print_to(&self, _buf: &mut String) {
        match self.0 {}
    }
}

impl FromStr for UnknownCommand {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        // A known command lands here when its own grammar failed right after the word.
        if COMMAND_NAMES.iter().any(|(name, abbr)| {
            name.eq_ignore_ascii_case(s) || abbr.is_some_and(|a| a.eq_ignore_ascii_case(s))
        }) {
            return Err(format!(
                "'{s}' is a command, but its argument is missing or doesn't parse"
            ));
        }
        Err(format!(
            "'{s}' is not a recognized command; available: {}",
            COMMAND_NAMES
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

#[derive(Grammar)]
pub struct XecuteArg {
    pub arg: XecuteArgEnum,
    pub pc: Option<PostCondition>,
}

#[derive(Grammar)]
pub enum XecuteArgEnum {
    Expr(Expr),
    Paren(Paren<ParenXecuteArg>),
}

#[derive(Grammar)]
pub struct ParenXecuteArg {
    pub code: Expr,
    pub params: Vec<Prefix<CommaSep<MGap>, MthArg>>,
}

#[derive(Grammar)]
pub enum ZBreakSign {
    MinusMinus(StringEq!("--")),
    Minus(StringEq!("-")),
    Plus(StringEq!("+")),
}

#[derive(Grammar)]
pub enum ZBreakLocationSpec {
    Watchpoint(Prefix<StringEq!("*"), Variable>),
    SingleStep(StringEq!("$")),
    LineRef(LegacyEntryRef),
}

#[derive(Grammar)]
pub struct ZBreakLocation {
    pub sign: Option<ZBreakSign>,
    pub spec: Option<ZBreakLocationSpec>,
    pub delay: Option<Prefix<StringEq!("#"), Expr>>,
}

#[derive(Grammar)]
pub struct ZBreakLocationArg {
    pub location: ZBreakLocation,
    pub action: Option<Prefix<StringEq!(":"), Option<Expr>>>,
    pub condition: Option<Prefix<StringEq!(":"), Option<Expr>>>,
    pub execute_code: Option<Prefix<StringEq!(":"), Option<Expr>>>,
}

#[derive(Grammar)]
pub enum ZBreakTraceMode {
    On(StringEqCI!("ON")),
    Off(StringEqCI!("OFF")),
    All(StringEqCI!("ALL")),
}

#[derive(Grammar)]
pub enum ZBreakOnOff {
    On(StringEqCI!("ON")),
    Off(StringEqCI!("OFF")),
}

#[derive(Grammar)]
pub enum ZBreakInterruptMode {
    Normal(StringEqCI!("NORMAL")),
    Break(StringEqCI!("BREAK")),
}

#[derive(Grammar)]
pub enum ZBreakStepOption {
    Ext(StringEqCI!("EXT")),
    Method(StringEqCI!("METHOD")),
    Destruct(StringEqCI!("DESTRUCT")),
}

#[derive(Grammar)]
pub enum ZBreakCommandOption {
    Clear(Either<StringEqCI!("/CLEAR"), StringEqCI!("/C")>),
    Debug(
        Either<StringEqCI!("/DEBUG"), StringEqCI!("/D")>,
        Prefix<StringEq!(":"), Expr>,
    ),
    Trace(
        Either<StringEqCI!("/TRACE"), StringEqCI!("/T")>,
        Prefix<StringEq!(":"), ZBreakTraceMode>,
        Option<Prefix<StringEq!(":"), Expr>>,
    ),
    ErrorTrap(
        Either<StringEqCI!("/ERRORTRAP"), StringEqCI!("/E")>,
        Prefix<StringEq!(":"), ZBreakOnOff>,
    ),
    Interrupt(
        Either<StringEqCI!("/INTERRUPT"), StringEqCI!("/I")>,
        Prefix<StringEq!(":"), ZBreakInterruptMode>,
    ),
    Step(
        Either<StringEqCI!("/STEP"), StringEqCI!("/S")>,
        Vec1<Prefix<StringEq!(":"), ZBreakStepOption>>,
    ),
    NoStep(
        Either<StringEqCI!("/NOSTEP"), StringEqCI!("/N")>,
        Vec1<Prefix<StringEq!(":"), ZBreakStepOption>>,
    ),
}

#[derive(Grammar)]
pub enum ZBreakArg {
    Command(ZBreakCommandOption),
    Location(ZBreakLocationArg),
}

#[derive(Grammar)]
pub enum PrintLineRef {
    LabelOffset(Label, Option<(StringEq!("+"), Expr)>),
    Offset(StringEq!("+"), Expr),
}

#[derive(Grammar)]
pub struct PrintArgs {
    pub from: PrintLineRef,
    pub to: Option<Prefix<StringEq!(":"), PrintLineRef>>,
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum ParenedExpr<BeforeFirst = ()> {
    Normal(Prefix<BeforeFirst, Range<AtomicExpr>>),
    // The interesting case for BeforeFirst -- When the expression is parenthesized, the BeforeFirst is optional
    Paren(Prefix<Option<BeforeFirst>, ParenExpr>),
}

#[derive(Grammar)]
pub struct DText {
    pub kw: Range<FnText>,
    lp: StringEq!("("),
    pub arg: DTextArg,
    end: StringEq!(")"),
}

#[derive(Grammar)]
pub enum DTextArg {
    Label(AllowIndirection<LabelOffsetRoutine>),
    // `$TEXT(+0)`, `$TEXT(+3^rtn)`: an offset with no label.
    Offset(
        Prefix<StringEq!("+"), Range<Expr>>,
        Option<ExtendedRoutineReference>,
    ),
}

type ArgOf<T> = Wrap<MGap, Range<T>, MGap>;
// An argument that may be left empty, as in `$FN(x,,0)`.
type OptArgOf<T> = Wrap<MGap, Option<Range<T>>, MGap>;
type More<T> = Prefix<StringEq!(","), T>;

// ---- Function arguments ---------------------------------------------------------------------
// Every known function parses its arguments generically and `fun_args!` converts them, so a
// wrong number or kind of argument is reported by number rather than as a low-level parse error.
pub type GenericFunArgs = Option<VecSep<ArgOf<Option<FunArg>>, Range<StringEq!(",")>>>;

// The commas of a generic argument list, as `commas_of` lists them.
fn arg_commas(args: &GenericFunArgs) -> Vec<CommaAt> {
    let Some(list) = args else {
        return vec![];
    };
    let present = |arg: &ArgOf<Option<FunArg>>| arg.wrapped.start < arg.wrapped.end;
    (list.seps.iter().zip(list.items.windows(2)))
        .filter(|(_, pair)| pair.iter().all(present))
        .map(|(comma, pair)| CommaAt {
            start: comma.start,
            before: comma.start - pair[0].wrapped.end,
            after: pair[1].wrapped.start - comma.end,
        })
        .collect()
}

#[derive(Debug)]
pub enum FunArgError {
    TooMany {
        max: usize,
        extra: Vec<(usize, String)>,
    },
    TooFew {
        min: usize,
        given: usize,
    },
    WrongKind {
        index: usize,
        expected: &'static str,
    },
    Unsupported {
        given: usize,
    },
}

impl std::fmt::Display for FunArgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let plural = |n: usize| if n == 1 { "" } else { "s" };
        match self {
            Self::TooMany { max, extra } => {
                write!(f, "takes at most {max} argument{}; extra: ", plural(*max))?;
                for (i, (n, text)) in extra.iter().enumerate() {
                    let sep = if i == 0 { "" } else { ", " };
                    write!(f, "{sep}argument {n} (`{text}`)")?;
                }
                Ok(())
            }
            Self::TooFew { min, given } => write!(
                f,
                "takes at least {min} argument{}, but {given} {} given",
                plural(*min),
                if *given == 1 { "was" } else { "were" }
            ),
            Self::WrongKind { index, expected } => {
                write!(f, "argument {index} must be {expected}")
            }
            Self::Unsupported { given } => {
                write!(f, "isn't supported with {given} argument{}", plural(*given))
            }
        }
    }
}

// What one non-empty argument converts into.
pub trait FromFunArg: Sized {
    const EXPECTED: &'static str;
    fn from_fun_arg(arg: Range<FunArg>) -> Option<Self>;
}

// A field's ranged payload: `Range<T>`, or `Range<Option<T>>`/`Option<Range<T>>` when the
// argument may be left empty.
pub trait FromOptFunArg: Sized {
    const EXPECTED: &'static str;
    const ACCEPTS_EMPTY: bool;
    fn from_opt_fun_arg(arg: Range<Option<FunArg>>) -> Option<Self>;
}

impl<T: FromFunArg> FromOptFunArg for Range<T> {
    const EXPECTED: &'static str = T::EXPECTED;
    const ACCEPTS_EMPTY: bool = false;
    fn from_opt_fun_arg(arg: Range<Option<FunArg>>) -> Option<Self> {
        let (start, end) = (arg.start, arg.end);
        T::from_fun_arg(arg.transpose()?).map(|t| Range::new(start, t, end))
    }
}

impl<T: FromFunArg> FromOptFunArg for Range<Option<T>> {
    const EXPECTED: &'static str = T::EXPECTED;
    const ACCEPTS_EMPTY: bool = true;
    fn from_opt_fun_arg(arg: Range<Option<FunArg>>) -> Option<Self> {
        if arg.is_none() {
            return Some(arg.map(|_| None));
        }
        <Range<T>>::from_opt_fun_arg(arg).map(|r| r.map(Some))
    }
}

impl<T: FromFunArg> FromOptFunArg for Option<Range<T>> {
    const EXPECTED: &'static str = T::EXPECTED;
    const ACCEPTS_EMPTY: bool = true;
    fn from_opt_fun_arg(arg: Range<Option<FunArg>>) -> Option<Self> {
        if arg.is_none() {
            return Some(None);
        }
        <Range<T>>::from_opt_fun_arg(arg).map(Some)
    }
}

fn normal_arg(arg: Range<FunArg>) -> Option<Range<Expr>> {
    let (start, end) = (arg.start, arg.end);
    match arg.into_inner() {
        FunArg::Normal { expr, expand: None } => Some(Range::new(start, expr, end)),
        _ => None,
    }
}

// An expression that is a single operand.
fn into_ops(expr: Expr) -> Option<Range<ExprOps<()>>> {
    let GenericExpr { base, ops } = expr.0;
    if !ops.is_empty() {
        return None;
    }
    base.map(|e| match e {
        OpsExpr::Base(e) => Some(e),
        OpsExpr::Prefixed { .. } => None,
    })
    .transpose()
}

fn into_atomic(expr: Expr) -> Option<AtomicExpr> {
    match into_ops(expr)?.into_inner() {
        ExprOps::Base(base) => match *base {
            ParenedExpr::Normal(prefix) => Some(prefix.into_inner().into_inner()),
            ParenedExpr::Paren(..) => None,
        },
        ExprOps::Then { .. } => None,
    }
}

fn into_global_reference(Variable { base, subs }: Variable) -> Option<GlobalReference> {
    match base {
        AllowMacro::Plain(VariableBase::Global(global)) => Some(GlobalReference { global, subs }),
        _ => None,
    }
}

impl FromFunArg for Expr {
    const EXPECTED: &'static str = "an expression";
    fn from_fun_arg(arg: Range<FunArg>) -> Option<Self> {
        normal_arg(arg).map(Range::into_inner)
    }
}

impl FromFunArg for GetTarget {
    const EXPECTED: &'static str = "a variable or property";
    fn from_fun_arg(arg: Range<FunArg>) -> Option<Self> {
        GetTarget::try_from(into_ops(normal_arg(arg)?.into_inner())?).ok()
    }
}

impl FromFunArg for Variable {
    const EXPECTED: &'static str = "a variable";
    fn from_fun_arg(arg: Range<FunArg>) -> Option<Self> {
        match into_atomic(normal_arg(arg)?.into_inner())? {
            AtomicExpr::Variable(v) => Some(v),
            _ => None,
        }
    }
}

impl FromFunArg for GlobalReference {
    const EXPECTED: &'static str = "a global reference";
    fn from_fun_arg(arg: Range<FunArg>) -> Option<Self> {
        into_global_reference(Variable::from_fun_arg(arg)?)
    }
}

impl FromFunArg for AllowIndirection<Variable> {
    const EXPECTED: &'static str = "a variable";
    fn from_fun_arg(arg: Range<FunArg>) -> Option<Self> {
        match into_atomic(normal_arg(arg)?.into_inner())? {
            AtomicExpr::Variable(v) => Some(Self::Plain(v)),
            AtomicExpr::Indirection(ind) => Some(Self::Indirection(ind.map(Box::new))),
            _ => None,
        }
    }
}

impl FromFunArg for AllowIndirection<GlobalReference> {
    const EXPECTED: &'static str = "a global reference";
    fn from_fun_arg(arg: Range<FunArg>) -> Option<Self> {
        match <AllowIndirection<Variable>>::from_fun_arg(arg)? {
            AllowIndirection::Plain(v) => into_global_reference(v).map(Self::Plain),
            AllowIndirection::Indirection(ind) => Some(Self::Indirection(ind)),
        }
    }
}

impl FromFunArg for ExpColExp {
    const EXPECTED: &'static str = "`value:result`";
    fn from_fun_arg(arg: Range<FunArg>) -> Option<Self> {
        match arg.into_inner() {
            FunArg::ColonNormal(x) => Some(x),
            _ => None,
        }
    }
}

// One `$CASE` arm after the target: `value:result` or the `:default`.
#[derive(Grammar)]
pub enum CaseArm {
    Branch(ExpColExp),
    Default(ColExp),
}

impl FromFunArg for CaseArm {
    const EXPECTED: &'static str = "`value:result` or `:default`";
    fn from_fun_arg(arg: Range<FunArg>) -> Option<Self> {
        match arg.into_inner() {
            FunArg::ColonNormal(x) => Some(Self::Branch(x)),
            FunArg::ColonDefault(x) => Some(Self::Default(x)),
            _ => None,
        }
    }
}

impl FromFunArg for CondValue {
    const EXPECTED: &'static str = "a value or `condition:value`";
    fn from_fun_arg(arg: Range<FunArg>) -> Option<Self> {
        let (start, end) = (arg.start, arg.end);
        match arg.into_inner() {
            FunArg::ColonNormal(ExpColExp { e1, kw, e2 }) => Some(CondValue {
                cond: Some(Suffix::new(e1, kw)),
                value: e2,
            }),
            FunArg::Normal { expr, expand: None } => Some(CondValue {
                cond: None,
                value: Range::new(start, expr, end),
            }),
            _ => None,
        }
    }
}

impl FromFunArg for PositionArg {
    const EXPECTED: &'static str = "a position";
    fn from_fun_arg(arg: Range<FunArg>) -> Option<Self> {
        let (start, end) = (arg.start, arg.end);
        match arg.into_inner() {
            FunArg::Star { kw, md } => Some(Self::Rel { star: kw, md }),
            FunArg::Normal { expr, expand: None } => Some(Self::Abs(Range::new(start, expr, end))),
            _ => None,
        }
    }
}

impl FromFunArg for MthArg {
    const EXPECTED: &'static str = "an argument";
    fn from_fun_arg(arg: Range<FunArg>) -> Option<Self> {
        let (start, end) = (arg.start, arg.end);
        match arg.into_inner() {
            FunArg::ByRef { dot, target } => Some(Self::ByRef { dot, target }),
            FunArg::Normal { expr, expand } => Some(Self::Expr {
                expr: Range::new(start, expr, end),
                expand,
            }),
            _ => None,
        }
    }
}

type Comma = StringEq!(",");

// The generic arguments, consumed field by field by `FunArgField::take`.
pub struct ArgCursor {
    args: std::collections::VecDeque<(Option<Comma>, ArgOf<Option<FunArg>>)>,
    index: usize,
    // `f()` parses as one empty argument; it means no argument at all.
    given: usize,
    min: usize,
    max: Option<usize>,
}

impl ArgCursor {
    fn new(
        args: GenericFunArgs,
        min: usize,
        max: Option<usize>,
        first_accepts_empty: bool,
    ) -> Self {
        let (items, seps) = args.map(|v| (v.items, v.seps)).unwrap_or_default();
        let given = if items.len() == 1 && items[0].wrapped.is_none() && !first_accepts_empty {
            0
        } else {
            items.len()
        };
        let seps = std::iter::once(None).chain(seps.into_iter().map(|sep| Some(sep.into_inner())));
        Self {
            args: seps.zip(items).collect(),
            index: 0,
            given,
            min,
            max,
        }
    }

    // The count is checked before any argument is converted, so an extra argument is reported
    // as such rather than as an earlier one being of the wrong kind.
    fn check_count(&self) -> Result<(), FunArgError> {
        if let Some(max) = self.max
            && self.given > max
        {
            let extra = self
                .args
                .iter()
                .skip(max)
                .enumerate()
                .map(|(i, (_, item))| (max + i + 1, item.print().trim().to_string()))
                .collect();
            return Err(FunArgError::TooMany { max, extra });
        }
        if self.given < self.min {
            return Err(self.too_few());
        }
        Ok(())
    }

    fn too_few(&self) -> FunArgError {
        FunArgError::TooFew {
            min: self.min,
            given: self.given,
        }
    }

    fn pop(&mut self) -> Result<(Option<Comma>, ArgOf<Option<FunArg>>), FunArgError> {
        self.args.pop_front().ok_or_else(|| self.too_few())
    }

    fn convert<P: FromOptFunArg>(
        &mut self,
        item: ArgOf<Option<FunArg>>,
    ) -> Result<Wrap<MGap, P, MGap>, FunArgError> {
        self.index += 1;
        match item
            .map(|arg| P::from_opt_fun_arg(arg).ok_or(()))
            .transpose()
        {
            Ok(v) => Ok(v),
            Err(()) if self.given == 0 => Err(self.too_few()),
            Err(()) => Err(FunArgError::WrongKind {
                index: self.index,
                expected: P::EXPECTED,
            }),
        }
    }

    fn finish(self) -> Result<(), FunArgError> {
        match self.args.front() {
            Some((_, item)) if self.given > 0 => Err(FunArgError::TooMany {
                max: self.max.unwrap_or(self.index),
                extra: vec![(self.index + 1, item.print().trim().to_string())],
            }),
            _ => Ok(()),
        }
    }
}

// The shapes an argument field takes: the first argument, a `,`-prefixed later one, either
// optionally, or a variadic tail.
pub trait FunArgField: Sized {
    const REQUIRED: bool;
    const SLOTS: usize;
    // Whether `f()` counts as one (empty) argument for this field rather than none.
    const ACCEPTS_EMPTY: bool = false;
    fn take(cursor: &mut ArgCursor) -> Result<Self, FunArgError>;
}

impl<P: FromOptFunArg> FunArgField for Wrap<MGap, P, MGap> {
    const REQUIRED: bool = true;
    const SLOTS: usize = 1;
    const ACCEPTS_EMPTY: bool = P::ACCEPTS_EMPTY;
    fn take(cursor: &mut ArgCursor) -> Result<Self, FunArgError> {
        let (_, item) = cursor.pop()?;
        cursor.convert(item)
    }
}

impl<P: FromOptFunArg> FunArgField for Option<Wrap<MGap, P, MGap>> {
    const REQUIRED: bool = false;
    const SLOTS: usize = 1;
    const ACCEPTS_EMPTY: bool = P::ACCEPTS_EMPTY;
    fn take(cursor: &mut ArgCursor) -> Result<Self, FunArgError> {
        let Some((_, item)) = cursor.args.pop_front() else {
            return Ok(None);
        };
        if cursor.given == 0 {
            return Ok(None);
        }
        cursor.convert(item).map(Some)
    }
}

impl<P: FromOptFunArg> FunArgField for Prefix<Comma, Wrap<MGap, P, MGap>> {
    const REQUIRED: bool = true;
    const SLOTS: usize = 1;
    fn take(cursor: &mut ArgCursor) -> Result<Self, FunArgError> {
        let (sep, item) = cursor.pop()?;
        let sep = sep.ok_or_else(|| cursor.too_few())?;
        Ok(Prefix::new(sep, cursor.convert(item)?))
    }
}

impl<P: FromOptFunArg> FunArgField for Option<Prefix<Comma, Wrap<MGap, P, MGap>>> {
    const REQUIRED: bool = false;
    const SLOTS: usize = 1;
    fn take(cursor: &mut ArgCursor) -> Result<Self, FunArgError> {
        let Some((sep, item)) = cursor.args.pop_front() else {
            return Ok(None);
        };
        let sep = sep.ok_or_else(|| cursor.too_few())?;
        Ok(Some(Prefix::new(sep, cursor.convert(item)?)))
    }
}

impl<P: FromOptFunArg> FunArgField for Vec<Prefix<Comma, Wrap<MGap, P, MGap>>> {
    const REQUIRED: bool = false;
    const SLOTS: usize = 0;
    fn take(cursor: &mut ArgCursor) -> Result<Self, FunArgError> {
        let mut rest = Vec::new();
        while let Some((sep, item)) = cursor.args.pop_front() {
            let sep = sep.ok_or_else(|| cursor.too_few())?;
            rest.push(Prefix::new(sep, cursor.convert(item)?));
        }
        Ok(rest)
    }
}

// `fun_args!("$NAME" NameArgs { field: ArgOf<Expr>, later: Option<More<ArgOf<Expr>>>, ... })`:
// the arguments of one function, parsed generically and converted field by field.
macro_rules! fun_args {
    ($name:literal $ty:ident { $first:ident : $firstty:ty $(, $field:ident : $fty:ty)* $(,)? }) => {
        #[derive(GrammarTryFromOther)]
        #[grammar(name = $name)]
        pub struct $ty {
            pub $first: $firstty,
            $(pub $field: $fty,)*
            pub commas: Vec<CommaAt>,
        }

        impl GrammarFrom for $ty {
            type Source = GenericFunArgs;
            fn print_to(&self, buf: &mut String) {
                Grammar::print_to(&self.$first, buf);
                $(Grammar::print_to(&self.$field, buf);)*
            }
        }

        impl TryFrom<GenericFunArgs> for $ty {
            type Error = FunArgError;
            fn try_from(args: GenericFunArgs) -> Result<Self, FunArgError> {
                const MIN: usize = <$firstty as FunArgField>::REQUIRED as usize $(+ <$fty as FunArgField>::REQUIRED as usize)*;
                const SLOTS: usize = <$firstty as FunArgField>::SLOTS $(+ <$fty as FunArgField>::SLOTS)*;
                const VARIADIC: bool = <$firstty as FunArgField>::SLOTS == 0 $(|| <$fty as FunArgField>::SLOTS == 0)*;
                let max = if VARIADIC { None } else { Some(SLOTS) };
                let commas = arg_commas(&args);
                let mut cursor = ArgCursor::new(args, MIN, max, <$firstty as FunArgField>::ACCEPTS_EMPTY);
                cursor.check_count()?;
                let value = Self {
                    $first: FunArgField::take(&mut cursor)?,
                    $($field: FunArgField::take(&mut cursor)?,)*
                    commas,
                };
                cursor.finish()?;
                Ok(value)
            }
        }
    };
}

#[derive(Grammar)]
pub struct CondValue {
    pub cond: Option<CondPrefix>,
    pub value: Range<Expr>,
}

type CondPrefix = Suffix<Range<Expr>, MGapXMGap<StringEq!(":")>>;

fun_args!("$LIST" ListRangeCallArgs {
    list: ArgOf<Expr>,
    first: More<ArgOf<ExpColExp>>,
    rest: Vec<More<ArgOf<ExpColExp>>>,
});

/// `$LIST(list, start1:end1, ...)` -- read-only, not a valid SET target.
#[derive(Grammar)]
pub struct ListRangeCall {
    pub kw: Range<FnList>,
    pub lp: StringEq!("("),
    pub args: ListRangeCallArgs,
    pub rp: StringEq!(")"),
}

impl Deref for ListRangeCall {
    type Target = ListRangeCallArgs;
    fn deref(&self) -> &ListRangeCallArgs {
        &self.args
    }
}

fun_args!("$LIST" ListPlainCallArgs {
    list: ArgOf<Expr>,
    position: Option<More<ArgOf<PositionArg>>>,
    end: Option<More<ArgOf<PositionArg>>>,
});

/// `$LIST(list, position, end)` -- the plain form, settable via `SET`.
#[derive(Grammar)]
pub struct ListPlainCall {
    pub kw: Range<FnList>,
    pub lp: StringEq!("("),
    pub args: ListPlainCallArgs,
    pub rp: StringEq!(")"),
}

impl Deref for ListPlainCall {
    type Target = ListPlainCallArgs;
    fn deref(&self) -> &ListPlainCallArgs {
        &self.args
    }
}

fun_args!("$PIECE" PieceCallArgs {
    string: ArgOf<Expr>,
    delimiter: More<ArgOf<Expr>>,
    from: Option<More<ArgOf<PositionArg>>>,
    to: Option<More<OptArgOf<PositionArg>>>,
});

/// The settable functions, extracted into named structs so both [`FunctionCall`]
/// and [`SetTargetFun`] can hold them.
#[derive(Grammar)]
pub struct PieceCall {
    pub kw: Range<FnPiece>,
    pub lp: StringEq!("("),
    pub args: PieceCallArgs,
    pub rp: StringEq!(")"),
}

impl Deref for PieceCall {
    type Target = PieceCallArgs;
    fn deref(&self) -> &PieceCallArgs {
        &self.args
    }
}

// `$MV(record, field, value, subvalue, ...)`: undocumented (not found in InterSystems docs),
// but used throughout the corpus for MultiValue-BASIC field/value/subvalue addressing, both as
// a read and, like `$PIECE`, as a `SET` target. Modeled with an open-ended `rest` since real
// usage runs from 1 up to 5 trailing positional args and its exact upper bound is unconfirmed.
fun_args!("$MV" MVCallArgs {
    record: ArgOf<Expr>,
    rest: Vec<More<ArgOf<Expr>>>,
});

#[derive(Grammar)]
pub struct MVCall {
    pub kw: Range<FnMV>,
    pub lp: StringEq!("("),
    pub args: MVCallArgs,
    pub rp: StringEq!(")"),
}

impl Deref for MVCall {
    type Target = MVCallArgs;
    fn deref(&self) -> &MVCallArgs {
        &self.args
    }
}

#[derive(Grammar)]
pub enum PositionArg {
    Rel {
        star: StringEq!("*"),
        md: Option<StarMod>,
    },
    Abs(Range<Expr>),
}

fun_args!("$EXTRACT" ExtractCallArgs {
    string: ArgOf<Expr>,
    from: Option<More<ArgOf<PositionArg>>>,
    to: Option<More<OptArgOf<PositionArg>>>,
});

#[derive(Grammar)]
pub struct ExtractCall {
    pub kw: Range<FnExtract>,
    pub lp: StringEq!("("),
    pub args: ExtractCallArgs,
    pub rp: StringEq!(")"),
}

impl Deref for ExtractCall {
    type Target = ExtractCallArgs;
    fn deref(&self) -> &ExtractCallArgs {
        &self.args
    }
}

fun_args!("$LISTGET" ListGetCallArgs {
    list: ArgOf<Expr>,
    position: Option<More<OptArgOf<PositionArg>>>,
    default: Option<More<ArgOf<Expr>>>,
});

#[derive(Grammar)]
pub struct ListGetCall {
    pub kw: Range<FnListGet>,
    pub lp: StringEq!("("),
    pub args: ListGetCallArgs,
    pub rp: StringEq!(")"),
}

impl Deref for ListGetCall {
    type Target = ListGetCallArgs;
    fn deref(&self) -> &ListGetCallArgs {
        &self.args
    }
}

fun_args!("$BIT" BitCallArgs {
    bits: ArgOf<Expr>,
    position: More<ArgOf<Expr>>,
});

#[derive(Grammar)]
pub struct BitCall {
    pub kw: Range<FnBit>,
    pub lp: StringEq!("("),
    pub args: BitCallArgs,
    pub rp: StringEq!(")"),
}

impl Deref for BitCall {
    type Target = BitCallArgs;
    fn deref(&self) -> &BitCallArgs {
        &self.args
    }
}

fun_args!("$VECTOR" VectorCallArgs {
    vector: ArgOf<Expr>,
    position: More<ArgOf<Expr>>,
    third: Option<More<ArgOf<Expr>>>,
});

#[derive(Grammar)]
pub struct VectorCall {
    pub kw: Range<FnVector>,
    pub lp: StringEq!("("),
    pub args: VectorCallArgs,
    pub rp: StringEq!(")"),
}

impl Deref for VectorCall {
    type Target = VectorCallArgs;
    fn deref(&self) -> &VectorCallArgs {
        &self.args
    }
}

fun_args!("$WEXTRACT" WExtractCallArgs {
    string: ArgOf<Expr>,
    from: Option<More<ArgOf<PositionArg>>>,
    to: Option<More<OptArgOf<PositionArg>>>,
});

#[derive(Grammar)]
pub struct WExtractCall {
    pub kw: Range<FnWExtract>,
    pub lp: StringEq!("("),
    pub args: WExtractCallArgs,
    pub rp: StringEq!(")"),
}

impl Deref for WExtractCall {
    type Target = WExtractCallArgs;
    fn deref(&self) -> &WExtractCallArgs {
        &self.args
    }
}

fun_args!("$PROPERTY" PropertyCallArgs {
    instance: ArgOf<Option<Expr>>,
    property: More<ArgOf<Expr>>,
    indexes: Vec<More<ArgOf<Expr>>>,
});

#[derive(Grammar)]
pub struct PropertyCall {
    pub kw: Either<Range<FnProperty>, Range<FnZObjProperty>>,
    pub lp: StringEq!("("),
    pub args: PropertyCallArgs,
    pub rp: StringEq!(")"),
}

impl Deref for PropertyCall {
    type Target = PropertyCallArgs;
    fn deref(&self) -> &PropertyCallArgs {
        &self.args
    }
}

fun_args!("$SEQUENCE" SequenceCallArgs {
    gvar: ArgOf<AllowIndirection<Variable>>,
});

#[derive(Grammar)]
pub struct SequenceCall {
    pub kw: Range<FnSequence>,
    pub lp: StringEq!("("),
    pub args: SequenceCallArgs,
    pub rp: StringEq!(")"),
}

impl Deref for SequenceCall {
    type Target = SequenceCallArgs;
    fn deref(&self) -> &SequenceCallArgs {
        &self.args
    }
}

fun_args!("$LISTBUILD" ListBuildArgs {
    first: ArgOf<Option<Expr>>,
    rest: Vec<More<ArgOf<Option<Expr>>>>,
});

#[derive(Grammar)]
pub struct ListBuild {
    pub kw: Range<FnListBuild>,
    pub lp: StringEq!("("),
    pub args: ListBuildArgs,
    pub rp: StringEq!(")"),
}

impl Deref for ListBuild {
    type Target = ListBuildArgs;
    fn deref(&self) -> &ListBuildArgs {
        &self.args
    }
}

#[derive(Grammar)]
pub struct ListBuildForSet {
    pub kw: Range<FnListBuild>,
    pub lp: StringEq!("("),
    pub first: ArgOf<Option<Variable>>,
    pub rest: Vec<More<ArgOf<Option<Variable>>>>,
    pub rp: StringEq!(")"),
}

// `SET $LISTGET(var1,var2,...)=list`: any number of variables, a placeholder comma for one to
// skip -- see `SetTargetSource`.
#[derive(Grammar)]
pub struct ListGetForSet {
    pub kw: Range<FnListGet>,
    pub lp: StringEq!("("),
    pub first: ArgOf<Option<Variable>>,
    pub rest: Vec<More<ArgOf<Option<Variable>>>>,
    pub rp: StringEq!(")"),
}

fun_args!("$LENGTH" LengthArgs {
    expression: ArgOf<Expr>,
    delimitor: Option<More<ArgOf<Expr>>>,
});

fun_args!("$DATA" DataArgs {
    variable: ArgOf<GetTarget>,
    target: Option<More<ArgOf<Variable>>>,
});

fun_args!("$ORDER" OrderArgs {
    variable: ArgOf<GetTarget>,
    direction: Option<More<ArgOf<Expr>>>,
    target: Option<More<ArgOf<Variable>>>,
});

fun_args!("$GET" GetArgs {
    variable: ArgOf<GetTarget>,
    default: Option<More<ArgOf<Expr>>>,
});

fun_args!("$CASE" CaseArgs {
    target: ArgOf<Expr>,
    arms: Vec<More<ArgOf<CaseArm>>>,
});

fun_args!("$BITCOUNT" BitCountArgs {
    bits: ArgOf<Expr>,
    value: Option<More<ArgOf<Expr>>>,
});

fun_args!("$BITFIND" BitFindArgs {
    bits: ArgOf<Expr>,
    value: More<ArgOf<Expr>>,
    position: Option<More<ArgOf<Expr>>>,
    direction: Option<More<ArgOf<Expr>>>,
});

fun_args!("$CHANGE" ChangeArgs {
    string: ArgOf<Expr>,
    target: More<ArgOf<Expr>>,
    replacement: More<ArgOf<Expr>>,
    occurrences: Option<More<ArgOf<Expr>>>,
    begin: Option<More<ArgOf<Expr>>>,
});

fun_args!("$CHAR" CharArgs {
    first: ArgOf<Expr>,
    rest: Vec<More<ArgOf<Expr>>>,
});

fun_args!("$CLASSNAME" ClassNameArgs {
    oref: Option<ArgOf<Expr>>,
});

fun_args!("$DECIMAL" DecimalArgs {
    num: ArgOf<Expr>,
    digits: Option<More<ArgOf<Expr>>>,
});

fun_args!("$DOUBLE" DoubleArgs {
    num: ArgOf<Expr>,
});

fun_args!("$FNUMBER" FNumberArgs {
    num: ArgOf<Expr>,
    format: Option<More<OptArgOf<Expr>>>,
    decimal: Option<More<ArgOf<Expr>>>,
});

fun_args!("$ISOBJECT" IsObjectArgs {
    expr: ArgOf<Expr>,
});

fun_args!("$INCREMENT" IncrementArgs {
    counter: ArgOf<GetTarget>,
    amount: Option<More<ArgOf<Expr>>>,
});

fun_args!("$LISTDATA" ListDataArgs {
    list: ArgOf<Expr>,
    position: Option<More<ArgOf<Expr>>>,
    var: Option<More<ArgOf<Variable>>>,
});

fun_args!("$LISTNEXT" ListNextArgs {
    list: ArgOf<Expr>,
    ptr: More<ArgOf<Variable>>,
    value: More<ArgOf<Variable>>,
});

fun_args!("$LISTUPDATE" ListUpdateArgs {
    list: ArgOf<Expr>,
    position: More<ArgOf<Expr>>,
    value: More<ArgOf<CondValue>>,
    rest: Vec<More<ArgOf<CondValue>>>,
});

// An optional argument may be left out of the middle: `$LOCATE(s, re,, start)`.
fun_args!("$LOCATE" LocateArgs {
    string: ArgOf<Expr>,
    regexp: More<ArgOf<Expr>>,
    start: Option<More<OptArgOf<Expr>>>,
    end: Option<More<OptArgOf<Variable>>>,
    val: Option<More<OptArgOf<Variable>>>,
});

fun_args!("$METHOD" MethodArgs {
    instance: ArgOf<Option<Expr>>,
    method: More<ArgOf<Expr>>,
    args: Vec<More<ArgOf<Option<MthArg>>>>,
});

fun_args!("$NAME" NameArgs {
    variable: ArgOf<GetTarget>,
    integer: Option<More<ArgOf<Expr>>>,
});

fun_args!("$PARAMETER" ParameterArgs {
    class: ArgOf<Option<Expr>>,
    parameter: More<ArgOf<Expr>>,
});

fun_args!("$PREFETCHOFF" PrefetchOffArgs {
    first: Option<ArgOf<AllowIndirection<GlobalReference>>>,
    second: Option<More<ArgOf<AllowIndirection<GlobalReference>>>>,
});

fun_args!("$PREFETCHON" PrefetchOnArgs {
    gref: ArgOf<AllowIndirection<GlobalReference>>,
    gref2: Option<More<ArgOf<AllowIndirection<GlobalReference>>>>,
});

fun_args!("$QUERY" QueryArgs {
    reference: ArgOf<GetTarget>,
    direction: Option<More<ArgOf<Expr>>>,
    target: Option<More<ArgOf<Variable>>>,
});

fun_args!("$SELECT" SelectArgs {
    first: ArgOf<ExpColExp>,
    rest: Vec<More<ArgOf<ExpColExp>>>,
});

fun_args!("$SORTBEGIN" SortBeginArgs {
    set_global: ArgOf<AllowIndirection<GlobalReference>>,
});

fun_args!("$SORTEND" SortEndArgs {
    first: Option<ArgOf<Option<AllowIndirection<GlobalReference>>>>,
    dosort: Option<More<ArgOf<Expr>>>,
});

fun_args!("$VECTORDEFINED" VectorDefinedArgs {
    vector: ArgOf<Expr>,
    position: More<ArgOf<Expr>>,
    value: Option<More<ArgOf<Variable>>>,
});

fun_args!("$VECTOROP" VectorOpArgs {
    op: ArgOf<Expr>,
    first: More<ArgOf<Expr>>,
    rest: Vec<More<ArgOf<Expr>>>,
});

fun_args!("$WCHAR" WCharArgs {
    first: ArgOf<Expr>,
    rest: Vec<More<ArgOf<Expr>>>,
});

// A parameter may be passed by reference, `.var`, as to a method.
fun_args!("$XECUTE" XecuteArgs {
    code: ArgOf<Expr>,
    params: Vec<More<ArgOf<MthArg>>>,
});

fun_args!("$CLASSMETHOD" ClassMethodArgs {
    classname: ArgOf<Option<Expr>>,
    method: More<ArgOf<Expr>>,
    args: Vec<More<ArgOf<Option<MthArg>>>>,
});

fun_args!("$COMPILE" CompileArgs {
    source: ArgOf<AllowIndirection<Variable>>,
    qspec: More<ArgOf<Expr>>,
    errors: More<ArgOf<Option<Variable>>>,
    object: Option<More<ArgOf<Option<AllowIndirection<Variable>>>>>,
    arg5: Option<More<ArgOf<Option<Expr>>>>,
    arg6: Option<More<ArgOf<Option<Expr>>>>,
    rname: Option<More<ArgOf<Option<Expr>>>>,
});

fun_args!("$ZPREVIOUS" ZPArgs {
    variable: ArgOf<AllowIndirection<Variable>>,
});

fun_args!("$ZORDER" ZOrderArgs {
    variable: ArgOf<AllowIndirection<Variable>>,
});

fun_args!("$NEXT" NextArgs {
    variable: ArgOf<AllowIndirection<Variable>>,
});

fun_args!("$ZOBJREF" ZObjRefArgs {
    oref: ArgOf<Expr>,
});

fun_args!("$ZSTRIP" ZStripArgs {
    string: ArgOf<Expr>,
    action: More<ArgOf<Expr>>,
    remchar: Option<More<ArgOf<Option<Expr>>>>,
    keepchar: Option<More<ArgOf<Option<Expr>>>>,
});

fun_args!("$ZDATETIME/$ZDATETIMEH" ZDateTimeArgs {
    value: ArgOf<Expr>,
    rest: Vec<More<ArgOf<Option<Expr>>>>,
});

fun_args!("$ZDATE/$ZDATEH" ZDateArgs {
    value: ArgOf<Expr>,
    rest: Vec<More<ArgOf<Option<Expr>>>>,
});

fun_args!("$ZTIME/$ZTIMEH" ZTimeArgs {
    value: ArgOf<Expr>,
    rest: Vec<More<ArgOf<Option<Expr>>>>,
});

#[derive(Grammar)]
pub enum FunctionCall {
    DText(DText),
    Length {
        kw: Range<FnLength>,
        lp: StringEq!("("),
        args: LengthArgs,
        rp: StringEq!(")"),
    },
    Data {
        kw: Range<FnData>,
        lp: StringEq!("("),
        args: DataArgs,
        rp: StringEq!(")"),
    },
    Order {
        kw: Range<FnOrder>,
        lp: StringEq!("("),
        args: OrderArgs,
        rp: StringEq!(")"),
    },
    Get {
        kw: Range<FnGet>,
        lp: StringEq!("("),
        args: GetArgs,
        rp: StringEq!(")"),
    },
    Case {
        kw: Range<FnCase>,
        lp: StringEq!("("),
        args: CaseArgs,
        rp: StringEq!(")"),
    },
    Bit(BitCall),
    BitCount {
        kw: Range<FnBitCount>,
        lp: StringEq!("("),
        args: BitCountArgs,
        rp: StringEq!(")"),
    },
    BitFind {
        kw: Range<FnBitFind>,
        lp: StringEq!("("),
        args: BitFindArgs,
        rp: StringEq!(")"),
    },
    BitLogic {
        kw: Range<FnBitLogic>,
        lp: StringEq!("("),
        expr: ArgOf<BitLogicExpr>,
        length: Option<More<ArgOf<Expr>>>,
        rp: StringEq!(")"),
    },
    Change {
        kw: Range<FnChange>,
        lp: StringEq!("("),
        args: ChangeArgs,
        rp: StringEq!(")"),
    },
    Char {
        kw: Range<FnChar>,
        lp: StringEq!("("),
        args: CharArgs,
        rp: StringEq!(")"),
    },
    ClassName {
        kw: Either<Range<FnClassName>, Range<FnZObjClass>>,
        lp: StringEq!("("),
        args: ClassNameArgs,
        rp: StringEq!(")"),
    },
    Decimal {
        kw: Range<FnDecimal>,
        lp: StringEq!("("),
        args: DecimalArgs,
        rp: StringEq!(")"),
    },
    Double {
        kw: Range<FnDouble>,
        lp: StringEq!("("),
        args: DoubleArgs,
        rp: StringEq!(")"),
    },
    Extract(ExtractCall),
    FNumber {
        kw: Range<FnFNumber>,
        lp: StringEq!("("),
        args: FNumberArgs,
        rp: StringEq!(")"),
    },
    IsObject {
        kw: Range<FnIsObject>,
        lp: StringEq!("("),
        args: IsObjectArgs,
        rp: StringEq!(")"),
    },
    // The "I" case is dangerous
    Increment {
        kw: Either<Range<FnIncrement>, Range<FnZIncrement>>,
        lp: StringEq!("("),
        args: IncrementArgs,
        rp: StringEq!(")"),
    },
    ListPlain(ListPlainCall),
    ListRange(ListRangeCall),
    ListBuild(ListBuild),
    ListData {
        kw: Range<FnListData>,
        lp: StringEq!("("),
        args: ListDataArgs,
        rp: StringEq!(")"),
    },
    ListGet(ListGetCall),
    ListNext {
        kw: Range<FnListNext>,
        lp: StringEq!("("),
        args: ListNextArgs,
        rp: StringEq!(")"),
    },
    ListUpdate {
        kw: Range<FnListUpdate>,
        lp: StringEq!("("),
        args: ListUpdateArgs,
        rp: StringEq!(")"),
    },
    Locate {
        kw: Range<FnLocate>,
        lp: StringEq!("("),
        args: LocateArgs,
        rp: StringEq!(")"),
    },
    MV(MVCall),
    Method {
        kw: Either<Range<FnMethod>, Range<FnZObjMethod>>,
        lp: StringEq!("("),
        args: MethodArgs,
        rp: StringEq!(")"),
    },
    Name {
        kw: Range<FnName>,
        lp: StringEq!("("),
        args: NameArgs,
        rp: StringEq!(")"),
    },
    Parameter {
        kw: Range<FnParameter>,
        lp: StringEq!("("),
        args: ParameterArgs,
        rp: StringEq!(")"),
    },
    Piece(PieceCall),
    PrefetchOff {
        kw: Range<FnPrefetchOff>,
        lp: StringEq!("("),
        args: PrefetchOffArgs,
        rp: StringEq!(")"),
    },
    PrefetchOn {
        kw: Range<FnPrefetchOn>,
        lp: StringEq!("("),
        args: PrefetchOnArgs,
        rp: StringEq!(")"),
    },
    Property(PropertyCall),
    Query {
        kw: Range<FnQuery>,
        lp: StringEq!("("),
        args: QueryArgs,
        rp: StringEq!(")"),
    },
    Select {
        kw: Range<FnSelect>,
        lp: StringEq!("("),
        args: SelectArgs,
        rp: StringEq!(")"),
    },
    Sequence(SequenceCall),
    SortBegin {
        kw: Range<FnSortBegin>,
        lp: StringEq!("("),
        args: SortBeginArgs,
        rp: StringEq!(")"),
    },
    SortEnd {
        kw: Range<FnSortEnd>,
        lp: StringEq!("("),
        args: SortEndArgs,
        rp: StringEq!(")"),
    },
    Vector(VectorCall),
    VectorDefined {
        kw: Range<FnVectorDefined>,
        lp: StringEq!("("),
        args: VectorDefinedArgs,
        rp: StringEq!(")"),
    },
    VectorOp {
        kw: Range<FnVectorOp>,
        lp: StringEq!("("),
        args: VectorOpArgs,
        rp: StringEq!(")"),
    },
    WChar {
        kw: Range<FnWChar>,
        lp: StringEq!("("),
        args: WCharArgs,
        rp: StringEq!(")"),
    },
    WExtract(WExtractCall),
    Xecute {
        kw: Range<FnXecute>,
        lp: StringEq!("("),
        args: XecuteArgs,
        rp: StringEq!(")"),
    },
    ClassMethod {
        kw: Either<Range<FnClassMethod>, Range<FnZObjClassMethod>>,
        lp: StringEq!("("),
        args: ClassMethodArgs,
        rp: StringEq!(")"),
    },
    Compile {
        kw: Range<FnCompile>,
        lp: StringEq!("("),
        args: CompileArgs,
        rp: StringEq!(")"),
    },
    // Legacy `$ZPREVIOUS(glvn)`/`$ZP`: the last subscript at that level, a one-argument reverse
    // `$ORDER`. Modeled on its own rather than folded into `Order`.
    ZP {
        kw: Range<FnZPrevious>,
        lp: StringEq!("("),
        args: ZPArgs,
        rp: StringEq!(")"),
    },
    // Legacy `$ZORDER(glvn)`/`$ZO` and `$NEXT(glvn)`/`$N`: the next node/subscript, like `$ORDER`.
    ZOrder {
        kw: Range<FnZOrder>,
        lp: StringEq!("("),
        args: ZOrderArgs,
        rp: StringEq!(")"),
    },
    Next {
        kw: Range<FnNext>,
        lp: StringEq!("("),
        args: NextArgs,
        rp: StringEq!(")"),
    },
    // Internal: the object's underlying storage handle, used by system collection classes.
    ZObjRef {
        kw: Range<FnZObjRef>,
        lp: StringEq!("("),
        args: ZObjRefArgs,
        rp: StringEq!(")"),
    },
    ZStrip {
        kw: Range<FnZStrip>,
        lp: StringEq!("("),
        args: ZStripArgs,
        rp: StringEq!(")"),
    },
    ZDateTime {
        // The `H` forms first: `ZDATETIME` and `ZDT` are prefixes of them.
        kw: Either<Range<FnZDateTimeH>, Range<FnZDateTime>>,
        lp: StringEq!("("),
        args: ZDateTimeArgs,
        rp: StringEq!(")"),
    },
    ZDate {
        kw: Either<Range<FnZDateH>, Range<FnZDate>>,
        lp: StringEq!("("),
        args: ZDateArgs,
        rp: StringEq!(")"),
    },
    ZTime {
        kw: Either<Range<FnZTimeH>, Range<FnZTime>>,
        lp: StringEq!("("),
        args: ZTimeArgs,
        rp: StringEq!(")"),
    },
    // `$ZF(-n, …)` callouts, per selector. `-100` precedes `-1` because
    // `StringEq` matches without a word boundary, so `-1` prefixes `-100`.
    ZFm100 {
        kw: Range<FnZF>,
        lp: StringEq!("("),
        selector: Wrap<SGap, StringEq!("-100"), SGap>,
        flags: More<ArgOf<Expr>>,
        program: More<ArgOf<Expr>>,
        args: Vec<More<ArgOf<Option<MthArg>>>>,
        rp: StringEq!(")"),
    },
    ZFm1 {
        kw: Range<FnZF>,
        lp: StringEq!("("),
        selector: Wrap<SGap, StringEq!("-1"), SGap>,
        program: Option<More<ArgOf<Expr>>>,
        // Undocumented, but IRIS takes them.
        extra: Vec<More<ArgOf<Expr>>>,
        rp: StringEq!(")"),
    },
    ZFm2 {
        kw: Range<FnZF>,
        lp: StringEq!("("),
        selector: Wrap<SGap, StringEq!("-2"), SGap>,
        program: Option<More<ArgOf<Expr>>>,
        rp: StringEq!(")"),
    },
    ZFm3 {
        kw: Range<FnZF>,
        lp: StringEq!("("),
        selector: Wrap<SGap, StringEq!("-3"), SGap>,
        dll: More<ArgOf<Expr>>,
        args: Vec<More<ArgOf<Option<MthArg>>>>,
        rp: StringEq!(")"),
    },
    ZFm4 {
        kw: Range<FnZF>,
        lp: StringEq!("("),
        selector: Wrap<SGap, StringEq!("-4"), SGap>,
        n: More<ArgOf<Expr>>,
        // `$ZF(-4,2)` unloads every DLL: no third argument.
        third: Option<More<ArgOf<Expr>>>,
        fourth: Option<More<ArgOf<Expr>>>,
        rp: StringEq!(")"),
    },
    ZFm5 {
        kw: Range<FnZF>,
        lp: StringEq!("("),
        selector: Wrap<SGap, StringEq!("-5"), SGap>,
        dll_id: More<ArgOf<Expr>>,
        func_id: More<ArgOf<Expr>>,
        args: Vec<More<ArgOf<Option<MthArg>>>>,
        rp: StringEq!(")"),
    },
    ZFm6 {
        kw: Range<FnZF>,
        lp: StringEq!("("),
        selector: Wrap<SGap, StringEq!("-6"), SGap>,
        dll_index: More<ArgOf<Expr>>,
        args: Vec<More<ArgOf<Option<MthArg>>>>,
        rp: StringEq!(")"),
    },
    ZU {
        kw: Range<FnZUtil>,
        args: FunArgList,
    },
    // Every function whose arguments are all plain expressions, parsed with any number of them
    // so the checker can name the extra ones (see `SIMPLE_FUNCTIONS`).
    Simple(SimpleFunctionCall),
    // A hand-written shape above (`$TEXT`, `$BITLOGIC`, `$ZF`) that didn't match: always fails
    // the conversion, with the argument count.
    Generic(GenericFunctionCall),
    // Anything else spelled like a function: accepted here, reported by the checker.
    Unknown(UnknownFunctionCall),
}

impl FunctionCall {
    // The commas between the arguments of a call whose arguments are typed; a `Simple` or
    // `Unknown` call keeps its list, whose commas `commas_of` reads.
    pub fn arg_commas(&self) -> Vec<CommaAt> {
        match self {
            FunctionCall::Length { args, .. } => args.commas.clone(),
            FunctionCall::Data { args, .. } => args.commas.clone(),
            FunctionCall::Order { args, .. } => args.commas.clone(),
            FunctionCall::Get { args, .. } => args.commas.clone(),
            FunctionCall::Case { args, .. } => args.commas.clone(),
            FunctionCall::Bit(call) => call.args.commas.clone(),
            FunctionCall::BitCount { args, .. } => args.commas.clone(),
            FunctionCall::BitFind { args, .. } => args.commas.clone(),
            FunctionCall::Change { args, .. } => args.commas.clone(),
            FunctionCall::Char { args, .. } => args.commas.clone(),
            FunctionCall::ClassName { args, .. } => args.commas.clone(),
            FunctionCall::Decimal { args, .. } => args.commas.clone(),
            FunctionCall::Double { args, .. } => args.commas.clone(),
            FunctionCall::Extract(call) => call.args.commas.clone(),
            FunctionCall::FNumber { args, .. } => args.commas.clone(),
            FunctionCall::IsObject { args, .. } => args.commas.clone(),
            FunctionCall::Increment { args, .. } => args.commas.clone(),
            FunctionCall::ListPlain(call) => call.args.commas.clone(),
            FunctionCall::ListRange(call) => call.args.commas.clone(),
            FunctionCall::ListBuild(call) => call.args.commas.clone(),
            FunctionCall::ListData { args, .. } => args.commas.clone(),
            FunctionCall::ListGet(call) => call.args.commas.clone(),
            FunctionCall::ListNext { args, .. } => args.commas.clone(),
            FunctionCall::ListUpdate { args, .. } => args.commas.clone(),
            FunctionCall::Locate { args, .. } => args.commas.clone(),
            FunctionCall::MV(call) => call.args.commas.clone(),
            FunctionCall::Method { args, .. } => args.commas.clone(),
            FunctionCall::Name { args, .. } => args.commas.clone(),
            FunctionCall::Parameter { args, .. } => args.commas.clone(),
            FunctionCall::Piece(call) => call.args.commas.clone(),
            FunctionCall::PrefetchOff { args, .. } => args.commas.clone(),
            FunctionCall::PrefetchOn { args, .. } => args.commas.clone(),
            FunctionCall::Property(call) => call.args.commas.clone(),
            FunctionCall::Query { args, .. } => args.commas.clone(),
            FunctionCall::Select { args, .. } => args.commas.clone(),
            FunctionCall::Sequence(call) => call.args.commas.clone(),
            FunctionCall::SortBegin { args, .. } => args.commas.clone(),
            FunctionCall::SortEnd { args, .. } => args.commas.clone(),
            FunctionCall::Vector(call) => call.args.commas.clone(),
            FunctionCall::VectorDefined { args, .. } => args.commas.clone(),
            FunctionCall::VectorOp { args, .. } => args.commas.clone(),
            FunctionCall::WChar { args, .. } => args.commas.clone(),
            FunctionCall::WExtract(call) => call.args.commas.clone(),
            FunctionCall::Xecute { args, .. } => args.commas.clone(),
            FunctionCall::ClassMethod { args, .. } => args.commas.clone(),
            FunctionCall::Compile { args, .. } => args.commas.clone(),
            FunctionCall::ZP { args, .. } => args.commas.clone(),
            FunctionCall::ZOrder { args, .. } => args.commas.clone(),
            FunctionCall::Next { args, .. } => args.commas.clone(),
            FunctionCall::ZObjRef { args, .. } => args.commas.clone(),
            FunctionCall::ZStrip { args, .. } => args.commas.clone(),
            FunctionCall::ZDateTime { args, .. } => args.commas.clone(),
            FunctionCall::ZDate { args, .. } => args.commas.clone(),
            FunctionCall::ZTime { args, .. } => args.commas.clone(),
            FunctionCall::ZU { args, .. } => commas_of(&args.wrapped.wrapped),
            // `$BITLOGIC` and `$ZF` parse their arguments in place, commas hidden in them.
            FunctionCall::BitLogic { .. }
            | FunctionCall::ZFm100 { .. }
            | FunctionCall::ZFm1 { .. }
            | FunctionCall::ZFm2 { .. }
            | FunctionCall::ZFm3 { .. }
            | FunctionCall::ZFm4 { .. }
            | FunctionCall::ZFm5 { .. }
            | FunctionCall::ZFm6 { .. }
            | FunctionCall::DText(_)
            | FunctionCall::Simple(_)
            | FunctionCall::Generic(_)
            | FunctionCall::Unknown(_) => vec![],
        }
    }

    // The function's name as written; `None` for a call that names no known function.
    pub fn name(&self) -> Option<KeywordUse> {
        Some(match self {
            FunctionCall::DText(call) => call.kw.spelled(),
            FunctionCall::Length { kw, .. } => kw.spelled(),
            FunctionCall::Data { kw, .. } => kw.spelled(),
            FunctionCall::Order { kw, .. } => kw.spelled(),
            FunctionCall::Get { kw, .. } => kw.spelled(),
            FunctionCall::Case { kw, .. } => kw.spelled(),
            FunctionCall::Bit(call) => call.kw.spelled(),
            FunctionCall::BitCount { kw, .. } => kw.spelled(),
            FunctionCall::BitFind { kw, .. } => kw.spelled(),
            FunctionCall::BitLogic { kw, .. } => kw.spelled(),
            FunctionCall::Change { kw, .. } => kw.spelled(),
            FunctionCall::Char { kw, .. } => kw.spelled(),
            FunctionCall::ClassName { kw, .. } => kw.spelled(),
            FunctionCall::Decimal { kw, .. } => kw.spelled(),
            FunctionCall::Double { kw, .. } => kw.spelled(),
            FunctionCall::Extract(call) => call.kw.spelled(),
            FunctionCall::FNumber { kw, .. } => kw.spelled(),
            FunctionCall::IsObject { kw, .. } => kw.spelled(),
            FunctionCall::Increment { kw, .. } => kw.spelled(),
            FunctionCall::ListPlain(call) => call.kw.spelled(),
            FunctionCall::ListRange(call) => call.kw.spelled(),
            FunctionCall::ListBuild(call) => call.kw.spelled(),
            FunctionCall::ListData { kw, .. } => kw.spelled(),
            FunctionCall::ListGet(call) => call.kw.spelled(),
            FunctionCall::ListNext { kw, .. } => kw.spelled(),
            FunctionCall::ListUpdate { kw, .. } => kw.spelled(),
            FunctionCall::Locate { kw, .. } => kw.spelled(),
            FunctionCall::MV(call) => call.kw.spelled(),
            FunctionCall::Method { kw, .. } => kw.spelled(),
            FunctionCall::Name { kw, .. } => kw.spelled(),
            FunctionCall::Parameter { kw, .. } => kw.spelled(),
            FunctionCall::Piece(call) => call.kw.spelled(),
            FunctionCall::PrefetchOff { kw, .. } => kw.spelled(),
            FunctionCall::PrefetchOn { kw, .. } => kw.spelled(),
            FunctionCall::Property(call) => call.kw.spelled(),
            FunctionCall::Query { kw, .. } => kw.spelled(),
            FunctionCall::Select { kw, .. } => kw.spelled(),
            FunctionCall::Sequence(call) => call.kw.spelled(),
            FunctionCall::SortBegin { kw, .. } => kw.spelled(),
            FunctionCall::SortEnd { kw, .. } => kw.spelled(),
            FunctionCall::Vector(call) => call.kw.spelled(),
            FunctionCall::VectorDefined { kw, .. } => kw.spelled(),
            FunctionCall::VectorOp { kw, .. } => kw.spelled(),
            FunctionCall::WChar { kw, .. } => kw.spelled(),
            FunctionCall::WExtract(call) => call.kw.spelled(),
            FunctionCall::Xecute { kw, .. } => kw.spelled(),
            FunctionCall::ClassMethod { kw, .. } => kw.spelled(),
            FunctionCall::Compile { kw, .. } => kw.spelled(),
            FunctionCall::ZP { kw, .. } => kw.spelled(),
            FunctionCall::ZOrder { kw, .. } => kw.spelled(),
            FunctionCall::Next { kw, .. } => kw.spelled(),
            FunctionCall::ZObjRef { kw, .. } => kw.spelled(),
            FunctionCall::ZStrip { kw, .. } => kw.spelled(),
            FunctionCall::ZDateTime { kw, .. } => kw.spelled(),
            FunctionCall::ZDate { kw, .. } => kw.spelled(),
            FunctionCall::ZTime { kw, .. } => kw.spelled(),
            FunctionCall::ZFm100 { kw, .. } => kw.spelled(),
            FunctionCall::ZFm1 { kw, .. } => kw.spelled(),
            FunctionCall::ZFm2 { kw, .. } => kw.spelled(),
            FunctionCall::ZFm3 { kw, .. } => kw.spelled(),
            FunctionCall::ZFm4 { kw, .. } => kw.spelled(),
            FunctionCall::ZFm5 { kw, .. } => kw.spelled(),
            FunctionCall::ZFm6 { kw, .. } => kw.spelled(),
            FunctionCall::ZU { kw, .. } => kw.spelled(),
            FunctionCall::Simple(call) => call.spelled(),
            FunctionCall::Generic(_) | FunctionCall::Unknown(_) => return None,
        })
    }
}

impl SetTargetFun {
    // An undocumented function has no spelling to hold it to.
    pub fn name(&self) -> Option<KeywordUse> {
        Some(match self {
            SetTargetFun::Piece(call) => call.kw.spelled(),
            SetTargetFun::Extract(call) => call.kw.spelled(),
            SetTargetFun::List(call) => call.kw.spelled(),
            SetTargetFun::ListGet(call) => call.kw.spelled(),
            SetTargetFun::ListBuild(call) => call.kw.spelled(),
            SetTargetFun::Bit(call) => call.kw.spelled(),
            SetTargetFun::Vector(call) => call.kw.spelled(),
            SetTargetFun::WExtract(call) => call.kw.spelled(),
            SetTargetFun::Property(call) => call.kw.spelled(),
            SetTargetFun::Sequence(call) => call.kw.spelled(),
            SetTargetFun::MV(call) => call.kw.spelled(),
            SetTargetFun::Simple(call) => call.spelled(),
            SetTargetFun::Undocumented(_) => return None,
        })
    }
}

pub struct SimpleFunction {
    pub names: &'static [&'static str],
    pub min: usize,
    pub max: usize,
    pub ret: &'static str,
}

// Functions whose arguments are all plain expressions: name (with abbreviations), how many
// arguments they take, and what they return.
pub const SIMPLE_FUNCTIONS: &[SimpleFunction] = &[
    SimpleFunction {
        names: &["ZArcCos"],
        min: 1,
        max: 1,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["ZArcSin"],
        min: 1,
        max: 1,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["ZArcTan"],
        min: 1,
        max: 1,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["ZCos"],
        min: 1,
        max: 1,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["ZCot"],
        min: 1,
        max: 1,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["ZCsc"],
        min: 1,
        max: 1,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["ZSec"],
        min: 1,
        max: 1,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["ZSin"],
        min: 1,
        max: 1,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["ZTan"],
        min: 1,
        max: 1,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["ZAbs"],
        min: 1,
        max: 1,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["ZExp"],
        min: 1,
        max: 1,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["ZLn"],
        min: 1,
        max: 1,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["ZLog"],
        min: 1,
        max: 1,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["ZSqr"],
        min: 1,
        max: 1,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["ZDAscii", "ZDA"],
        min: 1,
        max: 2,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["ZLAscii", "ZLA"],
        min: 1,
        max: 2,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["ZQAscii", "ZQA"],
        min: 1,
        max: 2,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["ZWAscii", "ZWA"],
        min: 1,
        max: 2,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["ZHex", "ZH"],
        min: 1,
        max: 1,
        ret: "%String",
    },
    // `$ZDCHAR(n1, n2, ...)` and the three below encode each argument, like `$CHAR`; the
    // documentation shows one.
    SimpleFunction {
        names: &["ZDChar", "ZDC"],
        min: 1,
        max: usize::MAX,
        ret: "%String",
    },
    SimpleFunction {
        names: &["ZLChar", "ZLC"],
        min: 1,
        max: usize::MAX,
        ret: "%String",
    },
    SimpleFunction {
        names: &["ZQChar", "ZQC"],
        min: 1,
        max: usize::MAX,
        ret: "%String",
    },
    SimpleFunction {
        names: &["ZWChar", "ZWC"],
        min: 1,
        max: usize::MAX,
        ret: "%String",
    },
    SimpleFunction {
        names: &["ZWBPack"],
        min: 1,
        max: 1,
        ret: "%String",
    },
    SimpleFunction {
        names: &["ZWPack"],
        min: 1,
        max: 1,
        ret: "%String",
    },
    SimpleFunction {
        names: &["ZWBUnpack"],
        min: 1,
        max: 1,
        ret: "%String",
    },
    SimpleFunction {
        names: &["ZWUnpack"],
        min: 1,
        max: 1,
        ret: "%String",
    },
    SimpleFunction {
        names: &["Ascii", "A"],
        min: 1,
        max: 2,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["Factor"],
        min: 1,
        max: 2,
        ret: "%String",
    },
    SimpleFunction {
        names: &["Find", "F"],
        min: 2,
        max: 3,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["INumber", "IN"],
        min: 2,
        max: 3,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["IsValidDouble"],
        min: 1,
        max: 4,
        ret: "%Boolean",
    },
    SimpleFunction {
        names: &["IsValidNum"],
        min: 1,
        max: 4,
        ret: "%Boolean",
    },
    SimpleFunction {
        names: &["IsVector"],
        min: 1,
        max: 1,
        ret: "%Boolean",
    },
    SimpleFunction {
        names: &["Justify", "J"],
        min: 2,
        max: 3,
        ret: "%String",
    },
    SimpleFunction {
        names: &["ListFind", "LF"],
        min: 2,
        max: 3,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["ListFromString", "LFS"],
        min: 1,
        max: 3,
        ret: "%List",
    },
    SimpleFunction {
        names: &["ListLength", "LL"],
        min: 1,
        max: 1,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["ListSame", "LS"],
        min: 2,
        max: 2,
        ret: "%Boolean",
    },
    SimpleFunction {
        names: &["ListToString", "LTS"],
        min: 1,
        max: 3,
        ret: "%String",
    },
    SimpleFunction {
        names: &["ListValid", "LV"],
        min: 1,
        max: 1,
        ret: "%Boolean",
    },
    SimpleFunction {
        names: &["Lock"],
        min: 1,
        max: 2,
        ret: "%String",
    },
    SimpleFunction {
        names: &["Match"],
        min: 2,
        max: 2,
        ret: "%Boolean",
    },
    SimpleFunction {
        names: &["MVV"],
        min: 1,
        max: 1,
        ret: "%String",
    },
    SimpleFunction {
        names: &["NConvert", "NC"],
        min: 2,
        max: 3,
        ret: "%String",
    },
    SimpleFunction {
        names: &["Normalize"],
        min: 2,
        max: 2,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["Now"],
        min: 0,
        max: 1,
        ret: "%String",
    },
    SimpleFunction {
        names: &["Number", "NUM"],
        min: 1,
        max: 4,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["QLength", "QL"],
        min: 1,
        max: 1,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["QSubscript", "QS"],
        min: 2,
        max: 2,
        ret: "%String",
    },
    SimpleFunction {
        names: &["Random", "R"],
        min: 1,
        max: 1,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["Replace"],
        min: 3,
        max: 6,
        ret: "%String",
    },
    SimpleFunction {
        names: &["Reverse", "RE"],
        min: 1,
        max: 1,
        ret: "%String",
    },
    SimpleFunction {
        names: &["SConvert", "SC"],
        min: 2,
        max: 4,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["Stack", "ST"],
        min: 1,
        max: 2,
        ret: "%String",
    },
    SimpleFunction {
        names: &["Translate", "TR"],
        min: 2,
        max: 3,
        ret: "%String",
    },
    SimpleFunction {
        names: &["View", "V"],
        min: 1,
        max: 3,
        ret: "%String",
    },
    SimpleFunction {
        names: &["WAscii", "WA"],
        min: 1,
        max: 2,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["WFind", "WF"],
        min: 2,
        max: 3,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["WIsWide"],
        min: 1,
        max: 1,
        ret: "%Boolean",
    },
    SimpleFunction {
        names: &["WLength", "WL"],
        min: 1,
        max: 2,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["WReverse", "WRE"],
        min: 1,
        max: 1,
        ret: "%String",
    },
    SimpleFunction {
        names: &["ZBitAnd"],
        min: 2,
        max: 2,
        ret: "%Binary",
    },
    SimpleFunction {
        names: &["ZBitCount"],
        min: 1,
        max: 1,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["ZBitFind"],
        min: 2,
        max: 3,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["ZBitGet"],
        min: 2,
        max: 2,
        ret: "%Boolean",
    },
    SimpleFunction {
        names: &["ZBitLen"],
        min: 1,
        max: 1,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["ZBitNot"],
        min: 1,
        max: 1,
        ret: "%Binary",
    },
    SimpleFunction {
        names: &["ZBitOr"],
        min: 2,
        max: 2,
        ret: "%Binary",
    },
    SimpleFunction {
        names: &["ZBitSet"],
        min: 3,
        max: 3,
        ret: "%Binary",
    },
    SimpleFunction {
        names: &["ZBitStr"],
        min: 1,
        max: 2,
        ret: "%Binary",
    },
    SimpleFunction {
        names: &["ZBitXor"],
        min: 2,
        max: 2,
        ret: "%Binary",
    },
    SimpleFunction {
        names: &["ZBoolean", "ZB"],
        min: 3,
        max: 3,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["ZConvert", "ZCVT"],
        min: 2,
        max: 4,
        ret: "%String",
    },
    SimpleFunction {
        names: &["ZCrc"],
        min: 2,
        max: 3,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["ZCyc"],
        min: 1,
        max: 1,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["ZIsWide"],
        min: 1,
        max: 1,
        ret: "%Boolean",
    },
    SimpleFunction {
        names: &["ZName"],
        min: 1,
        max: 3,
        ret: "%Boolean",
    },
    SimpleFunction {
        names: &["ZPosition"],
        min: 2,
        max: 3,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["ZPower"],
        min: 2,
        max: 2,
        ret: "%Numeric",
    },
    SimpleFunction {
        names: &["ZSearch", "ZSE"],
        min: 1,
        max: 1,
        ret: "%String",
    },
    // `$ZSEEK()` reads the position without moving.
    SimpleFunction {
        names: &["ZSeek"],
        min: 0,
        max: 2,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["ZVersion"],
        min: 1,
        max: 1,
        ret: "%String",
    },
    SimpleFunction {
        names: &["ZWidth"],
        min: 1,
        max: 2,
        ret: "%Integer",
    },
    SimpleFunction {
        names: &["ZZenkaku"],
        min: 1,
        max: 3,
        ret: "%String",
    },
];

// Arguments past the documented ones that IRIS takes, and the checker reports (A5156): a fourth
// of `$ZBITSET`, and of `$VIEW` in mode -7 alone, which IRIS's own routines pass; in any other mode
// a fourth raises <FUNCTION>.
fn undocumented_extra(function: &SimpleFunction, args: &[Range<Option<Expr>>]) -> usize {
    match function.names[0] {
        "ZBitSet" => 1,
        "View" if args.get(1).is_some_and(|mode| mode.print().trim() == "-7") => 1,
        _ => 0,
    }
}

pub fn simple_function(name: &str) -> Option<&'static SimpleFunction> {
    SIMPLE_FUNCTIONS
        .iter()
        .find(|f| f.names.iter().any(|n| n.eq_ignore_ascii_case(name)))
}

#[derive(Grammar)]
#[grammar(validated)]
pub struct SimpleFunctionName(pub StringOf1<IsAlphabetic>);

impl Validate for SimpleFunctionName {
    const REQUIREMENT: &'static str = "be a function taking plain expression arguments";
    fn validate(&self) -> bool {
        simple_function(&self.0.print()).is_some()
    }
}

pub type ExprArgList = ParenList<Range<Option<Expr>>>;

#[derive(Grammar)]
pub struct SimpleFunctionCallSource {
    name: Range<SimpleFunctionName>,
    args: ExprArgList,
}

#[derive(GrammarTryFromOther)]
#[grammar(name = "simple function call")]
pub struct SimpleFunctionCall {
    pub name: Range<SimpleFunctionName>,
    pub args: ExprArgList,
}

impl SimpleFunctionCall {
    pub fn is_settable(&self) -> bool {
        self.name.print().eq_ignore_ascii_case("MVV")
    }
}

impl Spelled for SimpleFunctionCall {
    fn spelled(&self) -> KeywordUse {
        let function = simple_function(&self.name.print()).expect("validated on parse");
        KeywordUse {
            kind: KeywordKind::Function,
            start: self.name.start,
            text: self.name.print(),
            full: function.names[0],
            abbr: function.names.get(1).copied(),
        }
    }
}

impl GrammarFrom for SimpleFunctionCall {
    type Source = SimpleFunctionCallSource;
    fn print_to(&self, buf: &mut String) {
        self.name.print_to(buf);
        self.args.print_to(buf);
    }
}

// A `FunArgError` naming its function, for the table-driven functions that share one grammar.
#[derive(Debug)]
pub struct NamedFunArgError(pub String, pub FunArgError);

impl std::fmt::Display for NamedFunArgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "${} {}", self.0, self.1)
    }
}

impl TryFrom<SimpleFunctionCallSource> for SimpleFunctionCall {
    type Error = NamedFunArgError;
    fn try_from(
        SimpleFunctionCallSource { name, args }: SimpleFunctionCallSource,
    ) -> Result<Self, NamedFunArgError> {
        let function = simple_function(&name.print()).expect("validated by the grammar");
        let named = |err| NamedFunArgError(name.print(), err);
        let items = &args.items;
        let given = if let [only] = items.as_slice()
            && only.is_none()
        {
            0
        } else {
            items.len()
        };
        let max = function.max + undocumented_extra(function, items);
        if given > max {
            let extra = items[max..]
                .iter()
                .enumerate()
                .map(|(i, arg)| (max + i + 1, arg.print().trim().to_string()))
                .collect();
            return Err(named(FunArgError::TooMany { max, extra }));
        }
        if given < function.min {
            return Err(named(FunArgError::TooFew {
                min: function.min,
                given,
            }));
        }
        Ok(Self { name, args })
    }
}

// The functions with a hand-written argument grammar, and how many arguments they take.
const HAND_WRITTEN_ARITY: &[(&[&str], usize, usize)] = &[
    (&["TEXT", "T"], 1, 1),
    (&["BITLOGIC"], 1, 2),
    (&["ZF"], 1, usize::MAX),
];

#[derive(Grammar)]
#[grammar(validated)]
pub struct HandWrittenFunctionName(pub StringOf1<IsAlphabetic>);

impl Validate for HandWrittenFunctionName {
    const REQUIREMENT: &'static str = "be a function with a hand-written argument grammar";
    fn validate(&self) -> bool {
        hand_written_arity(&self.0.print()).is_some()
    }
}

fn hand_written_arity(name: &str) -> Option<(usize, usize)> {
    HAND_WRITTEN_ARITY
        .iter()
        .find(|(names, _, _)| names.iter().any(|n| n.eq_ignore_ascii_case(name)))
        .map(|&(_, min, max)| (min, max))
}

#[derive(Grammar)]
pub struct GenericFunctionCallSource {
    name: HandWrittenFunctionName,
    args: Paren<GenericFunArgs>,
}

#[derive(GrammarTryFromOther)]
#[grammar(name = "function call")]
pub struct GenericFunctionCall {
    pub name: HandWrittenFunctionName,
    pub args: Paren<GenericFunArgs>,
}

impl GrammarFrom for GenericFunctionCall {
    type Source = GenericFunctionCallSource;
    fn print_to(&self, buf: &mut String) {
        self.name.print_to(buf);
        self.args.print_to(buf);
    }
}

impl TryFrom<GenericFunctionCallSource> for GenericFunctionCall {
    type Error = NamedFunArgError;
    fn try_from(
        GenericFunctionCallSource { name, args }: GenericFunctionCallSource,
    ) -> Result<Self, NamedFunArgError> {
        let (min, max) = hand_written_arity(&name.print()).expect("validated by the grammar");
        let cursor = ArgCursor::new(args.wrapped, min, Some(max), false);
        let err = match cursor.check_count() {
            Err(err) => err,
            Ok(()) => FunArgError::Unsupported {
                given: cursor.given,
            },
        };
        Err(NamedFunArgError(name.print(), err))
    }
}

#[derive(Grammar)]
#[grammar(validated)]
pub struct UnknownFunctionName(pub StringOf1<IsAlphabetic>);

impl Validate for UnknownFunctionName {
    const REQUIREMENT: &'static str = "not be a known function";
    fn validate(&self) -> bool {
        let name = self.0.print();
        !FUNCTION_KEYWORDS.iter().any(|(full, abbr)| {
            full.eq_ignore_ascii_case(&name) || abbr.is_some_and(|a| a.eq_ignore_ascii_case(&name))
        }) && simple_function(&name).is_none()
            && hand_written_arity(&name).is_none()
    }
}

#[derive(Grammar)]
pub struct UnknownFunctionCall {
    pub name: UnknownFunctionName,
    pub args: FunArgList,
}

impl UnknownFunctionCall {
    // The object internals IRIS's own classes assign to, undocumented like the rest of `$ZOBJ*`.
    pub fn is_settable(&self) -> bool {
        let name = self.name.print();
        ["ZOBJVAL", "ZOBJOID", "ZOBJMOD", "ZOBJMODS"]
            .iter()
            .any(|settable| settable.eq_ignore_ascii_case(&name))
    }
}

// Every full function name, hand-written or in `SIMPLE_FUNCTIONS`, for the checker's did-you-mean
// list; the style test keeps it in sync with `FUNCTION_KEYWORDS` and `SIMPLE_FUNCTIONS`.
pub const FUNCTION_NAMES: &[&str] = &[
    "TEXT",
    "LENGTH",
    "DATA",
    "ORDER",
    "GET",
    "CASE",
    "BIT",
    "BITCOUNT",
    "BITFIND",
    "BITLOGIC",
    "CHANGE",
    "CHAR",
    "CLASSNAME",
    "ZOBJCLASS",
    "DECIMAL",
    "DOUBLE",
    "EXTRACT",
    "FNUMBER",
    "FACTOR",
    "FIND",
    "INUMBER",
    "ISOBJECT",
    "ISVALIDDOUBLE",
    "ISVALIDNUM",
    "ISVECTOR",
    "INCREMENT",
    "ZINCREMENT",
    "JUSTIFY",
    "LIST",
    "LISTBUILD",
    "LISTDATA",
    "LISTFIND",
    "LISTFROMSTRING",
    "LISTGET",
    "LISTLENGTH",
    "LISTNEXT",
    "LISTSAME",
    "LISTTOSTRING",
    "LISTUPDATE",
    "LISTVALID",
    "LOCATE",
    "MATCH",
    "METHOD",
    "ZOBJMETHOD",
    "MV",
    "NAME",
    "NCONVERT",
    "NORMALIZE",
    "NOW",
    "NUMBER",
    "PARAMETER",
    "PIECE",
    "PREFETCHOFF",
    "PREFETCHON",
    "PROPERTY",
    "ZOBJPROPERTY",
    "QLENGTH",
    "QSUBSCRIPT",
    "QUERY",
    "RANDOM",
    "REPLACE",
    "REVERSE",
    "SCONVERT",
    "SELECT",
    "SEQUENCE",
    "SORTBEGIN",
    "SORTEND",
    "STACK",
    "TRANSLATE",
    "VECTOR",
    "VECTORDEFINED",
    "VECTOROP",
    "VIEW",
    "WEXTRACT",
    "ASCII",
    "WASCII",
    "WCHAR",
    "WFIND",
    "WISWIDE",
    "WLENGTH",
    "WREVERSE",
    "XECUTE",
    "CLASSMETHOD",
    "ZOBJCLASSMETHOD",
    "COMPILE",
    "ZARCCOS",
    "ZARCSIN",
    "ZARCTAN",
    "ZCOS",
    "ZCOT",
    "ZCSC",
    "ZSEC",
    "ZSIN",
    "ZTAN",
    "ZABS",
    "ZEXP",
    "ZLN",
    "ZLOG",
    "ZSQR",
    "ZDASCII",
    "ZLASCII",
    "ZQASCII",
    "ZWASCII",
    "ZHEX",
    "ZDCHAR",
    "ZLCHAR",
    "ZQCHAR",
    "ZWCHAR",
    "ZWBPACK",
    "ZWPACK",
    "ZWBUNPACK",
    "ZWUNPACK",
    "ZWIDTH",
    "ZCYC",
    "ZISWIDE",
    "ZNAME",
    "ZVERSION",
    "ZPOWER",
    "ZPOSITION",
    "ZCRC",
    "ZBITGET",
    "ZBITAND",
    "ZBITOR",
    "ZBITXOR",
    "ZBITNOT",
    "ZBITCOUNT",
    "ZBITLEN",
    "MVV",
    "ZBITFIND",
    "ZBITSET",
    "ZBITSTR",
    "LOCK",
    "ZORDER",
    "ZPREVIOUS",
    "NEXT",
    "ZBOOLEAN",
    "ZOBJREF",
    "ZCONVERT",
    "ZZENKAKU",
    "ZSTRIP",
    "ZSEARCH",
    "ZSEEK",
    "ZDATETIMEH",
    "ZDATETIME",
    "ZDATEH",
    "ZDATE",
    "ZTIMEH",
    "ZTIME",
    "ZF",
    "ZUTIL",
];

#[derive(Grammar)]
#[grammar(inline)]
pub enum AtomicExpr {
    DAO(DAO),
    Number(Number),
    Variable(Variable),
    CacheDirectCallback(CacheDirectCallback),
    String(OSString),
    CallSubroutine {
        prefix: StringEq!("$$"),
        label: Range<Option<Range<AllowIndirection<Label>>>>,
        routine: Option<ExtendedRoutineReference>,
        args: Option<MthArgList>,
    },
    FunctionCall(Prefix<StringEq!("$"), FunctionCall>),
    SpecialVariable {
        sym: StringEq!("$"),
        var: SpecialVariable,
    },
    This(StringEqCI!("##this")),
    Super(Prefix<StringEqCI!("##super"), Range<MthArgList>>),
    Class {
        kw: StringEqCI!("##class"),
        cls: Paren<QuotableClassName>,
        // `##class(C)oref.M()` casts `M` to `C`'s on `oref`, a variable or `$this` (documented as
        // "Casting a Method"); a parenthesized `oref` is not.
        oref: Option<CastReceiver>,
    },
    QuoteExpression {
        kw: StringEqCI!("##quoteExp"),
        cls: Paren<Expr>,
    },
    // Evaluated by the compiler; its value is spliced into the source as text.
    CompileTime {
        kw: Either<StringEqCI!("##safeexpression"), StringEqCI!("##expression")>,
        expr: Paren<Expr>,
    },
    QuoteString {
        kw: StringEqCI!("##quote"),
        cls: MacroArgList,
    },
    Member {
        member: Prefix<StringEq!(".."), Range<MemberRef>>,
        args: Option<Range<MthArgList>>,
    },
    BeginQuoteEndQuote(
        StringEqCI!("##beginquote"),
        StringEndsWith<StringEqCI!("##EndQuote")>,
    ),
    // Indirection is eval
    Indirection(Range<Indirection>),
}

#[derive(Grammar)]
#[grammar(inline)]
pub struct ExprOpsStruct<BeforeFirst> {
    pub base: Box<Range<ParenedExpr<BeforeFirst>>>,
    pub ops: Vec<SufOp>,
}

#[derive(GrammarFromOther)]
pub enum ExprOps<BeforeFirst> {
    Base(Box<ParenedExpr<BeforeFirst>>),
    Then {
        base: Box<Range<ExprOps<BeforeFirst>>>,
        op: SufOp,
    },
}

impl<BeforeFirst: Grammar> GrammarFrom for ExprOps<BeforeFirst> {
    type Source = ExprOpsStruct<BeforeFirst>;
    fn print_to(&self, buf: &mut String) {
        match self {
            ExprOps::Base(base) => base.print_to(buf),
            ExprOps::Then { base, op } => {
                base.print_to(buf);
                op.print_to(buf);
            }
        }
    }
}

impl<BeforeFirst: Grammar> From<ExprOpsStruct<BeforeFirst>> for ExprOps<BeforeFirst> {
    fn from(value: ExprOpsStruct<BeforeFirst>) -> Self {
        let start = value.base.start;
        let mut end = value.base.end;
        let mut base = Self::Base(Box::new(value.base.into_inner()));
        for op in value.ops {
            let op_end = op.args.as_ref().map_or(op.mem.end, |args| args.end);
            base = Self::Then {
                base: Box::new(Range::new(start, base, end)),
                op,
            };
            end = op_end;
        }
        base
    }
}

#[derive(Grammar)]
#[grammar(inline)]
pub enum OpsExpr<BeforeFirst = ()> {
    Base(ExprOps<BeforeFirst>),
    Prefixed {
        op: Wrap<BeforeFirst, PrefixOperator, MGap>,
        remaining: Box<Range<OpsExpr<()>>>,
    },
}

#[derive(Grammar)]
#[grammar(name = "Expression")]
pub struct Expr(GenericExpr);

impl std::ops::DerefMut for Expr {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Deref for Expr {
    type Target = GenericExpr;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Grammar)]
pub struct ExpressionAfterIf(GenericExpr<StringEq!(" ")>);

impl std::ops::DerefMut for ExpressionAfterIf {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Deref for ExpressionAfterIf {
    type Target = GenericExpr<StringEq!(" ")>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Grammar)]
pub struct ExpressionAfterWhile(GenericExpr<LGap>);

impl std::ops::DerefMut for ExpressionAfterWhile {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Deref for ExpressionAfterWhile {
    type Target = GenericExpr<LGap>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Grammar)]
pub struct ExpressionAfterColon(GenericExpr<(), ()>);

impl std::ops::DerefMut for ExpressionAfterColon {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Deref for ExpressionAfterColon {
    type Target = GenericExpr<(), ()>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Grammar)]
pub struct GenericExpr<BeforeFirst = (), BetweenInfix = MGap> {
    pub base: Range<OpsExpr<BeforeFirst>>,
    pub ops: Vec<Range<InfixOperation<BetweenInfix>>>,
}

impl Expr {
    pub fn as_variable(&self) -> Option<&Variable> {
        if !self.ops.is_empty() {
            return None;
        }
        let OpsExpr::Base(this) = self.base.deref() else {
            return None;
        };
        let ExprOps::Base(this) = this else {
            return None;
        };
        let ParenedExpr::Normal(this) = this.deref() else {
            return None;
        };
        match this.deref().deref() {
            AtomicExpr::Variable(this) => Some(this),
            _ => None,
        }
    }
}

pub type ParenExpr = Paren<Wrap<MGap, Expr, MGap>>;

pub type ParenList<Item> = Paren<MGapXMGap<VecSep<Item, CommaSep<MGap>>>>;
pub type MthArgList = ParenList<Range<Option<MthArg>>>;
pub type FunArgList = ParenList<Range<Option<FunArg>>>;

char_class!(pub IsSpaceOrTab, "space", |ch| matches!(ch, ' ' | '\t'));

#[derive(Grammar)]
#[grammar(hidden)]
pub struct SpaceOrTab(StringOf<IsSpaceOrTab>);
#[derive(Grammar)]
#[grammar(hidden)]
pub struct SpaceOrTab1(StringOf1<IsSpaceOrTab>);

char_class!(pub IsRoutineNamePart, "routine_name_part", |ch| {
    ch.is_alphanumeric() || ch == '.'
});

// https://docs.intersystems.com/iris20262/csp/docbook/DocBook.UI.Page.cls?KEY=RCOS_appx_identifiers#RCOS_appx_identifiers_cos_routines
#[derive(Grammar, Clone, PartialEq, Eq)]
pub struct RoutineNameAndLabel {
    head: CharOf<IsLetterOrPercent>,
    body: Option<(
        Vec<(StringOf<IsLetterOrNumberIncludingUnicode>, StringEq!("."))>,
        StringOf1<IsLetterOrNumberIncludingUnicode>,
    )>,
}

#[derive(Grammar)]
pub enum Label {
    Standard(RoutineNameAndLabel),
    // Some old code allows numeric labels
    Lecacy(StringOf1<IsLetterOrNumber>),
}

char_class!(pub IsMacroNameChar, "macro_name_char", |ch| {
    ch.is_alphanumeric() || ch == '%'
});

#[derive(Grammar, Clone)]
pub struct MacroName(pub Range<MacroNameText>);

#[derive(Grammar, Clone)]
pub struct MacroNameText(StringOf1<IsMacroNameChar>);

#[derive(Grammar)]
pub struct Value(Expr);

#[derive(Grammar)]
pub enum ScopeKeyword {
    Private(StringEqCI!("Private")),
    Public(StringEqCI!("Public")),
}

pub type RoutineName = RoutineNameAndLabel;

#[derive(Grammar)]
pub enum RoutineType {
    Inc(StringEqCI!("INC")),
    Int(StringEqCI!("INT")),
    Mac(StringEqCI!("MAC")),
}

pub type RoutineTypeSpec = Wrap<StringEq!("["), RoutineAttributes, StringEq!("]")>;

/// `[Type=INT]`, or with `Generated`, how IRIS exports a routine it generated: `[Type=INT,Generated]`
/// from a namespace, `[Generated]` alone when the type is the file's. Undocumented, and parsed for
/// what it is.
#[derive(Grammar)]
pub enum RoutineAttributes {
    Typed {
        routine_type: Prefix<Raw<(StringEqCI!("Type"), StringEq!("="))>, RoutineType>,
        generated: Option<Raw<(StringEq!(","), StringEqCI!("Generated"))>>,
    },
    Generated(StringEqCI!("Generated")),
}

impl RoutineAttributes {
    pub fn routine_type(&self) -> Option<&RoutineType> {
        match self {
            RoutineAttributes::Typed { routine_type, .. } => Some(routine_type),
            RoutineAttributes::Generated(_) => None,
        }
    }
}

#[derive(Grammar)]
pub struct RoutineHeader {
    keyword: Raw<(StringEqCI!("ROUTINE"), SpaceOrTab1)>,
    pub name: Range<RoutineName>,
    pub type_spec: Option<SpaceThen<RoutineTypeSpec>>,
}

pub type SpaceThen<T> = Prefix<SpaceOrTab, T>;

#[derive(Grammar)]
pub struct ParameterEntry {
    pub by_ref: Option<StringEq!("&")>,
    pub param: Range<LocalVariableName>,
    pub expand: Option<StringEq!("...")>,
    pub default: Option<Prefix<Raw<Wrap<MGap, StringEq!("="), MGap>>, Value>>,
}

pub type ParameterList = Wrap<
    Raw<(StringEq!("("), MGap)>,
    Option<VecSep<ParameterEntry, Raw<(MGap, StringEq!(","), MGap)>>>,
    Raw<(MGap, StringEq!(")"))>,
>;

pub type PublicVarList = Wrap<
    Raw<(StringEq!("["), MGap)>,
    Option<VecSep<Range<AllowMacro<LocalVariableName>>, Raw<(MGap, StringEq!(","), MGap)>>>,
    Raw<(MGap, StringEq!("]"))>,
>;

// https://docs.intersystems.com/iris20262/csp/docbook/DocBook.UI.Page.cls?KEY=ALEGROU#ALEGROU_subrssyntax
#[derive(Grammar)]
pub struct SubroutineHeader {
    // It is weird but some subroutine headers are integer
    pub label: Range<Label>,
    pub params: Option<SpaceThen<ParameterList>>,
    pub scope: Option<SpaceThen<ScopeKeyword>>,
}

// Subroutine can't includes its ending \n because some files doesn't end with a newline
#[derive(Grammar)]
pub struct Subroutine {
    pub header: AllowMacro<SubroutineHeader>,
    pub body: Range<RoutineMain>,
}

// https://docs.intersystems.com/iris20262/csp/docbook/DocBook.UI.Page.cls?KEY=GCOS_procedures#GCOS_procedures_procsyntax
#[derive(Grammar)]
pub struct ProcedureHeader {
    pub label: Range<Label>,
    // `lab {` without one IRIS refuses (ERROR #1026).
    pub params: SpaceThen<ParameterList>,
    pub publics: Option<Prefix<StringOf<IsWhitespace>, AllowMacro<PublicVarList>>>,
    pub scope: Option<SpaceThen<ScopeKeyword>>,
}

#[derive(Grammar)]
pub struct Procedure {
    pub header: Range<AllowMacro<ProcedureHeader>>,
    gap: Raw<VecSep<SGap, NL>>,
    pub body: Range<Brace<ProcedureBody>>,
    end: Raw<EndOfLine>,
    pub trailer: Option<ProcedureTrailer>,
}

// Command lines after a procedure's `}` and before the next label: IRIS compiles them, though only
// an offset reaches them. The lookahead for a line keeps an empty main from taking the newlines
// the next label is parted by.
#[derive(Grammar)]
#[grammar(inline)]
pub struct ProcedureTrailer {
    gap: Raw<(Vec1<NL>, NotFollowedBy<NotFollowedBy<SGap1>>)>,
    pub lines: Range<RoutineMain>,
}

#[derive(Grammar)]
pub struct ProcedureBody {
    // The first line needs no space and does not permit dots
    pub main: ProcedureMain,
    pub subroutines: Option<Prefix<Raw<Vec1<NL>>, VecSep<RoutineElement, Raw<Vec1<NL>>>>>,
    end: Raw<VecSep<SGap, NL>>,
}

// RoutineElements can't includes its ending \n because some files doesn't end with a newline
#[derive(Grammar)]
pub enum RoutineElement {
    // Procedure must be attempted first because subroutine header can match procedure header
    Procedure(Procedure),
    Subroutine(Subroutine),
}

#[derive(Grammar)]
pub struct Routine {
    pub header: RoutineHeader,
    pub main: RoutineMain,
    pub subroutines: Option<Subroutines>,
    end: Raw<(VecSep<SGap, NL>, Option<LineComment>)>,
}

// The labels after the main body, each behind the newlines, comment lines among them, that part
// it from what precedes it.
#[derive(Grammar)]
#[grammar(inline)]
pub struct Subroutines {
    pub gap: Raw<Vec1<NL>>,
    pub list: VecSep<RoutineElement, Raw<Vec1<NL>>>,
}

impl Deref for Subroutines {
    type Target = VecSep<RoutineElement, Raw<Vec1<NL>>>;

    fn deref(&self) -> &Self::Target {
        &self.list
    }
}

#[derive(Grammar)]
pub struct RoutineWithMacro {
    pub header: RoutineHeader,
    pub macroed_units: Vec<Range<MacroedUnit>>,
}

#[derive(Grammar, Clone)]
pub enum MacroedUnit {
    Redex(MacroRedex),
    Plain(Raw<Vec1<(NotFollowedBy<MacroRedex>, CharOf<AnyChar>)>>),
}

char_class!(pub IsNotCloseParen, "not_close_paren", |ch| ch != ')');

#[derive(Grammar, Clone)]
pub enum MacroRedex {
    // `##lit(text)`: `text` verbatim, shielded from expansion.
    Lit(
        StringEqCI!("##lit("),
        StringOf<IsNotCloseParen>,
        StringEq!(")"),
    ),
    // `##beginlit text ##endlit`: the same, undocumented, in IRIS's own includes.
    BeginLit(
        StringEqCI!("##beginlit"),
        Raw<Vec<(NotFollowedBy<StringEqCI!("##endlit")>, CharOf<AnyChar>)>>,
        StringEqCI!("##endlit"),
    ),
    H(HMacro),
    // Passed through whole, so a `;` inside (`&nbsp;`, a JavaScript statement) isn't a comment.
    Embedded(Raw<EmbeddedBlock>),
    // Passed through whole, so a `//` or `;` inside isn't a line comment eating the `*/`.
    RC(RangeComment),
    LC(LineComment),
    DDD(MacroCall),
    STR(OSString),
}

#[derive(Grammar)]
pub enum EmbeddedBlock {
    Html(StringEqCI!("&html<"), HTML, StringEq!(">")),
    Js(JsMarker, EmbeddedJS, StringEq!(">")),
}

pub type JsMarker = Either<StringEqCI!("&js<"), StringEqCI!("&javascript<")>;

// - CommandLine can't includes its ending \n because some files doesn't end with a newline
#[derive(Grammar)]
pub struct CommandLine<Start> {
    start_of_line: Start,
    // some old routines contain no space between the last dot and the first command
    pub periods: Vec<(StringEq!("."), SGap)>,
    pub commands: Option<VecSep<Range<Command>, SGap1>>,
}

// Main Blocks should never ends with '\n'
// RoutineMain requires some prefixing space, while ProcedureMain does not
#[derive(Grammar)]
pub struct ProcedureMain {
    pub line_1: Option<CommandLine<SGap>>,
    pub line_n: Option<Prefix<Raw<Vec1<NL>>, VecSep<CommandLine<LineStart>, Raw<Vec1<NL>>>>>,
}

char_class!(pub IsNotHash, "not_hash", |ch| ch != '#');

// A command line starts with whitespace (column 0 is for labels) -- except a `#dim` at column 0.
pub type LineStart = Either<SGap1, NotFollowedBy<CharOf<IsNotHash>>>;

#[derive(Grammar)]
pub struct RoutineMain {
    pub line_1: Option<CommandLine<SGap1>>,
    pub line_n: Option<Prefix<Raw<Vec1<NL>>, VecSep<CommandLine<SGap1>, Raw<Vec1<NL>>>>>,
}

#[derive(Grammar, Clone)]
#[grammar(inline)]
pub enum LineCommentStart {
    LLLineComment(StringEq!("//")),
    ELineComment(StringEq!(";")),
    HELineComment(StringEq!("#;")),
    HHELineComment(StringEq!("##;")),
    // `##lit(;...)` shields a `;` from the preprocessor; once expanded it's this comment. Only
    // the unexpanded (standalone parser) path sees it spelled out.
    LitELineComment(StringEqCI!("##lit(;")),
}

#[derive(Grammar)]
pub struct EndOfLine {
    gap: SGap,
    comment: Option<LineComment>,
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn routine(body: &str) -> Result<(), Error> {
        Routine::scan(&format!("ROUTINE r [Type=MAC]\n{body}"))
    }

    fn methods(cls: &ClassDefinition) -> Vec<&Method> {
        cls.member_list
            .members
            .seps
            .iter()
            .filter_map(|member| match member.deref() {
                Member::Method(method) => Some(method),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn storage_special_variable() {
        routine(" w $STORAGE,$ST,$S,$SY,$STACK,$SYSTEM\n").expect("Ok");
    }

    #[test]
    fn routine_names_carry_no_caret() {
        routine(" d ^%ZSTART,^ZWELCOME,^%ZLANGC00,Tag^%ZSTOP\n").expect("Ok");
        routine("#include ^ZWELCOME\n q\n").expect_err("an include is named without a caret");
        Routine::scan("ROUTINE ^%ZSTART [Type=MAC]\n q\n").expect_err("a header names no caret");
    }

    #[test]
    fn zseek_without_arguments() {
        routine(" s p=$ZSEEK(),q=$ZSEEK(0,1),r=$ZSEEK(10)\n").expect("Ok");
        routine(" s p=$ZSEEK(1,2,3)\n").expect_err("at most two");
    }

    #[test]
    fn zf_minus_4_takes_two_arguments() {
        routine(" s x=$ZF(-4,2),y=$ZF(-4,1,\"dll\"),z=$ZF(-4,2,id,1)\n").expect("Ok");
        routine(" s x=$ZF(-4)\n").expect_err("the utility number is required");
    }

    #[test]
    fn known_command_without_its_argument_is_not_reported_as_unknown() {
        for line in [" set\n", " s\n", " merge\n", " zkill\n", " zk\n"] {
            let err = routine(line).expect_err(line).to_string();
            assert!(
                err.contains("is a command, but its argument"),
                "{line}: {err}"
            );
        }
        let err = routine(" foo 1\n").expect_err("foo").to_string();
        assert!(err.contains("is not a recognized command"), "{err}");
    }

    #[test]
    fn z_command_abbreviation_without_its_argument_is_not_a_user_command() {
        routine(" zk x\n zn \"USER\"\n ZDN x\n").expect("Ok");
        routine(" zk\n").expect_err("ZKILL needs an argument");
        routine(" zn\n").expect_err("ZNSPACE needs an argument");
    }

    #[test]
    fn abbreviated_function_with_bad_arguments_is_not_unknown() {
        for call in [
            "$P(a)",
            "$E()",
            "$LG(a,1,2,3)",
            "$ZDT()",
            "$ZINCR()",
            "$ZP()",
        ] {
            let err = routine(&format!(" s x={call}\n"))
                .expect_err(call)
                .to_string();
            assert!(err.contains("argument"), "{call}: {err}");
        }
    }

    #[test]
    fn read_column_is_an_expression() {
        routine(" r ?n,x:0 r ?(n+1),y:0 r ?$l(\"abc\"),z:0 r !?5,w\n").expect("Ok");
    }

    #[test]
    fn variable_names_may_start_with_or_contain_any_letter() {
        routine(" s Ω=1,aé=3,%é=2 s ^é=2,^a.é.b=1,^|\"USER\"|é=3,^%é=4\n").expect("Ok");
    }

    #[test]
    fn language_cache_is_objectscript() {
        let cls = ClassDefinition::parse(
            "Class A.L\n{\n\nClassMethod M() [ Language = cache ]\n{\n quit 1\n}\n\n}\n",
        )
        .expect("Ok");
        assert!(methods(&cls)[0].header.is_objectscript());
    }

    #[test]
    fn legacy_index_collations() {
        let index = |index: &str| {
            let cls = ClassDefinition::parse(&format!(
                "Class A.C Extends %Persistent\n{{\n\nProperty Name As %String;\n\n{index}\n\n}}\n"
            ))?;
            let collations: Vec<_> = cls
                .member_list
                .members
                .seps
                .iter()
                .filter_map(|member| match member.deref() {
                    Member::Index(index) => Some(index),
                    _ => None,
                })
                .flat_map(|index| index.on_properties().unwrap_or_default())
                .map(|key| key.collation())
                .collect();
            Ok::<_, Error>(collations)
        };
        assert_eq!(
            index("Index A On Name As Upper;\n\nIndex B On Name As String(50);").expect("Ok"),
            [Some("Upper".to_string()), Some("String(50)".to_string())]
        );
        index(
            "Index A On Name As Space;\n\nIndex B On Name As AlphaUp;\n\nIndex C On Name As Mvr;",
        )
        .expect("Ok");
        index("Index A On Name As Upper(50);").expect_err("only STRING takes a length");
    }

    #[test]
    fn class_header_directives_in_any_order() {
        let class =
            |header: &str| ClassDefinition::parse(&format!("{header}\nClass A.H\n{{\n\n}}\n"));
        class("Include %occInclude\nImport %SYS\n").expect("Ok");
        class("Import %SYS\nIncludeGenerator %occInclude\nInclude %occInclude\n").expect("Ok");
        assert!(
            class("Include %occInclude\nInclude %occStatus\n").is_err(),
            "IRIS refuses a repeat"
        );
    }

    #[test]
    fn index_key_kind_case() {
        let class = |index: &str| {
            ClassDefinition::parse(&format!(
                "Class A.I Extends %Persistent\n{{\n\nProperty Tags As list Of %String;\n\n{index}\n\n}}\n"
            ))
        };
        class("Index TagsIdx On Tags(elements);").expect("Ok");
        class("Index TagsIdx On Tags(ELEMENTS) [ Data = (Tags(ELEMENTS)) ];").expect("Ok");
        assert!(
            class("Index TagsIdx On Tags(ELEMENTS) [ Data = (Tags(elements)) ];").is_err(),
            "IRIS reads the Data form case-sensitively"
        );
    }

    #[test]
    fn structured_system_variable_abbreviations() {
        routine(" w $d(^$J($j)),^$L(\"x\"),^$R(\"x\"),^$G(\"^x\"),^$GLOBAL(\"^x\")\n").expect("Ok");
        routine(" w ^$JO($j)\n").expect_err("JO is neither JOB nor J");
    }

    #[test]
    fn zparent_special_variable() {
        routine(" w $ZPARENT,$ZP,$ZPI,$ZPOS,$ZPOSITION\n").expect("Ok");
        assert!(matches!(
            SpecialVariable::parse("ZPA"),
            Ok(SpecialVariable::Unknown(_))
        ));
    }

    #[test]
    fn negative_number_in_dao() {
        routine(" s x={\"a\":-1}\n s y=[-1.5,2]\n").expect("Ok");
    }

    #[test]
    fn negated_and_or_operators() {
        routine(" if a'&b w 1\n if a'!b w 2\n").expect("Ok");
    }

    #[test]
    fn and_before_identifier_starting_like_an_embedded_marker() {
        routine(" s z=x&sqlflag\n s y=x&htmlx\n s w=x&jsy\n").expect("Ok");
        routine(" &sql(SELECT 1 INTO :x)\n &html<<b>x</b>>\n &js<var a = 1;>\n").expect("Ok");
        routine(" s z=x&sql(1)\n").expect_err("&sql( is not an operator");
    }

    #[test]
    fn keyword_lookup_is_case_insensitive() {
        let cls = ClassDefinition::parse(
            "Class A.B\n{\n\nClassMethod E() As %Integer [ codemode = expression ]\n{\n1 + 1\n}\n\n}\n",
        )
        .expect("Ok");
        let keywords = methods(&cls)[0].header.keywords.as_ref().expect("keywords");
        let has = |is: fn(&MethodKeyword) -> bool| {
            keywords
                .items()
                .any(|entry| matches!(&***entry, Either::Left(keyword) if is(keyword)))
        };
        assert!(has(|keyword| matches!(keyword, MethodKeyword::CodeMode(_))));
        assert!(!has(|keyword| matches!(
            keyword,
            MethodKeyword::Language(_)
        )));
    }

    #[test]
    fn signature_types() {
        assert!(matches!(
            SignatureType::parse("..#ELEMENTTYPE"),
            Ok(SignatureType::Parameter(_, name)) if name.print() == "ELEMENTTYPE"
        ));
        assert!(matches!(
            SignatureType::parse("%Library.ObjectHandle"),
            Ok(SignatureType::Class(_))
        ));
        assert!(Type::parse("..#ELEMENTTYPE").is_ok());
    }

    #[test]
    fn command_keywords_carry_their_offsets_and_documented_spellings() {
        let command =
            Range::<Command>::parse("if x { Quit } elseif y { Quit } E { Quit }").expect("Ok");
        let keywords = command.keywords(command.start);
        let keywords: Vec<_> = keywords
            .iter()
            .map(|kw| (kw.start, kw.text.as_str(), kw.full, kw.abbr))
            .collect();
        assert_eq!(
            keywords,
            [
                (0, "if", "If", Some("I")),
                (14, "elseif", "ElseIf", None),
                (32, "E", "Else", Some("E")),
            ]
        );
        let halt = Command::parse("h").expect("Ok");
        assert_eq!(halt.keywords(0)[0].full, "Halt");
        let hang = Command::parse("h 1").expect("Ok");
        assert_eq!(hang.keywords(0)[0].full, "Hang");
        assert!(
            Command::parse("$$$M(1)")
                .expect("Ok")
                .keywords(0)
                .is_empty()
        );
    }

    #[test]
    fn hash_hash_sql() {
        routine(" ##sql(SELECT 1 INTO :x)\n").expect("Ok");
    }

    #[test]
    fn semicolon_comment_with_brace_in_method_body() {
        let cls = ClassDefinition::parse(
            "Class A.C\n{\n\nMethod M()\n{\n ; TODO handle the { case\n quit\n}\n\nMethod N()\n{\n s x=\";{\" ; a comment with }\n quit x\n}\n\n}\n",
        )
        .expect("Ok");
        let methods = methods(&cls);
        assert_eq!(methods.len(), 2);
        for method in methods {
            ProcedureBody::scan(&method.body.prefixed.wrapped.print()).expect("Ok");
        }
    }

    #[test]
    fn nested_function_arguments_parse_quickly() {
        let mut expr = String::from("x");
        for _ in 0..16 {
            expr = format!("$p({expr},\",\",1)");
        }
        let start = Instant::now();
        // The recursive descent needs more stack than a test thread has by default.
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(move || routine(&format!(" s y={expr}\n")).expect("Ok"))
            .unwrap()
            .join()
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn python_method_body_is_not_brace_matched() {
        let cls = ClassDefinition::parse(
            "Class A.D\n{\n\nMethod M() [ Language = python ]\n{\n    x = \"}\"\n    y = '}'\n    # comment {\n    z = '''\n    }\n    '''\n    return x\n}\n\nMethod N()\n{\n quit 1\n}\n\n}\n",
        )
        .expect("Ok");
        let methods = methods(&cls);
        assert_eq!(methods.len(), 2);
        assert!(
            methods[0]
                .body
                .prefixed
                .wrapped
                .print()
                .contains("return x")
        );
        assert_eq!(methods[1].body.prefixed.wrapped.print(), "\n quit 1\n");
    }

    #[test]
    fn macro_call_may_have_space_before_its_arguments() {
        let units = <Vec<MacroedUnit>>::parse("$$$M (1,2)").expect("Ok");
        assert_eq!(
            units.len(),
            1,
            "{:?}",
            units.iter().map(|u| u.print()).collect::<Vec<_>>()
        );
        routine("#define M(%a,%b) %a+%b\n s x=$$$M (1,2)\n").expect("Ok");
    }

    #[test]
    fn listget_set_target_is_a_variable_list() {
        routine(" set $lg(a,b,,,,,lockHash,,proto,%qfun,x(1),^g(2))=data\n").expect("Ok");
        routine(" set $LISTGET(a)=data\n set $ListBuild(a,,b)=data\n").expect("Ok");
        routine(" set $lg(a,2)=data\n").expect_err("a position is not a variable");
    }

    #[test]
    fn order_of_a_system_function_reference() {
        routine(" s k=$order($zobjval(a,b,0,3,b,k)) q:k=\"\"\n s n=$data($zobjref(o))\n")
            .expect("Ok");
        routine(" s k=$order($piece(x,1))\n").expect_err("only $z functions are references");
    }

    #[test]
    fn lit_shielded_semicolon_starts_a_comment() {
        let cls = ClassDefinition::parse(
            "Class A.L\n{\n\nClassMethod M()\n{\n Quit\nINIT ;Name        Type\n ##lit(;;) $$$SourceA ##lit(;) \"x\"        ##lit(;) 0\n ##lit(;;) /api/x         ##lit(;) 1\n}\n\n}\n",
        )
        .expect("Ok");
        for method in methods(&cls) {
            ProcedureBody::scan(&method.body.prefixed.wrapped.print()).expect("Ok");
        }
    }

    // A class-level `[ Language = python ]` default isn't visible to the method grammar; an
    // ObjectScript brace-matching that doesn't end where a member may end is not believed.
    #[test]
    fn python_default_language_method_body_falls_back_to_the_foreign_scan() {
        let cls = ClassDefinition::parse(
            "Class A.P [ Language = python ]\n{\n\nMethod M()\n{\n    x = '}'\n    return x\n}\n\nMethod N() [ Language = objectscript ]\n{\n quit 1\n}\n\n}\n",
        )
        .expect("Ok");
        let methods = methods(&cls);
        assert_eq!(methods.len(), 2);
        assert!(
            methods[0]
                .body
                .prefixed
                .wrapped
                .print()
                .contains("return x")
        );
        assert_eq!(methods[1].body.prefixed.wrapped.print(), "\n quit 1\n");
    }

    #[test]
    fn query_and_trigger_bodies_may_contain_a_quoted_brace() {
        ClassDefinition::parse(
            "Class A.Q\n{\n\nQuery Q() As %SQLQuery\n{\nSELECT Name FROM T WHERE Name = '}'\n}\n\nTrigger T [ Event = INSERT, Language = tsql ]\n{\n SET @x = '}'\n}\n\nMethod N()\n{\n quit 1\n}\n\n}\n",
        )
        .expect("Ok");
    }

    #[test]
    fn javascript_method_body_may_close_a_block_at_column_zero() {
        let cls = ClassDefinition::parse(
            "Class A.E\n{\n\nClientMethod f(n) [ Language = javascript ]\n{\n\tswitch (n) {\n\tcase 1:\n\t\tfor (var i=0;i<n;i++) { x(i); }\n\t\tbreak;\n}\n// done\nreturn n;\n}\n\n/// Doc\nMethod N()\n{\n quit 1\n}\n\n}\n",
        )
        .expect("Ok");
        let methods = methods(&cls);
        assert_eq!(methods.len(), 2);
        assert!(
            methods[0]
                .body
                .prefixed
                .wrapped
                .print()
                .contains("return n")
        );
        assert_eq!(methods[1].body.prefixed.wrapped.print(), "\n quit 1\n");
    }
}
