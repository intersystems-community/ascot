use super::*;

// An argument that may be left empty (`$FN(x,,0)`): the expression inside, if any.
fn opt_arg<'b, M, W, T>(arg: &'b Option<M>) -> Option<&'b Range<T>>
where
    M: Deref<Target = W>,
    W: Deref<Target = Option<Range<T>>> + 'b,
{
    arg.as_deref().and_then(|w| w.deref().as_ref())
}

impl CheckExprInner for cos::FunctionCall {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        match self {
            FunctionCall::Length {
                args:
                    LengthArgs {
                        expression,
                        delimitor,
                    },
                ..
            } => check_length(
                env,
                flow,
                expression.deref(),
                delimitor.as_deref().map(|v| &**v),
            ),
            FunctionCall::Simple(call) => check_simple_function(env, flow, call),
            FunctionCall::Generic(_) => unreachable!("never converts"),
            FunctionCall::Unknown(call) => {
                for arg in call.args.items.iter() {
                    if let Some(cos::FunArg::Normal { expr, .. }) = arg.deref() {
                        let _ = expr.to_ty(env, flow)?;
                    }
                }
                let name = call.name.print();
                let names = cos::FUNCTION_NAMES.iter().copied().chain(
                    cos::SIMPLE_FUNCTIONS
                        .iter()
                        .flat_map(|f| f.names.iter().copied()),
                );
                let suggestions = crate::check::top_similar(&name, names, 5);
                Err(QuasiTypeError::unknown_function(name, suggestions))
            }
            FunctionCall::Data {
                args: DataArgs { variable, target },
                ..
            } => check_data(env, flow, variable.deref(), target.as_deref().map(|v| &**v)),
            FunctionCall::Order {
                args:
                    OrderArgs {
                        variable,
                        direction,
                        target,
                    },
                ..
            } => check_order(
                env,
                flow,
                variable.deref(),
                direction.as_deref().map(|v| &**v),
                target.as_deref().map(|v| &**v),
            ),
            FunctionCall::Get {
                args: GetArgs { variable, default },
                ..
            } => check_get(
                env,
                flow,
                variable.deref(),
                default.as_deref().map(|v| &**v),
            ),
            FunctionCall::DText(dtext) => {
                match &dtext.arg {
                    cos::DTextArg::Label(AllowIndirection::Plain(label)) => {
                        if let Some(offset) = label.offset.as_deref() {
                            let _ = offset.to_ty(env, flow)?;
                        }
                    }
                    cos::DTextArg::Offset(offset, _) => {
                        let _ = offset.deref().to_ty(env, flow)?;
                    }
                    cos::DTextArg::Label(AllowIndirection::Indirection(_)) => {}
                }
                Ok(Ty::from("%String").into())
            }
            FunctionCall::ZP {
                args: ZPArgs { variable },
                ..
            } => {
                let _ = variable.to_maybe_ty(env, flow)?;
                Ok(PowerTy::from("%DataType"))
            }
            // Internal storage handle -- opaque, like `$ZUTIL`'s catch-all return.
            FunctionCall::ZObjRef {
                args: ZObjRefArgs { oref },
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
                    },
                ..
            } => {
                string.deref().to_ty(env, flow)?;
                flow.with_range(env.range(action), |flow| {
                    let action = action.to_power_ty(env, flow)?;
                    if let Some(text) = const_as_string(&action, flow)
                        && let Err(reason) = check_zstrip_action(&text)
                    {
                        return Err(QuasiTypeError::invalid_zstrip_action(text, reason));
                    }
                    action.to_ty(env)
                })?;
                if let Some(remchar) = remchar {
                    remchar.check(env, flow)?
                }
                if let Some(keepchar) = keepchar {
                    keepchar.check(env, flow)?
                }
                Ok(Ty::from("%String").into())
            }
            FunctionCall::ZDateTime {
                args: ZDateTimeArgs { value, rest },
                ..
            }
            | FunctionCall::ZDate {
                args: ZDateArgs { value, rest },
                ..
            }
            | FunctionCall::ZTime {
                args: ZTimeArgs { value, rest },
                ..
            } => {
                let _ = value.to_ty(env, flow)?;
                for arg in rest {
                    arg.check(env, flow)?;
                }
                Ok(Ty::from("%String").into())
            }
            FunctionCall::ZFm1 { program, .. } | FunctionCall::ZFm2 { program, .. } => {
                check_read_args(env, flow, [program.as_deref().map(|v| &**v)], "%Integer")
            }
            FunctionCall::ZFm100 {
                flags,
                program,
                args,
                ..
            } => {
                let _ = flags.to_ty(env, flow)?;
                let _ = program.to_ty(env, flow)?;
                check_args_any(env, flow, args.iter().map(|v| &***v))?;
                Ok(Ty::from("%Integer").into())
            }
            FunctionCall::ZFm3 { dll, args, .. } => {
                let _ = dll.to_ty(env, flow)?;
                Ok(check_args_any(env, flow, args.iter().map(|v| &***v))?.into())
            }
            FunctionCall::ZFm4 {
                n, third, fourth, ..
            } => check_read_args(
                env,
                flow,
                [
                    Some(n.deref().deref()),
                    Some(third.deref().deref()),
                    fourth.as_deref().map(|v| &**v),
                ],
                "%Integer",
            ),
            FunctionCall::ZFm5 {
                dll_id,
                func_id,
                args,
                ..
            } => {
                let _ = dll_id.to_ty(env, flow)?;
                let _ = func_id.to_ty(env, flow)?;
                Ok(check_args_any(env, flow, args.iter().map(|v| &***v))?.into())
            }
            FunctionCall::ZFm6 {
                dll_index, args, ..
            } => {
                let _ = dll_index.to_ty(env, flow)?;
                Ok(check_args_any(env, flow, args.iter().map(|v| &***v))?.into())
            }
            FunctionCall::ZU { args, .. } => check_zu_args(env, flow, args),
            FunctionCall::Case {
                args: CaseArgs { target, arms },
                ..
            } => {
                let target = target.deref();
                let mut branches = Vec::new();
                let mut default = None;
                for arm in arms {
                    if default.is_some() {
                        return Err(QuasiTypeError::case_default_not_last());
                    }
                    match arm.deref().deref().deref() {
                        CaseArm::Branch(branch) => branches.push((&branch.e1, &branch.e2)),
                        CaseArm::Default(value) => default = Some(value.deref()),
                    }
                }
                Ok(check_case(env, flow, target, branches, default)?.into())
            }
            FunctionCall::Bit(c) => check_bit_call(env, flow, c),
            FunctionCall::BitCount {
                args: BitCountArgs { bits, value },
                ..
            } => check_bitcount(env, flow, bits.deref(), value.as_deref().map(|v| &**v)),
            FunctionCall::BitFind {
                args:
                    BitFindArgs {
                        bits,
                        value,
                        position,
                        direction,
                    },
                ..
            } => check_bitfind(
                env,
                flow,
                bits.deref(),
                value.deref().deref(),
                position.as_deref().map(|v| &**v),
                direction.as_deref().map(|v| &**v),
            ),
            FunctionCall::BitLogic { expr, length, .. } => {
                check_bitlogic(env, flow, expr.deref(), length.as_deref().map(|v| &**v))
            }
            FunctionCall::Change {
                args:
                    ChangeArgs {
                        string,
                        target,
                        replacement,
                        occurrences,
                        begin,
                    },
                ..
            } => check_change(
                env,
                flow,
                string.deref(),
                target.deref().deref(),
                replacement.deref().deref(),
                occurrences.as_deref().map(|v| &**v),
                begin.as_deref().map(|v| &**v),
            ),
            FunctionCall::Char {
                args: CharArgs { first, rest },
                ..
            } => check_char(env, flow, first.deref(), rest.iter().map(|v| &***v)),
            FunctionCall::ClassName {
                args: ClassNameArgs { oref },
                ..
            } => check_classname(env, flow, oref.as_deref()),
            FunctionCall::Decimal {
                args: DecimalArgs { num, digits },
                ..
            } => check_decimal(env, flow, num.deref(), digits.as_deref().map(|v| &**v)),
            FunctionCall::Double {
                args: DoubleArgs { num },
                ..
            } => check_double(env, flow, num.deref()),
            FunctionCall::Extract(c) => check_extract_call(env, flow, c),
            FunctionCall::FNumber {
                args:
                    FNumberArgs {
                        num,
                        format,
                        decimal,
                    },
                ..
            } => {
                let _ = num.deref().to_ty(env, flow)?;
                if let Some(format) = opt_arg(format) {
                    flow.with_range(env.range(format), |flow| {
                        let format_ty = format.to_power_ty(env, flow)?;
                        if let Some(text) = const_as_string(&format_ty, flow)
                            && let Err(reason) = check_fnumber_format(&text)
                        {
                            return Err(QuasiTypeError::invalid_fnumber_format(text, reason));
                        }
                        format_ty.to_ty(env)
                    })?;
                }
                if let Some(decimal) = decimal {
                    let _ = decimal.to_ty(env, flow)?;
                }
                Ok(Ty::from("%String").into())
            }
            FunctionCall::Increment {
                args: IncrementArgs { counter, amount },
                ..
            } => check_increment(env, flow, counter.deref(), amount.as_deref().map(|v| &**v)),
            FunctionCall::IsObject {
                args: IsObjectArgs { expr },
                ..
            } => check_is_object(env, flow, expr),
            FunctionCall::ListRange(c) => check_list_range_call(env, flow, c),
            FunctionCall::ListPlain(c) => check_list_plain_call(env, flow, c),
            FunctionCall::ListBuild(ListBuild {
                args: ListBuildArgs { first, rest },
                ..
            }) => {
                for arg in once(first.deref()).chain(rest.iter().map(|v| &***v)) {
                    arg.check(env, flow)?;
                }
                Ok(Ty::from("%List").into())
            }
            FunctionCall::ListData {
                args:
                    ListDataArgs {
                        list,
                        position,
                        var,
                    },
                ..
            } => {
                check_read_args(
                    env,
                    flow,
                    [Some(list.deref()), position.as_deref().map(|v| &**v)],
                    "%Boolean",
                )?;
                check_out_var(env, flow, var.as_deref().map(|v| &**v), "%String")?;
                Ok(Ty::from("%Boolean").into())
            }
            FunctionCall::ListGet(c) => check_listget_call(env, flow, c),
            FunctionCall::ListNext {
                args: ListNextArgs { list, ptr, value },
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
                    },
                ..
            } => {
                let _ = list.deref().to_ty(env, flow)?;
                let _ = position.deref().deref().to_ty(env, flow)?;
                for v in std::iter::once(value.deref().deref()).chain(rest.iter().map(|v| &***v)) {
                    if let Some(cond) = &v.cond {
                        let _ = cond.deref().to_ty(env, flow)?;
                    }
                    let _ = (&v.value).to_ty(env, flow)?;
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
                    },
                ..
            } => {
                check_read_args(
                    env,
                    flow,
                    [
                        Some(string.deref()),
                        Some(regexp.deref().deref()),
                        start.as_deref().map(|v| &**v),
                    ],
                    "%Integer",
                )?;
                check_out_var(env, flow, end.as_deref().map(|v| &**v), "%Integer")?;
                check_out_var(env, flow, val.as_deref().map(|v| &**v), "%String")?;
                Ok(Ty::from("%Integer").into())
            }
            FunctionCall::MV(c) => check_mv_call(env, flow, c),
            FunctionCall::Method {
                args:
                    MethodArgs {
                        instance,
                        method,
                        args,
                    },
                ..
            } => Ok(check_classmethod(
                env,
                flow,
                instance.deref(),
                method.deref().deref(),
                args.iter().map(|v| &***v),
            )?
            .into()),
            FunctionCall::Name {
                args: NameArgs { variable, integer },
                ..
            } => {
                let _ = variable.deref().to_ty(env, flow)?;
                check_read_args(env, flow, [integer.as_deref().map(|v| &**v)], "%String")
            }
            FunctionCall::Parameter {
                args: ParameterArgs { class, parameter },
                ..
            } => {
                class.check(env, flow)?;
                check_read_args(env, flow, [Some(parameter.deref().deref())], "%String")
            }
            FunctionCall::Piece(c) => check_piece_call(env, flow, c),
            FunctionCall::PrefetchOff {
                args: PrefetchOffArgs { first, second },
                ..
            } => {
                for g in first
                    .as_deref()
                    .into_iter()
                    .chain(second.as_deref().map(|v| &**v))
                {
                    check_global_ref(env, flow, g)?;
                }
                Ok(Ty::from("%String").into())
            }
            FunctionCall::PrefetchOn {
                args: PrefetchOnArgs { gref, gref2 },
                ..
            } => {
                for g in std::iter::once(gref.deref()).chain(gref2.as_deref().map(|v| &**v)) {
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
                    },
                ..
            } => {
                let _ = reference.deref().to_ty(env, flow)?;
                if let Some(direction) = direction.as_deref().map(|v| &**v) {
                    let _ = direction.to_ty(env, flow)?;
                }
                check_out_var(env, flow, target.as_deref().map(|v| &**v), "%String")?;
                Ok(Ty::from("%String").into())
            }
            FunctionCall::Select {
                args: SelectArgs { first, rest },
                ..
            } => {
                let mut exhaustive = false;
                for b in std::iter::once(first.deref()).chain(rest.iter().map(|v| &***v)) {
                    if let PowerTy::Const(c) = (&b.e1).to_power_ty(env, flow)? {
                        exhaustive |= c.as_integer() != 0;
                    }
                    let _ = (&b.e2).to_ty(env, flow)?;
                }
                // $SELECT throws <ILLEGAL VALUE> at runtime if no condition matches;
                // without a literal catch-all branch (`1:...`), that can't be ruled out.
                Ok(if exhaustive {
                    MaybeTy::Plain(Ty::from("%DataType"))
                } else {
                    MaybeTy::Maybe(Ty::from("%DataType"))
                }
                .into())
            }
            FunctionCall::Sequence(c) => check_sequence_call(env, flow, c),
            FunctionCall::SortBegin {
                args: SortBeginArgs { set_global },
                ..
            } => {
                check_global_ref(env, flow, set_global.deref())?;
                Ok(Ty::from("%Integer").into())
            }
            FunctionCall::SortEnd {
                args: SortEndArgs { first, dosort },
                ..
            } => {
                if let Some(Some(g)) = first.as_deref().map(|f| f.deref()) {
                    check_global_ref(env, flow, g)?;
                }
                check_read_args(env, flow, [dosort.as_deref().map(|v| &**v)], "%Integer")
            }
            FunctionCall::Vector(c) => check_vector_call(env, flow, c),
            FunctionCall::VectorDefined {
                args:
                    VectorDefinedArgs {
                        vector,
                        position,
                        value,
                    },
                ..
            } => {
                check_read_args(
                    env,
                    flow,
                    [Some(vector.deref()), Some(position.deref().deref())],
                    "%Boolean",
                )?;
                check_out_var(env, flow, value.as_deref().map(|v| &**v), "%String")?;
                Ok(Ty::from("%Boolean").into())
            }
            FunctionCall::VectorOp {
                args: VectorOpArgs { op, first, rest },
                ..
            } => check_read_args(
                env,
                flow,
                [Some(op.deref()), Some(first.deref().deref())]
                    .into_iter()
                    .chain(rest.iter().map(|v| Some(&***v))),
                "%String",
            ),
            FunctionCall::WChar {
                args: WCharArgs { first, rest },
                ..
            } => check_read_args(
                env,
                flow,
                std::iter::once(Some(first.deref())).chain(rest.iter().map(|v| Some(&***v))),
                "%String",
            ),
            FunctionCall::WExtract(c) => check_wextract_call(env, flow, c),
            FunctionCall::Xecute {
                args: XecuteArgs { code, params },
                ..
            } => check_read_args(
                env,
                flow,
                std::iter::once(Some(code.deref())).chain(params.iter().map(|v| Some(&***v))),
                "%String",
            ),
            FunctionCall::ClassMethod {
                args:
                    ClassMethodArgs {
                        classname,
                        method,
                        args,
                    },
                ..
            } => Ok(check_classmethod(
                env,
                flow,
                classname.deref(),
                method.deref().deref(),
                args.iter().map(|v| &***v),
            )?
            .into()),
            FunctionCall::Compile {
                args:
                    CompileArgs {
                        source,
                        qspec,
                        errors,
                        object,
                        arg5,
                        arg6,
                        rname,
                    },
                ..
            } => check_compile(
                env,
                flow,
                source.deref(),
                qspec.deref().deref(),
                errors.deref().deref(),
                object.as_deref().map(|v| &**v),
                arg5.as_deref().map(|v| &**v),
                arg6.as_deref().map(|v| &**v),
                rname.as_deref().map(|v| &**v),
            ),
            FunctionCall::ZOrder {
                args: ZOrderArgs { variable },
                ..
            }
            | FunctionCall::Next {
                args: NextArgs { variable },
                ..
            } => {
                let _ = variable.to_maybe_ty(env, flow)?;
                Ok(PowerTy::from("%DataType"))
            }
        }
    }
}

