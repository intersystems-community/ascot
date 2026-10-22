//! The type checker. Entry points are `check_cls` (every member of a class) and `check_rtn` (a
//! macro-expanded routine); both return the `Diagnostic`s behind the README's "Diagnostics"
//! feature and collect inlay hints and cross-file references along the way.
//!
//! This file walks commands and expressions (`CheckCommand`/`CheckExprInner`), threading a `Flow`
//! (`flow.rs`) for what each local variable holds on the current path and a `CodeEnv` (`env.rs`)
//! for everything static around it. Submodules:
//! - `types.rs`: the type lattice (`Ty`, `MaybeTy`, `PowerTy`, ...) and signatures.
//! - `env.rs`: workspace/document/code-block environments; class and member resolution.
//! - `flow.rs`: flow-sensitive variable state, branch merging, loop handling.
//! - `builtins.rs`: typing rules for `$`-functions.
//! - `errors.rs`: the catalog of diagnostics (`TypeErrorKind`) and their messages.

// `TypeError`/`QuasiTypeError` carry diagnostic context and are legitimately large; they're
// returned rarely enough that boxing them isn't worth the churn.
#![allow(clippy::result_large_err)]

mod builtins;
mod env;
mod errors;
mod flow;
pub mod types;

use either::Either::{self, Left, Right};
use env::*;
use flow::*;
use std::{collections::HashMap, iter::once, ops::Deref};
use tygr::{Grammar, IntoInner, OLC, Range, VecSep};
use tygr_objectscript::cos::{CommandLike, KeywordKind, KeywordUse, Spelled};
use types::*;

pub use env::{DocumentEnv, Labels, WorkspaceEnv};
pub use errors::{
    Available, ClassLookup, Declared, Detail, DynamicGet, InRoutine, IntoTypeError, QuasiTypeError,
    Suggestions, SuperclassOf, SyntaxErrorText, TypeError, TypeErrorKind, TypeParamHint,
    UnconfirmedReason,
};

trait CheckExprInner {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError>;

    fn to_maybe_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<MaybeTy, QuasiTypeError> {
        Ok(self.to_power_ty(env, flow)?.to_maybe_ty(env))
    }

    fn to_ty<'a>(&self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<Ty, QuasiTypeError> {
        self.to_power_ty(env, flow)?.to_ty(env)
    }
}

// So `Range<Box<T>>` (boxed for size, ranged for precise error locations) resolves through
// `&Range<U>`'s blanket below with `U = Box<T>`, rather than method-call deref skipping straight
// past the `Range` to the unranged leaf.
impl<T: CheckExprInner> CheckExprInner for Box<T> {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        (**self).to_power_ty(env, flow)
    }
}

impl<T: CheckExprInner + Grammar> CheckExpr for &Range<T> {
    fn to_ranged_power_ty<'a>(
        self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<Range<PowerTy>, TypeError> {
        self.as_ref().to_ranged_power_ty(env, flow)
    }

    fn to_power_ty<'a>(self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<PowerTy, TypeError> {
        self.as_ref().to_power_ty(env, flow)
    }

    fn to_maybe_ty<'a>(self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<MaybeTy, TypeError> {
        self.as_ref().to_maybe_ty(env, flow)
    }

    fn to_ty<'a>(self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<Ty, TypeError> {
        self.as_ref().to_ty(env, flow)
    }
}

impl<T: CheckExprInner + Grammar> CheckExpr for Range<&T> {
    fn to_ranged_power_ty<'a>(
        self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<Range<PowerTy>, TypeError> {
        flow.with_range(env.range(&self), |flow| {
            match self.deref().to_power_ty(env, flow) {
                Ok(ty) => Ok(self.as_ref().map(|_| ty)),
                Err(err) => Err(err
                    .with_expr_text(|| self.print())
                    .into_error(|| env.range(&self))),
            }
        })
    }

    fn to_power_ty<'a>(self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<PowerTy, TypeError> {
        flow.with_range(env.range(&self), |flow| {
            self.deref().to_power_ty(env, flow).map_err(|err| {
                err.with_expr_text(|| self.print())
                    .into_error(|| env.range(&self))
            })
        })
    }

    fn to_maybe_ty<'a>(self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<MaybeTy, TypeError> {
        flow.with_range(env.range(&self), |flow| {
            self.deref().to_maybe_ty(env, flow).map_err(|err| {
                err.with_expr_text(|| self.print())
                    .into_error(|| env.range(&self))
            })
        })
    }

    fn to_ty<'a>(self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<Ty, TypeError> {
        flow.with_range(env.range(&self), |flow| {
            self.deref().to_ty(env, flow).map_err(|err| {
                err.with_expr_text(|| self.print())
                    .into_error(|| env.range(&self))
            })
        })
    }
}

use crate::{
    SourceMap,
    bindings::{self, ArgMode, Diagnostic, DiagnosticRelatedInformation, InlayHint, Location},
    cos::{self, *},
    details::{self, MemberDetails, MemberKindSpecificDetails, MethodDetails, WithRange},
};

pub trait CheckMember {
    fn check(&self, env: &DocumentEnv, cls: &ClassDocument) -> Result<(), TypeError>;
}

fn creates_hint(position: bindings::Position, creates: Vec<String>) -> Option<InlayHint> {
    (!creates.is_empty()).then(|| InlayHint {
        position,
        label: format!("[{}]", creates.join(", ")),
        padding_left: true,
        padding_right: true,
    })
}

// Rebases a position between a method's own local text (header + body, starting at 0) and the
// whole document -- round-trips through a byte offset rather than composing (line, character)
// pairs directly, since the two `OLC`s don't share line numbers.
struct Shifter<'a> {
    body_olc: OLC,
    doc_olc: &'a OLC,
    body_start_offset: usize,
}

impl Shifter<'_> {
    fn position(&self, relative: bindings::Position) -> bindings::Position {
        let offset = self
            .body_olc
            .line_character_to_offset((relative.line as usize, relative.character as usize));
        bindings::Position::from(
            self.doc_olc
                .offset_to_line_character(offset + self.body_start_offset),
        )
    }

    fn range(&self, relative: bindings::Range) -> bindings::Range {
        bindings::Range {
            start: self.position(relative.start),
            end: self.position(relative.end),
        }
    }

    // The inverse of `position`.
    fn unposition(&self, absolute: bindings::Position) -> bindings::Position {
        let offset = self
            .doc_olc
            .line_character_to_offset((absolute.line as usize, absolute.character as usize));
        bindings::Position::from(
            self.body_olc
                .offset_to_line_character(offset - self.body_start_offset),
        )
    }

    fn unrange(&self, absolute: bindings::Range) -> bindings::Range {
        bindings::Range {
            start: self.unposition(absolute.start),
            end: self.unposition(absolute.end),
        }
    }
}

fn try_relocate_range(
    range: bindings::Range,
    source_map: &SourceMap,
    shifter: &Shifter,
) -> Option<bindings::Range> {
    Some(shifter.range(source_map.relocate_source_range(range)?))
}

fn try_relocate_position(
    position: bindings::Position,
    source_map: &SourceMap,
    shifter: &Shifter,
) -> Option<bindings::Position> {
    Some(shifter.position(source_map.relocate_position(position)?))
}

// Shared by a method's own diagnostics and the one hard `TypeError` (if any) `with_procedure`
// returns -- both carry a `range` plus `related_information` in the same shape.
// Each relocated range is followed by what its macro expanded to, as `relocate_diagnostics` does
// for a routine. A range is left untouched if `source_map` doesn't recognize it as part of this
// method's own expansion -- e.g. `related_information` pointing at a different member's
// declaration, which happens to share this document's uri without being part of this expansion.
fn relocate_local(
    range: &mut bindings::Range,
    related_information: &mut Vec<DiagnosticRelatedInformation>,
    source_map: &SourceMap,
    uri: &str,
    shifter: &Shifter,
) {
    let expand = |range: &mut bindings::Range| {
        let text = source_map.expansion_at(range.start);
        let relocated = try_relocate_range(*range, source_map, shifter)?;
        *range = relocated;
        Some(DiagnosticRelatedInformation {
            location: Location::new(uri.to_string(), relocated),
            message: format!("↳ Expands to: {}", text?),
        })
    };
    let mut relocated: Vec<_> = expand(range).into_iter().collect();
    for mut info in std::mem::take(related_information) {
        let hop = if info.location.uri == uri {
            expand(&mut info.location.range)
        } else {
            None
        };
        relocated.push(info);
        relocated.extend(hop);
    }
    *related_information = relocated;
}

fn relocate_method_diagnostics(
    diagnostics: Vec<Diagnostic>,
    source_map: &SourceMap,
    uri: &str,
    shifter: &Shifter,
) -> Vec<Diagnostic> {
    diagnostics
        .into_iter()
        .map(|mut diagnostic| {
            relocate_local(
                &mut diagnostic.range,
                &mut diagnostic.related_information,
                source_map,
                uri,
                shifter,
            );
            diagnostic
        })
        .collect()
}

fn relocate_method_error(
    mut err: TypeError,
    source_map: &SourceMap,
    uri: &str,
    shifter: &Shifter,
) -> TypeError {
    relocate_local(
        &mut err.range,
        &mut err.related_information,
        source_map,
        uri,
        shifter,
    );
    err
}

// Only `source` (this method's own call site) is relocated -- `target` is always an already
// resolved, final declaration location.
fn relocate_method_references(
    references: Vec<(bindings::Range, Location)>,
    source_map: &SourceMap,
    shifter: &Shifter,
) -> Vec<(bindings::Range, Location)> {
    references
        .into_iter()
        .filter_map(|(source, target)| {
            Some((try_relocate_range(source, source_map, shifter)?, target))
        })
        .collect()
}

fn relocate_method_hints(
    hints: Vec<InlayHint>,
    source_map: &SourceMap,
    shifter: &Shifter,
) -> Vec<InlayHint> {
    hints
        .into_iter()
        .filter_map(|hint| {
            Some(InlayHint {
                position: try_relocate_position(hint.position, source_map, shifter)?,
                ..hint
            })
        })
        .collect()
}

fn relocate_method_hover_types(
    hover_types: Vec<(bindings::Range, String)>,
    source_map: &SourceMap,
    shifter: &Shifter,
) -> Vec<(bindings::Range, String)> {
    hover_types
        .into_iter()
        .filter_map(|(range, ty)| Some((try_relocate_range(range, source_map, shifter)?, ty)))
        .collect()
}

impl CheckMember for MemberDetails {
    fn check(&self, env: &DocumentEnv, cls: &ClassDocument) -> Result<(), TypeError> {
        match &self.kind {
            MemberKindSpecificDetails::MethodOrClassMethod(method_details) => {
                if method_details.is_abstract {
                    return Ok(());
                }
                if method_details.body.range.end.line - method_details.body.range.start.line > 10 {
                    let mut position = method_details.body.range.end;
                    position.character += 1;
                    env.inlay_hints.borrow_mut().push(InlayHint {
                        position,
                        label: format!(
                            "{}Method {}",
                            if method_details.is_class_method {
                                "Class"
                            } else {
                                ""
                            },
                            self.name.it
                        ),
                        padding_left: true,
                        padding_right: false,
                    });
                }
                method_details.check(&self.name.it, env, cls)
            }
            _ => Ok(()),
        }
    }
}

impl CheckMember for Range<&cos::Parameter> {
    fn check(&self, _env: &DocumentEnv, _cls: &ClassDocument) -> Result<(), TypeError> {
        Ok(())
    }
}

impl MethodDetails {
    fn args(&self) -> Vec<WithRange<NormalArg>> {
        let mut args = self.normal_args.clone();
        if let Some(arg) = &self.variadic_arg {
            args.push(arg.as_ref().map(NormalArg::from));
        }
        args
    }
}

impl MethodDetails {
    fn check(&self, name: &str, env: &DocumentEnv, cls: &ClassDocument) -> Result<(), TypeError> {
        let language = self.language.as_ref().or(cls.language);
        let mode = self.mode.as_ref().or(cls.mode);
        // The body isn't ObjectScript, so argument-usage tracking (e.g. "assigned but never
        // read") doesn't apply — nothing below this ever sees the body to mark anything read.
        if let Some(cos::MethodLanguage::Python(..)) = language {
            return Ok(());
        }
        let (expanded, source_map, definitions) = match env.base.expand_method_body(
            env.uri(),
            &self.header_text,
            self.body.it.as_str(),
            &cls.macro_definitions,
        ) {
            Ok(result) => result,
            Err(diagnostic) => {
                env.base.add_soft_diagnostic(diagnostic);
                return Ok(());
            }
        };
        let expanded_olc = OLC::new(&expanded);
        let method_env = DocumentEnv::new(env.base, &expanded_olc, env.uri());
        let header_len = self.header_text.len();
        let shifter = Shifter {
            body_olc: OLC::new(&format!("{}{}", self.header_text, self.body.it)),
            doc_olc: env.olc(),
            body_start_offset: self.header_start,
        };
        let body_text = expanded[header_len..].to_string();
        let body_range = bindings::Range {
            start: bindings::Position::from(expanded_olc.offset_to_line_character(header_len)),
            end: bindings::Position::from(expanded_olc.offset_to_line_character(expanded.len())),
        };
        let diagnostics_mark = env.base.soft_diagnostics_mark();
        let references_mark = env.base.references_mark();
        // An `%ObjectHandle` parameter of this class's own method is an instance of this class --
        // IRIS's `Self` stand-in (`Config.*`'s `Load(Obj As %ObjectHandle)`), read the same way
        // `resolve_object_handle` reads one in a member's signature.
        let this_ty = Ty::OREF(ClsOrIns::Ins(cls.class.clone()));
        let is_object_handle =
            |ty: &Ty| matches!(ty, Ty::OREF(ClsOrIns::Ins(c)) if c.name == "%Library.ObjectHandle");
        let local_args = self
            .args()
            .into_iter()
            .map(|arg| {
                let mut arg = WithRange::new(arg.it, shifter.unrange(arg.range));
                if arg.it.ty.as_ref().is_some_and(is_object_handle) {
                    arg.it.ty = Some(this_ty.clone());
                }
                arg
            })
            .collect();
        let local_out = self.out.clone().map(Some);
        let local_out = WithRange::new(local_out.it, shifter.unrange(local_out.range));
        let is_generator = matches!(
            mode,
            Some(cos::CodeMode::Generator(..) | cos::CodeMode::ObjectGenerator(..))
        );
        // `?` is deferred until after relocation below, so an early return doesn't skip
        // relocating diagnostics/references/hints already recorded before the error.
        let result = Flow::with_procedure(
            env.uri(),
            env.base,
            body_range,
            local_args,
            self.procedure_block.clone(),
            local_out,
            |flow| {
                let code_env = if self.is_class_method {
                    method_env.for_class_member(header_len, Some(&cls.class), name, &definitions)
                } else {
                    method_env.for_instance_member(header_len, Some(&cls.class), name, &definitions)
                };
                let body = MethodBody {
                    language,
                    mode,
                    body: WithRange::new(&body_text, self.body.range),
                };
                if is_generator {
                    flow.mark_incomplete();
                }
                body.check(&code_env, flow)
            },
            true,
            env.base.is_inherited_member(&cls.class.name, name),
        );
        for diagnostic in relocate_method_diagnostics(
            env.base.soft_diagnostics_since(diagnostics_mark),
            &source_map,
            env.uri(),
            &shifter,
        ) {
            env.base.add_soft_diagnostic(diagnostic);
        }
        for (source, target) in relocate_method_references(
            env.base.references_since(references_mark),
            &source_map,
            &shifter,
        ) {
            env.base.reference(source, target);
        }
        for hint in
            relocate_method_hints(method_env.inlay_hints.into_inner(), &source_map, &shifter)
        {
            env.inlay_hints.borrow_mut().push(hint);
        }
        for entry in
            relocate_method_hover_types(method_env.hover_types.into_inner(), &source_map, &shifter)
        {
            env.hover_types.borrow_mut().push(entry);
        }
        let creates =
            result.map_err(|err| relocate_method_error(err, &source_map, env.uri(), &shifter))?;
        if let Some(hint) = creates_hint(self.body.range.start, creates) {
            env.inlay_hints.borrow_mut().push(hint);
        }
        Ok(())
    }
}

