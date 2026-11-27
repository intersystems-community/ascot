//! Typing rules for ObjectScript's built-in `$`-functions (`$PIECE`, `$LIST*`, `$EXTRACT`,
//! `$BIT*`, `$VECTOR`, `$PROPERTY`, ...): argument counts, which arguments are outputs, and
//! result types.
//!
//! Implements `CheckExprInner` for `cos::FunctionCall` so `mod.rs` treats a `$`-function call
//! like any other expression. Class-library calls (`##class(...)`) are resolved through `env.rs`
//! instead, and `$`-special variables are handled in `mod.rs`; nothing here touches the workspace.

use super::*;

// Which functions refuse an empty argument, by position, as measured on IRIS: the first, the
// second, and any later one. A function not listed for a position isn't checked there.
const FIRST: u8 = 1;
const SECOND: u8 = 2;
const LATER: u8 = 4;

const NO_EMPTY: &[(&str, u8)] = &[
    ("Ascii", FIRST | SECOND),
    ("Bit", FIRST | SECOND),
    ("BitCount", FIRST | SECOND),
    ("BitFind", FIRST | SECOND | LATER),
    ("BitLogic", FIRST | SECOND),
    ("Change", SECOND),
    ("Char", FIRST | SECOND | LATER),
    ("ClassMethod", SECOND),
    ("Compile", SECOND),
    ("Data", FIRST | SECOND),
    ("Decimal", FIRST | SECOND),
    ("Extract", FIRST | SECOND),
    ("FNumber", FIRST),
    ("Factor", FIRST | SECOND),
    ("Find", FIRST | SECOND | LATER),
    ("Get", FIRST | SECOND),
    ("INumber", FIRST | SECOND | LATER),
    ("Increment", FIRST | SECOND),
    ("IsValidDouble", FIRST),
    ("IsValidNum", FIRST),
    ("Justify", FIRST | SECOND | LATER),
    ("Length", FIRST | SECOND),
    ("List", FIRST | SECOND),
    ("ListData", FIRST | SECOND),
    ("ListFind", FIRST | SECOND | LATER),
    ("ListFromString", FIRST | LATER),
    ("ListGet", FIRST | SECOND),
    ("ListNext", SECOND),
    ("ListSame", FIRST | SECOND),
    ("ListToString", FIRST | LATER),
    ("ListUpdate", SECOND),
    ("Locate", FIRST | SECOND),
    ("Lock", FIRST | SECOND),
    ("MV", FIRST),
    ("Match", FIRST | SECOND),
    ("Method", SECOND),
    ("NConvert", FIRST | SECOND),
    ("Next", FIRST | SECOND),
    ("Normalize", FIRST | SECOND),
    ("Number", FIRST),
    ("Parameter", SECOND),
    ("Piece", FIRST | SECOND | LATER),
    ("Property", SECOND),
    ("QSubscript", FIRST | SECOND),
    ("Replace", FIRST | SECOND),
    ("SConvert", FIRST | SECOND),
    ("Stack", FIRST | SECOND),
    ("Translate", FIRST | SECOND | LATER),
    ("Vector", FIRST | SECOND),
    ("VectorDefined", FIRST | SECOND),
    ("View", FIRST | SECOND | LATER),
    ("WAscii", FIRST | SECOND),
    ("WChar", FIRST | SECOND | LATER),
    ("WExtract", FIRST | SECOND),
    ("WFind", FIRST | SECOND | LATER),
    ("WLength", FIRST | SECOND),
    ("Xecute", FIRST),
    ("ZBitAnd", FIRST | SECOND),
    ("ZBitFind", FIRST | SECOND | LATER),
    ("ZBitGet", FIRST | SECOND),
    ("ZBitOr", FIRST | SECOND),
    ("ZBitSet", FIRST | SECOND | LATER),
    ("ZBitStr", FIRST | SECOND),
    ("ZBitXor", FIRST | SECOND),
    ("ZBoolean", FIRST | SECOND | LATER),
    ("ZConvert", FIRST | SECOND | LATER),
    ("ZCrc", FIRST | SECOND | LATER),
    ("ZDAscii", FIRST | SECOND),
    ("ZDChar", FIRST | SECOND | LATER),
    ("ZDate", FIRST),
    ("ZDateH", FIRST),
    ("ZDateTime", FIRST),
    ("ZDateTimeH", FIRST),
    ("ZF", FIRST | SECOND | LATER),
    ("ZIncrement", FIRST | SECOND),
    ("ZLAscii", FIRST | SECOND),
    ("ZLChar", FIRST | SECOND | LATER),
    ("ZName", FIRST | SECOND | LATER),
    ("ZObjClassMethod", SECOND),
    ("ZObjMethod", SECOND),
    ("ZObjProperty", SECOND),
    ("ZPosition", FIRST | SECOND | LATER),
    ("ZPower", FIRST | SECOND),
    ("ZQAscii", FIRST | SECOND),
    ("ZQChar", FIRST | SECOND | LATER),
    ("ZSeek", FIRST | SECOND),
    ("ZStrip", FIRST | SECOND),
    ("ZTime", FIRST),
    ("ZTimeH", FIRST),
    ("ZUtil", FIRST | SECOND | LATER),
    ("ZWAscii", FIRST | SECOND),
    ("ZWChar", FIRST | SECOND | LATER),
    ("ZWidth", FIRST | SECOND),
    ("ZZenkaku", FIRST | SECOND | LATER),
];

// Any `$`-function call, to see which of its arguments are empty.
#[derive(Grammar)]
struct AnyCall(
    Option<tygr::StringEq!("$")>,
    tygr::StringOf1<cos::IsAlphabetic>,
    cos::FunArgList,
);

// `$Z` functions IRIS compiles but doesn't document: the object-system internals the kernel's
// own classes call. Noted with a hint and typed as Any, per GOVERNANCE.md.
const UNDOCUMENTED_FUNCTIONS: &[&str] = &[
    "ZOBJEXPORT",
    "ZOBJVAL",
    "ZOBJNEW",
    "ZOBJNEXT",
    "ZOBJOID",
    "ZOBJMOD",
    "ZOBJMODS",
];

fn check_empty_args(call: &cos::FunctionCall) -> Result<(), QuasiTypeError> {
    let Some(name) = call.name() else {
        return Ok(());
    };
    let Ok(AnyCall(_, _, args)) = AnyCall::parse(&call.print()) else {
        return Ok(());
    };
    // `$ZSEEK()`: no argument at all, not an empty one, for a function that takes none.
    if args.items.len() == 1
        && args.items[0].is_none()
        && cos::simple_function(name.full).is_some_and(|f| f.min == 0)
    {
        return Ok(());
    }
    let positions = NO_EMPTY
        .iter()
        .find(|(f, _)| f.eq_ignore_ascii_case(name.full))
        .map_or(0, |&(_, positions)| positions);
    for (i, arg) in args.items.iter().enumerate() {
        let at = match i {
            0 => FIRST,
            1 => SECOND,
            _ => LATER,
        };
        if arg.is_none() && positions & at != 0 {
            let function = format!("${}", name.full.to_uppercase());
            return Err(TypeErrorKind::OmittedArgument {
                function,
                position: i + 1,
            }
            .into());
        }
    }
    Ok(())
}

// IRIS won't name a procedure block's private variable.
// `$NAME`, `$QUERY` and `$ZORDER` refuse a private local; `$ORDER` and `$ZPREVIOUS` one without
// subscripts, as measured.
fn require_public<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    target: &cos::GetTarget,
    function: &str,
    subscripted_too: bool,
) -> Result<(), QuasiTypeError> {
    if let cos::GetTarget::Var(variable) = target {
        require_public_variable(env, flow, variable, function, subscripted_too)?;
    }
    Ok(())
}

fn require_public_variable<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    variable: &Variable,
    function: &str,
    subscripted_too: bool,
) -> Result<(), QuasiTypeError> {
    if let Left(local) = variable.as_local(env, flow)?
        && (subscripted_too || local.subs.is_empty())
        && !flow.is_public(&local.name)?
    {
        return Err(TypeErrorKind::PrivateVariableNotAllowed {
            function: function.to_string(),
            name: local.name,
        }
        .into());
    }
    Ok(())
}

