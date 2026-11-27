//! Digests a parsed class or routine (from `tygr-objectscript`) into structured facts.
//!
//! Three levels of detail come out of a parse:
//! - *Details* (`ClassDetails`, `MemberDetails`, `RoutineElementDetails`): everything, with
//!   source ranges. This is what `lib.rs` caches per document and what `check/` reads.
//! - *Info* (`bindings::ClassInfo`, `MemberInfo`, ...): the subset exposed through WIT to
//!   language servers; the conversions live in `bindings.rs`.
//! - *Ty* (`check::types`): just what the type checker needs from a signature or property.
//!
//! Hover, go-to-definition, document/workspace symbols and reference search are all answered
//! from these details without re-running the type checker.

use std::collections::HashMap;
use std::ops::Deref;

use either::Either::{self, Left, Right};
use tygr::{Grammar, IntoInner, OLC, Parse, Prefix, Print, Ranged, StringOf};

use crate::bindings::{self, RefMode};
use crate::check::types::{
    Const, NormalArg, ProcedureBlock, Ty, TyProcedure, VariadicArg, parameter_ty, with_params,
};
use crate::cos::{self, AllowMacro, FailableKeyword, HasMemberKeyword, KeywordList};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WithRange<T> {
    pub it: T,
    pub range: bindings::Range,
}

// Offsets into a text as line/character positions: the text's own `OLC`, or an environment that
// shifts them (a method body checked at an offset into its class).
pub trait Positions {
    fn offset_to_line_character(&self, offset: usize) -> (usize, usize);
    fn line_character_to_offset(&self, line_character: (usize, usize)) -> usize;

    fn position(&self, offset: usize) -> bindings::Position {
        bindings::Position::from(self.offset_to_line_character(offset))
    }

    // The inverse of `position`.
    fn offset(&self, position: bindings::Position) -> usize {
        self.line_character_to_offset((position.line as usize, position.character as usize))
    }

    fn span(&self, start: usize, end: usize) -> bindings::Range {
        bindings::Range {
            start: self.position(start),
            end: self.position(end),
        }
    }

    fn range<T>(&self, ranged: &Ranged<T>) -> bindings::Range {
        self.span(ranged.start, ranged.end)
    }

    fn with_range<T>(&self, ranged: Ranged<T>) -> WithRange<T> {
        WithRange {
            range: self.range(&ranged),
            it: ranged.into_inner(),
        }
    }
}

// A position past the text, a line or inside a character is clamped to the nearest valid one.
impl Positions for OLC {
    fn offset_to_line_character(&self, offset: usize) -> (usize, usize) {
        OLC::offset_to_line_character(self, offset).unwrap_or_else(|e| e.bound)
    }

    fn line_character_to_offset(&self, line_character: (usize, usize)) -> usize {
        OLC::line_character_to_offset(self, line_character).unwrap_or_else(|e| e.bound)
    }
}

impl<T> WithRange<T> {
    pub fn new(it: T, range: bindings::Range) -> Self {
        Self { it, range }
    }

    pub fn as_ref(&self) -> WithRange<&T> {
        WithRange {
            it: &self.it,
            range: self.range,
        }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> WithRange<U> {
        WithRange {
            it: f(self.it),
            range: self.range,
        }
    }
}

impl<T> Deref for WithRange<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.it
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum MemberNs {
    Parameter,
    Property,
    Method,
    Index,
    Query,
    // Only reachable through the members they generate.
    Other,
}

/// A class's members by name, one map per `MemberNs`, so a same-named pair within one is a
/// conflict. Each key repeats its member's `name`, for the lookup.
#[derive(Default)]
pub(crate) struct Members {
    pub(crate) parameter: HashMap<String, WithRange<MemberDetails>>,
    pub(crate) property: HashMap<String, WithRange<MemberDetails>>,
    pub(crate) method: HashMap<String, WithRange<MemberDetails>>,
    pub(crate) index: HashMap<String, WithRange<MemberDetails>>,
    pub(crate) query: HashMap<String, WithRange<MemberDetails>>,
    /// The `MemberNs::Other` kinds, which nothing looks up by name, in source order.
    pub(crate) other: Vec<WithRange<MemberDetails>>,
    /// Members whose name an earlier one of the same namespace already took, left out of the
    /// maps for `check_cls` to report.
    pub(crate) duplicates: Vec<WithRange<MemberDetails>>,
}

impl Members {
    fn insert(&mut self, member: WithRange<MemberDetails>) {
        let map = match MemberNs::of(&member.kind) {
            MemberNs::Parameter => &mut self.parameter,
            MemberNs::Property => &mut self.property,
            MemberNs::Method => &mut self.method,
            MemberNs::Index => &mut self.index,
            MemberNs::Query => &mut self.query,
            MemberNs::Other => return self.other.push(member),
        };
        match map.entry(member.name.it.clone()) {
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(member);
            }
            std::collections::hash_map::Entry::Occupied(_) => self.duplicates.push(member),
        }
    }

    pub(crate) fn all(&self) -> impl Iterator<Item = &WithRange<MemberDetails>> {
        [
            &self.parameter,
            &self.property,
            &self.method,
            &self.index,
            &self.query,
        ]
        .into_iter()
        .flat_map(HashMap::values)
        .chain(&self.other)
        .chain(&self.duplicates)
    }

    /// Every member in source order.
    pub(crate) fn in_order(&self) -> Vec<&WithRange<MemberDetails>> {
        let mut members: Vec<_> = self.all().collect();
        members.sort_by_key(|member| (member.range.start.line, member.range.start.character));
        members
    }
}

