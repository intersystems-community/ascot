//! Macro preprocessing for routines: `#define`/`#def1arg`, `#include`, and the `$$$name` calls
//! that use them.
//!
//! `.mac`/`.int` files are type-checked in expanded form, so this module (1) collects
//! `MacroDefinitions` from a routine and the `.inc` files it includes (found in the `Workspace`,
//! or via IRIS), (2) produces the expanded text, and (3) builds a `SourceMap` that translates
//! positions in the expanded text back to the original file. The map is how diagnostics from
//! `check/` land on the right line and how hover shows "Expands to". Classes are not
//! macro-expanded yet.

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    ops::Deref,
    sync::Arc,
};

use tygr::{Grammar, IntoInner, OLC, Range};
use tygr_objectscript::cos::{HMacro, MacroArg, MacroCallStruct, MacroedUnit};

use crate::{
    DocDetails, WorkspaceStruct,
    bindings::{self, Diagnostic, Location, Position},
    cos::{self, HMacroCommand, MacroRedex, Routine, RoutineWithMacro},
    tygr_error_to_diagnostics,
};

// What IRIS makes available without an explicit include (verified by compiling probes on an
// IRIS instance): a class gets the whole %systemInclude closure (%occInclude, %msql, %cspInclude, ...)
// plus %occErrors; a routine gets nothing.
pub(crate) const CLASS_IMPLICIT_INCLUDES: &[&str] = &["%systemInclude", "%occErrors"];
pub(crate) const ROUTINE_IMPLICIT_INCLUDES: &[&str] = &[];

