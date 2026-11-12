//! The crate's shared data model, generated from `wit/ascot.wit` by `wit-bindgen`.
//!
//! Every `Position`, `Range`, `Diagnostic`, `InlayHint`, `ClassInfo`, `MemberInfo`, ... used by
//! `lib.rs`, `check/`, `lsp.rs` and `mcp.rs` is defined once in the WIT file and re-exported here,
//! so the native front ends and the wasm component (`bindings/js`, which the InterSystems
//! Language Server embeds) speak the same types. `GuestWorkspace` is the trait `Workspace`
//! implements to satisfy the WIT `workspace` resource; `IrisContext` is the host-provided
//! import that `atelier.rs` stands in for on native builds. The rest of this file converts
//! `details.rs`/`check/types.rs` values into those WIT records.

pub(crate) struct Component;

mod wit_bind {

    wit_bindgen::generate!({
        path: "src/wit/ascot.wit",
        additional_derives: [serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq, Hash],
        additional_derives_ignore: ["env"],
        // The style record's JSON spelling is the JS binding's, camelCase keys and kebab-case values,
        // so one style file serves every binding; serde would spell the Rust names otherwise.
        additional_type_attributes: {
            "iris:ascot/common/style": [#[serde(rename_all = "camelCase")]],
            "iris:ascot/common/command-style": [#[serde(rename_all = "camelCase")]],
            "iris:ascot/common/function-style": [#[serde(rename_all = "camelCase")]],
            "iris:ascot/common/brace-breaks": [#[serde(rename_all = "camelCase")]],
            "iris:ascot/common/require-description": [#[serde(rename_all = "camelCase")]],
            "iris:ascot/common/keyword-form": [#[serde(rename_all = "kebab-case")]],
            "iris:ascot/common/keyword-case": [#[serde(rename_all = "kebab-case")]],
            "iris:ascot/common/and-operator": [#[serde(rename_all = "kebab-case")]],
        },
    });

    pub use exports::iris::ascot::exported::{Guest, GuestWorkspace, Workspace};
    pub use iris::ascot::common::*;
    pub use iris::ascot::imported::{Atelier, Env, IrisContext};

    #[cfg(target_arch = "wasm32")]
    use super::Component;
    #[cfg(target_arch = "wasm32")]
    export!(Component);
}

impl std::fmt::Display for bindings::AndOperator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            bindings::AndOperator::Comma => ",",
            bindings::AndOperator::DoubleAmpersand => "&&",
        })
    }
}

impl bindings::Location {
    pub fn new(uri: String, range: bindings::Range) -> Self {
        Self { uri, range }
    }
}

use std::ops::Deref;

pub use wit_bind::*;

use crate::{
    bindings, check, cos,
    details::{self, MethodDetails, WithRange},
};

impl From<&details::IndexDetails> for wit_bind::IndexInfo {
    fn from(index: &details::IndexDetails) -> Self {
        Self {
            on: index.on.as_ref().map(|keys| {
                keys.iter()
                    .map(|key| wit_bind::IndexKey {
                        property: key.property.it.clone(),
                        subscript: key.subscript,
                        collation: key.collation.clone(),
                    })
                    .collect()
            }),
            abstract_: index.abstract_,
            condition: index.condition.clone(),
            coshard_with: index.coshard_with.clone(),
            data: index.data.clone(),
            deferred: index.deferred,
            extent: index.extent,
            id_key: index.id_key,
            internal: index.internal,
            primary_key: index.primary_key,
            shard_key: index.shard_key,
            sql_name: index.sql_name.clone(),
            type_: index.type_,
            unique: index.unique,
        }
    }
}

impl std::fmt::Display for wit_bind::Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.uri, self.range)
    }
}

impl std::fmt::Display for wit_bind::Range {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.end.line == self.start.line {
            write!(f, "{}-{}", self.start, self.end.character + 1)
        } else {
            write!(f, "{}", self.start)
        }
    }
}

impl std::fmt::Display for wit_bind::Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line + 1, self.character + 1)
    }
}

impl std::fmt::Display for wit_bind::DiagnosticRelatedInformation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

impl wit_bind::Diagnostic {
    pub fn describe(&self) -> String {
        let mut s = format!(":{} - {} [{}]\n", self.range, self.message, self.code);
        for related in &self.related_information {
            s += &format!("- {}\n", related);
        }
        s
    }
}

impl Range {
    pub fn point(position: Position) -> Self {
        Self {
            start: position,
            end: position,
        }
    }
}

impl From<&check::types::NormalArg> for wit_bind::NormalArg {
    fn from(arg: &check::types::NormalArg) -> Self {
        Self {
            mode: arg.mode,
            name: arg.name.clone(),
            t: arg.ty.as_ref().map(|ty| ty.to_string()),
            default: arg.default.clone(),
        }
    }
}