trait CheckCommandInner {
    fn inner_check<'a>(&self, env: &CodeEnv<'a>, flow: &mut Flow<'a>)
    -> Result<(), QuasiTypeError>;

    // The keywords this spells out, given where it starts -- checked against the style.
    fn keywords_at(&self, _start: usize) -> Vec<KeywordUse> {
        vec![]
    }
}

trait CheckCommand {
    fn check<'a>(&self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<(), TypeError>;
}

impl<T: CheckCommandInner> CheckCommand for Range<T> {
    fn check<'a>(&self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<(), TypeError> {
        report_style(env, self.deref().keywords_at(self.start));
        flow.with_range(env.range(self), |flow| {
            self.inner_check(env, flow).with_range(|| env.range(self))
        })
    }
}

struct MethodBody<'a> {
    language: Option<&'a cos::MethodLanguage>,
    mode: Option<&'a cos::CodeMode>,
    body: WithRange<&'a String>,
}

impl<'b> CheckCommand for MethodBody<'b> {
    fn check<'a>(&self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<(), TypeError> {
        match (&self.language, &self.mode, &self.body) {
            (
                Option::None | Option::Some(cos::MethodLanguage::ObjectScript(..)),
                Option::None | Option::Some(cos::CodeMode::Code(..)),
                body,
            ) => {
                let body = <cos::ProcedureBody>::parse(body).map_err(|err| {
                    TypeError::invalid_syntax(
                        bindings::Range {
                            start: env.position(err.pos()),
                            end: env.position(err.pos() + 1),
                        },
                        err,
                    )
                })?;
                check_block_body(&body, env, flow, true)
            }
            (
                Option::None | Option::Some(cos::MethodLanguage::ObjectScript(..)),
                Option::Some(cos::CodeMode::Expression(..)),
                body,
            ) => {
                let body_expr = <cos::ExprEntry>::parse(body)
                    .map_err(|err| {
                        TypeError::invalid_syntax(
                            bindings::Range {
                                start: env.position(err.pos()),
                                end: env.position(err.pos() + 1),
                            },
                            err,
                        )
                    })?
                    .into_inner();
                let body_type = body_expr.to_power_ty(env, flow)?;
                body_type
                    .to_ty(env)
                    .map_err(|err| err.into_error(|| body.range))?;
                flow.handle_return(body.as_ref().map(|_| Some(body_type)))
            }
            // todo: check other kinds of ObjectScript methods
            _ => flow.handle_void(),
        }
    }
}

fn label_details(
    env: &CodeEnv<'_>,
    sub: &cos::Subroutine,
) -> Option<details::RoutineElementDetails> {
    let AllowMacro::Plain(header) = &sub.header else {
        return None;
    };
    let mut normal_args = vec![];
    let mut variadic_arg = None;
    for entry in header
        .params
        .iter()
        .flat_map(|x| &***x)
        .flat_map(|x| x.items.iter())
    {
        let mode = if entry.by_ref.is_some() {
            ArgMode::ByRef
        } else {
            ArgMode::Default
        };
        let name = entry.param.deref().print();
        let range = env.range(&entry.param);
        if entry.expand.is_some() {
            variadic_arg = Some(WithRange::new(VariadicArg::new(mode, name, None), range));
        } else {
            let default = entry.default.as_deref().map(|default| default.print());
            normal_args.push(WithRange::new(
                NormalArg::new(mode, name, None, default),
                range,
            ));
        }
    }
    Some(details::RoutineElementDetails {
        name: WithRange::new(header.label.print(), env.range(&header.label)),
        normal_args,
        variadic_arg,
        kind: details::RoutineElementKind::Subroutine,
    })
}

// A method body or routine procedure block, with its own labels (`Set $ZT="Trap"` handlers,
// `Goto` targets), checked the way a routine's labels are. In a procedure (a routine's `{...}`
// block; a method unless `ProcedureBlock = 0`) they return for the body: a method's declared
// return type, or whatever a procedure's first return establishes. (A label meant for `Do`
// belongs at class level.) A `Not ProcedureBlock` method's labels are subroutines like a
// routine's, with their own returns, falling through into one another. `is_body` is false for
// a control-flow block (`If {...}`), which a `Goto` may leave.
fn check_block_body<'a>(
    body: &cos::ProcedureBody,
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    is_body: bool,
) -> Result<(), TypeError> {
    let details: Vec<_> = body
        .subroutines
        .iter()
        .flat_map(|subs| subs.items.iter())
        .filter_map(|sub| label_details(env, sub))
        .collect();
    let is_procedure = is_body && flow.is_procedure_block();
    let labels = env::scoped_labels(
        env.uri(),
        &details.iter().map(|d| (d, false)).collect::<Vec<_>>(),
        flow.is_procedure_block().then(|| flow.output_expectation()),
    );
    let nested;
    let env = if is_body || !labels.is_empty() {
        nested = env.with_nested_labels(labels, is_procedure);
        &nested
    } else {
        env
    };
    body.main.check(env, flow)?;
    let subroutines: Vec<_> = body
        .subroutines
        .iter()
        .flat_map(|subs| subs.items.iter())
        .collect();
    if subroutines.is_empty() {
        return Ok(());
    }
    let first = details.first().and_then(|d| env.label_entry(&d.name.it));
    flow.tail_call(first.map(|first| &first.output), flow.current_range())?;
    // The labels read the body's variables from their own flows, so the deferred
    // "never read"/"never assigned" reports for the main body can't be trusted.
    flow.mark_incomplete();
    for (sub, details) in subroutines.iter().zip(&details) {
        // Each label is checked on its own: one's hard error doesn't hide the next one's.
        if let Err(err) = check_label_body(env, details, sub) {
            env.add_soft_diagnostic(err.into());
        }
    }
    Ok(())
}

fn check_label_body<'a>(
    env: &CodeEnv<'a>,
    details: &details::RoutineElementDetails,
    sub: &cos::Subroutine,
) -> Result<Vec<String>, TypeError> {
    let Some(entry) = env.label_entry(&details.name.it) else {
        return Ok(vec![]);
    };
    let next = entry.next.as_deref().and_then(|next| env.label_entry(next));
    // No commands at all (an alias label, or comments only) falls through to the next label --
    // nothing to check, and nothing to demand a QUIT from.
    let Some(range) = body_end_range(env, &sub.body) else {
        if let (Some(output), Some(next)) = (entry.output.get(), next) {
            let _ = next.output.set(output.clone());
        }
        return Ok(vec![]);
    };
    let output = entry.output.get_or_init(Default::default);
    Flow::with_label(
        env.uri(),
        env.deref(),
        range,
        details.args(),
        output.clone(),
        |flow| {
            sub.check(env, flow)?;
            flow.tail_call(next.map(|next| &next.output), range)
        },
    )
}

impl CheckCommand for cos::ProcedureMain {
    fn check<'a>(&self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<(), TypeError> {
        if let Some(line) = &self.line_1 {
            line.check(env, flow)?;
        }
        for line in self.line_n.iter().flat_map(|lines| lines.items.iter()) {
            line.check(env, flow)?;
        }
        Ok(())
    }
}

impl CheckCommand for cos::ProcedureBody {
    fn check<'a>(&self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<(), TypeError> {
        check_block_body(self, env, flow, false)
    }
}

impl<Start> CheckCommand for cos::CommandLine<Start> {
    fn check<'a>(&self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<(), TypeError> {
        let mut commands = self.commands.iter().flat_map(|cmds| cmds.items.iter());
        if !self.periods.is_empty() {
            return match commands.next() {
                Some(command) => Err(TypeError::unimplemented(
                    "Dotted block".into(),
                    env.range(command),
                )),
                None => Ok(()),
            };
        }
        for command in commands {
            command.check(env, flow)?;
        }
        Ok(())
    }
}

impl CheckCommandInner for cos::EntryRefAndArgs {
    fn inner_check<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<(), QuasiTypeError> {
        match self {
            EntryRefAndArgs::Expression(expr_call) => match expr_call {
                cos::MethodOrFuncCallExpr::Method { obj, mem, args } => {
                    let ty = base_hoty(obj, env, flow)?;
                    let diverges = matches!(&ty, HOTy::Value(ty) if never_returns(ty, &mem.id()));
                    let ret = check_member_of(ty, mem.deref(), Some(args), env, flow)?;
                    warn_if_discarding_status_or_boolean(flow, ret.as_ref());
                    if diverges {
                        flow.handle_throw()?;
                    }
                    Ok(())
                }
                cos::MethodOrFuncCallExpr::SelfMethod { member, args } => {
                    let obj = Ty::OREF(env.this()?);
                    let ret = check_member(obj, member.deref(), Some(args), env, flow)?;
                    warn_if_discarding_status_or_boolean(flow, ret.as_ref());
                    Ok(())
                }
                cos::MethodOrFuncCallExpr::Super(sup) => {
                    let ret = check_super_call(env, flow, &sup.prefixed)?;
                    warn_if_discarding_status_or_boolean(flow, ret.as_ref());
                    Ok(())
                }
                cos::MethodOrFuncCallExpr::Function(call) => {
                    let ret = match call.to_maybe_ty(env, flow)? {
                        MaybeTy::Maybe(ty) | MaybeTy::Plain(ty) => Some(ty),
                        MaybeTy::Undefined => None,
                    };
                    warn_if_discarding_status_or_boolean(flow, ret.as_ref());
                    Ok(())
                }
            },
            EntryRefAndArgs::Legacy { proc, args } => {
                let ret = check_legacy_label_call(env, flow, proc, args)?;
                warn_if_discarding_status_or_boolean(flow, ret.as_ref());
                Ok(())
            }
        }
    }
}

// Methods that always throw, so the path ends after `Do`ing them.
fn never_returns(ty: &Ty, member: &str) -> bool {
    const NEVER_RETURNS: &[(&str, &str)] = &[("%SYSTEM.Process", "ThrowError")];
    let Ty::OREF(ClsOrIns::Cls(cls) | ClsOrIns::Ins(cls)) = ty else {
        return false;
    };
    NEVER_RETURNS.iter().any(|(class, method)| {
        cls.name.eq_ignore_ascii_case(class) && member.eq_ignore_ascii_case(method)
    })
}

// `Do` never looks at what its call returns -- fine for a procedure with nothing meaningful to
// report, but a %Status/%Boolean return almost always means the caller is expected to check it
// (e.g. `$$$ISERR`/`If`), so silently dropping it here is usually a real mistake.
fn warn_if_discarding_status_or_boolean(flow: &Flow, ty: Option<&Ty>) {
    if let Some(Ty::OREF(ClsOrIns::Ins(cty))) = ty
        && (cty.name.eq_ignore_ascii_case("%Library.Status")
            || cty.name.eq_ignore_ascii_case("%Library.Boolean"))
    {
        flow.add_soft_diagnostic(QuasiTypeError::discarded_return_value(cty.to_string()));
    }
}

fn check_legacy_label_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    proc: &Range<LegacyEntryRef>,
    args: &Option<MthArgList>,
) -> Result<Option<Ty>, QuasiTypeError> {
    let label;
    let offset;
    let routine: Option<&ExtendedRoutineReference>;
    match proc.deref() {
        LegacyEntryRef::Label(lor) => {
            label = Some(&lor.label);
            offset = lor.offset.as_deref();
            routine = lor.routine.as_ref();
        }
        LegacyEntryRef::Routine(rtn) => {
            label = None;
            offset = None;
            routine = Some(rtn);
        }
    }
    check_label_call(env, flow, label, offset, routine, args, env.range(proc))
}

// `Goto $CASE(selector, v1:label1, ..., :default)`: every arm names a label of this routine.
fn check_goto_case<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    cases: &FunArgList,
) -> Result<(), QuasiTypeError> {
    let goto = |flow: &mut Flow<'a>, target: &Range<Expr>| -> Result<(), QuasiTypeError> {
        match target.as_variable() {
            Some(label) => {
                let range = env.range(target);
                call_label(env, flow, Some(label.print()), None, None, &None, range).map(|_| ())
            }
            None => target
                .to_power_ty(env, flow)
                .map(|_| ())
                .map_err(Into::into),
        }
    };
    for (i, arg) in cases.items.iter().enumerate() {
        match arg.deref() {
            Some(cos::FunArg::Normal { expr: selector, .. }) if i == 0 => {
                let _ = selector.to_power_ty(env, flow)?;
            }
            Some(cos::FunArg::ColonNormal(cos::ExpColExp { e1, e2, .. })) if i > 0 => {
                let _ = e1.to_power_ty(env, flow)?;
                goto(flow, e2)?;
            }
            Some(cos::FunArg::ColonDefault(default)) if i > 0 => goto(flow, default)?,
            _ => return Err(QuasiTypeError::unimplemented("Goto $case argument".into())),
        }
    }
    Ok(flow.handle_void()?)
}

