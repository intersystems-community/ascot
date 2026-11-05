//! Static context for the checker, in three nested scopes:
//! - `WorkspaceEnv`: one per check run. Resolves classes and members across every open document
//!   (and IRIS, when connected), records cross-file references, and raises "soft" diagnostics
//!   for anything it couldn't confirm.
//! - `DocumentEnv`/`ClassDocument`: the file being checked, with its macros, labels and
//!   `#include`s and, for a class, its inheritance chain and member tables.
//! - `CodeEnv`: one method body or routine block, with `this`, formal arguments and local labels.
//!
//! Reads `details.rs` and `lib.rs`'s `WorkspaceStruct`; produces `types.rs` values for
//! `mod.rs`/`flow.rs` to check against. `lib.rs` also builds a `WorkspaceEnv` to resolve members
//! for hover and go-to-definition, so member lookup rules live here only once.

use std::borrow::Cow;
use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
};

use tygr::OLC;

use crate::bindings::{InlayHint, MemberKind, RefMode};
use crate::check::errors::*;
use crate::check::flow::{Flow, Join, OutputCell};
use crate::check::types::*;
pub use crate::details::Positions;
use crate::details::{
    MemberDetails, MemberKindSpecificDetails, MemberNs, RoutineElementDetails, WithRange,
};
use crate::{DocKind, MacroDefinitions, Name, SourceMap, WorkspaceStruct, bindings, cos};
use std::marker::PhantomData;

// A member a lookup found, with the class declaring it, which its type parameters resolve against.
struct Found<T> {
    member: Member<T>,
    is_instance_only: bool,
    origin: String,
}

// A generated method, and whether it's instance-only.
type Generated = (Arc<TyProcedure>, bool);

// One of the three member namespaces IRIS tells apart by syntax -- `..#X`, `..X`, `..X(...)` --
// and the shape of what it holds.
pub(crate) trait Namespace {
    type Ty: Instantiate + ClassNames + Default;
    // Where its members are declared.
    const NS: MemberNs;
    // The type of a member a workspace class declares, and whether it's instance-only.
    fn declared(member: &WithRange<MemberDetails>, uri: &str) -> Option<(Self::Ty, bool)>;
    // Likewise for one IRIS reports, corrected by the mask.
    fn reported(member: bindings::MemberInfo) -> Option<(Self::Ty, bool)>;
    // A method as this namespace sees it: the generated members, and those taken as present past
    // a class nobody can see, are all methods.
    fn method(_proc: TyProcedureOrAny) -> Option<Self::Ty> {
        None
    }
    // The member a `%Dispatch*` hook of `obj` stands in for when no class has the name, typed
    // from the hook's signature as `find` resolves it; `None` where no hook applies.
    fn dispatched(
        _obj: &ClsOrIns,
        _find: impl FnOnce(&str) -> Option<TyProcedureOrAny>,
    ) -> Option<Self::Ty> {
        None
    }
}

pub(crate) struct Parameters;
pub(crate) struct Properties<A>(PhantomData<A>);
pub(crate) struct Methods;

// How `..X` is used, which decides the `%Dispatch*` hook a missing property falls back to.
pub(crate) trait Access {
    fn hook(obj: &ClsOrIns) -> Option<&'static str>;
}
pub(crate) struct Read;
pub(crate) struct Assign;
// `Set ..X(subs) = v`, a node of a multidimensional property.
pub(crate) struct AssignNode;
// `..X(subs)` once no method is named `X`: a node, or whatever `%DispatchMethod` makes of it.
pub(crate) struct Called;
// An index key: IRIS wants a declared property, hook or not.
pub(crate) struct Indexed;
impl Access for Read {
    fn hook(_: &ClsOrIns) -> Option<&'static str> {
        Some("%DispatchGetProperty")
    }
}
impl Access for Assign {
    fn hook(_: &ClsOrIns) -> Option<&'static str> {
        Some("%DispatchSetProperty")
    }
}
impl Access for AssignNode {
    fn hook(_: &ClsOrIns) -> Option<&'static str> {
        Some("%DispatchSetMultidimProperty")
    }
}
impl Access for Called {
    fn hook(obj: &ClsOrIns) -> Option<&'static str> {
        Some(if obj.is_ins() {
            "%DispatchMethod"
        } else {
            "%DispatchClassMethod"
        })
    }
}
impl Access for Indexed {
    fn hook(_: &ClsOrIns) -> Option<&'static str> {
        None
    }
}

impl Namespace for Parameters {
    type Ty = Ty;
    const NS: MemberNs = MemberNs::Parameter;

    fn declared(member: &WithRange<MemberDetails>, _: &str) -> Option<(Ty, bool)> {
        let MemberKindSpecificDetails::Parameter { ty, .. } = &member.kind else {
            return None;
        };
        Some((ty.clone().unwrap_or_default(), false))
    }

    fn reported(member: bindings::MemberInfo) -> Option<(Ty, bool)> {
        let MemberKind::Parameter(p) = member.kind else {
            return None;
        };
        Some((p.t.as_deref().map(parameter_ty).unwrap_or_default(), false))
    }
}

impl<A: Access> Namespace for Properties<A> {
    type Ty = PropertyTy;
    const NS: MemberNs = MemberNs::Property;

    fn declared(member: &WithRange<MemberDetails>, _: &str) -> Option<(PropertyTy, bool)> {
        let MemberKindSpecificDetails::Property {
            ty,
            element_ty,
            multidimensional,
            ..
        } = &member.kind
        else {
            return None;
        };
        let ty = match (ty, element_ty) {
            _ if *multidimensional => PropertyTy::Multidimensional(ty.clone().unwrap_or_default()),
            (Some(Ty::OREF(cls)), Some(element)) => PropertyTy::Collection {
                kind: if cls.name().ends_with("List") {
                    CollectionClass::List
                } else {
                    CollectionClass::Array
                },
                element: element.clone(),
            },
            _ => PropertyTy::Value(ty.clone().unwrap_or_default()),
        };
        Some((ty, true))
    }

    fn reported(member: bindings::MemberInfo) -> Option<(PropertyTy, bool)> {
        let ty = match member.kind {
            MemberKind::Property(info) if info.multidimensional => {
                PropertyTy::Multidimensional(ty_from_declared(info.t).unwrap_or_default())
            }
            MemberKind::Property(info) => match info.collection {
                Some(collection) => PropertyTy::Collection {
                    kind: match collection {
                        bindings::CollectionKind::List => CollectionClass::List,
                        bindings::CollectionKind::Array => CollectionClass::Array,
                    },
                    element: ty_from_declared(info.t).unwrap_or_default(),
                },
                None => PropertyTy::Value(ty_from_declared(info.t).unwrap_or_default()),
            },
            MemberKind::Relationship(info) => PropertyTy::Value(match info.cardinality {
                Some(bindings::Cardinality::Many | bindings::Cardinality::Children) => {
                    relationship_object(info.t)
                }
                _ => ty_from_declared(info.t).unwrap_or_default(),
            }),
            _ => return None,
        };
        Some((ty, true))
    }

    // Only the hook's return type matters.
    fn dispatched(
        obj: &ClsOrIns,
        find: impl FnOnce(&str) -> Option<TyProcedureOrAny>,
    ) -> Option<PropertyTy> {
        let hook = find(A::hook(obj)?)?;
        let out = hook.procedure().and_then(|hook| hook.out.clone());
        Some(PropertyTy::Dispatched(out.unwrap_or_default()))
    }
}

impl Namespace for Methods {
    type Ty = TyProcedureOrAny;
    const NS: MemberNs = MemberNs::Method;

    fn declared(member: &WithRange<MemberDetails>, uri: &str) -> Option<(TyProcedureOrAny, bool)> {
        let MemberKindSpecificDetails::MethodOrClassMethod(m) = &member.kind else {
            return None;
        };
        let proc = TyProcedure {
            normal_args: m
                .normal_args
                .iter()
                .map(|arg| arg.it.clone().with_declared_at(arg.range))
                .collect(),
            variadic_arg: m.variadic_arg.as_ref().map(|arg| arg.it.clone()),
            out: m.out.it.clone(),
            procedure_block: Some(
                m.procedure_block
                    .as_ref()
                    .map_or(vec![], |x| x.iter().map(|x| x.to_string()).collect()),
            ),
            formal_list: true,
            uri: Some(uri.to_string()),
            declared_at: Some(member.range),
        };
        Some((
            TyProcedureOrAny::Procedure(Arc::new(proc)),
            !m.is_class_method,
        ))
    }

    fn reported(member: bindings::MemberInfo) -> Option<(TyProcedureOrAny, bool)> {
        let location = location(&member);
        let (m, is_instance_only) = match member.kind {
            MemberKind::Method(m) => (m, true),
            MemberKind::ClassMethod(m) => (m, false),
            _ => return None,
        };
        let mut proc = TyProcedure::from(m);
        proc.uri = location.as_ref().map(|location| location.uri.clone());
        proc.declared_at = location.map(|location| location.range);
        Some((
            TyProcedureOrAny::Procedure(Arc::new(proc)),
            is_instance_only,
        ))
    }

    fn method(proc: TyProcedureOrAny) -> Option<TyProcedureOrAny> {
        Some(proc)
    }
}

// Why `find` can't even say whether the member exists.
enum FindError {
    NoSuchClass(String),
    InstanceOnly(String),
}

impl From<FindError> for QuasiTypeError {
    fn from(err: FindError) -> Self {
        match err {
            FindError::NoSuchClass(cls) => QuasiTypeError::cannot_find_class(cls),
            FindError::InstanceOnly(mem) => {
                QuasiTypeError::access_instance_only_member_from_class_context(mem)
            }
        }
    }
}

