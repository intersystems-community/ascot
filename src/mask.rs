//! `mask.json`: ascot's corrections to what IRIS reports about its own classes and routines, in
//! the declaration format (`declared`). `scripts/prebuilt-declarations.sh` composes it into the
//! pre-built declarations, and a live instance is read through it here.

use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

use serde::Deserialize;

use crate::bindings::{MemberInfo, MemberKind, RoutineInfo, Signature};
use crate::check::types::Ty;
use crate::declared::DeclaredClass;
use crate::details::MemberNs;

#[derive(Deserialize)]
struct Mask {
    classes: BTreeMap<String, MaskedClass>,
    /// The `%SYS` routine labels the system macros call, which IRIS ships as object code only.
    #[serde(default)]
    routines: BTreeMap<String, RoutineInfo>,
}

#[derive(Deserialize)]
struct MaskedClass {
    /// Members that replace the ones IRIS reports under the same name.
    #[serde(flatten)]
    declared: DeclaredClass,
    /// Members the dictionary lists but the compiler doesn't recognize (`##super` from an
    /// override reports it missing, MPP5386) -- the inert `%OnNew` and `%Dispatch*` defaults.
    #[serde(default)]
    drop: Vec<String>,
    /// What the class stands for as a type: `"Any"`, or another class's name.
    #[serde(rename = "type")]
    ty: Option<String>,
    /// The class parameters that type members (`..#NAME`), with their defaults.
    #[serde(default)]
    generics: BTreeMap<String, String>,
}

// The classes' members as `MemberInfo`s, by class and lower-cased name (a property and a method
// may share one). Parsing them types their signatures, which reads `RAW` through `class_type`,
// so the two stay separate statics.
static RAW: LazyLock<Mask> =
    LazyLock::new(|| serde_json::from_str(include_str!("mask.json")).expect("a valid mask.json"));
static MEMBERS: LazyLock<HashMap<String, HashMap<String, Vec<MemberInfo>>>> = LazyLock::new(|| {
    (RAW.classes.iter())
        .map(|(name, c)| {
            let info = (c.declared.to_class_info(name))
                .unwrap_or_else(|| panic!("mask.json: {name} does not parse"));
            let mut members: HashMap<String, Vec<MemberInfo>> = HashMap::new();
            for m in info.members {
                members
                    .entry(m.name.to_ascii_lowercase())
                    .or_default()
                    .push(m);
            }
            (name.clone(), members)
        })
        .collect()
});

pub(crate) fn class_type(cls: &str) -> Option<Ty> {
    match RAW.classes.get(cls)?.ty.as_deref()? {
        "Any" => Some(Ty::Any),
        t => Some(Ty::from(t)),
    }
}

// Whether the mask says `cls` has no member `member`, whatever the dictionary lists.
pub(crate) fn dropped(cls: &str, member: &str) -> bool {
    (RAW.classes.get(cls)).is_some_and(|c| c.drop.iter().any(|d| d.eq_ignore_ascii_case(member)))
}

/// The member of `cls` named `member` in `kind`'s namespace as the mask declares it, if it does.
pub(crate) fn replacement(
    cls: &str,
    member: &str,
    kind: &MemberKind,
) -> Option<&'static MemberInfo> {
    let ns = MemberNs::of_info(kind);
    (MEMBERS.get(cls)?.get(&member.to_ascii_lowercase())?.iter())
        .find(|m| MemberNs::of_info(&m.kind) == ns)
}

/// The default of `cls`'s generic parameter `param`, if the mask names it as one.
pub(crate) fn generic_default(cls: &str, param: &str) -> Option<String> {
    let generics = &RAW.classes.get(cls)?.generics;
    generics
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(param))
        .map(|(_, default)| default.clone())
}

/// A label's signature in a routine the mask covers.
pub(crate) fn curated_label(routine: &str, label: &str) -> Option<Signature> {
    let labels = RAW.routines.get(routine)?.labels.as_ref()?;
    labels.iter().find(|l| l.name == label)?.signature.clone()
}