fn hint_public_list(env: &CodeEnv, names: &[String], position: bindings::Position) {
    if !names.is_empty() {
        env.hint(InlayHint {
            position,
            label: format!(
                "Public [{}]",
                names
                    .iter()
                    .map(|name| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            padding_left: true,
            padding_right: false,
        });
    }
}

impl CheckCommandInner for cos::Block {
    fn inner_check<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<(), QuasiTypeError> {
        match self {
            cos::Block::Command(block) => Ok(block
                .items
                .iter()
                .try_for_each(|command| command.check(env, flow))?),
            cos::Block::Curry(block) => Ok(block.check(env, flow)?),
        }
    }
}

// How each keyword in `keywords` is written, if a style was given.
fn report_style(env: &CodeEnv, keywords: impl IntoIterator<Item = KeywordUse>) {
    let Some(styles) = env.keyword_styles() else {
        return;
    };
    for kw in keywords {
        if let Some(expected) = styles.of(&kw).expected(&kw) {
            let range = env.span(kw.start, kw.start + kw.text.len());
            let (what, sigil) = match kw.kind {
                KeywordKind::Command => ("Command keyword", ""),
                KeywordKind::Function => ("Function", "$"),
            };
            env.soft_diagnostic(TypeError::keyword_style(
                what,
                format!("{sigil}{}", kw.text),
                format!("{sigil}{expected}"),
                range,
            ));
        }
    }
}

impl CheckCommandInner for cos::Command {
    fn keywords_at(&self, start: usize) -> Vec<KeywordUse> {
        self.keywords(start)
    }

    fn inner_check<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<(), QuasiTypeError> {
        match self {
            cos::Command::Do(cos::DoCommand { pc, args, .. }) => {
                let args = args
                    .as_ref()
                    .ok_or_else(|| QuasiTypeError::unimplemented("Do without arguments".into()))?;
                check_pc(pc, env, flow, |flow| {
                    for arg in args.items.iter() {
                        check_pc(&arg.pc, env, flow, |flow| {
                            Ok(arg.entry_ref.check(env, flow)?)
                        })?;
                    }
                    Ok(())
                })
            }
            cos::Command::DoWhile(cos::DoWhileCommand { iter, cnds, .. }) => {
                let cnds = cnds.as_ref().ok_or_else(|| {
                    QuasiTypeError::unimplemented("do-while without conditions".into())
                })?;
                check_do_while(
                    env,
                    flow,
                    env.range(iter),
                    iter.deref().as_ref().deref(),
                    cnds,
                )
            }
            cos::Command::If(cos::IfCommand {
                cnds: Some(cnds),
                branches,
                ..
            }) => check_if(env, flow, cnds, branches),
            // Legacy argumentless `If` tests `$TEST`, which isn't tracked: an optional branch.
            cos::Command::If(cos::IfCommand {
                cnds: None,
                branches,
                ..
            }) => {
                let mut branch = flow.split();
                if let Some(cos::IfBranches { thn, .. }) = branches {
                    let result = thn.check(env, &mut branch);
                    check_branch(env, &mut branch, result);
                }
                flow.merge(branch)
            }
            cos::Command::While(cos::WhileCommand { cnds, bdy, .. }) => {
                let cnds = cnds.as_ref().ok_or_else(|| {
                    QuasiTypeError::unimplemented("while without conditions".into())
                })?;
                check_while(env, flow, cnds, env.range(bdy), bdy.as_ref().deref())?;

                env.hint(InlayHint {
                    position: env.position(bdy.as_ref().end),
                    label: "end while".to_string(),
                    padding_left: true,
                    padding_right: false,
                });

                Ok(())
            }
            cos::Command::For(cos::ForCommand { iter, bdy, .. }) => {
                check_for(env, flow, iter.as_deref(), bdy.as_ref())
            }
            cos::Command::Goto(cos::GotoCommand { pc, args, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    let args = args
                        .as_ref()
                        .ok_or_else(|| {
                            QuasiTypeError::unimplemented("Goto without arguments".into())
                        })?
                        .deref();
                    let args: Vec<_> = args.items.iter().collect();
                    if args.len() != 1 {
                        return Err(QuasiTypeError::unimplemented(
                            "Only goto with exactly one arg is supported".into(),
                        ));
                    }
                    let GoToArg { location, pc } = args[0];
                    check_pc(pc, env, flow, |flow| match location {
                        AllowCase::Plain(cos::Location(location)) => match location {
                            AllowIndirection::Plain(location) => {
                                if let LegacyEntryRef::Label(lor) = location.deref()
                                    && lor.routine.is_none()
                                {
                                    let label = lor.label.print();
                                    if env.goto_leaves_block(&label) {
                                        return Err(QuasiTypeError::goto_out_of_block(label));
                                    }
                                }
                                let _ = check_legacy_label_call(env, flow, location, &None)?;
                                Ok(flow.handle_void()?)
                            }
                            AllowIndirection::Indirection(indir) => {
                                let _ = indir.to_ty(env, flow)?;
                                Ok(flow.handle_void()?)
                            }
                        },
                        AllowCase::Case(_, kw, cases) => {
                            report_style(env, [kw.spelled()]);
                            check_goto_case(env, flow, cases)
                        }
                    })
                })
            }
            cos::Command::Break(cos::BreakCommand { pc, exp, .. }) => {
                check_pc(pc, env, flow, |_flow| {
                    if exp.is_some() {
                        Err(QuasiTypeError::unimplemented("Break w/ argument".into()))
                    } else {
                        Ok(())
                    }
                })
            }
            cos::Command::Quit(cos::QuitCommand { pc, exp, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    let exp_ty = match exp.deref() {
                        Some(exp) => Some(exit_value(exp, env, flow)?),
                        None => None,
                    };
                    flow.handle_quit(env.with_range(exp.as_ref().map(|_| exp_ty)))
                })
            }
            cos::Command::Return(cos::ReturnCommand { pc, exp, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    let ty = match exp.deref() {
                        Some(exp) => Some(exit_value(exp, env, flow)?),
                        None => None,
                    };
                    Ok(flow.handle_return(env.with_range(exp.as_ref().map(|_| ty)))?)
                })
            }
            cos::Command::Continue(cos::ContinueCommand { pc, .. }) => {
                check_pc(pc, env, flow, |flow| flow.handle_continue())
            }
            cos::Command::Dim(cos::DimCommand {
                name,
                as_type,
                default,
                ..
            }) => {
                let declared = match as_type.deref() {
                    Some(ty) => {
                        let ty = ty.deref();
                        match ty {
                            cos::DimType::Simple(ty) => Ty::from(ty.print()),
                            _ => {
                                return Err(QuasiTypeError::unimplemented(format!(
                                    "Fancy #Dim type `{}`",
                                    ty.print()
                                )));
                            }
                        }
                    }
                    None => Ty::Any,
                };
                let declared = env.with_range(as_type.as_ref().map(|_| declared));
                for name in name.items.iter() {
                    flow.handle_dim(name.print(), declared.clone())?
                }
                if let Some(default) = default {
                    // No per-name range exists in a `#Dim x,y=...` list; anchor on the
                    // default value's range instead.
                    let default_range = env.range(&default.prefixed);
                    let default = default.to_power_ty(env, flow)?;
                    for name in name.items.iter() {
                        flow.handle_set(Local::bare(name.print()), default.clone(), default_range)?
                    }
                }
                Ok(())
            }
            cos::Command::Set(cos::SetCommand {
                pc, assignments, ..
            }) => check_pc(pc, env, flow, |flow: &mut Flow| {
                for assignment in assignments.items.iter() {
                    assignment.check(env, flow)?;
                }
                Ok(())
            }),
            cos::Command::Kill(cos::KillCommand { pc, targets, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    for target in targets.iter().flat_map(|targets| targets.items.iter()) {
                        match target.deref() {
                            cos::AllowMacro::Plain(arg) => match &arg {
                                cos::KillArgument::Single(target) => {
                                    if let Some(local) = target
                                        .as_variable()
                                        .and_then(|x| {
                                            x.as_local(env, flow).map(|x| x.left()).transpose()
                                        })
                                        .transpose()?
                                    {
                                        flow.handle_kill(local)?;
                                    }
                                }
                                cos::KillArgument::Except(keep) => {
                                    let mut names = Vec::new();
                                    for target in keep.items.iter() {
                                        if let Some(variable) = target.as_variable()
                                            && let Left(local) = variable.as_local(env, flow)?
                                        {
                                            names.push(local.name);
                                        }
                                    }
                                    flow.handle_kill_except(&names)?;
                                }
                            },
                            cos::AllowMacro::Macro(..) => {
                                return Err(QuasiTypeError::unimplemented("Kill macro".into()));
                            }
                        }
                    }
                    Ok(())
                })
            }
            cos::Command::New(cos::NewCommand { pc, targets, .. }) => {
                check_pc(pc, env, flow, |flow| match targets {
                    None => flow.handle_new_except(&[], flow.range()),
                    Some(targets) => {
                        for target in targets.items.iter() {
                            match target.deref() {
                                // A special variable (`New $Namespace`, ...) isn't a local
                                // binding at all -- it's always a deliberate, meaningful
                                // save/restore, never a redundant re-`New` of something the
                                // procedure block already implicitly `New`'d.
                                cos::NewArgument::Single(cos::AllowMacro::Plain(
                                    cos::AllowIndirection::Plain(cos::NewTarget::Special(_)),
                                )) => {}
                                cos::NewArgument::Single(name) => {
                                    flow.handle_new(name.print(), env.range(target))?;
                                }
                                cos::NewArgument::Except(keep) => {
                                    let keep: Vec<String> =
                                        keep.items.iter().map(|name| name.print()).collect();
                                    flow.handle_new_except(&keep, env.range(target))?;
                                }
                            }
                        }
                        Ok(())
                    }
                })
            }
            cos::Command::Merge(cos::MergeCommand {
                pc,
                merge_arguments,
                ..
            }) => check_pc(pc, env, flow, |flow| {
                for merge_argument in merge_arguments.items.iter() {
                    merge_argument.check(env, flow)?;
                }
                Ok(())
            }),
            cos::Command::Try(cos::TryCommand {
                b1,
                catch_target,
                b2,
                ..
            }) => check_try(env, flow, b1.deref(), catch_target.as_ref(), b2),
            cos::Command::Throw(cos::ThrowCommand { pc, e, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    if let Some(e) = e {
                        e.to_ty(env, flow)?;
                    }
                    flow.handle_throw()
                })
            }
            cos::Command::ZTrap(cos::ZTrapCommand { pc, exp, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    if let Some(exp) = exp {
                        exp.to_ty(env, flow)?;
                    }
                    flow.handle_throw()
                })
            }
            cos::Command::Write(cos::WriteCommand { pc, args, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    for arg in args.iter().flat_map(|args| args.items.iter()) {
                        if let cos::WriteArg::Expr(expr) = arg {
                            let _ = expr.to_ty(env, flow)?;
                        }
                    }
                    Ok(())
                })
            }
            cos::Command::Read(cos::ReadCommand { pc, args, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    for arg in args.items.iter() {
                        if let cos::ReadArg::Var(_, target, len, timeout) = arg {
                            if let Some(len) = len {
                                len.len.to_ty(env, flow)?;
                            }
                            if let Some(timeout) = timeout {
                                timeout.timeout.to_ty(env, flow)?;
                            }
                            let string = PowerTy::from(Ty::from("%String"));
                            check_set_target(target, string, env.range(target), env, flow)?;
                        }
                    }
                    Ok(())
                })
            }
            cos::Command::Open(cos::OpenCommand { pc, args, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    for arg in args.items.iter() {
                        arg.device.to_ty(env, flow)?;
                        check_io_parameters(env, flow, arg.parameters.as_deref())?;
                        if let Some(Some(timeout)) = arg.timeout.as_deref() {
                            timeout.to_ty(env, flow)?;
                        }
                        if let Some(mnespace) = arg.mnespace.as_deref() {
                            mnespace.to_ty(env, flow)?;
                        }
                    }
                    Ok(())
                })
            }
            cos::Command::Close(cos::CloseCommand { pc, args, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    for arg in args.items.iter() {
                        arg.device.to_ty(env, flow)?;
                        if let Some(cos::CloseParameters::Many(parameters)) =
                            arg.parameters.as_deref()
                        {
                            for parameter in parameters.items.iter() {
                                if let Some(val) = &parameter.val {
                                    val.to_ty(env, flow)?;
                                }
                            }
                        }
                    }
                    Ok(())
                })
            }
            cos::Command::Use(cos::UseCommand { pc, args, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    for arg in args.items.iter() {
                        arg.device.to_ty(env, flow)?;
                        let parameters = arg.parameters.as_deref().and_then(Option::as_ref);
                        check_io_parameters(env, flow, parameters)?;
                        if let Some(mnespace) = arg.mnespace.as_deref() {
                            mnespace.to_ty(env, flow)?;
                        }
                    }
                    Ok(())
                })
            }
            cos::Command::Lock(cos::LockCommand { .. }) => Ok(()),
            cos::Command::Hang(cos::HangCommand { .. }) => Ok(()),
            cos::Command::Job(cos::JobCommand { .. }) => Ok(()),
            // The transaction level isn't tracked: it depends on the run-time conditions around
            // each `TSTART`/`TCOMMIT`, and a flow-insensitive count only produced false alarms.
            cos::Command::TStart(cos::TStartCommand { pc, .. })
            | cos::Command::TCommit(cos::TCommitCommand { pc, .. })
            | cos::Command::TRollback(cos::TRollbackCommand { pc, .. }) => {
                check_pc(pc, env, flow, |_| Ok(()))
            }
            cos::Command::ZWrite(cos::ZWriteCommand { pc, exp, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    if let Some(exp) = exp {
                        let _ = exp.to_ty(env, flow)?;
                    }
                    Ok(())
                })
            }
            cos::Command::ZKill(cos::ZKillCommand { pc, targets, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    for target in targets.items.iter() {
                        target.check(env, flow)?;
                    }
                    Ok(())
                })
            }
            cos::Command::ZNspace(cos::ZNspaceCommand { pc, e, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    let _ = e.to_ty(env, flow)?;
                    Ok(())
                })
            }
            cos::Command::Xecute(cos::XecuteCommand { pc, args, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    for arg in args.items.iter() {
                        check_pc(&arg.pc, env, flow, |flow| {
                            match &arg.arg {
                                cos::XecuteArgEnum::Expr(code) => {
                                    code.to_ty(env, flow)?;
                                }
                                cos::XecuteArgEnum::Paren(call) => {
                                    call.code.to_ty(env, flow)?;
                                    for param in &call.params {
                                        match param.deref() {
                                            cos::MthArg::Expr { expr, .. } => {
                                                expr.to_ty(env, flow)?;
                                            }
                                            cos::MthArg::ByRef { target, .. } => {
                                                if let Some(variable) = target.as_variable()
                                                    && let Left(local) =
                                                        variable.as_local(env, flow)?
                                                {
                                                    flow.invalidate(local, Ty::Any.into())?;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            flow.read_write_publics()
                        })?;
                    }
                    Ok(())
                })
            }
            cos::Command::View(cos::ViewCommand { pc, exp, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    for arg in exp.iter().flat_map(|args| args.items.iter()) {
                        match arg {
                            cos::ViewArgument::Block(e) => {
                                e.to_ty(env, flow)?;
                            }
                            cos::ViewArgument::Full {
                                offset,
                                mode,
                                length,
                                new_value,
                            } => {
                                offset.to_ty(env, flow)?;
                                mode.to_ty(env, flow)?;
                                length.to_ty(env, flow)?;
                                if let Some(new_value) = new_value {
                                    new_value.to_ty(env, flow)?;
                                }
                            }
                        }
                    }
                    Ok(())
                })
            }
            cos::Command::CallMacro(x) => x.deref().check_as_command(env, flow),
            cos::Command::ESQL(_, sql, _) => check_embedded_sql(flow, &sql.print()),
            cos::Command::HTML(_, html, _) => check_embedded_expressions(flow, &html.print()),
            cos::Command::JS(cos::JSCommand { content, .. }) => {
                check_embedded_expressions(flow, &content.print())
            }
            cos::Command::ZQuit(cos::ZQuitCommand { pc, exp, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    if let Some(exp) = exp {
                        exp.to_ty(env, flow)?;
                    }
                    flow.handle_throw()
                })
            }
            cos::Command::ElseIf(..) => Err(QuasiTypeError::unimplemented(
                "Stand-alone ElseIf Command".to_string(),
            )),
            cos::Command::Else(..) => Err(QuasiTypeError::unimplemented(
                "Stand-alone Else Command".to_string(),
            )),
            cos::Command::Halt(cos::HaltCommand { pc, .. }) => {
                check_pc(pc, env, flow, |flow| Ok(flow.handle_void()?))
            }
            // `H` alone halts; with an argument it hangs.
            cos::Command::H(cos::HCommand { pc, e, .. }) => {
                check_pc(pc, env, flow, |flow| match e {
                    Some(e) => {
                        e.to_ty(env, flow)?;
                        Ok(())
                    }
                    None => Ok(flow.handle_void()?),
                })
            }
            cos::Command::ZZDump(cos::ZZDumpCommand { pc, exp, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    for e in exp.items.iter() {
                        e.to_ty(env, flow)?;
                    }
                    Ok(())
                })
            }
            cos::Command::MVP(cos::MVPCommand { pc, args, .. })
            | cos::Command::MVC(cos::MVCCommand { pc, args, .. }) => {
                check_pc(pc, env, flow, |flow| {
                    for arg in args.items.iter() {
                        if let cos::WriteArg::Expr(expr) = arg {
                            let _ = expr.to_ty(env, flow)?;
                        }
                    }
                    Ok(())
                })
            }
            cos::Command::ZBreak(cos::ZBreakCommand { .. })
            | cos::Command::ZPrint(cos::ZPrintCommand { .. })
            | cos::Command::Print(cos::PrintCommand { .. }) => Ok(()),
            cos::Command::Unknown(unknown) => match unknown.0 {},
        }
    }
}