impl<T> Member<T> {
    // A member no source declares: generated, dispatched, or taken on trust.
    fn undeclared(ty: T) -> Self {
        Member { ty, location: None }
    }
}

fn generated_procedure(
    normal_args: Vec<NormalArg>,
    variadic: bool,
    out: Option<Ty>,
) -> Arc<TyProcedure> {
    Arc::new(TyProcedure {
        normal_args,
        variadic_arg: variadic.then(|| VariadicArg::new(None, "args".into(), None)),
        out,
        procedure_block: Some(vec![]),
        formal_list: true,
        uri: None,
        declared_at: None,
    })
}

// Where a host-reported member is declared: only known when the host could map it to a uri
// (e.g. not a bare IRIS system class), and then only as far as the document.
fn location(member: &bindings::MemberInfo) -> Option<bindings::Location> {
    member
        .uri
        .clone()
        .map(|uri| bindings::Location::new(uri, bindings::Range::default()))
}

// The `IDKEY` index IRIS gives a persistent class that declares no id key: no `On` clause, so
// its accessors take the id.
fn default_id_key() -> bindings::IndexInfo {
    bindings::IndexInfo {
        on: None,
        abstract_: false,
        condition: None,
        coshard_with: None,
        data: vec![],
        deferred: false,
        extent: false,
        id_key: true,
        internal: false,
        primary_key: false,
        shard_key: false,
        sql_name: None,
        type_: Some(bindings::IndexType::Key),
        unique: false,
    }
}

// An IRIS-reported member of `cls`, typed and masked by its declaring class.
fn iris_found<N: Namespace>(mut member: bindings::MemberInfo, cls: &str) -> Option<Found<N::Ty>> {
    let origin = member.origin.clone().unwrap_or_else(|| cls.to_string());
    if crate::mask::dropped(&origin, &member.name) {
        return None;
    }
    if let Some(masked) = crate::mask::replacement(&origin, &member.name, &member.kind) {
        member.kind = masked.kind.clone();
    }
    let location = location(&member);
    let (ty, is_instance_only) = N::reported(member)?;
    Some(Found {
        member: Member { ty, location },
        is_instance_only,
        origin,
    })
}

// A class as a check run sees it; `None` from `resolve` means no class by that name exists: IRIS
// has none, or there is no IRIS at all and the class isn't in the workspace.
enum Class {
    Workspace(String, Arc<crate::ClassDetails>),
    Iris(Arc<bindings::ClassInfo>),
    // One nobody can see: an IRIS lookup that failed (with its reason), or a workspace file that
    // doesn't parse. Taken as present, with whatever members anyone asks of it.
    Unknown(Option<String>),
}

// `Query Q(args)`'s generated family, in the shapes IRIS gives them (`QUERY_METHOD_SUFFIXES`):
// `QFunc(args)` is its result set; `QExecute` takes a handle out and the query's arguments;
// `QFetch`, `QFetchRows`, `QClose`, `QGetInfo` and `QGetODBCInfo` are fixed; `QPrepare` and
// `QSendODBC` are left loose.
fn query_methods(
    normal: Vec<NormalArg>,
    variadic: Option<VariadicArg>,
) -> Vec<(String, Generated)> {
    let (by_ref, output) = (Some(RefMode::ByRef), Some(RefMode::Output));
    let arg = |mode, name: &str, ty: &str, default: Option<&str>| {
        NormalArg::new(
            mode,
            name.into(),
            Some(Ty::from(ty)),
            default.map(str::to_string),
        )
    };
    let handle = |mode| arg(mode, "qHandle", "%Binary", None);
    let status = || Some(Ty::from("%Status"));
    let proc = |args, variadic, out| {
        Arc::new(TyProcedure {
            normal_args: args,
            variadic_arg: variadic,
            out,
            procedure_block: Some(vec![]),
            formal_list: true,
            uri: None,
            declared_at: None,
        })
    };
    let execute = std::iter::once(handle(output))
        .chain(normal.iter().cloned())
        .collect();
    [
        (
            "Func",
            proc(
                normal,
                variadic.clone(),
                Some(Ty::from("%SQL.StatementResult")),
            ),
        ),
        ("Execute", proc(execute, variadic, status())),
        (
            "Fetch",
            proc(
                vec![
                    handle(by_ref),
                    arg(output, "Row", "%List", None),
                    arg(output, "AtEnd", "%Integer", Some("0")),
                ],
                None,
                status(),
            ),
        ),
        (
            "FetchRows",
            proc(
                vec![
                    handle(by_ref),
                    arg(None, "FetchCount", "%Integer", Some("0")),
                    arg(output, "RowSet", "%List", None),
                    arg(output, "ReturnCount", "%Integer", None),
                    arg(output, "AtEnd", "%Integer", None),
                ],
                None,
                status(),
            ),
        ),
        ("Close", proc(vec![handle(by_ref)], None, status())),
        (
            "GetInfo",
            proc(
                vec![
                    arg(output, "colinfo", "%List", None),
                    arg(output, "parminfo", "%List", None),
                    arg(output, "idinfo", "%List", None),
                    handle(by_ref),
                    arg(None, "extoption", "%Integer", Some("0")),
                    arg(output, "extinfo", "%List", None),
                ],
                None,
                status(),
            ),
        ),
        (
            "GetODBCInfo",
            proc(
                vec![
                    arg(output, "colinfo", "%List", None),
                    arg(output, "parminfo", "%List", None),
                ],
                None,
                status(),
            ),
        ),
        ("Prepare", generated_procedure(vec![], true, status())),
        ("SendODBC", generated_procedure(vec![], true, status())),
    ]
    .into_iter()
    .map(|(suffix, proc)| (suffix.to_string(), (proc, false)))
    .collect()
}

