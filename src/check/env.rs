use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    ops::Deref,
};

use tygr::{IntoInner, OLC, Range};

use crate::bindings::{ArgMode, InlayHint, MemberKind};
use crate::check::errors::*;
use crate::check::types::*;
use crate::cos::{CodeMode, MethodLanguage};
use crate::details::{MemberDetails, MemberKindSpecificDetails, MemberNs, WithRange};
use crate::{MacroDefinitions, SourceMap, WorkspaceStruct, bindings, cos};

#[derive(Clone)]
pub struct MemberTy {
    is_instance_only: bool,
    // `Property X [ MultiDimensional ]`: `..X(subs)` addresses a node, not a call.
    pub multidimensional: bool,
    pub ty: HOTy,
    // Where this member is declared — only known when resolved directly against a workspace
    // class (not for callee-synthesized members like `%New`, or ones resolved via IRIS).
    pub location: Option<bindings::Location>,
}

enum GeneratableKind {
    Property(Ty),
    // The properties it's defined `On`, in declaration order — the value arguments its
    // generated `Open`/`Exists`/`Delete` methods take, one per property.
    Index(Vec<String>),
}

impl Default for MemberTy {
    fn default() -> Self {
        Self {
            is_instance_only: false,
            multidimensional: false,
            ty: HOTy::Value(Ty::Any),
            location: None,
        }
    }
}

impl TryFrom<&MemberDetails> for MemberTy {
    type Error = ();

    fn try_from(details: &MemberDetails) -> Result<Self, Self::Error> {
        Self::try_from(&details.kind)
    }
}

impl TryFrom<&MemberKindSpecificDetails> for MemberTy {
    type Error = ();

    fn try_from(value: &MemberKindSpecificDetails) -> Result<Self, Self::Error> {
        match value {
            MemberKindSpecificDetails::MethodOrClassMethod(m) => Ok(MemberTy {
                is_instance_only: !m.is_class_method,
                ty: HOTy::Procedure(Arc::new(TyProcedure {
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
                    uri: None,
                    declared_at: None,
                })),
                ..MemberTy::default()
            }),
            MemberKindSpecificDetails::Property {
                ty,
                element_ty,
                keywords,
                ..
            } => Ok(MemberTy {
                is_instance_only: true,
                multidimensional: keywords
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case("MultiDimensional")),
                ty: match (ty, element_ty) {
                    (Some(Ty::OREF(cls)), Some(element)) => HOTy::Collection {
                        kind: if cls.name().ends_with("List") {
                            CollectionClass::List
                        } else {
                            CollectionClass::Array
                        },
                        element: element.clone(),
                    },
                    _ => HOTy::Value(ty.clone().unwrap_or_default()),
                },
                ..MemberTy::default()
            }),
            MemberKindSpecificDetails::Parameter { ty, .. } => Ok(MemberTy {
                is_instance_only: false,
                ty: HOTy::Value(ty.clone().unwrap_or_default()),
                ..MemberTy::default()
            }),
            MemberKindSpecificDetails::ClientMethod
            | MemberKindSpecificDetails::ForeignKey
            | MemberKindSpecificDetails::Index { .. }
            | MemberKindSpecificDetails::Projection
            | MemberKindSpecificDetails::Trigger
            | MemberKindSpecificDetails::XData
            | MemberKindSpecificDetails::Storage
            | MemberKindSpecificDetails::Query => Err(()),
        }
    }
}

impl TryFrom<bindings::MemberInfo> for MemberTy {
    type Error = ();

    fn try_from(member: bindings::MemberInfo) -> Result<Self, Self::Error> {
        let uri = member.uri.clone();
        let declared_at = bindings::Range {
            start: member.before,
            end: member.after,
        };
        // Only known when the host that answered this query could map the member to a uri
        // (e.g. not a bare IRIS system class).
        let location = uri
            .clone()
            .map(|uri| bindings::Location::new(uri, declared_at));
        let locate = |mut proc: TyProcedure| {
            proc.uri = uri.clone();
            proc.declared_at = uri.is_some().then_some(declared_at);
            proc
        };
        match member.kind {
            MemberKind::Method(m) => Ok(MemberTy {
                is_instance_only: true,
                ty: HOTy::Procedure(Arc::new(locate(TyProcedure::from(m)))),
                location,
                multidimensional: false,
            }),
            MemberKind::ClassMethod(m) => Ok(MemberTy {
                is_instance_only: false,
                ty: HOTy::Procedure(Arc::new(locate(TyProcedure::from(m)))),
                location,
                multidimensional: false,
            }),
            MemberKind::Property(info) => Ok(MemberTy {
                is_instance_only: true,
                multidimensional: info.multidimensional,
                ty: match info.collection {
                    Some(collection) => HOTy::Collection {
                        kind: match collection {
                            bindings::CollectionKind::List => CollectionClass::List,
                            bindings::CollectionKind::Array => CollectionClass::Array,
                        },
                        element: ty_from_declared(info.t).unwrap_or_default(),
                    },
                    None => HOTy::Value(ty_from_declared(info.t).unwrap_or_default()),
                },
                location,
            }),
            MemberKind::Relationship(t) => Ok(MemberTy {
                is_instance_only: true,
                ty: HOTy::Value(ty_from_declared(t).unwrap_or_default()),
                location,
                multidimensional: false,
            }),
            MemberKind::Parameter(p) => Ok(MemberTy {
                is_instance_only: false,
                ty: HOTy::Value(p.t.as_deref().map(parameter_ty).unwrap_or_default()),
                location,
                multidimensional: false,
            }),
            MemberKind::ClientMethod(_)
            | MemberKind::ForeignKey
            | MemberKind::Index
            | MemberKind::Projection
            | MemberKind::Trigger
            | MemberKind::XData
            | MemberKind::Storage
            | MemberKind::Query => Err(()),
        }
    }
}

// Every non-`Dispatch*`, non-`%New` member `%Library.SystemBase` and `%Library.RegisteredObject`
// contribute to every object class -- cross-checked against InterSystems' own class reference
// (docs.intersystems.com, `%25CSP.Documatic.cls?LIBRARY=%25SYS&CLASSNAME=...`), not memory:
// - https://docs.intersystems.com/irisforhealth20262/csp/documatic/%25CSP.Documatic.cls?LIBRARY=%25SYS&CLASSNAME=%25Library.SystemBase
// - https://docs.intersystems.com/irisforhealth20262/csp/documatic/%25CSP.Documatic.cls?LIBRARY=%25SYS&CLASSNAME=%25Library.RegisteredObject
// `%New` is excluded: it already gets its own, more precise handling above (bridging into
// `%OnNew`'s return type). The `Dispatch*` hooks are excluded too: `%DispatchMethod`/
// `%DispatchClassMethod`/`%DispatchGetProperty`/`%DispatchSetProperty` already get their own,
// more precise handling in `dispatch_member`, and the remaining three
// (`%DispatchGetModified`/`%DispatchSetModified`/`%DispatchSetMultidimProperty`) are abstract
// override points, essentially never called directly, so aren't worth the same treatment.
const REGISTERED_OBJECT_MEMBERS: &[&str] = &[
    // %Library.SystemBase
    "%ClassIsLatestVersion",
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

// `..X(...)` is a method (or a multidimensional property) before it is a plain property `X`;
// bare `..X` the other way round -- IRIS keeps the two apart by syntax.
fn lookup_order(kind: AccessKind) -> &'static [MemberNs] {
    match kind {
        AccessKind::Call => &[MemberNs::HigherOrder, MemberNs::Property],
        AccessKind::Get | AccessKind::Set => &[MemberNs::Property, MemberNs::HigherOrder],
    }
}