// The path ends here even when the value fails to check -- otherwise a failed `else { return bad }`
// branch (kept alive by `check_branch`) reads as falling through to the end of the method.
fn exit_value<'a>(
    exp: &Range<cos::Expr>,
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
) -> Result<PowerTy, QuasiTypeError> {
    exp.to_power_ty(env, flow)
        .and_then(|ty| {
            ty.to_ty(env)
                .map_err(|err| err.into_error(|| env.range(exp)))?;
            Ok(ty)
        })
        .map_err(|err| {
            let _ = flow.handle_void();
            err.into()
        })
}

// One `if`/`elseif`/`else` branch's own checking failure (e.g. genuinely unreachable code, per
// `Flow::bindings`) shouldn't abort checking of its siblings -- they're independent control-flow
// paths. Downgraded to a soft diagnostic; the branch carries on with its bindings as of the
// failure -- treating it as dead made everything after an `if` whose every branch failed
// "unreachable".
fn check_branch(env: &CodeEnv<'_>, flow: &mut Flow<'_>, result: Result<(), TypeError>) {
    if let Err(err) = result {
        env.add_soft_diagnostic(err.into());
        flow.mark_incomplete();
    }
}

fn check_if<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    cnds: &IfConditions,
    branches: &Option<cos::IfBranches>,
) -> Result<(), QuasiTypeError> {
    // todo: I am not sure the range here is accurate
    let mut if_false = check_and(
        cnds.first.as_ref().map(|x| x.deref()),
        &cnds.remaining,
        env,
        flow,
        true,
    )?;
    let mut others = vec![];
    match branches {
        None => {
            others.push(if_false);
        }
        Some(cos::IfBranches { thn, eifs, els }) => {
            let result = thn.check(env, flow);
            check_branch(env, flow, result);
            let mut flow = if_false;
            for eif in eifs {
                if_false = check_and(
                    eif.cnds.first.as_ref().map(|x| x.deref()),
                    &eif.cnds.remaining,
                    env,
                    &mut flow,
                    true,
                )?;
                let result = eif.thn.check(env, &mut flow);
                check_branch(env, &mut flow, result);
                others.push(flow);
                flow = if_false;
            }
            if let Some(els) = els.as_deref()
                && let Some(els) = els.body.as_ref()
            {
                let result = els.check(env, &mut flow);
                check_branch(env, &mut flow, result);
            }
            others.push(flow);
        }
    }
    for other in others {
        flow.merge(other)?;
    }
    Ok(())
}

fn check_try<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    try_block: &Range<cos::Block>,
    catch_target: Option<&cos::CatchTarget>,
    catch_block: &Range<cos::Block>,
) -> Result<(), QuasiTypeError> {
    let before = flow.split();
    flow.with_block(env.range(try_block), |flow| try_block.check(env, flow))?;
    // The exception may be thrown anywhere in the try block, so the catch block starts from
    // anything between its start and its end.
    let mut catch_flow = flow.split();
    catch_flow.merge(before)?;
    if let Some(target) = catch_target
        && let Left(local) = target.deref().as_local(env, flow)?
    {
        // `CatchTarget` carries no range of its own; anchor on the catch block instead.
        catch_flow.handle_set(local, Ty::Any.into(), env.range(catch_block))?
    }
    catch_flow.with_block(env.range(catch_block), |flow| catch_block.check(env, flow))?;
    flow.merge(catch_flow)
}

fn check_while<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    cnds: &WhileConditions,
    range: bindings::Range,
    block: &ProcedureBody,
) -> Result<(), QuasiTypeError> {
    // The condition is re-evaluated every iteration, so it's part of each loop pass: the first
    // (throwaway) pass primes whatever it writes (e.g. a `$LISTNEXT` by-ref pointer), and the
    // second re-applies its refinement to the state the body left behind. The loop is left where
    // the last pass found it false.
    let if_false = std::cell::RefCell::new(None);
    flow.with_loop(range, false, false, env, |flow| {
        let exit = check_and(
            cnds.first.as_ref().map(|x| x.deref()),
            &cnds.remaining,
            env,
            flow,
            false,
        )?;
        if_false.replace(Some(exit));
        block.check(env, flow)
    })?;
    match if_false.into_inner() {
        Some(if_false) => flow.merge_loop_exit(if_false),
        None => Ok(()),
    }
}

fn check_do_while<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    range: bindings::Range,
    block: &cos::ProcedureBody,
    cnds: &WhileConditions,
) -> Result<(), QuasiTypeError> {
    flow.with_loop(range, false, false, env, |flow| block.check(env, flow))?;
    // See `check_while`: prime with a throwaway evaluation before the real one below.
    let was_suppressed = flow.suppress_diagnostics();
    let result = env.with_hints_suppressed(|| {
        check_and(
            cnds.first.as_ref().map(|x| x.deref()),
            &cnds.remaining,
            env,
            flow,
            false,
        )
    });
    flow.restore_diagnostics(was_suppressed);
    result?;
    let if_false = check_and(
        cnds.first.as_ref().map(|x| x.deref()),
        &cnds.remaining,
        env,
        flow,
        false,
    )?;
    flow.merge(if_false)
}

fn check_for<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    iterator: Option<&cos::ForIterator>,
    block: Option<&Range<cos::Block>>,
) -> Result<(), QuasiTypeError> {
    if let Some(block) = block
        && let cos::Block::Curry(block) = block.deref()
    {
        env.hint(InlayHint {
            position: env.position(block.end),
            label: "end for".to_string(),
            padding_left: true,
            padding_right: false,
        });
    }

    match iterator {
        Some(cos::ForIterator { var, specs, .. }) => {
            let CheckForSpecs {
                declared,
                infinite,
                optional,
            } = check_for_specs(env, flow, specs)?;
            // `var` carries no range of its own; anchor on the loop body instead.
            let range = block.map(|b| env.range(b)).unwrap_or_default();
            if let Left(local) = var.as_local(env, flow)? {
                flow.handle_set(local, declared.into(), range)?
            }
            flow.with_loop(range, optional, infinite, env, |flow| {
                if let Some(block) = block {
                    block.check(env, flow)
                } else {
                    Ok(())
                }
            })
        }
        None => {
            if let Some(block) = block {
                flow.with_loop(env.range(block), false, true, env, |flow| {
                    block.check(env, flow)
                })?;
            }
            Ok(())
        }
    }
}

struct CheckForSpecs {
    declared: Ty,
    infinite: bool,
    // true if the for loop can be empty
    optional: bool,
}

fn check_for_specs<'a, T>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    specs: &VecSep<ForSpec, T>,
) -> Result<CheckForSpecs, TypeError> {
    let mut declared = None;
    let mut infinite = false;
    let mut optional = true;
    for spec in specs.items.iter() {
        let spec_ty = match spec {
            cos::ForSpec::Range {
                start,
                increment,
                end,
            } => {
                let start = start.to_ty(env, flow)?;
                increment.to_ty(env, flow)?;
                if let Some(end) = end.as_deref() {
                    end.to_ty(env, flow)?;
                } else {
                    infinite = true;
                    optional = false;
                }
                start
            }
            cos::ForSpec::Value(value) => {
                optional = false;
                value.to_ty(env, flow)?
            }
        };
        if declared.is_none() {
            declared = Some(spec_ty);
        }
    }
    Ok(CheckForSpecs {
        declared: declared.unwrap(),
        infinite,
        optional,
    })
}

#[allow(clippy::type_complexity)]
fn check_and<'a, T: Grammar>(
    first: Range<&GenericExpr<T>>,
    remaining: &[Prefix<LGapXLGap<StringEq!(",")>, Range<Expr>>],
    env: &CodeEnv<'a>,
    if_true: &mut Flow<'a>,
    report_always: bool,
) -> Result<Flow<'a>, TypeError> {
    let mut if_false = split_on_conjunction(first, env, if_true, report_always)?;
    for cnd in remaining.iter() {
        let flow =
            split_on_conjunction(cnd.as_ref().map(Deref::deref), env, if_true, report_always)?;
        if_false.merge(flow).with_range(|| env.range(cnd))?;
    }
    Ok(if_false)
}

// `a && b` (or `a & b`) as a condition refines on each operand in turn, exactly like `If a,b`.
fn split_on_conjunction<'a, X: Grammar, Y: Grammar>(
    expr: Range<&GenericExpr<X, Y>>,
    env: &CodeEnv<'a>,
    if_true: &mut Flow<'a>,
    report_always: bool,
) -> Result<Flow<'a>, TypeError> {
    fn split<'a>(
        ty: Range<PowerTy>,
        env: &CodeEnv<'a>,
        if_true: &mut Flow<'a>,
        report_always: bool,
    ) -> Flow<'a> {
        if_true.with_range(env.range(&ty), |if_true| {
            if_true.split_on(ty.into_inner(), report_always)
        })
    }
    let conjuncts: Option<Vec<_>> = expr
        .ops
        .iter()
        .map(|op| match op.deref() {
            InfixOperation::Other { op, rhs }
                if matches!(op.deref(), cos::Operator::LA(_) | cos::Operator::And(..)) =>
            {
                Some(rhs)
            }
            _ => None,
        })
        .collect();
    let Some(conjuncts) = conjuncts.filter(|conjuncts| !conjuncts.is_empty()) else {
        let ty = expr.to_ranged_power_ty(env, if_true)?;
        return Ok(split(ty, env, if_true, report_always));
    };
    let ty = expr.base.to_ranged_power_ty(env, if_true)?;
    let mut if_false = split(ty, env, if_true, report_always);
    for rhs in conjuncts {
        let ty = rhs.to_ranged_power_ty(env, if_true)?;
        let flow = split(ty, env, if_true, report_always);
        if_false.merge(flow).with_range(|| env.range(rhs))?;
    }
    Ok(if_false)
}

fn check_pc<'a>(
    pc: &Option<cos::PostCondition>,
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    and_then: impl Fn(&mut Flow<'a>) -> Result<(), QuasiTypeError>,
) -> Result<(), QuasiTypeError> {
    if let Some(pc) = pc {
        let pc_ty = pc.as_ref().map(Deref::deref).to_power_ty(env, flow)?;
        let old_flow = flow.split_on(pc_ty, true);
        and_then(flow)?;
        flow.merge(old_flow)
    } else {
        and_then(flow)
    }
}

fn check_io_parameters<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    parameters: Option<&Either<Paren<cos::IOParameters>, cos::IOParameter>>,
) -> Result<(), QuasiTypeError> {
    let parameters: Vec<&cos::IOParameter> = match parameters {
        None => vec![],
        Some(Left(list)) => list.items.iter().collect(),
        Some(Right(one)) => vec![one],
    };
    for parameter in parameters {
        if let Some(val) = &parameter.val {
            val.to_ty(env, flow)?;
        }
    }
    Ok(())
}

// The `#(expr)#` text is raw, not parsed: the locals it names just count as read.
fn check_embedded_expressions(flow: &mut Flow<'_>, text: &str) -> Result<(), QuasiTypeError> {
    let mut rest = text;
    while let Some(start) = rest.find("#(") {
        let Some(end) = rest[start..].find(")#") else {
            break;
        };
        let code = &rest.as_bytes()[start + 2..start + end];
        let mut i = 0;
        let mut quote = false;
        while i < code.len() {
            let c = code[i];
            if c == b'"' {
                quote = !quote;
            } else if !quote && (c.is_ascii_alphabetic() || c == b'%') {
                let mut j = i + 1;
                while j < code.len() && code[j].is_ascii_alphanumeric() {
                    j += 1;
                }
                let prefixed = i > 0 && matches!(code[i - 1], b'.' | b'$' | b'#' | b'^');
                let name = std::str::from_utf8(&code[i..j]).unwrap_or_default();
                if !prefixed && !flow::is_percent_variable(name) {
                    flow.note_read(name)?;
                }
                i = j;
                continue;
            }
            i += 1;
        }
        rest = &rest[start + end + 2..];
    }
    Ok(())
}

// Host variables: the `INTO` list is assigned, the others read; every statement sets `SQLCODE`.
fn check_embedded_sql(flow: &mut Flow<'_>, sql: &str) -> Result<(), QuasiTypeError> {
    let bytes = sql.as_bytes();
    let word_end = |mut j: usize| {
        while j < bytes.len()
            && (bytes[j].is_ascii_alphanumeric() || matches!(bytes[j], b'%' | b'_'))
        {
            j += 1;
        }
        j
    };
    let (mut i, mut quote, mut into) = (0, None, false);
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            quote = (c != q).then_some(q);
        } else if c == b'\'' || c == b'"' {
            quote = Some(c);
        } else if c == b':'
            && bytes
                .get(i + 1)
                .is_some_and(|n| n.is_ascii_alphabetic() || *n == b'%')
        {
            let end = word_end(i + 1);
            let name = sql[i + 1..end].to_string();
            let is_object = bytes.get(end) == Some(&b'.');
            if flow::is_percent_variable(&name) {
            } else if into && !is_object {
                flow.handle_set(Local::bare(name), Ty::Any.into(), flow.range())?;
            } else {
                flow.handle_get(Local::bare(name))?;
            }
            i = end;
            continue;
        } else if c.is_ascii_alphabetic() {
            let end = word_end(i);
            into = sql[i..end].eq_ignore_ascii_case("INTO");
            i = end;
            continue;
        }
        i += 1;
    }
    flow.invalidate(
        Local::bare("SQLCODE".to_string()),
        Ty::from("%Integer").into(),
    )
}

impl CheckCommandInner for cos::MergeArgument {
    fn inner_check<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<(), QuasiTypeError> {
        let ty = self.source.to_ranged_power_ty(env, flow)?;
        // `MergeArgument.target` carries no range of its own; anchor on the source instead.
        let range = env.range(&ty);
        check_merge_target(&self.target, ty.into_inner(), range, env, flow)
    }
}

