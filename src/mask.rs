use crate::bindings::ArgMode;
use crate::check::types::{HOTy, NormalArg, Ty, TyProcedure, VariadicArg, ty_from_mask};
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

#[derive(Deserialize)]
#[serde(untagged)]
enum Entry {
    // `null`: a member the dictionary lists but the compiler doesn't recognize (`##super` from
    // an override reports it missing, MPP5386) -- the inert `%OnNew` and `%Dispatch*` defaults.
    Class(BTreeMap<String, Option<Correction>>),
    Routine(BTreeMap<String, LabelSignature>),
}

static MASK: LazyLock<BTreeMap<String, Entry>> =
    LazyLock::new(|| serde_json::from_str(include_str!("mask.json")).expect("a valid mask.json"));

fn members(cls: &str) -> impl Iterator<Item = (&'static String, &'static Option<Correction>)> {
    match MASK.get(&format!("{cls}.cls")) {
        Some(Entry::Class(members)) => Some(members),
        _ => None,
    }
    .into_iter()
    .flatten()
}

fn entry(cls: &str, member: &str) -> Option<&'static Option<Correction>> {
    members(cls)
        .find(|(name, _)| name.eq_ignore_ascii_case(member))
        .map(|(_, c)| c)
}

// Whether the mask says `cls` has no member `member`, whatever the dictionary lists.
pub(crate) fn dropped(cls: &str, member: &str) -> bool {
    matches!(entry(cls, member), Some(None))
}

pub(crate) fn correction(cls: &str, member: &str) -> Option<&'static Correction> {
    entry(cls, member)?
        .as_ref()
        .filter(|c| c.parameter.is_none())
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
    let (name, declared) = members(cls).find(|(name, c)| {
        c.as_ref().is_some_and(|c| c.parameter.is_some()) && name.eq_ignore_ascii_case(param)
    })?;
    args.get(name)
        .cloned()
        .or_else(|| declared.as_ref()?.default.clone())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LabelSignature {
    args: Vec<String>,
    publics: Option<Vec<String>>,
    returns: Option<String>,
}

/// Each arg is `name`, `name=` (optional), `&name` (by reference) or a final `name...`.
pub(crate) fn label(routine: &str, label: &str) -> Option<Arc<TyProcedure>> {
    let sig =
        [".int", ".mac"]
            .iter()
            .find_map(|ext| match MASK.get(&format!("{routine}{ext}")) {
                Some(Entry::Routine(labels)) => labels.get(label),
                _ => None,
            })?;
    let mode = |name: &str| match name.strip_prefix('&') {
        Some(name) => (ArgMode::ByRef, name.to_string()),
        None => (ArgMode::Default, name.to_string()),
    };
    let mut normal_args = vec![];
    let mut variadic_arg = None;
    for arg in &sig.args {
        if let Some(name) = arg.strip_suffix("...") {
            let (mode, name) = mode(name);
            variadic_arg = Some(VariadicArg::new(mode, name, None));
        } else {
            let (name, default) = match arg.strip_suffix('=') {
                Some(name) => (name, Some(String::new())),
                None => (arg.as_str(), None),
            };
            let (mode, name) = mode(name);
            normal_args.push(NormalArg::new(mode, name, None, default));
        }
    }
    Some(Arc::new(TyProcedure {
        normal_args,
        variadic_arg,
        out: ty_from_mask(sig.returns.clone()).or(Some(Ty::Any)),
        procedure_block: sig.publics.clone(),
        uri: None,
        declared_at: None,
    }))
}
