//! Flow-sensitive state for one code block: which local variables are defined on the current
//! path, with what type, and whether they have been read yet.
//!
//! `Flow` is forked at `If`/`Else`, loops, `Try`/`Catch` and `Quit`/`Return`, and the branches
//! are merged back with `merge_all_bindings`. That merge is where "may be undefined", "assigned
//! but never read" and "inconsistent declaration across branches" diagnostics come from. Driven
//! by the command rules in `mod.rs`; produces `types.rs` values; reports through `errors.rs`.

use crate::bindings;
use crate::bindings::ArgMode;
use crate::bindings::Location;
use crate::check::env::*;
use crate::check::errors::*;
use crate::check::types::*;
use crate::details::WithRange;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
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

struct Shared<'a> {
    uri: &'a str,
    env: &'a WorkspaceEnv<'a>,
    procedure_block: bool,
    // `None` is for subroutines and procedures only. It means that the output expectation
    // is unclear until the first return/quit command. Shared (the `Rc`) with the labels that
    // return for this body (see `check_block_body`).
    output: OutputExpectation,
    ty_var: RefCell<u32>,
    reading_list: RefCell<HashMap<Written, bool>>,
    // Kept in declaration order for a deterministic diagnostic when more than one is never
    // assigned.
    output_params: Vec<(String, bindings::Range)>,
    output_written: RefCell<HashSet<String>>,
    // Visible in the signature already, so excluded from `creates` below.
    known_names: HashSet<String>,
    // Every not-`New`'d (`new != Some(true)`) variable written anywhere in the procedure, i.e.
    // one whose value silently leaks into the caller's scope on return.
    creates: RefCell<HashSet<String>>,
    // Set once an `if` branch's checking failed partway (see `check_branch`): the deferred "never
    // read"/"never assigned" reports can't be trusted after that.
    incomplete: std::cell::Cell<bool>,
}

pub(crate) type OutputExpectation = Rc<RefCell<Option<WithRange<Option<Ty>>>>>;

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