// An argument that may be left empty (`$FN(x,,0)`): the expression inside, if any.
fn opt_arg<'b, M, W, T>(arg: &'b Option<M>) -> Option<&'b Ranged<T>>
where
    M: Deref<Target = W>,
    W: Deref<Target = Option<Ranged<T>>> + 'b,
{
    arg.as_deref().and_then(|w| w.deref().as_ref())
}

fn arg<'b, M, W, T>(arg: &'b Option<M>) -> Option<&'b Ranged<T>>
where
    M: Deref<Target = W>,
    W: Deref<Target = Ranged<T>> + 'b,
{
    arg.as_deref().map(|w| w.deref())
}

// `$ZF(-3)`, `$ZF(-5)` and `$ZF(-6)`: `selectors` name the callout, whose `args` go by value.
fn check_zf_callout<'a, 'b>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    selectors: impl IntoIterator<Item = &'b cos::More<cos::ArgOf<Expr>>>,
    args: &[cos::More<cos::ArgOf<Option<cos::MthArg>>>],
) -> Result<PowerTy, QuasiTypeError> {
    if each_arg(args).any(|arg| matches!(**arg, Some(cos::MthArg::ByRef { .. }))) {
        return Err(TypeErrorKind::ByRefArgumentNotAllowed {
            function: "$ZF".into(),
        }
        .into());
    }
    for selector in selectors {
        let _ = selector.to_ty(env, flow)?;
    }
    Ok(check_args_any(env, flow, each_arg(args))?.into())
}