impl From<&check::types::VariadicArg> for wit_bind::VariadicArg {
    fn from(arg: &check::types::VariadicArg) -> Self {
        Self {
            mode: arg.mode,
            name: arg.name.clone(),
            t: arg.ty.as_ref().map(|ty| ty.to_string()),
        }
    }
}

impl From<&MethodDetails> for wit_bind::Signature {
    fn from(details: &MethodDetails) -> Self {
        bindings::Signature {
            normal: details
                .normal_args
                .iter()
                .map(Deref::deref)
                .map(bindings::NormalArg::from)
                .collect(),
            variadic: details
                .variadic_arg
                .as_deref()
                .map(bindings::VariadicArg::from),
            t: details.out.it.as_ref().map(|t| t.to_string()),
            procedure_block: Some(
                details
                    .procedure_block
                    .as_ref()
                    .map_or(vec![], |x| x.iter().map(|x| x.to_string()).collect()),
            ),
        }
    }
}

impl From<&details::MemberKindSpecificDetails> for wit_bind::MemberKind {
    fn from(kind: &details::MemberKindSpecificDetails) -> Self {
        use crate::details::MemberKindSpecificDetails;
        match kind {
            MemberKindSpecificDetails::MethodOrClassMethod(m) => {
                if m.is_class_method {
                    MemberKind::ClassMethod(wit_bind::Signature::from(m.as_ref()))
                } else {
                    MemberKind::Method(wit_bind::Signature::from(m.as_ref()))
                }
            }
            MemberKindSpecificDetails::ClientMethod => {
                MemberKind::ClientMethod(wit_bind::Signature {
                    normal: vec![],
                    variadic: None,
                    t: None,
                    procedure_block: None,
                })
            }
            MemberKindSpecificDetails::Property {
                ty,
                element_ty,
                multidimensional,
                ..
            } => {
                let multidimensional = *multidimensional;
                // A `many`/`children` relationship was read as the collection of the related
                // objects (see `details`); reported as the relationship it is.
                if let Some(check::types::Ty::OREF(check::types::ClsOrIns::Ins(cls))) = ty
                    && cls.name == "%Library.RelationshipObject"
                    && let Some(element) = cls.generics.get("ELEMENTTYPE")
                {
                    return MemberKind::Relationship(wit_bind::RelationshipInfo {
                        t: Some(element.clone()),
                        cardinality: Some(wit_bind::Cardinality::Many),
                    });
                }
                MemberKind::Property(match (ty, element_ty) {
                    (Some(ty), Some(element)) => wit_bind::PropertyInfo {
                        t: Some(element.to_string()),
                        collection: Some(if ty.to_string().ends_with("List") {
                            wit_bind::CollectionKind::List
                        } else {
                            wit_bind::CollectionKind::Array
                        }),
                        multidimensional,
                    },
                    (ty, _) => wit_bind::PropertyInfo {
                        t: ty.as_ref().map(|t| t.to_string()),
                        collection: None,
                        multidimensional,
                    },
                })
            }
            // The meta-type as written (`INTEGER`), which is what IRIS reports too; `ty` is
            // ascot's reading of it.
            MemberKindSpecificDetails::Parameter {
                as_type, default, ..
            } => MemberKind::Parameter(wit_bind::ParameterInfo {
                t: as_type.clone(),
                v: default.clone(),
            }),
            MemberKindSpecificDetails::ForeignKey => MemberKind::ForeignKey,
            MemberKindSpecificDetails::Index(index) => MemberKind::Index(index.into()),
            MemberKindSpecificDetails::Projection => MemberKind::Projection,
            MemberKindSpecificDetails::Trigger => MemberKind::Trigger,
            MemberKindSpecificDetails::XData => MemberKind::XData,
            MemberKindSpecificDetails::Storage => MemberKind::Storage,
            MemberKindSpecificDetails::Query {
                normal_args,
                variadic_arg,
                ty,
            } => MemberKind::Query(wit_bind::Signature {
                normal: normal_args
                    .iter()
                    .map(Deref::deref)
                    .map(bindings::NormalArg::from)
                    .collect(),
                variadic: variadic_arg.as_deref().map(bindings::VariadicArg::from),
                t: ty.as_ref().map(|t| t.to_string()),
                procedure_block: None,
            }),
        }
    }
}

