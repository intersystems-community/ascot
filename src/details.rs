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

use either::Either::{Left, Right};
use tygr::{Grammar, IntoInner, OLC, Prefix, Range, StringOf};

use crate::bindings::{self, ArgMode};
use crate::check::types::{NormalArg, ProcedureBlock, Ty, TyProcedure, VariadicArg, parameter_ty};
use crate::cos::{self, AllowMacro, CodeMode, KeywordList, MethodLanguage};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WithRange<T> {
    pub it: T,
    pub range: bindings::Range,
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
    // `..#X`
    Parameter,
    // `..X`: a property
    Property,
    // `..X(...)`: a method or a multidimensional property
    HigherOrder,
    Other,
}

// A multidimensional property is in both `Property` (its root node) and `HigherOrder`.
const MULTIDIMENSIONAL: &[MemberNs] = &[MemberNs::Property, MemberNs::HigherOrder];

impl MemberNs {
    pub(crate) fn of(kind: &MemberKindSpecificDetails) -> &'static [Self] {
        match kind {
            MemberKindSpecificDetails::Parameter { .. } => &[Self::Parameter],
            MemberKindSpecificDetails::Property { keywords, .. } => {
                if keywords
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case("MultiDimensional"))
                {
                    MULTIDIMENSIONAL
                } else {
                    &[Self::Property]
                }
            }
            MemberKindSpecificDetails::MethodOrClassMethod(..)
            | MemberKindSpecificDetails::ClientMethod => &[Self::HigherOrder],
            _ => &[Self::Other],
        }
    }

    pub(crate) fn of_info(kind: &bindings::MemberKind) -> &'static [Self] {
        match kind {
            bindings::MemberKind::Parameter(_) => &[Self::Parameter],
            bindings::MemberKind::Property(info) if info.multidimensional => MULTIDIMENSIONAL,
            bindings::MemberKind::Property(_) | bindings::MemberKind::Relationship(_) => {
                &[Self::Property]
            }
            bindings::MemberKind::Method(_)
            | bindings::MemberKind::ClassMethod(_)
            | bindings::MemberKind::ClientMethod(_) => &[Self::HigherOrder],
            _ => &[Self::Other],
        }
    }
}

pub(crate) struct ClassDetails {
    pub(crate) doc: String,
    pub(crate) name: WithRange<String>,
    pub(crate) extends: Vec<String>,
    // The `Import` packages, for resolving unqualified class names in this class.
    pub(crate) imports: Vec<String>,
    pub(crate) deprecated: bool,
    pub(crate) is_datatype: bool,
    pub(crate) language: Option<MethodLanguage>,
    pub(crate) mode: Option<CodeMode>,
    pub(crate) members: Vec<WithRange<MemberDetails>>,
    // One namespace per syntax (see `MemberNs`), so a same-named pair within one is a conflict.
    // The other kinds are only reachable through the members they generate.
    pub(crate) members_by_name: HashMap<(MemberNs, String), usize>,
    pub(crate) include_names: Vec<WithRange<String>>,
    // The `Include (...)` clause's own span, for pointing a "cannot find include" diagnostic at
    // it instead of at the class's declaration -- `None` only when there's no `Include` at all,
    // in which case `include_names` is also empty and this is never read.
    pub(crate) include_range: Option<bindings::Range>,
    // `[ CompileAfter = , ... ]`: parsed, reported by `check_cls`.
    pub(crate) keywords_without_value: Vec<WithRange<String>>,
    // From this class's own `[PropertyClass = ...]` keyword -- each listed class's own
    // `Parameter`s become valid property type-parameters for every property in this class *and*
    // its subclasses (see `check::env::WorkspaceEnv::property_class_names`, which walks
    // `extends` to accumulate every ancestor's own list too).
    pub(crate) property_class_names: Vec<String>,
}

pub struct MemberDetails {
    pub name: WithRange<String>,
    pub doc: String,
    pub deprecated: bool,
    pub kind: MemberKindSpecificDetails,
}