impl CheckCommandInner for cos::SetArgument {
    fn inner_check<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<(), QuasiTypeError> {
        let ty = self.value.to_ranged_power_ty(env, flow)?.into_inner();
        if self.value.deref().type_unclear() {
            env.hint(bindings::InlayHint {
                position: env.position(self.target.end),
                label: format!("As {}", ty),
                padding_left: true,
                padding_right: true,
            });
        }
        let target_range = env.range(&self.target);
        match self.target.deref() {
            cos::AssignTarget::Multi(multi_set_target) => {
                for target in multi_set_target.targets.items.iter() {
                    check_set_target(target, ty.clone(), target_range, env, flow)?;
                }
                Ok(())
            }
            cos::AssignTarget::Single(set_target) => {
                check_set_target(set_target, ty, target_range, env, flow)
            }
        }
    }
}

impl CheckCommandInner for cos::ZKillTarget {
    fn inner_check<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<(), QuasiTypeError> {
        match &self.var {
            cos::AllowIndirection::Plain(var) => {
                if let Left(local) = var.as_local(env, flow)? {
                    flow.handle_zkill(local)?;
                }
                Ok(())
            }
            cos::AllowIndirection::Indirection(indirection) => {
                let _ = indirection.expr.to_ty(env, flow)?;
                flow.invalidate_all()
            }
        }
    }
}

trait TypeUnclear {
    fn type_unclear(&self) -> bool;
}

impl<A, B> TypeUnclear for GenericExpr<A, B> {
    fn type_unclear(&self) -> bool {
        self.ops.is_empty() && self.base.type_unclear()
    }
}

impl<T> TypeUnclear for OpsExpr<T> {
    fn type_unclear(&self) -> bool {
        match self {
            OpsExpr::Base(expr) => expr.type_unclear(),
            OpsExpr::Prefixed { .. } => false,
        }
    }
}

impl<T> TypeUnclear for ParenedExpr<T> {
    fn type_unclear(&self) -> bool {
        match self {
            ParenedExpr::Normal(expr) => expr.type_unclear(),
            ParenedExpr::Paren(expr) => expr.type_unclear(),
        }
    }
}

impl TypeUnclear for AtomicExpr {
    fn type_unclear(&self) -> bool {
        match self {
            AtomicExpr::DAO(..)
            | AtomicExpr::Number(..)
            | AtomicExpr::Variable(..)
            | AtomicExpr::LegacyCOM { .. }
            | AtomicExpr::String(..)
            | AtomicExpr::SpecialVariable { .. }
            | AtomicExpr::This(..)
            | AtomicExpr::Class { .. }
            | AtomicExpr::QuoteExpression { .. }
            | AtomicExpr::QuoteString { .. }
            | AtomicExpr::BeginQuoteEndQuote(..) => false,
            AtomicExpr::CallSubroutine { .. }
            | AtomicExpr::FunctionCall(..)
            | AtomicExpr::Super(..) => true,
            AtomicExpr::Member { args, .. } => args.is_some(),
            AtomicExpr::Indirection(..) => true,
        }
    }
}

impl<T> TypeUnclear for ExprOps<T> {
    fn type_unclear(&self) -> bool {
        match self {
            ExprOps::Base(base) => base.type_unclear(),
            ExprOps::Then {
                base,
                op: SufOp { mem, args },
            } => match (mem.print().as_str(), args) {
                ("%New", Some(..)) => base.type_unclear(),
                _ => true,
            },
        }
    }
}

fn check_merge_target<'a>(
    target: &cos::SetTarget,
    source: PowerTy,
    range: bindings::Range,
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
) -> Result<(), QuasiTypeError> {
    check_set_target(target, source, range, env, flow)?;
    if let Some(target) = target.as_variable()
        && let Left(target) = target.as_local(env, flow)?
    {
        flow.handle_merge(target)?
    }
    Ok(())
}

fn check_set_target<'a>(
    target: &cos::SetTarget,
    source: PowerTy,
    range: bindings::Range,
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
) -> Result<(), QuasiTypeError> {
    match target {
        SetTarget::Sub { obj, op } => {
            let obj = obj.to_ty(env, flow)?;
            check_mem(env, obj, op.mem.deref(), AccessKind::Set)?;
            Ok(())
        }
        // `Set ..Prop(subs) = value` isn't modeled apart from `Set ..Prop = value`, so
        // `%DispatchSetMultidimProperty` is never consulted (see `dispatch_member`).
        SetTarget::Member { member, .. } => {
            let obj = Ty::OREF(env.this()?);
            check_mem(env, obj, member.deref(), AccessKind::Set)?;
            Ok(())
        }
        SetTarget::Var(variable) => {
            let _ = variable.to_maybe_ty(env, flow)?;
            if let Left(id) = variable.as_local(env, flow)? {
                flow.handle_set(id, source, range)
            } else {
                Ok(())
            }
        }
        SetTarget::Fun(fun) => builtins::check_settarget_fun(fun, env, flow),
        SetTarget::Indirection(indirection) => {
            let _ = indirection.expr.to_ty(env, flow)?;
            flow.invalidate_all()
        }
        // Every variant is a bare keyword (see `SpecialVariable`) -- nothing to type-check, and
        // setting one has no effect this checker tracks (unlike a local variable's `Flow`
        // binding), so there's nothing more to do than accept it.
        SetTarget::Special(..) => Ok(()),
    }
}

trait CheckExpr: Sized {
    fn to_ranged_power_ty<'a>(
        self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<Range<PowerTy>, TypeError>;

    fn to_power_ty<'a>(self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<PowerTy, TypeError>;

    fn to_maybe_ty<'a>(self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<MaybeTy, TypeError>;

    fn to_ty<'a>(self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<Ty, TypeError>;
}

// `text`, when available, is the printed source of whichever single expression node `ty` came
// from — `None` for a left operand that's itself the result of an earlier infix operation
// rather than one node with its own literal text.
fn ranged_quasi_ty_to_ty(
    env: &CodeEnv<'_>,
    ty: Range<PowerTy>,
    text: Option<&dyn Fn() -> String>,
) -> Result<Ty, TypeError> {
    ty.to_ty(env)
        .map_err(|err| match text {
            Some(text) => err.with_expr_text(text),
            None => err,
        })
        .with_range(|| env.range(&ty))
}

impl CheckExprInner for Expr {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        self.deref().to_power_ty(env, flow)
    }
}

impl<X: Grammar, Y> CheckExprInner for GenericExpr<X, Y> {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        let mut ty = self.base.to_ranged_power_ty(env, flow)?;
        let start = ty.start;
        let mut prev_class = None;
        // Only the first operand is a single node with its own literal text — once an operator
        // has run, the accumulated `ty` is a synthesized value spanning multiple nodes.
        let mut lhs_text: Option<&dyn Fn() -> String> = Some(&|| self.base.print());
        for op in &self.ops {
            let end = op.end;
            let class = precedence_of(op.deref());
            // Only a lower-precedence operator immediately followed by a higher-precedence
            // one actually diverges from math convention (`+` then `*` regroups under
            // left-to-right evaluation; `*` then `+` doesn't, since the multiplication is
            // already a complete sub-expression by the time the addition sees it).
            if let (Some(prev), Some(class)) = (prev_class, class)
                && prev < class
            {
                return Err(QuasiTypeError::mixed_precedence());
            }
            prev_class = class;
            ty = Range::new(
                start,
                check_infix(ty, lhs_text.take(), op.deref(), env, flow)?,
                end,
            );
        }
        Ok(ty.into_inner())
    }
}

// Loosest to tightest, matching conventional math/language precedence — never ObjectScript's
// own (it has none; every one of these binds equally, strictly left-to-right).
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum Precedence {
    Logical,
    Comparison,
    Additive,
    Multiplicative,
    Power,
}

// `None` breaks the chain: string concatenation (`_`) has no consensus conventional
// precedence to check against, and a pattern match (`Match`) isn't a binary operator at all.
fn precedence_of<X>(op: &InfixOperation<X>) -> Option<Precedence> {
    let InfixOperation::Other { op, .. } = op else {
        return None;
    };
    match op.deref() {
        cos::Operator::Or(_)
        | cos::Operator::And(..)
        | cos::Operator::NAnd(_)
        | cos::Operator::NOr(_)
        | cos::Operator::LO(_)
        | cos::Operator::LA(_) => Some(Precedence::Logical),
        cos::Operator::Eq(_)
        | cos::Operator::Ne(_)
        | cos::Operator::Le(_)
        | cos::Operator::Lt(_)
        | cos::Operator::Ge(_)
        | cos::Operator::Gt(_)
        | cos::Operator::Ng(_)
        | cos::Operator::Nl(_)
        | cos::Operator::Contains(_)
        | cos::Operator::NotContains(_)
        | cos::Operator::SortsAfter(_)
        | cos::Operator::Follows(_)
        | cos::Operator::NotSortsAfter(_)
        | cos::Operator::NotFollows(_) => Some(Precedence::Comparison),
        cos::Operator::Add(_) | cos::Operator::Sub(_) => Some(Precedence::Additive),
        cos::Operator::Mul(_)
        | cos::Operator::Div(..)
        | cos::Operator::IntDiv(_)
        | cos::Operator::Mod(..) => Some(Precedence::Multiplicative),
        cos::Operator::Pow(_) => Some(Precedence::Power),
        cos::Operator::Concat(_) => None,
    }
}

impl TryFrom<&cos::Operator> for TermOp {
    type Error = ();

    fn try_from(value: &cos::Operator) -> Result<Self, Self::Error> {
        match value {
            cos::Operator::Eq(..) => Ok(Self::Eq),
            cos::Operator::Ne(..) => Ok(Self::Ne),
            cos::Operator::Le(..) => Ok(Self::Le),
            cos::Operator::Lt(..) => Ok(Self::Lt),
            cos::Operator::Ge(..) => Ok(Self::Ge),
            cos::Operator::Gt(..) => Ok(Self::Gt),
            cos::Operator::Ng(..) => Ok(Self::Ng),
            cos::Operator::Nl(..) => Ok(Self::Nl),
            cos::Operator::LO(..) => Err(()),
            cos::Operator::LA(..) => Err(()),
            cos::Operator::And(..) => Err(()),
            cos::Operator::NAnd(..) => Err(()),
            cos::Operator::Or(..) => Err(()),
            cos::Operator::NOr(..) => Err(()),
            cos::Operator::Add(..) => Ok(Self::Add),
            cos::Operator::Sub(..) => Ok(Self::Sub),
            cos::Operator::Pow(..) => Ok(Self::Pow),
            cos::Operator::Mul(..) => Ok(Self::Mul),
            cos::Operator::Div(..) => Err(()),
            cos::Operator::IntDiv(..) => Ok(Self::IntDiv),
            cos::Operator::Mod(..) => Ok(Self::Mod),
            cos::Operator::Concat(..) => Err(()),
            cos::Operator::Contains(..) => Err(()),
            cos::Operator::NotContains(..) => Err(()),
            cos::Operator::SortsAfter(..) => Err(()),
            cos::Operator::Follows(..) => Err(()),
            cos::Operator::NotSortsAfter(..) => Err(()),
            cos::Operator::NotFollows(..) => Err(()),
        }
    }
}

fn check_infix<'a, X>(
    lhs: Range<PowerTy>,
    lhs_text: Option<&dyn Fn() -> String>,
    op: &InfixOperation<X>,
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
) -> Result<PowerTy, TypeError> {
    match op {
        cos::InfixOperation::Match { .. } => Ok(PowerTy::from("%Boolean")),
        cos::InfixOperation::Other { op, rhs: rhs_node } => {
            let op = op.deref();
            let rhs = rhs_node.to_ranged_power_ty(env, flow)?;
            match (lhs.deref(), TermOp::try_from(op), rhs.deref()) {
                (PowerTy::Const(c), Ok(o), PowerTy::Term { target, term }) => {
                    return Ok(PowerTy::Term {
                        target: *target,
                        term: Term::infix(Term::Const(c.clone()), o, term.clone()),
                    });
                }
                (PowerTy::Term { target, term }, Ok(o), PowerTy::Const(c)) => {
                    return Ok(PowerTy::Term {
                        target: *target,
                        term: Term::infix(term.clone(), o, Term::Const(c.clone())),
                    });
                }
                // A fold that overflows or divides by zero falls through to the plain typing.
                (PowerTy::Const(x), Ok(o), PowerTy::Const(y)) => {
                    if let Some(c) = eval(x, o, y) {
                        return Ok(PowerTy::Const(c));
                    }
                }
                (PowerTy::Union(x1, x2, xn), Ok(o), PowerTy::Const(y)) => {
                    let xs = once(x1).chain(once(x2)).chain(xn.iter());
                    if let Some(cs) = xs.map(|x| eval(x, o, y)).collect() {
                        return Ok(PowerTy::union(cs));
                    }
                }
                (PowerTy::Const(x), Ok(o), PowerTy::Union(y1, y2, yn)) => {
                    let ys = once(y1).chain(once(y2)).chain(yn.iter());
                    if let Some(cs) = ys.map(|y| eval(x, o, y)).collect() {
                        return Ok(PowerTy::union(cs));
                    }
                }
                (PowerTy::Union(x1, x2, xn), Ok(o), PowerTy::Union(y1, y2, yn)) => {
                    let xs = once(x1).chain(once(x2)).chain(xn.iter());
                    let ys = once(y1).chain(once(y2)).chain(yn.iter());
                    if let Some(cs) = xs.flat_map(|x| ys.clone().map(|y| eval(x, o, y))).collect() {
                        return Ok(PowerTy::union(cs));
                    }
                }
                _ => (),
            }
            let lhs = ranged_quasi_ty_to_ty(env, lhs, lhs_text)?;
            let rhs = ranged_quasi_ty_to_ty(env, rhs, Some(&|| rhs_node.print()))?;
            match op {
                cos::Operator::Eq(_) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::Ne(_) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::Le(_) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::Lt(_) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::Ge(_) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::Gt(_) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::Ng(_) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::Nl(_) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::LO(_) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::LA(_) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::And(..) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::NAnd(_) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::Or(_) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::NOr(_) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::Add(_) | cos::Operator::Sub(_) => Ok(env.merge_ty(lhs, rhs).into()),
                cos::Operator::Pow(_) => Ok(PowerTy::from("%DataType")),
                cos::Operator::Mul(_) => Ok(PowerTy::from("%DataType")),
                cos::Operator::Div(..) => Ok(PowerTy::from("%DataType")),
                cos::Operator::IntDiv(_) => Ok(PowerTy::from("%Integer")),
                cos::Operator::Mod(..) => Ok(PowerTy::from("%Integer")),
                cos::Operator::Concat(_) => Ok(PowerTy::from("%String")),
                cos::Operator::Contains(_) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::NotContains(_) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::SortsAfter(_) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::Follows(_) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::NotSortsAfter(_) => Ok(PowerTy::from("%Boolean")),
                cos::Operator::NotFollows(_) => Ok(PowerTy::from("%Boolean")),
            }
        }
    }
}

fn eval(x: &Const, o: TermOp, y: &Const) -> Option<Const> {
    let t = Term::infix(Term::Const(x.clone()), o, Term::Const(y.clone()));
    // the const doesn't matter
    t.eval(&types::Const::Integer(0))
}

impl<T> CheckExprInner for cos::OpsExpr<T> {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        match self {
            OpsExpr::Base(expr) => expr.to_power_ty(env, flow),
            OpsExpr::Prefixed { op, remaining } => {
                let ty = remaining.as_ref().to_power_ty(env, flow)?;
                Ok(check_prefix(op, ty, env, flow)?)
            }
        }
    }
}