pub(crate) fn implicit_includes(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

// Macro name -> (arg list, if any; body text).
#[derive(Clone)]
pub struct MacroDefinition {
    // `None` for a parameterless `#define`.
    pub(crate) params: Option<Vec<String>>,
    // `#def1arg`: its one parameter (and `%literalargs`) stands for the whole argument text,
    // commas included, so a call's arity is never checked.
    pub(crate) literal_args: bool,
    pub(crate) body: String,
}

impl MacroDefinition {
    // `##safeexpression`/`##expression` run ObjectScript at compile time; `check::mod` knows the
    // common ones (`$$$ERROR`, ...) by name instead.
    fn is_compile_time(&self) -> bool {
        self.body.contains("##safeexpression") || self.body.contains("##expression")
    }
}

pub(crate) type MacroDefinitions = HashMap<String, MacroDefinition>;

// Where a span of expanded text came from. `uri: None` means the top-level file passed to
// `expand_rtn`. `parent`, if set, is the `#include` directive that pulled `uri`'s text in, so
// walking it up always reaches a `uri: None` origin.
#[derive(Clone)]
struct Origin {
    uri: Option<String>,
    range: std::ops::Range<usize>,
    parent: Option<Arc<Origin>>,
    // How many macro substitutions deep this text is; bounded by `MAX_MACRO_DEPTH`.
    depth: usize,
}

// Deeper than any real `#define` chain; a self-referential macro would otherwise expand forever.
const MAX_MACRO_DEPTH: usize = 200;

impl Origin {
    fn top_level(range: std::ops::Range<usize>) -> Self {
        Origin {
            uri: None,
            range,
            parent: None,
            depth: 0,
        }
    }

    fn with_range(&self, range: std::ops::Range<usize>) -> Self {
        Origin {
            uri: self.uri.clone(),
            range,
            parent: self.parent.clone(),
            depth: self.depth,
        }
    }

    fn for_included_file(&self, uri: String, range: std::ops::Range<usize>) -> Self {
        Origin {
            uri: Some(uri),
            range,
            parent: Some(Arc::new(self.clone())),
            depth: self.depth,
        }
    }

    fn deeper(&self) -> Self {
        Origin {
            depth: self.depth + 1,
            ..self.clone()
        }
    }
}

// (expanded range, origin, exact?), sorted by expanded start so `locate` can bsearch.
type SourceMapVec = Vec<(std::ops::Range<usize>, Origin, bool)>;

// Maps expanded-text positions/ranges back to the original source, across any `#include`d files
// that contributed to the expansion.
pub struct SourceMap {
    vec: SourceMapVec,
    source_olc: OLC,
    source_lines: usize,
    pub(crate) expanded_olc: OLC,
    expanded_text: String,
    // OLC for each uri that ever appears as an `Origin::uri`.
    included: HashMap<String, OLC>,
    // Every top-level macro invocation's own source span, sorted by start — lets `hover` answer
    // "what did the call at this source position expand to?" See `macro_expansion_at_position`.
    invocations: Vec<std::ops::Range<usize>>,
    // Unresolved `#include`s, already positioned — must NOT go through `relocate_diagnostics`.
    pub(crate) unresolved: Vec<Diagnostic>,
    // The top-level file's own `#include` directives: source span and the include's name.
    include_directives: Vec<(std::ops::Range<usize>, String)>,
    include_names: HashSet<String>,
}

impl SourceMap {
    pub(crate) fn include_directives(&self) -> impl Iterator<Item = (bindings::Range, &str)> {
        self.include_directives.iter().map(|(span, name)| {
            (
                bindings::Range {
                    start: Position::from(self.source_olc.offset_to_line_character(span.start)),
                    end: Position::from(self.source_olc.offset_to_line_character(span.end)),
                },
                name.as_str(),
            )
        })
    }

    // Every `#include` name this expansion asked for, resolved or not, nested ones included.
    pub(crate) fn include_names(&self) -> impl Iterator<Item = &String> {
        self.include_names.iter()
    }

    fn expanded_line_count(&self) -> usize {
        self.expanded_text.matches('\n').count() + 1
    }

    // False means `position` definitely isn't part of this expansion (e.g. it's really a
    // different member's declaration, which happens to share this document's uri) -- callers
    // must not feed it to any `expanded_olc`-based lookup, which panics out of bounds.
    pub(crate) fn is_in_expanded_bounds(&self, position: Position) -> bool {
        (position.line as usize) < self.expanded_line_count()
    }

    // Relocates a single expanded-text position back to the top-level source, or `None` if it
    // came from an `#include`d file (there's no sane point in the top-level file to collapse
    // it onto, so callers should drop it instead) or isn't part of this expansion at all.
    pub(crate) fn relocate_position(&self, position: Position) -> Option<Position> {
        if !self.is_in_expanded_bounds(position) {
            return None;
        }
        let offset = self
            .expanded_olc
            .line_character_to_offset((position.line as usize, position.character as usize));
        let (source_offset, _) = self.to_source_offset(offset)?;
        Some(Position::from(
            self.source_olc.offset_to_line_character(source_offset),
        ))
    }

    // The end is exclusive, so it is relocated through the last character it covers; otherwise
    // a range ending exactly where a macro's substituted argument ends would take the position
    // of whatever the macro body puts next.
    pub(crate) fn relocate_source_range(&self, range: bindings::Range) -> Option<bindings::Range> {
        if !self.is_in_expanded_bounds(range.end) {
            return None;
        }
        let end = self
            .expanded_olc
            .line_character_to_offset((range.end.line as usize, range.end.character as usize));
        let Some(last) = end.checked_sub(1) else {
            let start = self.relocate_position(range.start)?;
            return Some(bindings::Range { start, end: start });
        };
        let (span, origin, precise) = self.locate(last)?;
        if origin.uri.is_some() {
            return None;
        }
        let end = if *precise {
            origin.range.start + (last - span.start) + 1
        } else {
            origin.range.end
        };
        Some(bindings::Range {
            start: self.relocate_position(range.start)?,
            end: Position::from(self.source_olc.offset_to_line_character(end)),
        })
    }

    // What the macro invocation at `position` (in the routine's original, unexpanded text)
    // ultimately expanded to — recursively, so a macro that itself calls other macros shows
    // its full expansion, not just its own body's literal text. `None` if `position` isn't
    // inside any macro invocation.
    pub(crate) fn macro_expansion_at_position(&self, position: Position) -> Option<String> {
        if position.line as usize >= self.source_lines {
            return None;
        }
        let offset = self
            .source_olc
            .line_character_to_offset((position.line as usize, position.character as usize));
        let idx = self
            .invocations
            .partition_point(|span| span.start <= offset)
            .checked_sub(1)?;
        let span = self.invocations.get(idx)?;
        if offset >= span.end {
            return None;
        }
        let mut pieces: Vec<&(std::ops::Range<usize>, Origin, bool)> = self
            .vec
            .iter()
            .filter(|(_, origin, _)| {
                origin.uri.is_none()
                    && origin.range.start >= span.start
                    && origin.range.end <= span.end
            })
            .collect();
        pieces.sort_by_key(|(expanded, ..)| expanded.start);
        (!pieces.is_empty()).then(|| {
            pieces
                .iter()
                .map(|(expanded, ..)| &self.expanded_text[expanded.clone()])
                .collect()
        })
    }

    fn locate_idx(&self, pos: usize) -> Option<usize> {
        let idx = self
            .vec
            .partition_point(|(expanded, ..)| expanded.start <= pos);
        let idx = idx.checked_sub(1)?;
        (pos <= self.vec.get(idx)?.0.end).then_some(idx)
    }

    fn locate(&self, pos: usize) -> Option<&(std::ops::Range<usize>, Origin, bool)> {
        self.vec.get(self.locate_idx(pos)?)
    }

    // The full chain of provenance for `expanded`, from the top-level file (always first) down
    // to the deepest known position — one link per `#include` boundary crossed to get there.
    // Every range is in its own paired uri's coordinate space (`None` = the top-level file).
    fn relocation_chain(
        &self,
        expanded: std::ops::Range<usize>,
    ) -> Vec<(Option<String>, std::ops::Range<usize>)> {
        let Some(last) = expanded.end.max(expanded.start + 1).checked_sub(1) else {
            return vec![];
        };
        let (Some(start_idx), Some(end_idx)) =
            (self.locate_idx(expanded.start), self.locate_idx(last))
        else {
            return vec![];
        };
        let (start_expanded, start_origin, start_precise) = &self.vec[start_idx];
        let (end_expanded, end_origin, end_precise) = &self.vec[end_idx];
        // A wide range (e.g. a whole subroutine body) can start or end inside a macro's
        // expansion while the other endpoint is literal source — collapsing the whole thing to
        // `start_origin`'s span alone (as if both ends were imprecise) would badly undershoot.
        // Only fall back per-endpoint: an imprecise endpoint maps to its macro call's own
        // source span, a precise one to its exact position.
        let leaf = if start_origin.uri == end_origin.uri {
            let start = if *start_precise {
                start_origin.range.start + (expanded.start - start_expanded.start)
            } else {
                start_origin.range.start
            };
            let end = if *end_precise {
                end_origin.range.start + (last - end_expanded.start) + 1
            } else {
                end_origin.range.end
            };
            (start_origin.uri.clone(), start..end)
        } else {
            (start_origin.uri.clone(), start_origin.range.clone())
        };
        let mut chain = vec![leaf];
        let mut ancestor = start_origin.parent.as_deref();
        while let Some(origin) = ancestor {
            chain.push((origin.uri.clone(), origin.range.clone()));
            ancestor = origin.parent.as_deref();
        }
        chain.reverse();
        chain
    }

    // Like `relocation_chain`'s top-level entry, but for a single offset. `None` if `expanded`
    // came from an `#include`d file rather than the top-level source.
    pub fn to_source_offset(&self, expanded: usize) -> Option<(usize, bool)> {
        let (start_expanded, start_origin, start_precise) = self.locate(expanded)?;
        if start_origin.uri.is_some() {
            return None;
        }
        if *start_precise {
            let start = start_origin.range.start + (expanded - start_expanded.start);
            Some((start, true))
        } else {
            Some((start_origin.range.start, false))
        }
    }

    fn range_in(
        &self,
        uri: &Option<String>,
        range: &std::ops::Range<usize>,
    ) -> Option<bindings::Range> {
        let olc = match uri {
            None => &self.source_olc,
            Some(uri) => self.included.get(uri)?,
        };
        Some(bindings::Range {
            start: Position::from(olc.offset_to_line_character(range.start)),
            end: Position::from(olc.offset_to_line_character(range.end)),
        })
    }

    // The fully macro-expanded text of the physical line containing `offset` — not just the
    // origin of the sub-span a diagnostic happens to point at, since a macro's own expansion
    // can splice further macros (each with their own origin) and unbraced control commands
    // (`if`, `for`, ...) that swallow the rest of the line into their scope. Showing the whole
    // line is what actually explains the diagnostic in those cases.
    fn expanded_line_text(&self, offset: usize) -> Option<String> {
        let start = self.expanded_text[..offset]
            .rfind('\n')
            .map_or(0, |i| i + 1);
        let end = self.expanded_text[offset..]
            .find('\n')
            .map_or(self.expanded_text.len(), |i| offset + i);
        const MAX_LEN: usize = 200;
        let text = self.expanded_text[start..end].trim();
        if text.is_empty() {
            return None;
        }
        if text.len() <= MAX_LEN {
            return Some(text.to_string());
        }
        let mut cut = MAX_LEN;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        // Marked as cut, so a long expansion isn't mistaken for the whole of it.
        Some(format!("{}…", &text[..cut]))
    }

    // The expanded line at `position`, if it only exists because of a macro substitution.
    pub(crate) fn expansion_at(&self, position: Position) -> Option<String> {
        if !self.is_in_expanded_bounds(position) {
            return None;
        }
        let offset = self
            .expanded_olc
            .line_character_to_offset((position.line as usize, position.character as usize));
        let idx = self.locate_idx(offset)?;
        if self.vec[idx].2 {
            return None;
        }
        self.expanded_line_text(offset)
    }

    // The primary (top-level-file) range, plus one `related_information` entry per further
    // `#include` hop in the chain, outer-to-inner, plus (if the primary range only exists
    // because of a macro substitution) what that macro actually expanded to.
    fn relocate_range(
        &self,
        range: &bindings::Range,
        uri: &str,
    ) -> (bindings::Range, Vec<bindings::DiagnosticRelatedInformation>) {
        if !self.is_in_expanded_bounds(range.start) || !self.is_in_expanded_bounds(range.end) {
            return (*range, vec![]);
        }
        let start = self
            .expanded_olc
            .line_character_to_offset((range.start.line as usize, range.start.character as usize));
        let end = self
            .expanded_olc
            .line_character_to_offset((range.end.line as usize, range.end.character as usize));
        let chain = self.relocation_chain(start..end);
        let Some((top_uri, top_range)) = chain.first() else {
            return (*range, vec![]);
        };
        debug_assert!(top_uri.is_none());
        let Some(primary) = self.range_in(top_uri, top_range) else {
            return (*range, vec![]);
        };
        let mut hops: Vec<_> = chain[1..]
            .iter()
            .filter_map(|(uri, range)| {
                let uri = uri.clone()?;
                let range = self.range_in(&Some(uri.clone()), range)?;
                Some(bindings::DiagnosticRelatedInformation {
                    location: Location::new(uri, range),
                    message: "↳ Included from here.".into(),
                })
            })
            .collect();
        if let Some(text) = self.expansion_at(range.start) {
            hops.push(bindings::DiagnosticRelatedInformation {
                location: Location::new(uri.to_string(), primary),
                message: format!("↳ Expands to: {text}"),
            });
        }
        (primary, hops)
    }

    fn relocate_related(
        &self,
        mut info: bindings::DiagnosticRelatedInformation,
        uri: &str,
    ) -> Vec<bindings::DiagnosticRelatedInformation> {
        if info.location.uri != uri {
            return vec![info];
        }
        let (range, hops) = self.relocate_range(&info.location.range, uri);
        info.location.range = range;
        let mut result = vec![info];
        result.extend(hops);
        result
    }

    // Relocates every diagnostic's range and related-information back to source, expanding each
    // into its own chain: the main range's chain first, then each original related-information
    // entry followed immediately by its own chain.
    pub(crate) fn relocate_diagnostics(
        &self,
        diagnostics: Vec<bindings::Diagnostic>,
        uri: &str,
    ) -> Vec<bindings::Diagnostic> {
        diagnostics
            .into_iter()
            .map(|mut diagnostic| {
                let (range, mut related_information) = self.relocate_range(&diagnostic.range, uri);
                diagnostic.range = range;
                for info in std::mem::take(&mut diagnostic.related_information) {
                    related_information.extend(self.relocate_related(info, uri));
                }
                diagnostic.related_information = related_information;
                diagnostic
            })
            .collect()
    }
}

impl WorkspaceStruct {
    // Keeps `inc_index` in sync with `uri`'s latest tier-A header name — dropping the old
    // name's entry first, in case it changed.
    // Resolves an `#include name`: an already-open `.inc` document first, then (if connected)
    // by fetching it from the server -- covers a system or shared include that isn't part of
    // the local editor workspace. Returns its uri (the host's own if it offered one, else a
    // synthetic placeholder just for origin-tracking), full text, and un-substituted macro units.
    fn resolve_include(&self, name: &str) -> Option<(String, String, Vec<Range<MacroedUnit>>)> {
        if let Some(uri) = {
            let this = &self;
            this.inc_index.borrow().get(name).cloned()
        } {
            // `uri`'s own tier B -- just its header and un-substituted macro units, computed
            // and cached (via `ensure_details`) the moment it's first `#include`d.
            let parsed = match self.ensure_details(&uri).ok()? {
                DocDetails::Inc(parsed) => parsed,
                _ => return None,
            };
            let text = self.doc_text(&uri)?;
            return Some((uri, text, parsed.macroed_units.clone()));
        }
        let routine = self.iris.as_ref()?.get_inc(name)?;
        let parsed = RoutineWithMacro::parse(&routine.text).ok()?;
        let uri = routine
            .uri
            .unwrap_or_else(|| format!("iris-include:///{name}.inc"));
        Some((uri, routine.text, parsed.macroed_units))
    }

    fn expand_rtn_helper(
        &self,
        src: &str,
        dones: &mut DoneList,
    ) -> Result<(Routine, MacroDefinitions), tygr::Error> {
        let (mut definitions, _, _) =
            self.resolve_class_includes(&implicit_includes(ROUTINE_IMPLICIT_INCLUDES));
        let RoutineWithMacro {
            header,
            macroed_units,
        } = RoutineWithMacro::parse(src)?;
        let header_text = header.print();
        dones.push(&header_text, &Origin::top_level(0..header_text.len()), true);
        // A `.int` is the compiler's own output: already expanded, nothing to do here.
        if header
            .type_spec
            .as_ref()
            .is_some_and(|spec| spec.print().to_uppercase().contains("INT"))
        {
            for unit in &macroed_units {
                dones.push(
                    &unit.print(),
                    &Origin::top_level(unit.start..unit.end),
                    true,
                );
            }
            return Ok((Routine::parse(&dones.text)?, definitions));
        }
        let mut todos = TodoList::new(
            macroed_units
                .iter()
                .map(|x| {
                    let origin = Origin::top_level(x.start..x.end);
                    (Cow::Borrowed(x.deref()), origin, true)
                })
                .collect(),
        );
        self.expand_units(&mut definitions, &mut todos, dones)?;
        Ok((Routine::parse(&dones.text)?, definitions))
    }

    // The substitution loop: `#define`/`#include`/macro-calls update `definitions` or requeue
    // their expansion onto `todos`; `#if` is resolved by `resolve_if`; everything else prints
    // as-is via `dones`.
    fn expand_units(
        &self,
        definitions: &mut MacroDefinitions,
        todos: &mut TodoList<'_>,
        dones: &mut DoneList,
    ) -> Result<(), tygr::Error> {
        while let Some((unit, origin, precise)) = todos.pop() {
            match unit.as_ref() {
                MacroedUnit::Plain(unit) => dones.push_plain(&unit.print(), &origin, precise),
                MacroedUnit::Redex(unit) => match unit {
                    MacroRedex::H(HMacro { command, input }) => match command.deref() {
                        HMacroCommand::Define { name, args, .. } => {
                            definitions.insert(
                                name.print(),
                                MacroDefinition {
                                    params: args.as_ref().map(|args| {
                                        args.items.iter().map(|arg| arg.print()).collect()
                                    }),
                                    literal_args: false,
                                    body: import_macro_input(input),
                                },
                            );
                        }
                        HMacroCommand::Def1arg { name, arg, .. } => {
                            definitions.insert(
                                name.print(),
                                MacroDefinition {
                                    params: arg.as_ref().map(|arg| vec![arg.deref().print()]),
                                    literal_args: true,
                                    body: import_macro_input(input),
                                },
                            );
                        }
                        HMacroCommand::Include(included_name) => {
                            let included_name = included_name.deref();
                            if origin.uri.is_none() {
                                dones
                                    .include_directives
                                    .push((origin.range.clone(), included_name.print()));
                            }
                            dones.include_names.insert(included_name.print());
                            match self.resolve_include(&included_name.print()) {
                                // IRIS processes a repeated `#include` once; this also ends
                                // include cycles.
                                Some((inc_uri, ..)) if dones.included.contains_key(&inc_uri) => {}
                                Some((inc_uri, inc_text, inc_units)) => {
                                    dones.note_included_file(inc_uri.clone(), &inc_text);
                                    let items = inc_units
                                        .into_iter()
                                        .map(|item| {
                                            let inc_range = item.start..item.end;
                                            (
                                                Cow::Owned(item.into_inner()),
                                                origin
                                                    .for_included_file(inc_uri.clone(), inc_range),
                                                true,
                                            )
                                        })
                                        .collect();
                                    todos.push_items(items);
                                }
                                None => {
                                    dones.note_unresolved_include(included_name.print(), &origin)
                                }
                            }
                        }
                        HMacroCommand::Noshow(..) => (),
                        HMacroCommand::Show(..) => (),
                        HMacroCommand::If(..) => {
                            self.resolve_if(
                                definitions,
                                todos,
                                dones,
                                input.clone(),
                                unit.print(),
                                origin,
                                precise,
                            )?;
                        }
                        HMacroCommand::IfDef(..) | HMacroCommand::IfNDef(..) => {
                            let defined = definitions.contains_key(&macro_name_in(input));
                            let keep =
                                defined == matches!(command.deref(), HMacroCommand::IfDef(..));
                            self.resolve_branch(
                                definitions,
                                todos,
                                dones,
                                Some(keep),
                                unit.print(),
                                origin,
                                precise,
                            )?;
                        }
                        HMacroCommand::Undef(..) => {
                            definitions.remove(&macro_name_in(input));
                        }
                        _ => dones.push(&unit.print(), &origin, precise),
                    },
                    MacroRedex::DDD(ddd) => {
                        dones.note_invocation(&origin);
                        let MacroCallStruct { name, args } = ddd.deref();
                        let substituted = match definitions.get(&name.print()) {
                            Some(_) if origin.depth >= MAX_MACRO_DEPTH => {
                                dones.note_unresolved(
                                    crate::check::TypeError::macro_expansion_too_deep,
                                    name.print(),
                                    &origin,
                                );
                                false
                            }
                            Some(def) if def.is_compile_time() => false,
                            // `$$$M(args)` on a parameterless `M` keeps the argument list: the
                            // body is typically a method name the arguments then call.
                            Some(MacroDefinition {
                                params: None, body, ..
                            }) => {
                                let mut units = <Vec<MacroedUnit>>::parse(body)?;
                                if let Some(args) = args.as_ref() {
                                    units.extend(<Vec<MacroedUnit>>::parse(&args.print())?);
                                }
                                todos.push(units, origin.deeper());
                                true
                            }
                            Some(MacroDefinition {
                                params: Some(pars),
                                literal_args: true,
                                body,
                            }) if args.is_some() => {
                                let args = args.as_deref().unwrap();
                                let text = args
                                    .items
                                    .iter()
                                    .map(|arg| arg.print())
                                    .collect::<Vec<_>>()
                                    .join(",");
                                let arg_origin = match (args.items.first(), args.items.last()) {
                                    (Some(first), Some(last)) if precise => {
                                        origin.with_range(first.start..last.end)
                                    }
                                    _ => origin.clone(),
                                };
                                let literal = "%literalargs".to_string();
                                let param_map: Vec<(&String, String, Origin, bool)> = pars
                                    .iter()
                                    .chain(std::iter::once(&literal))
                                    .map(|par| (par, text.clone(), arg_origin.clone(), precise))
                                    .collect();
                                let items =
                                    substitute_macro_body(body, &param_map, &origin.deeper())?;
                                todos.push_items(items);
                                true
                            }
                            Some(MacroDefinition {
                                params: Some(pars),
                                body,
                                ..
                            }) if args
                                .as_ref()
                                .is_some_and(|args| pars.len() == args.items.len()) =>
                            {
                                let args = args.as_deref().unwrap();
                                let mut param_map: Vec<(&String, String, Origin, bool)> = pars
                                    .iter()
                                    .zip(args.items.iter())
                                    .map(|(par, arg)| {
                                        let arg_origin = if precise {
                                            origin.with_range(arg.start..arg.end)
                                        } else {
                                            origin.clone()
                                        };
                                        (par, arg.print(), arg_origin, precise)
                                    })
                                    .collect();
                                param_map.sort_by(|(k1, ..), (k2, ..)| k2.cmp(k1));
                                let items =
                                    substitute_macro_body(body, &param_map, &origin.deeper())?;
                                todos.push_items(items);
                                true
                            }
                            _ => false,
                        };
                        // Not substituted -- `name` either isn't a `#define`d macro at all, is
                        // compile-time evaluated (e.g. `$$$ERROR`, resolved by `check::mod`
                        // instead) or its arity didn't match. Left as literal text, but its own
                        // arguments (e.g. `$$$GeneralError` inside `$$$ERROR(...)`) still expand.
                        if !substituted {
                            match args.as_deref() {
                                Some(args) => {
                                    let text = unit.print();
                                    let base = origin.range.start;
                                    let arg_span = |arg: &Range<MacroArg>| {
                                        Some(
                                            arg.start.checked_sub(base)?
                                                ..arg.end.checked_sub(base)?,
                                        )
                                    };
                                    // Precise means `text` sits at `origin.range`, so it's pushed
                                    // piecewise: an argument whose expansion changed its length
                                    // maps to the argument's own span, the rest stays exact.
                                    let piecewise = precise
                                        && args.items.iter().all(|arg| {
                                            arg_span(arg).and_then(|span| text.get(span))
                                                == Some(arg.print().as_str())
                                        });
                                    if piecewise {
                                        let mut done = 0;
                                        for arg in args.items.iter() {
                                            let span = arg_span(arg).unwrap();
                                            let piece =
                                                origin.with_range(base + done..base + span.start);
                                            dones.push(&text[done..span.start], &piece, true);
                                            let expanded =
                                                self.expand_text(definitions, &text[span.clone()])?;
                                            let exact = expanded == text[span.clone()];
                                            let piece = origin
                                                .with_range(base + span.start..base + span.end);
                                            dones.push(&expanded, &piece, exact);
                                            done = span.end;
                                        }
                                        let piece =
                                            origin.with_range(base + done..origin.range.end);
                                        dones.push(&text[done..], &piece, true);
                                    } else {
                                        let mut arg_texts = Vec::with_capacity(args.items.len());
                                        for arg in args.items.iter() {
                                            arg_texts
                                                .push(self.expand_text(definitions, &arg.print())?);
                                        }
                                        dones.push(
                                            &format!(
                                                "$$${}({})",
                                                name.print(),
                                                arg_texts.join(",")
                                            ),
                                            &origin,
                                            false,
                                        );
                                    }
                                }
                                None => dones.push(&unit.print(), &origin, precise),
                            }
                        }
                    }
                    MacroRedex::Lit(_, text, _) => dones.push(&text.print(), &origin, precise),
                    MacroRedex::Embedded(block) => dones.push(&block.print(), &origin, precise),
                    MacroRedex::LC(..) => (),
                    unit => dones.push(&unit.print(), &origin, precise),
                },
            }
        }
        Ok(())
    }

    fn expand_condition(
        &self,
        definitions: &mut MacroDefinitions,
        input: &cos::MacroInput,
    ) -> Result<String, tygr::Error> {
        self.expand_text(definitions, &import_macro_input(input))
    }

    // Expands `text` in isolation -- e.g. one `#if` condition, or one argument to a call that
    // otherwise didn't resolve as a `#define`d macro (see `MacroRedex::DDD`). Always terminates:
    // it only ever recurses on strictly smaller, nested argument text.
    fn expand_text(
        &self,
        definitions: &mut MacroDefinitions,
        text: &str,
    ) -> Result<String, tygr::Error> {
        let mut dones = DoneList::new();
        let mut todos = TodoList::new(Vec::new());
        todos.push(<Vec<MacroedUnit>>::parse(text)?, Origin::top_level(0..0));
        self.expand_units(definitions, &mut todos, &mut dones)?;
        Ok(dones.text)
    }

    // Resolves one `#if`/`#elseif` branch by its condition: "1" keeps it, "0" drops it, and
    // anything else is unresolved (see `resolve_branch`).
    #[allow(clippy::too_many_arguments)]
    fn resolve_if(
        &self,
        definitions: &mut MacroDefinitions,
        todos: &mut TodoList<'_>,
        dones: &mut DoneList,
        condition: cos::MacroInput,
        printed: String,
        origin: Origin,
        precise: bool,
    ) -> Result<(), tygr::Error> {
        let expanded = self.expand_condition(definitions, &condition)?;
        // `#if 0 /* Maintenance */` -- a trailing comment is common and not part of the test.
        let end = ["/*", "//", ";"]
            .iter()
            .filter_map(|marker| expanded.find(marker))
            .min()
            .unwrap_or(expanded.len());
        let decided = match expanded[..end].trim() {
            "1" => Some(true),
            "0" => Some(false),
            _ => None,
        };
        self.resolve_branch(definitions, todos, dones, decided, printed, origin, precise)
    }

    // Keeps a decided-true branch (discarding the rest of the chain via take_branch+skip_branch),
    // drops a decided-false one and tries the closing `#elseif` in turn, and leaves an undecided
    // one alone -- the directive prints unexpanded and `todos` is left untouched, so the rest of
    // the chain is also left unresolved rather than discarded.
    #[allow(clippy::too_many_arguments)]
    fn resolve_branch(
        &self,
        definitions: &mut MacroDefinitions,
        todos: &mut TodoList<'_>,
        dones: &mut DoneList,
        decided: Option<bool>,
        printed: String,
        origin: Origin,
        precise: bool,
    ) -> Result<(), tygr::Error> {
        match decided {
            Some(true) => {
                let (branch, mut end) = take_branch(todos);
                while !matches!(end, BranchEnd::EndIf) {
                    end = skip_branch(todos);
                }
                todos.restore(branch);
                Ok(())
            }
            Some(false) => match skip_branch(todos) {
                BranchEnd::EndIf | BranchEnd::Else => Ok(()),
                BranchEnd::Elseif {
                    condition,
                    printed,
                    origin,
                    precise,
                } => self.resolve_if(
                    definitions,
                    todos,
                    dones,
                    condition,
                    printed,
                    origin,
                    precise,
                ),
            },
            None => {
                dones.push(&printed, &origin, precise);
                Ok(())
            }
        }
    }

    pub fn expand_rtn(
        &self,
        uri: &str,
        src: &str,
    ) -> Result<(Routine, SourceMap, MacroDefinitions), Diagnostic> {
        let mut dones = DoneList::new();
        let result = self.expand_rtn_helper(src, &mut dones);
        let map = dones.into_source_map(src);
        match result {
            Ok((routine, definitions)) => Ok((routine, map, definitions)),
            // A macro left unexpanded by an unresolved `#include` often doesn't re-parse (same
            // failure mode `note_unresolved_include` already calls out for `#include` itself) --
            // report the missing include(s), the actionable root cause, instead of the resulting
            // generic parse error.
            Err(error) => match map.unresolved.split_first() {
                Some((first, rest)) => {
                    let mut diagnostic = first.clone();
                    diagnostic.related_information.extend(rest.iter().map(|d| {
                        bindings::DiagnosticRelatedInformation {
                            location: Location::new(uri.to_string(), d.range),
                            message: d.message.clone(),
                        }
                    }));
                    Err(diagnostic)
                }
                None => {
                    let error = tygr_error_to_diagnostics(error, &map.expanded_text);
                    Err(map.relocate_diagnostics(vec![error], uri).pop().unwrap())
                }
            },
        }
    }

    pub(crate) fn resolve_class_includes(
        &self,
        names: &[String],
    ) -> (MacroDefinitions, Vec<String>, HashSet<String>) {
        let mut definitions = HashMap::new();
        let mut failed = Vec::new();
        let mut include_names = HashSet::new();
        for name in names {
            include_names.insert(name.clone());
            match self.resolve_include(name) {
                Some((inc_uri, inc_text, inc_units)) => {
                    let mut dones = DoneList::new();
                    dones.note_included_file(inc_uri.clone(), &inc_text);
                    let mut todos = TodoList::new(
                        inc_units
                            .iter()
                            .map(|x| {
                                let origin = Origin::top_level(0..0)
                                    .for_included_file(inc_uri.clone(), x.start..x.end);
                                (Cow::Borrowed(x.deref()), origin, true)
                            })
                            .collect(),
                    );
                    let _ = self.expand_units(&mut definitions, &mut todos, &mut dones);
                    // Whatever the include itself included: an edit there changes this class too.
                    include_names.extend(dones.include_names);
                }
                None => failed.push(name.clone()),
            }
        }
        (definitions, failed, include_names)
    }

    // `header` is pushed verbatim, unexpanded, ahead of the expanded `body` -- like
    // `expand_rtn_helper` pushes a routine's own header -- so both share one coordinate space.
    pub(crate) fn expand_method_body(
        &self,
        uri: &str,
        header: &str,
        body: &str,
        base_definitions: &MacroDefinitions,
    ) -> Result<(String, SourceMap, MacroDefinitions), Diagnostic> {
        let mut dones = DoneList::new();
        // The body's own `#define`s land here too, for `check::mod` to recognize.
        let mut definitions = base_definitions.clone();
        // Tokenized together so every offset, including a macro argument's, is relative to the
        // same text; the header itself is copied verbatim (a `{$$$X}` parameter default must not
        // change its length, or the body would be cut at the wrong offset).
        let text = format!("{header}{body}");
        let result = (|| -> Result<(), tygr::Error> {
            let units = <Vec<Range<MacroedUnit>>>::parse(&text)?;
            let (in_header, in_body): (Vec<_>, Vec<_>) =
                units.iter().partition(|x| x.end <= header.len());
            for x in in_header {
                dones.push(&x.print(), &Origin::top_level(x.start..x.end), true);
            }
            let mut todos = TodoList::new(
                in_body
                    .into_iter()
                    .map(|x| {
                        (
                            Cow::Borrowed(x.deref()),
                            Origin::top_level(x.start..x.end),
                            true,
                        )
                    })
                    .collect(),
            );
            self.expand_units(&mut definitions, &mut todos, &mut dones)
        })();
        let map = dones.into_source_map(&text);
        match result {
            Ok(()) => {
                let expanded_text = map.expanded_text.clone();
                Ok((expanded_text, map, definitions))
            }
            Err(error) => {
                let error = tygr_error_to_diagnostics(error, &map.expanded_text);
                Err(map.relocate_diagnostics(vec![error], uri).pop().unwrap())
            }
        }
    }
}

// Rebuilds `body` with every `%N`-style parameter in `param_map` replaced by its argument text
// (same left-to-right, sequential-per-parameter semantics as a series of `str::replace` calls),
// tracking the origin of each resulting piece: a substituted argument keeps its own (precise, if
// the call site was) origin, while literal macro-body text keeps the macro call's own origin.
// Returns the re-tokenized units, each tagged with the origin of the span it came from.
fn substitute_macro_body<'a>(
    body: &str,
    param_map: &[(&String, String, Origin, bool)],
    call_origin: &Origin,
) -> Result<Vec<(Cow<'a, MacroedUnit>, Origin, bool)>, tygr::Error> {
    struct Span {
        text: String,
        origin: Origin,
        precise: bool,
        // A pasted argument: literal text, not rescanned for the remaining parameters' names.
        argument: bool,
    }
    let mut spans = vec![Span {
        text: body.to_string(),
        origin: call_origin.clone(),
        precise: false,
        argument: false,
    }];
    for (name, replacement, arg_origin, arg_precise) in param_map {
        let mut next = Vec::with_capacity(spans.len());
        for span in spans {
            if span.argument {
                next.push(span);
                continue;
            }
            let mut rest = span.text.as_str();
            loop {
                match rest.find(name.as_str()) {
                    None => {
                        if !rest.is_empty() {
                            next.push(Span {
                                text: rest.to_string(),
                                origin: span.origin.clone(),
                                precise: span.precise,
                                argument: false,
                            });
                        }
                        break;
                    }
                    Some(idx) => {
                        if idx > 0 {
                            next.push(Span {
                                text: rest[..idx].to_string(),
                                origin: span.origin.clone(),
                                precise: span.precise,
                                argument: false,
                            });
                        }
                        next.push(Span {
                            text: replacement.clone(),
                            origin: arg_origin.clone(),
                            precise: *arg_precise,
                            argument: true,
                        });
                        rest = &rest[idx + name.len()..];
                    }
                }
            }
        }
        spans = next;
    }

    // Re-tokenize the whole substituted text, so a redex assembled across spans (`$$$Name(` in the
    // body completed by an argument, or an argument that is itself a bare macro name) is seen.
    // Plain text is split back at span boundaries to keep each piece's origin; a straddling redex
    // gets the call's.
    let text: String = spans.iter().map(|span| span.text.as_str()).collect();
    let mut bounds = Vec::with_capacity(spans.len());
    let mut offset = 0;
    for span in &spans {
        bounds.push((offset, offset + span.text.len(), span));
        offset += span.text.len();
    }
    let mut items = Vec::new();
    for unit in <Vec<Range<MacroedUnit>>>::parse(&text)? {
        let overlapping = bounds
            .iter()
            .filter(|(start, end, _)| *start < unit.end && unit.start < *end);
        match unit.deref() {
            MacroedUnit::Plain(_) => {
                for (start, end, span) in overlapping {
                    let piece = &text[unit.start.max(*start)..unit.end.min(*end)];
                    for part in <Vec<MacroedUnit>>::parse(piece)? {
                        items.push((Cow::Owned(part), span.origin.clone(), span.precise));
                    }
                }
            }
            // Never precise: a redex's own argument offsets are relative to this re-tokenized
            // text, not to the source, so `MacroRedex::DDD` couldn't map them back.
            MacroedUnit::Redex(_) => {
                let mut overlapping = overlapping;
                let origin = match (overlapping.next(), overlapping.next()) {
                    (Some((_, _, span)), None) => span.origin.clone(),
                    _ => call_origin.clone(),
                };
                items.push((Cow::Owned(unit.into_inner()), origin, false));
            }
        }
    }
    Ok(items)
}