impl MemberNs {
    // A multidimensional property is a `Property` too: `..X(subs)` reaches it once no method is
    // named `X`, and IRIS compiles a same-named method beside it.
    pub(crate) fn of(kind: &MemberKindSpecificDetails) -> Self {
        match kind {
            MemberKindSpecificDetails::Parameter { .. } => Self::Parameter,
            MemberKindSpecificDetails::Property { .. } => Self::Property,
            MemberKindSpecificDetails::MethodOrClassMethod(..)
            | MemberKindSpecificDetails::ClientMethod => Self::Method,
            MemberKindSpecificDetails::Index(_) => Self::Index,
            MemberKindSpecificDetails::Query { .. } => Self::Query,
            _ => Self::Other,
        }
    }

    pub(crate) fn of_info(kind: &bindings::MemberKind) -> Self {
        match kind {
            bindings::MemberKind::Parameter(_) => Self::Parameter,
            bindings::MemberKind::Property(_) | bindings::MemberKind::Relationship(_) => {
                Self::Property
            }
            bindings::MemberKind::Method(_)
            | bindings::MemberKind::ClassMethod(_)
            | bindings::MemberKind::ClientMethod(_) => Self::Method,
            bindings::MemberKind::Index(_) => Self::Index,
            bindings::MemberKind::Query(_) => Self::Query,
            _ => Self::Other,
        }
    }
}

pub(crate) struct ClassDetails {
    pub(crate) doc: String,
    pub(crate) name: WithRange<String>,
    pub(crate) extends: Vec<WithRange<String>>,
    // The `Import` packages, for resolving unqualified class names in this class.
    pub(crate) imports: Vec<String>,
    pub(crate) keywords: ClassKeywords,
    // What in the keyword list is no class keyword as written; `check_cls` reports it.
    pub(crate) keyword_issues: Vec<WithRange<KeywordIssue>>,
    // Forms the documentation doesn't give, which IRIS compiles; `check_cls` reports them.
    pub(crate) undocumented: Vec<WithRange<String>>,
    pub(crate) members: Members,
    pub(crate) include_names: Vec<WithRange<String>>,
    // The `Include (...)` clause's own span, for pointing a "cannot find include" diagnostic at
    // it instead of at the class's declaration -- `None` only when there's no `Include` at all,
    // in which case `include_names` is also empty and this is never read.
    pub(crate) include_range: Option<bindings::Range>,
}

// The class keywords the checker reads; the rest of the list is parsed and dropped.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ClassKeywords {
    pub(crate) abstract_: bool,
    pub(crate) class_type: Option<bindings::ClassType>,
    pub(crate) deprecated: bool,
    pub(crate) final_: bool,
    pub(crate) inheritance: Option<bindings::Inheritance>,
    pub(crate) language: Option<bindings::Language>,
    pub(crate) procedure_block: Option<bool>,
    pub(crate) property_class: Vec<String>,
    pub(crate) sql_table_name: Option<String>,
    pub(crate) sql_row_id_name: Option<String>,
}

// A keyword list entry that is no keyword of its declaration as written.
pub struct KeywordIssue {
    pub entry: String,
    pub keyword: String,
}

impl From<&cos::KeywordEntry> for KeywordIssue {
    fn from(entry: &cos::KeywordEntry) -> Self {
        Self {
            entry: entry.print(),
            keyword: entry.keyword().print(),
        }
    }
}

pub struct MemberDetails {
    pub name: WithRange<String>,
    pub doc: String,
    pub deprecated: bool,
    pub final_: bool,
    // What in its keyword list is no keyword of its kind as written; `check_cls` reports it.
    pub keyword_issues: Vec<WithRange<KeywordIssue>>,
    // Forms the documentation doesn't give, which IRIS compiles; `check_cls` reports them.
    pub undocumented: Vec<WithRange<String>>,
    pub type_names: Vec<WithRange<String>>,
    pub kind: MemberKindSpecificDetails,
}

// `Index name On keys [ keywords ]`, as declared: `bindings::IndexInfo` with the keys' ranges.
#[derive(Default)]
pub struct IndexDetails {
    // `None` without an `On` clause.
    pub on: Option<Vec<IndexKey>>,
    pub abstract_: bool,
    pub condition: Option<String>,
    pub coshard_with: Option<String>,
    pub data: Vec<String>,
    pub deferred: bool,
    pub extent: bool,
    pub id_key: bool,
    pub internal: bool,
    pub primary_key: bool,
    pub shard_key: bool,
    pub sql_name: Option<String>,
    pub type_: Option<bindings::IndexType>,
    pub unique: bool,
}

pub struct IndexKey {
    pub property: WithRange<String>,
    pub subscript: Option<bindings::IndexSubscript>,
    pub collation: Option<String>,
}