pub enum MemberKindSpecificDetails {
    MethodOrClassMethod(Box<MethodDetails>),
    Property {
        ty: Option<Ty>,
        keywords: Vec<String>,
        // Names from the type's own `(...)` parameter list -- e.g. %String's `MAXLEN` -- kept
        // separately from `keywords` (the `[...]` list) since the two are easy to confuse (see
        // `check::mod`'s cross-check against `PROPERTY_KEYWORDS`).
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
        keywords: Vec<String>,
    },
    ClientMethod,
    ForeignKey,
    Index {
        // The properties this index is defined `On`, in order — used to type-check calls to
        // its generated `<Name>Open`/`<Name>Exists`/`<Name>Delete` methods' value arguments.
        on_properties: Vec<String>,
    },
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
    pub language: Option<cos::MethodLanguage>,
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
            uri: uri.map(str::to_string),
            declared_at: uri.is_some().then_some(self.name.range),
        }
    }
}

fn import_args(
    olc: &OLC,
    params: &Option<cos::SpaceThen<cos::ParameterList>>,
) -> (Vec<WithRange<NormalArg>>, Option<WithRange<VariadicArg>>) {
    let mut args = params
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
            if entry.expand.is_some() {
                Right(with_range(
                    entry
                        .param
                        .as_ref()
                        .map(|_| VariadicArg::new(mode, name, None)),
                    olc,
                ))
            } else {
                let default = entry.default.as_deref().map(|default| default.print());
                Left(with_range(
                    entry
                        .param
                        .as_ref()
                        .map(|_| NormalArg::new(mode, name, None, default)),
                    olc,
                ))
            }
        })
        .collect::<Vec<_>>();
    let variadic_arg = if args.last().is_some_and(|arg| arg.is_right()) {
        args.pop().and_then(|arg| arg.right())
    } else {
        None
    };
    let normal_args = args.into_iter().filter_map(|arg| arg.left()).collect();
    (normal_args, variadic_arg)
}

fn public_list(
    olc: &OLC,
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
            AllowMacro::Plain(name) => Some(with_range(x.as_ref().map(|_| name.print()), olc)),
            AllowMacro::Macro(..) => None,
        })
        .collect()
}

fn subroutine_kind(olc: &OLC, subroutine: &cos::Subroutine) -> Option<RoutineElementDetails> {
    let AllowMacro::Plain(header) = &subroutine.header else {
        return None;
    };
    let (normal_args, variadic_arg) = import_args(olc, &header.params);
    Some(RoutineElementDetails {
        name: with_range(header.label.as_ref().map(Grammar::print), olc),
        kind: RoutineElementKind::Subroutine,
        normal_args,
        variadic_arg,
    })
}

fn procedure_kind(olc: &OLC, procedure: &cos::Procedure) -> Option<RoutineElementDetails> {
    let AllowMacro::Plain(header) = procedure.header.deref() else {
        return None;
    };
    let (normal_args, variadic_arg) = import_args(olc, &header.params);
    Some(RoutineElementDetails {
        name: with_range(header.label.as_ref().map(Grammar::print), olc),
        kind: RoutineElementKind::Procedure {
            public_list: public_list(olc, &header.publics),
        },
        normal_args,
        variadic_arg,
    })
}

pub fn routine_element_kind(
    olc: &OLC,
    element: &cos::RoutineElement,
) -> Option<RoutineElementDetails> {
    match element {
        cos::RoutineElement::Subroutine(subroutine) => subroutine_kind(olc, subroutine),
        cos::RoutineElement::Procedure(procedure) => procedure_kind(olc, procedure),
    }
}

fn with_range<T>(ranged: Range<T>, olc: &OLC) -> WithRange<T> {
    let range = bindings::Range {
        start: bindings::Position::from(olc.offset_to_line_character(ranged.start)),
        end: bindings::Position::from(olc.offset_to_line_character(ranged.end)),
    };
    WithRange {
        it: ranged.into_inner(),
        range,
    }
}