fn each_arg<'b, M, W, T: 'b>(args: &'b [M]) -> impl Iterator<Item = &'b Ranged<T>>
where
    M: Deref<Target = W>,
    W: Deref<Target = Ranged<T>> + 'b,
{
    args.iter().map(|m| m.deref().deref())
}

impl CheckExprInner for cos::FunctionCall {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        super::report_style(env, self.name(), &self.arg_commas());
        check_empty_args(self)?;
        match self {
            FunctionCall::Length {
                args:
                    LengthArgs {
                        expression,
                        delimitor,
                        ..
                    },
                ..
            } => check_read_args(
                env,
                flow,
                [Some(expression.deref()), arg(delimitor)],
                "%Integer",
            ),
            FunctionCall::Simple(call) => check_simple_function(env, flow, call),
            FunctionCall::Generic(_) => unreachable!("never converts"),
            FunctionCall::Unknown(call) => check_unknown_call(env, flow, call),
            FunctionCall::Data {
                args: DataArgs {
                    variable, target, ..
                },
                ..
            } => check_data(env, flow, variable.deref(), arg(target)),
            FunctionCall::Order {
                args:
                    OrderArgs {
                        variable,
                        direction,
                        target,
                        ..
                    },
                ..
            } => check_order(env, flow, variable.deref(), arg(direction), arg(target)),
            FunctionCall::Get {
                args: GetArgs {
                    variable, default, ..
                },
                ..
            } => check_get(env, flow, variable.deref(), arg(default)),
            FunctionCall::DText(dtext) => {
                let offset = match &dtext.arg {
                    DTextArg::Label(AllowIndirection::Plain(label)) => label.offset.as_deref(),
                    DTextArg::Offset(offset, _) => Some(offset.deref()),
                    DTextArg::Label(AllowIndirection::Indirection(_)) => None,
                };
                check_read_args(env, flow, [offset], "%String")
            }
            // Internal storage handle -- opaque, like `$ZUTIL`'s catch-all return.
            FunctionCall::ZObjRef {
                args: ZObjRefArgs { oref, .. },
                ..
            } => {
                let _ = oref.deref().to_ty(env, flow)?;
                Ok(Ty::Any.into())
            }
            FunctionCall::ZStrip {
                args:
                    ZStripArgs {
                        string,
                        action,
                        remchar,
                        keepchar,
                        ..
                    },
                ..
            } => {
                string.deref().to_ty(env, flow)?;
                check_code_arg(env, flow, action, check_zstrip_action, |action, reason| {
                    TypeErrorKind::InvalidZStripAction { action, reason }
                })?;
                for chars in [remchar, keepchar].into_iter().flatten() {
                    chars.check(env, flow)?;
                }
                Ok(Ty::from("%String").into())
            }
            FunctionCall::ZDateTime {
                args: ZDateTimeArgs { value, rest, .. },
                ..
            }
            | FunctionCall::ZDate {
                args: ZDateArgs { value, rest, .. },
                ..
            }
            | FunctionCall::ZTime {
                args: ZTimeArgs { value, rest, .. },
                ..
            } => {
                let _ = value.to_ty(env, flow)?;
                for arg in rest {
                    arg.check(env, flow)?;
                }
                Ok(Ty::from("%String").into())
            }
            FunctionCall::ZFm1 { program, extra, .. } => {
                let ty = check_read_args(env, flow, [arg(program)], "%Integer")?;
                if !extra.is_empty() {
                    check_read_args(env, flow, each_arg(extra).map(Some), "%Integer")?;
                    let given = 2 + extra.len();
                    flow.add_soft_diagnostic(
                        TypeErrorKind::UndocumentedArgument {
                            function: "ZF(-1)".to_string(),
                            documented: 2,
                            given,
                        }
                        .into(),
                    );
                }
                Ok(ty)
            }
            FunctionCall::ZFm2 { program, .. } => {
                check_read_args(env, flow, [arg(program)], "%Integer")
            }
            FunctionCall::ZFm100 {
                flags,
                program,
                args,
                ..
            } => {
                let _ = flags.to_ty(env, flow)?;
                let _ = program.to_ty(env, flow)?;
                check_args_any(env, flow, each_arg(args))?;
                Ok(Ty::from("%Integer").into())
            }
            FunctionCall::ZFm3 { dll, args, .. } => check_zf_callout(env, flow, [dll], args),
            FunctionCall::ZFm4 {
                n, third, fourth, ..
            } => check_read_args(
                env,
                flow,
                [Some(n.deref().deref()), arg(third), arg(fourth)],
                "%Integer",
            ),
            FunctionCall::ZFm5 {
                dll_id,
                func_id,
                args,
                ..
            } => check_zf_callout(env, flow, [dll_id, func_id], args),
            FunctionCall::ZFm6 {
                dll_index, args, ..
            } => check_zf_callout(env, flow, [dll_index], args),
            FunctionCall::ZU { kw, args } => check_zu_args(env, flow, kw, args),
            FunctionCall::Case {
                args: CaseArgs { target, arms, .. },
                ..
            } => {
                let target = target.deref();
                let mut branches = Vec::new();
                let mut default = None;
                for arm in arms {
                    if default.is_some() {
                        return Err(TypeErrorKind::CaseDefaultNotLast.into());
                    }
                    match arm.deref().deref().deref() {
                        CaseArm::Branch(branch) => branches.push((&branch.e1, &branch.e2)),
                        CaseArm::Default(value) => default = Some(value.deref()),
                    }
                }
                let target = target.to_power_ty(env, flow)?;
                if branches.is_empty() && default.is_none() {
                    flow.add_soft_diagnostic(TypeErrorKind::CaseWithoutArms.into());
                    return Ok(Ty::Any.into());
                }
                let branches = branches
                    .iter()
                    .map(|(value, then)| (Taken::Equal(&target, value), *then))
                    .chain(default.map(|then| (Taken::Always, then)));
                let (ty, exhaustive) = check_branches(env, flow, branches)?;
                if !exhaustive {
                    flow.add_soft_diagnostic(TypeErrorKind::CaseWithoutDefault.into());
                }
                Ok(ty.into())
            }
            FunctionCall::Bit(c) => check_bit_call(env, flow, c),
            FunctionCall::BitCount {
                args: BitCountArgs { bits, value, .. },
                ..
            } => {
                check_lenient_arg(env, flow, bits.as_ref())?;
                check_read_args(env, flow, [arg(value)], "%Integer")
            }
            FunctionCall::BitFind {
                args:
                    BitFindArgs {
                        bits,
                        value,
                        position,
                        direction,
                        ..
                    },
                ..
            } => {
                check_lenient_arg(env, flow, bits.as_ref())?;
                check_read_args(
                    env,
                    flow,
                    [Some(value.deref().deref()), arg(position), arg(direction)],
                    "%Integer",
                )
            }
            FunctionCall::BitLogic { expr, length, .. } => {
                let _ = expr.deref().to_ty(env, flow)?;
                check_read_args(env, flow, [arg(length)], "%DataType")
            }
            FunctionCall::Change {
                args:
                    ChangeArgs {
                        string,
                        target,
                        replacement,
                        occurrences,
                        begin,
                        ..
                    },
                ..
            } => check_read_args(
                env,
                flow,
                [
                    Some(string.deref()),
                    Some(target.deref().deref()),
                    Some(replacement.deref().deref()),
                    arg(occurrences),
                    arg(begin),
                ],
                "%String",
            ),
            FunctionCall::Char {
                args: CharArgs { first, rest, .. },
                ..
            }
            | FunctionCall::WChar {
                args: WCharArgs { first, rest, .. },
                ..
            } => check_read_args(
                env,
                flow,
                once(Some(first.deref())).chain(each_arg(rest).map(Some)),
                "%String",
            ),
            FunctionCall::ClassName {
                args: ClassNameArgs { oref, .. },
                ..
            } => check_read_args(env, flow, [oref.as_deref()], "%String"),
            FunctionCall::Decimal {
                args: DecimalArgs { num, digits, .. },
                ..
            } => check_read_args(env, flow, [Some(num.deref()), arg(digits)], "%Decimal"),
            FunctionCall::Double {
                args: DoubleArgs { num, .. },
                ..
            } => check_read_args(env, flow, [Some(num.deref())], "%Double"),
            FunctionCall::Extract(c) => check_extract_call(env, flow, c),
            FunctionCall::FNumber {
                args:
                    FNumberArgs {
                        num,
                        format,
                        decimal,
                        ..
                    },
                ..
            } => {
                let _ = num.deref().to_ty(env, flow)?;
                if let Some(format) = opt_arg(format) {
                    check_code_arg(env, flow, format, check_fnumber_format, |format, reason| {
                        TypeErrorKind::InvalidFNumberFormat { format, reason }
                    })?;
                }
                check_read_args(env, flow, [arg(decimal)], "%String")
            }
            FunctionCall::Increment {
                args: IncrementArgs {
                    counter, amount, ..
                },
                ..
            } => check_increment(env, flow, counter.deref(), arg(amount)),
            FunctionCall::IsObject {
                args: IsObjectArgs { expr, .. },
                ..
            } => check_is_object(env, flow, expr),
            FunctionCall::ListRange(c) => {
                let _ = c.list.deref().to_ty(env, flow)?;
                for pair in std::iter::once(c.first.deref()).chain(c.rest.iter().map(|v| &**v)) {
                    let _ = (&pair.e1).to_ty(env, flow)?;
                    let _ = (&pair.e2).to_ty(env, flow)?;
                }
                Ok(Ty::from("%String").into())
            }
            FunctionCall::ListPlain(c) => check_list_plain_call(env, flow, c),
            FunctionCall::ListBuild(ListBuild {
                args: ListBuildArgs { first, rest, .. },
                ..
            }) => {
                let mut null_free = true;
                for arg in once(first.deref()).chain(each_arg(rest)) {
                    null_free &= match arg.deref() {
                        Some(element) => {
                            check_lenient_arg(env, flow, Ranged::new(arg.start, element, arg.end))?
                        }
                        None => false,
                    };
                }
                Ok(if null_free {
                    PowerTy::NullFreeList
                } else {
                    Ty::from("%List").into()
                })
            }
            FunctionCall::ListData {
                args:
                    ListDataArgs {
                        list,
                        position,
                        var,
                        ..
                    },
                ..
            } => {
                check_read_args(env, flow, [Some(list.deref()), arg(position)], "%Boolean")?;
                check_out_var(env, flow, arg(var), "%String")?;
                Ok(Ty::from("%Boolean").into())
            }
            FunctionCall::ListGet(c) => {
                check_read_args(env, flow, [Some(c.list.deref())], "%String")?;
                check_read_args(env, flow, [opt_arg(&c.position)], "%String")?;
                check_read_args(env, flow, [arg(&c.default)], "%String")
            }
            FunctionCall::ListNext {
                args: ListNextArgs {
                    list, ptr, value, ..
                },
                ..
            } => check_listnext(
                env,
                flow,
                list.deref(),
                ptr.deref().deref(),
                value.deref().deref(),
            ),
            FunctionCall::ListUpdate {
                args:
                    ListUpdateArgs {
                        list,
                        position,
                        value,
                        rest,
                        ..
                    },
                ..
            } => {
                let _ = list.deref().to_ty(env, flow)?;
                let _ = position.deref().deref().to_ty(env, flow)?;
                for v in std::iter::once(value.deref().deref()).chain(each_arg(rest)) {
                    check_read_args(env, flow, [v.cond.as_deref(), Some(&v.value)], "%List")?;
                }
                Ok(Ty::from("%List").into())
            }
            FunctionCall::Locate {
                args:
                    LocateArgs {
                        string,
                        regexp,
                        start,
                        end,
                        val,
                        ..
                    },
                ..
            } => {
                check_read_args(
                    env,
                    flow,
                    [
                        Some(string.deref()),
                        Some(regexp.deref().deref()),
                        opt_arg(start),
                    ],
                    "%Integer",
                )?;
                check_out_var(env, flow, opt_arg(end), "%Integer")?;
                check_out_var(env, flow, opt_arg(val), "%String")?;
                Ok(Ty::from("%Integer").into())
            }
            FunctionCall::MV(c) => check_mv_call(env, flow, c),
            FunctionCall::Method {
                args:
                    MethodArgs {
                        instance: classname,
                        method,
                        args,
                        ..
                    },
                ..
            }
            | FunctionCall::ClassMethod {
                args:
                    ClassMethodArgs {
                        classname,
                        method,
                        args,
                        ..
                    },
                ..
            } => {
                classname.check(env, flow)?;
                let _ = method.to_ty(env, flow)?;
                Ok(check_args_any(env, flow, each_arg(args))?.into())
            }
            FunctionCall::Name {
                args: NameArgs {
                    variable, integer, ..
                },
                ..
            } => {
                require_public(env, flow, variable.deref(), "$NAME", true)?;
                if let Left(local) = target_as_local(env, flow, variable)? {
                    flow.handle_get(local)?;
                }
                check_read_args(env, flow, [arg(integer)], "%String")
            }
            FunctionCall::Parameter {
                args: ParameterArgs {
                    class, parameter, ..
                },
                ..
            } => {
                class.check(env, flow)?;
                check_read_args(env, flow, [Some(parameter.deref().deref())], "%String")
            }
            FunctionCall::Piece(c) => check_piece_call(env, flow, c),
            FunctionCall::PrefetchOff {
                args: PrefetchOffArgs { first, second, .. },
                ..
            } => {
                for g in first.as_deref().into_iter().chain(arg(second)) {
                    check_global_arg(env, flow, g)?;
                }
                Ok(Ty::from("%String").into())
            }
            FunctionCall::PrefetchOn {
                args: PrefetchOnArgs { gref, gref2, .. },
                ..
            } => {
                for g in std::iter::once(gref.deref()).chain(arg(gref2)) {
                    if let AllowIndirection::Plain(g) = g.deref() {
                        check_global_ref(env, flow, g)?;
                    }
                }
                Ok(Ty::from("%Integer").into())
            }
            FunctionCall::Property(c) => check_property_call(env, flow, c),
            FunctionCall::Query {
                args:
                    QueryArgs {
                        reference,
                        direction,
                        target,
                        ..
                    },
                ..
            } => {
                require_public(env, flow, reference.deref(), "$QUERY", true)?;
                if let Left(local) = target_as_local(env, flow, reference)? {
                    flow.handle_get(local)?;
                }
                check_read_args(env, flow, [arg(direction)], "%String")?;
                check_out_var(env, flow, arg(target), "%String")?;
                Ok(Ty::from("%String").into())
            }
            FunctionCall::Select {
                args: SelectArgs { first, rest, .. },
                ..
            } => {
                let branches = std::iter::once(first.deref())
                    .chain(each_arg(rest))
                    .map(|b| (Taken::If(&b.e1), &b.e2));
                let (ty, exhaustive) = check_branches(env, flow, branches)?;
                if !exhaustive {
                    flow.add_soft_diagnostic(TypeErrorKind::SelectWithoutDefault.into());
                }
                Ok(ty.into())
            }
            FunctionCall::Sequence(c) => check_sequence_call(env, flow, c),
            FunctionCall::SortBegin {
                args: SortBeginArgs { set_global, .. },
                ..
            } => {
                check_global_arg(env, flow, set_global)?;
                Ok(Ty::from("%Integer").into())
            }
            FunctionCall::SortEnd {
                args: SortEndArgs { first, dosort, .. },
                ..
            } => {
                if let Some(Some(g)) = first.as_deref().map(|f| f.deref()) {
                    check_global_arg(env, flow, g)?;
                }
                check_read_args(env, flow, [arg(dosort)], "%Integer")
            }
            FunctionCall::Vector(c) => check_vector_call(env, flow, c),
            FunctionCall::VectorDefined {
                args:
                    VectorDefinedArgs {
                        vector,
                        position,
                        value,
                        ..
                    },
                ..
            } => {
                check_vector_arg(env, flow, vector.as_ref())?;
                check_read_args(env, flow, [Some(position.deref().deref())], "%Boolean")?;
                check_out_var(env, flow, arg(value), "%String")?;
                Ok(Ty::from("%Boolean").into())
            }
            FunctionCall::VectorOp {
                args: VectorOpArgs {
                    op, first, rest, ..
                },
                ..
            } => {
                check_vector_arg(env, flow, first.deref().as_ref())?;
                check_read_args(
                    env,
                    flow,
                    once(Some(op.deref())).chain(each_arg(rest).map(Some)),
                    "%String",
                )
            }
            FunctionCall::WExtract(c) => check_wextract_call(env, flow, c),
            FunctionCall::Xecute {
                args: XecuteArgs { code, params, .. },
                ..
            } => {
                let _ = code.to_ty(env, flow)?;
                for param in each_arg(params) {
                    match param.deref() {
                        cos::MthArg::Expr { expr, .. } => {
                            let _ = expr.to_ty(env, flow)?;
                        }
                        // The code may assign it: whatever was known of it is gone.
                        cos::MthArg::ByRef { target, .. } => {
                            if let Some(variable) = target.as_variable()
                                && let Left(local) = variable.as_local(env, flow)?
                            {
                                flow.invalidate(local, Ty::Any.into())?;
                            }
                        }
                    }
                }
                Ok(Ty::from("%String").into())
            }
            FunctionCall::Compile { args, .. } => check_compile(env, flow, args),
            FunctionCall::ZOrder {
                args: ZOrderArgs { variable, .. },
                ..
            } => {
                require_public(env, flow, variable.deref(), "$ZORDER", true)?;
                let _ = variable.to_maybe_ty(env, flow)?;
                Ok(PowerTy::from("%DataType"))
            }
            FunctionCall::ZP {
                args: ZPArgs { variable, .. },
                ..
            } => {
                if let AllowIndirection::Plain(plain) = variable.deref().deref() {
                    require_public_variable(env, flow, plain, "$ZPREVIOUS", false)?;
                }
                let _ = variable.to_maybe_ty(env, flow)?;
                Ok(PowerTy::from("%DataType"))
            }
            FunctionCall::Next {
                args: NextArgs { variable, .. },
                ..
            } => {
                let _ = variable.to_maybe_ty(env, flow)?;
                Ok(PowerTy::from("%DataType"))
            }
        }
    }
}