pub enum MemberKindSpecificDetails {
    MethodOrClassMethod(Box<MethodDetails>),
    Property {
        ty: Option<Ty>,
        // `[ MultiDimensional ]`: `..X(subs)` addresses a node typed like the property.
        multidimensional: bool,
        // Names from the type's own `(...)` parameter list -- e.g. %String's `MAXLEN` -- easy to
        // confuse with the `[...]` keywords (see `check::mod`'s hint when one is written there).
        // For a collection property (`list Of X(...)`), the element class `X` with its parameters.
        element_ty: Option<Ty>,
        // `[ InitialExpression = v ]` when `v` is a constant.
        initial: Option<WithRange<Const>>,
        // `[ SqlFieldName = n ]`: the column's name in place of the property's.
        sql_field_name: Option<String>,
        // A relationship's; `None` for a property.
        cardinality: Option<bindings::Cardinality>,
    },
    Parameter {
        ty: Option<Ty>,
        default: Option<String>,
        // Raw meta-type text (e.g. `"STRING"`), kept separately from `ty` since an unrecognized
        // meta-type maps `ty` to `Ty::Any`, indistinguishable from no declared type at all.
        as_type: Option<String>,
    },
    ClientMethod,
    ForeignKey,
    Index(IndexDetails),
    Projection,
    Trigger,
    XData,
    Storage,
    Query {
        normal_args: Vec<WithRange<NormalArg>>,
        variadic_arg: Option<WithRange<VariadicArg>>,
        ty: Option<Ty>,
        sql: Option<bindings::SqlProjection>,
    },
}

pub struct MethodDetails {
    pub is_class_method: bool,
    pub normal_args: Vec<WithRange<NormalArg>>,
    pub variadic_arg: Option<WithRange<VariadicArg>>,
    pub out: WithRange<Option<Ty>>,
    pub language: Option<bindings::Language>,
    pub mode: Option<cos::CodeMode>,
    pub procedure_block: ProcedureBlock,
    pub is_abstract: bool,
    pub is_private: bool,
    pub sql: Option<bindings::SqlProjection>,
    pub body: WithRange<String>,
    // Byte offset of the method's own declaration (`Method`/`ClassMethod` keyword).
    pub header_start: usize,
    // Source text from `header_start` through the body's opening `{`.
    pub header_text: String,
    pub brace_defaults: Vec<BraceDefault>,
}

// An argument default in braces, `x As %Integer = {expr}`: IRIS evaluates it on entry, in the
// method's own context, in argument order, when the argument wasn't passed. Ranged against the
// class document, like `normal_args`.
pub struct BraceDefault {
    pub name: String,
    pub value: DefaultValue,
}

pub enum DefaultValue {
    Expr(Ranged<cos::Expr>),
    // The braces' text when it isn't an expression, or `…`, a default that is no value.
    Braces(WithRange<String>),
    // No literal and no braces: the expression's text, for the checker to report.
    Unbraced(WithRange<String>),
}

pub enum RoutineElementKind {
    Subroutine,
    /// Variables auto-passed by reference via the `[a,b]` public-var list.
    Procedure {
        public_list: Vec<WithRange<String>>,
    },
}

pub struct RoutineElementDetails {
    pub name: WithRange<String>,
    // The comment lines right above the label, their markers dropped: its description, as a
    // method's `///` lines are. Filled in by `routine_elements`.
    pub doc: String,
    pub normal_args: Vec<WithRange<NormalArg>>,
    pub variadic_arg: Option<WithRange<VariadicArg>>,
    pub kind: RoutineElementKind,
    pub formal_list: bool,
}

impl RoutineElementDetails {
    // `uri` is `None` for a routine resolved via IRIS rather than the workspace -- there's no
    // real document to point a "go to definition" at.
    pub fn to_ty_procedure(&self, uri: Option<&str>) -> TyProcedure {
        let public_list = match &self.kind {
            RoutineElementKind::Subroutine => None,
            RoutineElementKind::Procedure { public_list } => {
                Some(public_list.iter().map(|name| name.it.clone()).collect())
            }
        };
        TyProcedure {
            normal_args: self
                .normal_args
                .iter()
                .map(|arg| arg.it.clone().with_declared_at(arg.range))
                .collect(),
            variadic_arg: self
                .variadic_arg
                .as_ref()
                .map(|arg| arg.it.clone().with_declared_at(arg.range)),
            out: Some(Ty::Any),
            procedure_block: public_list,
            formal_list: self.formal_list,
            uri: uri.map(str::to_string),
            declared_at: uri.is_some().then_some(self.name.range),
        }
    }
}

type Args = (Vec<WithRange<NormalArg>>, Option<WithRange<VariadicArg>>);

fn import_args(pos: &impl Positions, params: Option<&cos::SpaceThen<cos::ParameterList>>) -> Args {
    let args = params
        .into_iter()
        .flat_map(|x| &***x)
        .flat_map(|x| x.items.iter())
        .map(|entry| {
            let mode = entry.by_ref.is_some().then_some(RefMode::ByRef);
            let name = entry.param.deref().print();
            let at = entry.param.as_ref();
            if entry.expand.is_some() {
                Right(pos.with_range(at.map(|_| VariadicArg::new(mode, name, None))))
            } else {
                let default = entry.default.as_deref().map(|default| default.print());
                Left(pos.with_range(at.map(|_| NormalArg::new(mode, name, None, default))))
            }
        })
        .collect();
    split_args(args)
}

// A method's or query's `(...)` formal list, ranged against the class's whole-document `olc`.
fn formal_args(olc: &OLC, spec: Option<&cos::FormalSpec>) -> Args {
    let args = spec
        .iter()
        .flat_map(|spec| spec.formals().into_iter())
        .map(|formal| {
            if formal.variadic.is_none() {
                Left(olc.with_range(formal.as_ref().map(NormalArg::from)))
            } else {
                Right(olc.with_range(formal.as_ref().map(VariadicArg::from)))
            }
        })
        .collect();
    split_args(args)
}