/// Check a [`SetTargetFun`] -- the settable functions on the left of `SET`.
/// Reuses the same per-function checks as the read positions.
// `Set $PIECE(x, ..) = v` (likewise `$EXTRACT`, `$LIST`, `$BIT`, ...) on an undefined `x` is fine:
// it starts from the empty string, so the target is defined from here on rather than read.
fn define_set_target_base<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    string: &Range<Expr>,
) -> Result<(), QuasiTypeError> {
    if let Some(variable) = string.as_variable()
        && let Left(local) = variable.as_local(env, flow)?
        && flow
            .handle_get(local.clone())?
            .to_maybe_ty(env)
            .maybe_defined()
            .is_none()
    {
        flow.handle_set(local, Ty::from("%String").into(), env.range(string))?;
    }
    Ok(())
}

pub(super) fn check_settarget_fun<'a>(
    fun: &cos::SetTargetFun,
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
) -> Result<(), QuasiTypeError> {
    use cos::SetTargetFun::*;
    match fun {
        Piece(c) => {
            define_set_target_base(env, flow, &c.string)?;
            let _ = check_piece_call(env, flow, c)?;
        }
        Extract(c) => {
            define_set_target_base(env, flow, &c.string)?;
            let _ = check_extract_call(env, flow, c)?;
        }
        List(c) => {
            define_set_target_base(env, flow, &c.list)?;
            let _ = check_list_plain_call(env, flow, c)?;
        }
        ListGet(c) => {
            define_set_target_base(env, flow, &c.list)?;
            let _ = check_listget_call(env, flow, c)?;
        }
        Bit(c) => {
            define_set_target_base(env, flow, &c.bits)?;
            let _ = check_bit_call(env, flow, c)?;
        }
        Vector(c) => {
            define_set_target_base(env, flow, &c.vector)?;
            let _ = check_vector_call(env, flow, c)?;
        }
        WExtract(c) => {
            define_set_target_base(env, flow, &c.string)?;
            let _ = check_wextract_call(env, flow, c)?;
        }
        Property(c) => {
            let _ = check_property_call(env, flow, c)?;
        }
        Sequence(c) => {
            let _ = check_sequence_call(env, flow, c)?;
        }
        ListBuild(c) => check_set_listbuild(env, flow, c)?,
        MV(c) => {
            let _ = check_mv_call(env, flow, c)?;
        }
    };
    Ok(())
}