// `Set $PIECE(x, ..) = v` (likewise `$EXTRACT`, `$LIST`, `$BIT`, ...) on an undefined `x` is fine:
// it starts from the empty string, so the target is defined from here on rather than read; then
// the call is checked as a read.
fn check_set_fun<'a, C>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    base: Option<&Variable>,
    range: bindings::Range,
    c: &C,
    check: impl FnOnce(&CodeEnv<'a>, &mut Flow<'a>, &C) -> Result<PowerTy, QuasiTypeError>,
) -> Result<(), QuasiTypeError> {
    if let Some(variable) = base
        && let Left(local) = variable.as_local(env, flow)?
        && flow
            .handle_get(local.clone())?
            .to_maybe_ty(env)
            .maybe_defined()
            .is_none()
    {
        flow.handle_set(local, Ty::from("%String").into(), range)?;
    }
    check(env, flow, c).map(drop)
}

fn check_set_expr_fun<'a, C>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    base: &Ranged<Expr>,
    c: &C,
    check: impl FnOnce(&CodeEnv<'a>, &mut Flow<'a>, &C) -> Result<PowerTy, QuasiTypeError>,
) -> Result<(), QuasiTypeError> {
    // `SET $PIECE(..P, ...) = v` writes a node of the object, which a MultiDimensional property
    // is; any other property is `<OBJECT DISPATCH>` when it runs.
    if let Some(AtomicExpr::Member { member, args: None }) = base.as_atomic() {
        super::require_multidimensional(env, flow, member.deref())?;
    }
    check_set_fun(env, flow, base.as_variable(), env.range(base), c, check)
}

// A variable named alone is read leniently, an undefined one as absent; any other expression
// must have a value. Whether the argument surely has one.
fn check_lenient_arg<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    arg: Ranged<&cos::VarOrExpr>,
) -> Result<bool, QuasiTypeError> {
    match arg.ranged {
        cos::VarOrExpr::Var(var) => {
            let ty = match var.as_local(env, flow)? {
                Left(local) => flow.handle_get(local)?,
                Right(ty) => ty,
            };
            Ok(!ty.to_maybe_ty(env).maybe_undefined())
        }
        cos::VarOrExpr::Expr(expr) => {
            let _ = Ranged::new(arg.start, expr, arg.end).to_ty(env, flow)?;
            Ok(true)
        }
    }
}

// A vector is a variable or a literal; a property is one only when MultiDimensional.
fn check_vector_arg<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    arg: Ranged<&cos::VarOrExpr>,
) -> Result<(), QuasiTypeError> {
    if let cos::VarOrExpr::Expr(expr) = arg.ranged
        && let Some(AtomicExpr::Member { member, args: None }) = expr.as_atomic()
    {
        super::require_multidimensional(env, flow, member.deref())?;
    }
    check_lenient_arg(env, flow, arg)?;
    Ok(())
}

/// Check a [`SetTargetFun`] -- the settable functions on the left of `SET`.
/// Reuses the same per-function checks as the read positions.
pub(super) fn check_settarget_fun<'a>(
    fun: &cos::SetTargetFun,
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
) -> Result<(), QuasiTypeError> {
    super::report_style(env, fun.name(), &[]);
    use cos::SetTargetFun::*;
    match fun {
        Piece(c) => check_set_expr_fun(env, flow, &c.string, c, check_piece_call)?,
        Extract(c) => check_set_expr_fun(env, flow, &c.string, c, check_extract_call)?,
        List(c) => check_set_expr_fun(env, flow, &c.list, c, check_list_plain_call)?,
        ListGet(c) => check_set_var_list(env, flow, &c.first, &c.rest)?,
        Bit(c) => check_set_fun(
            env,
            flow,
            c.bits.as_variable(),
            env.range(c.bits.deref()),
            c,
            check_bit_call,
        )?,
        Vector(c) => {
            if c.third.is_none() {
                return Err(TypeErrorKind::SetVectorWithoutType.into());
            }
            check_set_fun(
                env,
                flow,
                c.vector.as_variable(),
                env.range(c.vector.deref()),
                c,
                check_vector_call,
            )?;
        }
        WExtract(c) => check_set_expr_fun(env, flow, &c.string, c, check_wextract_call)?,
        Property(c) => check_property_call(env, flow, c).map(drop)?,
        Sequence(c) => check_sequence_call(env, flow, c).map(drop)?,
        ListBuild(c) => check_set_var_list(env, flow, &c.first, &c.rest)?,
        MV(c) => check_mv_call(env, flow, c).map(drop)?,
        Undocumented(c) => check_unknown_call(env, flow, c).map(drop)?,
        Simple(c) => {
            env.soft_diagnostic(
                TypeErrorKind::UndocumentedSyntax {
                    what: format!("`${}` as a Set target", c.name.print()),
                }
                .at(env.range(&c.name)),
            );
            let _ = check_simple_function(env, flow, c)?;
        }
    };
    Ok(())
}