fn check_prefix<'a>(
    op: &PrefixOperator,
    ty: PowerTy,
    env: &CodeEnv<'a>,
    _flow: &mut Flow<'a>,
) -> Result<PowerTy, TypeError> {
    match ty {
        PowerTy::Const(c) => Ok(match op {
            cos::PrefixOperator::Not(_) => PowerTy::Const((c.as_integer() == 0).into()),
            cos::PrefixOperator::Minus(_) => PowerTy::from("%DataType"),
            cos::PrefixOperator::Plus(_) => PowerTy::Const(c.as_integer().into()),
        }),
        PowerTy::Term { target, term } if matches!(op, cos::PrefixOperator::Not(_)) => {
            Ok(PowerTy::Term {
                target,
                term: term.negate(),
            })
        }
        PowerTy::Case { target, .. } if matches!(op, cos::PrefixOperator::Not(_)) => {
            Ok(PowerTy::Term {
                target,
                term: Term::Target.negate(),
            })
        }
        ty => {
            let _ = ty.to_ty(env);
            match op {
                cos::PrefixOperator::Not(_) => Ok(PowerTy::from("%Boolean")),
                cos::PrefixOperator::Minus(_) => Ok(ty),
                cos::PrefixOperator::Plus(_) => Ok(ty),
            }
        }
    }
}

impl<T> CheckExprInner for ExprOps<T> {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        match self {
            ExprOps::Base(base) => base.to_power_ty(env, flow),
            ExprOps::Then { base, op } => {
                // `$SYSTEM.X.Y()` is shorthand for `##class(%SYSTEM.X).Y()`.
                if op.args.is_none() && !op.mem.is_parameter() && is_system_var(base) {
                    let cls = env.resolve_class(&format!("%SYSTEM.{}", op.mem.id()));
                    return Ok(Ty::OREF(ClsOrIns::Cls(cls)).into());
                }
                let ty = base_hoty(base, env, flow)?;
                // `$SYSTEM.Security.System.M()`, `$system.Context.SQL()`: a nested `%SYSTEM`
                // class, the latter reached through a kernel accessor that yields its instance.
                if let HOTy::Value(Ty::OREF(ClsOrIns::Cls(cls))) = &ty
                    && cls.name.starts_with("%SYSTEM.")
                    && !op.mem.is_parameter()
                {
                    let nested = format!("{}.{}", cls.name, op.mem.id());
                    if env.class_exists(&nested) {
                        let nested = env.resolve_class(&nested);
                        return Ok(Ty::OREF(if op.args.is_none() {
                            ClsOrIns::Cls(nested)
                        } else {
                            ClsOrIns::Ins(nested)
                        })
                        .into());
                    }
                }
                let ty = check_member_of(ty, &op.mem, op.args.as_ref(), env, flow)?;
                if let Some(ty) = ty {
                    Ok(ty.into())
                } else {
                    Err(QuasiTypeError::undefined())
                }
            }
        }
    }
}

// The base of a member operation, keeping a collection property a collection (rather than its
// `%Collection.Abstract*` class) so `..List.GetAt(i)` is typed by the element.
fn base_hoty<'a, T>(
    base: &Range<ExprOps<T>>,
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
) -> Result<HOTy, QuasiTypeError> {
    let member = match base.deref() {
        ExprOps::Then { base: inner, op }
            if op.args.is_none() && !op.mem.is_parameter() && !is_system_var(inner) =>
        {
            check_mem(env, inner.to_ty(env, flow)?, &op.mem, AccessKind::Get)?
        }
        ExprOps::Base(atom)
            if let ParenedExpr::Normal(atom) = atom.deref()
                && let AtomicExpr::Member { member, args: None } = atom.deref().deref() =>
        {
            check_mem(env, Ty::OREF(env.this()?), member, AccessKind::Get)?
        }
        _ => return base.to_ty(env, flow).map(HOTy::Value),
    };
    Ok(member.ty)
}

fn check_member_of<'a>(
    base: HOTy,
    mem: &Range<MemberRef>,
    args: Option<&Range<MthArgList>>,
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
) -> Result<Option<Ty>, QuasiTypeError> {
    if let HOTy::Collection { kind, element } = &base
        && let Some(args) = args
        && let Some(proc) = collection_method(*kind, element, &mem.id())
    {
        let proc = TyProcedureOrAny::Procedure(proc);
        return check_call(env, flow, proc, args.items.iter(), env.range(mem));
    }
    check_member(base.to_ty()?, mem, args, env, flow)
}

// The `%Collection.AbstractList`/`AbstractArray` methods whose signatures involve the element
// type; the rest resolve on the collection class.
fn collection_method(
    kind: CollectionClass,
    element: &Ty,
    name: &str,
) -> Option<std::sync::Arc<TyProcedure>> {
    let key = kind.key_ty();
    let element = element.clone();
    let arg = |name: &str, ty: Ty, mode| NormalArg {
        mode,
        name: name.to_string(),
        ty: Some(ty),
        default: None,
        declared_at: None,
        variadic: false,
    };
    let (normal_args, out) = match name {
        "GetAt" | "RemoveAt" => (vec![arg("key", key, ArgMode::Default)], element),
        "GetNext" | "GetPrevious" => (vec![arg("key", key, ArgMode::ByRef)], element),
        "SetAt" => (
            vec![
                arg("element", element, ArgMode::Default),
                arg("key", key, ArgMode::Default),
            ],
            Ty::from("%Status"),
        ),
        "Insert" if kind == CollectionClass::List => (
            vec![arg("element", element, ArgMode::Default)],
            Ty::from("%Status"),
        ),
        "InsertAt" if kind == CollectionClass::List => (
            vec![
                arg("element", element, ArgMode::Default),
                arg("key", key, ArgMode::Default),
            ],
            Ty::from("%Status"),
        ),
        "Count" => (vec![], Ty::from("%Integer")),
        _ => return None,
    };
    Some(std::sync::Arc::new(TyProcedure {
        normal_args,
        variadic_arg: None,
        out: Some(out),
        procedure_block: Some(vec![]),
        uri: None,
        declared_at: None,
    }))
}

fn is_system_var<T>(base: &ExprOps<T>) -> bool {
    let ExprOps::Base(base) = base else {
        return false;
    };
    let ParenedExpr::Normal(base) = base.deref() else {
        return false;
    };
    matches!(
        base.deref().deref(),
        AtomicExpr::SpecialVariable {
            var: SpecialVariable::SYSTEM(..),
            ..
        }
    )
}

fn check_member<'a>(
    ty: Ty,
    mem: &Range<MemberRef>,
    args: Option<&Range<MthArgList>>,
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
) -> Result<Option<Ty>, QuasiTypeError> {
    let kind = if args.is_some() {
        AccessKind::Call
    } else {
        AccessKind::Get
    };
    let member = check_mem(env, ty, mem, kind)?;
    match (member.ty, args) {
        // `..Prop(subs)`: a multidimensional property's node, typed like the property.
        (HOTy::Value(ty), Some(args)) if member.multidimensional => {
            builtins::check_args_any(env, flow, args.items.iter())?;
            Ok(Some(ty))
        }
        (ty, Some(args)) => {
            let ty = ty.as_procedure()?;
            check_call(env, flow, ty, args.items.iter(), env.range(mem))
        }
        (ty, None) => Ok(ty.to_ty()?.into()),
    }
}

impl CheckExprInner for cos::GetTarget {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        match self {
            cos::GetTarget::Var(var) => var.to_power_ty(env, flow),
            cos::GetTarget::Member { member, args } => {
                let obj = Ty::OREF(env.this()?);
                check_member(obj, member.deref(), args.as_ref(), env, flow)?
                    .map(PowerTy::from)
                    .ok_or_else(QuasiTypeError::undefined)
            }
            cos::GetTarget::Indirection(ind) => Ok(ind.to_power_ty(env, flow)?),
            cos::GetTarget::Fun(call) => call.to_power_ty(env, flow),
            cos::GetTarget::Sub { base, op } => {
                let base = base_hoty(base, env, flow)?;
                check_member_of(base, &op.mem, op.args.as_ref(), env, flow)?
                    .map(PowerTy::from)
                    .ok_or_else(QuasiTypeError::undefined)
            }
        }
    }
}

// `##super(args)`: calls the superclass's version of the enclosing method.
fn check_super_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    args: &Range<MthArgList>,
) -> Result<Option<Ty>, QuasiTypeError> {
    let range = env.range(args);
    let ty = env.resolve_super_mstr(range)?.ty.as_procedure()?;
    check_call(env, flow, ty, args.items.iter(), range)
}

fn check_mem(
    env: &CodeEnv<'_>,
    obj: Ty,
    mem: &Range<MemberRef>,
    kind: AccessKind,
) -> Result<MemberTy, QuasiTypeError> {
    let range = env.range(mem);
    let member = env.resolve_ty_mref(&obj, mem.deref(), range, kind)?;
    if let Some(location) = &member.location {
        env.reference(range, location.clone());
    }
    Ok(member)
}

fn check_param_arg<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    par: NormalArg,
    arg: &Range<Option<cos::MthArg>>,
    declared_at: Option<bindings::Location>,
) -> Result<(), QuasiTypeError> {
    {
        let arg_text = match arg.deref() {
            Some(arg) => match arg {
                cos::MthArg::ByRef { target, .. } => target.print(),
                cos::MthArg::Expr { expr, .. } => expr.print(),
            },
            None => "".to_string(),
        };
        if !arg_text.starts_with(&par.name) {
            let prefix = if par.mode == ArgMode::Default {
                ""
            } else {
                "."
            };
            let (label, padding_right) = if let Some(default) = par.default.as_ref()
                && arg.is_none()
            {
                (format!("{}{}: {}", prefix, par.name, default), false)
            } else {
                (format!("{}{}:", prefix, par.name), arg.is_some())
            };
            env.hint(bindings::InlayHint {
                position: env.position(arg.start),
                label,
                padding_left: false,
                padding_right,
            });
        }
    }
    // A mode mismatch compiles (the caller's `.` alone decides by-reference; `ByRef`/`Output`
    // document intent), so it's reported softly and the rest of the call is still checked.
    match (&par.mode, arg.deref()) {
        (_, Some(cos::MthArg::Expr { expr, expand })) => {
            // A value where `ByRef`/`Output` is declared: the callee's changes never reach the
            // caller.
            if par.mode != ArgMode::Default {
                flow.add_soft_diagnostic(QuasiTypeError::WithRange(
                    TypeError::mismatch_argument_mode(
                        par.name.clone(),
                        par.mode,
                        ArgMode::Default,
                        env.range(arg),
                        declared_at.clone(),
                    ),
                ));
            }
            let target = par.ty.clone().unwrap_or_default();
            let source = expr.to_power_ty(env, flow)?;
            // `args...` spreads `args(1)..args(args)` over this and every later parameter;
            // only the count variable itself is visible here.
            if expand.is_some() {
                return Ok(());
            }
            source.to_ty(env)?;
            env.assert_value_compatible(&source, &target)
                .map_err(|err| err.assign(par.name))?;
        }
        (_, Some(cos::MthArg::ByRef { target, .. })) => {
            if par.mode == ArgMode::Default {
                flow.add_soft_diagnostic(QuasiTypeError::WithRange(
                    TypeError::mismatch_argument_mode(
                        par.name.clone(),
                        ArgMode::Default,
                        ArgMode::ByRef,
                        env.range(arg),
                        declared_at.clone(),
                    ),
                ));
            }
            let source = PowerTy::from(par.ty.clone().unwrap_or_default());
            match target.as_variable() {
                // The callee may read the prior value before writing, so `invalidate` — not a
                // fresh, obligated write — models it: satisfies whatever was pending instead of
                // demanding another read.
                Some(variable) => {
                    let _ = variable.to_maybe_ty(env, flow)?;
                    if let Left(local) = variable.as_local(env, flow)? {
                        flow.invalidate(local, source)?;
                    }
                }
                None => check_merge_target(target, source, env.range(arg), env, flow)?,
            }
        }
        (_, None) => {
            if par.required() {
                return Err(QuasiTypeError::too_few_arguments(par.name, declared_at));
            }
        }
    }
    Ok(())
}

fn check_call<'a, 'b>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    base: TyProcedureOrAny,
    args: impl Iterator<Item = &'b Range<Option<cos::MthArg>>>,
    range: bindings::Range,
) -> Result<Option<Ty>, QuasiTypeError> {
    match base {
        // Unknown callee shape: conservatively assume it might read any public variable.
        TyProcedureOrAny::Any => {
            flow.read_publics()?;
            Ok(Some(builtins::check_args_any(env, flow, args)?))
        }
        TyProcedureOrAny::Procedure(base) => {
            // `f()` parses as one empty argument; it means no argument at all.
            let mut args: Vec<_> = args.collect();
            if let [only] = args.as_slice()
                && only.is_none()
            {
                args.clear();
            }
            // todo: add inlay hints
            let mut count = 0;
            for (i, arg) in args.into_iter().enumerate() {
                count += 1;
                let par = if let Some(par) = base.get_arg(i) {
                    par
                } else if arg.is_none() {
                    continue;
                } else {
                    return Err(TypeError::too_many_arguments(env.range(arg)).into());
                };
                let declared_at = Option::zip(base.uri.clone(), par.declared_at)
                    .map(|(uri, range)| bindings::Location::new(uri, range));
                check_param_arg(env, flow, par, arg, declared_at).with_range(|| env.range(arg))?;
                if matches!(
                    arg.deref(),
                    Some(cos::MthArg::Expr {
                        expand: Some(_),
                        ..
                    })
                ) {
                    count = base.normal_args.len();
                    break;
                }
            }
            for i in count..base.normal_args.len() {
                if let Some(par) = base.get_arg(i)
                    && par.required()
                {
                    let declared_at = Option::zip(base.uri.clone(), par.declared_at)
                        .map(|(uri, range)| bindings::Location::new(uri, range));
                    return Err(QuasiTypeError::too_few_arguments(par.name, declared_at));
                }
            }
            match &base.procedure_block {
                Some(names) => {
                    hint_public_list(env, names, range.end);
                    // A procedure block only ever sees exactly its own declared public names —
                    // it's otherwise isolated from the caller's other locals.
                    // A name never bound here isn't shadowed: the callee reaches the outer one.
                    for name in names {
                        if flow::is_percent_variable(name) || !flow.has_binding(name)? {
                            continue;
                        }
                        if flow.is_public(name)? {
                            flow.invalidate(Local::bare(name.to_string()), Ty::Any.into())?;
                        } else {
                            return Err(QuasiTypeError::require_private_variable(name.to_string()));
                        }
                    }
                }
                None => flow.read_write_publics()?,
            }
            Ok(base.out.clone())
        }
    }
}

impl CheckExprInner for MacroCallStruct {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        // Anything reaching here survived expansion: a compile-time macro (see
        // `MacroDefinition::is_compile_time`), whose value is simply unknown, or an undefined one.
        let name = self.name.print();
        if env.is_defined_macro(&name) {
            self.check_args(env, flow)?;
            Ok(Ty::Any.into())
        } else {
            Err(QuasiTypeError::undefined_macro(
                name.clone(),
                similar_macro_names(&name, env.known_macro_names()),
            ))
        }
    }
}