fn check_set_listbuild<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    ListBuildForSet { first, rest, .. }: &ListBuildForSet,
) -> Result<(), QuasiTypeError> {
    for arg in once(first.deref()).chain(rest.iter().map(|rest| rest.deref().deref())) {
        let range = env.range(arg);
        if let Some(arg) = arg.deref()
            && let Left(arg) = arg.as_local(env, flow)?
        {
            flow.handle_set(arg, Ty::Any.into(), range)?
        }
    }
    Ok(())
}

fn check_sequence_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    c: &SequenceCall,
) -> Result<PowerTy, QuasiTypeError> {
    if let AllowIndirection::Plain(gvar) = c.gvar.deref().deref()
        && let Left(local) = gvar.as_local(env, flow)?
    {
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
    let _ = c.bits.to_ty(env, flow)?;
    let _ = c.position.deref().to_ty(env, flow)?;
    Ok(Ty::from("%Integer").into())
}

fn check_piece_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    c: &PieceCall,
) -> Result<PowerTy, QuasiTypeError> {
    if let Some(from) = &c.from {
        let _ = from.to_ty(env, flow)?;
    }
    if let Some(to) = opt_arg(&c.to) {
        let _ = to.to_ty(env, flow)?;
    }
    check_read_args(
        env,
        flow,
        [Some(c.string.deref()), Some(c.delimiter.deref().deref())],
        "%String",
    )
}

