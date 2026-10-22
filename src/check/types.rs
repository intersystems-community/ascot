//! The checker's type vocabulary: `Ty` (a class, a literal kind, a collection, ...), `MaybeTy`
//! (possibly undefined) and `PowerTy` (a set of possibilities, the result of merging branches),
//! plus method signatures (`TyProcedure`, `NormalArg`, `VariadicArg`) and the property/parameter
//! keyword tables.
//!
//! Built from `details.rs` facts by `env.rs`, consumed by every rule in `mod.rs`/`builtins.rs`,
//! and rendered for inlay hints and hover text. Mostly plain data; the few class lookups needed
//! to normalize a type go through `WorkspaceEnv`.

use std::collections::BTreeMap;
use std::ops::Deref;
use std::sync::Arc;

use tygr::Grammar;

use crate::bindings::{self, ArgMode};
use crate::check::{WorkspaceEnv, errors::*};
use crate::cos::{CallType, CollectionKind, Formal, PropertyType, Type};
use crate::details::WithRange;

#[derive(Clone, Debug, PartialEq, Eq, Copy)]
pub enum TermOp {
    Eq,
    Ne,
    Le,
    Lt,
    Ge,
    Gt,
    Ng,
    Nl,
    Add,
    Sub,
    Pow,
    Mul,
    IntDiv,
    Mod,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Const {
    Integer(u32),
    String(String),
}
impl Const {
    pub fn as_integer(&self) -> u32 {
        match self {
            Const::Integer(u32) => *u32,
            Const::String(s) => str::parse(s).unwrap_or(0),
        }
    }

    pub fn as_string(&self) -> String {
        match self {
            Const::Integer(u32) => format!("{}", u32),
            Const::String(s) => s.clone(),
        }
    }
}

impl From<bool> for Const {
    fn from(value: bool) -> Self {
        Const::Integer(if value { 1 } else { 0 })
    }
}

impl From<u32> for Const {
    fn from(value: u32) -> Self {
        Const::Integer(value)
    }
}

impl std::fmt::Display for Const {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Const::Integer(i) => write!(f, "{}", i),
            Const::String(s) => write!(f, "\"{}\"", s),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Term {
    Target,
    Const(Const),
    Neg(Box<Term>),
    Infix {
        l: Box<Term>,
        o: TermOp,
        r: Box<Term>,
    },
}

impl Term {
    pub fn infix(l: Term, o: TermOp, r: Term) -> Self {
        Self::Infix {
            l: Box::new(l),
            o,
            r: Box::new(r),
        }
    }

    pub fn eq(c: Const) -> Self {
        Self::Infix {
            l: Box::new(Self::Target),
            o: TermOp::Eq,
            r: Box::new(Self::Const(c)),
        }
    }

    pub fn negate(self) -> Self {
        match self {
            Term::Neg(term) => *term,
            Term::Infix {
                l,
                o: TermOp::Eq,
                r,
            } => Term::Infix {
                l,
                o: TermOp::Ne,
                r,
            },
            Term::Infix {
                l,
                o: TermOp::Ne,
                r,
            } => Term::Infix {
                l,
                o: TermOp::Eq,
                r,
            },
            _ => Self::Neg(Box::new(self)),
        }
    }

    // An evaluation that overflows or divides by zero is no constraint at all, so it keeps `val`.
    fn filter(&self, val: &Const) -> bool {
        self.eval(val).is_none_or(|c| c.as_integer() != 0)
    }

    fn exact(&self) -> bool {
        match self {
            Term::Infix {
                l,
                o: TermOp::Eq,
                r,
            } => matches!(
                (l.deref(), r.deref()),
                (Term::Target, Term::Const(_)) | (Term::Const(_), Term::Target)
            ),
            _ => false,
        }
    }

