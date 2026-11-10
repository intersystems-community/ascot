//! Flow-sensitive state for one code block: which local variables are defined on the current
//! path, with what type, and whether they have been read yet.
//!
//! `Flow` is forked at `If`/`Else`, loops, `Try`/`Catch` and `Quit`/`Return`, and the branches
//! are merged back with `merge_bindings`. That merge is where "may be undefined", "assigned
//! but never read" and "inconsistent declaration across branches" diagnostics come from. Driven
//! by the command rules in `mod.rs`; produces `types.rs` values; reports through `errors.rs`.

use crate::bindings;
use crate::bindings::Location;
use crate::bindings::RefMode;
use crate::check::env::*;
use crate::check::errors::*;
use crate::check::types::*;
use crate::details::WithRange;
use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::collections::{BTreeSet, HashSet};
use std::iter::once;
use std::rc::Rc;

use super::errors::IntoTypeError;

// Its `bool` in `Shared::reading_list` is `false` until read, then `true`.
#[derive(PartialEq, Eq, Hash)]
struct Written {
    name: String,
    range: bindings::Range,
}

impl Written {
    fn new(name: String, range: bindings::Range) -> Self {
        Self { name, range }
    }
}

// A parameter passed by reference (`ByRef` or `Output`) and how the body used it: read needing
// its incoming value, read tolerating its absence, written (assigned, merged into, killed or
// handed on by reference).
struct RefParam {
    name: String,
    range: bindings::Range,
    mode: RefMode,
    needed: bool,
    read: bool,
    written: bool,
}

// The handler that used it: `handle_ref`, `handle_get`, or any write.
enum RefUse {
    Ref,
    Get,
    Set,
}

struct Shared<'a> {
    env: &'a CodeEnv<'a>,
    // Whether a name this flow has not seen may already hold a value: a free label's publics, or
    // a nested label's privates, set by the block before the label was reached. False for a
    // procedure's own body, which starts with its parameters and public list alone, and for a
    // trap handler, which starts from the body's state.
    may_exist: bool,
    // `None` is for subroutines and procedures only. It means that the output expectation
    // is unclear until the first return/quit command. Shared (the `Rc`) with the labels that
    // return for this body (see `check_block_body`).
    output: OutputCell,
    ty_var: RefCell<u32>,
    reading_list: RefCell<HashMap<Written, bool>>,
    // In declaration order, for a deterministic diagnostic when more than one qualifies.
    ref_params: RefCell<Vec<RefParam>>,
    // Visible in the signature already, so excluded from `creates` below.
    known_names: HashSet<String>,
    // Every not-`New`'d (`new != Some(true)`) variable written anywhere in the procedure, i.e.
    // one whose value silently leaks into the caller's scope on return.
    creates: RefCell<HashSet<String>>,
    // Every variable written, for a nested label whose block reads them after a `Do` of it.
    writes: RefCell<HashSet<String>>,
    // Set once an `if` branch's checking failed partway (see `check_branch`): the deferred "never
    // read"/"never assigned" reports can't be trusted after that.
    incomplete: std::cell::Cell<bool>,
}

struct LoopContext {
    range: bindings::Range,
    quits: Vec<WithRange<Bindings>>,
    continues: Vec<Bindings>,
}

impl LoopContext {
    fn new(range: bindings::Range) -> Self {
        Self {
            range,
            quits: vec![],
            continues: vec![],
        }
    }
}

// What a flow's returns are held to, once one establishes it: shared by the labels that return
// for the same caller.
pub(crate) type OutputCell = Rc<RefCell<Option<WithRange<Option<Ty>>>>>;

// What a body did to its variables, for whoever shares them.
pub(crate) struct Effects {
    // The not-`New`'d variables it assigned: what leaks into the caller's scope on return.
    pub(crate) creates: Vec<String>,
    // Every variable it assigned, including the block's privates.
    pub(crate) writes: HashSet<String>,
}

// The special variable that armed a trap, and so the one whose reset disarms it.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum TrapVar {
    ZTrap,
    ETrap,
}

impl TrapVar {
    fn name(self) -> &'static str {
        match self {
            TrapVar::ZTrap => "$ZTRAP",
            TrapVar::ETrap => "$ETRAP",
        }
    }
}

// The error traps a flow arms. `active` is where an error on this path goes: a label of this
// body, or (`None`) the handler this flow is, entered with the trap still naming it. `joins` is,
// per handler label, the merge of every state it can be entered from, deposited in the `CodeEnv`
// at the body's end for the label's own check.
#[derive(Clone, Default)]
struct Traps {
    active: Option<(TrapVar, Option<String>)>,
    joins: HashMap<String, Join>,
}