fn check_mv_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    c: &cos::MVCall,
) -> Result<PowerTy, QuasiTypeError> {
    let _ = c.record.deref().to_ty(env, flow)?;
    for arg in &c.rest {
        arg.deref().deref().to_ty(env, flow)?;
    }
    Ok(Ty::from("%String").into())
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
    if let Some(from) = &c.from {
        let _ = from.to_ty(env, flow)?;
    }
    if let Some(to) = opt_arg(&c.to) {
        let _ = to.to_ty(env, flow)?;
    }
    Ok(PowerTy::from("%String"))
}

fn check_wextract_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    c: &WExtractCall,
) -> Result<PowerTy, QuasiTypeError> {
    let _ = c.string.deref().to_ty(env, flow)?;
    if let Some(from) = &c.from {
        let _ = from.to_ty(env, flow)?;
    }
    if let Some(to) = opt_arg(&c.to) {
        let _ = to.to_ty(env, flow)?;
    }
    Ok(PowerTy::from("%String"))
}

fn check_list_range_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    c: &ListRangeCall,
) -> Result<PowerTy, QuasiTypeError> {
    let _ = c.list.deref().to_ty(env, flow)?;
    for pair in std::iter::once(c.first.deref()).chain(c.rest.iter().map(|v| &**v)) {
        let _ = (&pair.e1).to_ty(env, flow)?;
        let _ = (&pair.e2).to_ty(env, flow)?;
    }
    Ok(Ty::from("%String").into())
}