    // `None` when the arithmetic overflows `u32` or divides by zero: no constant to fold to.
    pub(crate) fn eval(&self, val: &Const) -> Option<Const> {
        Some(match self {
            Term::Target => val.clone(),
            Term::Const(c) => c.clone(),
            Term::Neg(term) => (term.eval(val)?.as_integer() == 0).into(),
            Term::Infix { l, o, r } => {
                let l = l.eval(val)?;
                let r = r.eval(val)?;
                match o {
                    TermOp::Eq => (l.as_string() == r.as_string()).into(),
                    TermOp::Ne => (l.as_string() != r.as_string()).into(),
                    TermOp::Le | TermOp::Ng => (l.as_integer() <= r.as_integer()).into(),
                    TermOp::Lt => (l.as_integer() < r.as_integer()).into(),
                    TermOp::Ge | TermOp::Nl => (l.as_integer() >= r.as_integer()).into(),
                    TermOp::Gt => (l.as_integer() > r.as_integer()).into(),
                    TermOp::Add => l.as_integer().checked_add(r.as_integer())?.into(),
                    TermOp::Sub => l.as_integer().checked_sub(r.as_integer())?.into(),
                    TermOp::Pow => l.as_integer().checked_pow(r.as_integer())?.into(),
                    TermOp::Mul => l.as_integer().checked_mul(r.as_integer())?.into(),
                    TermOp::IntDiv => l.as_integer().checked_div(r.as_integer())?.into(),
                    TermOp::Mod => l.as_integer().checked_rem(r.as_integer())?.into(),
                }
            }
        })
    }
}

impl std::fmt::Display for TermOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TermOp::Mod => write!(f, "#"),
            TermOp::Eq => write!(f, "="),
            TermOp::Ne => write!(f, "'="),
            TermOp::Le => write!(f, "<="),
            TermOp::Lt => write!(f, "<"),
            TermOp::Ge => write!(f, ">="),
            TermOp::Gt => write!(f, ">"),
            TermOp::Ng => write!(f, "'>"),
            TermOp::Nl => write!(f, "'<"),
            TermOp::Add => write!(f, "+"),
            TermOp::Sub => write!(f, "-"),
            TermOp::Pow => write!(f, "**"),
            TermOp::Mul => write!(f, "*"),
            TermOp::IntDiv => write!(f, "\\"),
        }
    }
}

impl std::fmt::Display for Term {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Term::Target => write!(f, "%?"),
            Term::Const(v) => write!(f, "\"{}\"", v),
            Term::Neg(inner) => write!(f, "'({})", inner),
            Term::Infix { l, o, r } => write!(f, "{} {} {}", l, o, r),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaybeTy {
    Undefined,
    Maybe(Ty),
    Plain(Ty),
}

impl MaybeTy {
    // Consumes `self` on purpose: converting to `Ty` moves the inner value out instead of cloning it.
    #[allow(clippy::wrong_self_convention)]
    fn to_ty(self) -> Result<Ty, QuasiTypeError> {
        match self {
            MaybeTy::Undefined => Err(QuasiTypeError::undefined()),
            MaybeTy::Maybe(ty) => Err(QuasiTypeError::maybe_undefined(ty)),
            MaybeTy::Plain(ty) => Ok(ty),
        }
    }

    pub(crate) fn maybe_undefined(&self) -> bool {
        match self {
            MaybeTy::Undefined => true,
            MaybeTy::Maybe(..) => true,
            MaybeTy::Plain(..) => false,
        }
    }

