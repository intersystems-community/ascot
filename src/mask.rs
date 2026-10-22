//! Corrections to the member signatures IRIS reports, where a declared `%ObjectHandle` says less
//! than the member returns: an instance of the class it's called on (`Self`), or a collection's
//! element (`..#ELEMENTTYPE`, the receiver's bound parameter). `mask.json` is IRIS 2026.2's
//! `%Library` members of that kind, in the format of an `ascot declare` file; every other
//! `%ObjectHandle` stays one, so a missing correction shows up as an error naming it.

use crate::bindings::{ClassInfo, MemberInfo};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::LazyLock;

#[derive(Deserialize)]
struct Mask {
    classes: BTreeMap<String, ClassInfo>,
}

static MASK: LazyLock<Mask> =
    LazyLock::new(|| serde_json::from_str(include_str!("mask.json")).expect("a valid mask.json"));

/// `member`, as IRIS reports it on `cls`, with its signature corrected if the mask has one for
/// the class declaring it.
pub(crate) fn apply(cls: &str, mut member: MemberInfo) -> MemberInfo {
    let origin = member.origin.as_deref().unwrap_or(cls);
    if let Some(masked) = MASK.classes.get(origin).and_then(|masked| {
        masked
            .members
            .iter()
            .find(|m| m.name.content.eq_ignore_ascii_case(&member.name.content))
    }) {
        member.kind = masked.kind.clone();
    }
    member
}