fn check_unknown_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    call: &cos::UnknownFunctionCall,
) -> Result<PowerTy, QuasiTypeError> {
    for arg in call.args.items(env) {
        if let Some(cos::FunArg::Normal { expr, .. }) = arg.deref() {
            let _ = expr.to_ty(env, flow)?;
        }
    }
    let name = call.name.print();
    if UNDOCUMENTED_FUNCTIONS
        .iter()
        .any(|f| f.eq_ignore_ascii_case(&name))
    {
        flow.add_soft_diagnostic(TypeErrorKind::UndocumentedFunction { name }.into());
        return Ok(PowerTy::plain(Ty::Any));
    }
    let names = cos::FUNCTION_NAMES.iter().copied().chain(
        cos::SIMPLE_FUNCTIONS
            .iter()
            .flat_map(|f| f.names.iter().copied()),
    );
    let suggestions = crate::check::top_similar(&name, names, 5);
    flow.add_soft_diagnostic(
        TypeErrorKind::UnknownFunction {
            name,
            suggestions: Suggestions(suggestions),
        }
        .into(),
    );
    Ok(PowerTy::plain(Ty::Any))
}

// One variable slot of `SET $LISTBUILD(a,,b)=` / `SET $LISTGET(a,,b)=`, empty for a placeholder.
type SetVarArg = Wrap<MGap, Ranged<Option<GetTarget>>, MGap>;

// `SET $LISTBUILD(a,,b)=list` / `SET $LISTGET(a,,b)=list`: every named variable is assigned.
fn check_set_var_list<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    first: &SetVarArg,
    rest: &[Prefix<StringEq!(","), SetVarArg>],
) -> Result<(), QuasiTypeError> {
    for arg in once(first.deref()).chain(each_arg(rest)) {
        let range = env.range(arg);
        match arg.deref() {
            Some(GetTarget::Var(variable)) => {
                if let Left(local) = variable.as_local(env, flow)? {
                    flow.handle_set(local, Ty::Any.into(), range)?
                }
            }
            Some(GetTarget::Member { member, args }) => {
                if args.is_none() {
                    super::require_multidimensional(env, flow, member.deref())?;
                }
                let obj = env.this_ty(flow);
                super::check_set_member(env, flow, obj, member.deref(), args.as_ref())?;
            }
            Some(GetTarget::Sub { base, op }) => {
                let obj = base.to_ty(env, flow)?;
                super::check_set_member(env, flow, obj, op.mem.deref(), op.args.as_ref())?;
            }
            Some(GetTarget::Indirection(indirection)) => {
                let _ = indirection.expr.to_ty(env, flow)?;
                flow.invalidate_all()?;
            }
            Some(GetTarget::Fun(_)) | None => {}
        }
    }
    Ok(())
}

fn check_sequence_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    c: &SequenceCall,
) -> Result<PowerTy, QuasiTypeError> {
    if let Left(local) = target_as_local(env, flow, &c.gvar)? {
        flow.handle_set(
            local,
            Ty::from("%Integer").into(),
            env.range(&c.gvar.wrapped),
        )?;
    }
    Ok(Ty::from("%Integer").into())
}

fn check_bit_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    c: &BitCall,
) -> Result<PowerTy, QuasiTypeError> {
    check_lenient_arg(env, flow, c.bits.as_ref())?;
    check_read_args(env, flow, [Some(c.position.deref().deref())], "%Integer")
}

fn check_piece_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    c: &PieceCall,
) -> Result<PowerTy, QuasiTypeError> {
    check_read_args(
        env,
        flow,
        [Some(c.string.deref()), Some(c.delimiter.deref().deref())],
        "%String",
    )?;
    check_read_args(env, flow, [arg(&c.from), opt_arg(&c.to)], "%String")
}

fn check_mv_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    c: &cos::MVCall,
) -> Result<PowerTy, QuasiTypeError> {
    let positions = c.rest.iter().map(|arg| arg.deref().deref());
    let in_positions = positions.flat_map(|p| {
        p.first
            .iter()
            .chain(p.more.iter().filter_map(|m| m.deref().deref().as_ref()))
    });
    for e in once(c.record.deref()).chain(in_positions) {
        let _ = e.to_ty(env, flow)?;
    }
    flow.add_soft_diagnostic(
        TypeErrorKind::UndocumentedFunction {
            name: "MV".to_string(),
        }
        .into(),
    );
    Ok(PowerTy::plain(Ty::Any))
}

impl CheckExprInner for PositionArg {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        match self {
            PositionArg::Rel { md: Some(md), .. } => Ok(md.e.to_ty(env, flow)?.into()),
            PositionArg::Rel { md: None, .. } => Ok(Ty::Any.into()),
            PositionArg::Abs(e) => Ok(e.to_ty(env, flow)?.into()),
        }
    }
}

fn check_extract_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    c: &ExtractCall,
) -> Result<PowerTy, QuasiTypeError> {
    let _ = c.string.deref().to_ty(env, flow)?;
    check_read_args(env, flow, [arg(&c.from), opt_arg(&c.to)], "%String")
}

fn check_wextract_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    c: &WExtractCall,
) -> Result<PowerTy, QuasiTypeError> {
    let _ = c.string.deref().to_ty(env, flow)?;
    check_read_args(env, flow, [arg(&c.from), opt_arg(&c.to)], "%String")
}

fn check_list_plain_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    c: &ListPlainCall,
) -> Result<PowerTy, QuasiTypeError> {
    let list = c.list.deref().to_ranged_power_ty(env, flow)?;
    let _ = ranged_quasi_ty_to_ty(env, flow, &list, Some(&|| c.list.print()))?;
    let element = check_read_args(env, flow, [arg(&c.position), arg(&c.end)], "%String")?;
    // `$LIST(list, position)` raises <NULL VALUE> on a null element: never an undefined value.
    Ok(match (&c.end, &*list) {
        (Some(_), PowerTy::NullFreeList) => PowerTy::NullFreeList,
        (Some(_), _) => Ty::from("%List").into(),
        (None, _) => element,
    })
}

fn check_vector_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    c: &VectorCall,
) -> Result<PowerTy, QuasiTypeError> {
    check_vector_arg(env, flow, c.vector.as_ref())?;
    check_read_args(
        env,
        flow,
        [Some(c.position.deref().deref()), arg(&c.third)],
        "%String",
    )
}

/// `$PROPERTY(instance, propertyname, index...)` — the instance is optional
/// (placeholder comma keeps the current object), followed by a property name
/// and any number of multidimensional subscripts.
fn check_property_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    c: &PropertyCall,
) -> Result<PowerTy, QuasiTypeError> {
    c.instance.check(env, flow)?;
    let _ = c.property.to_ty(env, flow)?;
    for index in each_arg(&c.indexes) {
        let _ = index.to_ty(env, flow)?;
    }
    Ok(PowerTy::from(Ty::Any))
}

impl CheckExprInner for cos::BitLogicExpr {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        for atom in self.items.iter() {
            let _ = atom.operand.to_power_ty(env, flow)?;
        }
        Ok(PowerTy::from("%DataType"))
    }
}

impl CheckExprInner for cos::BitLogicOperand {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        match self {
            cos::BitLogicOperand::Paren(exp) => exp.to_power_ty(env, flow),
            cos::BitLogicOperand::Property { member, args } => {
                let obj = env.this_ty(flow);
                super::defined(super::check_member(
                    obj,
                    member.deref(),
                    args.as_ref(),
                    env,
                    flow,
                )?)
            }
            // An undefined operand is an empty bit string.
            cos::BitLogicOperand::Variable(var) => match var.as_local(env, flow)? {
                Left(local) => flow.handle_get(local),
                Right(ty) => Ok(ty),
            },
            cos::BitLogicOperand::Empty(_) => Ok(PowerTy::from("%DataType")),
        }
    }
}