impl<'a> WorkspaceEnv<'a> {
    // A `Parameter` is a simple constant-like value, so none of the method-dispatch fallbacks
    // below apply to it.
    fn get_member(
        &self,
        obj: &str,
        mem: &str,
        is_parameter: bool,
        kind: AccessKind,
        is_instance: bool,
    ) -> Option<MemberTy> {
        if is_parameter {
            return self
                .get_member_internal(obj, mem, &[MemberNs::Parameter])
                .or_else(|| self.unverified_any());
        }
        if mem == "%New" {
            // Every object class has `%New`; `%OnNew` only decides its arguments, so failing to
            // resolve it (a superclass outside the workspace, no IRIS connection) leaves the
            // arguments unchecked rather than making `%New` itself unverified.
            let was_unverified = self.unverified.get();
            let proc: TyProcedureOrAny = self
                .get_member_internal(obj, "%OnNew", &[MemberNs::HigherOrder])
                .and_then(|mem| mem.ty.as_procedure().ok())
                .unwrap_or_default();
            self.unverified.set(was_unverified);
            let mut proc = match proc {
                TyProcedureOrAny::Procedure(p) => p.as_ref().clone(),
                TyProcedureOrAny::Any => TyProcedure {
                    normal_args: vec![],
                    variadic_arg: Some(VariadicArg::new(ArgMode::Default, "args".into(), None)),
                    out: None,
                    procedure_block: None,
                    uri: None,
                    declared_at: None,
                },
            };
            proc.out = Some(Ty::from(obj.to_string()));
            Some(MemberTy {
                is_instance_only: false,
                ty: HOTy::Procedure(Arc::new(proc)),
                ..MemberTy::default()
            })
        } else if REGISTERED_OBJECT_MEMBERS.contains(&mem)
            && self.confirmed_outside_system_base(obj) != Some(true)
        {
            // Every *object* class ultimately inherits these from `%Library.SystemBase`/
            // `%Library.RegisteredObject` (via `%RegisteredObject`/`%SerialObject`) -- see
            // `REGISTERED_OBJECT_MEMBERS`. Hardcoded rather than relying on the `extends` chain
            // actually reaching them, same reasoning as `%New` above: that chain is routinely
            // unresolvable (a system class outside the workspace, no IRIS connection) even though
            // the members are always there.
            // Excluded when `confirmed_outside_system_base` rules it out -- a confirmed datatype
            // (`%String`, `%RawString`, ...), a functional-index class, or a class with no real
            // object ancestor at all (an SQL view class, or an isolated one like `%Library.Any`):
            // see that function's own doc comment for the class-hierarchy roots this reflects.
            Some(MemberTy {
                is_instance_only: false,
                ty: HOTy::Value(Ty::Any),
                ..MemberTy::default()
            })
        } else if let Some(mem) = self.get_member_internal(obj, mem, lookup_order(kind)) {
            Some(self.resolve_object_handle(obj, mem))
        } else {
            self.dispatch_member(obj, kind, is_instance)
                .or_else(|| self.generated_member(obj, mem))
                .or_else(|| self.unverified_any())
        }
    }

    // IRIS's dynamic-dispatch methods -- see
    // https://docs.intersystems.com/irislatest/csp/docbook/DocBook.UI.Page.cls?KEY=GOOP_dynamic_dispatch
    // Only `%DispatchGetProperty`'s own return type feeds into the result; the others just
    // signal "dynamically resolvable" and resolve to `Any`.
    //
    // A direct multidimensional property reference isn't distinguished from an ordinary call or
    // set with the wrong arity, so `%DispatchSetMultidimProperty` isn't consulted.
    fn dispatch_member(&self, obj: &str, kind: AccessKind, is_instance: bool) -> Option<MemberTy> {
        match kind {
            AccessKind::Get => {
                let mem = self.find_dispatch_hook(obj, "%DispatchGetProperty")?;
                let ty = match mem.ty.as_procedure().ok()? {
                    TyProcedureOrAny::Procedure(ty_procedure) => {
                        // No declared return type (the common case -- a dispatch method's
                        // whole point is not knowing the resolved property's type ahead of
                        // time) falls back to `Any`, rather than treating it as unresolved.
                        HOTy::Value(ty_procedure.out.clone().unwrap_or_default())
                    }
                    TyProcedureOrAny::Any => HOTy::Value(Ty::Any),
                };
                Some(MemberTy {
                    is_instance_only: true,
                    ty,
                    ..MemberTy::default()
                })
            }
            AccessKind::Set => {
                self.find_dispatch_hook(obj, "%DispatchSetProperty")?;
                Some(MemberTy {
                    is_instance_only: true,
                    ty: HOTy::Value(Ty::Any),
                    ..MemberTy::default()
                })
            }
            AccessKind::Call => {
                let dispatch = if is_instance {
                    "%DispatchMethod"
                } else {
                    "%DispatchClassMethod"
                };
                self.find_dispatch_hook(obj, dispatch)?;
                Some(MemberTy {
                    is_instance_only: is_instance,
                    ty: HOTy::Value(Ty::Any),
                    ..MemberTy::default()
                })
            }
        }
    }

    // Same lookup as `get_member_internal`, but only for the 4 dispatch hooks above: every
    // `%RegisteredObject`/`%SerialObject` inherits a meaningless default implementation of them
    // from `%Library.SystemBase`, so a match only counts if its declaring class differs.
    fn find_dispatch_hook(&self, cls: &str, mem: &str) -> Option<MemberTy> {
        self.find_dispatch_hook_in(cls, mem, &mut HashSet::new())
    }

    fn find_dispatch_hook_in(
        &self,
        cls: &str,
        mem: &str,
        seen: &mut HashSet<String>,
    ) -> Option<MemberTy> {
        const DISPATCH_DEFAULT_ORIGIN: &str = "%Library.SystemBase";
        if !seen.insert(cls.to_string()) {
            return None;
        }
        if let Some(uri) = self.workspace.resolve_cls_uri(cls) {
            self.touched.borrow_mut().insert(uri.clone());
            let details = self.workspace.cls_details(&uri).ok()?;
            if let Some(member) = details.member(MemberNs::HigherOrder, mem) {
                let mut ty: MemberTy = member.deref().try_into().ok()?;
                ty.location = Some(bindings::Location::new(uri.clone(), member.range));
                return Some(ty);
            }
            self.qualified_extends(cls, &details)
                .into_iter()
                .find_map(|sup| self.find_dispatch_hook_in(&sup, mem, seen))
        } else if let Some(iris) = self.workspace.iris.as_ref() {
            let found = iris.get_cls(cls)?.members.into_iter().find(|m| {
                m.name.content.eq_ignore_ascii_case(mem)
                    && m.origin.as_deref() != Some(DISPATCH_DEFAULT_ORIGIN)
            })?;
            MemberTy::try_from(found).ok()
        } else {
            None
        }
    }

