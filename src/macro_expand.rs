//! Macro preprocessing for routines: `#define`/`#def1arg`, `#include`, and the `$$$name` calls
//! that use them.
//!
//! `.mac`/`.int` files are type-checked in expanded form, so this module (1) collects
//! `MacroDefinitions` from a routine and the `.inc` files it includes (found in the `Workspace`,
//! or via IRIS), (2) produces the expanded text, and (3) builds a `SourceMap` that translates
//! positions in the expanded text back to the original file. The map is how diagnostics from
//! `check/` land on the right line and how hover shows "Expands to". A class's method bodies
//! are expanded one by one by `expand_method_body`, with its `Include`s' definitions and the
//! macros earlier bodies `#define`d; hover expansion is routine-only.

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    ops::Deref,
    sync::Arc,
};

use tygr::{IntoInner, OLC, Parse, Print, Ranged};
use tygr_objectscript::cos::{HMacro, MacroArg, MacroCallStruct, MacroedUnit};

use crate::{
    DocDetails, DocKind, WorkspaceStruct,
    bindings::{self, Diagnostic, Location, Position},
    check::{Expansion, IrisLookup, SyntaxScope, TypeErrorKind},
    cos::{self, HMacroCommand, MacroRedex, Routine, RoutineWithMacro},
    details::Positions,
    tygr_error_to_diagnostics,
};

// What IRIS makes available without an explicit include (verified by compiling probes on an
// IRIS instance): a class, even one with no superclass, gets the whole %systemInclude closure
// (%occInclude, %msql, %cspInclude, ...) but not %occErrors; a routine gets nothing.
pub(crate) const CLASS_IMPLICIT_INCLUDES: &[&str] = &["%systemInclude"];

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
    // `##safeexpression`/`##expression`/`##function` run ObjectScript at compile time;
    // `check::mod` knows the common ones (`$$$ERROR`, ...) by name instead.
    fn is_compile_time(&self) -> bool {
        ["##safeexpression", "##expression", "##function"]
            .iter()
            .any(|directive| self.body.contains(directive))
    }

    // `##function` splices the arguments into code it runs at compile time, inside a string as
    // often as not (`"%s"="*"`), so they need not be expressions.
    pub(crate) fn splices_text(&self) -> bool {
        self.body.contains("##function")
    }
}

pub(crate) type MacroDefinitions = HashMap<String, MacroDefinition>;

// What the method bodies compiled so far `#define`d, and in which method: IRIS compiles methods
// in name order and keeps each body's definitions for the ones after it.
#[derive(Clone, Default)]
pub(crate) struct BodyMacros {
    pub(crate) definitions: MacroDefinitions,
    pub(crate) methods: HashMap<String, String>,
}

// The definitions in force while expanding: a layer of its own, where `#define`s land, over the
// base a class shares with every method body, read and never copied.
#[derive(Default)]
pub(crate) struct Definitions<'b> {
    base: Option<&'b MacroDefinitions>,
    // `None` is an `#undef` of a base definition.
    own: HashMap<String, Option<MacroDefinition>>,
}

impl<'b> Definitions<'b> {
    pub(crate) fn over(base: &'b MacroDefinitions) -> Self {
        Definitions {
            base: Some(base),
            own: HashMap::new(),
        }
    }

    pub(crate) fn get(&self, name: &str) -> Option<&MacroDefinition> {
        match self.own.get(name) {
            Some(own) => own.as_ref(),
            None => self.base.and_then(|base| base.get(name)),
        }
    }

    fn contains_key(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    fn insert(&mut self, name: String, definition: MacroDefinition) {
        self.own.insert(name, Some(definition));
    }

    fn remove(&mut self, name: &str) {
        if self.base.is_some_and(|base| base.contains_key(name)) {
            self.own.insert(name.to_string(), None);
        } else {
            self.own.remove(name);
        }
    }

    // Its own layer, the base left out.
    pub(crate) fn own(&self) -> impl Iterator<Item = (&String, &MacroDefinition)> {
        self.own
            .iter()
            .filter_map(|(name, definition)| Some((name, definition.as_ref()?)))
    }

    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        let base = self.base.into_iter().flat_map(|base| base.keys());
        base.filter(move |name| !self.own.contains_key(*name))
            .chain(self.own().map(|(name, _)| name))
            .map(String::as_str)
    }