// Builds the diagnostic for something left unresolved, once its source range is known.
type UnresolvedError = fn(String, bindings::Range) -> crate::check::TypeError;

struct DoneList {
    text: String,
    map: SourceMapVec,
    included: HashMap<String, OLC>,
    invocations: Vec<std::ops::Range<usize>>,
    // `#include`s naming a file not open in the workspace — dropped rather than spliced back in
    // literally, since the raw `#include Name` text rarely re-parses and would otherwise mask
    // this diagnostic behind a generic syntax error.
    unresolved: Vec<(UnresolvedError, String, Origin)>,
    include_directives: Vec<(std::ops::Range<usize>, String)>,
    include_names: HashSet<String>,
}

impl DoneList {
    fn new() -> Self {
        Self {
            text: String::new(),
            map: Vec::new(),
            included: HashMap::new(),
            invocations: Vec::new(),
            unresolved: Vec::new(),
            include_directives: Vec::new(),
            include_names: HashSet::new(),
        }
    }

    fn note_unresolved_include(&mut self, name: String, origin: &Origin) {
        self.note_unresolved(crate::check::TypeError::cannot_find_include, name, origin);
    }

    fn note_unresolved(&mut self, make: UnresolvedError, name: String, origin: &Origin) {
        self.unresolved.push((make, name, origin.clone()));
    }