    fn generatable_kind(&self, cls: &str, name: &str) -> Option<GeneratableKind> {
        self.generatable_kind_in(cls, name, &mut HashSet::new())
    }

    fn generatable_kind_in(
        &self,
        cls: &str,
        name: &str,
        seen: &mut HashSet<String>,
    ) -> Option<GeneratableKind> {
        if !seen.insert(cls.to_string()) {
            return None;
        }
        let uri = self.workspace.resolve_cls_uri(cls)?;
        self.touched.borrow_mut().insert(uri.clone());
        let details = self.workspace.cls_details(&uri).ok()?;
        if let Some(member) = details
            .member(MemberNs::Property, name)
            .or_else(|| details.member(MemberNs::HigherOrder, name))
            .or_else(|| details.member(MemberNs::Other, name))
        {
            return match &member.deref().kind {
                MemberKindSpecificDetails::Property { ty, .. } => {
                    Some(GeneratableKind::Property(ty.clone().unwrap_or_default()))
                }
                MemberKindSpecificDetails::Index { on_properties } => {
                    Some(GeneratableKind::Index(on_properties.clone()))
                }
                _ => None,
            };
        }
        self.qualified_extends(cls, &details)
            .into_iter()
            .find_map(|sup| self.generatable_kind_in(&sup, name, seen))
    }

    // Best-effort: a superclass outside the workspace (or an IRIS-only generated member) isn't
    // represented.
    pub(crate) fn member_names(&self, cls: &str) -> Vec<String> {
        let mut seen_classes = HashSet::new();
        let mut names = Vec::new();
        self.collect_member_names(cls, &mut seen_classes, &mut names);
        names
    }

    fn collect_member_names(&self, cls: &str, seen: &mut HashSet<String>, names: &mut Vec<String>) {
        if !seen.insert(cls.to_string()) {
            return;
        }
        let Some(uri) = self.workspace.resolve_cls_uri(cls) else {
            // Not a workspace class (e.g. a datatype like %Integer) -- IRIS is the only other
            // source, and doesn't expose superclass-walking the way `cls_details.extends` does,
            // so this is a single, non-recursive lookup rather than a further `collect_*` call.
            if let Some(iris) = self.workspace.iris.as_ref() {
                names.extend(
                    iris.get_cls(cls)
                        .into_iter()
                        .flat_map(|ci| ci.members)
                        .map(|m| m.name.content),
                );
            }
            return;
        };
        self.touched.borrow_mut().insert(uri.clone());
        let Ok(details) = self.workspace.cls_details(&uri) else {
            return;
        };
        names.extend(details.members_by_name.keys().map(|(_, name)| name.clone()));
        for sup in self.qualified_extends(cls, &details) {
            self.collect_member_names(&sup, seen, names);
        }
    }