    fn into_own(self) -> MacroDefinitions {
        self.own
            .into_iter()
            .filter_map(|(name, definition)| Some((name, definition?)))
            .collect()
    }
}

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
            range,
            ..self.clone()
        }
    }

    fn narrowed(&self, precise: bool, range: std::ops::Range<usize>) -> Self {
        if precise {
            self.with_range(range)
        } else {
            self.clone()
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

/// How faithfully a source range stands for a range of expanded text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fidelity {
    /// Strictly inside one unbroken run of source text.
    Exact,
    /// Inside one run, but starting or ending at a macro's edge: an invocation's or a pasted argument's.
    AtBoundary,
    /// In text a macro body generated, or across an edge: the range covers the text, an endpoint
    /// inside an expansion standing for its whole invocation.
    Inexact,
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
        self.include_directives
            .iter()
            .map(|(span, name)| (self.source_olc.span(span.start, span.end), name.as_str()))
    }

    // Every `#include` name this expansion asked for, resolved or not, nested ones included.
    pub(crate) fn include_names(&self) -> impl Iterator<Item = &String> {
        self.include_names.iter()
    }

    // `None` past the expanded text, e.g. another member's declaration sharing this uri.
    fn offset(&self, position: Position) -> Option<usize> {
        let line_character = (position.line as usize, position.character as usize);
        OLC::line_character_to_offset(&self.expanded_olc, line_character).ok()
    }

    pub(crate) fn map_position(&self, position: Position) -> Option<(Position, Fidelity)> {
        let (offset, fidelity) = self.map_point(self.offset(position)?)?;
        Some((self.source_olc.position(offset), fidelity))
    }

    pub(crate) fn map_lsp_range(
        &self,
        range: bindings::Range,
    ) -> Option<(bindings::Range, Fidelity)> {
        let expanded = self.offset(range.start)?..self.offset(range.end)?;
        let (source, fidelity) = self.map_range(expanded)?;
        Some((self.source_olc.span(source.start, source.end), fidelity))
    }

    // What the macro invocation at `position` (in the routine's original, unexpanded text)
    // ultimately expanded to — recursively, so a macro that itself calls other macros shows
    // its full expansion, not just its own body's literal text. `None` if `position` isn't
    // inside any macro invocation.
    pub(crate) fn macro_expansion_at_position(&self, position: Position) -> Option<String> {
        if position.line as usize >= self.source_lines {
            return None;
        }
        let offset = self.source_olc.offset(position);
        // The innermost invocation holding `offset`: a nested call's argument is recorded too.
        let idx = self
            .invocations
            .partition_point(|span| span.start <= offset);
        let span = self.invocations[..idx]
            .iter()
            .rev()
            .find(|span| offset < span.end)?;
        // The pieces tile the expansion, so a gap starts another copy of an argument the body
        // pastes more than once.
        let mut end = None;
        let text: String = (self.vec.iter())
            .filter(|(_, origin, _)| {
                origin.uri.is_none()
                    && origin.range.start >= span.start
                    && origin.range.end <= span.end
            })
            .take_while(|(expanded, ..)| {
                let next = end.is_none_or(|end| end == expanded.start);
                end = Some(expanded.end);
                next
            })
            .map(|(expanded, ..)| &self.expanded_text[expanded.clone()])
            .collect();
        (!text.is_empty()).then_some(text)
    }

    fn locate_idx(&self, pos: usize) -> Option<usize> {
        let idx = self
            .vec
            .partition_point(|(expanded, ..)| expanded.start <= pos);
        let idx = idx.checked_sub(1)?;
        (pos <= self.vec.get(idx)?.0.end).then_some(idx)
    }

    // Pieces `i..=j` as one run of source text: precise, one file, contiguous.
    fn unbroken(&self, i: usize, j: usize) -> bool {
        let pieces = &self.vec[i..=j];
        (pieces.iter()).all(|(_, origin, precise)| *precise && origin.uri == pieces[0].1.uri)
            && (pieces.windows(2)).all(|pair| pair[0].1.range.end == pair[1].1.range.start)
    }

    // The end is exclusive, so it is located through the last character covered: a range ending
    // where a pasted argument ends must not take the position of what the body puts next.
    fn locate_range(
        &self,
        expanded: std::ops::Range<usize>,
    ) -> Option<(&Origin, std::ops::Range<usize>, Fidelity)> {
        let std::ops::Range { start, end } = expanded;
        let mut i = self.locate_idx(start)?;
        let mut j = if end > start {
            self.locate_idx(end - 1)?
        } else {
            i
        };
        // A point where an expansion begins is the end of the source text before it.
        if end == start
            && i > 0
            && self.vec[i].0.start == start
            && !self.unbroken(i, i)
            && self.unbroken(i - 1, i - 1)
        {
            (i, j) = (i - 1, i - 1);
        }
        let (first, last) = (&self.vec[i], &self.vec[j]);
        let at = |(expanded, origin, _): &(std::ops::Range<usize>, Origin, bool), offset| {
            origin.range.start + (offset - expanded.start)
        };
        if first.1.uri != last.1.uri {
            return Some((&first.1, first.1.range.clone(), Fidelity::Inexact));
        }
        if !self.unbroken(i, j) {
            let start = if first.2 {
                at(first, start)
            } else {
                first.1.range.start
            };
            let end = if last.2 {
                at(last, end)
            } else {
                last.1.range.end
            };
            return Some((&first.1, start..end, Fidelity::Inexact));
        }
        // The document's own edges are no macro's.
        let at_boundary = (start == first.0.start && i > 0 && !self.unbroken(i - 1, i))
            || (end == last.0.end && j + 1 < self.vec.len() && !self.unbroken(j, j + 1));
        let fidelity = if at_boundary {
            Fidelity::AtBoundary
        } else {
            Fidelity::Exact
        };
        Some((&first.1, at(first, start)..at(last, end), fidelity))
    }

    // `None` for text an included file supplied, or outside this expansion.
    pub(crate) fn map_range(
        &self,
        expanded: std::ops::Range<usize>,
    ) -> Option<(std::ops::Range<usize>, Fidelity)> {
        let (origin, range, fidelity) = self.locate_range(expanded)?;
        origin.uri.is_none().then_some((range, fidelity))
    }

    pub(crate) fn map_point(&self, offset: usize) -> Option<(usize, Fidelity)> {
        let (range, fidelity) = self.map_range(offset..offset)?;
        Some((range.start, fidelity))
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
        Some(olc.span(range.start, range.end))
    }

    // The fully macro-expanded text of the physical line containing `offset` — not just the
    // origin of the sub-span a diagnostic happens to point at, since a macro's own expansion
    // can splice further macros (each with their own origin) and unbraced control commands
    // (`if`, `for`, ...) that swallow the rest of the line into their scope. Showing the whole
    // line is what actually explains the diagnostic in those cases. Also returns the span of
    // expanded text it shows.
    fn expanded_line_text(&self, offset: usize) -> Option<(std::ops::Range<usize>, String)> {
        let start = self.expanded_text[..offset]
            .rfind('\n')
            .map_or(0, |i| i + 1);
        let end = self.expanded_text[offset..]
            .find('\n')
            .map_or(self.expanded_text.len(), |i| offset + i);
        const MAX_LEN: usize = 200;
        let line = &self.expanded_text[start..end];
        let text = line.trim();
        if text.is_empty() {
            return None;
        }
        let start = start + (line.len() - line.trim_start().len());
        let span = start..start + text.len();
        if text.len() <= MAX_LEN {
            return Some((span, text.to_string()));
        }
        let mut cut = MAX_LEN;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        // Marked as cut, so a long expansion isn't mistaken for the whole of it.
        Some((span, format!("{}…", &text[..cut])))
    }

    // What the line at `position` expanded to, located at the source of that line in the file it
    // came from, if `position` only exists because of a macro substitution. `uri` is the
    // top-level file's.
    pub(crate) fn expansion_info(
        &self,
        position: Position,
        uri: &str,
    ) -> Option<bindings::DiagnosticRelatedInformation> {
        let offset = self.offset(position)?;
        if self.vec[self.locate_idx(offset)?].2 {
            return None;
        }
        let (span, text) = self.expanded_line_text(offset)?;
        let (origin, line, _) = self.locate_range(span)?;
        let range = self.range_in(&origin.uri, &line)?;
        let line_uri = origin.uri.clone().unwrap_or_else(|| uri.to_string());
        Some(bindings::DiagnosticRelatedInformation {
            location: Location::new(line_uri, range),
            message: format!("↳ Expands to: {text}"),
        })
    }

    // `range` in the top-level file, or at the `#include` that pulled its text in, with one
    // related-information entry per further hop and, when `Inexact`, the expansion.
    fn relocate_range(
        &self,
        range: &bindings::Range,
        uri: &str,
    ) -> Option<(
        bindings::Range,
        Fidelity,
        Vec<bindings::DiagnosticRelatedInformation>,
    )> {
        let expanded = self.offset(range.start)?..self.offset(range.end)?;
        let (origin, leaf, fidelity) = self.locate_range(expanded)?;
        let mut chain = vec![(&origin.uri, leaf)];
        let ancestors = std::iter::successors(origin.parent.as_deref(), |o| o.parent.as_deref());
        chain.extend(ancestors.map(|o| (&o.uri, o.range.clone())));
        let (top_uri, top) = chain.pop()?;
        debug_assert!(top_uri.is_none());
        let mut hops: Vec<_> = (chain.iter().rev())
            .filter_map(|(uri, range)| {
                let uri = (*uri).clone()?;
                let range = self.range_in(&Some(uri.clone()), range)?;
                Some(bindings::DiagnosticRelatedInformation {
                    location: Location::new(uri, range),
                    message: "↳ In the included file.".into(),
                })
            })
            .collect();
        if fidelity == Fidelity::Inexact {
            hops.extend(self.expansion_info(range.start, uri));
        }
        Some((self.source_olc.span(top.start, top.end), fidelity, hops))
    }

    pub(crate) fn expanded_text(&self) -> &str {
        &self.expanded_text
    }

    // An `Inexact` range goes to the invocation holding it; an `S` finding there is the macro's
    // spelling, not the document's, and is dropped.
    pub(crate) fn relocate_diagnostics(
        &self,
        diagnostics: Vec<bindings::Diagnostic>,
        uri: &str,
    ) -> Vec<bindings::Diagnostic> {
        diagnostics
            .into_iter()
            .filter_map(|mut diagnostic| {
                let mut related_information = match self.relocate_range(&diagnostic.range, uri) {
                    Some((_, Fidelity::Inexact, _)) if diagnostic.code.starts_with('S') => {
                        return None;
                    }
                    Some((range, _, hops)) => {
                        diagnostic.range = range;
                        hops
                    }
                    None => vec![],
                };
                for mut info in std::mem::take(&mut diagnostic.related_information) {
                    let hops = if info.location.uri == uri
                        && let Some((range, _, hops)) =
                            self.relocate_range(&info.location.range, uri)
                    {
                        info.location.range = range;
                        hops
                    } else {
                        vec![]
                    };
                    related_information.push(info);
                    related_information.extend(hops);
                }
                diagnostic.related_information = related_information;
                Some(diagnostic)
            })
            .collect()
    }

    fn relocate_error(&self, error: tygr::Error, uri: &str, scope: SyntaxScope) -> Diagnostic {
        let error = tygr_error_to_diagnostics(error, &self.expanded_text, scope);
        self.relocate_diagnostics(vec![error], uri).pop().unwrap()
    }
}