/// Type-check the (present) arguments of a read-context function and return a
/// fixed result type. Omitted trailing slots are `None` and skipped.
// Table-driven (see `cos::SIMPLE_FUNCTIONS`): every argument is read, then the count is checked
// so an extra argument is named rather than left as a parse error.
fn check_simple_function<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    call: &cos::SimpleFunctionCall,
) -> Result<PowerTy, QuasiTypeError> {
    let function = cos::simple_function(&call.name.print()).expect("validated by the grammar");
    for (i, arg) in call.args.items(env).enumerate() {
        let Some(expr) = arg.deref() else { continue };
        let expr = Ranged::new(arg.start, expr, arg.end);
        // `$ZCONVERT(s, mode, table, handle)`: the handle is state the call writes, and IRIS
        // takes nothing but a plain local there (measured: a subscript, a global or a literal is
        // a compile error).
        if function.names[0] == "ZConvert" && i == 3 {
            if let Some(variable) = expr.as_variable()
                && let Left(local) = variable.as_local(env, flow)?
                && local.subs.is_empty()
            {
                flow.invalidate(local, Ty::from("%String").into())?;
                continue;
            }
            return Err(TypeErrorKind::HandleNotALocal.at(env.range(&expr)).into());
        }
        let _ = expr.to_ty(env, flow)?;
    }
    let given = call.given();
    if given > function.max {
        let name = call.name.print().to_uppercase();
        flow.add_soft_diagnostic(
            TypeErrorKind::UndocumentedArgument {
                function: name,
                documented: function.max,
                given,
            }
            .into(),
        );
    }
    // `$LISTFROMSTRING("a,,b")` makes an empty-string element of the empty piece, not a null one.
    if function.names[0] == "ListFromString" {
        return Ok(PowerTy::NullFreeList);
    }
    Ok(Ty::from(function.ret).into())
}

fn check_read_args<'a, 'b, T: CheckExprInner + Grammar>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    args: impl IntoIterator<Item = Option<&'b Ranged<T>>>,
    ret: &str,
) -> Result<PowerTy, QuasiTypeError> {
    for arg in args.into_iter().flatten() {
        let _ = arg.to_ty(env, flow)?;
    }
    Ok(Ty::from(ret).into())
}

fn check_global_ref<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    g: &cos::GlobalReference,
) -> Result<(), QuasiTypeError> {
    for sub in g.subs.iter().flat_map(|subs| subs.items(env)) {
        let _ = sub.to_ty(env, flow)?;
    }
    Ok(())
}

fn check_global_arg<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    g: &AllowIndirection<cos::GlobalReference>,
) -> Result<(), QuasiTypeError> {
    match g {
        AllowIndirection::Plain(g) => check_global_ref(env, flow, g),
        AllowIndirection::Indirection(indirection) => {
            let _ = indirection.to_ty(env, flow)?;
            Ok(())
        }
    }
}

fn check_out_var<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    var: Option<&Ranged<Variable>>,
    ty: &str,
) -> Result<(), QuasiTypeError> {
    if let Some(var) = var
        && let Left(local) = var.as_local(env, flow)?
    {
        flow.invalidate(local, Ty::from(ty).into())?;
    }
    Ok(())
}

/// `$LISTNEXT(list, ptr, value)` iterates a list. `ptr` is read (IRIS raises `<FUNCTION>` on an
/// undefined or empty pointer), then becomes a defined integer. A false call leaves `value` as it
/// was; a true one defines it, unless the element is null, which kills it.
fn check_listnext<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    list: &Ranged<Expr>,
    ptr: &Ranged<Variable>,
    value: &Ranged<Variable>,
) -> Result<PowerTy, QuasiTypeError> {
    let list_ty = list.to_ranged_power_ty(env, flow)?;
    let _ = ranged_quasi_ty_to_ty(env, flow, &list_ty, Some(&|| list.print()))?;
    let _ = ptr.to_ty(env, flow)?;
    check_out_var(env, flow, Some(ptr), "%Integer")?;
    if let Left(local) = value.as_local(env, flow)? {
        let range = env.range(value);
        if *list_ty == PowerTy::NullFreeList {
            let cases = |old_ty| vec![("0", old_ty)];
            let value = Some(Some(Ty::from("%String")));
            return bind_out_var(env, flow, local, range, cases, value);
        }
        flow.handle_get(local.clone())?;
        flow.handle_set(local, MaybeTy::Maybe(Ty::from("%String")).into(), range)?;
    }
    Ok(Ty::from("%Boolean").into())
}

// `local` is set by the call whose value `var` binds: in each of `cases` of that value it is of the
// type given (`None` undefined; `old_ty` the type it had), `default` in any other case.
fn bind_out_var<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    local: Local,
    range: bindings::Range,
    cases: impl FnOnce(Option<Ty>) -> Vec<(&'static str, Option<Ty>)>,
    default: Option<Option<Ty>>,
) -> Result<PowerTy, QuasiTypeError> {
    let old_ty = flow.handle_get(local.clone())?.to_ty(env).ok();
    let var = flow.fresh();
    flow.handle_set(local, PowerTy::case(var, cases(old_ty), default), range)?;
    Ok(PowerTy::bind(var))
}

fn target_as_local<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    target: &Ranged<GetTarget>,
) -> Result<Either<Local, PowerTy>, QuasiTypeError> {
    match target.deref() {
        GetTarget::Var(var) => var.as_local(env, flow),
        other => {
            let _ = other.to_power_ty(env, flow)?;
            Ok(Right(Ty::Any.into()))
        }
    }
}

fn check_increment<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    counter: &Ranged<GetTarget>,
    amount: Option<&Ranged<Expr>>,
) -> Result<PowerTy, QuasiTypeError> {
    check_read_args(env, flow, [amount], "%Integer")?;
    if let Left(local) = target_as_local(env, flow, counter)? {
        // $INCREMENT reads the counter's current value before writing the incremented one.
        flow.handle_get(local.clone())?;
        flow.handle_set(local, Ty::from("%Integer").into(), env.range(counter))?;
    }
    Ok(Ty::from("%Integer").into())
}

impl CheckCommandInner for Option<Expr> {
    fn inner_check<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<(), QuasiTypeError> {
        if let Some(this) = self {
            let _ = this.to_ty(env, flow)?;
        }
        Ok(())
    }
}

pub(super) fn check_args_any<'a, 'b>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    args: impl Iterator<Item = &'b Ranged<Option<cos::MthArg>>>,
) -> Result<Ty, TypeError> {
    for arg in args {
        match arg.deref() {
            Some(cos::MthArg::Expr { expr, .. }) => {
                expr.to_ty(env, flow)?;
            }
            // An unknown callee may set anything through a by-reference argument, subscripts
            // included.
            Some(cos::MthArg::ByRef { target, .. }) => {
                let range = env.range(arg);
                check_dottable(env, target, range);
                let at = |err: QuasiTypeError| err.into_error(|| range);
                let local = target.as_variable().map(|v| v.as_local(env, flow));
                match local.transpose().map_err(at)? {
                    Some(Left(local)) => flow.invalidate(local, Ty::Any.into()).map_err(at)?,
                    _ => check_set_target(target, Ty::Any.into(), range, env, flow).map_err(at)?,
                }
            }
            None => {}
        }
    }
    Ok(Ty::Any)
}

