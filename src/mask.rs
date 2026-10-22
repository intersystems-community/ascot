//! Corrections to the member signatures IRIS reports, where a declared `%ObjectHandle` says less
//! than the member returns: an instance of the class it's called on (`This`), or an instance of
//! a generic parameter (`..#NAME`). A generic parameter is a `CLASSNAME` parameter the mask
//! declares, its value the default, bound like an ObjectScript parameter. `mask.json` maps a
//! class to just its corrected members, from IRIS 2026.2's `%Library`; every other
//! `%ObjectHandle` stays one, so a missing correction shows up as an error naming it.

use crate::bindings::{MemberInfo, MemberKind};
use std::collections::BTreeMap;
use std::sync::LazyLock;

static MASK: LazyLock<BTreeMap<String, Vec<MemberInfo>>> =
    LazyLock::new(|| serde_json::from_str(include_str!("mask.json")).expect("a valid mask.json"));

pub(crate) fn generic_params(cls: &str) -> impl Iterator<Item = &'static MemberInfo> {
    MASK.get(cls).into_iter().flatten().filter(
        |m| matches!(&m.kind, MemberKind::Parameter(p) if p.t.as_deref() == Some("CLASSNAME")),
    )
}

/// `member`, as IRIS reports it on `cls`, with its signature corrected -- if the mask has a
/// correction for it, in the class declaring it.
pub(crate) fn correction(cls: &str, member: &MemberInfo) -> Option<MemberInfo> {
    let origin = member.origin.as_deref().unwrap_or(cls);
    let generics: Vec<_> = generic_params(origin).collect();
    let masked = MASK.get(origin).into_iter().flatten().find(|m| {
        m.name.content.eq_ignore_ascii_case(&member.name.content)
            && !generics.iter().any(|g| std::ptr::eq(*g, *m))
    })?;
    Some(MemberInfo {
        kind: masked.kind.clone(),
        ..member.clone()
    })
}

/// What generic parameter `param` of `cls` is, given `cls`'s generic arguments: the one bound to
/// it, or else its default. `None` if `cls` has no such parameter.
pub(crate) fn generic_arg(
    cls: &str,
    args: &BTreeMap<String, String>,
    param: &str,
) -> Option<String> {
    let declared = generic_params(cls).find(|g| g.name.content.eq_ignore_ascii_case(param))?;
    let MemberKind::Parameter(default) = &declared.kind else {
        unreachable!("a generic parameter is a Parameter")
    };
    args.get(&declared.name.content)
        .cloned()
        .or_else(|| default.v.clone())
}