    // Every class named in `cls`'s own or any ancestor's `[PropertyClass = ...]` -- each one's
    // own `Parameter`s are valid property type-parameters throughout `cls`'s whole subtree (see
    // https://docs.intersystems.com/irislatest/csp/docbook/DocBook.UI.Page.cls?KEY=ROBJ_class_propertyclass,
    // "Effect on Subclasses").
    pub(crate) fn property_class_names(&self, cls: &str) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut classes = Vec::new();
        self.collect_property_classes(cls, &mut seen, &mut classes);
        classes
    }

    fn collect_property_classes(
        &self,
        cls: &str,
        seen: &mut HashSet<String>,
        classes: &mut Vec<String>,
    ) {
        if !seen.insert(cls.to_string()) {
            return;
        }
        let Some(uri) = self.workspace.resolve_cls_uri(cls) else {
            if let Some(iris) = self.workspace.iris.as_ref()
                && let Some(ci) = iris.get_cls(cls)
            {
                classes.extend(ci.property_class_names);
                for sup in ci.extends {
                    self.collect_property_classes(&sup, seen, classes);
                }
            }
            return;
        };
        self.touched.borrow_mut().insert(uri.clone());
        let Ok(details) = self.workspace.cls_details(&uri) else {
            return;
        };
        classes.extend(details.property_class_names.iter().cloned());
        for sup in self.qualified_extends(cls, &details) {
            self.collect_property_classes(&sup, seen, classes);
        }
    }

    // IRIS generates several method families that are never written in source, so a plain
    // name lookup never finds them: an `Index`'s `<Name>Open`/`<Name>Exists`/`<Name>Delete`,
    // and a `Property`'s `<Name>Get`/`<Name>Set`/`<Name>Reset`/`<Name>IsValid`/`<Name>GetObject`
    // (the last only when the property's own type is itself a class, not a plain data type).
    fn generated_member(&self, cls: &str, mem: &str) -> Option<MemberTy> {
        fn procedure(
            normal_args: Vec<NormalArg>,
            out: Option<Ty>,
            is_instance_only: bool,
        ) -> MemberTy {
            MemberTy {
                is_instance_only,
                multidimensional: false,
                ty: HOTy::Procedure(Arc::new(TyProcedure {
                    normal_args,
                    variadic_arg: None,
                    out,
                    procedure_block: Some(vec![]),
                    uri: None,
                    declared_at: None,
                })),
                location: None,
            }
        }
        fn value_arg(ty: Ty) -> Vec<NormalArg> {
            vec![NormalArg::new(
                ArgMode::Default,
                "value".into(),
                Some(ty),
                None,
            )]
        }

        for suffix in ["Open", "Exists", "Delete"] {
            if let Some(name) = mem.strip_suffix(suffix)
                && !name.is_empty()
                && let Some(GeneratableKind::Index(on_properties)) =
                    self.generatable_kind(cls, name)
            {
                let out = match suffix {
                    "Open" => Some(Ty::from(cls.to_string())),
                    _ => Some(Ty::from("%Status")),
                };
                // One value argument per indexed property, in order, typed like that property —
                // e.g. `Index AB On (A, B)` generates `ABOpen(aVal As <A's type>, bVal As <B's
                // type>)`. Falls back to `%Any` for a property we can't resolve the type of.
                let args = on_properties
                    .iter()
                    .map(|property| {
                        let ty = match self.generatable_kind(cls, property) {
                            Some(GeneratableKind::Property(ty)) => ty,
                            _ => Ty::Any,
                        };
                        NormalArg::new(ArgMode::Default, "value".into(), Some(ty), None)
                    })
                    .collect();
                return Some(procedure(args, out, false));
            }
        }
        // The property methods IRIS generates for `Property X` (Defining and Using Classes,
        // "Property Methods"): accessors, validation/normalization, the Logical/Display/Odbc/
        // Storage conversions, and -- for a reference property -- the object accessors.
        const PROPERTY_METHOD_SUFFIXES: &[&str] = &[
            "Get",
            "Set",
            "Reset",
            "IsValid",
            "IsModified",
            "Normalize",
            "LogicalToDisplay",
            "DisplayToLogical",
            "LogicalToOdbc",
            "OdbcToLogical",
            "LogicalToStorage",
            "StorageToLogical",
            "LogicalToXSD",
            "XSDToLogical",
            "GetStored",
            "GetObject",
            "GetObjectId",
            "SetObject",
            "SetObjectId",
            "NewObject",
            "GetSwizzled",
        ];
        for suffix in PROPERTY_METHOD_SUFFIXES {
            let Some(name) = mem.strip_suffix(suffix) else {
                continue;
            };
            let Some(GeneratableKind::Property(ty)) = self.generatable_kind(cls, name) else {
                continue;
            };
            let string = || Some(Ty::from("%String"));
            let status = || Some(Ty::from("%Status"));
            // Only a reference-typed property (its own type is a class, not a plain data type
            // like %String/%Integer) has an object behind it. When we can't tell (the property's
            // own class is outside the workspace, no IRIS connection), assume it does — same
            // permissive-but-flagged treatment as `get_member_internal`'s fallback, via the same
            // `unverified` signal `resolve_obj_mstr` checks.
            let is_reference = || match &ty {
                Ty::OREF(obj) => match self.is_datatype(obj.name()) {
                    Some(is_datatype) => !is_datatype,
                    None => {
                        self.unverified.set(true);
                        true
                    }
                },
                _ => false,
            };
            let id_arg = || {
                vec![NormalArg::new(
                    ArgMode::Default,
                    "id".into(),
                    string(),
                    None,
                )]
            };
            return match *suffix {
                "Get" => Some(procedure(vec![], Some(ty), true)),
                "Set" => Some(procedure(value_arg(ty), status(), true)),
                "Reset" => Some(procedure(vec![], status(), true)),
                "IsModified" => Some(procedure(vec![], Some(Ty::from("%Boolean")), true)),
                "IsValid" => Some(procedure(value_arg(ty.clone()), status(), false)),
                "Normalize" | "DisplayToLogical" | "OdbcToLogical" | "StorageToLogical"
                | "XSDToLogical" => Some(procedure(value_arg(ty.clone()), Some(ty), false)),
                "LogicalToDisplay" | "LogicalToOdbc" | "LogicalToStorage" | "LogicalToXSD" => {
                    Some(procedure(value_arg(ty), string(), false))
                }
                "GetStored" => Some(procedure(id_arg(), Some(ty), false)),
                "GetObject" if is_reference() => Some(procedure(
                    vec![NormalArg::new(
                        ArgMode::Output,
                        "concurrency".into(),
                        Some(Ty::from("%Integer")),
                        None,
                    )],
                    Some(ty),
                    true,
                )),
                "GetObjectId" if is_reference() => Some(procedure(vec![], string(), true)),
                "SetObject" if is_reference() => Some(procedure(value_arg(ty), status(), true)),
                "SetObjectId" if is_reference() => Some(procedure(id_arg(), status(), true)),
                "NewObject" | "GetSwizzled" if is_reference() => {
                    Some(procedure(vec![], Some(ty), true))
                }
                _ => None,
            };
        }
        None
    }

    // `%ObjectHandle` in a method signature stands for an instance of the class the
    // method is called on (e.g. `%New`/`%Open` return it). Substitute the receiver
    // class into the method's arg and return types.
    fn resolve_object_handle(&self, obj: &str, mut mem: MemberTy) -> MemberTy {
        if let HOTy::Procedure(proc) = &mem.ty {
            let mut proc = proc.as_ref().clone();
            let sub = |ty: &mut Option<Ty>| {
                if let Some(Ty::OREF(ClsOrIns::Ins(cty))) = ty
                    && cty.name == "%Library.ObjectHandle"
                {
                    *ty = Some(Ty::from(obj.to_string()));
                }
            };
            for arg in &mut proc.normal_args {
                sub(&mut arg.ty);
            }
            if let Some(arg) = &mut proc.variadic_arg {
                sub(&mut arg.ty);
            }
            sub(&mut proc.out);
            mem.ty = HOTy::Procedure(Arc::new(proc));
        }
        mem
    }

    // Whether `name` is a declared `Parameter` on `cls` -- e.g. %Integer's `MINVAL`, referenced
    // from a property's `(...)` list. `None` if it can't be determined at all.
    pub(crate) fn parameter_exists(&self, cls: &str, name: &str) -> Option<bool> {
        self.unverified.set(false);
        let found = self
            .get_member_internal(cls, name, &[MemberNs::Parameter])
            .is_some();
        if self.unverified.replace(false) {
            None
        } else {
            Some(found)
        }
    }

    // "Did you mean" candidates for `parameter_exists`'s negative case -- `cls`'s own other
    // `Parameter`s (workspace) or `Parameter`-tagged members (IRIS's `get_cls`).
    pub(crate) fn parameter_names(&self, cls: &str) -> Vec<String> {
        if let Some(uri) = self.workspace.resolve_cls_uri(cls) {
            self.touched.borrow_mut().insert(uri.clone());
            if let Ok(details) = self.workspace.cls_details(&uri) {
                return details
                    .members_by_name
                    .keys()
                    .filter(|(ns, _)| *ns == MemberNs::Parameter)
                    .map(|(_, name)| name.clone())
                    .collect();
            }
        }
        self.workspace
            .iris
            .as_ref()
            .and_then(|iris| iris.get_cls(cls))
            .map(|ci| {
                ci.members
                    .into_iter()
                    .filter(|m| matches!(m.kind, MemberKind::Parameter(_)))
                    .map(|m| m.name.content)
                    .collect()
            })
            .unwrap_or_default()
    }

    // `order`: which kinds of member `mem` may be, most likely first (see `lookup_order`).
    fn get_member_internal(&self, cls: &str, mem: &str, order: &[MemberNs]) -> Option<MemberTy> {
        self.get_member_in(cls, mem, order, &mut HashSet::new())
    }

    fn get_member_in(
        &self,
        cls: &str,
        mem: &str,
        order: &[MemberNs],
        seen: &mut HashSet<String>,
    ) -> Option<MemberTy> {
        if !seen.insert(cls.to_string()) {
            return None;
        }
        if let Some(uri) = self.workspace.resolve_cls_uri(cls) {
            self.touched.borrow_mut().insert(uri.clone());
            let details = self.workspace.cls_details(&uri).ok()?;
            order
                .iter()
                .find_map(|&ns| details.member(ns, mem))
                .and_then(|member| {
                    let mut ty: MemberTy = member.deref().try_into().ok()?;
                    ty.location = Some(bindings::Location::new(uri.clone(), member.range));
                    if let HOTy::Procedure(proc) = &ty.ty {
                        let mut proc = proc.as_ref().clone();
                        proc.uri = Some(uri.clone());
                        proc.declared_at = Some(member.range);
                        ty.ty = HOTy::Procedure(Arc::new(proc));
                    }
                    Some(self.qualify_member_ty(ty, cls))
                })
                .or_else(|| {
                    self.qualified_extends(cls, &details)
                        .into_iter()
                        .find_map(|sup| self.get_member_in(&sup, mem, order, seen))
                })
        } else if self.workspace.iris.is_some() {
            self.iris_member(cls, mem, order)
        } else {
            self.unverified.set(true);
            None
        }
    }

    // `##super(...)`'s target: same lookup as `get_member_internal`, but skips `cls`'s own
    // members and starts at its direct superclasses instead — otherwise it would just find the
    // very override that's calling it.
    fn get_super_member(&self, cls: &str, mem: &str, is_instance: bool) -> Option<MemberTy> {
        self.direct_supers(cls)?
            .iter()
            .find_map(|sup| {
                self.get_member_internal(sup, mem, &[MemberNs::HigherOrder])
                    .or_else(|| self.dispatch_member(sup, AccessKind::Call, is_instance))
            })
            .or_else(|| self.unverified_any())
            .map(|mem| self.resolve_object_handle(cls, mem))
    }

    // `cls`'s direct superclasses, normalized. `None` when `cls` itself can't be resolved.
    fn direct_supers(&self, cls: &str) -> Option<Vec<String>> {
        let mut supers: Vec<String> = if let Some(uri) = self.workspace.resolve_cls_uri(cls) {
            self.touched.borrow_mut().insert(uri.clone());
            let details = self.workspace.cls_details(&uri).ok()?;
            return Some(self.qualified_extends(cls, &details));
        } else if let Some(iris) = self.workspace.iris.as_ref() {
            iris.get_cls(cls).map(|ci| ci.extends).unwrap_or_default()
        } else {
            self.unverified.set(true);
            return None;
        };
        supers.iter_mut().for_each(normalize_class_name);
        Some(supers)
    }

    // Whether some superclass of `cls` declares `mem`, i.e. `cls`'s own `mem` overrides it.
    pub(crate) fn is_inherited_member(&self, cls: &str, mem: &str) -> bool {
        REGISTERED_OBJECT_MEMBERS.contains(&mem)
            || self.direct_supers(cls).is_some_and(|supers| {
                supers.iter().any(|sup| {
                    self.get_member_internal(sup, mem, &[MemberNs::HigherOrder])
                        .is_some()
                })
            })
    }

    // Every `Include` declared by a superclass of `cls`, open in this workspace or reached
    // through IRIS.
    pub(crate) fn inherited_include_names(
        &self,
        cls: &crate::details::ClassDetails,
    ) -> Vec<String> {
        let mut names = Vec::new();
        let mut seen = HashSet::new();
        let mut pending = cls.extends.clone();
        while let Some(mut sup) = pending.pop() {
            normalize_class_name(&mut sup);
            if !seen.insert(sup.clone()) {
                continue;
            }
            let Some(uri) = self.workspace.resolve_cls_uri(&sup) else {
                if let Some(info) = self
                    .workspace
                    .iris
                    .as_ref()
                    .and_then(|iris| iris.get_cls(&sup))
                {
                    names.extend(info.includes);
                    pending.extend(info.extends);
                }
                continue;
            };
            self.touched.borrow_mut().insert(uri.clone());
            let Ok(details) = self.workspace.cls_details(&uri) else {
                continue;
            };
            names.extend(details.include_names.iter().cloned());
            pending.extend(self.qualified_extends(&sup, &details));
        }
        names
    }

    // Last resort, once every locally-derivable answer (a direct member, `generated_member`,
    // `%DispatchGetProperty`) has already come up empty: if that emptiness is because some
    // ancestor along the way couldn't be verified (no local definition, no IRIS), assume the
    // member exists and is typed `%Any` rather than reporting a false "cannot find member" —
    // same permissive treatment `generated_member`'s own `GetObject` case already documents.
    fn unverified_any(&self) -> Option<MemberTy> {
        self.unverified.get().then(MemberTy::default)
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
            if let Some(uri) = self.workspace.resolve_cls_uri(context)
                && let Ok(details) = self.workspace.cls_details(&uri)
            {
                candidates.extend(details.imports.iter().map(|p| format!("{p}.{name}")));
            }
        }
        candidates.push(format!("User.{name}"));
        candidates
            .into_iter()
            .find(|candidate| self.class_exists(candidate))
            .unwrap_or(name)
    }

    pub(crate) fn class_exists(&self, cls: &str) -> bool {
        self.workspace.resolve_cls_uri(cls).is_some()
            || self
                .workspace
                .iris
                .as_ref()
                .is_some_and(|iris| iris.get_cls(cls).is_some())
    }

    fn qualified_extends(&self, cls: &str, details: &crate::ClassDetails) -> Vec<String> {
        details
            .extends
            .iter()
            .map(|sup| self.qualify_class_name(sup, Some(cls)))
            .collect()
    }

    // A member's declared type names are written relative to the declaring class.
    fn qualify_member_ty(&self, mut ty: MemberTy, context: &str) -> MemberTy {
        let qualify = |t: &mut Ty| {
            if let Ty::OREF(ClsOrIns::Ins(c) | ClsOrIns::Cls(c)) = t {
                c.name = self.qualify_class_name(&c.name, Some(context));
            }
        };
        match &mut ty.ty {
            HOTy::Value(t) | HOTy::Collection { element: t, .. } => qualify(t),
            HOTy::Procedure(proc) => {
                if matches!(proc.out, Some(Ty::OREF(_))) {
                    let mut proc = proc.as_ref().clone();
                    qualify(proc.out.as_mut().unwrap());
                    ty.ty = HOTy::Procedure(Arc::new(proc));
                }
            }
        }
        ty
    }

    fn unverified_reason(&self) -> &'static str {
        if self.workspace.iris.is_some() {
            "that isn't on the IRIS instance"
        } else {
            "outside this workspace, with no IRIS connection to check it against"
        }
    }

    fn iris_member(&self, cls: &str, mem: &str, order: &[MemberNs]) -> Option<MemberTy> {
        let iris = self.workspace.iris.as_ref()?;
        let members = iris.get_cls(cls)?.members;
        let found = order.iter().find_map(|&ns| {
            members.iter().find(|m| {
                MemberNs::of_info(&m.kind).contains(&ns)
                    && (m.name.content.eq_ignore_ascii_case(mem)
                        || m.aliases.iter().any(|a| a.eq_ignore_ascii_case(mem)))
            })
        })?;
        MemberTy::try_from(found.clone()).ok()
    }

    pub fn resolve_class(&self, name: &str) -> ClassTy {
        let mut cty = ClassTy::from(name.to_string());
        if let Some(uri) = self.workspace.resolve_cls_uri(&cty.name) {
            self.touched.borrow_mut().insert(uri.clone());
            if let Ok(details) = self.workspace.cls_details(&uri) {
                cty.location = Some(bindings::Location::new(uri, details.name.range));
            }
        } else if let Some(uri) = self
            .workspace
            .iris
            .as_ref()
            .and_then(|iris| iris.get_cls(&cty.name)?.uri)
        {
            let zero = bindings::Range::point(bindings::Position {
                line: 0,
                character: 0,
            });
            cty.location = Some(bindings::Location::new(uri, zero));
        }
        cty
    }

    pub fn can_assign_to(&self, source: &Ty, target: &Ty) -> bool {
        match (source, target) {
            (_, Ty::Any) => true,
            (Ty::Any, _) => true,
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

    // `None` means we can't tell (class outside the workspace with no IRIS connection).
    pub(crate) fn is_datatype(&self, cls: &str) -> Option<bool> {
        self.is_datatype_in(cls, &mut HashSet::new())
    }

    fn is_datatype_in(&self, cls: &str, seen: &mut HashSet<String>) -> Option<bool> {
        // A class reached twice (an inheritance cycle) adds nothing to the answer.
        if !seen.insert(cls.to_string()) {
            return Some(false);
        }
        if cls == "%Library.DataType" {
            return Some(true);
        }
        // The root of the object/reference hierarchy is never a datatype, even when it's
        // outside the workspace and there's no IRIS connection to confirm it.
        if cls == "%Library.RegisteredObject" {
            return Some(false);
        }
        match self.workspace.resolve_cls_uri(cls) {
            Some(uri) => {
                self.touched.borrow_mut().insert(uri.clone());
                let details = self.workspace.cls_details(&uri).ok()?;
                if details.is_datatype {
                    return Some(true);
                }
                let mut yes = false;
                for sup in self.qualified_extends(cls, &details) {
                    yes |= self.is_datatype_in(&sup, seen)?;
                }
                Some(yes)
            }
            // Outside the workspace, IRIS's compiled ClassType resolves the whole chain.
            None => self.workspace.iris.as_ref()?.get_cls(cls)?.is_datatype,
        }
    }

    // `None` means we can't tell (class outside the workspace with no IRIS connection). There are
    // only four real class-hierarchy roots (no `Extends` at all): `%Library.SystemBase` (shared
    // by every real object -- `%RegisteredObject`, `%SerialObject`, `%Persistent`, streams,
    // dynamic objects, all reach it via `%Library.Base`/`%SwizzleObject`/`%Library.AbstractSet`),
    // `%Library.DataType` (literal values), `%Library.FunctionalIndex` (SQL functional/bitmap
    // index classes), and, informally, everything else with no ancestor at all -- an SQL view
    // class (confirmed: no real `ClassType = view` class in a ~4900-class corpus has `Extends`),
    // or an isolated one-off like `%Library.Any`. `Some(true)` means `cls` is confirmed to land
    // outside `%Library.SystemBase` specifically -- i.e. one of the other three.
    fn confirmed_outside_system_base(&self, cls: &str) -> Option<bool> {
        self.confirmed_outside_system_base_in(cls, &mut HashSet::new())
    }

    fn confirmed_outside_system_base_in(
        &self,
        cls: &str,
        seen: &mut HashSet<String>,
    ) -> Option<bool> {
        if !seen.insert(cls.to_string()) {
            return Some(true);
        }
        if cls == "%Library.SystemBase" {
            return Some(false);
        }
        if cls == "%Library.DataType" || cls == "%Library.FunctionalIndex" {
            return Some(true);
        }
        let extends = match self.workspace.resolve_cls_uri(cls) {
            Some(uri) => {
                self.touched.borrow_mut().insert(uri.clone());
                self.workspace.cls_details(&uri).ok()?.extends.clone()
            }
            None => self.workspace.iris.as_ref()?.get_cls(cls)?.extends,
        };
        if extends.is_empty() {
            return Some(true);
        }
        let mut confirmed = true;
        for sup in &extends {
            let mut sup = sup.clone();
            normalize_class_name(&mut sup);
            confirmed &= self.confirmed_outside_system_base_in(&sup, seen)?;
        }
        Some(confirmed)
    }

    // `%`-prefixed system classes are assumed valid unconditionally, since they're essentially
    // never opened as workspace files.
    // Every `%`-prefixed name is trusted, a misspelled one (`%Sting`) included: only an IRIS
    // connection could confirm or deny it.
    pub(crate) fn check_declared_class_exists(&self, cls: &str, range: bindings::Range) {
        if cls.starts_with('%') {
            return;
        }
        if let Some(uri) = self.workspace.resolve_cls_uri(cls) {
            self.touched.borrow_mut().insert(uri);
            return;
        }
        if self
            .workspace
            .iris
            .as_ref()
            .is_some_and(|iris| iris.get_cls(cls).is_some())
        {
            return;
        }
        let message = if self.workspace.iris.is_some() {
            format!("Cannot find class '{cls}' in the workspace or on the IRIS instance")
        } else {
            format!(
                "Cannot confirm class '{cls}' exists — it isn't open in this workspace, with no IRIS connection to check it against"
            )
        };
        self.add_soft_diagnostic(bindings::Diagnostic {
            message,
            range,
            related_information: vec![],
            severity: bindings::DiagnosticSeverity::Hint,
        });
    }

    pub(crate) fn resolve_class_includes(
        &self,
        names: &[String],
    ) -> (MacroDefinitions, Vec<String>) {
        let (definitions, failed, resolved_uris) = self.workspace.resolve_class_includes(names);
        self.touched.borrow_mut().extend(resolved_uris);
        (definitions, failed)
    }

    pub(crate) fn expand_method_body(
        &self,
        uri: &str,
        header: &str,
        body: &str,
        base_definitions: &MacroDefinitions,
    ) -> Result<(String, SourceMap, MacroDefinitions), bindings::Diagnostic> {
        self.workspace
            .expand_method_body(uri, header, body, base_definitions)
    }

    // A workspace-wide routine's label -> signature map, for a cross-routine label call
    // (`Do Foo^OtherRoutine()`) -- same "resolve by name" shape as `resolve_cls_uri` for a
    // cross-class method call. Falls back to `IrisConnection::get_int_or_mac` when `name` isn't
    // open in the workspace; `None` only when neither finds it (or there's no IRIS connection).
    // `None` when there is no such routine; `Some(None)` when there is one but it can't be
    // parsed (a grammar gap), so its labels are simply unknown.
    pub(crate) fn resolve_rtn_labels(&self, name: &str) -> Option<Option<Labels>> {
        if let Some(uri) = self.workspace.resolve_rtn_uri(name) {
            self.touched.borrow_mut().insert(uri.clone());
            let rtn = self.workspace.rtn_details(&uri).ok();
            return Some(rtn.map(|rtn| labels_of(&rtn.map.expanded_olc, &rtn.routine, Some(&uri))));
        }
        let source = self.workspace.iris.as_ref()?.get_int_or_mac(name)?;
        let expanded = self
            .workspace
            .expand_rtn(source.uri.as_deref().unwrap_or(name), &source.text)
            .ok();
        Some(
            expanded.map(|(routine, map, _)| {
                labels_of(&map.expanded_olc, &routine, source.uri.as_deref())
            }),
        )
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
        if !self.can_assign_to(&source, target) {
            Err(Incompatible::new(source, target.clone()))
        } else {
            Ok(())
        }
    }

    fn is_subclass(&self, source: &ClassTy, target: &ClassTy) -> bool {
        self.merge_class(Cow::Borrowed(source), Cow::Borrowed(target)) == *target
    }

    pub fn merge_ty(&self, x: Ty, y: Ty) -> Ty {
        match (x, y) {
            (Ty::OREF(ClsOrIns::Ins(x)), Ty::OREF(ClsOrIns::Ins(y))) => Ty::OREF(ClsOrIns::Ins(
                self.merge_class(Cow::Owned(x), Cow::Owned(y)),
            )),
            _ => Ty::Any,
        }
    }

    fn merge_class(&self, x: Cow<'_, ClassTy>, y: Cow<'_, ClassTy>) -> ClassTy {
        if x == y {
            return x.into_owned();
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
        let mut order = Vec::new();
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([cls.to_string()]);
        while let Some(c) = queue.pop_front() {
            if !seen.insert(c.clone()) {
                continue;
            }
            let supers = if let Some(uri) = self.workspace.resolve_cls_uri(&c) {
                self.touched.borrow_mut().insert(uri.clone());
                self.workspace
                    .cls_details(&uri)
                    .map(|details| self.qualified_extends(&c, &details))
                    .unwrap_or_default()
            } else if let Some(iris) = self.workspace.iris.as_ref() {
                iris.get_cls(&c).map(|ci| ci.extends).unwrap_or_default()
            } else {
                vec![]
            };
            queue.extend(supers.into_iter().map(|mut s| {
                normalize_class_name(&mut s);
                s
            }));
            order.push(c);
        }
        order
    }

    pub fn new(workspace: &'a WorkspaceStruct) -> Self {
        Self {
            workspace,
            touched: RefCell::new(HashSet::new()),
            references: RefCell::new(Vec::new()),
            soft_diagnostics: RefCell::new(Vec::new()),
            unverified: Cell::new(false),
            suppress_soft_diagnostics: Cell::new(false),
        }
    }

    pub fn touched_uris(&self) -> HashSet<String> {
        self.touched.borrow().clone()
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
            (MaybeTy::Undefined, MaybeTy::Plain(ty)) => MaybeTy::Maybe(ty),
            (MaybeTy::Plain(ty), MaybeTy::Undefined) => MaybeTy::Maybe(ty),
            (MaybeTy::Undefined, ty) => ty,
            (ty, MaybeTy::Undefined) => ty,
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
}

impl Incompatible {
    pub(crate) fn new(source: Ty, target: Ty) -> Self {
        Self { source, target }
    }

    pub(crate) fn assign(self, name: String) -> QuasiTypeError {
        QuasiTypeError::assign_incompatible(name, self.source, self.target)
    }

    pub(crate) fn general(self) -> QuasiTypeError {
        QuasiTypeError::type_incompatible(self.source, self.target)
    }
}

pub struct WorkspaceEnv<'a> {
    workspace: &'a WorkspaceStruct,
    touched: RefCell<HashSet<String>>,
    references: RefCell<Vec<(bindings::Range, bindings::Location)>>,
    soft_diagnostics: RefCell<Vec<bindings::Diagnostic>>,
    // Set by `get_member_internal`'s final fallback (a class chain touches something outside
    // the workspace with no IRIS connection to confirm it, so a member's existence is simply
    // assumed) and read right back by whichever `resolve_*` call triggered it, to raise a soft
    // diagnostic at that exact reference — see `resolve_obj_mstr`.
    unverified: Cell<bool>,
    // Set during a loop's throwaway priming pass (see `Flow::with_loop`) — its findings reflect
    // pre-loop bindings only, not real, so every soft diagnostic raised meanwhile is dropped.
    suppress_soft_diagnostics: Cell<bool>,
}

pub struct ClassDocument<'a> {
    pub class: ClassTy,
    pub language: Option<&'a MethodLanguage>,
    pub mode: Option<&'a CodeMode>,
    pub macro_definitions: MacroDefinitions,
    // Set when this class has its own `Include`(s) that failed to resolve -- already reported
    // once at the class header (see `check_cls`), so a method body that fails to expand/parse as
    // a result shouldn't also raise its own, likely-confusing generic syntax error.
    pub has_unresolved_include: bool,
}

impl<'a> ClassDocument<'a> {
    pub fn new(
        class: ClassTy,
        language: Option<&'a MethodLanguage>,
        mode: Option<&'a CodeMode>,
        macro_definitions: MacroDefinitions,
        has_unresolved_include: bool,
    ) -> Self {
        Self {
            class,
            language,
            mode,
            macro_definitions,
            has_unresolved_include,
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
        }
    }

    pub fn olc(&self) -> &'a OLC {
        self.olc
    }

    pub fn for_routine<'b>(
        &'b self,
        labels: Labels,
        macro_definitions: &'b MacroDefinitions,
    ) -> CodeEnv<'b> {
        CodeEnv {
            base: self,
            is_instance: false,
            offset: 0,
            this: None,
            labels: Some(Arc::new(labels)),
            method_name: None,
            macro_definitions: Some(macro_definitions),
        }
    }

    pub fn for_class_member(
        &self,
        offset: usize,
        this: Option<&'a ClassTy>,
        method_name: &'a str,
        macro_definitions: &'a MacroDefinitions,
    ) -> CodeEnv<'_> {
        CodeEnv {
            base: self,
            is_instance: false,
            offset,
            this,
            labels: None,
            method_name: Some(method_name),
            macro_definitions: Some(macro_definitions),
        }
    }

    pub fn for_instance_member(
        &self,
        offset: usize,
        this: Option<&'a ClassTy>,
        method_name: &'a str,
        macro_definitions: &'a MacroDefinitions,
    ) -> CodeEnv<'_> {
        CodeEnv {
            base: self,
            is_instance: true,
            offset,
            this,
            method_name: Some(method_name),
            labels: None,
            macro_definitions: Some(macro_definitions),
        }
    }
}