    // Records a top-level macro call's own source span, for `macro_expansion_at_position`.
    // Only meaningful for a call visible directly in the routine's source — one nested inside
    // another macro's `#define`d body has no source span of its own to record.
    fn note_invocation(&mut self, origin: &Origin) {
        if origin.uri.is_none() {
            self.invocations.push(origin.range.clone());
        }
    }

    fn push(&mut self, text: &str, origin: &Origin, precise: bool) {
        let start = self.text.len();
        self.text.push_str(text);
        self.map
            .push((start..self.text.len(), origin.clone(), precise));
    }

    // Like `push`, but a `#dim` at column 0 (legal in a routine, where the plain grammar keeps
    // column 0 for labels) is indented by one space, mapped imprecisely so positions stay exact.
    fn push_plain(&mut self, text: &str, origin: &Origin, precise: bool) {
        let mut done = 0;
        let mut at_line_start = self.text.is_empty() || self.text.ends_with('\n');
        for (i, ch) in text.char_indices() {
            if at_line_start
                && text[i..]
                    .get(..4)
                    .is_some_and(|w| w.eq_ignore_ascii_case("#dim"))
            {
                let piece = if precise {
                    origin.with_range(origin.range.start + done..origin.range.start + i)
                } else {
                    origin.clone()
                };
                self.push(&text[done..i], &piece, precise);
                self.push(" ", origin, false);
                done = i;
            }
            at_line_start = ch == '\n';
        }
        let piece = if precise {
            origin.with_range(origin.range.start + done..origin.range.end)
        } else {
            origin.clone()
        };
        self.push(&text[done..], &piece, precise);
    }