    pub(crate) fn maybe_defined(&self) -> Option<&Ty> {
        match self {
            MaybeTy::Undefined => None,
            MaybeTy::Maybe(ty) => Some(ty),
            MaybeTy::Plain(ty) => Some(ty),
        }
    }
}

impl From<Option<Ty>> for MaybeTy {
    fn from(value: Option<Ty>) -> Self {
        match value {
            Some(ty) => MaybeTy::Plain(ty),
            None => MaybeTy::Undefined,
        }
    }
}

impl From<MaybeTy> for PowerTy {
    fn from(value: MaybeTy) -> Self {
        Self::Plain(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PowerTy {
    Plain(MaybeTy),
    Const(Const),
    Union(Const, Const, Vec<Const>),
    Never,
    Term {
        target: u32,
        term: Term,
    },
    Case {
        target: u32,
        cases: Vec<(&'static str, Option<Ty>)>,
        // The outer Option determines if there is a default branch.
        //   There can be no default branches when all possible values are listed by cases.
        // The inner Option determines if the default is defined, same as in AllBindings.assign
        default: Option<Option<Ty>>,
    },
}

impl PowerTy {
    pub(crate) fn undefined() -> Self {
        MaybeTy::Undefined.into()
    }

    pub(crate) fn plain(ty: Ty) -> Self {
        MaybeTy::Plain(ty).into()
    }

    pub(crate) fn union(mut rst: Vec<Const>) -> Self {
        rst.sort();
        rst.dedup();
        rst.reverse();
        if let Some(fst) = rst.pop() {
            if let Some(snd) = rst.pop() {
                rst.reverse();
                Self::Union(fst, snd, rst)
            } else {
                Self::Const(fst)
            }
        } else {
            Self::Never
        }
    }

    pub fn bind(target: u32) -> Self {
        Self::Term {
            target,
            term: Term::Target,
        }
    }

    pub fn case(
        target: u32,
        mut cases: Vec<(&'static str, Option<Ty>)>,
        default: Option<Option<Ty>>,
    ) -> Self {
        match (cases.len(), default) {
            (0, Some(default)) => default.map_or(Self::undefined(), Self::plain),
            (0, None) => {
                // Unreachable (bottom/never/absurd): all cases ruled out, no default possible.
                Ty::Any.into()
            }
            (1, None) => cases
                .pop()
                .unwrap()
                .1
                .map_or(Self::undefined(), Self::plain),
            (_, default) => Self::Case {
                target,
                cases,
                default,
            },
        }
    }

    pub fn to_maybe_ty(&self, env: &WorkspaceEnv) -> MaybeTy {
        match self {
            PowerTy::Plain(ty) => ty.clone(),
            PowerTy::Term { .. } => MaybeTy::Plain(Ty::from("%DataType")),
            PowerTy::Case { cases, default, .. } => {
                let mut default = default.clone().map(MaybeTy::from);
                for (_, ty) in cases {
                    let ty = MaybeTy::from(ty.clone());
                    if let Some(lft) = default {
                        default = Some(env.merge_maybe_ty(lft, ty))
                    } else {
                        default = Some(ty)
                    }
                }
                default.unwrap_or(MaybeTy::Plain(Ty::Any))
            }
            PowerTy::Const(Const::Integer(_)) => MaybeTy::Plain(Ty::from("%Integer")),
            PowerTy::Const(Const::String(..)) => MaybeTy::Plain(Ty::from("%String")),
            PowerTy::Union(..) => MaybeTy::Plain(Ty::from("%DataType")),
            PowerTy::Never => MaybeTy::Plain(Ty::Any),
        }
    }

    pub fn to_ty(&self, env: &WorkspaceEnv) -> Result<Ty, QuasiTypeError> {
        self.to_maybe_ty(env).to_ty()
    }

    pub fn refine_on_prop(self, target: u32, term: &Term) -> Self {
        match self {
            PowerTy::Case {
                target: case_target,
                mut cases,
                mut default,
            } if case_target == target => {
                refine_case_cases(term, &mut cases, &mut default);
                PowerTy::case(target, cases, default)
            }
            _ => self,
        }
    }
}

fn refine_case_cases(
    term: &Term,
    cases: &mut Vec<(&'static str, Option<Ty>)>,
    default: &mut Option<Option<Ty>>,
) {
    cases.retain(|(val, _)| term.filter(&Const::String(val.to_string())));
    if term.exact() && !cases.is_empty() {
        *default = None
    }
}

impl std::fmt::Display for PowerTy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PowerTy::Plain(ty) => ty.fmt(f),
            PowerTy::Const(c) => write!(f, "%DataType = {}", c),
            PowerTy::Union(fst, snd, rst) => write!(
                f,
                "%DataType ∈ {{{fst}, {snd}{}}}",
                rst.iter()
                    .map(|c| format!(", {c}"))
                    .collect::<Vec<_>>()
                    .join("")
            ),
            PowerTy::Term { target, term } => {
                write!(f, "%DataType = {} where %? = %{})", term, target)
            }
            PowerTy::Case {
                target,
                cases,
                default,
            } => {
                write!(f, "$CASE(%{}, ", target)?;
                for (i, (name, ty)) in cases.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "\"{}\":", name)?;
                    if let Some(ty) = ty {
                        write!(f, "{}", ty)?;
                    } else {
                        write!(f, "<UNDEFINED>")?;
                    }
                }
                if let Some(ty) = default {
                    write!(f, ", :")?;
                    if let Some(ty) = ty {
                        write!(f, "{}", ty)?;
                    } else {
                        write!(f, "<UNDEFINED>")?;
                    }
                }
                write!(f, ")")
            }
            PowerTy::Never => write!(f, "<NEVER>"),
        }
    }
}

impl std::fmt::Display for MaybeTy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MaybeTy::Plain(ty) => ty.fmt(f),
            MaybeTy::Maybe(ty) => write!(f, "{}?", ty),
            MaybeTy::Undefined => write!(f, "<UNDEFINED>"),
        }
    }
}