// A generated property method: suffix, argument, result (`Prop`: the property's type), instance-only.
struct Shape(&'static str, Arg, Out, bool);

#[derive(Clone, Copy)]
enum Arg {
    None,
    Value,
    Id,
    Concurrency,
}

#[derive(Clone, Copy)]
enum Out {
    Prop,
    String,
    Status,
    Boolean,
}

impl Shape {
    fn procedure(&self, prop: &Ty) -> Generated {
        let Shape(_, arg, out, is_instance_only) = *self;
        let one = |mode, name: &str, ty| vec![NormalArg::new(mode, name.into(), Some(ty), None)];
        let args = match arg {
            Arg::None => vec![],
            Arg::Value => one(None, "value", prop.clone()),
            Arg::Id => one(None, "id", Ty::from("%String")),
            Arg::Concurrency => one(Some(RefMode::Output), "concurrency", Ty::from("%Integer")),
        };
        let out = match out {
            Out::Prop => prop.clone(),
            Out::String => Ty::from("%String"),
            Out::Status => Ty::from("%Status"),
            Out::Boolean => Ty::from("%Boolean"),
        };
        (
            generated_procedure(args, false, Some(out)),
            is_instance_only,
        )
    }
}

const ACCESSORS: &[Shape] = &[
    Shape("Get", Arg::None, Out::Prop, true),
    Shape("Set", Arg::Value, Out::Status, true),
    Shape("IsModified", Arg::None, Out::Boolean, true),
    Shape("GetStored", Arg::Id, Out::Prop, false),
];

const OBJECT_ACCESSORS: &[Shape] = &[
    Shape("GetObject", Arg::Concurrency, Out::Prop, true),
    Shape("GetObjectId", Arg::None, Out::String, true),
    Shape("SetObject", Arg::Value, Out::Status, true),
    Shape("SetObjectId", Arg::Id, Out::Status, true),
    Shape("NewObject", Arg::None, Out::Prop, true),
    Shape("GetSwizzled", Arg::None, Out::Prop, true),
];

const CONVERSIONS: &[Shape] = &[
    Shape("IsValid", Arg::Value, Out::Status, false),
    Shape("LogicalToDisplay", Arg::Value, Out::String, false),
    Shape("LogicalToOdbc", Arg::Value, Out::String, false),
    Shape("LogicalToStorage", Arg::Value, Out::String, false),
    Shape("LogicalToXSD", Arg::Value, Out::String, false),
    Shape("Normalize", Arg::Value, Out::Prop, false),
    Shape("DisplayToLogical", Arg::Value, Out::Prop, false),
    Shape("OdbcToLogical", Arg::Value, Out::Prop, false),
    Shape("StorageToLogical", Arg::Value, Out::Prop, false),
    Shape("XSDToLogical", Arg::Value, Out::Prop, false),
];

// Every member `%Library.SystemBase` and `%Library.RegisteredObject` contribute
// to every object class, taken as present when that chain can't be resolved (see
// `registered_object_member`) -- cross-checked against InterSystems' own class reference
// (docs.intersystems.com, `%25CSP.Documatic.cls?LIBRARY=%25SYS&CLASSNAME=...`), not memory:
// - https://docs.intersystems.com/irisforhealth20262/csp/documatic/%25CSP.Documatic.cls?LIBRARY=%25SYS&CLASSNAME=%25Library.SystemBase
// - https://docs.intersystems.com/irisforhealth20262/csp/documatic/%25CSP.Documatic.cls?LIBRARY=%25SYS&CLASSNAME=%25Library.RegisteredObject
// Minus the `%Dispatch*` hooks and `%OnNew`, whose defaults mask.json drops.
const REGISTERED_OBJECT_MEMBERS: &[&str] = &[
    // %Library.SystemBase
    "%ClassIsLatestVersion",
    "%New",
    "%ClassName",
    "%Extends",
    "%GetParameter",
    "%IsA",
    "%OriginalNamespace",
    "%PackageName",
    "%SetModified",
    // %Library.RegisteredObject
    "%AddToSaveSet",
    "%ConstructClone",
    "%IsModified",
    "%NormalizeObject",
    "%ObjectModified",
    "%RemoveFromSaveSet",
    "%SerializeObject",
    "%ValidateObject",
];

impl<'a> WorkspaceEnv<'a> {
    // The names in `N`'s namespace along `cls`'s chain -- all ObjectScript can refer to there.
    pub(crate) fn names<N: Namespace>(&self, cls: &str) -> Vec<String> {
        let mut names = Vec::new();
        for (cls, resolved) in self.ancestry(cls) {
            match &resolved {
                Some(Class::Workspace(_, details)) => names.extend(details.names(N::NS).cloned()),
                Some(Class::Iris(info)) => names.extend(
                    (info.members.iter())
                        .filter(|m| MemberNs::of_info(&m.kind) == N::NS)
                        .filter(|m| {
                            !crate::mask::dropped(m.origin.as_deref().unwrap_or(&cls), &m.name)
                        })
                        .map(|m| m.name.clone()),
                ),
                Some(Class::Unknown(_)) | None => continue,
            }
            if N::NS == MemberNs::Method {
                names.extend(self.generated_members(&cls, &resolved).keys().cloned());
            }
        }
        names
    }

    // Every class named in `cls`'s own or any ancestor's `[PropertyClass = ...]` -- each one's
    // own `Parameter`s are valid property type-parameters throughout `cls`'s whole subtree (see
    // https://docs.intersystems.com/irislatest/csp/docbook/DocBook.UI.Page.cls?KEY=ROBJ_class_propertyclass,
    // "Effect on Subclasses").
    pub(crate) fn property_classes(&self, cls: &str) -> Vec<String> {
        (self.ancestry(cls))
            .flat_map(|(_, resolved)| match resolved {
                Some(Class::Workspace(_, details)) => details.keywords.property_class.clone(),
                Some(Class::Iris(info)) => info.property_class.clone(),
                Some(Class::Unknown(_)) | None => vec![],
            })
            .collect()
    }

    // The methods IRIS generates for `cls`'s own properties, indices and queries, built on the
    // first miss among the declared ones. Over-approximates: what IRIS emits is generator-dependent.
    fn generated_members(
        &self,
        cls: &str,
        resolved: &Option<Class>,
    ) -> Rc<HashMap<String, Generated>> {
        if let Some(built) = self.generated.borrow().get(cls) {
            return built.clone();
        }
        let mut generated: HashMap<String, Generated> = HashMap::new();
        let mut add = |name: &str, family: Vec<(String, Generated)>| {
            for (suffix, method) in family {
                generated.entry(format!("{name}{suffix}")).or_insert(method);
            }
        };
        // `%New` takes the class's own `%OnNew`'s arguments, which the dictionary doesn't record.
        let new = |on_new: Option<Found<TyProcedureOrAny>>| match on_new
            .and_then(|f| f.member.ty.procedure().cloned())
        {
            Some(on_new) => {
                let new = Arc::new(TyProcedure {
                    normal_args: on_new.normal_args.clone(),
                    variadic_arg: on_new.variadic_arg.clone(),
                    out: Some(Ty::This),
                    procedure_block: Some(vec![]),
                    formal_list: true,
                    uri: on_new.uri.clone(),
                    declared_at: on_new.declared_at,
                });
                vec![("%New".to_string(), (new, false))]
            }
            None => vec![],
        };
        match resolved {
            Some(Class::Workspace(_, details)) => {
                for (name, property) in &details.members.property {
                    if let MemberKindSpecificDetails::Property { ty, .. } = &property.kind {
                        add(name, self.property_methods(ty.clone().unwrap_or_default()));
                    }
                }
                for (name, index) in &details.members.index {
                    if let MemberKindSpecificDetails::Index(index) = &index.kind {
                        add(name, self.index_methods(cls, &index.into()));
                    }
                }
                // A persistent class without an id key of its own gets IRIS's `IDKEY` on the id.
                let declares_id_key = details.members.index.values().any(|index| {
                    matches!(&index.kind, MemberKindSpecificDetails::Index(index) if index.id_key)
                });
                if !declares_id_key && self.is_persistent(cls) {
                    add("IDKEY", self.index_methods(cls, &default_id_key()));
                }
                for (name, query) in &details.members.query {
                    if let MemberKindSpecificDetails::Query {
                        normal_args,
                        variadic_arg,
                        ..
                    } = &query.kind
                    {
                        let normal = normal_args.iter().map(|arg| arg.it.clone()).collect();
                        let variadic = variadic_arg.as_ref().map(|arg| arg.it.clone());
                        add(name, query_methods(normal, variadic));
                    }
                }
                add(
                    "",
                    new(self.declared_in::<Methods>(cls, resolved, "%OnNew")),
                );
            }
            Some(Class::Iris(info)) => {
                for member in &info.members {
                    let name = member.name.as_str();
                    match &member.kind {
                        MemberKind::Property(_) | MemberKind::Relationship(_) => {
                            let found = iris_found::<Properties<Read>>(member.clone(), cls);
                            let ty = match found.map(|found| found.member.ty) {
                                Some(PropertyTy::Value(ty)) => ty,
                                _ => Ty::Any,
                            };
                            add(name, self.property_methods(ty));
                        }
                        MemberKind::Index(index) => add(name, self.index_methods(cls, index)),
                        MemberKind::Query(sig) => {
                            let proc = TyProcedure::from(sig.clone());
                            add(name, query_methods(proc.normal_args, proc.variadic_arg));
                        }
                        MemberKind::Method(_) | MemberKind::ClassMethod(_) if name == "%OnNew" => {
                            add("", new(iris_found::<Methods>(member.clone(), cls)));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        let generated = Rc::new(generated);
        self.generated
            .borrow_mut()
            .insert(cls.to_string(), generated.clone());
        generated
    }

    // `Property X As <ty>`'s accessors, object accessors for a reference, and the type class's
    // class methods projected under `X` (`%String`'s `LogicalToDisplay` becomes `XLogicalToDisplay`).
    fn property_methods(&self, ty: Ty) -> Vec<(String, Generated)> {
        let family = |shapes: &[Shape]| -> Vec<(String, Generated)> {
            shapes
                .iter()
                .map(|shape| (shape.0.to_string(), shape.procedure(&ty)))
                .collect()
        };
        let type_class = match &ty {
            Ty::OREF(obj) => Some(obj.name().to_string()),
            _ => None,
        };
        // Object accessors need a reference type; assume one when the type can't be resolved.
        let is_datatype = type_class.as_deref().map(|cls| self.is_datatype(cls));
        let mut methods = family(ACCESSORS);
        if is_datatype.is_some_and(|is_datatype| is_datatype != Some(true)) {
            methods.extend(family(OBJECT_ACCESSORS));
        }
        if is_datatype == Some(Some(false)) {
            return methods;
        }
        let (projected, complete) = match type_class.as_deref() {
            Some(cls) => self.class_methods(cls),
            None => (HashMap::new(), false),
        };
        methods.extend(
            projected
                .into_iter()
                .filter(|(name, _)| !matches!(name.as_str(), "LogicalToJSON" | "JSONToLogical")),
        );
        // An unresolvable or undeclared type gets `%String`'s conversion family.
        if !complete {
            methods.extend(family(CONVERSIONS));
        }
        methods
    }

    // `cls`'s class methods, own and inherited, nearest first; `false` past a class nobody can see.
    fn class_methods(&self, cls: &str) -> (HashMap<String, Generated>, bool) {
        let mut methods: HashMap<String, Generated> = HashMap::new();
        let mut complete = true;
        for (cls, resolved) in self.ancestry(cls) {
            match &resolved {
                Some(Class::Workspace(_, details)) => {
                    for (name, member) in &details.members.method {
                        if let MemberKindSpecificDetails::MethodOrClassMethod(method) = &member.kind
                            && method.is_class_method
                            && let Some(found) = self.declared_in::<Methods>(&cls, &resolved, name)
                            && let Some(proc) = found.member.ty.procedure()
                        {
                            methods.entry(name.clone()).or_insert((proc.clone(), false));
                        }
                    }
                }
                Some(Class::Iris(info)) => {
                    for member in &info.members {
                        if matches!(member.kind, MemberKind::ClassMethod(_))
                            && let Some(found) = iris_found::<Methods>(member.clone(), &cls)
                            && let Some(proc) = found.member.ty.procedure()
                        {
                            methods
                                .entry(member.name.clone())
                                .or_insert((proc.clone(), false));
                        }
                    }
                }
                Some(Class::Unknown(_)) | None => complete = false,
            }
        }
        (methods, complete)
    }

    // The accessors IRIS generates for an index, as measured: nothing for a bitmap or an index
    // subscripted on a collection; `Exists(keys..., Output id)` for any other with keys, or for an
    // id key, whose one argument is the id; `Open(keys..., concurrency, ByRef sc)` and
    // `Delete(keys..., concurrency)` too when it is unique, a primary key or the id key. `Open`
    // returns the class it is called on, not `cls`.
    fn index_methods(&self, cls: &str, index: &bindings::IndexInfo) -> Vec<(String, Generated)> {
        let keys = index.on.as_deref().unwrap_or_default();
        if index.type_ == Some(bindings::IndexType::Bitmap)
            || keys.iter().any(|key| key.subscript.is_some())
            || (keys.is_empty() && !index.id_key)
        {
            return vec![];
        }
        let arg = |name: &str, mode, ty: Ty, default: Option<&str>| {
            NormalArg::new(mode, name.into(), Some(ty), default.map(str::to_string))
        };
        let this = ClsOrIns::Ins(ClassTy::from(cls.to_string()));
        let mut args: Vec<NormalArg> = keys
            .iter()
            .map(|key| {
                // Quietly: `check_cls` resolves the key itself, hint and all.
                let was_suppressed = self.suppress_soft_diagnostics();
                let found = self
                    .find::<Properties<Read>>(
                        &this,
                        &key.property,
                        true,
                        bindings::Range::default(),
                    )
                    .ok()
                    .flatten();
                self.restore_soft_diagnostics(was_suppressed);
                let ty = match found.map(|found| found.ty) {
                    Some(PropertyTy::Value(ty)) => ty,
                    _ => Ty::Any,
                };
                arg("value", None, ty, None)
            })
            .collect();
        if keys.is_empty() {
            args.push(arg("id", None, Ty::from("%String"), None));
        }
        let exists = (
            "Exists",
            vec![arg("id", Some(RefMode::Output), Ty::from("%String"), None)],
            Ty::from("%Boolean"),
        );
        let open_and_delete = [
            (
                "Open",
                vec![
                    arg("concurrency", None, Ty::from("%Integer"), Some("-1")),
                    arg(
                        "sc",
                        Some(RefMode::ByRef),
                        Ty::from("%Status"),
                        Some("$$$OK"),
                    ),
                ],
                Ty::This,
            ),
            (
                "Delete",
                vec![arg("concurrency", None, Ty::from("%Integer"), Some("-1"))],
                Ty::from("%Status"),
            ),
        ];
        let unique = index.unique || index.primary_key || index.id_key;
        std::iter::once(exists)
            .chain(unique.then_some(open_and_delete).into_iter().flatten())
            .map(|(suffix, extra, out)| {
                let args = args.iter().cloned().chain(extra).collect();
                (
                    suffix.to_string(),
                    (generated_procedure(args, false, Some(out)), false),
                )
            })
            .collect()
    }

    // IRIS keeps the three namespaces apart by syntax: `..#X` is a parameter, `..X(...)` a method
    // before a property, bare `..X` a property only, never a method, even one without arguments.
    pub(crate) fn resolve_mem<N: Namespace>(
        &self,
        ty: &Ty,
        mem: String,
        range: bindings::Range,
        own: bool,
    ) -> Result<Member<N::Ty>, TypeError> {
        let Ty::OREF(obj) = ty else {
            return Ok(Member::undeclared(N::Ty::default()));
        };
        if let Some(found) = self.lookup_mem::<N>(ty, &mem, range, own)? {
            return Ok(found);
        }
        // No class declares it: reported softly, and taken as present and untyped, so every such
        // access is reported and the rest of the body still checked.
        let err = {
            let names = self.names::<N>(obj.name());
            let mut suggestions =
                crate::check::top_similar(&mem, names.iter().map(String::as_str), 5);
            let mut has_get = false;
            if N::NS == MemberNs::Property {
                // Every `%Get`-style accessor regardless of score: a misspelled property is
                // often one, and a `%Get` means the class may take any name.
                for name in self.names::<Methods>(obj.name()) {
                    has_get |= name.eq_ignore_ascii_case("%Get");
                    if name.starts_with("%Get")
                        && !suggestions.iter().any(|s| s.eq_ignore_ascii_case(&name))
                    {
                        suggestions.push(name);
                    }
                }
            }
            // `##class(X)` names X itself, not some subclass's instance: what it lacks is missing.
            if obj.is_ins() && self.is_abstract(obj.name()) {
                QuasiTypeError::cannot_find_member_on_abstract(obj.to_string(), mem, suggestions)
            } else {
                QuasiTypeError::cannot_find_member(obj.to_string(), mem, suggestions, has_get)
            }
        };
        self.add_soft_diagnostic(err.into_error(|| range).into());
        Ok(Member::undeclared(N::Ty::default()))
    }

    // `mem` of `ty` as some class declares it, `None` when none does; a member of an unresolved
    // `Any`/`This`/`..#NAME` type (the last a mask bug) is taken as present and untyped.
    pub(crate) fn lookup_mem<N: Namespace>(
        &self,
        ty: &Ty,
        mem: &str,
        range: bindings::Range,
        own: bool,
    ) -> Result<Option<Member<N::Ty>>, TypeError> {
        let Ty::OREF(obj) = ty else {
            return Ok(Some(Member::undeclared(N::Ty::default())));
        };
        let at = |err: FindError| QuasiTypeError::from(err).into_error(|| range);
        let mut found = self.find::<N>(obj, mem, own, range).map_err(at)?;
        // Dynamic dispatch: a class overriding the hook takes any member name no class has.
        if found.is_none() {
            let hook = |hook: &str| Some(self.find::<Methods>(obj, hook, own, range).ok()??.ty);
            found = N::dispatched(obj, hook).map(Member::undeclared);
        }
        Ok(found)
    }

    // Where IRIS finds `mem` on `obj`: each class along the chain in precedence order, its own
    // declared members then its generated ones, so the first match is the one IRIS compiles in.
    // The walk ends at a class nobody can see, since whatever it declares would shadow the rest;
    // `own = false` starts at the superclasses, for `##super`.
    fn find<N: Namespace>(
        &self,
        obj: &ClsOrIns,
        mem: &str,
        own: bool,
        range: bindings::Range,
    ) -> Result<Option<Member<N::Ty>>, FindError> {
        let (ClsOrIns::Ins(receiver) | ClsOrIns::Cls(receiver)) = obj;
        // A missing superclass is reported where it is named; the receiver's class is for here.
        if own && self.resolve(&receiver.name).is_none() {
            return Err(FindError::NoSuchClass(receiver.name.clone()));
        }
        let found = self
            .ancestry(&receiver.name)
            .skip(usize::from(!own))
            .find_map(|(cls, resolved)| {
                let found = match &resolved {
                    None => return None,
                    // Past a class nobody can see, the member may be anything: taken on trust, with
                    // a hint -- except what every object class has.
                    Some(Class::Unknown(reason)) => {
                        // Past a class nobody can see, the methods every object class gets from
                        // `%SystemBase`/`%RegisteredObject` are taken as present and untyped --
                        // unless `cls` is another hierarchy root.
                        let registered = REGISTERED_OBJECT_MEMBERS.contains(&mem)
                            && !["%Library.DataType", "%Library.FunctionalIndex"]
                                .contains(&cls.as_str());
                        let proc = if mem == "%New" {
                            TyProcedureOrAny::Procedure(generated_procedure(
                                vec![],
                                true,
                                Some(Ty::This),
                            ))
                        } else {
                            TyProcedureOrAny::Any
                        };
                        match registered.then(|| N::method(proc)).flatten() {
                            Some(ty) => Found {
                                member: Member::undeclared(ty),
                                is_instance_only: false,
                                origin: cls.clone(),
                            },
                            None => {
                                let err = match reason {
                                    Some(reason) => TypeError::iris_lookup_failed(
                                        DocKind::Cls,
                                        cls.clone(),
                                        reason.clone(),
                                        range,
                                    ),
                                    None => TypeError::unconfirmed_member(
                                        receiver.name.clone(),
                                        mem.to_string(),
                                        cls != receiver.name,
                                        self.workspace.iris.is_some(),
                                        range,
                                    ),
                                };
                                self.add_soft_diagnostic(err.into());
                                return Some(Ok(Some(Member::undeclared(N::Ty::default()))));
                            }
                        }
                    }
                    Some(_) => {
                        // Every generated member is a method, so only a method lookup consults them,
                        // and only once it's needed: building the table looks properties up itself.
                        let generated = || {
                            let table = (N::NS == MemberNs::Method)
                                .then(|| self.generated_members(&cls, &resolved))?;
                            let (proc, is_instance_only) = table.get(mem).cloned()?;
                            let proc = N::method(TyProcedureOrAny::Procedure(proc))?;
                            Some(Found {
                                member: Member::undeclared(proc),
                                is_instance_only,
                                origin: cls.clone(),
                            })
                        };
                        self.declared_in::<N>(&cls, &resolved, mem)
                            .or_else(generated)?
                    }
                };
                if found.is_instance_only && !obj.is_ins() {
                    return Some(Err(FindError::InstanceOnly(mem.to_string())));
                }
                let mut member = found.member;
                member.ty.instantiate(receiver, &found.origin);
                Some(Ok(Some(member)))
            });
        found.unwrap_or(Ok(None))
    }

    // `mem` as `cls` itself declares it, in the workspace or as IRIS reports it.
    fn declared_in<N: Namespace>(
        &self,
        cls: &str,
        resolved: &Option<Class>,
        mem: &str,
    ) -> Option<Found<N::Ty>> {
        match resolved {
            Some(Class::Workspace(uri, details)) => {
                let member = details.member(N::NS, mem)?;
                let (mut ty, is_instance_only) = N::declared(member, uri)?;
                // A member's declared type names are written relative to the declaring class.
                ty.class_names(&mut |t| self.qualify_ty(t, &details.name.it));
                let location = bindings::Location::new(uri.to_string(), member.range);
                Some(Found {
                    member: Member {
                        ty,
                        location: Some(location),
                    },
                    is_instance_only,
                    origin: details.name.it.clone(),
                })
            }
            Some(Class::Iris(info)) => {
                let named = |m: &&bindings::MemberInfo| {
                    m.name.eq_ignore_ascii_case(mem)
                        || m.aliases.iter().any(|a| a.eq_ignore_ascii_case(mem))
                };
                let member = (info.members.iter())
                    .find(|m| named(m) && MemberNs::of_info(&m.kind) == N::NS)
                    .cloned()
                    // One IRIS doesn't report, the mask may declare.
                    .or_else(|| crate::mask::declared(cls, mem, N::NS).cloned())?;
                iris_found::<N>(member, cls)
            }
            Some(Class::Unknown(_)) | None => None,
        }
    }

    // Every `Include` declared by a superclass of `cls`, open in this workspace or reached
    // through IRIS.
    pub(crate) fn inherited_include_names(
        &self,
        cls: &crate::details::ClassDetails,
    ) -> Vec<String> {
        (self.ancestry(&cls.name.it).skip(1))
            .flat_map(|(_, resolved)| match resolved {
                Some(Class::Workspace(_, details)) => {
                    details.include_names.iter().map(|i| i.it.clone()).collect()
                }
                Some(Class::Iris(info)) => info.include.clone(),
                Some(Class::Unknown(_)) | None => vec![],
            })
            .collect()
    }

    // `ty` as written in class `context`: its class names resolved as `qualify_class_name` does.
    pub(crate) fn qualify_ty(&self, ty: &mut Ty, context: &str) {
        if let Ty::OREF(ClsOrIns::Ins(c) | ClsOrIns::Cls(c)) = ty {
            c.name = self.qualify_class_name(&c.name, Some(context));
            for arg in c.generics.values_mut() {
                *arg = self.qualify_class_name(arg, Some(context));
            }
        }
    }

    // IRIS resolves an unqualified class name (`Extends AbstractModel`, `##class(Date)`) against
    // the referencing class's own package, then its `Import`s, then `User`.
    pub(crate) fn qualify_class_name(&self, name: &str, context: Option<&str>) -> String {
        let mut name = name.to_string();
        normalize_class_name(&mut name);
        if name.contains('.') {
            return name;
        }
        let mut candidates = vec![];
        if let Some(context) = context {
            if let Some((package, _)) = context.rsplit_once('.') {
                candidates.push(format!("{package}.{name}"));
            }
            // The `Import`s of the context, then of the ancestors it inherits them from. Finding
            // the ancestors resolves the context's `Extends`, so while that is under way only its
            // own apply.
            let classes: Vec<Option<Class>> =
                if self.qualifying.borrow_mut().insert(context.to_string()) {
                    let classes = self.ancestry(context).map(|(_, class)| class).collect();
                    self.qualifying.borrow_mut().remove(context);
                    classes
                } else {
                    vec![self.resolve(context)]
                };
            for class in classes.into_iter().flatten() {
                let imports = match &class {
                    Class::Workspace(_, details) => details.imports.clone(),
                    Class::Iris(info) => info.import.clone().unwrap_or_default(),
                    Class::Unknown(_) => vec![],
                };
                candidates.extend(imports.iter().map(|p| format!("{p}.{name}")));
            }
        }
        candidates.push(format!("User.{name}"));
        candidates
            .into_iter()
            .find(|candidate| self.class_exists(candidate))
            .unwrap_or(name)
    }

    pub(crate) fn class_exists(&self, cls: &str) -> bool {
        self.resolve(cls).is_some()
    }

    fn resolve(&self, cls: &str) -> Option<Class> {
        if let Some(uri) = self.resolve_uri(DocKind::Cls, cls) {
            return Some(match self.workspace.cls_details(&uri) {
                Ok(details) => Class::Workspace(uri, details),
                Err(_) => Class::Unknown(None),
            });
        }
        // Without IRIS there is no environment: a class outside the workspace doesn't exist,
        // and every reference to one is an error rather than something taken on trust.
        let iris = self.workspace.iris.as_ref()?;
        // Once per check run: IRIS's answer is large, and a chain is walked many times over.
        if let Some(known) = self.iris_classes.borrow().get(cls) {
            return known.clone().map(Class::Iris);
        }
        match iris.get_cls(cls) {
            Ok(info) => {
                let info = info.map(Arc::new);
                self.iris_classes
                    .borrow_mut()
                    .insert(cls.to_string(), info.clone());
                info.map(Class::Iris)
            }
            Err(reason) => Some(Class::Unknown(Some(reason))),
        }
    }

    // `cls` and its ancestors, nearest first in IRIS's precedence order and each once, resolved
    // as they come so a search stops as soon as it can.
    fn ancestry(&self, cls: &str) -> impl Iterator<Item = (String, Option<Class>)> + '_ {
        let mut seen = HashSet::new();
        let mut pending = vec![cls.to_string()];
        std::iter::from_fn(move || {
            loop {
                let cls = pending.pop()?;
                if !seen.insert(cls.clone()) {
                    continue;
                }
                let resolved = self.resolve(&cls);
                // The direct superclasses in precedence order, as far as `resolved` tells.
                let supers = match &resolved {
                    Some(Class::Workspace(_, details)) => {
                        let mut supers: Vec<String> = (details.extends.iter())
                            .map(|sup| self.qualify_class_name(sup, Some(&cls)))
                            .collect();
                        if details.keywords.inheritance == Some(bindings::Inheritance::Right) {
                            supers.reverse();
                        }
                        supers
                    }
                    Some(Class::Iris(info)) => {
                        let mut supers = info.extends.clone();
                        if info.inheritance == Some(bindings::Inheritance::Right) {
                            supers.reverse();
                        }
                        supers.extend(info.storage.clone());
                        supers.iter_mut().for_each(normalize_class_name);
                        supers
                    }
                    Some(Class::Unknown(_)) | None => vec![],
                };
                pending.extend(supers.into_iter().rev());
                return Some((cls, resolved));
            }
        })
    }

    // Where the class `name` is declared, for go-to-definition.
    pub fn class_location(&self, name: &str) -> Option<bindings::Location> {
        match self.resolve(name)? {
            Class::Workspace(uri, details) => {
                Some(bindings::Location::new(uri, details.name.range))
            }
            Class::Iris(info) => {
                let zero = bindings::Range::point(bindings::Position {
                    line: 0,
                    character: 0,
                });
                Some(bindings::Location::new(info.uri.clone()?, zero))
            }
            Class::Unknown(_) => None,
        }
    }

    pub fn can_assign_to(&self, source: &Ty, target: &Ty) -> bool {
        match (source, target) {
            (_, Ty::Any) | (Ty::Any, _) => true,
            // Anything coerces to a datatype via IRIS's implicit conversion.
            (_, Ty::OREF(ClsOrIns::Ins(target)))
                if self.is_datatype(&target.name).unwrap_or(true) =>
            {
                true
            }
            (Ty::OREF(ClsOrIns::Ins(source)), Ty::OREF(ClsOrIns::Ins(target))) => {
                self.is_subclass(source, target)
            }
            _ => false,
        }
    }

    // Whether `cls` is `[ Abstract ]`, so that a value of the type is some subclass's.
    pub(crate) fn is_abstract(&self, cls: &str) -> bool {
        match self.resolve(cls) {
            Some(Class::Workspace(_, details)) => details.keywords.abstract_,
            Some(Class::Iris(info)) => info.abstract_ || crate::mask::is_abstract(&info.name),
            Some(Class::Unknown(_)) | None => false,
        }
    }

    // `None` for a plain registered object, or when nobody can see.
    fn class_type(&self, cls: &str) -> Option<bindings::ClassType> {
        self.ancestry(cls).find_map(|(_, resolved)| match resolved {
            Some(Class::Workspace(_, details)) => details.keywords.class_type,
            Some(Class::Iris(info)) => info.class_type,
            Some(Class::Unknown(_)) | None => None,
        })
    }

    // Whose parameters (`CAPTION`, ...) a property of type `ty` takes besides the type's own.
    pub(crate) fn property_class(&self, ty: &str) -> Option<&'static str> {
        use bindings::ClassType::*;
        Some(match (self.is_datatype(ty)?, self.class_type(ty)) {
            (true, _) => "%Compiler.Type.Literal",
            (false, Some(Persistent)) => "%Compiler.Type.Object",
            (false, Some(Serial)) => "%Compiler.Type.Embedded",
            (false, Some(Stream)) => "%Compiler.Type.Stream",
            (false, _) => "%Compiler.Type.RegisteredObject",
        })
    }

    // `None` means we can't tell (class outside the workspace with no IRIS connection).
    pub(crate) fn is_datatype(&self, cls: &str) -> Option<bool> {
        let mut unknown = false;
        let yes = self
            .ancestry(cls)
            .find_map(|(cls, resolved)| match resolved {
                _ if cls == "%Library.DataType" => Some(true),
                // Never a datatype, even unresolved, and IRIS lets nothing mix it with one.
                _ if cls == "%Library.RegisteredObject" => Some(false),
                Some(Class::Workspace(_, details)) => (details.keywords.class_type
                    == Some(bindings::ClassType::Datatype))
                .then_some(true),
                // Outside the workspace, IRIS's compiled ClassType is known.
                Some(Class::Iris(info)) => {
                    (info.class_type == Some(bindings::ClassType::Datatype)).then_some(true)
                }
                Some(Class::Unknown(_)) | None => {
                    unknown = true;
                    None
                }
            });
        yes.or((!unknown).then_some(false))
    }

    // With IRIS, a `%`-prefixed system class is trusted, a misspelled one (`%Sting`) included:
    // IRIS lists them all, and the system ones are essentially never workspace files. Without
    // IRIS nothing is, and a class outside the workspace is an error.
    pub(crate) fn check_declared_class_exists(&self, cls: &str, range: bindings::Range) {
        let has_iris = self.workspace.iris.is_some();
        if cls.starts_with('%') && has_iris {
            return;
        }
        let diagnostic = match self.resolve(cls) {
            Some(Class::Workspace(..) | Class::Iris(_)) => return,
            Some(Class::Unknown(Some(reason))) => {
                TypeError::iris_lookup_failed(DocKind::Cls, cls.to_string(), reason, range)
            }
            None if !has_iris => TypeError::cannot_find_class(cls.to_string(), range),
            Some(Class::Unknown(None)) | None => {
                TypeError::unconfirmed_class(cls.to_string(), has_iris, range)
            }
        };
        self.add_soft_diagnostic(diagnostic.into());
    }

    // Where the include file `name` is declared, for go-to-definition on an `Include`.
    pub(crate) fn include_definition(&self, name: &str) -> Option<bindings::Location> {
        self.workspace.inc_definition(name)
    }

    pub(crate) fn resolve_class_includes(
        &self,
        names: &[String],
    ) -> (MacroDefinitions, Vec<crate::macro_expand::FailedInclude>) {
        let (definitions, failed, include_names) = self.workspace.resolve_class_includes(names);
        self.touch_includes(include_names);
        (definitions, failed)
    }

    pub(crate) fn implicit_class_definitions(&self) -> MacroDefinitions {
        let (definitions, include_names) = self.workspace.implicit_class_definitions();
        self.touch_includes(include_names);
        definitions
    }

    fn touch_includes(&self, include_names: HashSet<String>) {
        self.touched
            .borrow_mut()
            .extend(include_names.into_iter().map(|name| (DocKind::Inc, name)));
    }

    pub(crate) fn expand_method_body(
        &self,
        uri: &str,
        header: &str,
        body: &str,
        base_definitions: &MacroDefinitions,
        body_macros: &crate::macro_expand::BodyMacros,
    ) -> Result<(String, SourceMap, MacroDefinitions), bindings::Diagnostic> {
        self.workspace
            .expand_method_body(uri, header, body, base_definitions, body_macros)
    }

    // A workspace-wide routine's label -> signature map, for a cross-routine label call
    // (`Do Foo^OtherRoutine()`) -- same "resolve by name" shape as a
    // cross-class method call. Falls back to `IrisContext::get_rtn` when `name` isn't
    // open in the workspace. `Ok(None)` when the routine can't be parsed (a grammar gap), so its
    // labels are simply unknown; `Err` says why its labels are out of reach.
    pub(crate) fn resolve_rtn_labels(&self, name: &str) -> Result<Option<Labels>, QuasiTypeError> {
        if let Some(uri) = self.resolve_uri(DocKind::Rtn, name) {
            let rtn = self.workspace.rtn_details(&uri).ok();
            return Ok(rtn.map(|rtn| labels_of(&rtn.map.expanded_olc, &rtn.routine, Some(&uri))));
        }
        let Some(iris) = self.workspace.iris.as_ref() else {
            return Err(QuasiTypeError::cannot_find_routine(name.into()));
        };
        let info = iris
            .get_rtn(name)
            .map_err(|reason| {
                QuasiTypeError::iris_lookup_failed(DocKind::Rtn, name.into(), reason)
            })?
            .ok_or_else(|| QuasiTypeError::routine_not_on_iris(name.into()))?;
        let labels = info
            .labels
            .ok_or_else(|| QuasiTypeError::routine_without_source(name.into()))?;
        let labels = labels.into_iter().map(|label| {
            let proc = label
                .signature
                .map(|signature| Arc::new(TyProcedure::from_label(signature, info.uri.clone())));
            (
                label.name,
                proc.map_or(TyProcedureOrAny::Any, TyProcedureOrAny::Procedure),
            )
        });
        Ok(Some(labels.collect()))
    }

    // `""` is also the null OREF (`$$$NULLOREF`), so it goes wherever an object is expected:
    // `Set`, `#Dim` defaults, arguments and return values alike.
    pub(crate) fn assert_value_compatible(
        &self,
        source: &PowerTy,
        target: &Ty,
    ) -> Result<(), Incompatible> {
        if matches!(source, PowerTy::Const(Const::String(s)) if s.is_empty())
            && matches!(target, Ty::OREF(_))
        {
            return Ok(());
        }
        let source = source
            .to_maybe_ty(self)
            .maybe_defined()
            .cloned()
            .unwrap_or_default();
        self.assert_compatible(source, target)
    }

    pub(crate) fn assert_compatible(&self, source: Ty, target: &Ty) -> Result<(), Incompatible> {
        if self.can_assign_to(&source, target) {
            return Ok(());
        }
        // Narrowing an abstract class to a subclass: the value is some subclass's already.
        let downcast = match (&source, target) {
            (Ty::OREF(ClsOrIns::Ins(s)), Ty::OREF(ClsOrIns::Ins(t))) => {
                self.is_abstract(&s.name) && self.is_subclass(t, s)
            }
            _ => false,
        };
        Err(Incompatible {
            source,
            target: target.clone(),
            downcast,
        })
    }

    fn is_subclass(&self, source: &ClassTy, target: &ClassTy) -> bool {
        self.merge_class(Cow::Borrowed(source), Cow::Borrowed(target)) == *target
    }

    pub fn merge_ty(&self, x: Ty, y: Ty) -> Ty {
        match (x, y) {
            (Ty::OREF(ClsOrIns::Ins(x)), Ty::OREF(ClsOrIns::Ins(y))) => {
                Ty::from(self.merge_class(Cow::Owned(x), Cow::Owned(y)))
            }
            _ => Ty::Any,
        }
    }

    fn merge_class(&self, x: Cow<'_, ClassTy>, y: Cow<'_, ClassTy>) -> ClassTy {
        if x == y {
            return x.into_owned();
        }
        // Same class, different arguments: merge each argument both sides give.
        if x.name == y.name {
            let generics = x
                .generics
                .iter()
                .filter_map(|(param, a)| {
                    let b = y.generics.get(param)?;
                    let merged = self.merge_class(
                        Cow::Owned(ClassTy::from(a.clone())),
                        Cow::Owned(ClassTy::from(b.clone())),
                    );
                    Some((param.clone(), merged.name))
                })
                .collect();
            return ClassTy {
                name: x.name.clone(),
                generics,
            };
        }
        let y_ancestors: HashSet<String> = self.ancestors(&y.name).into_iter().collect();
        for a in self.ancestors(&x.name) {
            if y_ancestors.contains(&a) {
                return ClassTy::from(a);
            }
        }
        ClassTy::from("%Any".to_string())
    }

    // `cls` and its ancestors, nearest first. Not IRIS's exact MRO, but any
    // shared ancestor is a sound upper bound for merging.
    fn ancestors(&self, cls: &str) -> Vec<String> {
        self.ancestry(cls).map(|(cls, _)| cls).collect()
    }

    fn is_persistent(&self, cls: &str) -> bool {
        self.ancestry(cls)
            .any(|(cls, _)| matches!(cls.as_str(), "%Library.Persistent" | "%Persistent"))
    }

    pub fn new(workspace: &'a WorkspaceStruct) -> Self {
        Self {
            workspace,
            touched: RefCell::new(HashSet::new()),
            references: RefCell::new(Vec::new()),
            soft_diagnostics: RefCell::new(Vec::new()),
            generated: RefCell::new(HashMap::new()),
            iris_classes: RefCell::new(HashMap::new()),
            suppress_soft_diagnostics: Cell::new(false),
            qualifying: RefCell::new(HashSet::new()),
        }
    }

    pub(crate) fn touched_names(&self) -> HashSet<Name> {
        self.touched.borrow().clone()
    }

    fn resolve_uri(&self, kind: DocKind, name: &str) -> Option<String> {
        self.touched.borrow_mut().insert((kind, name.to_string()));
        self.workspace.resolve_uri(kind, name)
    }

    pub fn reference(&self, source: bindings::Range, target: bindings::Location) {
        self.references.borrow_mut().push((source, target));
    }

    pub fn references(&self) -> Vec<(bindings::Range, bindings::Location)> {
        self.references.borrow().clone()
    }

    pub fn soft_diagnostics(&self) -> Vec<bindings::Diagnostic> {
        self.soft_diagnostics.borrow().clone()
    }

    pub fn add_soft_diagnostic(&self, diagnostics: bindings::Diagnostic) {
        if self.suppress_soft_diagnostics.get() {
            return;
        }
        self.soft_diagnostics.borrow_mut().push(diagnostics);
    }

    // Brackets a loop's throwaway priming pass (see `Flow::with_loop`): returns the previous
    // state, to be handed back to `restore_soft_diagnostics` afterward.
    pub(crate) fn suppress_soft_diagnostics(&self) -> bool {
        self.suppress_soft_diagnostics.replace(true)
    }

    pub(crate) fn restore_soft_diagnostics(&self, was_suppressed: bool) {
        self.suppress_soft_diagnostics.set(was_suppressed);
    }

    pub(crate) fn soft_diagnostics_suppressed(&self) -> bool {
        self.suppress_soft_diagnostics.get()
    }

    pub(crate) fn soft_diagnostics_mark(&self) -> usize {
        self.soft_diagnostics.borrow().len()
    }

    pub(crate) fn soft_diagnostics_since(&self, mark: usize) -> Vec<bindings::Diagnostic> {
        self.soft_diagnostics.borrow_mut().split_off(mark)
    }

    pub(crate) fn references_mark(&self) -> usize {
        self.references.borrow().len()
    }

    pub(crate) fn references_since(
        &self,
        mark: usize,
    ) -> Vec<(bindings::Range, bindings::Location)> {
        self.references.borrow_mut().split_off(mark)
    }

    pub fn merge_power_ty(&self, this: PowerTy, that: PowerTy) -> PowerTy {
        if this == that {
            this
        } else {
            self.merge_maybe_ty(this.to_maybe_ty(self), that.to_maybe_ty(self))
                .into()
        }
    }

    pub fn merge_maybe_ty(&self, lhs: MaybeTy, rhs: MaybeTy) -> MaybeTy {
        match (lhs, rhs) {
            (MaybeTy::Undefined, MaybeTy::Plain(ty)) | (MaybeTy::Plain(ty), MaybeTy::Undefined) => {
                MaybeTy::Maybe(ty)
            }
            (MaybeTy::Undefined, ty) | (ty, MaybeTy::Undefined) => ty,
            (MaybeTy::Plain(x), MaybeTy::Plain(y)) => MaybeTy::Plain(self.merge_ty(x, y)),
            (MaybeTy::Maybe(x) | MaybeTy::Plain(x), MaybeTy::Maybe(y) | MaybeTy::Plain(y)) => {
                MaybeTy::Maybe(self.merge_ty(x, y))
            }
        }
    }
}

pub(crate) struct Incompatible {
    source: Ty,
    target: Ty,
    // The source is abstract and the target one of its subclasses: a warning, not an error.
    downcast: bool,
}

impl Incompatible {
    pub(crate) fn assign(self, name: String) -> QuasiTypeError {
        if self.downcast {
            QuasiTypeError::downcast_from_abstract(name, self.source, self.target)
        } else {
            QuasiTypeError::assign_incompatible(name, self.source, self.target)
        }
    }

    pub(crate) fn general(self) -> QuasiTypeError {
        if self.downcast {
            QuasiTypeError::type_downcast(self.source, self.target)
        } else {
            QuasiTypeError::type_incompatible(self.source, self.target)
        }
    }
}

pub struct WorkspaceEnv<'a> {
    workspace: &'a WorkspaceStruct,
    // Every name this check looked up, found or not -- see `WorkspaceStruct::record_deps`.
    touched: RefCell<HashSet<Name>>,
    references: RefCell<Vec<(bindings::Range, bindings::Location)>>,
    soft_diagnostics: RefCell<Vec<bindings::Diagnostic>>,
    generated: RefCell<HashMap<String, Rc<HashMap<String, Generated>>>>,
    // IRIS's description of each class asked about, or `None` for one it hasn't got.
    iris_classes: RefCell<HashMap<String, Option<Arc<bindings::ClassInfo>>>>,
    // Set during a loop's throwaway priming pass (see `Flow::with_loop`) — its findings reflect
    // pre-loop bindings only, not real, so every soft diagnostic raised meanwhile is dropped.
    suppress_soft_diagnostics: Cell<bool>,
    // The classes whose `Extends` are being resolved, whose ancestors `qualify_class_name` can't
    // consult yet.
    qualifying: RefCell<HashSet<String>>,
}

pub struct ClassDocument {
    pub class: ClassTy,
    // `[ Language = ... ]`: what its methods are written in unless they say otherwise.
    pub language: Option<bindings::Language>,
    pub macro_definitions: MacroDefinitions,
    pub body_macros: RefCell<crate::macro_expand::BodyMacros>,
}

impl ClassDocument {
    pub fn new(
        class: ClassTy,
        language: Option<bindings::Language>,
        macro_definitions: MacroDefinitions,
    ) -> Self {
        Self {
            class,
            language,
            macro_definitions,
            body_macros: Default::default(),
        }
    }
}

pub struct DocumentEnv<'a> {
    pub base: &'a WorkspaceEnv<'a>,
    olc: &'a OLC,
    uri: &'a str,
    pub inlay_hints: RefCell<Vec<bindings::InlayHint>>,
    pub hover_types: RefCell<Vec<(bindings::Range, String)>>,
    suppress_hints: Cell<bool>,
    // How the text `olc` indexes was expanded from the document, when it was.
    source_map: Option<&'a SourceMap>,
}

impl<'a> DocumentEnv<'a> {
    pub fn new(base: &'a WorkspaceEnv, olc: &'a OLC, uri: &'a str) -> Self {
        Self {
            base,
            olc,
            uri,
            inlay_hints: RefCell::new(vec![]),
            hover_types: RefCell::new(vec![]),
            suppress_hints: Cell::new(false),
            source_map: None,
        }
    }

    // The workspace's style rules, if a style was given.
    pub(crate) fn style(&self) -> Option<&crate::style::Styles> {
        self.base.workspace.style.as_ref()
    }

    pub fn with_source_map(mut self, source_map: &'a SourceMap) -> Self {
        self.source_map = Some(source_map);
        self
    }

    pub fn olc(&self) -> &'a OLC {
        self.olc
    }

    pub fn for_routine<'b>(
        &'b self,
        labels: ScopedLabels,
        macro_definitions: &'b MacroDefinitions,
    ) -> CodeEnv<'b> {
        CodeEnv {
            base: self,
            is_instance: false,
            offset: 0,
            this: None,
            labels: Some(Rc::new(labels)),
            method_name: None,
            macro_definitions: Some(macro_definitions),
            procedure_block: false,
            trap_joins: Default::default(),
            label_writes: Default::default(),
            in_try: false,
        }
    }

    pub fn for_member(
        &self,
        offset: usize,
        this: Option<&'a ClassTy>,
        is_instance: bool,
        method_name: &'a str,
        macro_definitions: &'a MacroDefinitions,
        procedure_block: bool,
    ) -> CodeEnv<'_> {
        CodeEnv {
            base: self,
            is_instance,
            offset,
            this,
            labels: None,
            method_name: Some(method_name),
            procedure_block,
            trap_joins: Default::default(),
            label_writes: Default::default(),
            in_try: false,
            macro_definitions: Some(macro_definitions),
        }
    }
}