// An include's uri, full text, and un-substituted macro units.
pub(crate) type Include = Arc<(String, String, Vec<Ranged<MacroedUnit>>)>;

// An `Include` that didn't resolve, with why IRIS couldn't be asked for it, when that's why.
pub(crate) type FailedInclude = (String, Option<String>);

impl WorkspaceStruct {
    // Resolves an `#include name`: an already-open `.inc` document first, then (if connected)
    // by fetching it from the server -- covers a system or shared include that isn't part of
    // the local editor workspace. Returns its uri (the host's own if it offered one, else a
    // synthetic placeholder just for origin-tracking), full text, and un-substituted macro units.
    fn resolve_include(&self, name: &str) -> Result<Option<Include>, String> {
        if let Some(uri) = self.resolve_uri(DocKind::Inc, name) {
            // `uri`'s own tier B -- just its header and un-substituted macro units, computed
            // and cached (via `ensure_details`) the moment it's first `#include`d.
            let Ok(DocDetails::Inc(parsed)) = self.ensure_details(&uri) else {
                return Ok(None);
            };
            let Some(text) = self.doc_text(&uri) else {
                return Ok(None);
            };
            return Ok(Some(Arc::new((uri, text, parsed.macroed_units.clone()))));
        }
        if let Some(include) = self.iris_includes.borrow().get(name) {
            return Ok(include.clone());
        }
        let Some(iris) = self.iris.as_ref() else {
            return Ok(None);
        };
        let include = iris.get_inc(name)?.and_then(|routine| {
            let parsed = RoutineWithMacro::parse(&routine.text).ok()?;
            let uri = routine
                .uri
                .unwrap_or_else(|| format!("iris-include:///{name}.inc"));
            Some(Arc::new((uri, routine.text, parsed.macroed_units)))
        });
        self.iris_includes
            .borrow_mut()
            .insert(name.to_string(), include.clone());
        Ok(include)
    }

    fn expand_rtn_helper(
        &self,
        src: &str,
        dones: &mut DoneList,
    ) -> Result<(Routine, MacroDefinitions), tygr::Error> {
        let mut definitions = Definitions::default();
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
            .is_some_and(|spec| matches!(spec.routine_type(), Some(cos::RoutineType::Int(_))))
        {
            for unit in &macroed_units {
                dones.push(
                    &unit.print(),
                    &Origin::top_level(unit.start..unit.end),
                    true,
                );
            }
            return Ok((Routine::parse(&dones.text)?, definitions.into_own()));
        }
        let mut todos = TodoList::new(precise_items(&macroed_units, Origin::top_level));
        self.expand_units(&mut definitions, &mut todos, dones)?;
        Ok((Routine::parse(&dones.text)?, definitions.into_own()))
    }