impl<T> From<T> for PowerTy
where
    Ty: From<T>,
{
    fn from(value: T) -> Self {
        Self::plain(Ty::from(value))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Ty {
    #[default]
    Any,
    OREF(ClsOrIns),
    // `This` and `..#NAME` in a mask-corrected IRIS signature, until `Instantiate` resolves them
    // against the receiver.
    This,
    Param(String),
}

// A member's resolved type, before it's known whether it's about to be called. ObjectScript has
// no first-class functions, so a `Procedure` is only ever valid as the callee of a call — using
// it any other way (assigning it, member-accessing it) must error at the point it's coerced to a
// plain `Ty` via `to_ty`, not later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HOTy {
    Value(Ty),
    Procedure(Arc<TyProcedure>),
    // A `list Of X` / `array Of X` property. Collections don't nest, so this lives here rather
    // than in `Ty`; forced into a `Ty` it is the collection class.
    Collection { kind: CollectionClass, element: Ty },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionClass {
    List,
    Array,
}

impl CollectionClass {
    pub fn class(self) -> Ty {
        Ty::from(match self {
            CollectionClass::List => "%Collection.AbstractList",
            CollectionClass::Array => "%Collection.AbstractArray",
        })
    }

    pub fn key_ty(self) -> Ty {
        Ty::from(match self {
            CollectionClass::List => "%Integer",
            CollectionClass::Array => "%String",
        })
    }
}

impl HOTy {
    pub fn to_ty(&self) -> Result<Ty, QuasiTypeError> {
        match self {
            HOTy::Value(ty) => Ok(ty.clone()),
            HOTy::Collection { kind, .. } => Ok(kind.class()),
            HOTy::Procedure(_) => Err(QuasiTypeError::procedure_not_called()),
        }
    }

    pub fn as_procedure(&self) -> Result<TyProcedureOrAny, QuasiTypeError> {
        match self {
            HOTy::Procedure(v) => Ok(TyProcedureOrAny::Procedure(v.clone())),
            HOTy::Value(Ty::Any) => Ok(TyProcedureOrAny::Any),
            HOTy::Value(ty) => Err(QuasiTypeError::not_procedure(ty.clone())),
            HOTy::Collection { kind, .. } => Err(QuasiTypeError::not_procedure(kind.class())),
        }
    }
}

impl From<Ty> for HOTy {
    fn from(ty: Ty) -> Self {
        Self::Value(ty)
    }
}

impl std::fmt::Display for HOTy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HOTy::Value(ty) => ty.fmt(f),
            HOTy::Procedure(proc) => proc.fmt(f),
            HOTy::Collection { kind, element } => match kind {
                CollectionClass::List => write!(f, "list Of {element}"),
                CollectionClass::Array => write!(f, "array Of {element}"),
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct ClassTy {
    pub name: String,
    // Where `name` is declared in the workspace, if it's been resolved — not part of the
    // type's identity (a `##class(X)` literal and one merged from elsewhere both denote the
    // same class regardless of which happened to resolve this), so excluded from equality.
    pub location: Option<bindings::Location>,
    // Generic arguments by parameter name (see `crate::mask`), excluded from equality too.
    pub generics: BTreeMap<String, String>,
}

impl PartialEq for ClassTy {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for ClassTy {}

pub fn normalize_class_name(name: &mut String) {
    if !name.contains(".") && name.starts_with("%") {
        name.insert_str(1, "Library.");
    }
}

impl From<String> for ClassTy {
    fn from(mut name: String) -> Self {
        normalize_class_name(&mut name);
        ClassTy {
            name,
            location: None,
            generics: BTreeMap::new(),
        }
    }
}

impl std::fmt::Display for ClassTy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.name.strip_prefix("%Library.") {
            Some(this) => write!(f, "%{}", this)?,
            None => write!(f, "{}", self.name)?,
        }
        if !self.generics.is_empty() {
            let args: Vec<_> = self
                .generics
                .iter()
                .map(|(name, arg)| format!("{name} = \"{arg}\""))
                .collect();
            write!(f, "({})", args.join(", "))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClsOrIns {
    Ins(ClassTy),
    Cls(ClassTy),
}

impl ClsOrIns {
    pub fn name(&self) -> &str {
        match self {
            ClsOrIns::Ins(name) => &name.name,
            ClsOrIns::Cls(name) => &name.name,
        }
    }

    pub fn is_ins(&self) -> bool {
        matches!(self, Self::Ins(..))
    }

    pub fn replace(&self, cty: ClassTy) -> ClsOrIns {
        match self {
            ClsOrIns::Ins(_) => Self::Ins(cty),
            ClsOrIns::Cls(_) => Self::Cls(cty),
        }
    }
}

// Which of IRIS's dynamic-dispatch methods (if any) a failed member lookup should fall back to
// -- see https://docs.intersystems.com/irislatest/csp/docbook/DocBook.UI.Page.cls?KEY=GOOP_dynamic_dispatch
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AccessKind {
    Get,
    Set,
    Call,
}

impl std::fmt::Display for ClsOrIns {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClsOrIns::Ins(cty) => write!(f, "{}", cty),
            ClsOrIns::Cls(cty) => write!(f, "Class '{}'", cty),
        }
    }
}

impl Ty {
    pub fn ins(cty: ClassTy) -> Self {
        Self::OREF(ClsOrIns::Ins(cty))
    }
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::Any => write!(f, "Any"),
            Ty::OREF(cls_or_ins) => cls_or_ins.fmt(f),
            Ty::This => write!(f, "This"),
            Ty::Param(name) => write!(f, "..#{name}"),
        }
    }
}

impl From<String> for Ty {
    fn from(value: String) -> Self {
        Self::OREF(ClsOrIns::Ins(ClassTy::from(value)))
    }
}

// A class parameter's `As` clause uses compiler meta-types (STRING, INTEGER, ...), not datatype
// class names -- these are the only ones IRIS recognizes there.
pub const PARAMETER_TYPE_KEYWORDS: &[&str] = &[
    "BOOLEAN",
    "CLASSNAME",
    "COSCODE",
    "COSEXPRESSION",
    "COSIDENTIFIER",
    "INTEGER",
    "SQL",
    "SQLIDENTIFIER",
    "STRING",
    "TEXT",
    "CONFIGVALUE",
];

pub fn parameter_ty(meta_type: &str) -> Ty {
    match meta_type.to_uppercase().as_str() {
        "STRING" | "CLASSNAME" | "COSCODE" | "COSIDENTIFIER" | "SQL" | "SQLIDENTIFIER" | "TEXT" => {
            Ty::from("%Library.String")
        }
        "INTEGER" => Ty::from("%Library.Integer"),
        "BOOLEAN" => Ty::from("%Library.Boolean"),
        _ => Ty::Any,
    }
}

impl From<&str> for Ty {
    fn from(value: &str) -> Self {
        Self::from(value.to_string())
    }
}

// Recognized bracketed `[ ... ]` keywords, matched case-insensitively. Sourced from
// https://docs.intersystems.com/irislatest/csp/docbook/DocBook.UI.Page.cls?KEY=ROBJ_property and
// KEY=ROBJ_parameter.
pub const PROPERTY_KEYWORDS: &[&str] = &[
    "ALIASES",
    "CALCULATED",
    "CARDINALITY",
    "CLIENTNAME",
    "COLLECTION",
    "COMPUTELOCALONLY",
    "DEFERRED",
    "DEPRECATED",
    "FINAL",
    "IDENTITY",
    "INITIALEXPRESSION",
    "INTERNAL",
    "INVERSE",
    "MULTIDIMENSIONAL",
    "ONDELETE",
    "PRIVATE",
    "READONLY",
    "REQUIRED",
    "SERVERONLY",
    "SQLCOLUMNNUMBER",
    "SQLCOMPUTECODE",
    "SQLCOMPUTED",
    "SQLCOMPUTEONCHANGE",
    "SQLFIELDNAME",
    "SQLLISTDELIMITER",
    "SQLLISTTYPE",
    "TRANSIENT",
];

pub const PARAMETER_KEYWORDS: &[&str] = &[
    "ABSTRACT",
    "CONSTRAINT",
    "DEPRECATED",
    "FINAL",
    "FLAGS",
    "INTERNAL",
];

impl From<&Type> for Ty {
    fn from(value: &Type) -> Self {
        Self::from(value.class_name.print())
    }
}

impl From<&PropertyType> for Ty {
    fn from(value: &PropertyType) -> Self {
        match &value.collection {
            Some(collection) => match collection {
                CollectionKind::ListOf(..) => Self::from("%Collection.AbstractList"),
                CollectionKind::ArrayOf(..) => Self::from("%Collection.AbstractArray"),
            },
            None => match &value.class_name {
                Some(class_name) => Self::from(class_name.print()),
                None => Self::default(),
            },
        }
    }
}

impl std::fmt::Display for ArgMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArgMode::Default => write!(f, "a normal"),
            ArgMode::Output => write!(f, "an output"),
            ArgMode::ByRef => write!(f, "a by-ref"),
        }
    }
}