/// label -> signature, for the subroutines/procedures declared in a routine. Plain data: built
/// once per routine and memoized on the workspace (which the MCP server needs to be `Send`), for
/// resolving `label^routine` calls into it.
// A routine's labels: a signature where one is known, else Any for a label known by name only.
pub type Labels = HashMap<String, TyProcedureOrAny>;

/// The labels in scope while checking one body: the signatures plus per-entry state that exists
/// only during checking (`LabelEntry::output` is a live `Rc` cell, so this map isn't `Send`).
pub type ScopedLabels = HashMap<String, LabelEntry>;

#[derive(Clone)]
pub struct LabelEntry {
    pub proc: Arc<TyProcedure>,
    // Inside a procedure block, the routine's own labels: a bare `Goto` to one fails to compile
    // (IRIS: <NOLINE>), though `Do`/`$$` may call them.
    pub outside_block: bool,
    // The label after this one, which it runs into when it falls off its end. `None` for the
    // last, or when a procedure block follows.
    pub next: Option<String>,
    // A label inside a method or procedure body, as opposed to one of the routine's own.
    pub nested: bool,
    // What its returns are held to: the body's when a `Goto` in the body reaches it, the label
    // before it's when that one falls through, else its own when checked. A nested label only
    // `Do`ne, or reached by an error trap, returns for its caller, so nothing is assumed of it.
    pub output: OnceCell<OutputCell>,
}

