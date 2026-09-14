use crate::bindings::{ArgMode, CollectionKind, MemberInfo, MemberKind, MethodInfo};

pub(crate) fn format_member_hover(member: &MemberInfo) -> String {
    let name = &member.name.content;
    let signature = match &member.kind {
        MemberKind::Method(info) => format!("Method {name}{}", format_method_signature(info)),
        MemberKind::ClassMethod(info) => {
            format!("ClassMethod {name}{}", format_method_signature(info))
        }
        MemberKind::ClientMethod(info) => {
            format!("ClientMethod {name}{}", format_method_signature(info))
        }
        MemberKind::Property(info) => {
            let of = match info.collection {
                Some(CollectionKind::List) => "list Of ",
                Some(CollectionKind::Array) => "array Of ",
                None => "",
            };
            let t = info.t.as_deref().map(|t| format!("{of}{t}"));
            format!("Property {name}{}", format_as(t.as_deref()))
        }
        MemberKind::Relationship(ty) => {
            format!("Relationship {name}{}", format_as(ty.as_deref()))
        }
        MemberKind::Parameter(info) => {
            let default = info
                .v
                .as_deref()
                .map(|v| format!(" = {v}"))
                .unwrap_or_default();
            format!("Parameter {name}{}{default}", format_as(info.t.as_deref()))
        }
        MemberKind::ForeignKey => format!("ForeignKey {name}"),
        MemberKind::Index => format!("Index {name}"),
        MemberKind::Projection => format!("Projection {name}"),
        MemberKind::Trigger => format!("Trigger {name}"),
        MemberKind::XData => format!("XData {name}"),
        MemberKind::Storage => format!("Storage {name}"),
        MemberKind::Query => format!("Query {name}"),
    };
    if member.doc.is_empty() {
        format!("```objectscript\n{signature}\n```")
    } else {
        format!("```objectscript\n{signature}\n```\n\n{}", member.doc)
    }
}

fn format_as(ty: Option<&str>) -> String {
    ty.map(|t| format!(" As {t}")).unwrap_or_default()
}

fn format_method_signature(info: &MethodInfo) -> String {
    let mut args: Vec<String> = info
        .normal
        .iter()
        .map(|arg| {
            format!(
                "{}{}{}",
                arg_mode_prefix(arg.mode),
                arg.name,
                format_as(arg.t.as_deref())
            )
        })
        .collect();
    if let Some(variadic) = &info.variadic {
        args.push(format!(
            "{}{}...{}",
            arg_mode_prefix(variadic.mode),
            variadic.name,
            format_as(variadic.t.as_deref())
        ));
    }
    format!("({}){}", args.join(", "), format_as(info.t.as_deref()))
}

fn arg_mode_prefix(mode: ArgMode) -> &'static str {
    match mode {
        ArgMode::ByRef => "ByRef ",
        ArgMode::Output => "Output ",
        ArgMode::Default => "",
    }
}