/// label -> signature, for the subroutines/procedures declared in the current routine.
pub type Labels = HashMap<String, Arc<TyProcedure>>;

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
                Arc::new(details.to_ty_procedure(uri)),
            )
        })
        .collect()
}

pub struct CodeEnv<'a> {
    this: Option<&'a ClassTy>,
    base: &'a DocumentEnv<'a>,
    is_instance: bool,
    offset: usize,
    labels: Option<Arc<Labels>>,
    // The enclosing method's name, when known — `##super(...)` calls the superclass's version
    // of this same method, so resolving it needs to know which method that is.
    method_name: Option<&'a str>,
    // The enclosing class's own resolved `Include`s, for suggesting a similarly named macro on
    // an undefined-macro error -- `None` outside a class method (e.g. a routine).
    macro_definitions: Option<&'a MacroDefinitions>,
}

impl<'a> CodeEnv<'a> {
    pub(crate) fn class_exists(&self, cls: &str) -> bool {
        self.base.base.class_exists(cls)
    }

    pub(crate) fn qualify_class_name(&self, name: &str) -> String {
        self.base
            .base
            .qualify_class_name(name, self.this.map(|this| this.name.as_str()))
    }

    // The same scope, plus the labels of the method body being checked.
    pub(crate) fn with_labels(&self, labels: Labels) -> CodeEnv<'a> {
        CodeEnv {
            this: self.this,
            base: self.base,
            is_instance: self.is_instance,
            offset: self.offset,
            labels: Some(Arc::new(labels)),
            method_name: self.method_name,
            macro_definitions: self.macro_definitions,
        }
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