    // Remembers `uri`'s own text so a precise origin into it can later be converted to a
    // line/character range in *its* coordinate space, not the top-level file's.
    fn note_included_file(&mut self, uri: String, text: &str) {
        self.included.entry(uri).or_insert_with(|| OLC::new(text));
    }

    fn into_source_map(self, src: &str) -> SourceMap {
        let source_olc = OLC::new(src);
        let mut invocations = self.invocations;
        invocations.sort_by_key(|span| span.start);
        let unresolved = self
            .unresolved
            .into_iter()
            .filter_map(|(make, name, origin)| {
                let olc = match &origin.uri {
                    None => &source_olc,
                    Some(uri) => self.included.get(uri)?,
                };
                let range = bindings::Range {
                    start: Position::from(olc.offset_to_line_character(origin.range.start)),
                    end: Position::from(olc.offset_to_line_character(origin.range.end)),
                };
                Some(make(name, range).into())
            })
            .collect();
        SourceMap {
            vec: self.map,
            source_olc,
            source_lines: src.matches('\n').count() + 1,
            expanded_olc: OLC::new(&self.text),
            expanded_text: self.text,
            included: self.included,
            invocations,
            unresolved,
            include_directives: self.include_directives,
            include_names: self.include_names,
        }
    }
}

type TodoItem<'a> = (Cow<'a, MacroedUnit>, Origin, bool);