pub(crate) fn with_range_name<T: Grammar>(ranged: &Range<T>, olc: &OLC) -> WithRange<String> {
    with_range(ranged.as_ref().map(|x| x.print()), olc)
}

fn range<T>(x: &Range<T>, olc: &OLC) -> bindings::Range {
    bindings::Range {
        start: bindings::Position::from(olc.offset_to_line_character(x.start)),
        end: bindings::Position::from(olc.offset_to_line_character(x.end)),
    }
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

fn is_deprecated(keywords: &Option<KeywordList>) -> bool {
    match keywords.as_ref() {
        None => false,
        Some(keywords) => keywords.get("Deprecated").is_some(),
    }
}

// `ProcedureBlock`, at class or method level: `Not ProcedureBlock`/`ProcedureBlock = 0` turn it
// off. `None` when unspecified -- a method then follows its class, a class defaults to on.
fn procedure_block(keywords: &Option<KeywordList>) -> Option<bool> {
    Some(match keywords.as_ref()?.get("ProcedureBlock")? {
        cos::KeywordEntry::Not { .. } => false,
        cos::KeywordEntry::Assign { value, .. } => value.print().trim() != "0",
        cos::KeywordEntry::Flag(_) => true,
    })
}

fn is_datatype(keywords: &Option<KeywordList>) -> bool {
    keywords
        .as_ref()
        .and_then(|keywords| keywords.get("ClassType"))
        .and_then(|entry| entry.value())
        .is_some_and(|value| value.print().eq_ignore_ascii_case("datatype"))
}

// A `[PropertyClass = ...]` value is either one class name or a parenthesized, comma-separated
// list of them.
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

fn keyword_value(keywords: &Option<KeywordList>, name: &str) -> Option<String> {
    keywords
        .iter()
        .flat_map(|keywords| keywords.entries.iter())
        .flat_map(|entries| entries.items.iter())
        .find_map(|entry| match &***entry {
            cos::KeywordEntry::Assign { keyword, value, .. }
                if keyword.print().eq_ignore_ascii_case(name) =>
            {
                Some(
                    value
                        .as_ref()
                        .map(|value| value.print())
                        .unwrap_or_default(),
                )
            }
            _ => None,
        })
}

fn keyword_names(keywords: &Option<KeywordList>) -> Vec<String> {
    keywords
        .iter()
        .flat_map(|keywords| keywords.entries.iter())
        .flat_map(|entries| entries.items.iter())
        .map(|entry| {
            match &***entry {
                cos::KeywordEntry::Assign { keyword, .. } => keyword,
                cos::KeywordEntry::Not { keyword, .. } => keyword,
                cos::KeywordEntry::Flag(keyword) => keyword,
            }
            .print()
        })
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
    pub(crate) fn member(&self, ns: MemberNs, name: &str) -> Option<&WithRange<MemberDetails>> {
        self.members_by_name
            .get(&(ns, name.to_string()))
            .map(|&idx| &self.members[idx])
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
                .map(|c| WithRange::new(c.print(), range(c, &olc)))
                .collect()
        });
        let include_range = cls
            .include
            .as_ref()
            .map(|include| range(&include.files, &olc));
        let deprecated = is_deprecated(&cls.keyword_list);
        let keywords_without_value = cls
            .keyword_list
            .iter()
            .flat_map(|keywords| keywords.entries.iter())
            .flat_map(|entries| entries.items.iter())
            .filter_map(|entry| match &***entry {
                cos::KeywordEntry::Assign {
                    keyword,
                    value: None,
                    ..
                } => {
                    // The keyword leads its entry.
                    let keyword = keyword.print();
                    let at = Range::new(entry.start, (), entry.start + keyword.len());
                    Some(WithRange::new(keyword, range(&at, &olc)))
                }
                _ => None,
            })
            .collect();
        let class_procedure_block = procedure_block(&cls.keyword_list).unwrap_or(true);
        let is_datatype = is_datatype(&cls.keyword_list);
        let language = cls
            .keyword_list
            .as_ref()
            .and_then(|keywords| keywords.get("Language"))
            .and_then(|entry| match entry {
                cos::KeywordEntry::Assign { value, .. } => {
                    MethodLanguage::parse(&value.print()).ok()
                }
                _ => None,
            });
        let mode = cls
            .keyword_list
            .as_ref()
            .and_then(|keywords| keywords.get("CodeMode"))
            .and_then(|entry| match entry {
                cos::KeywordEntry::Assign { value, .. } => CodeMode::parse(&value.print()).ok(),
                _ => None,
            });
        let property_class_names = cls
            .keyword_list
            .as_ref()
            .and_then(|keywords| keywords.get("PropertyClass"))
            .map(|entry| match entry {
                cos::KeywordEntry::Assign { value, .. } => class_list_text(&value.print()),
                _ => vec![],
            })
            .unwrap_or_default();
        let mut members: Vec<WithRange<MemberDetails>> = Vec::new();
        let mut members_by_name = HashMap::new();
        for (before_member, member) in cls.member_list.members.into_item_sep() {
            let doc = extract_document_comments(before_member.0.as_str());
            let range = range(&member, &olc);
            let member_start = member.start;
            let (name, deprecated, kind) = process_member(
                &olc,
                src,
                member_start,
                deprecated,
                member,
                class_procedure_block,
            );
            for &ns in MemberNs::of(&kind) {
                members_by_name.insert((ns, name.to_string()), members.len());
            }
            members.push(WithRange {
                it: MemberDetails {
                    name,
                    doc,
                    deprecated,
                    kind,
                },
                range,
            });
        }
        Ok(Self {
            doc,
            name,
            extends,
            deprecated,
            is_datatype,
            language,
            mode,
            members,
            members_by_name,
            include_names,
            imports,
            keywords_without_value,
            include_range,
            property_class_names,
        })
    }
}

