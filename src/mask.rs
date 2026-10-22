//! Corrections to member types IRIS 2026.2's `%Library` declares as `%ObjectHandle`, and to
//! arguments it declares without a default that are optional in practice. Any other
//! `%ObjectHandle` is left alone, so a missing correction shows up as an error naming it.

use crate::check::types::{HOTy, ty_from_mask};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Correction {
    returns: Option<String>,
    #[serde(default)]
    args: BTreeMap<String, ArgCorrection>,
    parameter: Option<String>,
    default: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArgCorrection {
    #[serde(rename = "type")]
    ty: Option<String>,
    #[serde(default)]
    optional: bool,
}

static MASK: LazyLock<BTreeMap<String, BTreeMap<String, Correction>>> =
    LazyLock::new(|| serde_json::from_str(include_str!("mask.json")).expect("a valid mask.json"));

fn members(cls: &str) -> impl Iterator<Item = (&'static String, &'static Correction)> {
    MASK.get(cls).into_iter().flatten()
}

pub(crate) fn correction(cls: &str, member: &str) -> Option<&'static Correction> {
    members(cls)
        .find(|(name, c)| name.eq_ignore_ascii_case(member) && c.parameter.is_none())
        .map(|(_, c)| c)
}

impl Correction {
    pub(crate) fn apply(&self, ty: &mut HOTy) {
        let HOTy::Procedure(proc) = ty else {
            return;
        };
        let proc = Arc::make_mut(proc);
        if let Some(returns) = &self.returns {
            proc.out = ty_from_mask(Some(returns.clone()));
        }
        for arg in &mut proc.normal_args {
            let Some(correction) = self.args.get(&arg.name) else {
                continue;
            };
            if let Some(t) = &correction.ty {
                arg.ty = ty_from_mask(Some(t.clone()));
            }
            if correction.optional {
                arg.default.get_or_insert_default();
            }
        }
    }
}

/// The argument bound to `cls`'s generic parameter `param`, or else its default.
pub(crate) fn generic_arg(
    cls: &str,
    args: &BTreeMap<String, String>,
    param: &str,
) -> Option<String> {
    let (name, declared) =
        members(cls).find(|(name, c)| c.parameter.is_some() && name.eq_ignore_ascii_case(param))?;
    args.get(name).cloned().or_else(|| declared.default.clone())
}