// Queue of remaining units, backed by a reversed `Vec` so push/pop-front are both O(1).
struct TodoList<'a>(Vec<TodoItem<'a>>);

impl<'a> TodoList<'a> {
    fn new(items: Vec<TodoItem<'a>>) -> Self {
        let mut list = Self(Vec::new());
        list.restore(items);
        list
    }

    fn pop(&mut self) -> Option<TodoItem<'a>> {
        self.0.pop()
    }

    // Pushes `included`, all sharing `origin` and marked imprecise (no finer-grained source to
    // point at within it).
    fn push(&mut self, included: Vec<MacroedUnit>, origin: Origin) {
        self.0.extend(
            included
                .into_iter()
                .map(|x| (Cow::Owned(x), origin.clone(), false))
                .rev(),
        );
    }

    fn push_items(&mut self, items: Vec<TodoItem<'a>>) {
        self.0.extend(items.into_iter().rev());
    }

    fn restore(&mut self, mut items: Vec<TodoItem<'a>>) {
        items.reverse();
        self.0.extend(items);
    }
}

// What closes a decided-false `#if`/`#elseif` branch, as found by `skip_branch`.
enum BranchEnd {
    EndIf,
    // Its body is left in `todos` to flow normally.
    Else,
    Elseif {
        condition: cos::MacroInput,
        printed: String,
        origin: Origin,
        precise: bool,
    },
}