fn process_member(
    olc: &OLC,
    src: &str,
    member_start: usize,
    deprecated: bool,
    member: Range<cos::Member>,
    class_procedure_block: bool,
) -> (WithRange<String>, bool, MemberKindSpecificDetails) {
    let (member_name, deprecated, kind) = match member.into_inner() {
        cos::Member::ForeignKey(fk) => (
            with_range_name(&fk.name, olc),
            deprecated || is_deprecated(&fk.keywords),
            MemberKindSpecificDetails::ForeignKey,
        ),
        cos::Member::Index(index) => (
            with_range_name(&index.name, olc),
            deprecated || is_deprecated(&index.keywords),
            MemberKindSpecificDetails::Index {
                on_properties: index.on_properties(),
            },
        ),
        cos::Member::Method(method) => {
            let name = with_range_name(&method.name, olc);
            let deprecated = deprecated || is_deprecated(&method.keywords);
            let is_class_method = match &method.keyword {
                cos::MethodKeyword::Method(_) => false,
                cos::MethodKeyword::ClassMethod(_) => true,
                cos::MethodKeyword::ClientMethod(_) => {
                    return (name, deprecated, MemberKindSpecificDetails::ClientMethod);
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
            (name, deprecated, kind)
        }
        cos::Member::Parameter(parameter) => {
            let name = with_range_name(&parameter.name, olc);
            let as_type = parameter.as_type.as_deref().map(Grammar::print);
            let ty = as_type.as_deref().map(parameter_ty);
            let default = parameter.default.as_ref().map(|v| v.deref().print());
            (
                name,
                deprecated || is_deprecated(&parameter.keywords),
                MemberKindSpecificDetails::Parameter {
                    ty,
                    default,
                    as_type,
                    keywords: keyword_names(&parameter.keywords),
                },
            )
        }
        cos::Member::Projection(projection) => (
            with_range_name(&projection.name, olc),
            deprecated || is_deprecated(&projection.keywords),
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
                with_range(name.0.as_ref().map(|_| name.name()), olc),
                deprecated || is_deprecated(&keywords),
                MemberKindSpecificDetails::Property {
                    ty: as_type.as_deref().map(Ty::from),
                    element_ty: as_type.as_deref().and_then(element_ty),
                    keywords: keyword_names(&keywords),
                    type_param_names: type_param_names(&type_params),
                },
            ),
            cos::Property::Relationship {
                name,
                as_type,
                keywords,
                ..
            } => (
                with_range(name.0.as_ref().map(|_| name.name()), olc),
                deprecated || is_deprecated(&keywords),
                MemberKindSpecificDetails::Property {
                    // A `many`/`children` relationship is the collection of the related objects.
                    ty: if keyword_value(&keywords, "Cardinality").is_some_and(|c| {
                        c.eq_ignore_ascii_case("many") || c.eq_ignore_ascii_case("children")
                    }) {
                        Some(Ty::from("%Library.RelationshipObject"))
                    } else {
                        as_type.as_deref().map(Ty::from)
                    },
                    element_ty: None,
                    keywords: keyword_names(&keywords),
                    type_param_names: vec![],
                },
            ),
        },
        cos::Member::Query(query) => (
            with_range_name(&query.name, olc),
            deprecated,
            MemberKindSpecificDetails::Query,
        ),
        cos::Member::Storage(storage) => (
            with_range_name(&storage.name, olc),
            deprecated,
            MemberKindSpecificDetails::Storage,
        ),
        cos::Member::Trigger(trigger) => (
            with_range_name(&trigger.name, olc),
            deprecated,
            MemberKindSpecificDetails::Trigger,
        ),
        cos::Member::XData(xdata) => (
            with_range_name(xdata.name(), olc),
            deprecated,
            MemberKindSpecificDetails::XData,
        ),
    };
    (member_name, deprecated, kind)
}

fn method_kind(
    olc: &OLC,
    src: &str,
    header_start: usize,
    method: cos::Method,
    is_class_method: bool,
    class_procedure_block: bool,
) -> MemberKindSpecificDetails {
    let body_start = method.body.wrapped.start;
    // `normal_args`/`out` stay ranged against the class's whole-document `olc`, like any other
    // member -- other methods' call-site diagnostics reference these same ranges directly.
    let header_text = &src[header_start..body_start];
    let mut args = method
        .formal_spec
        .iter()
        .flat_map(|spec| spec.formals().into_iter())
        .map(|formal| {
            if formal.variadic.is_none() {
                Left(with_range(formal.as_ref().map(NormalArg::from), olc))
            } else {
                Right(with_range(formal.as_ref().map(VariadicArg::from), olc))
            }
        })
        .collect::<Vec<_>>();
    let variadic_arg = if args.last().is_some_and(|arg| arg.is_right()) {
        args.pop().and_then(|arg| arg.right())
    } else {
        None
    };
    let normal_args = args.into_iter().filter_map(|arg| arg.left()).collect();
    let out = method
        .return_type
        .as_ref()
        .map(|x| x.as_deref().map(Ty::from));
    let out = with_range(out, olc);
    let language = method
        .keywords
        .as_ref()
        .and_then(|keywords| keywords.get("Language"))
        .and_then(|entry| match entry {
            cos::KeywordEntry::Assign { value, .. } => MethodLanguage::parse(&value.print()).ok(),
            _ => None,
        });
    let mode = method
        .keywords
        .as_ref()
        .and_then(|keywords| keywords.get("CodeMode"))
        .and_then(|entry| match entry {
            cos::KeywordEntry::Assign { value, .. } => CodeMode::parse(&value.print()).ok(),
            _ => None,
        });
    let procedure_block = procedure_block(&method.keywords)
        .unwrap_or(class_procedure_block)
        .then(Vec::new);
    let is_abstract = method
        .keywords
        .as_ref()
        .and_then(|keywords| keywords.get("Abstract"))
        .is_some_and(|entry| !matches!(entry, cos::KeywordEntry::Not { .. }));
    let raw_body = method.body.into_inner().into_inner();
    let body = with_range(raw_body, olc).map(|x| x.print());
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