// The methods IRIS generates from `Query Q`: `QFunc`, `QExecute`, ... Their shapes are fixed (see
// `check::env::query_methods`), so a class's declaration needn't carry them.
pub(crate) const QUERY_METHOD_SUFFIXES: [&str; 9] = [
    "Func",
    "Execute",
    "Fetch",
    "FetchRows",
    "Close",
    "GetInfo",
    "GetODBCInfo",
    "Prepare",
    "SendODBC",
];

// A variadic parameter can only come last.
fn split_args(mut args: Vec<Either<WithRange<NormalArg>, WithRange<VariadicArg>>>) -> Args {
    let variadic_arg = args.pop_if(|last| last.is_right()).and_then(Either::right);
    (
        args.into_iter().filter_map(Either::left).collect(),
        variadic_arg,
    )
}

fn public_list(
    pos: &impl Positions,
    publics: &Option<Prefix<StringOf<cos::IsWhitespace>, AllowMacro<cos::PublicVarList>>>,
) -> Vec<WithRange<String>> {
    publics
        .iter()
        .filter_map(|publics| match publics.deref() {
            AllowMacro::Plain(publics) => publics.deref().as_ref(),
            AllowMacro::Macro(..) => None,
        })
        .flat_map(|x| x.items.iter())
        .filter_map(|x| match x.deref() {
            AllowMacro::Plain(name) => Some(pos.with_range(x.as_ref().map(|_| name.print()))),
            AllowMacro::Macro(..) => None,
        })
        .collect()
}

pub fn routine_element_kind(
    pos: &impl Positions,
    element: &cos::RoutineElement,
) -> Option<RoutineElementDetails> {
    let (label, params, kind) = match element {
        cos::RoutineElement::Subroutine(subroutine) => {
            let AllowMacro::Plain(header) = &subroutine.header else {
                return None;
            };
            let params = header.params.as_ref();
            (&header.label, params, RoutineElementKind::Subroutine)
        }
        cos::RoutineElement::Procedure(procedure) => {
            let AllowMacro::Plain(header) = procedure.header.deref() else {
                return None;
            };
            let kind = RoutineElementKind::Procedure {
                public_list: public_list(pos, &header.publics),
            };
            (&header.label, Some(&header.params), kind)
        }
    };
    let (normal_args, variadic_arg) = import_args(pos, params);
    Some(RoutineElementDetails {
        name: with_range_name(label, pos),
        doc: String::new(),
        kind,
        normal_args,
        variadic_arg,
        formal_list: params.is_some(),
    })
}

// A procedure's `Ranged<Brace<…>>` end lies past the `}`.
pub(crate) fn routine_element_end(element: &cos::RoutineElement) -> usize {
    match element {
        cos::RoutineElement::Subroutine(subroutine) => subroutine.body.end,
        cos::RoutineElement::Procedure(procedure) => procedure.body.end,
    }
}

/// The routine's labels in order, each with its details, description included.
pub fn routine_elements<'r>(
    pos: &impl Positions,
    subroutines: Option<&'r cos::Subroutines>,
) -> Vec<(&'r cos::RoutineElement, Option<RoutineElementDetails>)> {
    let Some(subroutines) = subroutines else {
        return vec![];
    };
    let gaps = std::iter::once(&subroutines.gap).chain(&subroutines.seps);
    (subroutines.items.iter().zip(gaps))
        .map(|(element, gap)| {
            let details = routine_element_kind(pos, element).map(|mut details| {
                details.doc = label_doc(&gap.print());
                details
            });
            (element, details)
        })
        .collect()
}

// The trailing comment lines of the text before a label, their markers dropped.
fn label_doc(gap: &str) -> String {
    let mut lines: Vec<&str> = (gap.lines().rev())
        .map_while(|line| {
            let line = line.trim_start();
            ["##;", "#;", "//", ";"]
                .iter()
                .find_map(|marker| line.strip_prefix(marker))
        })
        .collect();
    lines.reverse();
    lines.join("\n")
}

pub(crate) fn with_range_name<T: Grammar>(
    ranged: &Ranged<T>,
    pos: &impl Positions,
) -> WithRange<String> {
    pos.with_range(ranged.as_ref().map(|x| x.print()))
}

// A member's name without the quotes of a quoted one: `Method "_isLabeled"()` declares `_isLabeled`.
fn type_names(member: &cos::Member, olc: &OLC) -> Vec<WithRange<String>> {
    let class = |class_name: &Ranged<cos::SignatureType>| match class_name.deref() {
        cos::SignatureType::Class(name) => {
            Some(WithRange::new(name.print(), olc.range(class_name)))
        }
        cos::SignatureType::Parameter(..) => None,
    };
    let of_type = |ty: Option<&cos::Type>| ty.and_then(|t| class(&t.class_name));
    let formals = |spec: Option<&cos::FormalSpec>| -> Vec<WithRange<String>> {
        (spec.into_iter().flat_map(cos::FormalSpec::formals))
            .filter_map(|formal| of_type(formal.as_type.as_deref()))
            .collect()
    };
    match member {
        cos::Member::Property(
            cos::Property::Simple { as_type, .. } | cos::Property::Relationship { as_type, .. },
        ) => (as_type.as_ref().and_then(|t| t.as_ref().left()))
            .and_then(|t| class(&t.ty.class_name))
            .into_iter()
            .collect(),
        cos::Member::Method(method) => of_type(method.header.return_type.deref().as_deref())
            .into_iter()
            .chain(formals(method.header.formal_spec.as_ref()))
            .collect(),
        cos::Member::Query(query) => of_type(query.return_type.as_deref())
            .into_iter()
            .chain(formals(query.formal_spec.as_ref()))
            .collect(),
        _ => vec![],
    }
}

