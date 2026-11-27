//! Markdown rendering of a class member for hover text, in the form the InterSystems Language
//! Server gives its own: `(**Class**) Kind <u>**Name**</u>(*arg* As **Type**) As **Type**`, a
//! rule, the description as written. Used by `Workspace::hover` in `lib.rs`.

use crate::bindings::{CollectionKind, MemberInfo, MemberKind, Signature, member_keyword};
use crate::check::types::mode_prefix;

pub(crate) fn format_member_hover(member: &MemberInfo, class: Option<&str>) -> String {
    let name = &member.name;
    let kind = member_keyword(&member.kind);
    let rest = match &member.kind {
        MemberKind::Method(info)
        | MemberKind::ClassMethod(info)
        | MemberKind::ClientMethod(info)
        | MemberKind::Query(info) => format_method_signature(info),
        MemberKind::Property(info) => {
            let of = match info.collection {
                Some(CollectionKind::List) => "list Of ",
                Some(CollectionKind::Array) => "array Of ",
                None => "",
            };
            let t = info.t.as_deref().map(|t| format!("{of}{t}"));
            format_as(t.as_deref())
        }
        MemberKind::Relationship(info) => format_as(info.t.as_deref()),
        MemberKind::Parameter(info) => {
            let default = info
                .v
                .as_deref()
                .map(|v| format!(" = {v}"))
                .unwrap_or_default();
            format!("{}{default}", format_as(info.t.as_deref()))
        }
        _ => String::new(),
    };
    let class = class.map(|c| format!("(**{c}**) ")).unwrap_or_default();
    let header = format!("{class}{kind} <u>**{name}**</u>{rest}");
    let body: Vec<&str> = [
        member.doc.trim(),
        if member.deprecated {
            "*Deprecated*"
        } else {
            ""
        },
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect();
    if body.is_empty() {
        header
    } else {
        format!("{header}\n***\n{}", body.join("\n\n"))
    }
}

fn format_as(ty: Option<&str>) -> String {
    ty.map(|t| format!(" As **{t}**")).unwrap_or_default()
}

fn format_method_signature(info: &Signature) -> String {
    let mut args: Vec<String> = (info.normal.iter())
        .map(|arg| {
            let default = arg
                .default
                .as_deref()
                .map(|d| format!(" = {d}"))
                .unwrap_or_default();
            format!(
                "{}*{}*{}{default}",
                mode_prefix(arg.mode),
                arg.name,
                format_as(arg.t.as_deref())
            )
        })
        .collect();
    if let Some(variadic) = &info.variadic {
        args.push(format!(
            "{}*{}*...{}",
            mode_prefix(variadic.mode),
            variadic.name,
            format_as(variadic.t.as_deref())
        ));
    }
    format!("({}){}", args.join(", "), format_as(info.t.as_deref()))
}