fn similar_macro_names<'a>(name: &str, candidates: impl Iterator<Item = &'a str>) -> Vec<String> {
    top_similar(name, candidates, 20)
}

// Case-insensitive Jaro-Winkler similarity -- names the `n` closest matches out of a candidate
// pool too large to print in full. Rewards a shared *prefix*, which is what a typo like
// `ISERROR` for `ISERR` actually is; plain edit distance ranks unrelated same-length names just
// as close.
pub(crate) fn top_similar<'a>(
    name: &str,
    candidates: impl Iterator<Item = &'a str>,
    n: usize,
) -> Vec<String> {
    let target = name.to_lowercase();
    let mut scored: Vec<(f64, &str)> = candidates
        .filter(|candidate| !candidate.eq_ignore_ascii_case(name))
        .map(|candidate| (jaro_winkler(&target, &candidate.to_lowercase()), candidate))
        .collect();
    scored.sort_by(|(s1, n1), (s2, n2)| s2.total_cmp(s1).then_with(|| n1.cmp(n2)));
    // Case variants score the same, so they're adjacent here, and which one survives is fixed
    // by the sort -- not by the (hash-map) order the candidates arrived in.
    scored.dedup_by_key(|(_, name)| name.to_lowercase());
    scored
        .into_iter()
        .take(n)
        .map(|(_, name)| name.to_string())
        .collect()
}

// 1.0 = identical, 0.0 = nothing in common. See Winkler (1990); the standard algorithm (a Jaro
// score, boosted for up to 4 shared leading characters).
fn jaro_winkler(a: &str, b: &str) -> f64 {
    let jaro = jaro(a, b);
    let prefix_len = a
        .chars()
        .zip(b.chars())
        .take(4)
        .take_while(|(x, y)| x == y)
        .count();
    jaro + prefix_len as f64 * 0.1 * (1.0 - jaro)
}

fn jaro(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() || b.is_empty() {
        return if a.is_empty() && b.is_empty() {
            1.0
        } else {
            0.0
        };
    }
    let match_distance = a.len().max(b.len()) / 2 - if a.len().max(b.len()) < 2 { 0 } else { 1 };
    let mut a_matched = vec![false; a.len()];
    let mut b_matched = vec![false; b.len()];
    let mut matches = 0usize;
    for (i, &ca) in a.iter().enumerate() {
        let start = i.saturating_sub(match_distance);
        let end = (i + match_distance + 1).min(b.len());
        for (j, matched) in b_matched.iter_mut().enumerate().take(end).skip(start) {
            if !*matched && ca == b[j] {
                a_matched[i] = true;
                *matched = true;
                matches += 1;
                break;
            }
        }
    }
    if matches == 0 {
        return 0.0;
    }
    let mut transpositions = 0usize;
    let mut b_iter = b
        .iter()
        .zip(b_matched.iter())
        .filter(|(_, m)| **m)
        .map(|(c, _)| c);
    for (&ca, _) in a.iter().zip(a_matched.iter()).filter(|(_, m)| **m) {
        if let Some(&cb) = b_iter.next()
            && ca != cb
        {
            transpositions += 1;
        }
    }
    let m = matches as f64;
    (m / a.len() as f64 + m / b.len() as f64 + (m - (transpositions / 2) as f64) / m) / 3.0
}

trait MacroCallStructExt {
    fn check_args<'a>(&self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<(), QuasiTypeError>;
    fn check_as_command<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<(), QuasiTypeError>;
}

impl MacroCallStructExt for MacroCallStruct {
    fn check_args<'a>(&self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<(), QuasiTypeError> {
        for arg in self.args.iter().flat_map(|args| args.items.iter()) {
            check_macro_arg_as_expr(arg, &Ty::Any, env, flow)?;
        }
        Ok(())
    }

    fn check_as_command<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<(), QuasiTypeError> {
        let name = self.name.print();
        if env.is_defined_macro(&name) {
            self.check_args(env, flow)
        } else {
            Err(QuasiTypeError::undefined_macro(
                name.clone(),
                similar_macro_names(&name, env.known_macro_names()),
            ))
        }
    }
}

// A macro argument is raw text (it is also substituted verbatim into a `#define` body), so it
// is re-parsed as an expression here.
fn check_macro_arg_as_expr<'a>(
    arg: &Range<cos::MacroArg>,
    expected_ty: &Ty,
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
) -> Result<(), QuasiTypeError> {
    env.assert_compatible(
        expr_from_macro_arg(arg)
            .map_err(|()| QuasiTypeError::invalid_macro_argument("expected an expression"))?
            .to_ty(env, flow)
            .map_err(|err| relocate_macro_arg_error(env, arg.start, err))?,
        expected_ty,
    )
    .map_err(Incompatible::general)
}

// `expr_from_macro_arg` re-prints `arg` and re-parses it as a standalone `Expr`, so a `WithRange`
// error `to_ty` already resolved is relative to that fresh, zero-based text -- not `arg`'s real
// position in the document. Recover the real position instead of discarding it (as a plain
// `strip_range` would): undo `env`'s offset->position conversion to get back the original
// zero-based offset, add where `arg` actually starts, and convert that back to a real position.
// A `WithoutRange` error has no (wrong) position to fix -- leave it for the usual `into_error`
// up the call stack to place, same as ever.
fn relocate_macro_arg_error(
    env: &CodeEnv<'_>,
    arg_start: usize,
    err: QuasiTypeError,
) -> QuasiTypeError {
    match err {
        QuasiTypeError::WithRange(err) => {
            let relocate = |pos: bindings::Position| env.position(env.offset(pos) + arg_start);
            QuasiTypeError::WithRange(TypeError {
                range: bindings::Range {
                    start: relocate(err.range.start),
                    end: relocate(err.range.end),
                },
                ..err
            })
        }
        without_range => without_range,
    }
}

fn expr_from_macro_arg(value: &Vec<MacroArgUnit>) -> Result<Expr, ()> {
    let value = value.print();
    let value = ExprEntry::parse(&value).or(Err(()))?;
    Ok(value.into_inner().into_inner())
}

impl CheckExprInner for Variable {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        match self.as_local(env, flow)? {
            Left(local) => flow.handle_get(local),
            Right(ty) => Ok(ty),
        }
    }
}

impl CheckExprInner for SpecialVariable {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        _flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        match self {
            SpecialVariable::THIS(..) => Ok(check_this(env)?.into()),
            SpecialVariable::Unknown(name) => {
                let name = name.print();
                let suggestions =
                    top_similar(&name, cos::SPECIAL_VARIABLE_NAMES.iter().copied(), 5);
                Err(QuasiTypeError::unknown_special_variable(name, suggestions))
            }
            _ => Ok(Ty::Any.into()),
        }
    }
}

impl<T> CheckExprInner for ParenedExpr<T> {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        match self {
            ParenedExpr::Normal(normal) => {
                // The only place a `##class(X)` literal's own span is visible before it gets
                // lost to auto-deref (`AtomicExpr::to_power_ty` itself never sees a range) —
                // peek here to record the go-to-definition reference alongside it.
                if let AtomicExpr::Class { cls, .. } = normal.deref().deref() {
                    let cty = env.resolve_class(&env.qualify_class_name(&cls.deref().print()));
                    if let Some(location) = &cty.location {
                        env.reference(env.range(normal.deref()), location.clone());
                    }
                }
                // Same trick, for a bare local-variable read's hover type — `subs: None` only,
                // since a subscripted read's `PowerTy` only reflects "is anything under this
                // variable assigned", not that element's actual type.
                let is_bare_local = matches!(
                    normal.deref().deref(),
                    AtomicExpr::Variable(cos::Variable {
                        base: AllowMacro::Plain(VariableBase::Local(_)),
                        subs: None,
                    })
                );
                let range = is_bare_local.then(|| env.range(normal.deref()));
                let ty = normal.to_power_ty(env, flow)?;
                if let Some(range) = range {
                    env.hover_type(range, ty.to_string());
                }
                Ok(ty)
            }
            ParenedExpr::Paren(inner) => Ok(inner.to_power_ty(env, flow)?),
        }
    }
}

impl CheckExprInner for AtomicExpr {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        match self {
            AtomicExpr::DAO(dao) => dao.to_power_ty(env, flow),
            AtomicExpr::Number(number) => builtins::check_number(number),
            AtomicExpr::Variable(variable) => variable.to_power_ty(env, flow),
            // `""` inside a string literal is a single, literal `"` -- unescape it so the
            // tracked constant is the value the string actually holds, not its source spelling.
            AtomicExpr::String(string) => Ok(PowerTy::Const(types::Const::String(
                string.deref().print().replace("\"\"", "\""),
            ))),
            AtomicExpr::SpecialVariable { var, .. } => var.to_power_ty(env, flow),
            AtomicExpr::This(..) => Ok(check_this(env)?.into()),
            // `##class(X)` denotes the class itself, not an instance of it — unlike every
            // other string-to-`Ty` conversion here, which always produces an instance. A class
            // that doesn't exist is reported here, at the `##class(...)`, not at whatever member
            // is then accessed on it; only an IRIS connection can say so for sure.
            AtomicExpr::Class { cls, .. } => {
                let name = env.qualify_class_name(&cls.deref().print());
                if env.has_iris() && !env.class_exists(&name) {
                    return Err(QuasiTypeError::cannot_find_class(name));
                }
                Ok(PowerTy::plain(Ty::OREF(ClsOrIns::Cls(
                    env.resolve_class(&name),
                ))))
            }
            AtomicExpr::Member { member, args } => {
                let obj = Ty::OREF(env.this()?);
                match check_member(obj, member.deref(), args.as_ref(), env, flow)? {
                    Some(ty) => Ok(ty.into()),
                    None => Err(QuasiTypeError::undefined()),
                }
            }
            AtomicExpr::BeginQuoteEndQuote(..) => Ok(PowerTy::from("%String")),
            AtomicExpr::Indirection(x) => Ok(x.to_power_ty(env, flow)?),
            AtomicExpr::FunctionCall(call) => call.to_power_ty(env, flow),
            AtomicExpr::QuoteString { .. } => Ok(Ty::from("%String").into()),
            AtomicExpr::Super(sup) => match check_super_call(env, flow, &sup.prefixed)? {
                Some(ty) => Ok(ty.into()),
                None => Err(QuasiTypeError::undefined()),
            },
            AtomicExpr::CallSubroutine {
                label,
                routine,
                args,
                ..
            } => Ok(check_label_call(
                env,
                flow,
                label.deref().as_ref(),
                None,
                routine.as_ref(),
                args,
                env.range(label),
            )?
            .ok_or_else(QuasiTypeError::procedure_call_returns_nothing)?
            .into()),
            AtomicExpr::LegacyCOM { .. } => Err(QuasiTypeError::unimplemented(
                "Legacy COM binding ('_obj!sub')".to_string(),
            )),
            AtomicExpr::QuoteExpression { .. } => Err(QuasiTypeError::unimplemented(
                "'##quoteExp(...)'".to_string(),
            )),
        }
    }
}

fn check_label_call<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    label: Option<&Range<AllowIndirection<Label>>>,
    offset: Option<&Range<Expr>>,
    routine: Option<&ExtendedRoutineReference>,
    args: &Option<MthArgList>,
    range: bindings::Range,
) -> Result<Option<Ty>, QuasiTypeError> {
    let label = label
        .map(|label| check_label(env, flow, label))
        .transpose()?
        .flatten();
    let offset = if let Some(offset) = offset {
        Some(offset.to_ty(env, flow)?)
    } else {
        None
    };
    let routine = routine
        .map(|routine| check_routine(env, flow, routine))
        .transpose()?
        .flatten();
    call_label(env, flow, label, offset, routine, args, range)
}

fn call_label<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    label: Option<String>,
    offset: Option<Ty>,
    routine: Option<String>,
    args: &Option<MthArgList>,
    range: bindings::Range,
) -> Result<Option<Ty>, QuasiTypeError> {
    let proc = env.resolve_label(label, offset, routine)?;
    if let TyProcedureOrAny::Procedure(proc) = &proc
        && let Some(location) = proc.location()
    {
        env.reference(range, location);
    }
    check_call(
        env,
        flow,
        proc,
        args.iter().flat_map(|args| args.items.iter()),
        range,
    )
}

// `None` for an indirect label — we can't know its name statically, so `resolve_label` falls
// back to `Any` the same way it does for a label that's merely absent.
fn check_label<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    label: &Range<AllowIndirection<Label>>,
) -> Result<Option<String>, QuasiTypeError> {
    match label.deref() {
        AllowIndirection::Plain(label) => Ok(Some(label.print())),
        AllowIndirection::Indirection(indirection) => {
            let _ = indirection.to_ty(env, flow)?;
            Ok(None)
        }
    }
}

// `None` for an indirect routine name, same reasoning as `check_label`. `extension` (an explicit
// `.mac`/`.int` qualifier) doesn't need checking on its own — any routine reference at all
// already makes `resolve_label` fall back to `Any`, regardless of extension.
fn check_routine<'a>(
    env: &CodeEnv<'a>,
    flow: &mut Flow<'a>,
    routine: &ExtendedRoutineReference,
) -> Result<Option<String>, QuasiTypeError> {
    match &routine.name {
        AllowIndirection::Plain(name) => Ok(Some(name.print())),
        AllowIndirection::Indirection(indir) => {
            let _ = indir.to_ty(env, flow)?;
            Ok(None)
        }
    }
}

fn check_this(env: &CodeEnv<'_>) -> Result<Ty, QuasiTypeError> {
    match &env.this()? {
        ClsOrIns::Ins(this) => Ok(Ty::from(this.to_string())),
        // In class methods, ##THIS refers to the string of its class name.
        ClsOrIns::Cls(..) => Ok(Ty::from("%String")),
    }
}

impl CheckExprInner for cos::DAO {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        match self {
            cos::DAO::Obj { content, .. } => {
                if let Some(content) = content.as_ref() {
                    for bind in content.items.iter() {
                        bind.value.check(env, flow)?;
                    }
                }
                Ok(Ty::from("%DynamicObject").into())
            }
            cos::DAO::Arr { content, .. } => {
                if let Some(content) = content.as_ref() {
                    for item in content.items.iter() {
                        item.check(env, flow)?;
                    }
                }
                Ok(Ty::from("%DynamicArray").into())
            }
        }
    }
}

impl CheckCommandInner for cos::DAOElem {
    fn inner_check<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<(), QuasiTypeError> {
        match self {
            cos::DAOElem::DAO(dao) => {
                let _ = dao.to_ty(env, flow)?;
            }
            cos::DAOElem::True(_) => (),
            cos::DAOElem::False(_) => (),
            cos::DAOElem::Null(_) => (),
            cos::DAOElem::Str(_) => (),
            cos::DAOElem::Num(..) => (),
            cos::DAOElem::OS { content, .. } => {
                let _ = content.to_ty(env, flow)?;
            }
        }
        Ok(())
    }
}

trait VariableExt {
    // Try to convert the variable into a local variable, checking any subscript expressions'
    // types along the way — `get` reaches only the exact node for a bare local, but the whole
    // subtree once there are literal subscripts (see `Flow::get`).
    fn as_local<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<Either<Local, PowerTy>, QuasiTypeError>;
}