    // The substitution loop: `#define`/`#include`/macro-calls update `definitions` or requeue
    // their expansion onto `todos`; `#if` is resolved by `resolve_if`; everything else prints
    // as-is via `dones`.
    fn expand_units(
        &self,
        definitions: &mut Definitions<'_>,
        todos: &mut TodoList<'_>,
        dones: &mut DoneList,
    ) -> Result<(), tygr::Error> {
        while let Some((unit, origin, precise)) = todos.pop() {
            match unit.as_ref() {
                MacroedUnit::Plain(unit) => dones.push_plain(&unit.print(), &origin, precise),
                MacroedUnit::Redex(redex) => match redex {
                    MacroRedex::H(HMacro { command, input }) => match command.deref() {
                        HMacroCommand::Define { name, args, .. } => {
                            let params = args.as_ref().map(|args| {
                                (args.iter().flat_map(|list| list.items.iter()))
                                    .map(|arg| arg.print())
                                    .collect()
                            });
                            define(definitions, name, params, false, input);
                            dones.body_macros.remove(&name.print());
                        }
                        HMacroCommand::Def1arg { name, arg, .. } => {
                            let params = arg.as_ref().map(|arg| vec![arg.deref().print()]);
                            define(definitions, name, params, true, input);
                            dones.body_macros.remove(&name.print());
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
                                Ok(Some(include)) if dones.included.contains_key(&include.0) => {}
                                Ok(Some(include)) => {
                                    let (inc_uri, inc_text, inc_units) = &*include;
                                    dones.note_included_file(inc_uri.clone(), inc_text);
                                    let items = inc_units
                                        .iter()
                                        .map(|item| {
                                            let inc_range = item.start..item.end;
                                            (
                                                Cow::Owned(item.deref().clone()),
                                                origin
                                                    .for_included_file(inc_uri.clone(), inc_range),
                                                true,
                                            )
                                        })
                                        .collect();
                                    todos.push_items(items);
                                }
                                other => dones.note_failed_include(
                                    included_name.print(),
                                    other.err(),
                                    &origin,
                                ),
                            }
                        }
                        HMacroCommand::Noshow(..) | HMacroCommand::Show(..) => (),
                        HMacroCommand::If(..) => {
                            self.resolve_if(definitions, todos, dones, (unit, origin, precise))?;
                        }
                        HMacroCommand::IfDef(..) | HMacroCommand::IfNDef(..) => {
                            let defined = definitions.contains_key(&macro_name_in(input));
                            let keep =
                                defined == matches!(command.deref(), HMacroCommand::IfDef(..));
                            self.resolve_branch(definitions, todos, dones, keep)?;
                        }
                        HMacroCommand::Undef(..) => {
                            let name = macro_name_in(input);
                            definitions.remove(&name);
                            dones.body_macros.remove(&name);
                        }
                        _ => dones.push(&unit.print(), &origin, precise),
                    },
                    MacroRedex::DDD(ddd) => {
                        dones.note_invocation(&origin);
                        let MacroCallStruct { name, args } = ddd.deref();
                        // The unit's own offsets, counted from its `$$$`, mapped into `origin`.
                        let unit_start = name.0.start - 3;
                        let at = |offset: usize| origin.range.start + (offset - unit_start);
                        let compile_time = definitions
                            .get(&name.print())
                            .is_some_and(|def| def.is_compile_time());
                        if let Some(method) = dones.body_macros.get(&name.print()).cloned()
                            && definitions.contains_key(&name.print())
                        {
                            dones.note_unresolved(
                                move |name| TypeErrorKind::MacroFromMethodBody {
                                    name,
                                    method: method.clone(),
                                },
                                name.print(),
                                &origin,
                            );
                        }
                        let substituted = match definitions.get(&name.print()) {
                            Some(_) if origin.depth >= MAX_MACRO_DEPTH => {
                                dones.note_unresolved(
                                    |name| TypeErrorKind::MacroExpansionTooDeep { name },
                                    name.print(),
                                    &origin,
                                );
                                false
                            }
                            Some(def) if def.is_compile_time() => false,
                            // `$$$M` for an `M()` declared with no parameters: the parentheses
                            // are optional at the call. `$$$M(args)` on a parameterless `M` keeps
                            // the argument list: the body is typically a method name the
                            // arguments then call.
                            Some(MacroDefinition { params, body, .. })
                                if params.is_none()
                                    || (args.is_none()
                                        && matches!(params.as_deref(), Some([]))) =>
                            {
                                let deeper = origin.deeper();
                                let mut items: Vec<TodoItem> = <Vec<MacroedUnit>>::parse(body)?
                                    .into_iter()
                                    .map(|unit| (Cow::Owned(unit), deeper.clone(), false))
                                    .collect();
                                if let Some(args) = args.as_ref() {
                                    let start = at(name.0.end);
                                    for unit in <Vec<Ranged<MacroedUnit>>>::parse(&args.print())? {
                                        let piece = origin.narrowed(
                                            precise,
                                            start + unit.start..start + unit.end,
                                        );
                                        items.push((Cow::Owned(unit.into_inner()), piece, precise));
                                    }
                                }
                                todos.push_items(items);
                                true
                            }
                            Some(MacroDefinition {
                                params: Some(pars),
                                literal_args: true,
                                body,
                            }) if let Some(args) = args.as_deref() => {
                                let text = args
                                    .items
                                    .iter()
                                    .map(|arg| arg.print())
                                    .collect::<Vec<_>>()
                                    .join(",");
                                let arg_origin = match (args.items.first(), args.items.last()) {
                                    (Some(first), Some(last)) if precise => {
                                        origin.with_range(at(first.start)..at(last.end))
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
                                params: Some(pars), ..
                            }) if let Some(args) = args.as_deref()
                                && args.items.len() != pars.len() =>
                            {
                                let expected = pars.len();
                                let too_many = args.items.len() > expected;
                                dones.note_unresolved(
                                    move |name| {
                                        if too_many {
                                            TypeErrorKind::MacroTooManyArguments { name, expected }
                                        } else {
                                            TypeErrorKind::MacroTooFewArguments { name, expected }
                                        }
                                    },
                                    name.print(),
                                    &origin,
                                );
                                false
                            }
                            Some(MacroDefinition {
                                params: Some(pars),
                                body,
                                ..
                            }) if let Some(args) = args.as_deref() => {
                                let mut param_map: Vec<(&String, String, Origin, bool)> = pars
                                    .iter()
                                    .zip(args.items.iter())
                                    .map(|(par, arg)| {
                                        let arg_origin =
                                            origin.narrowed(precise, at(arg.start)..at(arg.end));
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
                                    let arg_span = |arg: &Ranged<MacroArg>| {
                                        arg.start - unit_start..arg.end - unit_start
                                    };
                                    // Precise means `text` sits at `origin.range`, so it's pushed
                                    // piecewise: an argument is expanded at its own span, so what
                                    // stays literal in it keeps its place, and the rest is exact.
                                    let piecewise = precise
                                        && args.items.iter().all(|arg| {
                                            text.get(arg_span(arg)) == Some(arg.print().as_str())
                                        });
                                    if piecewise {
                                        let mut done = 0;
                                        for arg in args.items.iter() {
                                            let span = arg_span(arg);
                                            let piece =
                                                origin.with_range(base + done..base + span.start);
                                            dones.push(&text[done..span.start], &piece, true);
                                            let piece = origin
                                                .with_range(base + span.start..base + span.end);
                                            let expanded = self.expand_arg(
                                                definitions,
                                                &text[span.clone()],
                                                compile_time,
                                                Some(&piece),
                                            )?;
                                            dones.push_expanded(expanded);
                                            done = span.end;
                                        }
                                        let piece =
                                            origin.with_range(base + done..origin.range.end);
                                        dones.push(&text[done..], &piece, true);
                                    } else {
                                        let mut arg_texts = Vec::with_capacity(args.items.len());
                                        for arg in args.items.iter() {
                                            arg_texts.push(
                                                self.expand_arg(
                                                    definitions,
                                                    &arg.print(),
                                                    compile_time,
                                                    None,
                                                )?
                                                .text,
                                            );
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
                    // Only `text` is pushed, so a precise origin is narrowed past the `##lit(`.
                    MacroRedex::Lit(open, text, _) => {
                        let start = origin.range.start + open.print().len();
                        let piece = origin.narrowed(precise, start..start + text.print().len());
                        dones.push(&text.print(), &piece, precise)
                    }
                    MacroRedex::BeginLit(open, text, _) => {
                        let start = origin.range.start + open.print().len();
                        let piece = origin.narrowed(precise, start..start + text.print().len());
                        dones.push(&text.print(), &piece, precise)
                    }
                    // A comment included: matched whole, nothing in it expands, and a check may
                    // read it.
                    unit => dones.push(&unit.print(), &origin, precise),
                },
            }
        }
        Ok(())
    }

    // Expands `text` in isolation -- e.g. one `#if` condition, or one argument to a call that
    // otherwise didn't resolve as a `#define`d macro (see `MacroRedex::DDD`); given the `origin`
    // `text` sits at, what stays literal is mapped to its place. Always terminates: it only ever
    // recurses on strictly smaller, nested argument text.
    // An argument of a compile-time macro (`$$$ERROR(...)`): IRIS resolves the error names in it
    // against %occErrors whether or not the file includes it, so those count as defined here.
    fn expand_arg(
        &self,
        definitions: &mut Definitions<'_>,
        text: &str,
        compile_time: bool,
        origin: Option<&Origin>,
    ) -> Result<DoneList, tygr::Error> {
        if !compile_time || !text.contains("$$$") {
            return self.expand_text(definitions, text, origin);
        }
        if self.error_names.borrow().is_none() {
            let (names, _, _) = self.resolve_class_includes(&["%occErrors"]);
            *self.error_names.borrow_mut() = Some(names);
        }
        let error_names = self.error_names.borrow();
        let error_names = error_names.as_ref().unwrap();
        let mut added = Vec::new();
        for rest in text.split("$$$").skip(1) {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if let Some(def) = error_names.get(&name)
                && !definitions.contains_key(&name)
            {
                definitions.insert(name.clone(), def.clone());
                added.push(name);
            }
        }
        let expanded = self.expand_text(definitions, text, origin);
        for name in added {
            definitions.remove(&name);
        }
        expanded
    }

    fn expand_text(
        &self,
        definitions: &mut Definitions<'_>,
        text: &str,
        origin: Option<&Origin>,
    ) -> Result<DoneList, tygr::Error> {
        let mut dones = DoneList::default();
        let units = <Vec<Ranged<MacroedUnit>>>::parse(text)?;
        let mut todos = TodoList::new(match origin {
            Some(origin) => precise_items(&units, |range| {
                origin.with_range(origin.range.start + range.start..origin.range.start + range.end)
            }),
            None => units
                .iter()
                .map(|unit| (Cow::Borrowed(unit.deref()), Origin::top_level(0..0), false))
                .collect(),
        });
        self.expand_units(definitions, &mut todos, &mut dones)?;
        Ok(dones)
    }

    // Resolves the `#if`/`#elseif` `item` by its condition: "1" keeps its branch, "0" drops it,
    // and anything else is unresolved (see `resolve_branch`).
    fn resolve_if(
        &self,
        definitions: &mut Definitions<'_>,
        todos: &mut TodoList<'_>,
        dones: &mut DoneList,
        item: TodoItem<'_>,
    ) -> Result<(), tygr::Error> {
        let MacroedUnit::Redex(MacroRedex::H(HMacro { command, input })) = item.0.as_ref() else {
            unreachable!("resolve_if is only handed an #if or #elseif");
        };
        let directive = if matches!(command.prefixed, HMacroCommand::Elseif(..)) {
            "#elseif"
        } else {
            "#if"
        };
        // `#if 0 /* Maintenance */` -- a trailing comment is common and not part of the test.
        fn condition(text: &str) -> &str {
            let end = ["/*", "//", ";"]
                .iter()
                .filter_map(|marker| text.find(marker))
                .min()
                .unwrap_or(text.len());
            text[..end].trim()
        }
        let written = import_macro_input(input);
        let expanded = self.expand_text(definitions, &written, None)?.text;
        let (written, expanded) = (condition(&written), condition(&expanded));
        let decided = match expanded {
            "1" => true,
            "0" => false,
            _ => {
                let expansion = (expanded != written).then(|| expanded.to_string());
                dones.note_unresolved(
                    move |written| TypeErrorKind::UndecidedIf {
                        directive,
                        condition: written,
                        expansion: Expansion(expansion.clone()),
                    },
                    written.to_string(),
                    &item.1,
                );
                true
            }
        };
        self.resolve_branch(definitions, todos, dones, decided)
    }

    // Keeps a taken branch (discarding the rest of the chain); drops
    // a dropped one and tries the closing `#elseif` in turn.
    fn resolve_branch(
        &self,
        definitions: &mut Definitions<'_>,
        todos: &mut TodoList<'_>,
        dones: &mut DoneList,
        taken: bool,
    ) -> Result<(), tygr::Error> {
        if taken {
            let mut branch = Vec::new();
            let mut end = scan_branch(todos, |item| branch.push(item));
            while !matches!(end, BranchEnd::EndIf) {
                end = scan_branch(todos, |_| {});
            }
            todos.push_items(branch);
            return Ok(());
        }
        match scan_branch(todos, |_| {}) {
            BranchEnd::EndIf | BranchEnd::Else => Ok(()),
            BranchEnd::Elseif(elseif) => self.resolve_if(definitions, todos, dones, *elseif),
        }
    }

    pub fn expand_rtn(
        &self,
        uri: &str,
        src: &str,
    ) -> Result<(Routine, SourceMap, MacroDefinitions), Vec<Diagnostic>> {
        let mut dones = DoneList::default();
        let result = self.expand_rtn_helper(src, &mut dones);
        let failed_include = !dones.failed_includes.is_empty();
        let map = dones.into_source_map(src);
        match result {
            Ok((routine, definitions)) => Ok((routine, map, definitions)),
            // A macro left unexpanded by an unresolved `#include` often doesn't re-parse (same
            // failure mode `note_failed_include` already calls out for `#include` itself) --
            // report the missing include(s), the actionable root cause, instead of the resulting
            // generic parse error. Anything else noted (an undecided `#if`, a call's arity) leaves
            // the text as written, so the parse error is the routine's own.
            Err(error) => {
                let error =
                    (!failed_include).then(|| map.relocate_error(error, uri, SyntaxScope::Routine));
                Err(map.unresolved.into_iter().chain(error).collect())
            }
        }
    }

    pub(crate) fn resolve_class_includes(
        &self,
        names: &[impl AsRef<str>],
    ) -> (MacroDefinitions, Vec<FailedInclude>, HashSet<String>) {
        let mut definitions = Definitions::default();
        let mut failed = Vec::new();
        let mut include_names = HashSet::new();
        for name in names {
            let name = name.as_ref();
            include_names.insert(name.to_string());
            match self.resolve_include(name) {
                Ok(Some(include)) => {
                    let (inc_uri, inc_text, inc_units) = &*include;
                    let mut dones = DoneList::default();
                    dones.note_included_file(inc_uri.clone(), inc_text);
                    let mut todos = TodoList::new(precise_items(inc_units, |range| {
                        Origin::top_level(0..0).for_included_file(inc_uri.clone(), range)
                    }));
                    let _ = self.expand_units(&mut definitions, &mut todos, &mut dones);
                    // Whatever the include itself included: an edit there changes this class too.
                    include_names.extend(dones.include_names);
                    failed.extend(dones.failed_includes);
                }
                other => failed.push((name.to_string(), other.err())),
            }
        }
        (definitions.into_own(), failed, include_names)
    }

    // The labels the include files' own code lines define, callable from the including class's
    // method bodies as from a routine.
    pub(crate) fn include_labels(&self, names: &[String]) -> crate::check::Labels {
        let mut labels = crate::check::Labels::new();
        for name in names {
            if let Ok(Some(include)) = self.resolve_include(name) {
                let (uri, text, _) = &*include;
                if let Ok((routine, map, _)) = self.expand_rtn(uri, text) {
                    labels.extend(crate::check::labels_of(&map, &routine, Some(uri)));
                }
            }
        }
        labels
    }

    pub(crate) fn implicit_class_definitions(&self) -> (MacroDefinitions, HashSet<String>) {
        self.implicit_definitions
            .borrow_mut()
            .get_or_insert_with(|| {
                let (definitions, _, names) = self.resolve_class_includes(CLASS_IMPLICIT_INCLUDES);
                (definitions, names)
            })
            .clone()
    }

    // `header` is pushed verbatim, unexpanded, ahead of the expanded `body` -- like
    // `expand_rtn_helper` pushes a routine's own header -- so both share one coordinate space.
    pub(crate) fn expand_method_body<'b>(
        &self,
        uri: &str,
        method: &str,
        header: &str,
        body: &str,
        base_definitions: &'b MacroDefinitions,
        body_macros: &BodyMacros,
    ) -> Result<(String, SourceMap, Definitions<'b>, BodyMacros), Diagnostic> {
        let mut dones = DoneList {
            body_macros: body_macros.methods.clone(),
            ..Default::default()
        };
        // Carried definitions join the layer; its final state is what the next method inherits.
        let mut definitions = Definitions::over(base_definitions);
        for (name, definition) in &body_macros.definitions {
            definitions.insert(name.clone(), definition.clone());
        }
        // Tokenized together so every offset, including a macro argument's, is relative to the
        // same text; the header itself is copied verbatim (a `{$$$X}` parameter default must not
        // change its length, or the body would be cut at the wrong offset).
        let text = format!("{header}{body}");
        let result = (|| -> Result<(), tygr::Error> {
            let units = <Vec<Ranged<MacroedUnit>>>::parse(&text)?;
            let (in_header, in_body): (Vec<_>, Vec<_>) =
                units.iter().partition(|x| x.end <= header.len());
            for x in in_header {
                dones.push(&x.print(), &Origin::top_level(x.start..x.end), true);
            }
            let mut todos = TodoList::new(precise_items(in_body, Origin::top_level));
            self.expand_units(&mut definitions, &mut todos, &mut dones)
        })();
        // Measured on IRIS 2026.2: a redefinition replaces the carried definition and an `#undef`
        // ends it, while a redefinition of a name the class defines stays in its method.
        let mut body_macros = BodyMacros::default();
        for (name, definition) in definitions.own() {
            if !base_definitions.contains_key(name) {
                let owner = dones.body_macros.get(name).cloned();
                let owner = owner.unwrap_or_else(|| method.to_string());
                body_macros
                    .definitions
                    .insert(name.clone(), definition.clone());
                body_macros.methods.insert(name.clone(), owner);
            }
        }
        let map = dones.into_source_map(&text);
        match result {
            Ok(()) => {
                let expanded_text = map.expanded_text.clone();
                Ok((expanded_text, map, definitions, body_macros))
            }
            Err(error) => Err(map.relocate_error(error, uri, SyntaxScope::MethodBody)),
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
                // Only the whole name: the `%m` that starts `%maps` is not the parameter. What
                // precedes it doesn't matter, since a name can't contain `%`: `%New%args` is
                // `%New` and then the parameter.
                let whole = rest.match_indices(name.as_str()).find(|(idx, _)| {
                    !rest[idx + name.len()..].starts_with(|c: char| c.is_alphanumeric())
                });
                match whole.map(|(idx, _)| idx) {
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
                        let (mut before, mut after) = (&rest[..idx], &rest[idx + name.len()..]);
                        // IRIS's preprocessor pastes a quoted argument into `$g(%m)` as the string
                        // alone: it drops the `$g(` and the one character after the parameter,
                        // taken for the `)`. Its own classes rely on it. `$$g(` is a call.
                        if replacement.starts_with('"')
                            && before.len() >= 3
                            && before.is_char_boundary(before.len() - 3)
                            && before[before.len() - 3..].eq_ignore_ascii_case("$g(")
                            && !before[..before.len() - 3].ends_with('$')
                        {
                            before = &before[..before.len() - 3];
                            after = after
                                .get(after.chars().next().map_or(0, char::len_utf8)..)
                                .unwrap_or("");
                        }
                        if !before.is_empty() {
                            next.push(Span {
                                text: before.to_string(),
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
                        rest = after;
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
    for unit in <Vec<Ranged<MacroedUnit>>>::parse(&text)? {
        let mut overlapping = bounds
            .iter()
            .filter(|(start, end, _)| *start < unit.end && unit.start < *end);
        match unit.deref() {
            MacroedUnit::Plain(_) => {
                for (start, end, span) in overlapping {
                    let piece_start = unit.start.max(*start);
                    let piece = &text[piece_start..unit.end.min(*end)];
                    for part in <Vec<Ranged<MacroedUnit>>>::parse(piece)? {
                        // A precise span's text sits at its origin, so a part of it is at the
                        // same offset into the origin.
                        let at = span.origin.range.start + (piece_start - start);
                        let origin = span
                            .origin
                            .narrowed(span.precise, at + part.start..at + part.end);
                        items.push((Cow::Owned(part.into_inner()), origin, span.precise));
                    }
                }
            }
            // Inside one precise span, as precise as a part of it; across spans, the call's.
            MacroedUnit::Redex(_) => {
                let (origin, precise) = match (overlapping.next(), overlapping.next()) {
                    (Some((start, _, span)), None) if span.precise => {
                        let at = span.origin.range.start + (unit.start - start);
                        (span.origin.with_range(at..at + unit.end - unit.start), true)
                    }
                    (Some((_, _, span)), None) => (span.origin.clone(), false),
                    _ => (call_origin.clone(), false),
                };
                items.push((Cow::Owned(unit.into_inner()), origin, precise));
            }
        }
    }
    Ok(items)
}

// Builds the diagnostic's kind for something left unresolved; `into_source_map` places it.
type UnresolvedError = Box<dyn Fn(String) -> TypeErrorKind>;

#[derive(Default)]
struct DoneList {
    text: String,
    map: SourceMapVec,
    included: HashMap<String, OLC>,
    invocations: Vec<std::ops::Range<usize>>,
    // `#include`s naming a file not open in the workspace — dropped rather than spliced back in
    // literally, since the raw `#include Name` text rarely re-parses and would otherwise mask
    // this diagnostic behind a generic syntax error.
    unresolved: Vec<(UnresolvedError, String, Origin)>,
    // The `#include`s among them, for a class, whose `Include` line reports them itself.
    failed_includes: Vec<FailedInclude>,
    include_directives: Vec<(std::ops::Range<usize>, String)>,
    include_names: HashSet<String>,
    // Macros `#define`d in an earlier method's body, by the method's name: usable, but warned.
    body_macros: HashMap<String, String>,
}

impl DoneList {
    fn note_failed_include(&mut self, name: String, reason: Option<String>, origin: &Origin) {
        self.failed_includes.push((name.clone(), reason.clone()));
        let make = move |name| match &reason {
            None => TypeErrorKind::CannotFindInclude { name },
            Some(reason) => TypeErrorKind::IrisLookupFailed {
                lookup: IrisLookup {
                    kind: DocKind::Inc,
                    name,
                },
                reason: reason.clone(),
            },
        };
        self.note_unresolved(make, name, origin);
    }

    fn note_unresolved(
        &mut self,
        make: impl Fn(String) -> TypeErrorKind + 'static,
        name: String,
        origin: &Origin,
    ) {
        self.unresolved.push((Box::new(make), name, origin.clone()));
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
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        self.map
            .push((start..self.text.len(), origin.clone(), precise));
    }

    // Appends an expansion made apart (see `expand_arg`), its pieces mapped as they were.
    fn push_expanded(&mut self, expanded: DoneList) {
        let start = self.text.len();
        self.text.push_str(&expanded.text);
        self.map
            .extend(expanded.map.into_iter().map(|(range, origin, precise)| {
                (start + range.start..start + range.end, origin, precise)
            }));
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
                let piece =
                    origin.narrowed(precise, origin.range.start + done..origin.range.start + i);
                self.push(&text[done..i], &piece, precise);
                self.push(" ", origin, false);
                done = i;
            }
            at_line_start = ch == '\n';
        }
        let piece = origin.narrowed(precise, origin.range.start + done..origin.range.end);
        self.push(&text[done..], &piece, precise);
    }

    // Remembers `uri`'s own text so a precise origin into it can later be converted to a
    // line/character range in *its* coordinate space, not the top-level file's.
    fn note_included_file(&mut self, uri: String, text: &str) {
        self.included.entry(uri).or_insert_with(|| OLC::new(text));
    }

    fn into_source_map(self, src: &str) -> SourceMap {
        let mut invocations = self.invocations;
        invocations.sort_by_key(|span| span.start);
        let mut map = SourceMap {
            vec: self.map,
            source_olc: OLC::new(src),
            source_lines: src.matches('\n').count() + 1,
            expanded_olc: OLC::new(&self.text),
            expanded_text: self.text,
            included: self.included,
            invocations,
            unresolved: Vec::new(),
            include_directives: self.include_directives,
            include_names: self.include_names,
        };
        // Reported against the document checked: one from an included file sits at the top-level
        // `#include` that pulled it in, the hops down as related information.
        map.unresolved = self
            .unresolved
            .into_iter()
            .filter_map(|(make, name, origin)| {
                let mut chain: Vec<_> =
                    std::iter::successors(Some(&origin), |o| o.parent.as_deref()).collect();
                let top = chain.pop()?;
                let mut diagnostic: Diagnostic =
                    make(name).at(map.range_in(&top.uri, &top.range)?).into();
                for hop in chain.into_iter().rev() {
                    diagnostic
                        .related_information
                        .push(bindings::DiagnosticRelatedInformation {
                            location: Location::new(
                                hop.uri.clone()?,
                                map.range_in(&hop.uri, &hop.range)?,
                            ),
                            message: "↳ In the included file.".into(),
                        });
                }
                Some(diagnostic)
            })
            .collect();
        map
    }
}

type TodoItem<'a> = (Cow<'a, MacroedUnit>, Origin, bool);

// Queue of remaining units, backed by a reversed `Vec` so push/pop-front are both O(1).
struct TodoList<'a>(Vec<TodoItem<'a>>);

impl<'a> TodoList<'a> {
    fn new(mut items: Vec<TodoItem<'a>>) -> Self {
        items.reverse();
        Self(items)
    }

    fn pop(&mut self) -> Option<TodoItem<'a>> {
        self.0.pop()
    }

    fn push_items(&mut self, items: Vec<TodoItem<'a>>) {
        self.0.extend(items.into_iter().rev());
    }
}

fn precise_items<'a>(
    units: impl IntoIterator<Item = &'a Ranged<MacroedUnit>>,
    origin_at: impl Fn(std::ops::Range<usize>) -> Origin,
) -> Vec<TodoItem<'a>> {
    units
        .into_iter()
        .map(|x| (Cow::Borrowed(x.deref()), origin_at(x.start..x.end), true))
        .collect()
}

// What closes a decided-false `#if`/`#elseif` branch, as found by `scan_branch`.
enum BranchEnd<'a> {
    EndIf,
    // Its body is left in `todos` to flow normally.
    Else,
    // The directive itself, for `resolve_if`; boxed, being far larger than the other ends.
    Elseif(Box<TodoItem<'a>>),
}

// Scans a branch's body, feeding each unit to `keep`, until finding what closes it. A `level`
// counter tells a nested `#if`'s own terminator apart from the one closing this branch.
fn scan_branch<'a>(todos: &mut TodoList<'a>, mut keep: impl FnMut(TodoItem<'a>)) -> BranchEnd<'a> {
    let mut level = 0usize;
    while let Some(item) = todos.pop() {
        let MacroedUnit::Redex(MacroRedex::H(HMacro { command, .. })) = item.0.as_ref() else {
            keep(item);
            continue;
        };
        match command.deref() {
            HMacroCommand::EndIf(..) if level == 0 => return BranchEnd::EndIf,
            HMacroCommand::Else(..) if level == 0 => return BranchEnd::Else,
            HMacroCommand::Elseif(..) if level == 0 => return BranchEnd::Elseif(Box::new(item)),
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

fn define(
    definitions: &mut Definitions<'_>,
    name: &cos::MacroName,
    params: Option<Vec<String>>,
    literal_args: bool,
    input: &cos::MacroInput,
) {
    let body = import_macro_input(input);
    definitions.insert(
        name.print(),
        MacroDefinition {
            params,
            literal_args,
            body,
        },
    );
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
// usable as the macro's expansion text. Each continued line is kept as written, since IRIS emits
// it as a line of its own: one starting with a label stays a label. A `##;` comment and the
// whitespace a line ends with are the preprocessor's, not the macro's: `#define KB (2**7) ##;128`
// expands to `(2**7)`, and a value ending in a tab doesn't put the tab into the code.
fn import_macro_input(cos::MacroInput { line_i, line_z }: &cos::MacroInput) -> String {
    let mut result = String::new();
    for line in line_i {
        let printed = line.print();
        let end = printed
            .to_ascii_lowercase()
            .find("##continue")
            .unwrap_or(printed.len());
        result.push_str(without_comment(&printed[..end]));
        result.push('\n');
    }
    result.push_str(without_comment(&line_z.print()));
    result.trim_start().to_string()
}

fn without_comment(line: &str) -> &str {
    line[..line.find("##;").unwrap_or(line.len())].trim_end()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "ROUTINE X\n#define SETIT(%a) Set %a = $$Bar()\n#define CALLFOO Do Foo(1)\n $$$SETIT(yy)\n $$$CALLFOO\nFoo(a)\n Quit\nBar()\n Quit 1\n";

    fn map() -> SourceMap {
        let workspace = crate::build_workspace_struct(None, None);
        workspace.expand_rtn("X.mac", SRC).unwrap().1
    }

    // Where `text` is in the expanded text and in the source.
    fn find(map: &SourceMap, text: &str) -> (usize, usize) {
        let expanded = map.expanded_text().find(text).unwrap();
        (expanded, SRC.find(text).unwrap())
    }

    fn invocation(text: &str) -> std::ops::Range<usize> {
        let start = SRC.find(text).unwrap();
        start..start + text.len()
    }

    #[test]
    fn a_pasted_argument_is_exact_inside_and_at_boundary_at_its_edges() {
        let map = map();
        let (e, s) = find(&map, "yy");
        assert_eq!(map.map_point(e + 1), Some((s + 1, Fidelity::Exact)));
        assert_eq!(map.map_point(e), Some((s, Fidelity::AtBoundary)));
        assert_eq!(map.map_point(e + 2), Some((s + 2, Fidelity::AtBoundary)));
        assert_eq!(
            map.map_range(e..e + 2),
            Some((s..s + 2, Fidelity::AtBoundary))
        );
    }

    #[test]
    fn a_macro_body_is_inexact_and_stands_for_its_invocation() {
        let map = map();
        let set = map.expanded_text().find("Set yy").unwrap();
        let setit = invocation("$$$SETIT(yy)");
        assert_eq!(
            map.map_point(set + 1),
            Some((setit.start, Fidelity::Inexact))
        );
        assert_eq!(
            map.map_range(set..set + 3),
            Some((setit.clone(), Fidelity::Inexact))
        );
        // From the source text before the invocation into its expansion: the covering range.
        let covering = setit.start - 1..setit.end;
        assert_eq!(
            map.map_range(set - 1..set + 3),
            Some((covering, Fidelity::Inexact))
        );
        let foo = map.expanded_text().find("Foo(1)").unwrap();
        let callfoo = invocation("$$$CALLFOO");
        assert_eq!(
            map.map_range(foo + 1..foo + 1),
            Some((callfoo, Fidelity::Inexact))
        );
    }

    #[test]
    fn source_text_is_exact_up_to_the_document_edges() {
        let map = map();
        assert_eq!(map.map_point(0), Some((0, Fidelity::Exact)));
        let (e, s) = find(&map, "Quit 1");
        assert_eq!(map.map_range(e..e + 6), Some((s..s + 6, Fidelity::Exact)));
        let end = map.expanded_text().len();
        assert_eq!(map.map_point(end), Some((SRC.len(), Fidelity::Exact)));
        // A dropped `#define` line is a macro's edge, not the document's.
        let line = map.expanded_text().find("\n Set yy").unwrap();
        let invocation = invocation("\n $$$SETIT(yy)");
        assert_eq!(
            map.map_point(line),
            Some((invocation.start, Fidelity::AtBoundary))
        );
    }
}
