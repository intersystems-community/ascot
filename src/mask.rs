//! Corrections to the member types IRIS reports, where a declared `%ObjectHandle` says less than
//! the member returns: an instance of the class it's called on (`This`), or of a generic
//! parameter (`..#NAME`) -- a `CLASSNAME` parameter declared here, bound like an ObjectScript
//! parameter and otherwise its default. `mask.json`, from IRIS 2026.2's `%Library`, maps a class
//! to its corrected members; every other `%ObjectHandle` stays one, so a missing correction
//! shows up as an error naming it.

use crate::check::types::{HOTy, ty_from_mask};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Correction {
    returns: Option<String>,
    #[serde(default)]
    args: BTreeMap<String, String>,
    parameter: Option<String>,
    default: Option<String>,
}

static MASK: LazyLock<BTreeMap<String, BTreeMap<String, Correction>>> =
    LazyLock::new(|| serde_json::from_str(include_str!("mask.json")).expect("a valid mask.json"));

fn members(cls: &str) -> impl Iterator<Item = (&'static String, &'static Correction)> {
    MASK.get(cls).into_iter().flatten()
}

/// The correction for `member`, as declared in `cls`, if the mask has one.
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
            if let Some(t) = self.args.get(&arg.name) {
                arg.ty = ty_from_mask(Some(t.clone()));
            }
        }
    }
}

/// What generic parameter `param` of `cls` is, given `cls`'s generic arguments: the one bound to
/// it, or else its default. `None` if `cls` has no such parameter.
pub(crate) fn generic_arg(
    cls: &str,
    args: &BTreeMap<String, String>,
    param: &str,
) -> Option<String> {
    let (name, declared) =
        members(cls).find(|(name, c)| c.parameter.is_some() && name.eq_ignore_ascii_case(param))?;
    args.get(name).cloned().or_else(|| declared.default.clone())
}