    pub(crate) fn resolve_ty_mref(
        &self,
        ty: &Ty,
        mem: &cos::MemberRef,
        range: bindings::Range,
        kind: AccessKind,
    ) -> Result<MemberTy, TypeError> {
        let is_parameter = mem.is_parameter();
        let mem = mem.id();
        match ty {
            Ty::Any => Ok(MemberTy {
                is_instance_only: false,
                ty: HOTy::Value(Ty::Any),
                ..MemberTy::default()
            }),
            Ty::OREF(obj) => self.resolve_obj_mstr(obj, mem, is_parameter, range, kind),
        }
    }

    pub(crate) fn resolve_obj_mstr(
        &self,
        obj: &ClsOrIns,
        mem: String,
        is_parameter: bool,
        range: bindings::Range,
        kind: AccessKind,
    ) -> Result<MemberTy, TypeError> {
        self.unverified.set(false);
        let result = self.get_member(obj.name(), &mem, is_parameter, kind, obj.is_ins());
        if self.unverified.replace(false) {
            self.add_soft_diagnostic(bindings::Diagnostic {
                message: format!(
                    "Cannot confirm '{mem}' exists on {} — its class hierarchy includes a class {}",
                    obj.name(),
                    self.unverified_reason()
                ),
                range,
                related_information: vec![],
                severity: bindings::DiagnosticSeverity::Hint,
            });
        }
        if let Some(mem_desc) = result {
            if mem_desc.is_instance_only && !obj.is_ins() {
                Err(
                    QuasiTypeError::access_instance_only_member_from_class_context(mem)
                        .into_error(|| range),
                )
            } else {
                Ok(mem_desc)
            }
        } else if self.workspace.iris.is_some() && !self.class_exists(obj.name()) {
            Err(QuasiTypeError::cannot_find_class(obj.name().to_string()).into_error(|| range))
        } else {
            let (suggestions, has_get) = self.member_suggestions(obj.name(), &mem);
            Err(
                QuasiTypeError::cannot_find_member(obj.to_string(), mem, suggestions, has_get)
                    .into_error(|| range),
            )
        }
    }