// Scans a branch's body, feeding each unit to `keep`, until finding what closes it. A `level`
// counter tells a nested `#if`'s own terminator apart from the one closing this branch.
fn scan_branch<'a>(todos: &mut TodoList<'a>, mut keep: impl FnMut(TodoItem<'a>)) -> BranchEnd {
    let mut level = 0usize;
    while let Some(item) = todos.pop() {
        let MacroedUnit::Redex(MacroRedex::H(HMacro { command, input })) = item.0.as_ref() else {
            keep(item);
            continue;
        };
        match command.deref() {
            HMacroCommand::EndIf(..) if level == 0 => return BranchEnd::EndIf,
            HMacroCommand::Else(..) if level == 0 => return BranchEnd::Else,
            HMacroCommand::Elseif(..) if level == 0 => {
                return BranchEnd::Elseif {
                    condition: input.clone(),
                    printed: item.0.print(),
                    origin: item.1,
                    precise: item.2,
                };
            }
            // Only `#endif` closes a nested `#if` — an `#elseif`/`#else` at level > 0 is just
            // one of *its* own branches, not a terminator, so it doesn't reduce the level.
            HMacroCommand::If(..) | HMacroCommand::IfDef(..) | HMacroCommand::IfNDef(..) => {
                level += 1
            }
            HMacroCommand::EndIf(..) => level -= 1,
            _ => {}
        }
        keep(item);
    }
    BranchEnd::EndIf
}

fn skip_branch(todos: &mut TodoList<'_>) -> BranchEnd {
    scan_branch(todos, |_| {})
}

fn take_branch<'a>(todos: &mut TodoList<'a>) -> (Vec<TodoItem<'a>>, BranchEnd) {
    let mut taken = Vec::new();
    let end = scan_branch(todos, |item| taken.push(item));
    (taken, end)
}

// The macro name an `#ifdef`/`#ifndef`/`#undef` names: its first word.
fn macro_name_in(input: &cos::MacroInput) -> String {
    import_macro_input(input)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string()
}

// Joins a macro's (possibly multi-line, `##CONTINUE`-terminated) body into a single string
// usable as the macro's expansion text.
fn import_macro_input(cos::MacroInput { line_i, line_z }: &cos::MacroInput) -> String {
    let mut result = String::new();
    for line in line_i {
        let printed = line.print();
        let end = printed
            .to_ascii_lowercase()
            .find("##continue")
            .unwrap_or(printed.len());
        result.push_str(&printed[..end]);
        result.push_str("\n ");
    }
    result.push_str(&line_z.print());
    result.trim_start().to_string()
}