fn check_list_plain_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    c: &ListPlainCall,
) -> Result<PowerTy, QuasiTypeError> {
    let _ = c.list.deref().to_ty(env, flow)?;
    for arg in c.position.as_deref().into_iter().chain(c.end.as_deref()) {
        let _ = arg.deref().to_ty(env, flow)?;
    }
    Ok(Ty::from("%String").into())
}

fn check_listget_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    c: &ListGetCall,
) -> Result<PowerTy, QuasiTypeError> {
    check_read_args(
        env,
        flow,
        [Some(c.list.deref()), c.default.as_deref().map(|v| &**v)],
        "%String",
    )?;
    if let Some(position) = opt_arg(&c.position) {
        let _ = position.to_ty(env, flow)?;
    }
    Ok(PowerTy::from("%String"))
}

fn check_vector_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    c: &VectorCall,
) -> Result<PowerTy, QuasiTypeError> {
    check_read_args(
        env,
        flow,
        [
            Some(c.vector.deref()),
            Some(c.position.deref().deref()),
            c.third.as_deref().map(|v| &**v),
        ],
        "%String",
    )
}

fn check_property_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    c: &PropertyCall,
) -> Result<PowerTy, QuasiTypeError> {
    check_property(
        env,
        flow,
        c.instance.deref(),
        c.property.deref().deref(),
        c.indexes.iter().map(|v| &***v),
    )
}

fn check_bitcount<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    bits: &Range<Expr>,
    value: Option<&Range<Expr>>,
) -> Result<PowerTy, QuasiTypeError> {
    let _ = bits.to_ty(env, flow)?;
    if let Some(value) = value {
        let _ = value.to_ty(env, flow)?;
    }
    Ok(Ty::from("%Integer").into())
}

fn check_bitfind<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    bits: &Range<Expr>,
    value: &Range<Expr>,
    position: Option<&Range<Expr>>,
    direction: Option<&Range<Expr>>,
) -> Result<PowerTy, QuasiTypeError> {
    let _ = bits.to_ty(env, flow)?;
    let _ = value.to_ty(env, flow)?;
    if let Some(position) = position {
        let _ = position.to_ty(env, flow)?;
    }
    if let Some(direction) = direction {
        let _ = direction.to_ty(env, flow)?;
    }
    Ok(Ty::from("%Integer").into())
}

fn check_bitlogic<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    expr: &Range<cos::BitLogicExpr>,
    length: Option<&Range<Expr>>,
) -> Result<PowerTy, QuasiTypeError> {
    let _ = expr.to_ty(env, flow)?;
    if let Some(length) = length {
        let _ = length.to_ty(env, flow)?;
    }
    Ok(PowerTy::from("%DataType"))
}

impl CheckExprInner for cos::BitLogicExpr {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        for atom in self.items.iter() {
            let _ = atom.to_ty(env, flow)?;
        }
        Ok(PowerTy::from("%DataType"))
    }
}

impl CheckExprInner for cos::BitLogicAtom {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        Ok(self.operand.to_ty(env, flow)?.into())
    }
}

impl CheckExprInner for cos::BitLogicOperand {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        match self {
            cos::BitLogicOperand::Paren(exp) => Ok(exp.to_ty(env, flow)?.into()),
            cos::BitLogicOperand::Variable(var) => Ok(var.to_ty(env, flow)?.into()),
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
    for (i, arg) in call.args.items.iter().enumerate() {
        let Some(expr) = arg.deref() else { continue };
        let expr = Range::new(arg.start, expr, arg.end);
        // `$ZCONVERT(s, mode, table, handle)`: the handle is state the call writes.
        if function.names[0] == "ZCONVERT"
            && i == 3
            && let Some(variable) = expr.as_variable()
            && let Left(local) = variable.as_local(env, flow)?
        {
            flow.invalidate(local, Ty::from("%String").into())?;
            continue;
        }
        let _ = expr.to_ty(env, flow)?;
    }
    Ok(Ty::from(function.ret).into())
}

fn check_read_args<'a, 'b>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    args: impl IntoIterator<Item = Option<&'b Range<Expr>>>,
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
    for sub in g.subs.iter().flat_map(|subs| subs.items.iter()) {
        let _ = sub.to_ty(env, flow)?;
    }
    Ok(())
}