impl Traps {
    fn merge(
        &mut self,
        them: Traps,
        env: &WorkspaceEnv<'_>,
        undeclared: &Binding,
    ) -> Result<(), QuasiTypeError> {
        if self.active.is_none() {
            self.active = them.active;
        }
        for (label, join) in them.joins {
            match self.joins.get_mut(&label) {
                Some(mine) => mine.bindings.merge(Some(join.bindings), env, undeclared)?,
                None => {
                    self.joins.insert(label, join);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct Join {
    var: TrapVar,
    bindings: Bindings,
}

// The life time 'a is for the entire code block (i.e., method body or a stand-alone expression)
pub struct Flow<'a> {
    shared: Rc<Shared<'a>>,
    // `None` once this path has concluded (`Return`, `Quit`, or otherwise) — every operation
    // that ends a path must clear this. Reading past that point is dead code (see `bindings()`).
    bindings: Option<Bindings>,
    loop_context: Option<LoopContext>,
    current_range: bindings::Range,
    traps: Traps,
    // A bare or exclusive `New` was issued: every name created from here on lives in this frame.
    new_all: bool,
    // Past a `ZQuit`: the frames below are gone, so what this path returns is nobody's to check.
    detached: bool,
}

// A local variable name plus its literal subscript keys (checked types, in order, unused today
// beyond their count) — what `get`/`set`/`kill`/`invalidate` address.
#[derive(Clone)]
pub struct Local {
    pub name: String,
    pub subs: Vec<Ty>,
}

impl Local {
    pub fn bare(name: String) -> Local {
        Local { name, subs: vec![] }
    }
}

impl<'a> Flow<'a> {
    pub(crate) fn fresh(&mut self) -> u32 {
        let ty_var = *self.shared.ty_var.borrow();
        self.shared.ty_var.replace(ty_var + 1);
        ty_var
    }

    pub(crate) fn with_range<T>(
        &mut self,
        range: bindings::Range,
        handler: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let range = std::mem::replace(&mut self.current_range, range);
        let out = handler(self);
        self.current_range = range;
        out
    }

    // Falls back to `current_range` for a `QuasiTypeError` that never picked up a more specific
    // one (e.g. it isn't tied to the expression currently being checked). A no-op during a
    // loop's throwaway priming pass (see `WorkspaceEnv::suppress_soft_diagnostics`).
    pub(crate) fn add_soft_diagnostic(&self, err: QuasiTypeError) {
        self.shared
            .env
            .add_soft_diagnostic(err.into_error(|| self.current_range).into());
    }

    // `ty` read as a value (`text` is its source): an undefined one is reported softly and read
    // as `Any`, so the rest of the body is still checked.
    pub(crate) fn value(
        &self,
        ty: &PowerTy,
        text: impl FnOnce() -> String,
    ) -> Result<Ty, QuasiTypeError> {
        match ty.to_maybe_ty(self.shared.env) {
            MaybeTy::Undefined => {
                self.add_soft_diagnostic(QuasiTypeError::undefined().with_expr_text(text));
                Ok(Ty::Any)
            }
            MaybeTy::Maybe(ty) => {
                self.add_soft_diagnostic(
                    QuasiTypeError::maybe_undefined(ty.clone()).with_expr_text(text),
                );
                Ok(ty)
            }
            MaybeTy::Plain(ty) => Ok(ty),
        }
    }

    pub(crate) fn mark_incomplete(&self) {
        self.shared.incomplete.set(true);
    }

    // Runs `f` as a throwaway priming pass: nothing it reports or hints survives.
    pub(crate) fn primed<T>(&mut self, env: &CodeEnv, f: impl FnOnce(&mut Self) -> T) -> T {
        let was_suppressed = self.shared.env.suppress_soft_diagnostics();
        let result = env.with_hints_suppressed(|| f(self));
        self.shared.env.restore_soft_diagnostics(was_suppressed);
        result
    }

    // A label, held to `output` (see `LabelEntry::output`); an error handler starts from `join`,
    // the states it can be entered from (see `Traps`). Returns what it creates.
    pub(crate) fn with_label(
        env: &'a CodeEnv<'a>,
        range: bindings::Range,
        args: Vec<WithRange<NormalArg>>,
        output: OutputCell,
        join: Option<Join>,
        handler: impl FnOnce(&mut Self) -> Result<(), TypeError>,
    ) -> Result<Effects, TypeError> {
        Self::run(env, range, args, None, output, join, handler, false, false)
    }

    pub(crate) fn is_procedure_block(&self) -> bool {
        self.shared.env.in_procedure_block()
    }

    // Whether the procedure block has private variables, every local it uses. A flow that may not
    // know them all (a label `Do`ne or `Goto`ne) can only say yes.
    pub(crate) fn private_variables(&mut self) -> Result<Option<bool>, QuasiTypeError> {
        if !self.shared.env.in_procedure_block() {
            return Ok(None);
        }
        let any = self.bindings()?.bindings.values().any(|b| b.private);
        Ok((any || !self.shared.may_exist).then_some(any))
    }

    // `ZQuit`: the frames below are gone, and this code goes on without them (measured).
    pub(crate) fn detach(&mut self) -> Result<(), QuasiTypeError> {
        self.invalidate_all()?;
        self.detached = true;
        Ok(())
    }

    // `label` returns for this flow, so its returns are held to this flow's.
    pub(crate) fn share_output(&self, label: &OnceCell<OutputCell>) {
        let _ = label.set(self.shared.output.clone());
    }

    // Falling off the end of a body is a tail call of the label that follows it: that label
    // returns for this body, and reads what it assigned (so the deferred "never read" reports
    // can't be trusted). With nothing to run into, that's worth a word.
    pub(crate) fn tail_call(
        &mut self,
        next: Option<&OnceCell<OutputCell>>,
        range: bindings::Range,
    ) -> Result<(), TypeError> {
        if self.bindings.is_none() {
            return Ok(());
        }
        match next {
            Some(next) => {
                self.share_output(next);
                self.mark_incomplete();
            }
            None => self.add_soft_diagnostic(TypeError::missing_quit(range).into()),
        }
        self.handle_void()
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn with_procedure(
        env: &'a CodeEnv<'a>,
        range: bindings::Range,
        args: Vec<WithRange<NormalArg>>,
        public_list: ProcedureBlock,
        output: WithRange<Option<Option<Ty>>>,
        handler: impl FnOnce(&mut Self) -> Result<(), TypeError>,
        auto_return: bool,
        // Whether the signature can say `Output`: a method's can, a label's can't.
        output_keyword: bool,
    ) -> Result<Vec<String>, TypeError> {
        let output = Rc::new(RefCell::new(
            output.it.map(|it| WithRange::new(it, output.range)),
        ));
        Self::run(
            env,
            range,
            args,
            public_list,
            output,
            None,
            handler,
            auto_return,
            output_keyword,
        )
        .map(|effects| effects.creates)
    }

    // Runs `handler` in a fresh flow for a body with `args`; returns what the body creates.
    #[allow(clippy::too_many_arguments)]
    fn run(
        env: &'a CodeEnv<'a>,
        range: bindings::Range,
        args: Vec<WithRange<NormalArg>>,
        public_list: ProcedureBlock,
        output: OutputCell,
        join: Option<Join>,
        handler: impl FnOnce(&mut Self) -> Result<(), TypeError>,
        auto_return: bool,
        output_keyword: bool,
    ) -> Result<Effects, TypeError> {
        let procedure_block = env.in_procedure_block();
        let may_exist = public_list.is_none() && join.is_none();
        let mut bindings = HashMap::new();
        let mut reading_list = HashMap::new();
        let mut ref_params = Vec::new();
        let mut known_names = HashSet::new();
        for WithRange {
            it:
                NormalArg {
                    mode,
                    name,
                    ty,
                    variadic,
                    default,
                    ..
                },
            range,
        } in args
        {
            known_names.insert(name.clone());
            if is_percent_variable(&name) {
                continue;
            }
            // A default spelled as a constant is held to the parameter's PATTERN as an argument is.
            if let (Some(default), Some(ty)) = (&default, &ty)
                && let Some(constant) = Const::from_literal(default)
            {
                env.check_pattern(&PowerTy::Const(constant), ty, range, Some(&name));
            }
            let binding = {
                let (assigned, subscript_assigned, new) = match mode {
                    None => {
                        reading_list.insert(Written::new(name.clone(), range), false);
                        let subscripts = variadic.then_some(());
                        (ty.clone().unwrap_or_default().into(), subscripts, true)
                    }
                    Some(RefMode::ByRef) => {
                        (ty.clone().unwrap_or_default().into(), Some(()), false)
                    }
                    // The caller need not pass a value, so there is none until the body assigns
                    // one.
                    Some(RefMode::Output) => (PowerTy::undefined(), Some(()), false),
                };
                if let Some(mode) = mode {
                    ref_params.push(RefParam {
                        name: name.clone(),
                        range,
                        mode,
                        needed: false,
                        read: false,
                        written: false,
                    });
                }
                Binding {
                    assigned,
                    subscript_assigned,
                    new: Some(new),
                    new_at: None,
                    incoming: mode.is_some(),
                    private: procedure_block,
                    declared: Some(WithRange::new(ty.clone().unwrap_or_default(), range)),
                    assigned_at: HashSet::from([range]),
                }
            };
            if let Some(previous_definition) = bindings.insert(name.clone(), binding) {
                return Err(TypeError::redeclare_variable(
                    name,
                    range,
                    Location::new(
                        env.uri().to_string(),
                        previous_definition.declared.unwrap().range,
                    ),
                ));
            }
        }
        if let Some(public_list) = public_list {
            for public in public_list {
                known_names.insert(public.it.clone());
                match bindings.get_mut(public.as_str()) {
                    Some(public) => public.private = false,
                    None => {
                        bindings.insert(public.it, Binding::public(public.range));
                    }
                }
            }
        }
        // A handler's trap still names it when it is entered.
        let is_handler = join.is_some();
        let (bindings, traps) = match join {
            Some(Join { var, bindings }) => (
                bindings,
                Traps {
                    active: Some((var, None)),
                    joins: HashMap::new(),
                },
            ),
            None => (Bindings { bindings }, Traps::default()),
        };
        let mut flow = Self {
            shared: Rc::new(Shared {
                env,
                may_exist,
                output,
                ty_var: RefCell::new(0),
                reading_list: RefCell::new(reading_list),
                ref_params: RefCell::new(ref_params),
                known_names,
                creates: RefCell::new(HashSet::new()),
                writes: RefCell::new(HashSet::new()),
                incomplete: std::cell::Cell::new(false),
            }),
            bindings: Some(bindings),
            loop_context: None,
            current_range: range,
            traps,
            new_all: false,
            detached: false,
        };
        // A handler, or any nested label, may hand what it assigns on to another label or back to
        // its block, so its deferred reports can't be trusted.
        if is_handler || (may_exist && procedure_block) {
            flow.mark_incomplete();
        }
        handler(&mut flow)?;
        if flow.bindings.is_some() && auto_return {
            flow.handle_return(WithRange::new(
                None,
                bindings::Range {
                    start: range.end,
                    end: range.end,
                },
            ))?;
        }
        // Deferred until every split flow has run: still `false` means never read.
        if !flow.shared.incomplete.get() {
            let unread: Vec<(String, bindings::Range)> = {
                let list = flow.shared.reading_list.borrow();
                let unread = list.iter().filter(|(_, read)| !**read);
                unread.map(|(w, _)| (w.name.clone(), w.range)).collect()
            };
            for (name, range) in unread {
                flow.add_soft_diagnostic(TypeError::unread_assignment(name, range).into());
            }
            // The caller reads a by-reference parameter, and need not set an `Output` one, so the
            // body's own use says which mode fits: a `ByRef` written without its incoming value
            // ever being needed is `Output` in all but name, an `Output` read before it is
            // written is `ByRef`; an untouched `ByRef` is unused.
            for param in flow.shared.ref_params.borrow().iter() {
                let (name, range) = (param.name.clone(), param.range);
                let err = match (param.mode, param.needed, param.written, param.read) {
                    (RefMode::ByRef, false, true, _) if output_keyword => {
                        TypeError::by_ref_never_read(name, range)
                    }
                    (RefMode::ByRef, false, false, false) => {
                        TypeError::unread_assignment(name, range)
                    }
                    (RefMode::Output, true, true, _) => TypeError::output_read_first(name, range),
                    _ => continue,
                };
                flow.add_soft_diagnostic(err.into());
            }
        }
        // Likewise deferred: an `Output` parameter never assigned on any path leaves its caller an
        // undefined value.
        let ref_params = flow.shared.ref_params.borrow();
        if !flow.shared.incomplete.get()
            && let Some(param) = ref_params
                .iter()
                .find(|param| param.mode == RefMode::Output && !param.written)
        {
            return Err(TypeError::output_never_assigned(
                param.name.clone(),
                param.range,
            ));
        }
        drop(ref_params);
        let mut creates: Vec<String> = flow
            .shared
            .creates
            .borrow()
            .iter()
            .filter(|name| !flow.shared.known_names.contains(*name))
            .cloned()
            .collect();
        creates.sort();
        Ok(Effects {
            creates,
            writes: flow.shared.writes.borrow().clone(),
        })
    }

    pub(crate) fn with_loop(
        &mut self,
        range: bindings::Range,
        optional: bool,
        infinite: bool,
        env: &CodeEnv,
        handler: impl Fn(&mut Self) -> Result<(), TypeError>,
    ) -> Result<(), QuasiTypeError> {
        let before = self.split();
        let outer = self.loop_context.replace(LoopContext::new(range));
        let result = self.run_loop_passes(range, infinite, env, &before, handler);
        // Restored even when a pass failed: an enclosing `if` branch may swallow that failure
        // (`check_branch`) and carry on, and a leaked context would then trip its `merge`.
        let quits = std::mem::replace(&mut self.loop_context, outer).map(|context| context.quits);
        result?;
        for WithRange {
            it: bindings,
            range,
        } in quits.unwrap_or_default()
        {
            self.merge_bindings(Some(bindings)).with_range(|| range)?;
        }
        if optional {
            self.merge(before)?;
        }
        Ok(())
    }

    fn run_loop_passes(
        &mut self,
        range: bindings::Range,
        infinite: bool,
        env: &CodeEnv,
        before: &Self,
        handler: impl Fn(&mut Self) -> Result<(), TypeError>,
    ) -> Result<(), QuasiTypeError> {
        self.primed(env, &handler)?;
        // Run the body a second time, starting with union of all continue and the before
        for bindings in self.loop_context.take().unwrap().continues {
            self.merge_bindings(Some(bindings)).with_range(|| range)?;
        }
        self.merge_bindings(before.bindings.clone())
            .with_range(|| range)?;
        self.loop_context = Some(LoopContext::new(range));
        handler(self)?;
        // A `Continue` heads back to the loop head just like falling off the end of the body.
        for bindings in std::mem::take(&mut self.loop_context.as_mut().unwrap().continues) {
            self.merge_bindings(Some(bindings)).with_range(|| range)?;
        }
        if infinite {
            self.bindings = None;
        }
        Ok(())
    }

    // A `Try`/`Catch` block: an argumentless `Quit` leaves it (to just after the `Catch`), an
    // argumented one is IRIS's compile error (`quit_x_in_control`), and a `Continue` still
    // targets the enclosing loop.
    pub(crate) fn with_block(
        &mut self,
        range: bindings::Range,
        handler: impl FnOnce(&mut Self) -> Result<(), TypeError>,
    ) -> Result<(), QuasiTypeError> {
        let outer = self.loop_context.replace(LoopContext::new(range));
        let result = handler(self);
        let context = std::mem::replace(&mut self.loop_context, outer);
        result?;
        let Some(context) = context else {
            return Ok(());
        };
        for WithRange {
            it: bindings,
            range,
        } in context.quits
        {
            self.merge_bindings(Some(bindings)).with_range(|| range)?;
        }
        if !context.continues.is_empty() {
            match &mut self.loop_context {
                Some(outer) => outer.continues.extend(context.continues),
                None => return Err(QuasiTypeError::continue_not_in_loop()),
            }
        }
        Ok(())
    }

    // Merges a state split off inside the loop just left, as another way out of it: its loop
    // context is the loop's own (and empty, being split at the condition), not this flow's.
    pub(crate) fn merge_loop_exit(&mut self, mut them: Self) -> Result<(), QuasiTypeError> {
        them.loop_context = self
            .loop_context
            .as_ref()
            .map(|c| LoopContext::new(c.range));
        self.merge(them)
    }

    pub(crate) fn split(&mut self) -> Self {
        Self {
            shared: self.shared.clone(),
            bindings: self.bindings.clone(),
            loop_context: self
                .loop_context
                .as_ref()
                .map(|context| LoopContext::new(context.range)),
            current_range: self.current_range,
            traps: self.traps.clone(),
            new_all: self.new_all,
            detached: self.detached,
        }
    }

    // The condition is always `value`, so one branch is dead: the false one if it's true, else
    // this one.
    fn settle(&mut self, value: bool, report: bool) -> Flow<'a> {
        let mut false_branch = self.split();
        if report {
            self.add_soft_diagnostic(QuasiTypeError::condition_always(value));
        }
        if value {
            false_branch.bindings = None;
        } else {
            self.bindings = None;
        }
        false_branch
    }

    // `report`: raise `condition_always` on a statically true/false condition. Off for a
    // loop's own condition (`while 1 { ... quit }` is an idiom, not a mistake); still on for
    // an `If`.
    pub(crate) fn split_on(&mut self, condition_ty: PowerTy, report: bool) -> Self {
        match &condition_ty {
            PowerTy::Term { target, term } => {
                let mut false_branch = self.split();
                if let Some(bindings) = &mut self.bindings {
                    bindings.refine_on_prop(*target, term);
                }
                if let Some(bindings) = &mut false_branch.bindings {
                    bindings.refine_on_prop(*target, &term.clone().negate());
                }
                false_branch
            }
            // A bare `$DATA`-style case value is truthy exactly when it isn't 0.
            PowerTy::Case { target, .. } => self.split_on(PowerTy::bind(*target), report),
            // Never during priming: deadness here reflects pre-loop bindings a real iteration
            // can still widen (e.g. `Const("")` reassigned in the body), so killing a branch
            // now would wrongly cut the priming walk short.
            PowerTy::Const(c) if !self.shared.env.soft_diagnostics_suppressed() => {
                self.settle(c.truthy(), report)
            }
            PowerTy::Union(fst, snd, rst) if !self.shared.env.soft_diagnostics_suppressed() => {
                let cases: HashSet<bool> = once(fst)
                    .chain(once(snd))
                    .chain(rst.iter())
                    .map(Const::truthy)
                    .collect();
                match Vec::from_iter(cases)[..] {
                    [always] => self.settle(always, report),
                    _ => self.split(),
                }
            }
            _ => self.split(),
        }
    }

    fn merge_bindings(&mut self, that: Option<Bindings>) -> Result<(), QuasiTypeError> {
        let undeclared = self.undeclared();
        match &mut self.bindings {
            Some(this) => this.merge(that, self.shared.env, &undeclared),
            None => {
                self.bindings = that;
                Ok(())
            }
        }
    }

    pub(crate) fn merge(&mut self, them: Self) -> Result<(), QuasiTypeError> {
        self.merge_bindings(them.bindings)?;
        let undeclared = self.undeclared();
        self.traps.merge(them.traps, self.shared.env, &undeclared)?;
        self.new_all |= them.new_all;
        self.detached |= them.detached;
        match (&mut self.loop_context, them.loop_context) {
            (Some(mine), Some(theirs)) => {
                mine.quits.extend(theirs.quits);
                mine.continues.extend(theirs.continues);
            }
            (None, None) => {}
            (Some(_), None) | (None, Some(_)) => {
                return Err(QuasiTypeError::unimplemented(
                    "Internal error: inconsistent loop context".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn handle_continue(&mut self) -> Result<(), QuasiTypeError> {
        let bindings = self.bindings()?.clone();
        match &mut self.loop_context {
            Some(context) => {
                context.continues.push(bindings);
            }
            None => return Err(QuasiTypeError::continue_not_in_loop()),
        }
        self.bindings = None;
        Ok(())
    }

    pub(crate) fn handle_quit(
        &mut self,
        val: WithRange<Option<PowerTy>>,
    ) -> Result<(), QuasiTypeError> {
        let bindings = self.bindings()?.clone();
        let WithRange { it, range } = val;
        match (&mut self.loop_context, it) {
            // Not inside any loop: a quit is just a return.
            (None, it) => Ok(self.handle_return(WithRange::new(it, range))?),
            // A quit with a value only makes sense as a procedure-level return, not as a
            // loop's exit.
            (Some(context), Some(actual)) => Err(QuasiTypeError::quit_x_in_control(
                actual.to_ty(self.shared.env).unwrap_or_default(),
                Location::new(self.shared.env.uri().to_string(), context.range),
            )),
            // Breaking a loop hands control to whatever follows it, still within this same
            // procedure — our caller hasn't seen anything yet.
            (Some(context), None) => {
                context.quits.push(WithRange::new(bindings, range));
                self.bindings = None;
                Ok(())
            }
        }
    }

    pub(crate) fn handle_return(
        &mut self,
        val: WithRange<Option<PowerTy>>,
    ) -> Result<(), TypeError> {
        let WithRange { it, range } = val;
        self.read_olds().map_err(|err| err.into_error(|| range))?;
        if self.detached {
            self.bindings = None;
            return Ok(());
        }
        // A lazily-established kind uses `%Any`: we only need every branch to agree on
        // *whether* a value comes back, not on one exact type.
        let established = self.shared.output.borrow().clone();
        let established = established.unwrap_or_else(|| {
            let established = WithRange::new(it.as_ref().map(|_| Ty::Any), range);
            *self.shared.output.borrow_mut() = Some(established.clone());
            established
        });
        match (&established.it, it) {
            (Some(wanted), Some(actual)) => {
                self.shared.env.check_pattern(&actual, wanted, range, None);
                (self.shared.env.assert_value_compatible(&actual, wanted))
                    .or_else(|err| err.raise(self.shared.env, range, None))
            }
            (Some(wanted), None) => Err(QuasiTypeError::return_nothing_for_x(
                wanted.clone(),
                Location::new(self.shared.env.uri().to_string(), established.range),
            )),
            (None, actual) => {
                if let Some(actual) = actual {
                    self.add_soft_diagnostic(QuasiTypeError::return_x_for_nothing(
                        actual.to_ty(self.shared.env).unwrap_or_default(),
                        Location::new(self.shared.env.uri().to_string(), established.range),
                    ));
                }
                Ok(())
            }
        }
        .map_err(|err| err.into_error(|| range))?;
        self.bindings = None;
        Ok(())
    }

    // Whether a path still runs here: a `Quit`, `Return` or `Throw` on every way in ends it.
    pub(crate) fn is_live(&self) -> bool {
        self.bindings.is_some()
    }

    pub(crate) fn handle_void(&mut self) -> Result<(), TypeError> {
        self.bindings = None;
        Ok(())
    }

    // `Throw` always transfers control away -- same "this path is done" effect as
    // `handle_quit`/`handle_return`, minus their output-type/loop-context bookkeeping.
    pub(crate) fn handle_throw(&mut self) -> Result<(), QuasiTypeError> {
        self.bindings = None;
        Ok(())
    }

    // An error raised on purpose: from inside the handler the trap still names, it comes right
    // back, unless a `Try` around it catches it first.
    pub(crate) fn raise(&mut self, in_try: bool) -> Result<(), QuasiTypeError> {
        if let Some((var, None)) = self.traps.active
            && !in_try
        {
            self.add_soft_diagnostic(TypeErrorKind::TrapReenters { var: var.name() }.into());
        }
        self.handle_throw()
    }

    // `var` now names `label`, a handler of this body. A trap can fire anywhere from here on, so
    // this state and each one after it (`step_traps`) join into what the handler is checked from;
    // it may also read what this body assigns, so the deferred reports can't be trusted.
    pub(crate) fn arm_trap(&mut self, var: TrapVar, label: String) -> Result<(), QuasiTypeError> {
        let bindings = self.bindings()?.clone();
        let undeclared = self.undeclared();
        self.mark_incomplete();
        self.traps.active = Some((var, Some(label.clone())));
        match self.traps.joins.get_mut(&label) {
            Some(join) => join
                .bindings
                .merge(Some(bindings), self.shared.env, &undeclared)?,
            None => {
                self.traps.joins.insert(label, Join { var, bindings });
            }
        }
        Ok(())
    }

    // `var` is cleared or unknown. `$ZTRAP` takes precedence, so clearing `$ETRAP` under it
    // changes nothing.
    pub(crate) fn disarm_trap(&mut self, var: TrapVar) {
        if matches!(self.traps.active, Some((active, _)) if active == var) {
            self.traps.active = None;
        }
    }

    pub(crate) fn step_traps(&mut self) -> Result<(), QuasiTypeError> {
        let undeclared = self.undeclared();
        let Some((_, Some(label))) = &self.traps.active else {
            return Ok(());
        };
        let Some(join) = self.traps.joins.get_mut(label) else {
            return Ok(());
        };
        join.bindings
            .merge(self.bindings.clone(), self.shared.env, &undeclared)
    }

    // The handlers this body armed, each with its join, for `CodeEnv::deposit_traps`.
    pub(crate) fn take_traps(&mut self) -> HashMap<String, Join> {
        std::mem::take(&mut self.traps.joins)
    }

    fn bindings(&mut self) -> Result<&mut Bindings, QuasiTypeError> {
        self.bindings
            .as_mut()
            .ok_or_else(QuasiTypeError::unreachable)
    }

    fn lookup(&mut self, name: String) -> Result<&mut Binding, QuasiTypeError> {
        let undeclared = self.undeclared();
        Ok(self.bindings()?.entry(name, || undeclared))
    }

    // The binding of a name this flow has not seen: private in a procedure block, public outside
    // one; holding a value it may already have (see `Shared::may_exist`), unless a bare `New` has
    // made this frame the home of every name from here on.
    fn undeclared(&self) -> Binding {
        let private = self.shared.env.in_procedure_block();
        Binding::undeclared(
            private,
            self.shared.may_exist && !self.new_all,
            private || self.new_all,
        )
    }

    // Unconditionally (re-)declares `name`, overwriting any existing binding — must be able to
    // re-scope an existing parameter or public variable, not just a fresh name. `New` carries no
    // type, so there's nothing to take but the range.
    pub(crate) fn handle_new(
        &mut self,
        name: String,
        range: bindings::Range,
    ) -> Result<(), QuasiTypeError> {
        if is_percent_variable(&name) {
            return Ok(());
        }
        // IRIS refuses to `New` a private variable of a procedure block (#1038), parameters
        // included, in every frame of the block; only a public one may be.
        if self.lookup(name.clone())?.private {
            self.add_soft_diagnostic(QuasiTypeError::WithRange(
                TypeError::new_in_procedure_block(name, range),
            ));
            return Ok(());
        }
        let uri = self.shared.env.uri().to_string();
        let binding = self.lookup(name.clone())?;
        let previous = binding.new_at;
        binding.new = Some(true);
        binding.new_at = Some(range);
        // Like `Kill`: undefined until set again.
        binding.assigned = PowerTy::undefined();
        binding.assigned_at = HashSet::new();
        binding.subscript_assigned = None;
        if let Some(previous) = previous {
            self.add_soft_diagnostic(QuasiTypeError::WithRange(TypeError::redundant_new(
                name,
                range,
                Location::new(uri, previous),
            )));
        }
        Ok(())
    }

    // The bare and exclusive forms `New` every public not kept, undefined from here, and make this
    // frame the home of every name created later. A procedure block's privates are untouched, and
    // naming one is the error `New` of it gives (#1038).
    pub(crate) fn handle_new_except(
        &mut self,
        keep: &[String],
        range: bindings::Range,
    ) -> Result<(), QuasiTypeError> {
        for name in keep {
            if !is_percent_variable(name) && self.lookup(name.clone())?.private {
                self.add_soft_diagnostic(QuasiTypeError::WithRange(
                    TypeError::new_in_procedure_block(name.clone(), range),
                ));
            }
        }
        for name in self.other_names(keep)? {
            let binding = self.lookup(name)?;
            if binding.private {
                continue;
            }
            binding.new = Some(true);
            binding.new_at = Some(range);
            binding.assigned = PowerTy::undefined();
            binding.assigned_at = HashSet::new();
            binding.subscript_assigned = None;
        }
        self.new_all = true;
        Ok(())
    }

    pub(crate) fn invalidate_all(&mut self) -> Result<(), QuasiTypeError> {
        for name in self.other_names(&[])? {
            self.invalidate(Local::bare(name), Ty::Any.into())?;
        }
        Ok(())
    }

    pub(crate) fn handle_kill_except(&mut self, keep: &[String]) -> Result<(), QuasiTypeError> {
        for name in self.other_names(keep)? {
            self.handle_kill(Local::bare(name))?;
        }
        Ok(())
    }

    fn other_names(&mut self, keep: &[String]) -> Result<Vec<String>, QuasiTypeError> {
        Ok(self
            .bindings()?
            .bindings
            .keys()
            .filter(|name| !keep.contains(name) && !is_percent_variable(name))
            .cloned()
            .collect())
    }

    pub(crate) fn has_binding(&mut self, name: &str) -> Result<bool, QuasiTypeError> {
        Ok(self.bindings()?.bindings.contains_key(name))
    }

    pub(crate) fn range(&self) -> bindings::Range {
        self.current_range
    }

    // #Dim attaches a compile-time type annotation; unlike `New` it doesn't affect scope or
    // runtime value — an overlay on whatever binding already exists, defaulting to an ordinary
    // local if there wasn't one.
    pub(crate) fn handle_dim(
        &mut self,
        name: String,
        ty: WithRange<Ty>,
    ) -> Result<(), QuasiTypeError> {
        self.lookup(name)?.declared = Some(ty);
        Ok(())
    }

    pub(crate) fn handle_set(
        &mut self,
        local: Local,
        ty: PowerTy,
        range: bindings::Range,
    ) -> Result<(), QuasiTypeError> {
        let Local { name, subs } = local;
        self.note_ref_param(&name, RefUse::Set)?;
        let env = self.shared.env;
        let ty = match ty.to_maybe_ty(env).maybe_defined() {
            Some(_) => ty,
            None => {
                self.add_soft_diagnostic(QuasiTypeError::undefined());
                PowerTy::plain(Ty::Any)
            }
        };
        let binding = self.lookup(name.clone())?;
        let new = binding.new;
        if subs.is_empty() {
            let declared = binding
                .declared
                .as_ref()
                .map_or(&Ty::Any, |declared| &declared.it);
            env.check_pattern(&ty, declared, range, Some(&name));
            (env.assert_value_compatible(&ty, declared))
                .or_else(|err| err.raise(env, range, Some(name.clone())))?;
            binding.assigned = ty;
            binding.assigned_at = HashSet::from([range]);
            binding.incoming = false;
            // `or_insert`, not a blind `insert`: the only way this exact (name, range) pair is
            // set twice is the loop body being simulated twice (throwaway pass, then real) — if
            // the throwaway write was already marked read, that must survive the real pass
            // re-executing the same `Set`.
            self.shared
                .reading_list
                .borrow_mut()
                .entry(Written {
                    name: name.clone(),
                    range,
                })
                .or_insert(false);
        } else {
            binding.subscript_assigned = Some(());
        }
        self.note_write(&name, new);
        Ok(())
    }

    fn note_write(&self, name: &str, new: Option<bool>) {
        self.shared.writes.borrow_mut().insert(name.to_string());
        if new != Some(true) {
            self.shared.creates.borrow_mut().insert(name.to_string());
        }
    }

    // Narrows what is known about `local` without counting as a write (`$DATA`-style probes
    // only observe definedness, so they must not show up as assignments).
    pub(crate) fn handle_refine(
        &mut self,
        Local { name, subs }: Local,
        ty: PowerTy,
    ) -> Result<(), QuasiTypeError> {
        if subs.is_empty() {
            self.lookup(name.clone())?.assigned = ty;
        }
        Ok(())
    }

    // MERGE always writes opaquely into the target's subtree, regardless of its own literal
    // subscript depth — unlike `handle_set`, which only takes the subtree branch when the
    // reference itself was subscripted. The node comes along only if the source has one.
    pub(crate) fn handle_merge(
        &mut self,
        local: Local,
        source: PowerTy,
        range: bindings::Range,
    ) -> Result<(), QuasiTypeError> {
        self.note_ref_param(&local.name, RefUse::Set)?;
        let env = self.shared.env;
        let node = match source.to_maybe_ty(env) {
            MaybeTy::Undefined => None,
            MaybeTy::Plain(_) => Some(source),
            MaybeTy::Maybe(_) => Some(env.merge_power_ty(self.handle_get(local.clone())?, source)),
        };
        if let Some(node) = node {
            self.handle_set(local.clone(), node, range)?;
        }
        let binding = self.lookup(local.name.clone())?;
        binding.subscript_assigned = Some(());
        let is_new = binding.new;
        if is_new != Some(true) {
            self.shared.creates.borrow_mut().insert(local.name);
        }
        Ok(())
    }

    // A callee may have mutated this variable on our behalf — not a real assignment in the
    // caller's source, so it doesn't need a fresh read to avoid the "unread" lint. Still counts
    // as consuming whatever was pending, and marks the subscript as assigned since every call
    // site wants both. Ignores `local.subs`: a callee-mutated variable is invalidated wholesale
    // either way.
    pub(crate) fn invalidate(&mut self, local: Local, ty: PowerTy) -> Result<(), QuasiTypeError> {
        self.invalidate_used(local, ty, RefUse::Ref)
    }

    // The same for a callee's `Output` parameter, which disclaims the prior value: no read of
    // what came in, so handing on one of our own `Output` params is not A7781.
    pub(crate) fn invalidate_output(
        &mut self,
        local: Local,
        ty: PowerTy,
    ) -> Result<(), QuasiTypeError> {
        self.invalidate_used(local, ty, RefUse::Get)
    }

    fn invalidate_used(
        &mut self,
        local: Local,
        ty: PowerTy,
        used: RefUse,
    ) -> Result<(), QuasiTypeError> {
        self.note_ref_param(&local.name, used)?;
        self.note_ref_param(&local.name, RefUse::Set)?;
        self.read(&local.name)?;
        let binding = self.lookup(local.name.clone())?;
        binding.assigned = ty;
        binding.subscript_assigned = Some(());
        let new = binding.new;
        // Delegating one of our own `Output` params to a callee (`Do Other(.x)`) counts as
        // assigning it — the callee is trusted to write it, same as any other by-ref/output arg.
        self.note_write(&local.name, new);
        Ok(())
    }

    // Killing the whole variable wipes it entirely, including subscript knowledge. Killing a
    // specific path only removes that path — since individual subscripts aren't tracked, the
    // scalar and other subscripts might still exist, so both are left untouched.
    pub(crate) fn handle_kill(&mut self, local: Local) -> Result<(), QuasiTypeError> {
        self.note_ref_param(&local.name, RefUse::Set)?;
        if local.subs.is_empty() {
            let binding = self.lookup(local.name)?;
            binding.assigned = PowerTy::undefined();
            binding.assigned_at = HashSet::new();
            binding.subscript_assigned = None;
            binding.incoming = false;
        }
        Ok(())
    }

    // ZKILL removes only a node's own value, never its descendants — unlike `KILL`, this never
    // touches `subscript_assigned`, regardless of whether the target itself was subscripted.
    pub(crate) fn handle_zkill(&mut self, local: Local) -> Result<(), QuasiTypeError> {
        self.note_ref_param(&local.name, RefUse::Set)?;
        if local.subs.is_empty() {
            let binding = self.lookup(local.name)?;
            binding.assigned = PowerTy::undefined();
            binding.assigned_at = HashSet::new();
        }
        Ok(())
    }

    pub(crate) fn read_publics(&mut self) -> Result<(), QuasiTypeError> {
        for name in self.bindings()?.publics() {
            self.read(&name)?
        }
        Ok(())
    }

    pub(crate) fn read_write_publics(&mut self) -> Result<(), QuasiTypeError> {
        for name in self.bindings()?.publics() {
            self.invalidate(Local::bare(name), Ty::Any.into())?;
        }
        Ok(())
    }

    fn read_olds(&mut self) -> Result<(), QuasiTypeError> {
        for name in self.bindings()?.olds() {
            self.read(&name)?
        }
        Ok(())
    }

    // Records a use of `name` if it is a parameter passed by reference; every read and write of
    // a local passes through here. A read needs the incoming value only while the binding still
    // holds it, i.e. before this path's first write.
    fn note_ref_param(&mut self, name: &str, used: RefUse) -> Result<(), QuasiTypeError> {
        let Some(i) = self
            .shared
            .ref_params
            .borrow()
            .iter()
            .position(|param| param.name == name)
        else {
            return Ok(());
        };
        let incoming = self.lookup(name.to_string())?.incoming;
        let mut params = self.shared.ref_params.borrow_mut();
        let param = &mut params[i];
        match used {
            RefUse::Ref => {
                param.needed |= incoming;
                param.read = true;
            }
            RefUse::Get => param.read = true,
            RefUse::Set => param.written = true,
        }
        Ok(())
    }

    pub(crate) fn read(&mut self, name: &str) -> Result<(), QuasiTypeError> {
        let binding = self.lookup(name.to_string())?;
        for pending in std::mem::take(&mut binding.assigned_at) {
            if let Some(read) = self.shared.reading_list.borrow_mut().get_mut(&Written {
                name: name.to_string(),
                range: pending,
            }) {
                *read = true;
            }
        }
        Ok(())
    }

    pub(crate) fn handle_ref(&mut self, local: Local) -> Result<PowerTy, QuasiTypeError> {
        self.handle_access(local, RefUse::Ref)
    }

    // A read that tolerates an absent value (`$Get`, `$Data`, a target's old value).
    pub(crate) fn handle_get(&mut self, local: Local) -> Result<PowerTy, QuasiTypeError> {
        self.handle_access(local, RefUse::Get)
    }

    fn handle_access(
        &mut self,
        Local { name, subs }: Local,
        used: RefUse,
    ) -> Result<PowerTy, QuasiTypeError> {
        self.note_ref_param(&name, used)?;
        self.read(&name)?;
        let binding = self.lookup(name)?;
        Ok(if subs.is_empty() {
            binding.assigned.clone()
        } else if let Some(()) = binding.subscript_assigned {
            Ty::Any.into()
        } else {
            PowerTy::undefined()
        })
    }

    pub(crate) fn subscriptable(&mut self, name: &str) -> Result<bool, QuasiTypeError> {
        Ok(self.lookup(name.to_string())?.subscript_assigned.is_some())
    }

    pub(crate) fn is_public(&mut self, name: &str) -> Result<bool, QuasiTypeError> {
        Ok(!self.lookup(name.to_string())?.private)
    }
}

#[derive(Clone)]
struct Binding {
    declared: Option<WithRange<Ty>>,
    assigned: PowerTy,
    // A set: both sides of a merge usually carry the same pending writes, and a `Vec` doubling
    // on every `If` merge under nested loops is exponential (#42).
    assigned_at: HashSet<bindings::Range>,
    subscript_assigned: Option<()>,
    private: bool,
    // `None` means two merged branches disagree on whether this was `New`'d.
    new: Option<bool>,
    // Where the most recent explicit `New` (re-)scoped this variable, if any — `None` when
    // `new: Some(true)` came from elsewhere (a by-value parameter, or an undeclared variable's
    // default in a procedure-block routine). Only an explicit `New` on top of another explicit
    // `New` is reported; re-scoping a parameter/public variable is legitimate (see `handle_new`).
    new_at: Option<bindings::Range>,
    // A parameter passed by reference that this body hasn't written yet (see `RefParam`).
    incoming: bool,
}

// Untracked, like a global: `Variable::as_local` never yields one, by-name entry points skip it.
pub(crate) fn is_percent_variable(name: &str) -> bool {
    name.starts_with('%')
}

impl Binding {
    fn public(range: bindings::Range) -> Binding {
        Binding {
            assigned: Ty::Any.into(),
            subscript_assigned: Some(()),
            new: Some(false),
            new_at: None,
            incoming: false,
            private: false,
            declared: None,
            assigned_at: HashSet::from([range]),
        }
    }

    fn undeclared(private: bool, exists: bool, new: bool) -> Self {
        Self {
            declared: None,
            assigned: if exists {
                Ty::Any.into()
            } else {
                PowerTy::undefined()
            },
            assigned_at: HashSet::new(),
            subscript_assigned: exists.then_some(()),
            private,
            new: Some(new),
            new_at: None,
            incoming: false,
        }
    }

    // `declared`/`private` are set once and must agree across branches; `assigned`,
    // `subscript_assigned`, and `pending` are genuinely joined. `new` disagreeing isn't an
    // error by itself — it's only reported if the variable is used again, at that later use
    // site.
    fn merge(self, that: Self, env: &WorkspaceEnv, name: &str) -> Result<Self, QuasiTypeError> {
        // Compared by type only: each branch's `New`/`#Dim` carries its own source range, so
        // comparing the whole `WithRange` would falsely disagree even when the type matches.
        // A `#Dim` in only one branch is scoped to it, like a `New` there: past the merge the
        // variable is undeclared again. Only two different declared types disagree.
        let declared = match (self.declared, that.declared) {
            (Some(this_d), Some(that_d)) if this_d.it == that_d.it => Some(this_d),
            (Some(_), None) | (None, Some(_)) | (None, None) => None,
            (this_d, that_d) => {
                return Err(QuasiTypeError::inconsistent_declaration(
                    name.to_string(),
                    this_d.map(|d| d.it),
                    that_d.map(|d| d.it),
                ));
            }
        };
        let new = match (self.new, that.new) {
            (Some(this_new), Some(that_new)) if this_new == that_new => Some(this_new),
            _ => None,
        };
        // Ambiguous after a merge either way — which branch's `New` (if any) "most recently"
        // scoped this is no longer a single, reportable fact.
        let new_at = match (self.new_at, that.new_at) {
            (Some(this_at), Some(that_at)) if this_at == that_at => Some(this_at),
            _ => None,
        };
        let mut pending = self.assigned_at;
        pending.extend(that.assigned_at);
        Ok(Self {
            declared,
            assigned: env.merge_power_ty(self.assigned, that.assigned),
            assigned_at: pending,
            subscript_assigned: self.subscript_assigned.or(that.subscript_assigned),
            private: self.private,
            new,
            new_at,
            incoming: self.incoming || that.incoming,
        })
    }
}

#[derive(Clone, Default)]
pub struct Bindings {
    bindings: HashMap<String, Binding>,
}

impl Bindings {
    pub fn publics(&self) -> Vec<String> {
        self.names_where(|binding| !binding.private)
    }

    pub fn olds(&self) -> Vec<String> {
        self.names_where(|binding| binding.new != Some(true))
    }

    fn names_where(&self, keep: impl Fn(&Binding) -> bool) -> Vec<String> {
        self.bindings
            .iter()
            .filter(|(_, binding)| keep(binding))
            .map(|(name, _)| name.clone())
            .collect()
    }

    // Default is a closure, not a stored `procedure_block`, since only `Flow::lookup` knows it.
    fn entry(&mut self, name: String, default: impl FnOnce() -> Binding) -> &mut Binding {
        self.bindings.entry(name).or_insert_with(default)
    }

    // Merges another path's state in: a name bound on one side only is undeclared on the other.
    fn merge(
        &mut self,
        that: Option<Bindings>,
        env: &WorkspaceEnv<'_>,
        undeclared: &Binding,
    ) -> Result<(), QuasiTypeError> {
        let Some(mut that) = that else {
            return Ok(());
        };
        // Sorted, so which inconsistent declaration is reported first doesn't depend on hash order.
        let all_names: BTreeSet<String> = self
            .bindings
            .keys()
            .chain(that.bindings.keys())
            .cloned()
            .collect();
        for name in all_names {
            let this_binding = self
                .bindings
                .remove(&name)
                .unwrap_or_else(|| undeclared.clone());
            let that_binding = that
                .bindings
                .remove(&name)
                .unwrap_or_else(|| undeclared.clone());
            let merged = this_binding.merge(that_binding, env, &name)?;
            self.bindings.insert(name, merged);
        }
        Ok(())
    }

    fn refine_on_prop(&mut self, target: u32, term: &Term) {
        for binding in self.bindings.values_mut() {
            binding.assigned = binding.assigned.clone().refine_on_prop(target, term);
        }
    }
}