// The keyword the member is declared with.
pub(crate) fn member_keyword(kind: &wit_bind::MemberKind) -> &'static str {
    match kind {
        MemberKind::Method(_) => "Method",
        MemberKind::ClassMethod(_) => "ClassMethod",
        MemberKind::ClientMethod(_) => "ClientMethod",
        MemberKind::Property(_) => "Property",
        MemberKind::Relationship(_) => "Relationship",
        MemberKind::Parameter(_) => "Parameter",
        MemberKind::ForeignKey => "ForeignKey",
        MemberKind::Index(_) => "Index",
        MemberKind::Projection => "Projection",
        MemberKind::Trigger => "Trigger",
        MemberKind::XData => "XData",
        MemberKind::Storage => "Storage",
        MemberKind::Query(_) => "Query",
    }
}

pub(crate) fn member_kind_to_symbol_kind(kind: &wit_bind::MemberKind) -> wit_bind::SymbolKind {
    match kind {
        MemberKind::Method(_) | MemberKind::ClassMethod(_) | MemberKind::ClientMethod(_) => {
            SymbolKind::Method
        }
        MemberKind::Property(_) | MemberKind::Relationship(_) => SymbolKind::Property,
        MemberKind::Parameter(_) => SymbolKind::Constant,
        MemberKind::ForeignKey => SymbolKind::Key,
        MemberKind::Index(_) => SymbolKind::Array,
        MemberKind::Projection => SymbolKind::Interface,
        MemberKind::Trigger => SymbolKind::Event,
        MemberKind::XData => SymbolKind::Struct,
        MemberKind::Storage => SymbolKind::Object,
        MemberKind::Query(_) => SymbolKind::Function,
    }
}

impl From<&WithRange<details::MemberDetails>> for wit_bind::MemberInfo {
    fn from(member: &WithRange<details::MemberDetails>) -> Self {
        MemberInfo {
            doc: member.it.doc.clone(),
            name: member.it.name.it.clone(),
            aliases: vec![],
            deprecated: member.it.deprecated,
            kind: wit_bind::MemberKind::from(&member.it.kind),
            uri: None,
            origin: None,
        }
    }
}

impl From<&details::ClassDetails> for wit_bind::ClassInfo {
    fn from(details: &details::ClassDetails) -> Self {
        let k = &details.keywords;
        Self {
            doc: details.doc.clone(),
            name: details.name.it.clone(),
            extends: details.extends.iter().map(|sup| sup.it.clone()).collect(),
            storage: None,
            members: details
                .members
                .in_order()
                .into_iter()
                .map(wit_bind::MemberInfo::from)
                .collect(),
            include: details.include_names.iter().map(|i| i.it.clone()).collect(),
            import: Some(details.imports.clone()).filter(|imports| !imports.is_empty()),
            // `exported.workspace.query-cls` already returns this class's real uri as its own
            // tuple element -- redundant to repeat it here.
            uri: None,
            class_type: k.class_type,
            deprecated: k.deprecated,
            abstract_: k.abstract_,
            inheritance: k.inheritance,
            language: k.language,
            procedure_block: k.procedure_block,
            property_class: k.property_class.clone(),
        }
    }
}

// Generated records take only the derives `additional_derives` gives them all, and an enum can't derive Default.
#[allow(clippy::derivable_impls)]
impl Default for wit_bind::ClassInfo {
    fn default() -> Self {
        Self {
            doc: String::new(),
            name: String::new(),
            extends: vec![],
            storage: None,
            members: vec![],
            include: vec![],
            import: None,
            uri: None,
            class_type: None,
            deprecated: false,
            abstract_: false,
            inheritance: None,
            language: None,
            procedure_block: None,
            property_class: vec![],
        }
    }
}

// The grammar's spellings to the host's enums, variant for variant.
macro_rules! same_variants {
    ($($t:ident { $($v:ident),* $(,)? })*) => {$(
        impl From<&cos::$t> for $t {
            fn from(value: &cos::$t) -> Self {
                match value { $(cos::$t::$v(_) => Self::$v,)* }
            }
        }
    )*};
}

same_variants! {
    ClassType { Datatype, Dynamic, Index, Literal, Persistent, Serial, Stream, View }
    Inheritance { Left, Right }
}

impl From<&cos::MethodLanguage> for Language {
    fn from(language: &cos::MethodLanguage) -> Self {
        match language {
            cos::MethodLanguage::ObjectScript(_) | cos::MethodLanguage::Cache(_) => {
                Self::Objectscript
            }
            cos::MethodLanguage::Python(_) => Self::Python,
            cos::MethodLanguage::TSQL(_) => Self::Tsql,
            cos::MethodLanguage::ISPL(_) => Self::Ispl,
            cos::MethodLanguage::JavaScript(_) => Self::Javascript,
            cos::MethodLanguage::MVBasic(_) => Self::Mvbasic,
        }
    }
}