fn check_out_var<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    var: Option<&Range<Variable>>,
    ty: &str,
) -> Result<(), QuasiTypeError> {
    if let Some(var) = var
        && let Left(local) = var.as_local(env, flow)?
    {
        flow.invalidate(local, Ty::from(ty).into())?;
    }
    Ok(())
}

/// `$LISTNEXT(list, ptr, value)` iterates a list. `ptr` becomes a defined
/// integer unconditionally, but `value` receives an element only when the call
/// returns true — mirroring `$ORDER`'s flow-sensitive target.
fn check_listnext<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    list: &Range<Expr>,
    ptr: &Range<Variable>,
    value: &Range<Variable>,
) -> Result<PowerTy, QuasiTypeError> {
    let _ = list.to_ty(env, flow)?;
    check_out_var(env, flow, Some(ptr), "%Integer")?;
    if let Left(local) = value.as_local(env, flow)? {
        let range = env.range(value);
        let old_ty = flow.handle_get(local.clone())?.to_ty(env).ok();
        let var = flow.fresh();
        flow.handle_set(
            local,
            PowerTy::case(var, vec![("0", old_ty)], Some(Some(Ty::from("%String")))),
            range,
        )?;
        return Ok(PowerTy::bind(var));
    }
    Ok(Ty::from("%Boolean").into())
}

/// `$PROPERTY(instance, propertyname, index...)` — the instance is optional
/// (placeholder comma keeps the current object), followed by a property name
/// and any number of multidimensional subscripts.
fn check_property<'a, 'b>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    instance: &Range<Option<Expr>>,
    property: &Range<Expr>,
    indexes: impl Iterator<Item = &'b Range<Expr>>,
) -> Result<PowerTy, QuasiTypeError> {
    instance.check(env, flow)?;
    let _ = property.to_ty(env, flow)?;
    for index in indexes {
        let _ = index.to_ty(env, flow)?;
    }
    Ok(PowerTy::from(Ty::Any))
}

fn check_increment<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    counter: &Range<GetTarget>,
    amount: Option<&Range<Expr>>,
) -> Result<PowerTy, QuasiTypeError> {
    if let Some(amount) = amount {
        let _ = amount.to_ty(env, flow)?;
    }
    let local = match counter.deref() {
        GetTarget::Var(var) => var.as_local(env, flow)?,
        other => {
            let _ = other.to_power_ty(env, flow)?;
            Right(Ty::Any.into())
        }
    };
    if let Left(local) = local {
        // $INCREMENT reads the counter's current value before writing the incremented one.
        flow.handle_get(local.clone())?;
        flow.handle_set(local, Ty::from("%Integer").into(), env.range(counter))?;
    }
    Ok(Ty::from("%Integer").into())
}

fn check_change<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    string: &Range<Expr>,
    target: &Range<Expr>,
    replacement: &Range<Expr>,
    occurrences: Option<&Range<Expr>>,
    begin: Option<&Range<Expr>>,
) -> Result<PowerTy, QuasiTypeError> {
    let _ = string.to_ty(env, flow)?;
    let _ = target.to_ty(env, flow)?;
    let _ = replacement.to_ty(env, flow)?;
    if let Some(occurrences) = occurrences {
        let _ = occurrences.to_ty(env, flow)?;
    }
    if let Some(begin) = begin {
        let _ = begin.to_ty(env, flow)?;
    }
    Ok(Ty::from("%String").into())
}

fn check_char<'a, 'b>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    first: &Range<Expr>,
    rest: impl Iterator<Item = &'b Range<Expr>>,
) -> Result<PowerTy, QuasiTypeError> {
    let _ = first.to_ty(env, flow)?;
    for arg in rest {
        let _ = arg.to_ty(env, flow)?;
    }
    Ok(Ty::from("%String").into())
}

fn check_classname<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    oref: Option<&Range<Expr>>,
) -> Result<PowerTy, QuasiTypeError> {
    if let Some(oref) = oref {
        let _ = oref.to_ty(env, flow)?;
    }
    Ok(Ty::from("%String").into())
}

fn check_decimal<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    num: &Range<Expr>,
    digits: Option<&Range<Expr>>,
) -> Result<PowerTy, QuasiTypeError> {
    let _ = num.to_ty(env, flow)?;
    if let Some(digits) = digits {
        let _ = digits.to_ty(env, flow)?;
    }
    Ok(Ty::from("%Decimal").into())
}