impl VariableExt for cos::AllowIndirection<Variable> {
    fn as_local<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<Either<Local, PowerTy>, QuasiTypeError> {
        match self {
            cos::AllowIndirection::Plain(var) => var.as_local(env, flow),
            cos::AllowIndirection::Indirection(indirection) => {
                let _ = indirection.to_power_ty(env, flow)?;
                Ok(Right(Ty::Any.into()))
            }
        }
    }
}

impl VariableExt for Variable {
    fn as_local<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<Either<Local, PowerTy>, QuasiTypeError> {
        let subs = if let Some(subs) = self.subs.as_deref() {
            subs.items
                .iter()
                .map(|item| item.to_ty(env, flow))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            vec![]
        };
        match &self.base {
            AllowMacro::Plain(this) => match this {
                VariableBase::Local(name) => Ok(Left(Local {
                    name: name.print(),
                    subs,
                })),
                VariableBase::Public(..)
                | VariableBase::Global(..)
                | VariableBase::StructureSystemVariable { .. } => Ok(Right(Ty::Any.into())),
                // `r%Foo` (the property's last-persisted value) and `i%Foo` (its current
                // in-memory value) both name the same property, so they resolve identically here
                // — ascot only tracks a property's type, not its in-memory-vs-stored value.
                VariableBase::Instance(mem) => {
                    check_instance_member(env, env.range(mem.deref()), quotable_name(mem))
                }
                VariableBase::RowInstance(mem) => {
                    check_instance_member(env, env.range(mem.deref()), mem.deref().print())
                }
            },
            AllowMacro::Macro(macro_call) => macro_call.to_power_ty(env, flow).map(Either::Right),
        }
    }
}

// `i%"%%OID"` names the member without its quotes.
fn quotable_name(name: &QuotableClassMemberName) -> String {
    name.name()
}

fn check_instance_member(
    env: &CodeEnv<'_>,
    mem_range: bindings::Range,
    mem: String,
) -> Result<Either<Local, PowerTy>, QuasiTypeError> {
    match env.this()? {
        ClsOrIns::Cls(_) => {
            Err(QuasiTypeError::access_instance_only_member_from_class_context(mem))
        }
        ClsOrIns::Ins(cty) => {
            let member =
                env.resolve_obj_mstr(&ClsOrIns::Ins(cty), mem, false, mem_range, AccessKind::Get)?;
            if let Some(location) = &member.location {
                env.reference(mem_range, location.clone());
            }
            Ok(Right(member.ty.to_ty()?.into()))
        }
    }
}

trait AsCommandLines<Start: 'static> {
    fn as_command_lines(&self) -> impl Iterator<Item = &CommandLine<Start>>;
}

impl AsCommandLines<SGap1> for RoutineMain {
    fn as_command_lines(&self) -> impl Iterator<Item = &CommandLine<SGap1>> {
        self.line_1
            .iter()
            .chain(self.line_n.iter().flat_map(|line_n| line_n.items.iter()))
    }
}

// The span of `main`'s commands, for highlighting the whole body in a "missing QUIT" diagnostic.
fn body_end_range(env: &CodeEnv<'_>, main: &RoutineMain) -> Option<bindings::Range> {
    let mut commands = main.as_command_lines().flat_map(|line| {
        line.commands
            .iter()
            .flat_map(|commands| commands.items.iter())
    });
    let first = commands.next()?;
    let end = commands.last().map_or(first.end, |last| last.end);
    Some(bindings::Range {
        start: env.position(first.start),
        end: env.position(end),
    })
}

impl CheckCommand for RoutineMain {
    fn check<'a>(&self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<(), TypeError> {
        for line in self.as_command_lines() {
            line.check(env, flow)?;
        }
        Ok(())
    }
}

impl details::RoutineElementDetails {
    fn args(&self) -> Vec<WithRange<NormalArg>> {
        inputs_of(&self.normal_args, &self.variadic_arg)
    }
}

fn check_procedure(
    this: &Procedure,
    details: &details::RoutineElementDetails,
    env: &CodeEnv<'_>,
) -> Result<(), TypeError> {
    let range = env.range(&this.body);
    let creates = Flow::with_procedure(
        env.uri(),
        env.deref(),
        range,
        details.args(),
        match &details.kind {
            details::RoutineElementKind::Subroutine => None,
            details::RoutineElementKind::Procedure { public_list } => Some(public_list.clone()),
        },
        env.with_range(this.header.as_ref().map(|_| None)),
        |flow| check_block_body(&this.body, env, flow, true),
        true,
        false,
    )?;
    if let Some(hint) = creates_hint(details.name.range.end, creates) {
        env.hint(hint);
    }
    Ok(())
}

fn inputs_of(
    normal_args: &[WithRange<NormalArg>],
    variadic_arg: &Option<WithRange<VariadicArg>>,
) -> Vec<WithRange<NormalArg>> {
    let mut args = normal_args.to_vec();
    if let Some(arg) = variadic_arg {
        args.push(arg.as_ref().map(NormalArg::from));
    }
    args
}

impl CheckCommand for Subroutine {
    fn check<'a>(&self, env: &CodeEnv<'a>, flow: &mut Flow<'a>) -> Result<(), TypeError> {
        self.body.check(env, flow)
    }
}

pub fn check_rtn(
    env: &DocumentEnv,
    rtn: &Routine,
    definitions: &crate::MacroDefinitions,
) -> Vec<bindings::Diagnostic> {
    let olc = env.olc();
    let elements: Vec<(&cos::RoutineElement, Option<details::RoutineElementDetails>)> = rtn
        .subroutines
        .iter()
        .flat_map(|srs| srs.items.iter())
        .map(|elem| (elem, details::routine_element_kind(olc, elem)))
        .collect();
    let labels: Vec<_> = elements
        .iter()
        .filter_map(|(elem, details)| {
            let details = details.as_ref()?;
            Some((details, matches!(elem, cos::RoutineElement::Procedure(_))))
        })
        .collect();
    let env = env.for_routine(env::scoped_labels(env.uri(), &labels, None), definitions);
    let mut diagnostics = Vec::new();
    // An empty main (no code before the first tag) is the normal, idiomatic way to write a
    // tags-only routine — there's no meaningful position to blame it falling through on, so
    // don't check it at all rather than flag every such routine with a bogus range.
    if let Some(range) = body_end_range(&env, &rtn.main) {
        let first = labels
            .first()
            .filter(|(_, is_procedure)| !is_procedure)
            .and_then(|(details, _)| env.label_entry(&details.name.it));
        match Flow::with_label(
            env.uri(),
            env.deref(),
            range,
            vec![],
            Default::default(),
            |flow| {
                rtn.main.check(&env, flow)?;
                flow.tail_call(first.map(|first| &first.output), range)
            },
        ) {
            Ok(creates) => {
                if let Some(hint) = creates_hint(range.start, creates) {
                    env.hint(hint);
                }
            }
            Err(err) => diagnostics.push(err.into()),
        }
    }
    for (elem, details) in &elements {
        let Some(details) = details else { continue };
        let result = match elem {
            cos::RoutineElement::Procedure(procedure) => check_procedure(procedure, details, &env),
            cos::RoutineElement::Subroutine(subroutine) => {
                check_label_body(&env, details, subroutine).map(|creates| {
                    if let Some(hint) = creates_hint(details.name.range.end, creates) {
                        env.hint(hint);
                    }
                })
            }
        };
        if let Err(err) = result {
            diagnostics.push(err.into());
        }
    }
    diagnostics
}

pub(crate) fn check_cls(
    env: &DocumentEnv,
    cls: &details::ClassDetails,
) -> Vec<bindings::Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut defined = HashMap::new();
    for keyword in &cls.keywords_without_value {
        diagnostics
            .push(TypeError::class_keyword_without_value(keyword.it.clone(), keyword.range).into());
    }
    // Available to every class without an explicit `Include` (see `CLASS_IMPLICIT_INCLUDES`) --
    // resolved silently (no diagnostic on failure, unlike an explicit `Include`).
    let (mut macro_definitions, _) =
        env.base
            .resolve_class_includes(&crate::macro_expand::implicit_includes(
                crate::macro_expand::CLASS_IMPLICIT_INCLUDES,
            ));
    // An `Include` is inherited: a subclass sees every include file of its superclasses too.
    let (inherited_definitions, _) = env
        .base
        .resolve_class_includes(&env.base.inherited_include_names(cls));
    macro_definitions.extend(inherited_definitions);
    let include_names: Vec<String> = cls.include_names.iter().map(|i| i.it.clone()).collect();
    let (own_definitions, failed_includes) = env.base.resolve_class_includes(&include_names);
    for include in &cls.include_names {
        if let Some(location) = env.base.include_definition(&include.it) {
            env.base.reference(include.range, location);
        }
    }
    macro_definitions.extend(own_definitions);
    let has_unresolved_include = !failed_includes.is_empty();
    // `include_range` is only `None` when there's no `Include` at all, in which case
    // `failed_includes` is empty too and this range is never actually used.
    let include_range = cls.include_range.unwrap_or(cls.name.range);
    for name in failed_includes {
        diagnostics.push(TypeError::cannot_find_include(name, include_range).into());
    }
    let doc = ClassDocument::new(
        cls.name.it.to_string().into(),
        cls.language.as_ref(),
        cls.mode.as_ref(),
        macro_definitions,
        has_unresolved_include,
    );
    for member in &cls.members {
        let name = &member.name.it;
        let conflict = details::MemberNs::of(&member.kind)
            .iter()
            .filter(|&&ns| ns != details::MemberNs::Other)
            .filter_map(|&ns| defined.insert((ns, name.to_string()), member))
            .next();
        if let Some(previous) = conflict {
            diagnostics.push(Diagnostic::from(TypeError::duplicated_member_name(
                member.range,
                name.to_string(),
                Location::new(env.uri().to_string(), previous.range),
            )));
        }
        check_property_or_parameter_declaration(env, &cls.name.it, member, &mut diagnostics);
        // A missing `Include` (already reported once above) commonly leaves some macro
        // unexpanded in a way that breaks a method body's parse -- don't also raise the
        // resulting generic syntax error, same reasoning as `expand_rtn`'s own fallback.
        if let Err(err) = member.deref().check(env, &doc)
            && !(has_unresolved_include && matches!(err.kind, TypeErrorKind::InvalidSyntax { .. }))
        {
            diagnostics.push(err.into());
        }
    }
    diagnostics
}

fn check_property_or_parameter_declaration(
    env: &DocumentEnv,
    class_name: &str,
    member: &WithRange<MemberDetails>,
    diagnostics: &mut Vec<bindings::Diagnostic>,
) {
    match &member.kind {
        MemberKindSpecificDetails::Property {
            ty,
            element_ty,
            keywords,
            type_param_names,
        } => {
            if let Some(Ty::OREF(ClsOrIns::Ins(cty))) = ty {
                let name = env.base.qualify_class_name(&cty.name, Some(class_name));
                env.base.check_declared_class_exists(&name, member.range);
            }
            for keyword in keywords {
                if !PROPERTY_KEYWORDS
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case(keyword))
                {
                    diagnostics.push(
                        TypeError::unrecognized_property_keyword(keyword.clone(), member.range)
                            .into(),
                    );
                }
            }
            if !type_param_names.is_empty() {
                // For `list Of X(...)` / `array Of X(...)` the parameters are the element's.
                let type_class = match element_ty.as_ref().or(ty.as_ref()) {
                    Some(Ty::OREF(ClsOrIns::Ins(cty))) => Some(cty.name.clone()),
                    _ => None,
                };
                // A type-param can also come from a `[PropertyClass = ...]` on this class or an
                // ancestor (e.g. %JSON.Adaptor lists %JSON.PropertyParameters, making
                // `%JSONFIELDNAME` valid everywhere) -- see `WorkspaceEnv::property_class_names`.
                let property_classes = env.base.property_class_names(class_name);
                let candidate_classes: Vec<&str> = type_class
                    .as_deref()
                    .into_iter()
                    .chain(property_classes.iter().map(String::as_str))
                    .collect();
                for name in type_param_names {
                    let is_property_keyword = PROPERTY_KEYWORDS
                        .iter()
                        .any(|k| k.eq_ignore_ascii_case(name));
                    let results: Vec<Option<bool>> = candidate_classes
                        .iter()
                        .map(|cls| env.base.parameter_exists(cls, name))
                        .collect();
                    let found = results.contains(&Some(true));
                    // Report as absent once any candidate confirms it, even if others are
                    // unverifiable -- not just when every candidate is.
                    let confirmed_absent = !found && results.contains(&Some(false));
                    if is_property_keyword || confirmed_absent {
                        let class = type_class
                            .clone()
                            .unwrap_or_else(|| "this property's type".to_string());
                        let suggestions = if is_property_keyword {
                            vec![]
                        } else {
                            let names: Vec<String> = candidate_classes
                                .iter()
                                .flat_map(|cls| env.base.parameter_names(cls))
                                .collect();
                            top_similar(name, names.iter().map(String::as_str), 5)
                        };
                        diagnostics.push(
                            TypeError::unrecognized_type_param(
                                name.clone(),
                                class,
                                is_property_keyword,
                                suggestions,
                                member.range,
                            )
                            .into(),
                        );
                    }
                }
            }
        }
        MemberKindSpecificDetails::Parameter {
            as_type, keywords, ..
        } => {
            if let Some(meta_type) = as_type
                && !PARAMETER_TYPE_KEYWORDS
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case(meta_type))
            {
                diagnostics.push(
                    TypeError::unrecognized_parameter_type(meta_type.clone(), member.range).into(),
                );
            }
            for keyword in keywords {
                if !PARAMETER_KEYWORDS
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case(keyword))
                {
                    diagnostics.push(
                        TypeError::unrecognized_parameter_keyword(keyword.clone(), member.range)
                            .into(),
                    );
                }
            }
        }
        _ => {}
    }
}

impl From<TypeError> for bindings::Diagnostic {
    fn from(value: TypeError) -> Self {
        bindings::Diagnostic {
            message: value.kind.to_string(),
            range: value.range,
            related_information: value.related_information,
            severity: value.kind.severity(),
            code: value.kind.code().to_string(),
            code_description: value.kind.code_description(),
        }
    }
}

impl<T: CheckExprInner> CheckExprInner for AllowIndirection<T> {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        match self {
            AllowIndirection::Plain(x) => x.to_power_ty(env, flow),
            AllowIndirection::Indirection(indirection) => Ok(indirection.to_power_ty(env, flow)?),
        }
    }
}

impl<T: CheckCommandInner> CheckCommandInner for AllowIndirection<T> {
    fn inner_check<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<(), QuasiTypeError> {
        match self {
            AllowIndirection::Plain(x) => x.inner_check(env, flow),
            AllowIndirection::Indirection(indirection) => {
                let _ = indirection.to_power_ty(env, flow)?;
                Ok(())
            }
        }
    }
}

impl CheckExprInner for cos::Indirection {
    fn to_power_ty<'a>(
        &self,
        env: &CodeEnv<'a>,
        flow: &mut Flow<'a>,
    ) -> Result<PowerTy, QuasiTypeError> {
        let _ = self.expr.to_ty(env, flow)?;
        if let Some(subscripts) = &self.subscript_indirection {
            for subscript in subscripts.items.iter() {
                let _ = subscript.to_ty(env, flow)?;
            }
        }
        Ok(Ty::Any.into())
    }
}