/// Check `$ZUTIL`/`$ZU`: validate every argument and return `Any`, since these
/// legacy utility callouts are variadic and their return types are not modeled.
// Every `$ZUTIL` function is legacy: the replacement list names what to use instead, and a call
// it doesn't cover is undocumented. Both are judged from the leading literal arguments.
fn check_zu_args<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    kw: &Ranged<cos::FnZUtil>,
    args: &FunArgList,
) -> Result<PowerTy, QuasiTypeError> {
    let literals: Vec<u32> = args
        .items
        .iter()
        .map_while(|arg| match arg.deref() {
            Some(cos::FunArg::Normal { expr, .. }) => expr.print().trim().parse().ok(),
            _ => None,
        })
        .collect();
    // Not for a `$ZU` a macro expanded to, such as the system macros IRIS itself defines over it.
    if let Some(&first) = literals.first()
        && matches!(
            env.fidelity(kw),
            Some(Fidelity::Exact | Fidelity::AtBoundary)
        )
    {
        let kind = match super::zutil::lookup(&literals, args.items.len()) {
            Some(legacy) => {
                let call = legacy.args.iter().map(u32::to_string).collect::<Vec<_>>();
                TypeErrorKind::LegacyZUtil {
                    call: format!("$ZU({})", call.join(",")),
                    replacement: Replacement(legacy.replacement.map(str::to_string)),
                }
            }
            None => TypeErrorKind::UndocumentedZUtil {
                call: format!("$ZU({first})"),
            },
        };
        env.add_soft_error(kind.into(), env.range(kw));
    }
    for arg in args.items(env) {
        let Some(fun_arg) = arg.deref() else { continue };
        match fun_arg {
            cos::FunArg::ColonNormal(ec) => {
                let _ = ec.e1.to_ty(env, flow)?;
                let _ = ec.e2.to_ty(env, flow)?;
            }
            cos::FunArg::ColonDefault(e) => {
                let _ = e.to_ty(env, flow)?;
            }
            cos::FunArg::Normal { expr, .. } => {
                let _ = arg.as_ref().map(|_| expr).to_ty(env, flow)?;
            }
            cos::FunArg::Star { md, .. } => {
                if let Some(md) = md {
                    let _ = arg.as_ref().map(|_| &md.e).to_ty(env, flow)?;
                }
            }
            cos::FunArg::ByRef { target, .. } => {
                check_dottable(env, target, env.range(arg));
                check_set_target(target, PowerTy::from(Ty::Any), env.range(arg), env, flow)?;
            }
        }
    }
    Ok(Ty::Any.into())
}

fn check_compile<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    CompileArgs {
        source,
        qspec,
        errors,
        object,
        arg5,
        arg6,
        rname,
        ..
    }: &CompileArgs,
) -> Result<PowerTy, QuasiTypeError> {
    // The array is handed over by reference, as `.source` would be; named by indirection, any may be.
    match source.as_local(env, flow)? {
        Left(source) => flow.invalidate(source, Ty::Any.into())?,
        Right(_) => flow.invalidate_all()?,
    }
    let _ = qspec.to_ty(env, flow)?;
    if let Some(ranged) = errors.as_ref().map(Option::as_ref).transpose()
        && let Left(local) = ranged.as_local(env, flow)?
    {
        flow.invalidate(local, Ty::from("%String").into())?;
    }
    if let Some(ranged) = arg(object).and_then(|o| o.as_ref().map(Option::as_ref).transpose()) {
        match ranged.as_local(env, flow)? {
            Left(local) => flow.invalidate(local, Ty::from("%String").into())?,
            Right(_) => flow.invalidate_all()?,
        }
    }
    for arg in [arg5, arg6, rname].into_iter().flatten() {
        arg.check(env, flow)?;
    }
    Ok(Ty::from("%Integer").into())
}

// What takes a branch of `$SELECT` or `$CASE`.
enum Taken<'e> {
    If(&'e Ranged<Expr>),
    Equal(&'e PowerTy, &'e Ranged<Expr>),
    Always,
}

// The type of the branches' values, each typed where its branch is taken, its condition true
// and the earlier ones false, as `If` does, so a `$DATA(x)` guards the `x` of its branch; `flow`
// goes on from the branches taken, as the path where none is ends in <SELECT> or
// <ILLEGAL VALUE>. Also whether a condition always holds.
fn check_branches<'a, 'e>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    branches: impl IntoIterator<Item = (Taken<'e>, &'e Ranged<Expr>)>,
) -> Result<(Ty, bool), QuasiTypeError> {
    let mut exhaustive = false;
    let mut ty = None;
    let mut taken = vec![];
    let mut values = None;
    let mut covered = vec![];
    for (condition, value) in branches {
        let condition = match condition {
            Taken::If(condition) => {
                let ty = condition.to_ranged_power_ty(env, flow)?;
                // A condition is a read like any other, then what the flow splits on.
                ranged_quasi_ty_to_ty(env, flow, &ty, Some(&|| condition.print()))
                    .map_err(QuasiTypeError::WithRange)?;
                ty.into_inner()
            }
            Taken::Equal(target, this) => {
                let this = this.to_power_ty(env, flow)?;
                values.get_or_insert_with(|| values_of(env, target));
                covered.extend(consts(&this).into_iter().flatten().cloned());
                related(target, TermOp::Eq, &this).unwrap_or_else(|| PowerTy::from("%Boolean"))
            }
            Taken::Always => PowerTy::Const(Const::from(true)),
        };
        if let PowerTy::Const(c) = &condition {
            exhaustive |= c.truthy();
        }
        let if_false = flow.split_on(condition, false);
        let mut if_true = std::mem::replace(flow, if_false);
        if if_true.is_live() {
            let value = value.to_ty(env, &mut if_true)?;
            ty = Some(match ty {
                Some(x) => env.merge_ty(x, value),
                None => value,
            });
        }
        taken.push(if_true);
    }
    // Every value the target can hold is listed: no path falls through.
    if let Some(Some(values)) = values {
        exhaustive |= values
            .iter()
            .all(|v| covered.iter().any(|c| c.same_value(v)));
    }
    flow.handle_void()?;
    for branch in taken {
        flow.merge(branch)?;
    }
    Ok((ty.unwrap_or(Ty::Any), exhaustive))
}

// The values a `$CASE` target can hold, when known: the constants it is one of, or the domain of
// its declared datatype.
fn values_of(env: &CodeEnv<'_>, target: &PowerTy) -> Option<Vec<Const>> {
    match consts(target) {
        Some(cs) => Some(cs.into_iter().cloned().collect()),
        None => env.domain(&target.to_ty(env).ok()?),
    }
}

// A true `$ISOBJECT(x)` / `$ISOBJECT($GET(x))` proves `x` holds an object, hence is defined.
fn check_is_object<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    expr: &Ranged<Expr>,
) -> Result<PowerTy, QuasiTypeError> {
    let ty = check_read_args(env, flow, [Some(expr)], "%Boolean")?;
    if let Some(variable) = expr.as_variable().or_else(|| get_of_variable(expr))
        && let Left(local) = variable.as_local(env, flow)?
        && local.subs.is_empty()
    {
        let old_ty = flow.handle_get(local.clone())?.to_ty(env).ok();
        // A value already known to be of an object class keeps that (more precise) class.
        let object_ty = match &old_ty {
            Some(ty @ Ty::OREF(cls)) if env.is_datatype(cls.name()) == Some(false) => ty.clone(),
            _ => Ty::Any,
        };
        let var = flow.fresh();
        let cases = vec![("0", old_ty), ("1", Some(object_ty))];
        flow.handle_refine(local, PowerTy::case(var, cases, None))?;
        return Ok(PowerTy::bind(var));
    }
    Ok(ty)
}

fn get_of_variable(expr: &Expr) -> Option<&Variable> {
    if let AtomicExpr::FunctionCall(call) = expr.as_atomic()?
        && let FunctionCall::Get { args, .. } = call.deref()
        && let GetTarget::Var(variable) = args.variable.deref().deref()
    {
        return Some(variable);
    }
    None
}

fn check_get<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    target: &Ranged<GetTarget>,
    default: Option<&Ranged<Expr>>,
) -> Result<PowerTy, QuasiTypeError> {
    let default = if let Some(default) = default {
        default.to_ty(env, flow)?
    } else {
        Ty::from("%String")
    };
    match target.deref() {
        GetTarget::Var(target) => match target.as_local(env, flow)? {
            Left(local) => {
                let ty = flow.handle_get(local)?.to_maybe_ty(env);
                Ok(match ty.maybe_defined() {
                    Some(defined) if ty.maybe_undefined() => env.merge_ty(defined.clone(), default),
                    Some(defined) => defined.clone(),
                    None => default,
                }
                .into())
            }
            Right(_) => Ok(PowerTy::from("%DataType")),
        },
        // `Member`/`Indirection`/`Sub` are never flow-tracked locals -- read the plain type.
        target => target.to_power_ty(env, flow),
    }
}

pub(super) fn check_number(number: &Number) -> Result<PowerTy, QuasiTypeError> {
    match number {
        // Too big for a constant is still a number, just not a known one.
        Number::FromDigit(numeric, None, None) => Ok(str::parse(&numeric.print())
            .map_or(PowerTy::from("%DataType"), |n| {
                PowerTy::Const(types::Const::Integer(n))
            })),
        _ => Ok(PowerTy::from("%DataType")),
    }
}