fn check_double<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    num: &Range<Expr>,
) -> Result<PowerTy, QuasiTypeError> {
    let _ = num.to_ty(env, flow)?;
    Ok(Ty::from("%Double").into())
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
    args: impl Iterator<Item = &'b Range<Option<cos::MthArg>>>,
) -> Result<Ty, TypeError> {
    for arg in args {
        match arg.deref() {
            Some(cos::MthArg::Expr { expr, .. }) => {
                expr.to_ty(env, flow)?;
            }
            // An unknown callee may set anything through a by-reference argument, subscripts
            // included.
            Some(cos::MthArg::ByRef { target, .. }) => {
                let local = match target.as_variable() {
                    Some(variable) => variable
                        .as_local(env, flow)
                        .map_err(|err| err.into_error(|| env.range(arg)))?
                        .left(),
                    None => None,
                };
                match local {
                    Some(local) => flow
                        .invalidate(local, Ty::Any.into())
                        .map_err(|err| err.into_error(|| env.range(arg)))?,
                    None => {
                        check_set_target(target, PowerTy::from(Ty::Any), env.range(arg), env, flow)
                            .map_err(|err| err.into_error(|| env.range(arg)))?
                    }
                }
            }
            None => {}
        }
    }
    Ok(Ty::Any)
}

/// Check `$ZUTIL`/`$ZU`: validate every argument and return `Any`, since these
/// legacy utility callouts are variadic and their return types are not modeled.
fn check_zu_args<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    args: &FunArgList,
) -> Result<PowerTy, QuasiTypeError> {
    for arg in args.items.iter() {
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
                check_set_target(target, PowerTy::from(Ty::Any), env.range(arg), env, flow)?;
            }
        }
    }
    Ok(Ty::Any.into())
}

fn check_classmethod<'a, 'b>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    classname: &Range<Option<Expr>>,
    method: &Range<Expr>,
    args: impl Iterator<Item = &'b Range<Option<cos::MthArg>>>,
) -> Result<Ty, QuasiTypeError> {
    classname.check(env, flow)?;
    let _ = method.to_ty(env, flow)?;
    Ok(check_args_any(env, flow, args)?)
}

#[allow(clippy::too_many_arguments)]
fn check_compile<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    source: &Range<Variable>,
    qspec: &Range<Expr>,
    errors: &Range<Option<Variable>>,
    object: Option<&Range<Option<Variable>>>,
    arg5: Option<&Range<Option<Expr>>>,
    arg6: Option<&Range<Option<Expr>>>,
    rname: Option<&Range<Option<Expr>>>,
) -> Result<PowerTy, QuasiTypeError> {
    if let Left(source) = source.as_local(env, flow)? {
        flow.handle_merge(source)?;
    }
    let _ = qspec.to_ty(env, flow)?;
    for output in std::iter::once(errors).chain(object) {
        if let Some(ranged) = output.as_ref().map(Option::as_ref).transpose()
            && let Left(local) = ranged.as_local(env, flow)?
        {
            flow.invalidate(local, Ty::from("%String").into())?;
        }
    }
    for arg in [arg5, arg6, rname].into_iter().flatten() {
        arg.check(env, flow)?;
    }
    Ok(Ty::from("%Integer").into())
}

fn check_case<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    target: &Range<Expr>,
    branches: Vec<(&Range<Expr>, &Range<Expr>)>,
    default: Option<&Range<Expr>>,
) -> Result<MaybeTy, QuasiTypeError> {
    // todo: make $CASE flow-sensitive by understanding quasity.
    let _ = target.to_ty(env, flow)?;
    let mut tys = vec![];
    for (cnd, thn) in branches {
        let _ = cnd.to_ty(env, flow)?;
        tys.push(thn.to_ty(env, flow)?);
    }
    let definitely = if let Some(thn) = default {
        tys.push(thn.to_ty(env, flow)?);
        true
    } else {
        false
    };
    if let Some(mut x) = tys.pop() {
        for y in tys.drain(..) {
            x = env.merge_ty(x, y);
        }
        if definitely {
            Ok(MaybeTy::Plain(x))
        } else {
            Ok(MaybeTy::Maybe(x))
        }
    } else {
        Ok(MaybeTy::Undefined)
    }
}

// A true `$ISOBJECT(x)` / `$ISOBJECT($GET(x))` proves `x` holds an object, hence is defined.
fn check_is_object<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    expr: &Range<Expr>,
) -> Result<PowerTy, QuasiTypeError> {
    let ty = check_read_args(env, flow, [Some(expr)], "%Boolean")?;
    let Some(variable) = expr.as_variable().or_else(|| get_of_variable(expr)) else {
        return Ok(ty);
    };
    let Left(local) = variable.as_local(env, flow)? else {
        return Ok(ty);
    };
    if !local.subs.is_empty() {
        return Ok(ty);
    }
    let old_ty = flow.handle_get(local.clone())?.to_ty(env).ok();
    // A value already known to be of an object class keeps that (more precise) class.
    let object_ty = match &old_ty {
        Some(ty @ Ty::OREF(cls)) if env.is_datatype(cls.name()) == Some(false) => ty.clone(),
        _ => Ty::Any,
    };
    let var = flow.fresh();
    let cases = vec![("0", old_ty), ("1", Some(object_ty))];
    flow.handle_refine(local, PowerTy::case(var, cases, None))?;
    Ok(PowerTy::bind(var))
}

fn get_of_variable(expr: &Expr) -> Option<&Variable> {
    if !expr.ops.is_empty() {
        return None;
    }
    let OpsExpr::Base(ExprOps::Base(this)) = expr.base.deref() else {
        return None;
    };
    let ParenedExpr::Normal(this) = this.deref() else {
        return None;
    };
    let AtomicExpr::FunctionCall(call) = this.deref().deref() else {
        return None;
    };
    let FunctionCall::Get {
        args: GetArgs { variable, .. },
        ..
    } = call.deref()
    else {
        return None;
    };
    let GetTarget::Var(variable) = variable.deref().deref() else {
        return None;
    };
    Some(variable)
}

