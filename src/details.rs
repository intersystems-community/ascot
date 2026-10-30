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
use tygr::{Grammar, IntoInner, OLC, Prefix, Range, StringOf};

use crate::bindings::{self, ArgMode};
use crate::check::types::{NormalArg, ProcedureBlock, Ty, TyProcedure, VariadicArg, parameter_ty};
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

    fn range<T>(&self, ranged: &Range<T>) -> bindings::Range {
        self.span(ranged.start, ranged.end)
    }

    fn with_range<T>(&self, ranged: Range<T>) -> WithRange<T> {
        WithRange {
            range: self.range(&ranged),
            it: ranged.into_inner(),
        }
    }
}

impl Positions for OLC {
    fn offset_to_line_character(&self, offset: usize) -> (usize, usize) {
        OLC::offset_to_line_character(self, offset)
    }

    fn line_character_to_offset(&self, line_character: (usize, usize)) -> usize {
        OLC::line_character_to_offset(self, line_character)
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
            MemberKindSpecificDetails::Query => Self::Query,
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
            bindings::MemberKind::Query => Self::Query,
            _ => Self::Other,
        }
    }
}

pub(crate) struct ClassDetails {
    pub(crate) doc: String,
    pub(crate) name: WithRange<String>,
    pub(crate) extends: Vec<String>,
    // The `Import` packages, for resolving unqualified class names in this class.
    pub(crate) imports: Vec<String>,
    pub(crate) keywords: ClassKeywords,
    // What in the keyword list is no class keyword as written; `check_cls` reports it.
    pub(crate) keyword_issues: Vec<WithRange<KeywordIssue>>,
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
    pub(crate) class_type: Option<bindings::ClassType>,
    pub(crate) deprecated: bool,
    pub(crate) inheritance: Option<bindings::Inheritance>,
    pub(crate) language: Option<bindings::Language>,
    pub(crate) procedure_block: Option<bool>,
    pub(crate) property_class: Vec<String>,
}

// A keyword list entry that is no keyword of its declaration as written.
pub struct KeywordIssue {
    pub entry: String,
    pub keyword: String,
    // `Name = ` with nothing after, as in IRIS's own exports.
    pub without_value: bool,
}

impl From<&cos::KeywordEntry> for KeywordIssue {
    fn from(entry: &cos::KeywordEntry) -> Self {
        Self {
            entry: entry.print(),
            keyword: entry.keyword().print(),
            without_value: matches!(entry, cos::KeywordEntry::Assign { value: None, .. }),
        }
    }
}