    // "Did you mean" candidates for a member lookup that failed, plus whether the class has its
    // own `%Get` (a common dynamic-property escape hatch on classes not modeled statically).
    fn member_suggestions(&self, cls: &str, mem: &str) -> (Vec<String>, bool) {
        let names = self.member_names(cls);
        let mut suggestions = crate::check::top_similar(mem, names.iter().map(String::as_str), 5);
        // A dynamic-property-style accessor (`%Get`, `%GetData`, ...) is easy to forget and call
        // like a plain property instead -- surface every one of these regardless of how close
        // `mem` looks to their names, since a getter's own name rarely resembles the property
        // it was written instead of.
        for name in &names {
            if name.starts_with("%Get") && !suggestions.iter().any(|s| s.eq_ignore_ascii_case(name))
            {
                suggestions.push(name.clone());
            }
        }
        let has_get = self
            .get_member_internal(cls, "%Get", &[MemberNs::HigherOrder])
            .is_some();
        (suggestions, has_get)
    }

    // `##super(...)`'s target: the enclosing method's own name, resolved against `this`'s
    // superclasses instead of `this` itself — same shape as `resolve_obj_mstr`.
    pub(crate) fn resolve_super_mstr(&self, range: bindings::Range) -> Result<MemberTy, TypeError> {
        let this = self.this().map_err(|err| err.into_error(|| range))?;
        let mem = self
            .method_name
            .ok_or_else(QuasiTypeError::not_in_class)
            .map_err(|err| err.into_error(|| range))?
            .to_string();
        self.unverified.set(false);
        let result = self.get_super_member(this.name(), &mem, this.is_ins());
        if self.unverified.replace(false) {
            self.add_soft_diagnostic(bindings::Diagnostic {
                message: format!(
                    "Cannot confirm '{mem}' exists on a superclass of {} — its class hierarchy includes a class {}",
                    this.name(),
                    self.unverified_reason()
                ),
                range,
                related_information: vec![],
                severity: bindings::DiagnosticSeverity::Hint,
            });
        }
        match result {
            Some(mem_desc) if mem_desc.is_instance_only && !this.is_ins() => Err(
                QuasiTypeError::access_instance_only_member_from_class_context(mem)
                    .into_error(|| range),
            ),
            Some(mem_desc) => Ok(mem_desc),
            None => {
                let (suggestions, has_get) = self.member_suggestions(this.name(), &mem);
                Err(
                    QuasiTypeError::cannot_find_member(this.to_string(), mem, suggestions, has_get)
                        .into_error(|| range),
                )
            }
        }
    }