// `labels` in source order, each flagged when it's a procedure block.
pub(crate) fn scoped_labels(uri: &str, labels: &[(&RoutineElementDetails, bool)]) -> ScopedLabels {
    labels
        .iter()
        .enumerate()
        .map(|(i, (details, _))| {
            let next = labels
                .get(i + 1)
                .filter(|(_, is_procedure)| !is_procedure)
                .map(|(next, _)| next.name.it.clone());
            let entry = LabelEntry {
                proc: Arc::new(details.to_ty_procedure(Some(uri))),
                outside_block: false,
                nested: false,
                next,
                output: OnceCell::new(),
            };
            (details.name.it.clone(), entry)
        })
        .collect()
}

/// Builds `routine`'s label map — shared by `check_rtn` (the current routine) and
/// `WorkspaceEnv::resolve_rtn_labels` (some other routine already in the workspace).
pub(crate) fn labels_of(olc: &OLC, routine: &cos::Routine, uri: Option<&str>) -> Labels {
    routine
        .subroutines
        .iter()
        .flat_map(|subs| subs.items.iter())
        .filter_map(|elem| crate::details::routine_element_kind(olc, elem))
        .map(|details| {
            (
                details.name.it.clone(),
                TyProcedureOrAny::Procedure(Arc::new(details.to_ty_procedure(uri))),
            )
        })
        .collect()
}