// The life time 'a is for the entire code block (i.e., method body or a stand-alone expression)
pub struct Flow<'a> {
    shared: Rc<Shared<'a>>,
    // `None` once this path has concluded (`Return`, `Quit`, or otherwise) — every operation
    // that ends a path must clear this. Reading past that point is dead code (see `bindings()`).
    bindings: Option<Bindings>,
    loop_context: Option<LoopContext>,
    current_range: bindings::Range,
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

    pub(crate) fn mark_incomplete(&self) {
        self.shared.incomplete.set(true);
    }

    // Brackets a throwaway priming pass: returns the previous suppression state, to be restored
    // afterward via `restore_diagnostics`.
    pub(crate) fn suppress_diagnostics(&self) -> bool {
        self.shared.env.suppress_soft_diagnostics()
    }

    pub(crate) fn restore_diagnostics(&self, was_suppressed: bool) {
        self.shared.env.restore_soft_diagnostics(was_suppressed);
    }

    pub(crate) fn with_subroutine(
        uri: &'a str,
        env: &'a WorkspaceEnv,
        range: bindings::Range,
        args: Vec<WithRange<NormalArg>>,
        handler: impl FnOnce(&mut Self) -> Result<(), TypeError>,
    ) -> Result<Vec<String>, TypeError> {
        Self::with_label(uri, env, range, args, None, handler).map(|(creates, _)| creates)
    }

    // A label. `carried` is the output expectation to share (e.g. the previous label's, when
    // it fell through into this one); hands its own on when this one falls through.
    pub(crate) fn with_label(
        uri: &'a str,
        env: &'a WorkspaceEnv,
        range: bindings::Range,
        args: Vec<WithRange<NormalArg>>,
        carried: Option<OutputExpectation>,
        handler: impl FnOnce(&mut Self) -> Result<(), TypeError>,
    ) -> Result<(Vec<String>, Option<OutputExpectation>), TypeError> {
        let output = carried.unwrap_or_default();
        let (creates, fell_through) = Self::run(
            uri,
            env,
            range,
            args,
            None,
            output.clone(),
            handler,
            false,
            false,
        )?;
        Ok((creates, fell_through.then_some(output)))
    }

    pub(crate) fn output_expectation(&self) -> OutputExpectation {
        self.shared.output.clone()
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn with_procedure(
        uri: &'a str,
        env: &'a WorkspaceEnv,
        range: bindings::Range,
        args: Vec<WithRange<NormalArg>>,
        public_list: ProcedureBlock,
        output: WithRange<Option<Option<Ty>>>,
        handler: impl FnOnce(&mut Self) -> Result<(), TypeError>,
        auto_return: bool,
        // The parameter list is dictated by an inherited declaration (an override), so a
        // parameter the body never reads -- or an `Output` one it never assigns -- isn't a
        // mistake of this method's.
        inherited_signature: bool,
    ) -> Result<Vec<String>, TypeError> {
        let output = Rc::new(RefCell::new(
            output.it.map(|it| WithRange::new(it, output.range)),
        ));
        Self::run(
            uri,
            env,
            range,
            args,
            public_list,
            output,
            handler,
            auto_return,
            inherited_signature,
        )
        .map(|(creates, _)| creates)
    }

    // Runs `handler` in a fresh flow for a body with `args`; returns what the body creates, and
    // whether it fell off its end (into whatever label follows it).
    #[allow(clippy::too_many_arguments)]
    fn run(
        uri: &'a str,
        env: &'a WorkspaceEnv,
        range: bindings::Range,
        args: Vec<WithRange<NormalArg>>,
        public_list: ProcedureBlock,
        output: OutputExpectation,
        handler: impl FnOnce(&mut Self) -> Result<(), TypeError>,
        auto_return: bool,
        inherited_signature: bool,
    ) -> Result<(Vec<String>, bool), TypeError> {
        let procedure_block = public_list.is_some();
        let mut bindings = HashMap::new();
        let mut reading_list = HashMap::new();
        let mut output_params = Vec::new();
        let mut known_names = HashSet::new();
        for WithRange {
            it:
                NormalArg {
                    mode,
                    name,
                    ty,
                    variadic,
                    ..
                },
            range,
        } in args
        {
            known_names.insert(name.clone());
            if is_percent_variable(&name) {
                continue;
            }
            let binding = {
                let (assigned, subscript_assigned, new) = match mode {
                    ArgMode::Default => {
                        reading_list.insert(Written::new(name.clone(), range), inherited_signature);
                        let subscripts = variadic.then_some(());
                        (ty.clone().unwrap_or_default().into(), subscripts, true)
                    }
                    ArgMode::ByRef => {
                        reading_list.insert(Written::new(name.clone(), range), inherited_signature);
                        (ty.clone().unwrap_or_default().into(), Some(()), false)
                    }
                    ArgMode::Output => {
                        if !inherited_signature {
                            output_params.push((name.clone(), range));
                        }
                        (
                            MaybeTy::Maybe(ty.clone().unwrap_or_default()).into(),
                            Some(()),
                            false,
                        )
                    }
                };
                Binding {
                    assigned,
                    subscript_assigned,
                    new: Some(new),
                    new_at: None,
                    private: procedure_block,
                    declared: Some(WithRange::new(ty.clone().unwrap_or_default(), range)),
                    assigned_at: HashSet::from([range]),
                }
            };
            if let Some(previous_definition) = bindings.insert(name.clone(), binding) {
                return Err(TypeError::redeclare_variable(
                    name,
                    range,
                    Location::new(uri.to_string(), previous_definition.declared.unwrap().range),
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
        let mut flow = Self {
            shared: Rc::new(Shared {
                uri,
                env,
                procedure_block,
                output,
                ty_var: RefCell::new(0),
                reading_list: RefCell::new(reading_list),
                output_params,
                output_written: RefCell::new(HashSet::new()),
                known_names,
                creates: RefCell::new(HashSet::new()),
                incomplete: std::cell::Cell::new(false),
            }),
            bindings: Some(Bindings { bindings }),
            loop_context: None,
            current_range: range,
        };
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
        // Bad style rather than a fault: the next label's code simply runs, so the deferred
        // checks below still apply.
        let fell_through = flow.bindings.is_some();
        if fell_through {
            flow.add_soft_diagnostic(TypeError::missing_quit(range).into());
        }
        // Deferred until every split flow has run: still `false` means never read.
        if !flow.shared.incomplete.get()
            && let Some((w, _)) = flow
                .shared
                .reading_list
                .borrow()
                .iter()
                .find(|(_, read)| !**read)
        {
            flow.add_soft_diagnostic(TypeError::unread_assignment(w.name.clone(), w.range).into());
        }
        // Likewise deferred: an `Output` parameter absent from `output_written` was never
        // assigned on any path, so its caller gets an undefined value back.
        let output_written = flow.shared.output_written.borrow();
        if !flow.shared.incomplete.get()
            && let Some((name, range)) = flow
                .shared
                .output_params
                .iter()
                .find(|(name, _)| !output_written.contains(name))
        {
            return Err(TypeError::output_never_assigned(name.clone(), *range));
        }
        let mut creates: Vec<String> = flow
            .shared
            .creates
            .borrow()
            .iter()
            .filter(|name| !flow.shared.known_names.contains(*name))
            .cloned()
            .collect();
        creates.sort();
        Ok((creates, fell_through))
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
            merge_all_bindings(
                &mut self.bindings,
                Some(bindings),
                self.shared.env,
                self.shared.procedure_block,
            )
            .with_range(|| range)?;
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
        let was_suppressed = self.suppress_diagnostics();
        let result = env.with_hints_suppressed(|| handler(self));
        self.restore_diagnostics(was_suppressed);
        result?;
        // Run the body a second time, starting with union of all continue and the before
        for bindings in self.loop_context.take().unwrap().continues {
            merge_all_bindings(
                &mut self.bindings,
                Some(bindings),
                self.shared.env,
                self.shared.procedure_block,
            )
            .with_range(|| range)?;
        }
        merge_all_bindings(
            &mut self.bindings,
            before.bindings.clone(),
            self.shared.env,
            self.shared.procedure_block,
        )
        .with_range(|| range)?;
        self.loop_context = Some(LoopContext::new(range));
        handler(self)?;
        // A `Continue` heads back to the loop head just like falling off the end of the body.
        for bindings in self.loop_context.as_mut().unwrap().continues.drain(..) {
            merge_all_bindings(
                &mut self.bindings,
                Some(bindings),
                self.shared.env,
                self.shared.procedure_block,
            )
            .with_range(|| range)?;
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
            merge_all_bindings(
                &mut self.bindings,
                Some(bindings),
                self.shared.env,
                self.shared.procedure_block,
            )
            .with_range(|| range)?;
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
        }
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
                let mut false_branch = self.split();
                let cases = c.as_integer() > 0;
                if report {
                    self.add_soft_diagnostic(QuasiTypeError::condition_always(cases));
                }
                if cases {
                    false_branch.bindings = None;
                } else {
                    self.bindings = None;
                }
                false_branch
            }
            PowerTy::Union(fst, snd, rst) if !self.shared.env.soft_diagnostics_suppressed() => {
                let mut false_branch = self.split();
                let mut cases = HashSet::new();
                cases.insert(fst.as_integer() > 0);
                cases.insert(snd.as_integer() > 0);
                for rst in rst {
                    cases.insert(rst.as_integer() > 0);
                }
                if cases.len() == 1 {
                    let cases = *cases.iter().next().unwrap();
                    if report {
                        self.add_soft_diagnostic(QuasiTypeError::condition_always(cases));
                    }
                    if cases {
                        false_branch.bindings = None;
                    } else {
                        self.bindings = None;
                    }
                }
                false_branch
            }
            _ => self.split(),
        }
    }

    pub(crate) fn merge(&mut self, them: Self) -> Result<(), QuasiTypeError> {
        merge_all_bindings(
            &mut self.bindings,
            them.bindings,
            self.shared.env,
            self.shared.procedure_block,
        )?;
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
                Location::new(self.shared.uri.to_string(), context.range),
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
        // A lazily-established kind uses `%Any`: we only need every branch to agree on
        // *whether* a value comes back, not on one exact type.
        let established = self.shared.output.borrow().clone();
        let established = established.unwrap_or_else(|| {
            let established = WithRange::new(it.as_ref().map(|_| Ty::Any), range);
            *self.shared.output.borrow_mut() = Some(established.clone());
            established
        });
        match (&established.it, it) {
            (Some(wanted), Some(actual)) => self
                .shared
                .env
                .assert_value_compatible(&actual, wanted)
                .map_err(Incompatible::general),
            (Some(wanted), None) => Err(QuasiTypeError::return_nothing_for_x(
                wanted.clone(),
                Location::new(self.shared.uri.to_string(), established.range),
            )),
            (None, actual) => {
                if let Some(actual) = actual {
                    self.add_soft_diagnostic(QuasiTypeError::return_x_for_nothing(
                        actual.to_ty(self.shared.env).unwrap_or_default(),
                        Location::new(self.shared.uri.to_string(), established.range),
                    ));
                }
                Ok(())
            }
        }
        .map_err(|err| err.into_error(|| range))?;
        self.bindings = None;
        Ok(())
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

    fn bindings(&mut self) -> Result<&mut Bindings, QuasiTypeError> {
        self.bindings
            .as_mut()
            .ok_or_else(QuasiTypeError::unreachable)
    }

    // The only place `procedure_block` is needed: it decides the default binding for a
    // never-declared variable.
    fn lookup(&mut self, name: String) -> Result<&mut Binding, QuasiTypeError> {
        let procedure_block = self.shared.procedure_block;
        Ok(self
            .bindings()?
            .entry(name, || Binding::undeclared(procedure_block)))
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
        // Every local in a procedure block is already implicitly `New`'d on entry (see
        // `Binding::undeclared`), so an explicit `New` there is a no-op — except a by-ref/
        // `Output`/`Public` binding (`new` starts `false`) or a by-value parameter (legitimately
        // re-`New`'d to discard the caller's argument), which the checks below still exempt.
        if self.shared.procedure_block {
            if !self.shared.known_names.contains(&name)
                && self.lookup(name.clone())?.new == Some(true)
            {
                self.add_soft_diagnostic(QuasiTypeError::WithRange(
                    TypeError::new_in_procedure_block(name.clone(), range),
                ));
            }
            let binding = self.lookup(name)?;
            binding.new = Some(true);
            binding.new_at = Some(range);
            return Ok(());
        }
        let uri = self.shared.uri.to_string();
        let binding = self.lookup(name.clone())?;
        if let Some(previous) = binding.new_at {
            return Err(QuasiTypeError::WithRange(TypeError::redundant_new(
                name,
                range,
                Location::new(uri, previous),
            )));
        }
        binding.new = Some(true);
        binding.new_at = Some(range);
        // Like `Kill`: undefined until set again.
        binding.assigned = PowerTy::undefined();
        binding.assigned_at = HashSet::new();
        binding.subscript_assigned = None;
        Ok(())
    }

    pub(crate) fn handle_new_except(
        &mut self,
        keep: &[String],
        range: bindings::Range,
    ) -> Result<(), QuasiTypeError> {
        for name in self.other_names(keep)? {
            let binding = self.lookup(name)?;
            if binding.new != Some(true) {
                binding.new = Some(true);
                binding.new_at = Some(range);
            }
        }
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

    pub(crate) fn note_read(&mut self, name: &str) -> Result<(), QuasiTypeError> {
        self.read(name)
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
        if subs.is_empty() {
            let env = self.shared.env;
            let binding = self.lookup(name.clone())?;
            let declared = binding
                .declared
                .as_ref()
                .map_or(&Ty::Any, |declared| &declared.it);
            ty.to_maybe_ty(env)
                .maybe_defined()
                .ok_or(QuasiTypeError::undefined())?;
            env.assert_value_compatible(&ty, declared)
                .map_err(|err| err.assign(name.clone()))?;
            binding.assigned = ty;
            binding.assigned_at = HashSet::from([range]);
            let is_new = binding.new;
            if self.shared.output_params.iter().any(|(n, _)| *n == name) {
                self.shared.output_written.borrow_mut().insert(name.clone());
            }
            if is_new != Some(true) {
                self.shared.creates.borrow_mut().insert(name.clone());
            }
            // `or_insert`, not a blind `insert`: the only way this exact (name, range) pair is
            // set twice is the loop body being simulated twice (throwaway pass, then real) — if
            // the throwaway write was already marked read, that must survive the real pass
            // re-executing the same `Set`.
            self.shared
                .reading_list
                .borrow_mut()
                .entry(Written { name, range })
                .or_insert(false);
            Ok(())
        } else {
            let binding = self.lookup(name.clone())?;
            binding.subscript_assigned = Some(());
            let is_new = binding.new;
            if self.shared.output_params.iter().any(|(n, _)| *n == name) {
                self.shared.output_written.borrow_mut().insert(name.clone());
            }
            if is_new != Some(true) {
                self.shared.creates.borrow_mut().insert(name);
            }
            Ok(())
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
            if self.shared.output_params.iter().any(|(n, _)| *n == name) {
                self.shared.output_written.borrow_mut().insert(name);
            }
        }
        Ok(())
    }

    // MERGE always writes opaquely into the target's subtree, regardless of its own literal
    // subscript depth — unlike `handle_set`, which only takes the subtree branch when the
    // reference itself was subscripted.
    pub(crate) fn handle_merge(&mut self, local: Local) -> Result<(), QuasiTypeError> {
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
        self.read(&local.name)?;
        let name = local.name;
        let binding = self.lookup(name.clone())?;
        binding.assigned = ty;
        binding.subscript_assigned = Some(());
        let is_new = binding.new;
        // Delegating one of our own `Output` params to a callee (`Do Other(.x)`) counts as
        // assigning it — the callee is trusted to write it, same as any other by-ref/output arg.
        if self.shared.output_params.iter().any(|(n, _)| *n == name) {
            self.shared.output_written.borrow_mut().insert(name.clone());
        }
        if is_new != Some(true) {
            self.shared.creates.borrow_mut().insert(name);
        }
        Ok(())
    }

    // Killing the whole variable wipes it entirely, including subscript knowledge. Killing a
    // specific path only removes that path — since individual subscripts aren't tracked, the
    // scalar and other subscripts might still exist, so both are left untouched.
    pub(crate) fn handle_kill(&mut self, local: Local) -> Result<(), QuasiTypeError> {
        if local.subs.is_empty() {
            let binding = self.lookup(local.name)?;
            binding.assigned = PowerTy::undefined();
            binding.assigned_at = HashSet::new();
            binding.subscript_assigned = None;
        }
        Ok(())
    }

    // ZKILL removes only a node's own value, never its descendants — unlike `KILL`, this never
    // touches `subscript_assigned`, regardless of whether the target itself was subscripted.
    pub(crate) fn handle_zkill(&mut self, local: Local) -> Result<(), QuasiTypeError> {
        let binding = self.lookup(local.name)?;
        binding.assigned = PowerTy::undefined();
        binding.assigned_at = HashSet::new();
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

    fn read(&mut self, name: &str) -> Result<(), QuasiTypeError> {
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

    pub(crate) fn handle_get(
        &mut self,
        Local { name, subs }: Local,
    ) -> Result<PowerTy, QuasiTypeError> {
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
            private: false,
            declared: None,
            assigned_at: HashSet::from([range]),
        }
    }

    fn undeclared(procedure_block: bool) -> Self {
        Self {
            declared: None,
            assigned: if procedure_block {
                PowerTy::undefined()
            } else {
                Ty::Any.into()
            },
            assigned_at: HashSet::new(),
            subscript_assigned: if procedure_block { None } else { Some(()) },
            private: procedure_block,
            new: Some(procedure_block),
            new_at: None,
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
        })
    }
}

#[derive(Clone, Default)]
pub struct Bindings {
    bindings: HashMap<String, Binding>,
}

impl Bindings {
    pub fn publics(&self) -> Vec<String> {
        self.bindings
            .iter()
            .filter_map(|(k, v)| {
                if !v.private {
                    Some(k.to_string())
                } else {
                    None
                }
            })
            .collect()
    }
    pub fn olds(&self) -> Vec<String> {
        self.bindings
            .iter()
            .filter_map(|(k, v)| {
                if v.new != Some(true) {
                    Some(k.to_string())
                } else {
                    None
                }
            })
            .collect()
    }

    // Default is a closure, not a stored `procedure_block`, since only `Flow::lookup` knows it.
    fn entry(&mut self, name: String, default: impl FnOnce() -> Binding) -> &mut Binding {
        self.bindings.entry(name).or_insert_with(default)
    }

    fn refine_on_prop(&mut self, target: u32, term: &Term) {
        for binding in self.bindings.values_mut() {
            binding.assigned = binding.assigned.clone().refine_on_prop(target, term);
        }
    }
}

pub fn merge_all_bindings(
    this: &mut Option<Bindings>,
    that: Option<Bindings>,
    env: &WorkspaceEnv,
    procedure_block: bool,
) -> Result<(), QuasiTypeError> {
    match (this.as_mut(), that) {
        (None, that) => {
            *this = that;
            Ok(())
        }
        (Some(_), None) => Ok(()),
        (Some(this), Some(mut that)) => {
            let mut all_names = HashSet::new();
            for name in this.bindings.keys() {
                all_names.insert(name.to_string());
            }
            for name in that.bindings.keys() {
                all_names.insert(name.to_string());
            }
            for name in all_names {
                let this_binding = this
                    .bindings
                    .remove(&name)
                    .unwrap_or_else(|| Binding::undeclared(procedure_block));
                let that_binding = that
                    .bindings
                    .remove(&name)
                    .unwrap_or_else(|| Binding::undeclared(procedure_block));
                let merged = this_binding.merge(that_binding, env, &name)?;
                this.bindings.insert(name, merged);
            }
            Ok(())
        }
    }
}