    pub(crate) fn resolve_label(
        &self,
        label: Option<String>,
        offset: Option<Ty>,
        routine: Option<String>,
    ) -> Result<TyProcedureOrAny, QuasiTypeError> {
        match routine {
            // `label^routine`: resolved against that routine's own labels -- in the workspace,
            // or via IRIS (see `resolve_rtn_labels`); an unresolvable one errors either way.
            Some(routine) => match (label, offset) {
                (Some(label), None) => match self.resolve_rtn_labels(&routine) {
                    Some(Some(labels)) => match labels.get(&label) {
                        Some(proc) => Ok(TyProcedureOrAny::Procedure(proc.clone())),
                        None => Err(QuasiTypeError::cannot_find_label(label, Some(routine))),
                    },
                    Some(None) => Ok(TyProcedureOrAny::Any),
                    None => Err(QuasiTypeError::cannot_find_routine(routine)),
                },
                (_, _) => match self.resolve_rtn_labels(&routine) {
                    Some(_) => Ok(TyProcedureOrAny::Any),
                    None => Err(QuasiTypeError::cannot_find_routine(routine)),
                },
            },
            // No explicit routine: the current one.
            None => match (label, offset) {
                (Some(label), None) => {
                    match self.labels.as_ref().and_then(|labels| labels.get(&label)) {
                        Some(proc) => Ok(TyProcedureOrAny::Procedure(proc.clone())),
                        None => Err(QuasiTypeError::cannot_find_label(label, None)),
                    }
                }
                (Some(_), Some(_)) => Ok(TyProcedureOrAny::Any),
                (None, _) => Ok(TyProcedureOrAny::Any),
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

pub trait Locator {
    fn uri(&self) -> &str;
    fn offset_to_line_character(&self, offset: usize) -> (usize, usize);
    fn line_character_to_offset(&self, line_character: (usize, usize)) -> usize;

    fn position(&self, offset: usize) -> bindings::Position {
        let (line, character) = self.offset_to_line_character(offset);
        bindings::Position {
            line: line as u32,
            character: character as u32,
        }
    }

    // The inverse of `position`.
    fn offset(&self, position: bindings::Position) -> usize {
        self.line_character_to_offset((position.line as usize, position.character as usize))
    }

    fn range<T>(&self, ranged: &Range<T>) -> bindings::Range {
        bindings::Range {
            start: self.position(ranged.start),
            end: self.position(ranged.end),
        }
    }

    fn with_range<T>(&self, ranged: Range<T>) -> WithRange<T> {
        WithRange {
            range: self.range(&ranged),
            it: ranged.into_inner(),
        }
    }
}

impl<'a> Locator for DocumentEnv<'a> {
    fn uri(&self) -> &str {
        self.uri
    }

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

    fn offset_to_line_character(&self, offset: usize) -> (usize, usize) {
        self.base.offset_to_line_character(offset + self.offset)
    }

    fn line_character_to_offset(&self, line_character: (usize, usize)) -> usize {
        self.base.line_character_to_offset(line_character) - self.offset
    }
}