fn with_range_member_name(
    name: &cos::QuotableClassMemberName,
    pos: &impl Positions,
) -> WithRange<String> {
    pos.with_range(name.0.as_ref().map(|_| name.name()))
}

fn extract_document_comments(doc: &str) -> String {
    doc.split('\n')
        .filter_map(|line| {
            let line = line.trim_start();
            line.strip_prefix("///")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// The entries of a keyword list that are its declaration's keywords.
fn typed<K: Grammar>(
    keywords: Option<&KeywordList<FailableKeyword<K>>>,
) -> impl Iterator<Item = &K> {
    keywords
        .into_iter()
        .flat_map(KeywordList::items)
        .filter_map(|entry| (***entry).as_ref().left())
}

// The entries that are no keyword of the declaration as written, at their name's range.
fn keyword_issues_in<K: Grammar>(
    keywords: Option<&KeywordList<FailableKeyword<K>>>,
    olc: &OLC,
) -> Vec<WithRange<KeywordIssue>> {
    keywords
        .into_iter()
        .flat_map(KeywordList::items)
        .filter_map(|entry| (***entry).as_ref().right())
        .map(|entry| WithRange::new(KeywordIssue::from(entry), olc.range(&entry.keyword().0)))
        .collect()
}

fn is_deprecated<K: Grammar + HasMemberKeyword>(
    keywords: Option<&KeywordList<FailableKeyword<K>>>,
) -> bool {
    typed(keywords).any(|keyword| {
        matches!(keyword.member(), Some(cos::MemberKeyword::Deprecated(flag)) if flag.is_set())
    })
}

fn is_final(member: &cos::Member) -> bool {
    match member {
        cos::Member::Method(m) => typed(m.keywords.as_ref())
            .any(|k| matches!(k, cos::MethodKeyword::Final(flag) if flag.is_set())),
        cos::Member::Property(
            cos::Property::Simple { keywords, .. } | cos::Property::Relationship { keywords, .. },
        ) => typed(keywords.as_ref())
            .any(|k| matches!(k, cos::PropertyKeyword::Final(flag) if flag.is_set())),
        cos::Member::Parameter(p) => typed(p.keywords.as_ref())
            .any(|k| matches!(k, cos::ParameterKeyword::Final(flag) if flag.is_set())),
        _ => false,
    }
}

type Header = (WithRange<String>, bool, Vec<WithRange<KeywordIssue>>);

// A member's name, whether it or its class is deprecated, and its keyword list's issues.
fn header(member: &cos::Member, deprecated: bool, olc: &OLC) -> Header {
    fn of<K: Grammar + HasMemberKeyword>(
        name: &cos::QuotableClassMemberName,
        keywords: Option<&KeywordList<FailableKeyword<K>>>,
        deprecated: bool,
        olc: &OLC,
    ) -> Header {
        (
            with_range_member_name(name, olc),
            deprecated || is_deprecated(keywords),
            keyword_issues_in(keywords, olc),
        )
    }
    match member {
        cos::Member::ForeignKey(m) => of(&m.name, m.keywords.as_ref(), deprecated, olc),
        cos::Member::Index(m) => of(&m.name, m.keywords.as_ref(), deprecated, olc),
        cos::Member::Method(m) => of(&m.name, m.keywords.as_ref(), deprecated, olc),
        cos::Member::Parameter(m) => of(&m.name, m.keywords.as_ref(), deprecated, olc),
        cos::Member::Projection(m) => of(&m.name, m.keywords.as_ref(), deprecated, olc),
        cos::Member::Property(m) => of(m.name(), m.keywords().as_ref(), deprecated, olc),
        cos::Member::Query(m) => of(&m.name, m.keywords.as_ref(), deprecated, olc),
        cos::Member::Storage(m) => (with_range_member_name(&m.name, olc), deprecated, vec![]),
        cos::Member::Trigger(m) => of(&m.name, m.keywords.as_ref(), deprecated, olc),
        cos::Member::XData(m) => of(m.name(), m.keywords(), deprecated, olc),
    }
}

// `None` for a standard index (`index`).
pub(crate) fn index_type(type_: &cos::IndexType) -> Option<bindings::IndexType> {
    Some(match type_ {
        cos::IndexType::Bitmap(_) => bindings::IndexType::Bitmap,
        cos::IndexType::Bitslice(_) => bindings::IndexType::Bitslice,
        cos::IndexType::CollatedKey(_) => bindings::IndexType::CollatedKey,
        cos::IndexType::Columnar(_) => bindings::IndexType::Columnar,
        cos::IndexType::Key(_) => bindings::IndexType::Key,
        cos::IndexType::Index(_) => return None,
    })
}

// A `(a, b)` or single-item list value.
fn class_list_text(value: &str) -> Vec<String> {
    let value = value.trim();
    let value = value
        .strip_prefix('(')
        .and_then(|v| v.strip_suffix(')'))
        .unwrap_or(value);
    crate::atelier::split_classes(value)
}

// `[ InitialExpression = v ]` with `v` a constant, a quoted string or a number; an expression in
// braces is none.
fn initial_expression(
    keywords: Option<&KeywordList<FailableKeyword<cos::PropertyKeyword>>>,
    olc: &OLC,
) -> Option<WithRange<Const>> {
    (keywords.into_iter().flat_map(KeywordList::items)).find_map(|entry| {
        let cos::PropertyKeyword::InitialExpression(valued) = (***entry).as_ref().left()? else {
            return None;
        };
        let constant = Const::from_literal(&valued.value.print())?;
        Some(WithRange::new(constant, olc.range(entry)))
    })
}

impl ClassDetails {
    fn namespace(&self, ns: MemberNs) -> Option<&HashMap<String, WithRange<MemberDetails>>> {
        let members = &self.members;
        Some(match ns {
            MemberNs::Parameter => &members.parameter,
            MemberNs::Property => &members.property,
            MemberNs::Method => &members.method,
            MemberNs::Index => &members.index,
            MemberNs::Query => &members.query,
            MemberNs::Other => return None,
        })
    }

    pub(crate) fn member(&self, ns: MemberNs, name: &str) -> Option<&WithRange<MemberDetails>> {
        self.namespace(ns)?.get(name)
    }

    pub(crate) fn names(&self, ns: MemberNs) -> impl Iterator<Item = &String> {
        self.namespace(ns).into_iter().flat_map(HashMap::keys)
    }
}

impl TryFrom<&str> for ClassDetails {
    type Error = tygr::Error;

    fn try_from(src: &str) -> Result<ClassDetails, Self::Error> {
        let cls = cos::ClassDefinition::parse(src)?;
        let olc = OLC::new(src);
        let gaps = std::iter::once(&cls.gap_start).chain(cls.directives.iter().map(|d| d.gap()));
        let doc = extract_document_comments(&gaps.map(|gap| gap.0.as_str()).collect::<String>());
        let name = with_range_name(&cls.name, &olc);
        let extends_list = cls.extends.as_ref().map(|e| &e.class_list);
        let import_list = cls.import().map(|i| &i.packages);
        let include_list = cls.include().map(|i| &*i.files);
        let extends = (extends_list.into_iter().flat_map(cos::ClassList::classes))
            .map(|class| with_range_name(class, &olc))
            .collect();
        let imports = (import_list.into_iter().flat_map(cos::ClassList::classes))
            .map(|class| class.print())
            .collect();
        // `IncludeGenerator` is only for `[ CodeMode = generator ]` methods, whose bodies are
        // code that *writes* code at compile time -- ascot doesn't check those as ObjectScript
        // at all (see `MethodBody::check`), so resolving its macros would only produce spurious
        // "cannot find include" diagnostics for something nothing here actually uses.
        let include_names = (include_list.into_iter().flat_map(cos::ClassList::classes))
            .map(|class| with_range_name(class, &olc))
            .collect();
        let include_range = cls.include().map(|include| olc.range(&include.files));
        let mut keywords = ClassKeywords::default();
        for keyword in typed(cls.keyword_list.as_ref()) {
            use cos::ClassKeyword::*;
            match keyword {
                Abstract(flag) => keywords.abstract_ = flag.is_set(),
                ClassType(v) => {
                    keywords.class_type = v.value.as_ref().left().map(bindings::ClassType::from)
                }
                Deprecated(flag) => keywords.deprecated = flag.is_set(),
                Final(flag) => keywords.final_ = flag.is_set(),
                Inheritance(v) => keywords.inheritance = Some((&v.value).into()),
                Language(v) => keywords.language = Some((&v.value).into()),
                ProcedureBlock(flag) => keywords.procedure_block = Some(flag.is_set()),
                PropertyClass(v) => {
                    keywords.property_class = v.value.classes().iter().map(|c| c.print()).collect()
                }
                SqlTableName(v) => keywords.sql_table_name = Some(sql_name(&v.value)),
                SqlRowIdName(v) => keywords.sql_row_id_name = Some(sql_name(&v.value)),
                _ => {}
            }
        }
        let keyword_issues = keyword_issues_in(cls.keyword_list.as_ref(), &olc);
        let lists = [
            ("Extends", extends_list),
            ("Import", import_list),
            ("Include", include_list),
        ];
        let mut undocumented = Vec::new();
        for (clause, list) in lists {
            for (start, end) in list.iter().flat_map(|list| list.empty_entries()) {
                let what = format!("An empty entry in {clause}");
                undocumented.push(WithRange::new(what, olc.span(start, end)));
            }
        }
        let class_procedure_block = keywords.procedure_block.unwrap_or(true);
        let mut members = Members::default();
        for (before_member, member) in cls.member_list.members.into_item_sep() {
            let doc = extract_document_comments(before_member.0.as_str());
            let range = olc.range(&member);
            let undocumented_member = undocumented_in(&member, &olc);
            let type_names = type_names(&member, &olc);
            let final_ = is_final(&member);
            let (name, deprecated, keyword_issues) = header(&member, keywords.deprecated, &olc);
            let kind = process_member(&olc, src, member, class_procedure_block);
            members.insert(WithRange {
                it: MemberDetails {
                    name,
                    doc,
                    deprecated,
                    final_,
                    keyword_issues,
                    undocumented: undocumented_member,
                    type_names,
                    kind,
                },
                range,
            });
        }
        Ok(Self {
            doc,
            name,
            extends,
            keywords,
            keyword_issues,
            undocumented,
            members,
            include_names,
            imports,
            include_range,
        })
    }
}

// A form of the member the documentation doesn't give, which IRIS compiles: `list Of` before a
// relationship's class, which it ignores.
fn undocumented_in(member: &Ranged<cos::Member>, olc: &OLC) -> Vec<WithRange<String>> {
    match member.deref() {
        cos::Member::Property(cos::Property::Relationship {
            as_type: Some(Left(as_type)),
            ..
        }) if let Some(collection) = &as_type.collection => vec![WithRange::new(
            format!(
                "`{}` before a relationship's class",
                collection.print().trim()
            ),
            olc.range(member),
        )],
        _ => vec![],
    }
}

fn process_member(
    olc: &OLC,
    src: &str,
    member: Ranged<cos::Member>,
    class_procedure_block: bool,
) -> MemberKindSpecificDetails {
    let member_start = member.start;
    match member.into_inner() {
        cos::Member::ForeignKey(_) => MemberKindSpecificDetails::ForeignKey,
        cos::Member::Index(index) => {
            let mut details = IndexDetails {
                on: index.on_properties().map(|keys| {
                    keys.into_iter()
                        .map(|key| IndexKey {
                            property: olc.with_range(key.name.0.as_ref().map(|_| key.property())),
                            subscript: key.kind.as_ref().map(|kind| match kind {
                                cos::IndexPropertyKind::Keys(_) => bindings::IndexSubscript::Keys,
                                cos::IndexPropertyKind::Elements(_) => {
                                    bindings::IndexSubscript::Elements
                                }
                            }),
                            collation: key.collation(),
                        })
                        .collect()
                }),
                ..Default::default()
            };
            for keyword in typed(index.keywords.as_ref()) {
                use cos::IndexKeyword::*;
                match keyword {
                    Member(cos::MemberKeyword::Internal(flag)) => details.internal = flag.is_set(),
                    Member(_) => {}
                    Abstract(flag) => details.abstract_ = flag.is_set(),
                    Condition(v) => {
                        let condition = v.value.print();
                        let condition = condition.trim();
                        details.condition = Some(
                            condition
                                .strip_prefix('{')
                                .and_then(|c| c.strip_suffix('}'))
                                .unwrap_or(condition)
                                .trim()
                                .to_string(),
                        )
                    }
                    CoshardWith(v) => details.coshard_with = Some(v.value.print()),
                    Data(v) => details.data = class_list_text(&v.value.print()),
                    Deferred(flag) => details.deferred = flag.is_set(),
                    Extent(flag) => details.extent = flag.is_set(),
                    IdKey(flag) => details.id_key = flag.is_set(),
                    PrimaryKey(flag) => details.primary_key = flag.is_set(),
                    ShardKey(flag) => details.shard_key = flag.is_set(),
                    SqlName(v) => details.sql_name = Some(v.value.print()),
                    Type(v) => details.type_ = index_type(&v.value),
                    Unique(flag) => details.unique = flag.is_set(),
                }
            }
            MemberKindSpecificDetails::Index(details)
        }
        cos::Member::Method(method) => {
            let is_class_method = match &method.keyword {
                cos::MethodKind::Method(_) => false,
                cos::MethodKind::ClassMethod(_) => true,
                cos::MethodKind::ClientMethod(_) => return MemberKindSpecificDetails::ClientMethod,
            };
            method_kind(
                olc,
                src,
                member_start,
                method,
                is_class_method,
                class_procedure_block,
            )
        }
        cos::Member::Parameter(parameter) => {
            let as_type = parameter.as_type.as_deref().map(Print::print);
            let ty = as_type.as_deref().map(parameter_ty);
            let default = parameter.default.as_ref().map(|v| v.deref().print());
            MemberKindSpecificDetails::Parameter {
                ty,
                default,
                as_type,
            }
        }
        cos::Member::Projection(_) => MemberKindSpecificDetails::Projection,
        cos::Member::Property(property) => match property {
            cos::Property::Simple {
                as_type,
                keywords,
                ..
            } => MemberKindSpecificDetails::Property {
                ty: as_type.as_ref().map(|t| match t {
                    Left(t) => Ty::from(&**t),
                    Right(params) => with_params(Ty::from("%Library.String"), Some(params)),
                }),
                element_ty: (as_type.as_ref().and_then(|t| t.as_ref().left()))
                    .filter(|t| t.collection.is_some())
                    .map(|t| Ty::from(&t.ty)),
                initial: initial_expression(keywords.as_ref(), olc),
                multidimensional: typed(keywords.as_ref()).any(|keyword| {
                    matches!(keyword, cos::PropertyKeyword::MultiDimensional(flag) if flag.is_set())
                }),
                sql_field_name: sql_field_name(keywords.as_ref()),
                cardinality: None,
            },
            cos::Property::Relationship {
                as_type,
                keywords,
                ..
            } => {
                let class = as_type.as_ref().and_then(|t| t.as_ref().left()).map(|t| &**t);
                // A `many`/`children` relationship is the collection of the related objects.
                let cardinality = typed(keywords.as_ref()).find_map(|keyword| match keyword {
                    cos::PropertyKeyword::Cardinality(c) => Some((&c.value).into()),
                    _ => None,
                });
                let is_collection = matches!(
                    cardinality,
                    Some(bindings::Cardinality::Many | bindings::Cardinality::Children)
                );
                MemberKindSpecificDetails::Property {
                    ty: if is_collection {
                        Some(crate::check::types::relationship_object(
                            class.map(|t| t.ty.class_name.print()),
                        ))
                    } else {
                        class.map(|t| Ty::from(t.ty.class_name.print()))
                    },
                    element_ty: None,
                    initial: None,
                    multidimensional: false,
                    sql_field_name: sql_field_name(keywords.as_ref()),
                    cardinality,
                }
            }
        },
        cos::Member::Query(query) => {
            let (normal_args, variadic_arg) = formal_args(olc, query.formal_spec.as_ref());
            MemberKindSpecificDetails::Query {
                normal_args,
                variadic_arg,
                ty: query.return_type.as_deref().map(Ty::from),
                sql: {
                    let mut sql = bindings::SqlProjection::default();
                    for keyword in typed(query.keywords.as_ref()) {
                        match keyword {
                            cos::QueryKeyword::SqlProc(flag) => sql.procedure = flag.is_set(),
                            cos::QueryKeyword::SqlName(v) => sql.name = Some(sql_name(&v.value)),
                            cos::QueryKeyword::SqlView(flag) => sql.view = flag.is_set(),
                            cos::QueryKeyword::SqlViewName(v) => {
                                sql.view_name = Some(sql_name(&v.value))
                            }
                            _ => {}
                        }
                    }
                    Some(sql).filter(|sql| *sql != bindings::SqlProjection::default())
                },
            }
        }
        cos::Member::Storage(_) => MemberKindSpecificDetails::Storage,
        cos::Member::Trigger(_) => MemberKindSpecificDetails::Trigger,
        cos::Member::XData(_) => MemberKindSpecificDetails::XData,
    }
}

fn method_kind(
    olc: &OLC,
    src: &str,
    header_start: usize,
    mut method: cos::Method,
    is_class_method: bool,
    class_procedure_block: bool,
) -> MemberKindSpecificDetails {
    let body_start = method.body.wrapped.start;
    // `normal_args`/`out` stay ranged against the class's whole-document `olc`, like any other
    // member -- other methods' call-site diagnostics reference these same ranges directly.
    let header_text = &src[header_start..body_start];
    let (normal_args, variadic_arg) = formal_args(olc, method.formal_spec.as_ref());
    let brace_defaults = (method.header.formal_spec.take().into_iter())
        .flat_map(cos::FormalSpec::into_formals)
        .filter_map(|formal| {
            let formal = formal.into_inner();
            let name = formal.name.print();
            let default = formal.default?.prefixed;
            let at = olc.range(&default);
            let value = match default.into_inner() {
                cos::FormalDefaultValue::Brace { expr, .. } => DefaultValue::Expr(expr),
                cos::FormalDefaultValue::Braces { content, .. } => {
                    let range = olc.range(&content);
                    DefaultValue::Braces(WithRange::new(content.print(), range))
                }
                cos::FormalDefaultValue::Elided(_) => {
                    DefaultValue::Braces(WithRange::new("…".to_string(), at))
                }
                cos::FormalDefaultValue::Literal(..) => return None,
                cos::FormalDefaultValue::Expression(expr) => {
                    DefaultValue::Unbraced(WithRange::new(expr.print(), at))
                }
            };
            Some(BraceDefault { name, value })
        })
        .collect();
    let out = method
        .return_type
        .as_ref()
        .map(|x| x.as_deref().map(Ty::from));
    let out = olc.with_range(out);
    let mut language = None;
    let mut mode = None;
    let mut procedure_block = None;
    let mut public_list = Vec::new();
    let mut is_abstract = false;
    let mut is_private = false;
    let mut sql = bindings::SqlProjection::default();
    let keywords = method.header.keywords.take().into_iter();
    for entry in keywords
        .flat_map(|list| list.entries)
        .flat_map(|entries| entries.items)
    {
        use cos::MethodKeyword::*;
        let range = olc.range(&entry);
        match entry.into_inner().into_inner() {
            Left(Language(v)) => language = Some((&v.value).into()),
            Left(CodeMode(v)) => mode = Some(v.value),
            Left(ProcedureBlock(v)) => procedure_block = Some(v.value.is_set()),
            // Each name at the keyword's own range.
            Left(PublicList(v)) => {
                public_list = class_list_text(&v.value.print())
                    .into_iter()
                    .map(|name| WithRange::new(name, range))
                    .collect()
            }
            Left(Abstract(flag)) => is_abstract = flag.is_set(),
            Left(Private(flag)) => is_private = flag.is_set(),
            Left(SqlProc(flag)) => sql.procedure = flag.is_set(),
            Left(SqlName(v)) => sql.name = Some(sql_name(&v.value)),
            _ => {}
        }
    }
    let procedure_block = procedure_block
        .unwrap_or(class_procedure_block)
        .then_some(public_list);
    let raw_body = method.body.into_inner().into_inner();
    let body = olc.with_range(raw_body).map(|x| x.print());
    MemberKindSpecificDetails::MethodOrClassMethod(Box::new(MethodDetails {
        is_class_method,
        normal_args,
        variadic_arg,
        out,
        body,
        header_start,
        header_text: header_text.to_string(),
        brace_defaults,
        language,
        mode,
        procedure_block,
        is_abstract,
        is_private,
        sql: Some(sql).filter(|sql| *sql != bindings::SqlProjection::default()),
    }))
}

// A SQL name as a keyword value: bare, or in quotes.
fn sql_name(value: &cos::KeywordValue) -> String {
    value.print().trim_matches('"').to_string()
}

fn sql_field_name(
    keywords: Option<&cos::KeywordList<cos::FailableKeyword<cos::PropertyKeyword>>>,
) -> Option<String> {
    typed(keywords).find_map(|keyword| match keyword {
        cos::PropertyKeyword::SqlFieldName(v) => Some(sql_name(&v.value)),
        _ => None,
    })
}