#[derive(Clone)]
pub struct CodeEnv<'a> {
    this: Option<&'a ClassTy>,
    base: &'a DocumentEnv<'a>,
    is_instance: bool,
    offset: usize,
    labels: Option<Rc<ScopedLabels>>,
    // The enclosing method's name, when known — `##super(...)` calls the superclass's version
    // of this same method, so resolving it needs to know which method that is.
    method_name: Option<&'a str>,
    // The enclosing class's own resolved `Include`s, for suggesting a similarly named macro on
    // an undefined-macro error -- `None` outside a class method (e.g. a routine).
    macro_definitions: Option<&'a MacroDefinitions>,
    // A procedure block (a method body, `label() {...}`), where IRIS refuses the legacy forms
    // `TypeErrorKind::NotAllowedIn` reports.
    procedure_block: bool,
    // Error handlers armed by the bodies checked so far, each with the join of the states it can
    // be entered from (see `Flow::arm_trap`), until its label's own check takes it.
    trap_joins: RefCell<HashMap<String, Join>>,
    // What each nested label of this body assigns (see `check_block_body`), shared with the envs
    // derived for the body's blocks.
    label_writes: Rc<RefCell<HashMap<String, HashSet<String>>>>,
    // Inside a `Try` block, where IRIS refuses to set a trap and a `Throw` goes to the `Catch`.
    in_try: bool,
}