// `power_ty` used somewhere a string is expected (e.g. `$ZSTRIP`'s `action`, which the docs
// describe as "specified as a quoted string"). A constant integer coerces to its canonical
// string form at runtime like any other number-to-string conversion, but a bare number in one
// of these enumerated-string arguments is rarely intentional, so it's worth a warning even
// though it isn't wrong.
fn const_as_string(power_ty: &PowerTy, flow: &Flow<'_>) -> Option<String> {
    let PowerTy::Const(c) = power_ty else {
        return None;
    };
    if matches!(c, types::Const::Integer(_)) {
        flow.add_soft_diagnostic(
            TypeErrorKind::NumberImplicitlyConvertedToString {
                value: c.as_string(),
            }
            .into(),
        );
    }
    Some(c.as_string())
}

// An argument with its own little syntax (`$ZSTRIP`'s action, `$FNUMBER`'s format): a constant
// is validated by `check`, and a failure reported through `invalid`.
fn check_code_arg<'a, R>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    code: &Ranged<Expr>,
    check: impl FnOnce(&str) -> Result<(), R>,
    invalid: impl FnOnce(String, R) -> TypeErrorKind,
) -> Result<Ty, QuasiTypeError> {
    flow.with_range(env.range(code), |flow| {
        let ty = code.to_power_ty(env, flow)?;
        if let Some(text) = const_as_string(&ty, flow)
            && let Err(reason) = check(&text)
        {
            return Err(invalid(text, reason).into());
        }
        ty.to_ty(env)
    })
}

// `$ZSTRIP`'s `action` is an action code (`*`, or a subset of `<`/`>`/`=`) followed by zero or
// more mask codes (`E`/`A`/`P`/`C`/`N`/`L`/`U`/`W`, each optionally negated with a leading `'`,
// with every non-negated one preceding every negated one) -- see
// https://docs.intersystems.com/irislatest/csp/docbook/DocBook.UI.Page.cls?KEY=RCOS_fzstrip
fn check_zstrip_action(action: &str) -> Result<(), &'static str> {
    let rest = if let Some(rest) = action.strip_prefix('*') {
        rest
    } else {
        let mut rest = action;
        let mut seen = std::collections::HashSet::new();
        while let Some(c) = rest.chars().next()
            && matches!(c, '<' | '>' | '=')
            && seen.insert(c)
        {
            rest = &rest[1..];
        }
        rest
    };
    if !rest.is_empty() && rest.len() == action.len() {
        return Err("mask codes must follow an action code (*, <, > or =)");
    }
    let mut has_negated = false;
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        let negated = c == '\'';
        let letter = if negated {
            chars
                .next()
                .ok_or("a Unary Not (') must be followed by a mask code")?
        } else {
            c
        };
        if !"EAPCNLUW".contains(letter.to_ascii_uppercase()) {
            return Err("mask codes must be one of E, A, P, C, N, L, U, W");
        }
        if negated {
            has_negated = true;
        } else if has_negated {
            return Err("mask codes without a Unary Not (') must all precede the ones with one");
        }
    }
    Ok(())
}

// `$FNUMBER`'s `format` is zero or more single-character codes (`+ - , . D E G L N O P T`, the
// letters case-insensitive), usable singly or in any combination -- except for the pairs IRIS
// documents as producing a real `<SYNTAX>`/`<FUNCTION>` error when combined. See
// https://docs.intersystems.com/irislatest/csp/docbook/DocBook.UI.Page.cls?KEY=RCOS_ffnumber
fn check_fnumber_format(format: &str) -> Result<(), String> {
    const INCOMPATIBLE: &[(char, char)] = &[
        ('-', 'p'),
        (',', '.'),
        (',', 'n'),
        ('.', 'o'),
        ('l', 'p'),
        ('l', 't'),
        ('p', 't'),
        ('p', '+'),
        ('e', 'g'),
    ];
    let mut seen = Vec::new();
    for c in format.chars() {
        let code = c.to_ascii_lowercase();
        if !"+-,.deglnopt".contains(code) {
            return Err(format!("'{c}' is not a recognized format code"));
        }
        if let Some(&(a, b)) = INCOMPATIBLE
            .iter()
            .find(|&&(a, b)| (code == a && seen.contains(&b)) || (code == b && seen.contains(&a)))
        {
            return Err(format!(
                "'{}' and '{}' cannot be combined",
                a.to_ascii_uppercase(),
                b.to_ascii_uppercase()
            ));
        }
        seen.push(code);
    }
    Ok(())
}

fn check_data<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    variable: &Ranged<GetTarget>,
    target: Option<&Ranged<Variable>>,
) -> Result<PowerTy, QuasiTypeError> {
    let variable = match target_as_local(env, flow, variable)? {
        Left(local) if local.subs.is_empty() => local,
        // Individual subscripts aren't tracked, so a subscripted local is as opaque as a glvn --
        // though still a read of the variable.
        Left(local) => {
            flow.handle_get(Local::bare(local.name))?;
            return check_data_opaque(env, flow, target);
        }
        Right(_) => return check_data_opaque(env, flow, target),
    };
    let new_ty = flow.handle_get(variable.clone())?.to_maybe_ty(env);
    let maybe_subscript = flow.subscriptable(&variable.name)?;
    let var = flow.fresh();
    let mut cases = vec![];
    for (undefined, defined) in once(("0", "1")).chain(maybe_subscript.then_some(("10", "11"))) {
        if new_ty.maybe_undefined() {
            cases.push((undefined, None));
        }
        if let Some(ty) = new_ty.maybe_defined() {
            cases.push((defined, Some(ty.clone())));
        }
    }
    flow.handle_refine(variable, PowerTy::case(var, cases.clone(), None))?;
    if let Some(target) = target {
        let target_range = env.range(target);
        if let Left(target) = target.as_local(env, flow)? {
            let old_ty = flow.handle_get(target.clone())?.to_ty(env).ok();
            cases = cases
                .into_iter()
                .map(|(v, t)| (v, t.or_else(|| old_ty.clone())))
                .collect();
            let refined = PowerTy::case(var, cases.clone(), None);
            if refined.to_maybe_ty(env).maybe_defined().is_some() {
                flow.handle_set(target, refined, target_range)?;
            }
        }
    }
    Ok(match cases.as_slice() {
        [(value, _)] => PowerTy::Const(types::Const::String(value.to_string())),
        _ => PowerTy::bind(var),
    })
}

// `$DATA` of something nothing is known about: the value is only tied to `target` (when there
// is one) so conditions on it refine the target's definedness.
fn check_data_opaque<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    target: Option<&Ranged<Variable>>,
) -> Result<PowerTy, QuasiTypeError> {
    if let Some(target) = target
        && let Left(local) = target.as_local(env, flow)?
    {
        let cases = |old_ty: Option<Ty>| {
            vec![
                ("0", old_ty.clone()),
                ("1", Some(Ty::Any)),
                ("10", old_ty),
                ("11", Some(Ty::Any)),
            ]
        };
        return bind_out_var(env, flow, local, env.range(target), cases, None);
    }
    Ok(PowerTy::union(vec![
        0.into(),
        1.into(),
        10.into(),
        11.into(),
    ]))
}

fn check_order<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    variable: &Ranged<GetTarget>,
    direction: Option<&Ranged<Expr>>,
    target: Option<&Ranged<Variable>>,
) -> Result<PowerTy, QuasiTypeError> {
    require_public(env, flow, variable.deref(), "$ORDER", false)?;
    let new_ty = variable
        .to_maybe_ty(env, flow)?
        .maybe_defined()
        .cloned()
        .unwrap_or_default();
    check_read_args(env, flow, [direction], "%DataType")?;
    if let Some(target) = target
        && let Left(local) = target.as_local(env, flow)?
    {
        let target_range = env.range(target);
        let cases = |old_ty| vec![("", old_ty)];
        return bind_out_var(env, flow, local, target_range, cases, Some(Some(new_ty)));
    }
    Ok(PowerTy::from("%DataType"))
}