impl ArgMode {
    fn prefix(&self) -> &'static str {
        match self {
            ArgMode::Output => "Output ",
            ArgMode::ByRef => "ByRef ",
            ArgMode::Default => "",
        }
    }
}

impl From<&CallType> for ArgMode {
    fn from(ctype: &CallType) -> ArgMode {
        match ctype {
            CallType::Output(..) => ArgMode::Output,
            CallType::ByRef(..) => ArgMode::ByRef,
            CallType::ByVal(..) => ArgMode::Default,
        }
    }
}

// `ArgMode` is generated by `wit_bindgen::generate!`, so `#[derive(Default)]` can't be added to it directly.
#[allow(clippy::derivable_impls)]
impl Default for ArgMode {
    fn default() -> Self {
        ArgMode::Default
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalArg {
    pub mode: ArgMode,
    pub name: String,
    pub ty: Option<Ty>,
    pub default: Option<String>,
    // Where this parameter is declared, within `TyProcedure::uri`. For call-site errors.
    pub declared_at: Option<bindings::Range>,
    pub variadic: bool,
}

impl NormalArg {
    pub fn new(mode: ArgMode, name: String, ty: Option<Ty>, default: Option<String>) -> Self {
        Self {
            mode,
            name,
            ty,
            default,
            declared_at: None,
            variadic: false,
        }
    }

    pub fn with_declared_at(mut self, declared_at: bindings::Range) -> Self {
        self.declared_at = Some(declared_at);
        self
    }

    pub fn required(&self) -> bool {
        self.mode != ArgMode::Output && self.default.is_none()
    }
}

impl From<&Formal> for NormalArg {
    fn from(formal: &Formal) -> Self {
        Self::new(
            formal
                .call_type
                .as_deref()
                .map(ArgMode::from)
                .unwrap_or_default(),
            formal.name.print(),
            formal.as_type.as_deref().map(Ty::from),
            formal
                .default
                .as_ref()
                .map(|default| default.deref().print()),
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VariadicArg {
    pub mode: ArgMode,
    pub name: String,
    pub ty: Option<Ty>,
    pub declared_at: Option<bindings::Range>,
}

impl VariadicArg {
    pub fn new(mode: ArgMode, name: String, ty: Option<Ty>) -> Self {
        Self {
            mode,
            name,
            ty,
            declared_at: None,
        }
    }

    pub fn with_declared_at(mut self, declared_at: bindings::Range) -> Self {
        self.declared_at = Some(declared_at);
        self
    }
}

impl From<&Formal> for VariadicArg {
    fn from(formal: &Formal) -> Self {
        Self::new(
            formal
                .call_type
                .as_deref()
                .map(ArgMode::from)
                .unwrap_or_default(),
            formal.name.print(),
            formal.as_type.as_deref().map(Ty::from),
        )
    }
}

impl From<&VariadicArg> for NormalArg {
    fn from(arg: &VariadicArg) -> Self {
        // Variadic arguments default to the empty sequence.
        let normal = Self {
            variadic: true,
            ..Self::new(
                arg.mode,
                arg.name.clone(),
                arg.ty.clone(),
                Some("\"\"".to_string()),
            )
        };
        match arg.declared_at {
            Some(declared_at) => normal.with_declared_at(declared_at),
            None => normal,
        }
    }
}

pub type ProcedureBlock = Option<Vec<WithRange<String>>>;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct TyProcedure {
    pub normal_args: Vec<NormalArg>,
    pub variadic_arg: Option<VariadicArg>,
    pub out: Option<Ty>,
    pub procedure_block: Option<Vec<String>>,
    // Shared by every parameter's `declared_at`. Only populated for legacy routine labels so far.
    pub uri: Option<String>,
    // Where the procedure itself (not a parameter) is declared — e.g. a routine label's own
    // line. Only populated for legacy routine labels so far.
    pub declared_at: Option<bindings::Range>,
}

impl TyProcedure {
    pub fn location(&self) -> Option<bindings::Location> {
        Some(bindings::Location::new(
            self.uri.clone()?,
            self.declared_at?,
        ))
    }
}

impl std::fmt::Display for TyProcedure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "(")?;
        for (i, arg) in self.normal_args.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}{}", arg.mode.prefix(), arg.name)?;
            if let Some(ty) = arg.ty.as_ref() {
                write!(f, " As {}", ty)?;
            }
        }
        if let Some(variadic) = &self.variadic_arg {
            if !self.normal_args.is_empty() {
                write!(f, ", ")?;
            }
            write!(f, "{}{}", variadic.mode.prefix(), variadic.name)?;
            if let Some(ty) = variadic.ty.as_ref() {
                write!(f, " As {}", ty)?;
            }
            write!(f, "...")?;
        }
        write!(f, ")")?;
        if let Some(out) = &self.out {
            write!(f, " As {}", out)?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub enum TyProcedureOrAny {
    Procedure(Arc<TyProcedure>),
    #[default]
    Any,
}

impl TyProcedure {
    pub fn get_arg(&self, i: usize) -> Option<NormalArg> {
        if let Some(arg) = self.normal_args.get(i) {
            Some(arg.clone())
        } else {
            self.variadic_arg.as_ref().map(NormalArg::from)
        }
    }
}

// A host can report an empty string rather than omitting a declared type entirely -- `Ty::from`
// would otherwise take that literally, producing a nonsense class named "".
pub(crate) fn ty_from_declared(t: Option<String>) -> Option<Ty> {
    t.filter(|t| !t.is_empty()).map(Ty::from)
}

// A `many`/`children` relationship to `t`: a `%RelationshipObject` of `t`s, `t` bound to the
// generic parameter the mask gives it.
pub(crate) fn relationship_object(t: Option<String>) -> Ty {
    let mut cls = ClassTy::from("%Library.RelationshipObject".to_string());
    cls.generics = t
        .filter(|t| !t.is_empty())
        .zip(crate::mask::generic_params(&cls.name).next())
        .map(|(t, param)| (param.name.content.clone(), t))
        .into_iter()
        .collect();
    Ty::OREF(ClsOrIns::Ins(cls))
}

// A type in a mask-corrected member (see `crate::mask`), which may be `This` or a generic
// parameter `..#NAME`; resolved against the receiver by `Instantiate`.
pub(crate) fn ty_from_mask(t: Option<String>) -> Option<Ty> {
    use tygr_objectscript::cos::SignatureType;
    match t.as_deref().map(SignatureType::parse) {
        Some(Ok(SignatureType::This(_))) => Some(Ty::This),
        Some(Ok(SignatureType::Parameter(_, name))) => Some(Ty::Param(name.print())),
        _ => ty_from_declared(t),
    }
}

// A type as seen from `receiver`: `This` is the receiver's class, and `..#NAME` the receiver's
// generic argument for that parameter. Only the mask introduces either.
pub(crate) trait Instantiate {
    fn instantiate(&mut self, receiver: &ClassTy);
}

impl Instantiate for Ty {
    fn instantiate(&mut self, receiver: &ClassTy) {
        let resolved = match self {
            Ty::This => Some(receiver.name.clone()),
            Ty::Param(param) => crate::mask::generic_arg(&receiver.name, &receiver.generics, param),
            Ty::Any | Ty::OREF(_) => None,
        };
        if let Some(name) = resolved {
            *self = Ty::from(name);
        }
    }
}

impl<T: Instantiate> Instantiate for Option<T> {
    fn instantiate(&mut self, receiver: &ClassTy) {
        if let Some(t) = self {
            t.instantiate(receiver);
        }
    }
}

impl Instantiate for TyProcedure {
    fn instantiate(&mut self, receiver: &ClassTy) {
        for arg in &mut self.normal_args {
            arg.ty.instantiate(receiver);
        }
        if let Some(arg) = &mut self.variadic_arg {
            arg.ty.instantiate(receiver);
        }
        self.out.instantiate(receiver);
    }
}

impl Instantiate for HOTy {
    fn instantiate(&mut self, receiver: &ClassTy) {
        match self {
            HOTy::Value(ty) | HOTy::Collection { element: ty, .. } => ty.instantiate(receiver),
            HOTy::Procedure(proc) => Arc::make_mut(proc).instantiate(receiver),
        }
    }
}

impl From<bindings::MethodInfo> for TyProcedure {
    fn from(m: bindings::MethodInfo) -> Self {
        Self::from_method(m, ty_from_declared)
    }
}

impl TyProcedure {
    // `ty` reads each declared type: `ty_from_declared`, or `ty_from_mask` for a mask's member.
    pub(crate) fn from_method(
        bindings::MethodInfo {
            normal,
            variadic,
            t,
            procedure_block,
            body: _,
        }: bindings::MethodInfo,
        ty: fn(Option<String>) -> Option<Ty>,
    ) -> Self {
        Self {
            normal_args: normal
                .into_iter()
                .map(|arg: bindings::NormalArg| {
                    let normal = NormalArg::new(arg.mode, arg.name, ty(arg.t), arg.default);
                    match arg.declared_at {
                        Some(declared_at) => normal.with_declared_at(declared_at),
                        None => normal,
                    }
                })
                .collect(),
            variadic_arg: variadic.map(|v| {
                let variadic = VariadicArg::new(v.mode, v.name, ty(v.t));
                match v.declared_at {
                    Some(declared_at) => variadic.with_declared_at(declared_at),
                    None => variadic,
                }
            }),
            out: ty(t),
            procedure_block,
            uri: None,
            declared_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not only signatures: any member type, here a property's, is resolved against the receiver.
    #[test]
    fn a_value_type_is_instantiated_too() {
        let mut receiver = ClassTy::from("%Library.RelationshipObject".to_string());
        receiver
            .generics
            .insert("ELEMENTTYPE".to_string(), "Demo.Child".to_string());
        let mut element = HOTy::Value(ty_from_mask(Some("..#ELEMENTTYPE".to_string())).unwrap());
        element.instantiate(&receiver);
        assert!(
            matches!(element, HOTy::Value(Ty::OREF(ClsOrIns::Ins(c))) if c.name == "Demo.Child")
        );
        let mut this = HOTy::Value(ty_from_mask(Some("This".to_string())).unwrap());
        this.instantiate(&receiver);
        assert!(
            matches!(this, HOTy::Value(Ty::OREF(ClsOrIns::Ins(c))) if c.name == "%Library.RelationshipObject")
        );
    }
}