pub struct MemberDetails {
    pub name: WithRange<String>,
    pub doc: String,
    pub deprecated: bool,
    // What in its keyword list is no keyword of its kind as written; `check_cls` reports it.
    pub keyword_issues: Vec<WithRange<KeywordIssue>>,
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
        type_param_names: Vec<String>,
        // For a collection property, the element class (see `element_ty`).
        element_ty: Option<Ty>,
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
    Query,
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
    pub body: WithRange<String>,
    // Byte offset of the method's own declaration (`Method`/`ClassMethod` keyword).
    pub header_start: usize,
    // Source text from `header_start` through the body's opening `{`.
    pub header_text: String,
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

fn import_args(pos: &impl Positions, params: &Option<cos::SpaceThen<cos::ParameterList>>) -> Args {
    let args = params
        .iter()
        .flat_map(|x| &***x)
        .flat_map(|x| x.items.iter())
        .map(|entry| {
            let mode = if entry.by_ref.is_some() {
                ArgMode::ByRef
            } else {
                ArgMode::Default
            };
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

// A variadic parameter can only come last.
fn split_args(mut args: Vec<Either<WithRange<NormalArg>, WithRange<VariadicArg>>>) -> Args {
    let variadic_arg = if args.last().is_some_and(Either::is_right) {
        args.pop().and_then(Either::right)
    } else {
        None
    };
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

fn element(
    pos: &impl Positions,
    label: &Range<cos::Label>,
    params: &Option<cos::SpaceThen<cos::ParameterList>>,
    kind: RoutineElementKind,
) -> RoutineElementDetails {
    let (normal_args, variadic_arg) = import_args(pos, params);
    RoutineElementDetails {
        name: with_range_name(label, pos),
        kind,
        normal_args,
        variadic_arg,
        formal_list: params.is_some(),
    }
}

pub(crate) fn subroutine_kind(
    pos: &impl Positions,
    subroutine: &cos::Subroutine,
) -> Option<RoutineElementDetails> {
    let AllowMacro::Plain(header) = &subroutine.header else {
        return None;
    };
    Some(element(
        pos,
        &header.label,
        &header.params,
        RoutineElementKind::Subroutine,
    ))
}

fn procedure_kind(
    pos: &impl Positions,
    procedure: &cos::Procedure,
) -> Option<RoutineElementDetails> {
    let AllowMacro::Plain(header) = procedure.header.deref() else {
        return None;
    };
    let kind = RoutineElementKind::Procedure {
        public_list: public_list(pos, &header.publics),
    };
    Some(element(pos, &header.label, &header.params, kind))
}

pub fn routine_element_kind(
    pos: &impl Positions,
    element: &cos::RoutineElement,
) -> Option<RoutineElementDetails> {
    match element {
        cos::RoutineElement::Subroutine(subroutine) => subroutine_kind(pos, subroutine),
        cos::RoutineElement::Procedure(procedure) => procedure_kind(pos, procedure),
    }
}

pub(crate) fn with_range_name<T: Grammar>(
    ranged: &Range<T>,
    pos: &impl Positions,
) -> WithRange<String> {
    pos.with_range(ranged.as_ref().map(|x| x.print()))
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
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

// `list Of X` / `array Of X`: the element class `X`, which is what the type parameters describe.
fn element_ty(property_type: &cos::PropertyType) -> Option<Ty> {
    property_type.collection.as_ref()?;
    property_type
        .class_name
        .as_ref()
        .map(|class_name| Ty::from(class_name.print()))
}

fn type_param_names(type_params: &Option<cos::TypeParamList>) -> Vec<String> {
    type_params
        .iter()
        .flat_map(|params| params.deref().items.iter())
        .map(|param| param.name.print())
        .collect()
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
        let mut doc = String::new();
        doc.push_str(cls.gap_start.0.as_str());
        if let Some(import) = &cls.import {
            doc.push_str(import.gap.0.as_str());
        }
        if let Some(import) = &cls.include {
            doc.push_str(import.gap.0.as_str());
        }
        if let Some(import) = &cls.include_gen {
            doc.push_str(import.gap.0.as_str());
        }
        let doc = extract_document_comments(&doc);
        let name = with_range_name(&cls.name, &olc);
        let extends = cls.extends.as_ref().map_or(Vec::default(), |extends| {
            extends
                .class_list
                .classes()
                .into_iter()
                .map(|class| class.print())
                .collect()
        });
        let imports = cls.import.as_ref().map_or(Vec::default(), |import| {
            import
                .packages
                .classes()
                .into_iter()
                .map(|class| class.print())
                .collect()
        });
        // `IncludeGenerator` is only for `[ CodeMode = generator ]` methods, whose bodies are
        // code that *writes* code at compile time -- ascot doesn't check those as ObjectScript
        // at all (see `MethodBody::check`), so resolving its macros would only produce spurious
        // "cannot find include" diagnostics for something nothing here actually uses.
        let include_names = cls.include.as_ref().map_or(Vec::default(), |include| {
            include
                .files
                .classes()
                .into_iter()
                .map(|c| WithRange::new(c.print(), olc.range(c)))
                .collect()
        });
        let include_range = cls
            .include
            .as_ref()
            .map(|include| olc.range(&include.files));
        let mut keywords = ClassKeywords::default();
        for keyword in typed(cls.keyword_list.as_ref()) {
            use cos::ClassKeyword::*;
            match keyword {
                ClassType(v) => keywords.class_type = Some((&v.value).into()),
                Deprecated(flag) => keywords.deprecated = flag.is_set(),
                Inheritance(v) => keywords.inheritance = Some((&v.value).into()),
                Language(v) => keywords.language = Some((&v.value).into()),
                ProcedureBlock(flag) => keywords.procedure_block = Some(flag.is_set()),
                PropertyClass(v) => {
                    keywords.property_class = v.value.classes().iter().map(|c| c.print()).collect()
                }
                _ => {}
            }
        }
        let keyword_issues = keyword_issues_in(cls.keyword_list.as_ref(), &olc);
        let class_procedure_block = keywords.procedure_block.unwrap_or(true);
        let mut members = Members::default();
        for (before_member, member) in cls.member_list.members.into_item_sep() {
            let doc = extract_document_comments(before_member.0.as_str());
            let range = olc.range(&member);
            let (name, deprecated, keyword_issues, kind) = process_member(
                &olc,
                src,
                keywords.deprecated,
                member,
                class_procedure_block,
            );
            members.insert(WithRange {
                it: MemberDetails {
                    name,
                    doc,
                    deprecated,
                    keyword_issues,
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
            members,
            include_names,
            imports,
            include_range,
        })
    }
}

fn process_member(
    olc: &OLC,
    src: &str,
    deprecated: bool,
    member: Range<cos::Member>,
    class_procedure_block: bool,
) -> (
    WithRange<String>,
    bool,
    Vec<WithRange<KeywordIssue>>,
    MemberKindSpecificDetails,
) {
    let member_start = member.start;
    match member.into_inner() {
        cos::Member::ForeignKey(fk) => (
            with_range_name(&fk.name, olc),
            deprecated || is_deprecated(fk.keywords.as_ref()),
            keyword_issues_in(fk.keywords.as_ref(), olc),
            MemberKindSpecificDetails::ForeignKey,
        ),
        cos::Member::Index(index) => {
            let mut details = IndexDetails {
                on: index.on_properties().map(|keys| {
                    keys.into_iter()
                        .map(|key| IndexKey {
                            property: with_range_name(&key.name, olc),
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
            (
                with_range_name(&index.name, olc),
                deprecated || is_deprecated(index.keywords.as_ref()),
                keyword_issues_in(index.keywords.as_ref(), olc),
                MemberKindSpecificDetails::Index(details),
            )
        }
        cos::Member::Method(method) => {
            let name = with_range_name(&method.name, olc);
            let deprecated = deprecated || is_deprecated(method.keywords.as_ref());
            let keyword_issues = keyword_issues_in(method.keywords.as_ref(), olc);
            let is_class_method = match &method.keyword {
                cos::MethodKind::Method(_) => false,
                cos::MethodKind::ClassMethod(_) => true,
                cos::MethodKind::ClientMethod(_) => {
                    return (
                        name,
                        deprecated,
                        keyword_issues,
                        MemberKindSpecificDetails::ClientMethod,
                    );
                }
            };
            let kind = method_kind(
                olc,
                src,
                member_start,
                method,
                is_class_method,
                class_procedure_block,
            );
            (name, deprecated, keyword_issues, kind)
        }
        cos::Member::Parameter(parameter) => {
            let name = with_range_name(&parameter.name, olc);
            let as_type = parameter.as_type.as_deref().map(Grammar::print);
            let ty = as_type.as_deref().map(parameter_ty);
            let default = parameter.default.as_ref().map(|v| v.deref().print());
            (
                name,
                deprecated || is_deprecated(parameter.keywords.as_ref()),
                keyword_issues_in(parameter.keywords.as_ref(), olc),
                MemberKindSpecificDetails::Parameter {
                    ty,
                    default,
                    as_type,
                },
            )
        }
        cos::Member::Projection(projection) => (
            with_range_name(&projection.name, olc),
            deprecated || is_deprecated(projection.keywords.as_ref()),
            keyword_issues_in(projection.keywords.as_ref(), olc),
            MemberKindSpecificDetails::Projection,
        ),
        cos::Member::Property(property) => match property {
            cos::Property::Simple {
                name,
                as_type,
                type_params,
                keywords,
                ..
            } => (
                olc.with_range(name.0.as_ref().map(|_| name.name())),
                deprecated || is_deprecated(keywords.as_ref()),
                keyword_issues_in(keywords.as_ref(), olc),
                MemberKindSpecificDetails::Property {
                    ty: as_type.as_deref().map(Ty::from),
                    element_ty: as_type.as_deref().and_then(element_ty),
                    multidimensional: typed(keywords.as_ref()).any(|keyword| {
                        matches!(keyword, cos::PropertyKeyword::MultiDimensional(flag) if flag.is_set())
                    }),
                    type_param_names: type_param_names(&type_params),
                },
            ),
            cos::Property::Relationship {
                name,
                as_type,
                keywords,
                ..
            } => {
                // A `many`/`children` relationship is the collection of the related objects.
                let is_collection = typed(keywords.as_ref()).any(|keyword| {
                    matches!(keyword, cos::PropertyKeyword::Cardinality(cardinality)
                        if matches!(cardinality.value, cos::Cardinality::Many(_) | cos::Cardinality::Children(_)))
                });
                (
                    olc.with_range(name.0.as_ref().map(|_| name.name())),
                    deprecated || is_deprecated(keywords.as_ref()),
                    keyword_issues_in(keywords.as_ref(), olc),
                    MemberKindSpecificDetails::Property {
                        ty: if is_collection {
                            Some(crate::check::types::relationship_object(
                                as_type.as_deref().map(|t| t.class_name.print()),
                            ))
                        } else {
                            as_type.as_deref().map(Ty::from)
                        },
                        element_ty: None,
                        multidimensional: false,
                        type_param_names: vec![],
                    },
                )
            }
        },
        cos::Member::Query(query) => (
            with_range_name(&query.name, olc),
            deprecated || is_deprecated(query.keywords.as_ref()),
            keyword_issues_in(query.keywords.as_ref(), olc),
            MemberKindSpecificDetails::Query,
        ),
        cos::Member::Storage(storage) => (
            with_range_name(&storage.name, olc),
            deprecated,
            vec![],
            MemberKindSpecificDetails::Storage,
        ),
        cos::Member::Trigger(trigger) => (
            with_range_name(&trigger.name, olc),
            deprecated || is_deprecated(trigger.keywords.as_ref()),
            keyword_issues_in(trigger.keywords.as_ref(), olc),
            MemberKindSpecificDetails::Trigger,
        ),
        cos::Member::XData(xdata) => (
            with_range_name(xdata.name(), olc),
            deprecated || is_deprecated(xdata.keywords()),
            keyword_issues_in(xdata.keywords(), olc),
            MemberKindSpecificDetails::XData,
        ),
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
    let args = method
        .formal_spec
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
    let (normal_args, variadic_arg) = split_args(args);
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
            Left(ProcedureBlock(flag)) => procedure_block = Some(flag.is_set()),
            // Each name at the keyword's own range.
            Left(PublicList(v)) => {
                public_list = class_list_text(&v.value.print())
                    .into_iter()
                    .map(|name| WithRange::new(name, range))
                    .collect()
            }
            Left(Abstract(flag)) => is_abstract = flag.is_set(),
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
        language,
        mode,
        procedure_block,
        is_abstract,
    }))
}