fn check_get<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    target: &Range<GetTarget>,
    default: Option<&Range<Expr>>,
) -> Result<PowerTy, QuasiTypeError> {
    let default = if let Some(default) = default {
        default.to_ty(env, flow)?
    } else {
        Ty::from("%String")
    };
    match target.deref() {
        GetTarget::Var(target) => match target.as_local(env, flow)? {
            Left(local) => Ok(flow.handle_get(local)?.to_ty(env).unwrap_or(default).into()),
            Right(_) => Ok(PowerTy::from("%DataType")),
        },
        // `Member`/`Indirection`/`Sub` are never flow-tracked locals -- read the plain type.
        target => target.to_power_ty(env, flow),
    }
}

pub(super) fn check_number(number: &Number) -> Result<PowerTy, QuasiTypeError> {
    match number {
        Number::FromDigit(numeric, None, None) => Ok(PowerTy::Const(types::Const::Integer(
            str::parse(&numeric.print()).unwrap_or(0),
        ))),
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
        flow.add_soft_diagnostic(QuasiTypeError::number_implicitly_converted_to_string(
            c.as_string(),
        ));
    }
    Some(c.as_string())
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
    let mut has_plain = false;
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
            if !has_plain {
                return Err(
                    "a Unary Not (') mask code must follow at least one mask code without one",
                );
            }
            has_negated = true;
        } else {
            if has_negated {
                return Err(
                    "mask codes without a Unary Not (') must all precede the ones with one",
                );
            }
            has_plain = true;
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

fn check_length<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    expression: &Range<Expr>,
    delimitor: Option<&Range<Expr>>,
) -> Result<PowerTy, QuasiTypeError> {
    let _ = expression.to_ty(env, flow)?;
    if let Some(delimitor) = delimitor {
        let _ = delimitor.to_ty(env, flow)?;
    };
    Ok(Ty::from("%Integer").into())
}

fn check_data<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    variable: &Range<GetTarget>,
    target: Option<&Range<Variable>>,
) -> Result<PowerTy, QuasiTypeError> {
    let local = match variable.deref() {
        GetTarget::Var(var) => var.as_local(env, flow)?,
        other => {
            let _ = other.to_power_ty(env, flow)?;
            Right(Ty::Any.into())
        }
    };
    let Left(local) = local else {
        // Nothing is known about a glvn, so the value is only tied to `target` (when there is
        // one) so conditions on it refine the target's definedness.
        if let Some(target) = target
            && let Left(local) = target.as_local(env, flow)?
        {
            let old_ty = flow.handle_get(local.clone())?.to_ty(env).ok();
            let var = flow.fresh();
            let cases = vec![
                ("0", old_ty.clone()),
                ("1", Some(Ty::Any)),
                ("10", old_ty),
                ("11", Some(Ty::Any)),
            ];
            flow.handle_set(local, PowerTy::case(var, cases, None), env.range(target))?;
            return Ok(PowerTy::bind(var));
        }
        return Ok(PowerTy::union(vec![
            0.into(),
            1.into(),
            10.into(),
            11.into(),
        ]));
    };
    let variable = Local::bare(local.name);
    let new_ty = flow.handle_get(variable.clone())?.to_maybe_ty(env);
    let maybe_subscript = flow.subscriptable(&variable.name)?;
    let var = flow.fresh();
    let mut cases = vec![];
    if new_ty.maybe_undefined() {
        cases.push(("0", None));
    }
    if let Some(ty) = new_ty.maybe_defined() {
        cases.push(("1", Some(ty.clone())));
    }
    if maybe_subscript {
        if new_ty.maybe_undefined() {
            cases.push(("10", None));
        }
        if let Some(ty) = new_ty.maybe_defined() {
            cases.push(("11", Some(ty.clone())));
        }
    }
    let c = if cases.len() == 1 {
        Some(PowerTy::Const(types::Const::String(cases[0].0.to_string())))
    } else {
        None
    };
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
    if let Some(c) = c {
        Ok(c)
    } else {
        Ok(PowerTy::bind(var))
    }
}

fn check_order<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    variable: &Range<GetTarget>,
    direction: Option<&Range<Expr>>,
    target: Option<&Range<Variable>>,
) -> Result<PowerTy, QuasiTypeError> {
    let new_ty = variable
        .to_maybe_ty(env, flow)?
        .maybe_defined()
        .cloned()
        .unwrap_or_default();
    if let Some(direction) = direction {
        let _ = direction.to_ty(env, flow)?;
    }
    if let Some(target) = target {
        let target_range = env.range(target);
        if let Left(target) = target.as_local(env, flow)? {
            let old_ty = flow.handle_get(target.clone())?.to_ty(env).ok();
            let var = flow.fresh();
            flow.handle_set(
                target,
                PowerTy::case(var, vec![("", old_ty)], Some(Some(new_ty))),
                target_range,
            )?;
            return Ok(PowerTy::bind(var));
        }
    }
    Ok(PowerTy::from("%DataType"))
}