impl<'a> CodeEnv<'a> {
    pub(crate) fn style(&self) -> Option<&crate::style::Styles> {
        self.base.style()
    }

    pub(crate) fn soft_diagnostic(&self, err: TypeError) {
        self.base.base.add_soft_diagnostic(err.into())
    }

    pub(crate) fn qualify_class_name(&self, name: &str) -> String {
        self.base
            .base
            .qualify_class_name(name, self.this.map(|this| this.name.as_str()))
    }

    // The labels of a brace-delimited body on top of the enclosing ones; a nested label shadows
    // a same-named outer one.
    pub(crate) fn with_nested_labels(
        &self,
        nested: ScopedLabels,
        is_procedure: bool,
    ) -> CodeEnv<'a> {
        let mut labels: ScopedLabels = self
            .labels
            .iter()
            .flat_map(|labels| labels.iter())
            .map(|(name, label)| {
                let label = LabelEntry {
                    outside_block: is_procedure || label.outside_block,
                    ..label.clone()
                };
                (name.clone(), label)
            })
            .collect();
        labels.extend(nested.into_iter().map(|(name, mut label)| {
            label.nested = true;
            (name, label)
        }));
        CodeEnv {
            labels: Some(Rc::new(labels)),
            procedure_block: is_procedure || self.procedure_block,
            trap_joins: Default::default(),
            ..self.clone()
        }
    }

    pub(crate) fn label_entry(&self, label: &str) -> Option<&LabelEntry> {
        self.labels.as_ref().and_then(|labels| labels.get(label))
    }

    pub(crate) fn goto_leaves_block(&self, label: &str) -> bool {
        self.label_entry(label)
            .is_some_and(|label| label.outside_block)
    }

    pub(crate) fn in_procedure_block(&self) -> bool {
        self.procedure_block
    }

    // The env of a routine procedure's body, or of a label's, before its own labels are known.
    pub(crate) fn with_procedure_block(&self, procedure_block: bool) -> CodeEnv<'a> {
        CodeEnv {
            procedure_block,
            ..self.clone()
        }
    }

    pub(crate) fn in_try(&self) -> bool {
        self.in_try
    }

    pub(crate) fn with_try(&self) -> CodeEnv<'a> {
        CodeEnv {
            in_try: true,
            ..self.clone()
        }
    }

    pub(crate) fn record_label_writes(&self, label: &str, writes: HashSet<String>) {
        self.label_writes
            .borrow_mut()
            .entry(label.to_string())
            .or_default()
            .extend(writes);
    }

    pub(crate) fn label_writes(&self, label: &str) -> Vec<String> {
        self.label_writes
            .borrow()
            .get(label)
            .map(|writes| writes.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub(crate) fn label_writes_total(&self) -> usize {
        self.label_writes.borrow().values().map(HashSet::len).sum()
    }

    pub(crate) fn deposit_traps(&self, joins: HashMap<String, Join>) {
        self.trap_joins.borrow_mut().extend(joins);
    }

    pub(crate) fn take_trap_join(&self, label: &str) -> Option<Join> {
        self.trap_joins.borrow_mut().remove(label)
    }

    pub(crate) fn is_defined_macro(&self, name: &str) -> bool {
        self.macro_definitions
            .is_some_and(|definitions| definitions.contains_key(name))
    }

    pub(crate) fn known_macro_names(&self) -> impl Iterator<Item = &str> {
        self.macro_definitions
            .into_iter()
            .flatten()
            .map(|(k, _)| k.as_str())
    }
}

impl<'a> CodeEnv<'a> {
    pub(crate) fn line_of(&self, offset: usize) -> usize {
        self.base
            .olc
            .offset_to_line_character(offset + self.offset)
            .0
    }

    // Whether `node` is text a macro expanded to (or an include supplied), rather than text the
    // document spells out.
    pub(crate) fn is_expanded<T>(&self, node: &tygr::Range<T>) -> bool {
        self.base.source_map.is_some_and(|map| {
            map.to_source_offset(node.start + self.offset)
                .is_none_or(|(_, precise)| !precise)
        })
    }
}

impl<'a> Deref for CodeEnv<'a> {
    type Target = WorkspaceEnv<'a>;

    fn deref(&self) -> &Self::Target {
        self.base.base
    }
}

impl<'a> CodeEnv<'a> {
    pub(crate) fn this(&self) -> Result<ClsOrIns, QuasiTypeError> {
        let this = self.this.ok_or_else(QuasiTypeError::not_in_class)?.clone();
        Ok(if self.is_instance {
            ClsOrIns::Ins(this)
        } else {
            ClsOrIns::Cls(this)
        })
    }

    // `this`, or `None` under a warning outside a class: a routine may well run with `$this` set,
    // by a method that called it, so the caller falls back to `Any`.
    pub(crate) fn this_or_warn(&self, flow: &Flow<'_>) -> Option<ClsOrIns> {
        self.this()
            .map_err(|err| flow.add_soft_diagnostic(err))
            .ok()
    }

    pub(crate) fn this_ty(&self, flow: &Flow<'_>) -> Ty {
        self.this_or_warn(flow).map_or(Ty::Any, Ty::OREF)
    }

    // `##super(...)`'s target: the enclosing method's own name, resolved against `this`'s
    // superclasses instead of `this` itself.
    pub(crate) fn resolve_super_mstr(
        &self,
        range: bindings::Range,
    ) -> Result<TyProcedureOrAny, TypeError> {
        let this = self.this().map_err(|err| err.into_error(|| range))?;
        let mem = self
            .method_name
            .ok_or_else(QuasiTypeError::not_in_class)
            .map_err(|err| err.into_error(|| range))?
            .to_string();
        match self.find::<Methods>(&this, &mem, false, range) {
            Ok(Some(method)) => Ok(method.ty),
            Ok(None) => {
                let err = QuasiTypeError::super_without_method(mem, this.name().to_string());
                Err(err.into_error(|| range))
            }
            Err(err) => Err(QuasiTypeError::from(err).into_error(|| range)),
        }
    }

    pub(crate) fn resolve_label(
        &self,
        label: Option<String>,
        routine: Option<String>,
    ) -> Result<TyProcedureOrAny, QuasiTypeError> {
        match routine {
            // `label^routine`: resolved against that routine's own labels -- in the workspace,
            // or via IRIS (see `resolve_rtn_labels`).
            Some(routine) => match label {
                Some(label) => match self.resolve_rtn_labels(&routine) {
                    // A routine IRIS has without source: the curated signatures still cover the
                    // labels the system macros call.
                    Err(
                        err @ QuasiTypeError::WithoutRange(
                            TypeErrorKind::RoutineWithoutSource { .. }
                            | TypeErrorKind::IrisLookupFailed { .. },
                            _,
                        ),
                    ) => crate::mask::curated_label(&routine, &label)
                        .map(|signature| {
                            TyProcedureOrAny::Procedure(Arc::new(TyProcedure::from_label(
                                signature, None,
                            )))
                        })
                        .ok_or(err),
                    labels => match labels? {
                        Some(labels) => match labels.get(&label) {
                            Some(proc) => Ok(proc.clone()),
                            None => Err(QuasiTypeError::cannot_find_label(label, Some(routine))),
                        },
                        None => Ok(TyProcedureOrAny::Any),
                    },
                },
                None => self
                    .resolve_rtn_labels(&routine)
                    .map(|_| TyProcedureOrAny::Any),
            },
            // No explicit routine: the current one.
            None => match label {
                Some(label) => match self.labels.as_ref().and_then(|labels| labels.get(&label)) {
                    Some(label) => Ok(TyProcedureOrAny::Procedure(label.proc.clone())),
                    None => Err(QuasiTypeError::cannot_find_label(label, None)),
                },
                None => Ok(TyProcedureOrAny::Any),
            },
        }
    }

    pub fn hint(&self, hint: InlayHint) {
        if !self.base.suppress_hints.get() {
            self.base.inlay_hints.borrow_mut().push(hint)
        }
    }

    // Records a local variable's inferred type at `range`, for hover — same throwaway-pass
    // suppression as `hint`, since a priming pass shouldn't leave stale/duplicate entries.
    pub fn hover_type(&self, range: bindings::Range, ty: String) {
        if !self.base.suppress_hints.get() {
            self.base.hover_types.borrow_mut().push((range, ty))
        }
    }

    // Runs `f` with inlay-hint emission suppressed, for a throwaway priming pass.
    pub fn with_hints_suppressed<T>(&self, f: impl FnOnce() -> T) -> T {
        let was_suppressed = self.base.suppress_hints.replace(true);
        let result = f();
        self.base.suppress_hints.set(was_suppressed);
        result
    }
}

pub trait Locator: Positions {
    fn uri(&self) -> &str;
}

impl<'a> Locator for DocumentEnv<'a> {
    fn uri(&self) -> &str {
        self.uri
    }
}

impl<'a> Positions for DocumentEnv<'a> {
    fn offset_to_line_character(&self, offset: usize) -> (usize, usize) {
        self.olc.offset_to_line_character(offset)
    }

    fn line_character_to_offset(&self, line_character: (usize, usize)) -> usize {
        self.olc.line_character_to_offset(line_character)
    }
}

impl<'a> Locator for CodeEnv<'a> {
    fn uri(&self) -> &str {
        self.base.uri()
    }
}

// A method body's offsets, shifted to where the body sits in its class.
impl<'a> Positions for CodeEnv<'a> {
    fn offset_to_line_character(&self, offset: usize) -> (usize, usize) {
        self.base.offset_to_line_character(offset + self.offset)
    }

    fn line_character_to_offset(&self, line_character: (usize, usize)) -> usize {
        self.base.line_character_to_offset(line_character) - self.offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_many_relationship_from_iris_binds_its_element_type() {
        let children = bindings::MemberInfo {
            doc: String::new(),
            name: "Children".to_string(),
            aliases: vec![],
            deprecated: false,
            kind: MemberKind::Relationship(bindings::RelationshipInfo {
                t: Some("Ext.Child".to_string()),
                cardinality: Some(bindings::Cardinality::Many),
            }),
            uri: None,
            origin: None,
        };
        let Some((PropertyTy::Value(Ty::OREF(ClsOrIns::Ins(cls))), _)) =
            Properties::<Read>::reported(children)
        else {
            panic!("not an instance")
        };
        assert_eq!(cls.name, "%Library.RelationshipObject");
        assert_eq!(
            cls.generics,
            [("ELEMENTTYPE".to_string(), "Ext.Child".to_string())].into()
        );
    }
}
