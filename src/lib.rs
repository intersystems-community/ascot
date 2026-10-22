//! A type checker and code intelligence engine for InterSystems ObjectScript, with a grammar-only
//! parser for embedded IRIS SQL (SQL itself isn't type-checked yet).
//!
//! Most consumers want [`Workspace`] (a [`bindings::GuestWorkspace`] impl): open document text
//! by uri, then ask for diagnostics, inlay hints, hover info, or a go-to-definition target.
//! It's incremental — reopening a uri with the same text is cheap, and only whatever actually
//! depends on a changed file gets rechecked. `#include`/`##class`/label references are resolved
//! against every other document already open in the same `Workspace`.
//!
//! ```
//! use ascot::Workspace;
//! use ascot::bindings::GuestWorkspace;
//!
//! let workspace = Workspace::new(None, None);
//! workspace.open(
//!     "Test.cls".to_string(),
//!     "Class Test.Foo {\nClassMethod Bar() {\n set x = 1\n}\n}\n".to_string(),
//! );
//! let diagnostics = workspace.diagnostics("Test.cls".to_string());
//! assert_eq!(diagnostics.len(), 1); // `x` is assigned but never read
//! ```
//!
//! For lower-level access — parsing a class/routine into an AST directly, without the
//! workspace/caching layer — see [`cos`] (grammar) and [`check`] (the type checker built on it).
//! [`lsp`] and [`mcp`] wrap the same [`Workspace`] as a stdio LSP server and an MCP server,
//! respectively; both are also available as `ascot lsp`/`ascot mcp` from the CLI binary.
//!
//! # Where things live
//!
//! - `tygr-objectscript/` (separate crate): the ObjectScript and SQL grammars; parsing only.
//! - `details.rs`: digests a parsed class/routine into the facts the rest of the crate needs.
//! - `macro_expand.rs`: `#define`/`#include` expansion for routines, plus the source map that
//!   points diagnostics back at the unexpanded text.
//! - `check/`: the type checker, producing diagnostics and inlay hints.
//! - this file: the `Workspace`. Document cache, dependency tracking, incremental rechecking,
//!   and the hover/definition/references/symbol queries built on the above.
//! - `bindings.rs`: the WIT-generated types shared by every front end, and the wasm export.
//! - `render.rs`: Markdown formatting for hover text.
//! - `lsp.rs`, `mcp.rs`, `main.rs`: front ends (editor protocol, AI-agent protocol, CLI).
//! - `atelier.rs`: optional live-IRIS lookups for classes not in the workspace (native only).
//! - `iris.rs`: the `IrisClient` trait behind those, and declaration files answering it offline.

#[cfg(not(target_arch = "wasm32"))]
mod atelier;
pub mod bindings;
pub mod check;
pub mod details;
#[cfg(not(target_arch = "wasm32"))]
mod iris;
#[cfg(not(target_arch = "wasm32"))]
pub mod lsp;
mod macro_expand;
#[cfg(not(target_arch = "wasm32"))]
pub mod mcp;
mod render;
pub mod style;
#[cfg(not(target_arch = "wasm32"))]
pub use atelier::AtelierConnection;
#[cfg(not(target_arch = "wasm32"))]
pub use iris::{DEFAULT_NAMESPACE as DEFAULT_IRIS_NAMESPACE, IrisClient, open as open_iris};
pub(crate) use macro_expand::MacroDefinitions;
pub use macro_expand::SourceMap;
use tygr_objectscript::cos::ClassDefinitionHeader;
pub use tygr_objectscript::{cos, sql};

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    ops::Deref,
    sync::Arc,
};

use tygr::{Grammar, OLC, Range};

use crate::{
    bindings::{
        Diagnostic, InlayHint, IrisConnection, Location, MemberInfo, NameInfo, Position, Style,
    },
    check::WorkspaceEnv,
    cos::Routine,
    details::{ClassDetails, MemberKindSpecificDetails, WithRange, with_range_name},
};

// The only thing that decides how a document is analyzed: `.cls` files are checked as classes;
// `.inc` files are never checked standalone, only pulled in via another file's `#include`; `.mac`
// and `.int` routines are otherwise identical for our purposes, so share one variant.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocKind {
    Cls,
    Inc,
    Rtn,
}

// A header name qualified by kind: `#include X` and `$$^X` are different lookups.
pub(crate) type Name = (DocKind, String);

impl DocKind {
    // Purely a function of `uri`'s extension -- known even for a uri that failed tier A (or
    // was never opened at all), unlike anything actually stored in `doc_cache`.
    fn from_uri(uri: &str) -> Option<Self> {
        match uri.rsplit('.').next() {
            Some("cls") => Some(DocKind::Cls),
            Some("inc") => Some(DocKind::Inc),
            Some("mac" | "int") => Some(DocKind::Rtn),
            _ => None,
        }
    }
}

// A routine's macro-expanded form — the expensive part of analyzing it. Unlike `ClassDetails`,
// this depends on more than just the routine's own text: expansion inlines every `#include`d
// `.inc` file's content, so a change to any of its dependencies (recorded via
// `SourceMap::include_names`) drops it too, not just its own edit.
pub(crate) struct RtnDetails {
    pub(crate) routine: Routine,
    pub(crate) map: SourceMap,
    pub(crate) definitions: MacroDefinitions,
}

// A document's own text plus its cheap, header-only parse (just the name -- that's all a
// name-based lookup, e.g. `query-cls`/`workspace-symbol`/`resolve_*_uri`, ever needs from it),
// plus whatever tier B/C payload `ensure_details` has computed on top of it so far. A
// `DocCache` only ever exists once this much has succeeded -- a uri whose header fails to
// parse is `Err`, right in `WorkspaceStruct::doc_cache`, with no `DocCache` at all (see `open`).
struct DocCache {
    name: WithRange<String>,
    text: String,
    details: Option<Result<DocDetails, Diagnostic>>,
}

// Tier B (the full, kind-specific parse) plus tier C (the check built on top of it, for the two
// kinds that have one) -- computed and cached by `ensure_details`. Which variant a given `uri`
// produces is purely a function of `DocKind::from_uri(uri)`, not of anything stored here.
#[derive(Clone)]
enum DocDetails {
    Cls(Arc<ClassDetails>, Option<CachedCheck>),
    Rtn(Arc<RtnDetails>, Option<CachedCheck>),
    Inc(Arc<cos::RoutineWithMacro>),
}

impl DocCache {
    // The document's name -- normalized (case/package-insensitive) for a class, as-is for a
    // routine/`.inc`. Takes `uri` because kind isn't stored on `DocCache` itself anymore --
    // see `DocKind::from_uri`.
    fn name(&self, uri: &str) -> String {
        let mut name = self.name.it.clone();
        if DocKind::from_uri(uri) == Some(DocKind::Cls) {
            check::types::normalize_class_name(&mut name);
        }
        name
    }
}

// The output of actually running the checker: diagnostics, inlay hints, and go-to-definition
// references all come from the same traversal, so they're computed and cached together.
#[derive(Clone)]
struct CachedCheck {
    diagnostics: Vec<Diagnostic>,
    hints: Vec<InlayHint>,
    references: Vec<(bindings::Range, bindings::Location)>,
    hover_types: Vec<(bindings::Range, String)>,
}

pub struct WorkspaceStruct {
    // `RefCell` wraps only the `Ok` side -- per-entry, not around the whole map, so mutating
    // one uri's cached tiers (e.g. resolving a `#include` mid-expansion) can't conflict with
    // any other uri's; an `Err` entry is inert, only ever changing by `open` replacing it.
    pub(crate) doc_cache: HashMap<String, Result<RefCell<DocCache>, Diagnostic>>,
    // Name -> the names it looked up when last checked (found or not), and the reverse. Keyed by
    // name, not uri, so a lookup that found nothing is still an edge: whichever document later
    // claims the name stales its dependents like any other change of owner (`invalidate_name`).
    depends_on: RefCell<HashMap<Name, HashSet<Name>>>,
    dependents: RefCell<HashMap<Name, HashSet<Name>>>,
    // Name -> uri, populated as a byproduct of tier A — lets `resolve_cls_uri` be a plain O(1)
    // lookup once a class has been opened. A rename or removal is fixed up against `doc_cache`'s
    // own (old) entry directly, rather than keeping a uri -> name reverse index just for that.
    class_index: RefCell<HashMap<String, String>>,
    // Same idea as `class_index`, but for `.inc` header names — see `resolve_inc_uri`.
    inc_index: RefCell<HashMap<String, String>>,
    // Same idea as `class_index`, but for `ROUTINE` header names — see `resolve_rtn_uri`.
    rtn_index: RefCell<HashMap<String, String>>,
    pub(crate) iris: Option<IrisHandle>,
    // Labels of routines fetched from IRIS (not open in the workspace), by routine name, with
    // the text they were parsed from: reused only while the connection hands back that same
    // text, so this follows the connection's own cache policy rather than adding one. Without
    // it, every `$$Label^Routine` call into a 15k-line system routine re-expanded and re-parsed
    // it, and a single file took half a minute.
    pub(crate) iris_rtn_labels: RefCell<HashMap<String, (String, Option<check::Labels>)>>,
    pub(crate) style: Option<style::KeywordStyles>,
}

// `bindings::IrisConnection` is a WIT *imported* resource, producible only by a wasm host --
// every method on it is `unreachable!()` on a native build. Native builds get a real connection
// through `iris::IrisClient` instead (`Workspace::new_with_iris`); both share the same method
// names/signatures, so no call site under `check::env`/`macro_expand` needs to know which.
#[cfg(target_arch = "wasm32")]
type IrisHandle = IrisConnection;
#[cfg(not(target_arch = "wasm32"))]
type IrisHandle = Box<dyn iris::IrisClient>;

impl WorkspaceStruct {
    // Doubles as both open and edit; kind is inferred from `uri`'s extension. Returns the uris
    // (including this one) whose cached analysis is now stale and should be rechecked.
    fn open(&mut self, uri: String, text: String) -> Vec<String> {
        let Some(kind) = DocKind::from_uri(&uri) else {
            return vec![];
        };
        let entry = match kind {
            DocKind::Cls => match ClassDefinitionHeader::parse_prefix(&text) {
                Ok((header, _)) => {
                    let name = with_range_name(&header.name, &OLC::new(&text));
                    Ok(DocCache {
                        name,
                        text,
                        details: None,
                    })
                }
                Err(err) => Err(tygr_error_to_diagnostics(err, &text)),
            },
            // `.mac`/`.int`/`.inc` all share the same `ROUTINE Name [Type=...]` header grammar --
            // which of the two this is only matters once something asks for tier B (see
            // `ensure_details`), not here.
            DocKind::Inc | DocKind::Rtn => match cos::RoutineHeader::parse_prefix(&text) {
                Ok((header, _)) => {
                    let name = with_range_name(&header.name, &OLC::new(&text));
                    Ok(DocCache {
                        name,
                        text,
                        details: None,
                    })
                }
                Err(err) => Err(tygr_error_to_diagnostics(err, &text)),
            },
        };
        let new_name = entry.as_ref().ok().map(|doc| doc.name(&uri));
        // `insert` hands back whatever was there before, so `index_name` below can fix up a
        // stale forward-index entry on a rename without a separate lookup.
        let old_name = self
            .doc_cache
            .insert(uri.clone(), entry.map(RefCell::new))
            .and_then(Result::ok)
            .map(|cell| cell.into_inner().name(&uri));
        let mut affected = match (new_name, old_name) {
            (Some(new_name), old_name) => self.index_name(kind, &uri, old_name, new_name),
            // No longer parses: whatever it declared before, it doesn't now.
            (None, Some(old_name)) => self.unindex(kind, &uri, &old_name),
            (None, None) => vec![],
        };
        affected.extend(self.invalidate(&uri));
        affected.sort();
        affected.dedup();
        affected
    }

    fn remove_uri(&mut self, uri: &str) -> Vec<String> {
        let mut affected = self.invalidate(uri);
        if let Some(Ok(cell)) = self.doc_cache.remove(uri) {
            let name = cell.into_inner().name(uri);
            let kind = DocKind::from_uri(uri)
                .expect("a uri in doc_cache always has a recognized extension");
            affected.extend(self.unindex(kind, uri, &name));
            let key = (kind, name);
            if let Some(deps) = self.depends_on.borrow_mut().remove(&key) {
                let mut dependents = self.dependents.borrow_mut();
                for dep in deps {
                    if let Some(set) = dependents.get_mut(&dep) {
                        set.remove(&key);
                    }
                }
            }
        }
        affected
    }

    fn name_of(&self, uri: &str) -> Option<Name> {
        let kind = DocKind::from_uri(uri)?;
        let cell = self.doc_cache.get(uri)?.as_ref().ok()?;
        Some((kind, cell.borrow().name(uri)))
    }

    fn invalidate(&self, uri: &str) -> Vec<String> {
        let mut affected = vec![uri.to_string()];
        if let Some(name) = self.name_of(uri) {
            affected.extend(self.invalidate_name(&name));
        }
        affected
    }

    // Every document declaring `name` or (transitively) depending on it: caches dropped, uris returned.
    fn invalidate_name(&self, name: &Name) -> Vec<String> {
        let mut seen: HashSet<Name> = HashSet::from([name.clone()]);
        let mut queue = vec![name.clone()];
        while let Some(next) = queue.pop() {
            let Some(deps) = self.dependents.borrow().get(&next).cloned() else {
                continue;
            };
            for dep in deps {
                if seen.insert(dep.clone()) {
                    queue.push(dep);
                }
            }
        }
        let mut affected = vec![];
        for (uri, entry) in &self.doc_cache {
            let Ok(cell) = entry else {
                continue;
            };
            let kind = DocKind::from_uri(uri)
                .expect("a uri in doc_cache always has a recognized extension");
            if !seen.contains(&(kind, cell.borrow().name(uri))) {
                continue;
            }
            // A routine's expansion inlines its `#include`s, so every dependent's own expansion
            // needs dropping too, not just its check -- unlike a class's own parse (unaffected
            // by anything but its own edit) or a `.inc`'s tier B (never stale as a *dependent*
            // at all, since substitution happens in whoever includes it, not here).
            if kind == DocKind::Rtn {
                cell.borrow_mut().details = None;
            } else if let Some(Ok(DocDetails::Cls(_, check))) = &mut cell.borrow_mut().details {
                *check = None;
            }
            affected.push(uri.clone());
        }
        affected
    }

    fn record_deps(&self, uri: &str, mut deps: HashSet<Name>) {
        let Some(name) = self.name_of(uri) else {
            return;
        };
        deps.remove(&name);
        let mut depends_on = self.depends_on.borrow_mut();
        let mut dependents = self.dependents.borrow_mut();
        if let Some(old) = depends_on.get(&name) {
            for old_dep in old {
                if !deps.contains(old_dep)
                    && let Some(set) = dependents.get_mut(old_dep)
                {
                    set.remove(&name);
                }
            }
        }
        for dep in &deps {
            dependents
                .entry(dep.clone())
                .or_default()
                .insert(name.clone());
        }
        depends_on.insert(name, deps);
    }

    // The include file `name` (a `ROUTINE name [Type=INC]` header) open in the workspace, at
    // its header name. An include fetched from IRIS has no document to point at.
    pub(crate) fn inc_definition(&self, name: &str) -> Option<Location> {
        let uri = self.inc_index.borrow().get(name).cloned()?;
        let range = self.doc_cache.get(&uri)?.as_ref().ok()?.borrow().name.range;
        Some(Location::new(uri, range))
    }

    fn doc_text(&self, uri: &str) -> Option<String> {
        let cell = self.doc_cache.get(uri)?.as_ref().ok()?;
        Some(cell.borrow().text.clone())
    }

    pub(crate) fn cls_details(&self, uri: &str) -> Result<Arc<ClassDetails>, Diagnostic> {
        match self.ensure_details(uri)? {
            DocDetails::Cls(details, _) => Ok(details),
            _ => panic!("cls_details called on a uri that isn't an open class"),
        }
    }

    fn index_for(&self, kind: DocKind) -> &RefCell<HashMap<String, String>> {
        match kind {
            DocKind::Cls => &self.class_index,
            DocKind::Rtn => &self.rtn_index,
            DocKind::Inc => &self.inc_index,
        }
    }

    // Makes `uri` the owner of `name` (dropping its claim on `old_name`, if renamed). Returns
    // the other uris whose cached analysis this makes stale.
    fn index_name(
        &self,
        kind: DocKind,
        uri: &str,
        old_name: Option<String>,
        name: String,
    ) -> Vec<String> {
        let mut affected = vec![];
        if old_name.as_deref() != Some(name.as_str())
            && let Some(old_name) = old_name
        {
            affected.extend(self.unindex(kind, uri, &old_name));
        }
        // `open` then invalidates by name, reaching a previous owner (checked before this one
        // existed, so unaware of the conflict) and everything that looked `name` up and missed.
        self.index_for(kind)
            .borrow_mut()
            .insert(name, uri.to_string());
        affected
    }

    // Drops `uri`'s claim on `name`, handing it to another document of that kind still declaring
    // it, if any. Every such document's cached analysis (a duplicate-class error naming `uri`,
    // say) is stale, so they're all returned for rechecking.
    fn unindex(&self, kind: DocKind, uri: &str, name: &str) -> Vec<String> {
        let others: Vec<String> = self
            .doc_cache
            .iter()
            .filter_map(|(other, entry)| {
                let cell = entry.as_ref().ok()?;
                (other != uri
                    && DocKind::from_uri(other) == Some(kind)
                    && cell.borrow().name(other) == name)
                    .then(|| other.clone())
            })
            .collect();
        let index = self.index_for(kind);
        if index.borrow().get(name).is_some_and(|owner| owner == uri) {
            index.borrow_mut().remove(name);
            if let Some(successor) = others.iter().min() {
                index
                    .borrow_mut()
                    .insert(name.to_string(), successor.clone());
            }
        }
        self.invalidate_name(&(kind, name.to_string()))
    }

    // `class_index` only ever remembers one owner per name (whichever claimed it last), so it
    // can't answer "does any *other* uri also have this name" -- scan `doc_cache`'s own tier-A
    // names instead, same O(workspace) cost as the reverse index this replaced would have been.
    fn duplicate_class_name_diagnostic(
        &self,
        uri: &str,
        name: &str,
        range: bindings::Range,
    ) -> Option<Diagnostic> {
        let other_uri = self.doc_cache.iter().find_map(|(other_uri, entry)| {
            let cell = entry.as_ref().ok()?;
            (other_uri != uri
                && DocKind::from_uri(other_uri) == Some(DocKind::Cls)
                && cell.borrow().name(other_uri) == name)
                .then(|| other_uri.clone())
        })?;
        let other_range = self.cls_details(&other_uri).ok()?.name.range;
        let other = Location::new(other_uri, other_range);
        Some(check::TypeError::duplicate_class_name(range, name.to_string(), other).into())
    }

    fn workspace_env(&self) -> WorkspaceEnv<'_> {
        WorkspaceEnv::new(self)
    }

    // `class_index` is populated for every open class immediately (see `insert`), so this is
    // always a plain O(1) lookup -- `None` just means no open class has that name.
    pub(crate) fn resolve_cls_uri(&self, name: &str) -> Option<String> {
        self.class_index.borrow().get(name).cloned()
    }

    pub(crate) fn rtn_details(&self, uri: &str) -> Result<Arc<RtnDetails>, Diagnostic> {
        match self.ensure_details(uri)? {
            DocDetails::Rtn(details, _) => Ok(details),
            _ => panic!("rtn_details called on a uri that isn't an open routine"),
        }
    }

    // `.inc` files never reach the `expand_rtn` branch here -- they can't be macro-expanded on
    // their own (see `DocDetails::Inc`); `expand_rtn` resolves `#include`s by recursing back into
    // `doc_cache` (via `inc_parse`), but always for some *other* (`.inc`) uri's own cell -- never
    // this one -- so holding this cell borrowed for the whole call is safe.
    fn ensure_details(&self, uri: &str) -> Result<DocDetails, Diagnostic> {
        let cell = self
            .doc_cache
            .get(uri)
            .expect("ensure_details called on a uri that isn't in the workspace")
            .as_ref()
            .map_err(Clone::clone)?;
        let mut entry = cell.borrow_mut();
        let doc = &mut *entry;
        let text = doc.text.as_str();
        let fresh = doc.details.is_none();
        let details = doc
            .details
            .get_or_insert_with(|| match DocKind::from_uri(uri) {
                Some(DocKind::Cls) => ClassDetails::try_from(text)
                    .map(|details| DocDetails::Cls(Arc::new(details), None))
                    .map_err(|err| tygr_error_to_diagnostics(err, text)),
                Some(DocKind::Rtn) => {
                    self.expand_rtn(uri, text)
                        .map(|(routine, map, definitions)| {
                            DocDetails::Rtn(
                                Arc::new(RtnDetails {
                                    routine,
                                    map,
                                    definitions,
                                }),
                                None,
                            )
                        })
                }
                Some(DocKind::Inc) => cos::RoutineWithMacro::parse(text)
                    .map(|parsed| DocDetails::Inc(Arc::new(parsed)))
                    .map_err(|err| tygr_error_to_diagnostics(err, text)),
                None => unreachable!("a uri in doc_cache always has a recognized extension"),
            })
            .clone();
        // `rtn_details` (symbols, another routine's label lookup) may get here before
        // `ensure_checked` records the full dependency set; the `#include`s just resolved must
        // invalidate this expansion either way.
        drop(entry); // `record_deps` reads this cell
        if fresh && let Ok(DocDetails::Rtn(rtn, _)) = &details {
            self.record_deps(uri, include_names(&rtn.map));
        }
        details
    }

    // `rtn_index` is populated for every open routine immediately (see `insert`), so this is
    // always a plain O(1) lookup, same shape as `resolve_cls_uri`.
    pub(crate) fn resolve_rtn_uri(&self, name: &str) -> Option<String> {
        self.rtn_index.borrow().get(name).cloned()
    }

    // The routine declared in `uri` and its labels, as a `textDocument/documentSymbol` outline.
    fn routine_symbol(&self, uri: &str) -> Option<bindings::DocumentSymbol> {
        let rtn = self.rtn_details(uri).ok()?;
        let text = self.doc_text(uri)?;
        let olc = &rtn.map.expanded_olc;
        let (end_line, end_character) = OLC::new(&text).offset_to_line_character(text.len());
        let full_range = bindings::Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: end_line as u32,
                character: end_character as u32,
            },
        };
        let header_range = bindings::Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: text.lines().next().unwrap_or_default().len() as u32,
            },
        };
        let members = rtn
            .routine
            .subroutines
            .iter()
            .flat_map(|subs| subs.items.iter())
            .filter_map(|elem| details::routine_element_kind(olc, elem))
            .filter_map(|details| {
                let range = rtn.map.relocate_source_range(details.name.range)?;
                Some(bindings::SymbolInfo {
                    name: details.name.it,
                    kind: bindings::SymbolKind::Function,
                    deprecated: false,
                    range,
                    selection_range: range,
                })
            })
            .collect();
        Some(bindings::DocumentSymbol {
            root: bindings::SymbolInfo {
                name: rtn.routine.header.name.print(),
                kind: bindings::SymbolKind::Module,
                deprecated: false,
                range: full_range,
                selection_range: header_range,
            },
            members,
        })
    }

    // A failed tier-B parse (see `ensure_details`) has no check slot to write into -- its one
    // diagnostic is cheap to recompute from `cls_details`/`rtn_details`'s own cached `Err` on
    // every call, so there's no need to also cache it as a `CachedCheck` here.
    fn store_check(&self, uri: &str, check: CachedCheck) {
        let Some(Ok(cell)) = self.doc_cache.get(uri) else {
            return;
        };
        match &mut cell.borrow_mut().details {
            Some(Ok(DocDetails::Cls(_, check_slot))) => *check_slot = Some(check),
            Some(Ok(DocDetails::Rtn(_, check_slot))) => *check_slot = Some(check),
            _ => {}
        }
    }

    // Same shape as `ensure_details`: check `uri`'s cached `CachedCheck` (embedded in whatever
    // `ensure_details` already returned -- no separate lookup needed) before computing a fresh
    // one from its tier-B payload.
    fn ensure_checked(&self, uri: &str) -> CachedCheck {
        let empty = CachedCheck {
            diagnostics: vec![],
            hints: vec![],
            references: vec![],
            hover_types: vec![],
        };
        if !self.doc_cache.contains_key(uri) {
            return empty;
        }
        // A `.inc` file can't be macro-expanded (or checked) on its own -- see `DocDetails::Inc`.
        if DocKind::from_uri(uri) == Some(DocKind::Inc) {
            return empty;
        }
        let details = match self.ensure_details(uri) {
            Ok(details) => details,
            Err(diagnostic) => {
                return CachedCheck {
                    diagnostics: vec![diagnostic],
                    ..empty
                };
            }
        };
        let result = match details {
            DocDetails::Cls(_, Some(check)) | DocDetails::Rtn(_, Some(check)) => return check,
            DocDetails::Cls(cls, None) => {
                let env = self.workspace_env();
                let text = self.doc_text(uri).unwrap_or_default();
                let olc = OLC::new(&text);
                let doc_env = check::DocumentEnv::new(&env, &olc, uri);
                let mut diagnostics = check::check_cls(&doc_env, &cls);
                let mut name = cls.name.to_string();
                check::types::normalize_class_name(&mut name);
                if let Some(diagnostic) =
                    self.duplicate_class_name_diagnostic(uri, &name, cls.name.range)
                {
                    diagnostics.push(diagnostic);
                }
                diagnostics.extend(env.soft_diagnostics());
                let hints = doc_env.inlay_hints.into_inner();
                let hover_types = doc_env.hover_types.into_inner();
                let references = env.references();
                self.record_deps(uri, env.touched_names());
                CachedCheck {
                    diagnostics,
                    hints,
                    references,
                    hover_types,
                }
            }
            DocDetails::Rtn(rtn, None) => {
                let env = self.workspace_env();
                let doc_env = check::DocumentEnv::new(&env, &rtn.map.expanded_olc, uri);
                let mut diagnostics = check::check_rtn(&doc_env, &rtn.routine, &rtn.definitions);
                diagnostics.extend(env.soft_diagnostics());
                let mut diagnostics = rtn.map.relocate_diagnostics(diagnostics, uri);
                diagnostics.extend(rtn.map.unresolved.iter().cloned());
                let hints = doc_env
                    .inlay_hints
                    .into_inner()
                    .into_iter()
                    .filter_map(|hint| {
                        Some(InlayHint {
                            position: rtn.map.relocate_position(hint.position)?,
                            ..hint
                        })
                    })
                    .collect();
                let hover_types = doc_env
                    .hover_types
                    .into_inner()
                    .into_iter()
                    .filter_map(|(range, ty)| Some((rtn.map.relocate_source_range(range)?, ty)))
                    .collect();
                let mut references: Vec<_> = env
                    .references()
                    .into_iter()
                    .filter_map(|(range, mut location)| {
                        let range = rtn.map.relocate_source_range(range)?;
                        // A same-file label/procedure's `Location` is also in
                        // expanded-macro coordinates and needs relocating too.
                        if location.uri == uri {
                            location.range = rtn.map.relocate_source_range(location.range)?;
                        }
                        Some((range, location))
                    })
                    .collect();
                // The routine's own `#include` directives, for go-to-definition on them.
                references.extend(
                    rtn.map
                        .include_directives()
                        .filter_map(|(range, name)| Some((range, self.inc_definition(name)?))),
                );
                let mut deps = env.touched_names();
                deps.extend(include_names(&rtn.map));
                self.record_deps(uri, deps);
                CachedCheck {
                    diagnostics,
                    hints,
                    references,
                    hover_types,
                }
            }
            DocDetails::Inc(_) => unreachable!("DocKind::Inc already returned above"),
        };
        self.store_check(uri, result.clone());
        result
    }
}

// Internal mutability (RefCell) is needed because &mut can't pass wasm boundary for now.
pub struct Workspace(RefCell<WorkspaceStruct>);

impl std::ops::Deref for Workspace {
    type Target = RefCell<WorkspaceStruct>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

fn build_workspace_struct(iris: Option<IrisHandle>, style: Option<Style>) -> WorkspaceStruct {
    WorkspaceStruct {
        doc_cache: HashMap::new(),
        depends_on: RefCell::new(HashMap::new()),
        dependents: RefCell::new(HashMap::new()),
        class_index: RefCell::new(HashMap::new()),
        inc_index: RefCell::new(HashMap::new()),
        rtn_index: RefCell::new(HashMap::new()),
        iris,
        iris_rtn_labels: RefCell::new(HashMap::new()),
        style: style.as_ref().map(style::KeywordStyles::new),
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Workspace {
    /// The native equivalent of [`GuestWorkspace::new`](bindings::GuestWorkspace::new) (see
    /// `IrisHandle`) -- how a native consumer (CLI, LSP, MCP, or a direct library user) backs a
    /// `Workspace` with a real IRIS connection.
    pub fn new_with_iris(iris: Option<Box<dyn iris::IrisClient>>, style: Option<Style>) -> Self {
        Workspace(RefCell::new(build_workspace_struct(iris, style)))
    }
}

impl bindings::GuestWorkspace for Workspace {
    #[allow(async_fn_in_trait)]
    fn new(iris: Option<IrisConnection>, style: Option<Style>) -> Self {
        #[cfg(target_arch = "wasm32")]
        let iris: Option<IrisHandle> = iris;
        #[cfg(not(target_arch = "wasm32"))]
        let iris: Option<IrisHandle> = {
            // Never carries a real connection natively -- use `new_with_iris` for that.
            let _ = iris;
            None
        };
        Workspace(RefCell::new(build_workspace_struct(iris, style)))
    }

    #[allow(async_fn_in_trait)]
    fn open(&self, uri: String, text: String) -> Vec<String> {
        self.borrow_mut().open(uri, text)
    }

    #[allow(async_fn_in_trait)]
    fn close(&self, uri: String) -> Vec<String> {
        self.borrow_mut().remove_uri(&uri)
    }

    #[allow(async_fn_in_trait)]
    fn diagnostics(&self, uri: String) -> Vec<bindings::Diagnostic> {
        self.borrow().ensure_checked(&uri).diagnostics
    }

    #[allow(async_fn_in_trait)]
    fn inlay_hint(&self, uri: String, _range: bindings::Range) -> Vec<InlayHint> {
        self.borrow().ensure_checked(&uri).hints
    }

    // Where the reference at `position` is declared, if any — one of the go-to-definition
    // edges discovered as a byproduct of the last check. Covers class references
    // (`##class(Name)`, `x.Foo`, `x.Foo()`), instance members (`i%Foo`), self-references
    // (`..Method`), and same-routine label calls/gotos. Doesn't cover anything the checker
    // resolves to no location (e.g. `%New`, unmapped IRIS system classes), computed/indirect
    // `Goto`, or anything in SQL (which isn't type-checked yet).
    #[allow(async_fn_in_trait)]
    fn definition(&self, uri: String, position: bindings::Position) -> Option<bindings::Location> {
        self.borrow()
            .ensure_checked(&uri)
            .references
            .into_iter()
            .find(|(range, _)| position_in_range(position, *range))
            .map(|(_, location)| location)
    }

    // Every location across the workspace that refers to whatever's at `position` — the reverse
    // of `definition`. `position` may be on a reference itself (same lookup `definition` does)
    // or on the declaration site (found by checking every recorded reference's target instead of
    // its source), so this works whether invoked from a call site or from the declaration.
    #[allow(async_fn_in_trait)]
    fn references(
        &self,
        uri: String,
        position: bindings::Position,
        include_declaration: bool,
    ) -> Vec<bindings::Location> {
        let this = self.borrow();
        let uris: Vec<String> = this.doc_cache.keys().cloned().collect();
        let all: Vec<(String, bindings::Range, bindings::Location)> = uris
            .iter()
            .flat_map(|doc_uri| {
                this.ensure_checked(doc_uri)
                    .references
                    .into_iter()
                    .map(|(range, location)| (doc_uri.clone(), range, location))
                    .collect::<Vec<_>>()
            })
            .collect();
        let Some(target) = all
            .iter()
            .find(|(src_uri, range, _)| *src_uri == uri && position_in_range(position, *range))
            .map(|(_, _, location)| location.clone())
            .or_else(|| {
                all.iter()
                    .find(|(_, _, location)| {
                        location.uri == uri && position_in_range(position, location.range)
                    })
                    .map(|(_, _, location)| location.clone())
            })
        else {
            return vec![];
        };
        let mut result: Vec<bindings::Location> = all
            .into_iter()
            .filter(|(_, _, location)| *location == target)
            .map(|(src_uri, range, _)| bindings::Location::new(src_uri, range))
            .collect();
        if include_declaration {
            result.push(target);
        }
        result
    }

    // The class member declared at `position`, if any, rendered as markdown. Otherwise, the
    // inferred type of the local variable read there (a bare read like `Write x`/`Set y = x`,
    // not a subscripted one — see `hover_type`) and/or what the macro invocation there
    // expanded to — both shown together when both apply (e.g. a variable passed as a macro
    // argument), since they answer different questions.
    #[allow(async_fn_in_trait)]
    fn hover(&self, uri: String, position: bindings::Position) -> Option<String> {
        if let Some(member) = self.hover_member(uri.clone(), position) {
            return Some(render::format_member_hover(&member));
        }
        let ty = self
            .hover_type(uri.clone(), position)
            .map(|ty| format!("```objectscript\n{ty}\n```"));
        let expansion = self
            .macro_expansion(uri, position)
            .map(|expansion| format!("Expands to:\n```objectscript\n{expansion}\n```"));
        [ty, expansion]
            .into_iter()
            .flatten()
            .reduce(|a, b| format!("{a}\n\n---\n\n{b}"))
    }

    // Classes in the workspace whose name starts with `query`, e.g. for a completion dropdown
    // or workspace-symbol search.
    #[allow(async_fn_in_trait)]
    fn query_cls(&self, query: String) -> Vec<(String, bindings::ClassInfo)> {
        let this = self.borrow();
        // Tier A alone (name only) decides which classes match -- only matches pay for a full
        // (tier B) parse to build their `ClassInfo` below.
        let candidates: Vec<String> = this
            .doc_cache
            .iter()
            .filter_map(|(uri, entry)| {
                let doc = entry.as_ref().ok()?.borrow();
                (DocKind::from_uri(uri) == Some(DocKind::Cls) && doc.name.it.starts_with(&query))
                    .then(|| uri.clone())
            })
            .collect();
        candidates
            .into_iter()
            .filter_map(|uri| {
                let details = this.cls_details(&uri).ok()?;
                let mut info = bindings::ClassInfo::from(&*details);
                for member in &mut info.members {
                    member.uri = Some(uri.clone());
                }
                Some((uri, info))
            })
            .collect()
    }

    // Members of `cls` whose name starts with `query`.
    #[allow(async_fn_in_trait)]
    fn query_mem(&self, cls: String, query: String) -> Vec<(String, MemberInfo)> {
        self.query_cls(cls.clone())
            .into_iter()
            .filter(|(_, info)| info.name.content == cls)
            .flat_map(|(uri, info)| {
                info.members
                    .into_iter()
                    .filter(|member| member.name.content.starts_with(&query))
                    .map(move |member| (uri.clone(), member))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    // The class declared in `uri` and its members, or (in a `.mac`/`.int` routine) its labels,
    // as a `textDocument/documentSymbol` outline.
    #[allow(async_fn_in_trait)]
    fn document_symbol(&self, uri: String) -> Option<bindings::DocumentSymbol> {
        let this = self.borrow();
        this.doc_cache.get(&uri)?;
        match DocKind::from_uri(&uri)? {
            DocKind::Cls => {
                let details = this.cls_details(&uri).ok()?;
                let text = this.doc_text(&uri)?;
                let (end_line, end_character) =
                    OLC::new(&text).offset_to_line_character(text.len());
                let full_range = bindings::Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: end_line as u32,
                        character: end_character as u32,
                    },
                };
                Some(bindings::DocumentSymbol {
                    root: bindings::SymbolInfo {
                        name: details.name.to_string(),
                        kind: bindings::SymbolKind::Class,
                        deprecated: details.deprecated,
                        range: full_range,
                        selection_range: details.name.range,
                    },
                    members: details
                        .members
                        .iter()
                        .map(|member| bindings::SymbolInfo {
                            name: member.it.name.to_string(),
                            kind: bindings::member_kind_to_symbol_kind(
                                &bindings::MemberKind::from(&member.it.kind),
                            ),
                            deprecated: member.it.deprecated,
                            range: member.range,
                            selection_range: member.it.name.range,
                        })
                        .collect(),
                })
            }
            DocKind::Rtn => this.routine_symbol(&uri),
            DocKind::Inc => None,
        }
    }

    // Classes and members, and routines and labels, across the workspace whose name starts with
    // `query`, as a `workspace/symbol` result. Tier A alone decides which classes/routines match
    // (see `query_cls` and the routine filter below); only matches pay for the full (tier B)
    // parse needed to list their members/labels.
    #[allow(async_fn_in_trait)]
    fn workspace_symbol(&self, query: String) -> Vec<bindings::WorkspaceSymbol> {
        fn name_range(name: &NameInfo) -> bindings::Range {
            bindings::Range {
                start: name.before,
                end: name.after,
            }
        }
        let cls_symbols =
            self.query_cls(query.clone())
                .into_iter()
                .flat_map(|(uri, info)| {
                    let cls_symbol = bindings::WorkspaceSymbol {
                        name: info.name.content.clone(),
                        kind: bindings::SymbolKind::Class,
                        deprecated: info.deprecated,
                        location: bindings::Location::new(uri.clone(), name_range(&info.name)),
                    };
                    std::iter::once(cls_symbol)
                        .chain(info.members.into_iter().map(move |member| {
                            bindings::WorkspaceSymbol {
                                name: member.name.content.clone(),
                                kind: bindings::member_kind_to_symbol_kind(&member.kind),
                                deprecated: member.deprecated,
                                location: bindings::Location::new(
                                    uri.clone(),
                                    name_range(&member.name),
                                ),
                            }
                        }))
                        .collect::<Vec<_>>()
                });

        let this = self.borrow();
        let candidates: Vec<String> = this
            .doc_cache
            .iter()
            .filter_map(|(uri, entry)| {
                let doc = entry.as_ref().ok()?.borrow();
                (DocKind::from_uri(uri) == Some(DocKind::Rtn) && doc.name.it.starts_with(&query))
                    .then(|| uri.clone())
            })
            .collect();
        let rtn_symbols = candidates
            .into_iter()
            .flat_map(|uri| {
                let Some(symbol) = this.routine_symbol(&uri) else {
                    return vec![];
                };
                let rtn_symbol = bindings::WorkspaceSymbol {
                    name: symbol.root.name,
                    kind: symbol.root.kind,
                    deprecated: symbol.root.deprecated,
                    location: bindings::Location::new(uri.clone(), symbol.root.selection_range),
                };
                std::iter::once(rtn_symbol)
                    .chain(
                        symbol
                            .members
                            .into_iter()
                            .map(|member| bindings::WorkspaceSymbol {
                                name: member.name,
                                kind: member.kind,
                                deprecated: member.deprecated,
                                location: bindings::Location::new(
                                    uri.clone(),
                                    member.selection_range,
                                ),
                            }),
                    )
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        cls_symbols.chain(rtn_symbols).collect()
    }
}

impl Workspace {
    // Cheap (no full check needed) unless `position` is inside a method body, where it could
    // be on a call to a *different* member — then a full check's recorded references is the
    // only way to tell "hovering Foo's own body" apart from "hovering a call to Bar inside it".
    fn hover_member(&self, uri: String, position: bindings::Position) -> Option<MemberInfo> {
        let this = self.borrow();
        this.doc_cache.get(&uri)?;
        if DocKind::from_uri(&uri) != Some(DocKind::Cls) {
            return None;
        }
        let details = this.cls_details(&uri).ok()?;
        let member = details
            .members
            .iter()
            .find(|member| position_in_range(position, member.range))?;
        if let MemberKindSpecificDetails::MethodOrClassMethod(method) = &member.it.kind {
            let body_start = method.body.range.start;
            if (position.line, position.character) >= (body_start.line, body_start.character) {
                let location = this
                    .ensure_checked(&uri)
                    .references
                    .into_iter()
                    .find(|(range, _)| position_in_range(position, *range))
                    .map(|(_, location)| location)?;
                let target = this.cls_details(&location.uri).ok()?;
                return target
                    .members
                    .iter()
                    .find(|m| m.range == location.range)
                    .map(|m| MemberInfo {
                        uri: Some(location.uri.clone()),
                        ..MemberInfo::from(m)
                    });
            }
        }
        Some(MemberInfo {
            uri: Some(uri.clone()),
            ..MemberInfo::from(member)
        })
    }

    // The inferred type of the local variable read at `position`, if any — covers a bare read
    // of a local (`Write x`, `Set y = x`, ...), not a subscripted one (`x(1)`, whose type is
    // just "is anything assigned", not that element's actual type) or the target of the `Set`
    // that assigns it (see the `ParenedExpr::Normal` peek in `check::mod` for why).
    fn hover_type(&self, uri: String, position: bindings::Position) -> Option<String> {
        self.borrow()
            .ensure_checked(&uri)
            .hover_types
            .into_iter()
            .find(|(range, _)| position_in_range(position, *range))
            .map(|(_, ty)| ty)
    }

    // What the macro invocation at `position` expanded to, if any. `.mac`/`.int` only -- macros
    // in a `.cls` method body aren't expanded at all today, and a `.inc` file's own macros can't
    // be expanded independently of whoever `#include`s it (see `DocDetails::Inc`).
    fn macro_expansion(&self, uri: String, position: bindings::Position) -> Option<String> {
        let this = self.borrow();
        this.doc_cache.get(&uri)?;
        if DocKind::from_uri(&uri) != Some(DocKind::Rtn) {
            return None;
        }
        this.rtn_details(&uri)
            .ok()?
            .map
            .macro_expansion_at_position(position)
    }
}

fn include_names(map: &SourceMap) -> HashSet<Name> {
    map.include_names()
        .map(|name| (DocKind::Inc, name.clone()))
        .collect()
}

fn position_in_range(position: Position, range: bindings::Range) -> bool {
    fn le(a: Position, b: Position) -> bool {
        (a.line, a.character) <= (b.line, b.character)
    }
    le(range.start, position) && le(position, range.end)
}

pub(crate) fn tygr_error_to_diagnostics(e: cos::Error, src: &str) -> Diagnostic {
    let (line, character) = OLC::new(src).offset_to_line_character(e.pos());
    let line = line as u32;
    let character = character as u32;
    let start = bindings::Position { line, character };
    let end = bindings::Position {
        line,
        character: character + 1,
    };
    check::TypeError::document_syntax_error(bindings::Range { start, end }, e).into()
}

impl bindings::Guest for bindings::Analyzer {
    type Workspace = Workspace;
}

impl From<(usize, usize)> for Position {
    fn from(value: (usize, usize)) -> Self {
        let (line, character) = value;
        Position {
            line: line as u32,
            character: character as u32,
        }
    }
}

// `bindings::Position`/`Range` are generated by `wit_bindgen::generate!`, so
// `#[derive(Default)]` can't be added to them directly.
#[allow(clippy::derivable_impls)]
impl Default for bindings::Position {
    fn default() -> Self {
        Self {
            line: 0,
            character: 0,
        }
    }
}

#[allow(clippy::derivable_impls)]
impl Default for bindings::Range {
    fn default() -> Self {
        Self {
            start: Default::default(),
            end: Default::default(),
        }
    }
}

pub trait ToNameInfo {
    fn to_name_info(&self, olc: &OLC) -> NameInfo;
}

impl<T: Grammar> ToNameInfo for Range<T> {
    fn to_name_info(&self, olc: &OLC) -> NameInfo {
        NameInfo {
            before: Position::from(olc.offset_to_line_character(self.start)),
            content: self.deref().print(),
            after: Position::from(olc.offset_to_line_character(self.end)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::bindings::GuestWorkspace;

    use super::*;

    #[test]
    fn dependency_invalidation() {
        let workspace = Workspace::new(None, None);
        let base_uri = "Base.cls".to_string();
        let sub_uri = "Sub.cls".to_string();
        workspace.open(
            base_uri.clone(),
            "Class Test.Base {\nClassMethod Foo() {\n}\n}\n".to_string(),
        );
        workspace.open(
            sub_uri.clone(),
            "Class Test.Sub {\nClassMethod Bar() {\n do ##class(Test.Base).Foo()\n}\n}\n"
                .to_string(),
        );

        let diagnostics = workspace.diagnostics(sub_uri.clone());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        // Editing `Base` should invalidate `Sub` too, even though `Sub` wasn't touched.
        let affected: HashSet<String> = workspace
            .open(
                base_uri.clone(),
                "Class Test.Base {\nClassMethod Foo() {\n}\n}\n".to_string(),
            )
            .into_iter()
            .collect();
        assert!(affected.contains(&base_uri));
        assert!(affected.contains(&sub_uri));

        let affected: HashSet<String> = workspace.close(base_uri.clone()).into_iter().collect();
        assert!(affected.contains(&base_uri));
        assert!(affected.contains(&sub_uri));
    }

    #[test]
    fn include_dependency_invalidation() {
        let workspace = Workspace::new(None, None);
        let inc_uri = "Shared.inc".to_string();
        let mac_uri = "Main.mac".to_string();
        workspace.open(
            inc_uri.clone(),
            "ROUTINE Shared [Type=INC]\n#define GREETING \"hi\"\n".to_string(),
        );
        workspace.open(
            mac_uri.clone(),
            "ROUTINE Main\n\n#include Shared\n\tQuit\n".to_string(),
        );

        let diagnostics = workspace.diagnostics(mac_uri.clone());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        let affected: HashSet<String> = workspace
            .open(
                inc_uri.clone(),
                "ROUTINE Shared [Type=INC]\n#define GREETING \"hello\"\n".to_string(),
            )
            .into_iter()
            .collect();
        assert!(affected.contains(&inc_uri));
        assert!(affected.contains(&mac_uri));
    }

    #[test]
    fn opening_a_missing_include_later_invalidates_its_includers() {
        let workspace = Workspace::new(None, None);
        let mac_uri = "Main.mac".to_string();
        let cls_uri = "User.cls".to_string();
        let inc_uri = "Shared.inc".to_string();
        workspace.open(
            mac_uri.clone(),
            "ROUTINE Main\n\n#include Shared\n\tQuit\n".to_string(),
        );
        workspace.open(
            cls_uri.clone(),
            "Include Shared\nClass Test.User {\nClassMethod Foo() {\n}\n}\n".to_string(),
        );
        for uri in [&mac_uri, &cls_uri] {
            let diagnostics = workspace.diagnostics(uri.clone());
            assert!(
                diagnostics
                    .iter()
                    .any(|d| d.message.contains("Include file 'Shared' is not defined")),
                "{diagnostics:?}"
            );
        }

        let affected: HashSet<String> = workspace
            .open(
                inc_uri.clone(),
                "ROUTINE Shared [Type=INC]\n#define GREETING \"hi\"\n".to_string(),
            )
            .into_iter()
            .collect();
        assert!(affected.contains(&mac_uri), "{affected:?}");
        assert!(affected.contains(&cls_uri), "{affected:?}");
        for uri in [&mac_uri, &cls_uri] {
            let diagnostics = workspace.diagnostics(uri.clone());
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
        }
    }

    #[test]
    fn opening_a_missing_class_later_invalidates_its_users() {
        let workspace = Workspace::new(None, None);
        let base_uri = "Base.cls".to_string();
        let sub_uri = "Sub.cls".to_string();
        workspace.open(
            sub_uri.clone(),
            "Class Test.Sub {\nClassMethod Bar() {\n do ##class(Test.Base).Foo()\n}\n}\n"
                .to_string(),
        );
        let before = workspace.diagnostics(sub_uri.clone());
        assert!(!before.is_empty(), "{before:?}");

        let affected: HashSet<String> = workspace
            .open(
                base_uri.clone(),
                "Class Test.Base {\nClassMethod Foo() {\n}\n}\n".to_string(),
            )
            .into_iter()
            .collect();
        assert!(affected.contains(&sub_uri), "{affected:?}");
        let diagnostics = workspace.diagnostics(sub_uri.clone());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn missing_include_is_reported() {
        let workspace = Workspace::new(None, None);
        let uri = "Main.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Main\n\n\t#include NoSuchInclude\n\tQuit\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Include file 'NoSuchInclude' is not defined in the current workspace")
        );
    }

    // A macro meant to expand into a full command, immediately followed by `{` with no
    // `if`/`for` of its own (`$$$peekany("...") {}`), breaks the parse when left unexpanded --
    // should report the missing include, not the resulting generic syntax error.
    #[test]
    fn missing_include_is_reported_even_when_a_macro_call_opens_a_block() {
        let workspace = Workspace::new(None, None);
        let uri = "Main.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Main\n\n\t#include NoSuchInclude\n\t$$$fromInclude(\"x\"){\n\t\tQuit\n\t}\n\tQuit\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Include file 'NoSuchInclude' is not defined in the current workspace"),
            "{diagnostics:?}"
        );
    }

    // Two unresolved `#include`s, both needed for the parse to succeed -- every one of them
    // should surface, not just the first, so fixing one doesn't just uncover the next.
    #[test]
    fn every_unresolved_include_is_reported_when_the_parse_fails() {
        let workspace = Workspace::new(None, None);
        let uri = "Main.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Main\n\n\t#include FirstMissing\n\t#include SecondMissing\n\t$$$fromInclude(\"x\"){\n\t\tQuit\n\t}\n\tQuit\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri.clone());
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Include file 'FirstMissing' is not defined in the current workspace"),
            "{diagnostics:?}"
        );
        assert_eq!(
            diagnostics[0].related_information.len(),
            1,
            "{diagnostics:?}"
        );
        assert!(
            diagnostics[0].related_information[0]
                .message
                .contains("Include file 'SecondMissing' is not defined in the current workspace"),
            "{diagnostics:?}"
        );
        assert_eq!(diagnostics[0].related_information[0].location.uri, uri);
    }

    #[test]
    fn class_include_expands_a_macro_and_relocates_an_error_inside_it() {
        let workspace = Workspace::new(None, None);
        let inc_uri = "MacroLib.inc".to_string();
        workspace.open(
            inc_uri,
            "ROUTINE MacroLib [Type=INC]\n#define WRITEY Write y\n".to_string(),
        );
        let uri = "IncludeClass.cls".to_string();
        workspace.open(
            uri.clone(),
            "Include MacroLib\nClass Test.IncludeClass\n{\n\nClassMethod Foo()\n{\n $$$WRITEY\n}\n\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("undefined"),
            "{diagnostics:?}"
        );
        // Line 6 is `$$$WRITEY`'s own call site, not a position inside `MacroLib.inc`.
        assert_eq!(diagnostics[0].range.start.line, 6, "{diagnostics:?}");
    }

    #[test]
    fn class_missing_include_is_reported_once_at_the_include_line() {
        let workspace = Workspace::new(None, None);
        let uri = "MissingInclude.cls".to_string();
        workspace.open(
            uri.clone(),
            "Include NoSuchInclude\nClass Test.MissingInclude\n{\n\nClassMethod Foo()\n{\n}\n\nClassMethod Bar()\n{\n}\n\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Include file 'NoSuchInclude' is not defined in the current workspace"),
            "{diagnostics:?}"
        );
        // Reported once, at the `Include` line itself -- not once per method, and not at the
        // class's own declaration (confusingly implying the class name is the problem).
        assert_eq!(diagnostics[0].range.start.line, 0, "{diagnostics:?}");
        assert_eq!(diagnostics[0].range.start.character, 8, "{diagnostics:?}");
    }

    // Same failure mode as `missing_include_is_reported_even_when_a_macro_call_opens_a_block`,
    // but for a class method body instead of a routine -- shouldn't also raise the resulting
    // generic syntax error on top of the (already reported) missing include.
    #[test]
    fn class_missing_include_does_not_also_report_the_resulting_syntax_error() {
        let workspace = Workspace::new(None, None);
        let uri = "MissingInclude.cls".to_string();
        workspace.open(
            uri.clone(),
            "Include NoSuchInclude\nClass Test.MissingInclude\n{\n\nClassMethod Foo()\n{\n\t$$$fromInclude(\"x\"){\n\t\tQuit\n\t}\n}\n\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Include file 'NoSuchInclude' is not defined in the current workspace"),
            "{diagnostics:?}"
        );
    }

    // `IncludeGenerator` only supplies macros to `[ CodeMode = generator ]` methods, whose bodies
    // ascot never checks as ObjectScript (they generate code, rather than being code) -- an
    // unresolvable one shouldn't raise "cannot find include" for something nothing here uses.
    #[test]
    fn unresolved_include_generator_is_not_reported() {
        let workspace = Workspace::new(None, None);
        let uri = "IncludeGen.cls".to_string();
        workspace.open(
            uri.clone(),
            "IncludeGenerator %BigData.ShardingManager\n\nClass Test.IncludeGen\n{\n\nClassMethod Foo()\n{\n\tQuit\n}\n\n}\n"
                .to_string(),
        );
        assert!(workspace.diagnostics(uri).is_empty());
    }

    #[test]
    fn class_without_include_is_unaffected_by_macro_expansion() {
        let workspace = Workspace::new(None, None);
        let uri = "NoInclude.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.NoInclude {\nClassMethod Foo() As %Integer\n{\n set x = 0\n return x\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    struct OccErrorsIris;

    #[cfg(not(target_arch = "wasm32"))]
    impl crate::iris::IrisClient for OccErrorsIris {
        fn get_cls(&self, _cls: &str) -> Option<bindings::ClassInfo> {
            None
        }
        fn get_int_or_mac(&self, _name: &str) -> Option<bindings::RoutineSource> {
            None
        }
        fn get_inc(&self, name: &str) -> Option<bindings::RoutineSource> {
            (name == "%occErrors").then(|| bindings::RoutineSource {
                text: "ROUTINE Errors [Type=INC]\n#define GeneralError 5001\n".to_string(),
                uri: None,
            })
        }
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn class_gets_occ_errors_macros_via_iris_without_an_explicit_include() {
        let workspace = Workspace::new_with_iris(Some(Box::new(OccErrorsIris)), None);
        let uri = "NoIncludeButOcc.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.NoIncludeButOcc {\nClassMethod Foo()\n{\n write $$$GeneralError\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn macro_nested_inside_an_unresolved_call_still_expands() {
        let workspace = Workspace::new_with_iris(Some(Box::new(OccErrorsIris)), None);
        let uri = "NestedInUnresolvedCall.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.NestedInUnresolvedCall {\nClassMethod Foo() As %Status\n{\n #define ERROR(%code,%msg) $lb(0,%code,%msg)\n return $$$ERROR($$$GeneralError, \"bad\")\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn undefined_macro_suggests_similar_names() {
        let workspace = Workspace::new(None, None);
        let uri = "UndefMacro.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.UndefMacro {\nClassMethod Foo()\n{\n #define ISERR(%sc) (''%sc)\n #define ISOK(%sc) ('%sc)\n #define ERROR(%code) $lb(0,%code)\n write $$$ISERROR(1)\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        // `ISERR` is a prefix of `ISERROR` (a typo with a trailing extra chunk) -- Jaro-Winkler's
        // prefix bonus should put it first, ahead of same-edit-distance but unrelated names.
        assert!(
            diagnostics[0].message.contains("did you mean: ISERR, "),
            "{diagnostics:?}"
        );
    }

    // A macro call's argument is re-parsed as a standalone expression to type-check it (see
    // `check_macro_arg_as_expr`), so an error inside it (here, another undefined macro) must be
    // relocated back to its real position in the document -- not left pointing at the whole
    // outer call, which is what a plain `strip_range` used to fall back to.
    #[test]
    fn undefined_macro_inside_another_macros_argument_is_precisely_ranged() {
        let workspace = Workspace::new(None, None);
        let uri = "UndefMacroArg.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.UndefMacroArg {\nClassMethod Foo()\n{\n #define ERROR(%code,%msg) $lb(0,%code,%msg)\n set sc = $$$ERROR($$$UndefinedMacro,\"x\")\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("'UndefinedMacro' is undefined"),
            "{diagnostics:?}"
        );
        let range = diagnostics[0].range;
        assert_eq!(range.start.line, range.end.line, "{diagnostics:?}");
        assert_eq!(
            range.end.character - range.start.character,
            "$$$UndefinedMacro".len() as u32,
            "{diagnostics:?}"
        );
    }

    // `%Library.SystemBase`/`%Library.RegisteredObject`'s own members (`REGISTERED_OBJECT_MEMBERS`)
    // are hardcoded as always available rather than relying on the `extends` chain actually
    // resolving those system classes, which is routinely impossible without an IRIS connection --
    // so none of these should even get the usual "unverified" hint, let alone an error, for a
    // class extending nothing at all.
    #[test]
    fn registered_object_members_are_always_available() {
        let workspace = Workspace::new(None, None);
        let uri = "IsA.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.IsA Extends %RegisteredObject\n{\nClassMethod Foo(obj As Test.IsA) As %Boolean\n{\n write obj.%ClassName(1)\n write obj.%ConstructClone()\n Quit obj.%IsA(\"Test.IsA\")\n}\n}\n"
                .to_string(),
        );
        assert!(workspace.diagnostics(uri).is_empty());
    }

    // A class with no `Extends` at all has no real ancestor to inherit these from -- same
    // "rootless" bucket as an SQL view class or `%Library.Any` (see
    // `WorkspaceEnv::confirmed_outside_system_base`), not `%Library.SystemBase`, so the hardcode
    // must not apply here even though `is_datatype` alone wouldn't rule it out either.
    #[test]
    fn registered_object_members_are_not_available_on_a_class_with_no_ancestor() {
        let workspace = Workspace::new(None, None);
        let uri = "NoExtends.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.NoExtends\n{\nClassMethod Foo(obj As Test.NoExtends) As %Boolean\n{\n Quit obj.%IsA(\"Test.NoExtends\")\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("Cannot find member '%IsA'"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn cannot_find_member_suggests_a_similar_member() {
        let workspace = Workspace::new(None, None);
        let uri = "UndefMember.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.UndefMember\n{\nClassMethod Foo()\n{\n do ..Barr()\n}\nClassMethod Bar()\n{\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("did you mean: ")
                && diagnostics[0].message.contains("Bar"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn cannot_find_member_mentions_percent_get_when_available() {
        let workspace = Workspace::new(None, None);
        let uri = "HasGet.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.HasGet\n{\nClassMethod Foo()\n{\n do ..NoSuchThing()\n}\nClassMethod %Get(name As %String)\n{\n write name\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("%Get(\"NoSuchThing\")"),
            "{diagnostics:?}"
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn cannot_find_member_always_suggests_percent_get_prefixed_methods() {
        // A dynamic-property-style getter's own name (e.g. `%GetData`) rarely resembles the
        // property a caller mistakenly wrote instead (`AtEnd`) -- so this must not depend on
        // `top_similar`'s ranking to surface it.
        struct GetterIris;
        impl crate::iris::IrisClient for GetterIris {
            fn get_cls(&self, cls: &str) -> Option<bindings::ClassInfo> {
                let zero = bindings::Position {
                    line: 0,
                    character: 0,
                };
                let member = |name: &str| bindings::MemberInfo {
                    doc: String::new(),
                    before: zero,
                    name: bindings::NameInfo {
                        before: zero,
                        content: name.to_string(),
                        after: zero,
                    },
                    aliases: vec![],
                    deprecated: false,
                    kind: bindings::MemberKind::Method(bindings::MethodInfo {
                        normal: vec![],
                        variadic: None,
                        t: None,
                        procedure_block: None,
                        body: bindings::Range {
                            start: zero,
                            end: zero,
                        },
                    }),
                    after: zero,
                    uri: None,
                    origin: None,
                };
                (cls == "%Fake.Getter").then(|| bindings::ClassInfo {
                    doc: String::new(),
                    name: bindings::NameInfo {
                        before: zero,
                        content: cls.to_string(),
                        after: zero,
                    },
                    extends: vec![],
                    deprecated: false,
                    members: vec![member("%GetData"), member("Close")],
                    property_class_names: vec![],
                    includes: vec![],
                    is_datatype: Some(false),
                    uri: None,
                })
            }
            fn get_inc(&self, _name: &str) -> Option<bindings::RoutineSource> {
                None
            }
            fn get_int_or_mac(&self, _name: &str) -> Option<bindings::RoutineSource> {
                None
            }
        }
        let workspace = Workspace::new_with_iris(Some(Box::new(GetterIris)), None);
        let uri = "GetterLoop.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.GetterLoop\n{\nClassMethod Foo(rs As %Fake.Getter)\n{\n write rs.AtEnd\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("%GetData"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn sqlcode_undefined_before_any_esql() {
        let workspace = Workspace::new(None, None);
        let uri = "EsqlTest.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.EsqlTest {\nClassMethod Foo() {\n write SQLCODE\n quit\n}\n}\n".to_string(),
        );

        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("undefined"));
    }

    #[test]
    fn generated_index_methods_are_recognized() {
        let workspace = Workspace::new(None, None);
        let uri = "IdxTest.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.IdxTest {\nProperty Name As %String;\nIndex NameIdx On Name;\nClassMethod Foo() {\n write ..NameIdxOpen(\"bob\")\n write ..NameIdxExists(\"bob\")\n write ..NameIdxDelete(\"bob\")\n quit\n}\n}\n".to_string(),
        );

        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn generated_index_open_types_its_value_args_from_the_indexed_properties() {
        let workspace = Workspace::new(None, None);
        let base_uri = "IdxBase.cls".to_string();
        let uri = "IdxTest2.cls".to_string();
        workspace.open(base_uri, "Class Test.IdxBase {\n}\n".to_string());
        // `RefIdxOpen`'s value argument should be typed as `Test.IdxBase` (the indexed
        // property's own type), not `%Any` — passing a plain integer should be flagged.
        workspace.open(
            uri.clone(),
            "Class Test.IdxTest2 {\nProperty Ref As Test.IdxBase;\nIndex RefIdx On Ref;\nClassMethod Foo() {\n write ..RefIdxOpen(5)\n quit\n}\n}\n".to_string(),
        );

        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("Cannot assign"));
    }

    #[test]
    fn generated_property_accessor_methods_are_recognized() {
        let workspace = Workspace::new(None, None);
        let uri = "PropTest.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.PropTest {\nProperty Name As %String;\nMethod Foo() {\n write ..NameGet()\n write ..NameSet(\"bob\")\n write ..NameReset()\n write ..NameIsValid(\"bob\")\n quit\n}\n}\n".to_string(),
        );

        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn generated_get_object_only_applies_to_reference_properties() {
        let workspace = Workspace::new(None, None);
        let base_uri = "RefBase.cls".to_string();
        let datatype_uri = "DataTypeBase.cls".to_string();
        let uri = "RefTest.cls".to_string();
        workspace.open(
            base_uri,
            "Class Test.RefBase Extends %RegisteredObject {\n}\n".to_string(),
        );
        // A real reference class vs. one explicitly marked `[ClassType = datatype]` — resolving
        // this distinction (rather than treating every property type as "maybe an object")
        // needs a workspace-known class; an unresolvable external one like `%String` can't
        // prove the negative without an IRIS connection.
        workspace.open(
            datatype_uri,
            "Class Test.DataTypeBase [ClassType = datatype] {\n}\n".to_string(),
        );
        // No `Extends` here: extending an unresolvable external class (with no IRIS connection)
        // hits a separate, pre-existing "assume any member exists" fallback that would mask
        // this test's negative case.
        workspace.open(
            uri.clone(),
            "Class Test.RefTest {\nProperty Other As Test.RefBase;\nProperty Name As Test.DataTypeBase;\nMethod Foo() {\n do ..OtherGetObject()\n quit\n}\nMethod Bad() {\n do ..NameGetObject()\n quit\n}\n}\n".to_string(),
        );

        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("Cannot find member"));
    }

    #[test]
    fn super_call_resolves_to_the_superclass_method() {
        let workspace = Workspace::new(None, None);
        let base_uri = "SuperBase.cls".to_string();
        workspace.open(
            base_uri,
            "Class Test.SuperBase Extends %RegisteredObject {\nMethod Foo(a As %String) As %Integer {\n quit 1\n}\n}\n".to_string(),
        );

        // As an expression: the resolved return type flows through normally.
        let uri = "SuperOk.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.SuperOk Extends Test.SuperBase {\nMethod Foo(a As %String) As %Integer {\n set x = ##super(a)\n quit x\n}\n}\n".to_string(),
        );
        assert!(workspace.diagnostics(uri).is_empty());

        // As a bare `Do` command -- the other grammar path (`MethodOrFuncCallExpr::Super`).
        let uri = "SuperDo.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.SuperDo Extends Test.SuperBase {\nMethod Foo(a As %String) As %Integer {\n do ##super(a)\n quit 1\n}\n}\n".to_string(),
        );
        assert!(workspace.diagnostics(uri).is_empty());

        // Arguments are checked against the superclass's signature, not the override's own.
        let uri = "SuperTooMany.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.SuperTooMany Extends Test.SuperBase {\nMethod Foo(a As %String) As %Integer {\n quit ##super(a, 1)\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("Too many arguments"));

        // No ancestor (all local, none unresolvable) declares this method at all.
        let missing_base_uri = "SuperMissingBase.cls".to_string();
        workspace.open(
            missing_base_uri,
            "Class Test.SuperMissingBase {\n}\n".to_string(),
        );
        let uri = "SuperMissing.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.SuperMissing Extends Test.SuperMissingBase {\nMethod Bar() {\n do ##super()\n quit\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("Cannot find member"));
    }

    #[test]
    fn output_param_never_assigned_is_reported() {
        let workspace = Workspace::new(None, None);
        let uri = "OutputTest.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.OutputTest {\nClassMethod Foo(Output x) {\n quit\n}\n}\n".to_string(),
        );

        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("never assigned"));
    }

    #[test]
    fn output_param_assigned_via_set_is_not_reported() {
        let workspace = Workspace::new(None, None);
        let uri = "OutputTest2.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.OutputTest2 {\nClassMethod Foo(Output x) {\n set x = 1\n quit\n}\n}\n"
                .to_string(),
        );

        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn output_param_delegated_to_another_call_is_not_reported() {
        let workspace = Workspace::new(None, None);
        let uri = "OutputTest3.cls".to_string();
        // Delegating an `Output` param to another call's own `Output` argument (`.x`) is a
        // legitimate way to assign it — the callee is trusted to write it.
        workspace.open(
            uri.clone(),
            "Class Test.OutputTest3 {\nClassMethod Foo(Output x) As %Integer {\n do ..Helper(.x)\n quit x\n}\nClassMethod Helper(Output y) As %Integer {\n set y = 1\n quit y\n}\n}\n".to_string(),
        );

        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn parameter_and_property_share_a_name_without_conflict() {
        let workspace = Workspace::new(None, None);
        let uri = "ParamScope.cls".to_string();
        // `Parameter Foo` and `Property Foo` are in separate namespaces (`#Foo` vs `Foo`) —
        // neither a "duplicated member" diagnostic nor a resolution mix-up should occur.
        workspace.open(
            uri.clone(),
            "Class Test.ParamScope {\nParameter Foo = \"bar\";\nProperty Foo As %String;\nMethod Test() {\n write ..#Foo\n write ..Foo\n}\n}\n".to_string(),
        );

        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn property_declared_type_resolves_in_workspace_without_a_hint() {
        let workspace = Workspace::new(None, None);
        let helper_uri = "Helper.cls".to_string();
        workspace.open(
            helper_uri,
            "Class Test.Helper Extends %RegisteredObject {\n}\n".to_string(),
        );
        let uri = "Owner.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Owner Extends %RegisteredObject {\nProperty P As Test.Helper;\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn property_declared_type_outside_workspace_gets_a_hint() {
        let workspace = Workspace::new(None, None);
        let uri = "Owner.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Owner Extends %RegisteredObject {\nProperty P As App.NotOpen;\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].severity, bindings::DiagnosticSeverity::Hint);
        assert!(diagnostics[0].message.contains("App.NotOpen"));
    }

    #[test]
    fn unrecognized_property_keyword_is_reported() {
        let workspace = Workspace::new(None, None);
        let uri = "Owner.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Owner Extends %RegisteredObject {\nProperty P As %String [ Required, Flooble ];\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].severity, bindings::DiagnosticSeverity::Error);
        assert!(diagnostics[0].message.contains("'Flooble'"));
        assert!(
            diagnostics[0]
                .message
                .contains("not a recognized property keyword")
        );
    }

    #[test]
    fn property_keyword_misplaced_in_type_params_is_reported() {
        let workspace = Workspace::new(None, None);
        let uri = "Owner.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Owner Extends %RegisteredObject {\nProperty Value As %Integer(InitialExpression = 0);\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("'InitialExpression'"));
        assert!(
            diagnostics[0].message.contains("is a property keyword"),
            "{diagnostics:?}"
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn type_param_not_on_the_real_type_is_reported_via_iris() {
        // IRIS knows the class, and it has no parameters at all.
        struct NoSuchParamIris;
        impl crate::iris::IrisClient for NoSuchParamIris {
            fn get_cls(&self, cls: &str) -> Option<bindings::ClassInfo> {
                let zero = bindings::Position {
                    line: 0,
                    character: 0,
                };
                Some(bindings::ClassInfo {
                    doc: String::new(),
                    name: bindings::NameInfo {
                        before: zero,
                        content: cls.to_string(),
                        after: zero,
                    },
                    extends: vec![],
                    deprecated: false,
                    members: vec![],
                    property_class_names: vec![],
                    includes: vec![],
                    is_datatype: Some(true),
                    uri: None,
                })
            }
            fn get_inc(&self, _name: &str) -> Option<bindings::RoutineSource> {
                None
            }
            fn get_int_or_mac(&self, _name: &str) -> Option<bindings::RoutineSource> {
                None
            }
        }
        let workspace = Workspace::new_with_iris(Some(Box::new(NoSuchParamIris)), None);
        let uri = "Owner.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Owner Extends %RegisteredObject {\nProperty Value As %Integer(FooBarBaz = 1);\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("'FooBarBaz' is not a parameter of"),
            "{diagnostics:?}"
        );
        assert!(!diagnostics[0].message.contains("property keyword"));
    }

    #[test]
    fn property_class_keyword_adds_valid_type_params() {
        let workspace = Workspace::new(None, None);
        workspace.open(
            "MyPropClass.cls".to_string(),
            "Class Test.MyPropClass\n{\nParameter MYPARAM As %String = \"x\";\n}\n".to_string(),
        );
        let uri = "HasPropertyClass.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.HasPropertyClass Extends %RegisteredObject [ PropertyClass = Test.MyPropClass ]\n{\nProperty Foo As %String(MYPARAM = \"y\");\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn property_class_keyword_is_inherited_by_subclasses() {
        let workspace = Workspace::new(None, None);
        workspace.open(
            "MyPropClass2.cls".to_string(),
            "Class Test.MyPropClass2\n{\nParameter MYPARAM As %String = \"x\";\n}\n".to_string(),
        );
        workspace.open(
            "Base2.cls".to_string(),
            "Class Test.Base2 Extends %RegisteredObject [ PropertyClass = Test.MyPropClass2 ]\n{\n}\n"
                .to_string(),
        );
        let uri = "Sub2.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Sub2 Extends Test.Base2\n{\nProperty Foo As %String(MYPARAM = \"y\");\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn unrecognized_parameter_type_and_keyword_are_reported() {
        let workspace = Workspace::new(None, None);
        let uri = "Owner.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Owner Extends %RegisteredObject {\nParameter P As notatype [ Flooble ] = \"x\";\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert!(
            diagnostics
                .iter()
                .all(|d| d.severity == bindings::DiagnosticSeverity::Error)
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("'notatype'") && d.message.contains("parameter type"))
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("'Flooble'") && d.message.contains("parameter keyword"))
        );
    }

    #[test]
    fn recognized_parameter_types_and_keywords_are_not_reported() {
        let workspace = Workspace::new(None, None);
        let uri = "Owner.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Owner Extends %RegisteredObject {\nParameter A As string [ Deprecated ] = \"x\";\nParameter B As classname = \"x\";\nParameter C As cosexpression = \"1+1\";\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    struct MockIrisClient {
        is_datatype: Option<bool>,
        mem: Option<bindings::MemberInfo>,
    }

    #[cfg(not(target_arch = "wasm32"))]
    impl crate::iris::IrisClient for MockIrisClient {
        fn get_cls(&self, _cls: &str) -> Option<bindings::ClassInfo> {
            let zero = bindings::Position {
                line: 0,
                character: 0,
            };
            Some(bindings::ClassInfo {
                doc: String::new(),
                name: bindings::NameInfo {
                    before: zero,
                    content: String::new(),
                    after: zero,
                },
                extends: vec![],
                deprecated: false,
                members: self.mem.clone().into_iter().collect(),
                property_class_names: vec![],
                includes: vec![],
                is_datatype: self.is_datatype,
                uri: None,
            })
        }
        fn get_inc(&self, _name: &str) -> Option<bindings::RoutineSource> {
            None
        }
        fn get_int_or_mac(&self, _name: &str) -> Option<bindings::RoutineSource> {
            None
        }
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn property_declared_type_confirmed_by_iris_gets_no_hint() {
        let workspace = Workspace::new_with_iris(
            Some(Box::new(MockIrisClient {
                is_datatype: Some(true),
                mem: None,
            })),
            None,
        );
        let uri = "Owner.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Owner Extends %RegisteredObject {\nProperty P As App.NotOpen;\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn member_reference_resolved_by_iris_gets_no_hint() {
        let zero = bindings::Position {
            line: 0,
            character: 0,
        };
        let mem = bindings::MemberInfo {
            doc: String::new(),
            before: zero,
            name: bindings::NameInfo {
                before: zero,
                content: "SomeInheritedMethod".to_string(),
                after: zero,
            },
            aliases: vec![],
            deprecated: false,
            kind: bindings::MemberKind::Method(bindings::MethodInfo {
                normal: vec![],
                variadic: None,
                t: None,
                procedure_block: None,
                body: bindings::Range {
                    start: zero,
                    end: zero,
                },
            }),
            after: zero,
            uri: None,
            origin: None,
        };
        let workspace = Workspace::new_with_iris(
            Some(Box::new(MockIrisClient {
                is_datatype: None,
                mem: Some(mem),
            })),
            None,
        );
        let uri = "Probe.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Probe Extends %RegisteredObject {\nMethod Foo() {\n do ..SomeInheritedMethod()\n quit\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn member_with_an_empty_string_type_from_iris_is_treated_as_any() {
        // A host can report an empty string, rather than omitting it, for "no declared type" --
        // this must not leak into `Ty::from` as a literal (nonsense) class name "".
        let zero = bindings::Position {
            line: 0,
            character: 0,
        };
        let mem = bindings::MemberInfo {
            doc: String::new(),
            before: zero,
            name: bindings::NameInfo {
                before: zero,
                content: "SomeInheritedProp".to_string(),
                after: zero,
            },
            aliases: vec![],
            deprecated: false,
            kind: bindings::MemberKind::Property(bindings::PropertyInfo {
                t: Some(String::new()),
                collection: None,
                multidimensional: false,
            }),
            after: zero,
            uri: None,
            origin: None,
        };
        let workspace = Workspace::new_with_iris(
            Some(Box::new(MockIrisClient {
                is_datatype: None,
                mem: Some(mem),
            })),
            None,
        );
        let uri = "Probe.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Probe Extends %RegisteredObject {\nMethod Foo() {\n do ..SomeInheritedProp()\n quit\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn unverifiable_member_reference_gets_a_hint() {
        let workspace = Workspace::new(None, None);
        let uri = "Probe.cls".to_string();
        // `%RegisteredObject` is outside the workspace and there's no IRIS connection, so its
        // members can't be confirmed — intentionally not an error, but should surface as a hint
        // right at this reference, rather than silently accepting anything.
        workspace.open(
            uri.clone(),
            "Class Test.Probe Extends %RegisteredObject {\nMethod Foo() {\n do ..SomeInheritedMethod()\n quit\n}\n}\n".to_string(),
        );

        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].severity, bindings::DiagnosticSeverity::Hint);
        assert!(diagnostics[0].message.contains("SomeInheritedMethod"));
        assert_eq!(diagnostics[0].range.start.line, 2);
    }

    #[test]
    fn dispatch_method_resolves_an_undefined_instance_call() {
        let workspace = Workspace::new(None, None);
        let uri = "HasDispatchMethod.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.HasDispatchMethod\n{\nMethod %DispatchMethod(Method As %String, Args...)\n{\n write Method,Args\n}\nMethod Foo()\n{\n do ..Bar(1,2)\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn dispatch_class_method_resolves_an_undefined_class_call() {
        let workspace = Workspace::new(None, None);
        let uri = "HasDispatchClassMethod.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.HasDispatchClassMethod\n{\nClassMethod %DispatchClassMethod(Class As %String, Method As %String, Args...)\n{\n write Class,Method,Args\n}\nClassMethod UseIt()\n{\n do ..Bar(1,2)\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn dispatch_set_property_resolves_an_undefined_property_set_via_explicit_object() {
        let workspace = Workspace::new(None, None);
        let uri = "HasDispatchSetProperty.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.HasDispatchSetProperty\n{\nMethod %DispatchSetProperty(Property As %String, Value)\n{\n write Property,Value\n}\nClassMethod UseIt(obj As Test.HasDispatchSetProperty)\n{\n Set obj.Foo = 1\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn dispatch_set_property_resolves_an_undefined_property_set_on_self() {
        let workspace = Workspace::new(None, None);
        let uri = "HasDispatchSetPropertySelf.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.HasDispatchSetPropertySelf\n{\nMethod %DispatchSetProperty(Property As %String, Value)\n{\n write Property,Value\n}\nMethod Foo()\n{\n Set ..Bar = 1\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn set_on_self_still_resolves_a_real_property() {
        let workspace = Workspace::new(None, None);
        let uri = "SetSelfRealProperty.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.SetSelfRealProperty\n{\nProperty RealProp As %String;\nMethod Foo()\n{\n Set ..RealProp = \"hi\"\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    // Setting a special variable (`$ZTrap`, `$ETRAP`, ...) has no effect this checker tracks --
    // unlike a local variable, it's never a `Flow` binding, so there's nothing to check beyond
    // accepting it (previously misreported as "Type checking not implemented for 'This kind of
    // set'").
    #[test]
    fn set_special_variable_is_accepted() {
        let workspace = Workspace::new(None, None);
        let uri = "SetZTrap.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\nFoo()\n\tSet $ZTrap = \"Handler\"\n\tQuit\n".to_string(),
        );
        assert!(workspace.diagnostics(uri).is_empty());
    }

    #[test]
    fn dispatch_get_property_does_not_apply_from_class_context() {
        let workspace = Workspace::new(None, None);
        let uri = "DispatchGetOnlyFromClass.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.DispatchGetOnlyFromClass\n{\nMethod %DispatchGetProperty(Property As %String)\n{\n write Property\n}\nClassMethod UseIt()\n{\n Write ##class(Test.DispatchGetOnlyFromClass).Bar\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Cannot reference instance-only member"),
            "{diagnostics:?}"
        );
    }

    // Every `%RegisteredObject`/`%SerialObject` inherits a meaningless no-op/error default
    // implementation of `%DispatchClassMethod` etc. from `%Library.SystemBase` -- IRIS reports
    // it as a compiled member of every such class regardless of whether the class overrides it.
    #[cfg(not(target_arch = "wasm32"))]
    struct DispatchOriginIris {
        origin: Option<String>,
    }

    #[cfg(not(target_arch = "wasm32"))]
    impl crate::iris::IrisClient for DispatchOriginIris {
        fn get_cls(&self, _cls: &str) -> Option<bindings::ClassInfo> {
            let zero = bindings::Position {
                line: 0,
                character: 0,
            };
            Some(bindings::ClassInfo {
                doc: String::new(),
                name: bindings::NameInfo {
                    before: zero,
                    content: String::new(),
                    after: zero,
                },
                extends: vec![],
                deprecated: false,
                members: vec![bindings::MemberInfo {
                    doc: String::new(),
                    before: zero,
                    name: bindings::NameInfo {
                        before: zero,
                        content: "%DispatchClassMethod".to_string(),
                        after: zero,
                    },
                    aliases: vec![],
                    deprecated: false,
                    kind: bindings::MemberKind::Method(bindings::MethodInfo {
                        normal: vec![],
                        variadic: None,
                        t: None,
                        procedure_block: None,
                        body: bindings::Range {
                            start: zero,
                            end: zero,
                        },
                    }),
                    after: zero,
                    uri: None,
                    origin: self.origin.clone(),
                }],
                property_class_names: vec![],
                includes: vec![],
                is_datatype: Some(false),
                uri: None,
            })
        }
        fn get_inc(&self, _name: &str) -> Option<bindings::RoutineSource> {
            None
        }
        fn get_int_or_mac(&self, _name: &str) -> Option<bindings::RoutineSource> {
            None
        }
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn dispatch_class_method_inherited_unchanged_from_system_base_does_not_count() {
        let workspace = Workspace::new_with_iris(
            Some(Box::new(DispatchOriginIris {
                origin: Some("%Library.SystemBase".to_string()),
            })),
            None,
        );
        let uri = "NoRealDispatch.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.NoRealDispatch Extends %Fake\n{\nClassMethod Foo()\n{\n do ..Bar()\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("Cannot find member"),
            "{diagnostics:?}"
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn dispatch_class_method_overridden_on_an_iris_ancestor_is_resolved() {
        let workspace = Workspace::new_with_iris(
            Some(Box::new(DispatchOriginIris {
                origin: Some("%Library.Fake".to_string()),
            })),
            None,
        );
        let uri = "RealDispatch.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.RealDispatch Extends %Fake\n{\nClassMethod Foo()\n{\n do ..Bar()\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn open_ignores_a_file_with_an_unrecognized_extension() {
        let workspace = Workspace::new(None, None);
        let uri = "notes.txt".to_string();
        workspace.open(
            uri.clone(),
            "this is not ObjectScript at all {{{".to_string(),
        );
        assert!(workspace.diagnostics(uri).is_empty());
    }

    // `cities` starts `Const("")` but the loop's `Else` branch reassigns it to `%String` -- must
    // not be flagged always-true/false against pre-loop bindings alone.
    #[test]
    fn condition_narrowed_only_by_pre_loop_bindings_is_not_flagged() {
        let workspace = Workspace::new(None, None);
        let uri = "Loop.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Loop {\nClassMethod Foo(statement) As %String {\n set cities = \"\"\n while statement.%Next() {\n if cities '= \"\" {\n set cities = cities _ \"-\" _ statement.%Get(\"City\")\n } else {\n set cities = statement.%Get(\"City\")\n }\n }\n return cities\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    // `$Data`'s real return values are 0/1/10/11 -- comparing against 2 is genuinely always
    // false, but the diagnostic must anchor on *that* condition's own range, not the first
    // `If`'s, and only that branch's body should be unreachable.
    #[test]
    fn always_false_elseif_condition_is_reported_at_its_own_range() {
        let workspace = Workspace::new(None, None);
        let uri = "D.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.D {\nClassMethod M() As %String {\n set data = $Data(^EvalNode)\n if data = 0 {\n  quit \"U\"\n } elseif data = 2 {\n  quit \"C\"\n }\n quit \"X\"\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        let always_false = diagnostics
            .iter()
            .find(|d| d.message.contains("always false"))
            .unwrap_or_else(|| panic!("expected an always-false diagnostic: {diagnostics:?}"));
        assert_eq!(always_false.range.start.line, 5, "{diagnostics:?}");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("Unreachable") && d.range.start.line == 6),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn every_always_false_elseif_branch_is_reported_not_just_the_first() {
        let workspace = Workspace::new(None, None);
        let uri = "D2.cls".to_string();
        // Each of `elseif data = 2`/`elseif data = 3` is unreachable on its own -- checking the
        // second shouldn't be cut short just because the first was already found unreachable.
        workspace.open(
            uri.clone(),
            "Class Test.D2 {\nClassMethod M() As %String {\n set data = $Data(^EvalNode)\n if data = 0 {\n  quit \"U\"\n } elseif data = 1 {\n  quit \"V\"\n } elseif data = 2 {\n  quit \"C\"\n } elseif data = 3 {\n  quit \"B\"\n }\n quit \"X\"\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        let always_false_lines: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("always false"))
            .map(|d| d.range.start.line)
            .collect();
        assert_eq!(always_false_lines, vec![7, 9], "{diagnostics:?}");
        let unreachable_lines: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("Unreachable"))
            .map(|d| d.range.start.line)
            .collect();
        assert_eq!(unreachable_lines, vec![8, 10], "{diagnostics:?}");
    }

    #[test]
    fn duplicate_class_name_is_reported() {
        let workspace = Workspace::new(None, None);
        let a_uri = "A.cls".to_string();
        let b_uri = "B.cls".to_string();
        workspace.open(
            a_uri.clone(),
            "Class Test.Dup {\nClassMethod Foo() {\n}\n}\n".to_string(),
        );
        workspace.open(
            b_uri.clone(),
            "Class Test.Dup {\nClassMethod Bar() {\n}\n}\n".to_string(),
        );

        // Classes are name-indexed eagerly on `open` (tier A) -- unlike a routine's expansion, a
        // class's own parse never depends on any other document, so there's no reason to defer
        // it. Both sides see the conflict immediately, regardless of check order.
        let a_diagnostics = workspace.diagnostics(a_uri.clone());
        assert_eq!(a_diagnostics.len(), 1, "{a_diagnostics:?}");
        assert!(a_diagnostics[0].message.contains("Test.Dup"));
        assert_eq!(a_diagnostics[0].related_information[0].location.uri, b_uri);

        let b_diagnostics = workspace.diagnostics(b_uri.clone());
        assert_eq!(b_diagnostics.len(), 1, "{b_diagnostics:?}");
        assert_eq!(b_diagnostics[0].related_information[0].location.uri, a_uri);
    }

    #[test]
    fn diagnostics_and_inlay_hint_share_one_check() {
        let workspace = Workspace::new(None, None);
        let uri = "Shared.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Shared {\nClassMethod Foo() As %Integer {\n set x = ..Bar()\n write x\n quit x\n}\nClassMethod Bar() As %Integer {\n quit 1\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri.clone());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let hints = workspace.inlay_hint(uri.clone(), bindings::Range::default());
        assert!(!hints.is_empty(), "expected an inferred-type hint for `x`");
        // Re-checking after an unrelated no-op read shouldn't change the result.
        assert_eq!(workspace.diagnostics(uri.clone()), diagnostics);
        assert_eq!(workspace.inlay_hint(uri, bindings::Range::default()), hints);
    }

    #[test]
    fn creates_hint_shows_non_new_variable_written_in_subroutine() {
        let workspace = Workspace::new(None, None);
        let uri = "CreatesTest.mac".to_string();
        // A legacy subroutine (no `Public(...)` clause) defaults every undeclared local to
        // shared/non-`New`'d — writing `foo` here silently leaks it into the caller's scope.
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n\tQuit\n\nSub\t;\n\tSet foo = 1\n\tQuit\n".to_string(),
        );

        let diagnostics = workspace.diagnostics(uri.clone());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        let hints = workspace.inlay_hint(uri, bindings::Range::default());
        assert!(hints.iter().any(|h| h.label.contains("[foo]")), "{hints:?}");
    }

    #[test]
    fn creates_hint_shows_non_new_variable_written_via_merge() {
        let workspace = Workspace::new(None, None);
        let uri = "CreatesTest3.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n\tQuit\n\nSub\t;\n\tSet bar = 1\n\tMerge foo = bar\n\tQuit\n"
                .to_string(),
        );

        let diagnostics = workspace.diagnostics(uri.clone());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        let hints = workspace.inlay_hint(uri, bindings::Range::default());
        assert!(
            hints
                .iter()
                .any(|h| h.label.starts_with('[') && h.label.contains("foo")),
            "{hints:?}"
        );
    }

    #[test]
    fn creates_hint_is_absent_for_a_private_by_default_method() {
        let workspace = Workspace::new(None, None);
        let uri = "CreatesTest2.cls".to_string();
        // A `ClassMethod` is private-by-default (unlike a legacy subroutine): an undeclared
        // local defaults to `New`'d, so writing one isn't a "creates" leak.
        workspace.open(
            uri.clone(),
            "Class Test.CreatesTest2 {\nClassMethod Foo() {\n set x = 1\n write x\n}\n}\n"
                .to_string(),
        );

        let diagnostics = workspace.diagnostics(uri.clone());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        let hints = workspace.inlay_hint(uri, bindings::Range::default());
        assert!(!hints.iter().any(|h| h.label.starts_with('[')), "{hints:?}");
    }

    #[test]
    fn bare_do_without_arguments_is_unimplemented() {
        let workspace = Workspace::new(None, None);
        let uri = "BareDo.mac".to_string();
        workspace.open(uri.clone(), "ROUTINE Test\n\tDo\n\tQuit\n".to_string());
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("not implemented"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn do_on_a_status_returning_call_warns_that_it_is_discarded() {
        let workspace = Workspace::new(None, None);
        let uri = "DoStatus.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.DoStatus\n{\nClassMethod Save() As %Status\n{\n #define OK 1\n Quit $$$OK\n}\nClassMethod Foo()\n{\n Do ..Save()\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("returns %Status")
                && diagnostics[0].message.contains("Do discards"),
            "{diagnostics:?}"
        );
        assert_eq!(
            diagnostics[0].severity,
            bindings::DiagnosticSeverity::Warning,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn do_on_a_boolean_returning_call_warns_that_it_is_discarded() {
        let workspace = Workspace::new(None, None);
        let uri = "DoBoolean.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.DoBoolean\n{\nClassMethod IsGood() As %Boolean\n{\n Quit 1\n}\nClassMethod Foo()\n{\n Do ..IsGood()\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("returns %Boolean"));
    }

    #[test]
    fn do_on_a_call_with_no_return_value_is_unaffected() {
        let workspace = Workspace::new(None, None);
        let uri = "DoVoid.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.DoVoid\n{\nClassMethod NoReturn()\n{\n Quit\n}\nClassMethod Foo()\n{\n Do ..NoReturn()\n}\n}\n"
                .to_string(),
        );
        assert!(workspace.diagnostics(uri).is_empty());
    }

    #[test]
    fn setting_a_status_returning_call_does_not_warn() {
        let workspace = Workspace::new(None, None);
        let uri = "SetStatus.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.SetStatus\n{\nClassMethod Save() As %Status\n{\n #define OK 1\n Quit $$$OK\n}\nClassMethod Foo() As %Status\n{\n Set sc = ..Save()\n Quit sc\n}\n}\n"
                .to_string(),
        );
        assert!(workspace.diagnostics(uri).is_empty());
    }

    #[test]
    fn unrecognized_command_lists_available_commands() {
        let workspace = Workspace::new(None, None);
        let uri = "BadCommand.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\tYdummyword x\n\tQuit\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("'Ydummyword' is not a recognized command; available: Do, If,"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn zversion_supports_one_argument_but_zv_abbreviation_does_not() {
        let workspace = Workspace::new(None, None);
        let uri = "ZVersion.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n\tWrite $ZVERSION(1)\n\tQuit\n".to_string(),
        );
        assert!(workspace.diagnostics(uri).is_empty());

        let uri2 = "ZV.mac".to_string();
        workspace.open(
            uri2.clone(),
            "ROUTINE Test\n\n\tWrite $ZV(1)\n\tQuit\n".to_string(),
        );
        assert!(!workspace.diagnostics(uri2).is_empty());
    }

    #[test]
    fn zobjref_is_supported() {
        let workspace = Workspace::new(None, None);
        let uri = "ZObjRef.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n\tSet obj = 1\n\tSet x = $ZOBJREF(obj)\n\tQuit\n".to_string(),
        );
        assert!(workspace.diagnostics(uri).is_empty());
    }

    #[test]
    fn zstrip_accepts_well_formed_const_actions() {
        let workspace = Workspace::new(None, None);
        for action in ["*W", "<>NW", "N'W", "", "<", "<=>EAPCNLUW"] {
            let uri = "ZStripOk.mac".to_string();
            workspace.open(
                uri.clone(),
                format!("ROUTINE Test\n\n\tWrite $ZSTRIP(\"hi\",\"{action}\")\n\tQuit\n"),
            );
            let diagnostics = workspace.diagnostics(uri);
            assert!(diagnostics.is_empty(), "action {action:?}: {diagnostics:?}");
        }
    }

    #[test]
    fn zstrip_rejects_malformed_const_actions() {
        let workspace = Workspace::new(None, None);
        for (action, reason) in [
            ("'W", "must follow at least one mask code"),
            ("N'W'", "must be followed by a mask code"),
            ("N'WA", "must all precede"),
            ("Q", "must be one of"),
            ("**", "must be one of"),
        ] {
            let uri = "ZStripBad.mac".to_string();
            workspace.open(
                uri.clone(),
                format!("ROUTINE Test\n\n\tWrite $ZSTRIP(\"hi\",\"{action}\")\n\tQuit\n"),
            );
            let diagnostics = workspace.diagnostics(uri);
            assert_eq!(diagnostics.len(), 1, "action {action:?}: {diagnostics:?}");
            assert!(
                diagnostics[0].message.contains(reason),
                "action {action:?}: {:?}",
                diagnostics[0].message
            );
        }
    }

    #[test]
    fn zstrip_checks_a_variable_that_provably_holds_a_literal() {
        let workspace = Workspace::new(None, None);
        let uri = "ZStripConstVar.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n\tSet act = \"Q\"\n\tWrite $ZSTRIP(\"hi\",act)\n\tQuit\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("'Q' is not a valid $ZSTRIP action")
        );
    }

    #[test]
    fn zstrip_does_not_check_a_non_const_action() {
        let workspace = Workspace::new(None, None);
        let uri = "ZStripDynamic.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n\tSet act = $GET(^Global)\n\tWrite $ZSTRIP(\"hi\",act)\n\tQuit\n"
                .to_string(),
        );
        assert!(workspace.diagnostics(uri).is_empty());
    }

    #[test]
    fn zstrip_still_checks_a_non_const_actions_definedness() {
        let workspace = Workspace::new(None, None);
        let uri = "ZStripUndef.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.ZStripUndef {\nClassMethod M() {\n Write $ZSTRIP(\"hi\",undefinedVar)\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("is undefined"));
    }

    #[test]
    fn zstrip_warns_when_a_const_action_is_a_number() {
        let workspace = Workspace::new(None, None);
        let uri = "ZStripNumericAction.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.ZStripNumericAction {\nClassMethod M() {\n Write $ZSTRIP(\"hi\",1)\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        // The number isn't a valid action either, so this also gets the usual hard error.
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("implicitly converted to the string"))
        );
    }

    #[test]
    fn fnumber_accepts_well_formed_const_formats() {
        let workspace = Workspace::new(None, None);
        for format in ["", "+", "L", "+,", "E", "G", "N.", "O,"] {
            let uri = "FNumberOk.mac".to_string();
            workspace.open(
                uri.clone(),
                format!("ROUTINE Test\n\n\tWrite $FNUMBER(1.5,\"{format}\")\n\tQuit\n"),
            );
            let diagnostics = workspace.diagnostics(uri);
            assert!(diagnostics.is_empty(), "format {format:?}: {diagnostics:?}");
        }
    }

    #[test]
    fn fnumber_rejects_malformed_const_formats() {
        let workspace = Workspace::new(None, None);
        for (format, reason) in [
            ("Q", "not a recognized format code"),
            ("P+", "cannot be combined"),
            ("EG", "cannot be combined"),
            (",.", "cannot be combined"),
        ] {
            let uri = "FNumberBad.mac".to_string();
            workspace.open(
                uri.clone(),
                format!("ROUTINE Test\n\n\tWrite $FNUMBER(1.5,\"{format}\")\n\tQuit\n"),
            );
            let diagnostics = workspace.diagnostics(uri);
            assert_eq!(diagnostics.len(), 1, "format {format:?}: {diagnostics:?}");
            assert!(
                diagnostics[0].message.contains(reason),
                "format {format:?}: {:?}",
                diagnostics[0].message
            );
        }
    }

    #[test]
    fn fnumber_checks_a_variable_that_provably_holds_a_literal() {
        let workspace = Workspace::new(None, None);
        let uri = "FNumberConstVar.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n\tSet fmt = \"Q\"\n\tWrite $FNUMBER(1.5,fmt)\n\tQuit\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("not a valid $FNUMBER format")
        );
    }

    #[test]
    fn fnumber_does_not_check_a_non_const_format() {
        let workspace = Workspace::new(None, None);
        let uri = "FNumberDynamic.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n\tSet fmt = $GET(^Global)\n\tWrite $FNUMBER(1.5,fmt)\n\tQuit\n"
                .to_string(),
        );
        assert!(workspace.diagnostics(uri).is_empty());
    }

    #[test]
    fn fnumber_warns_when_a_const_format_is_a_number() {
        let workspace = Workspace::new(None, None);
        let uri = "FNumberNumericFormat.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.FNumberNumericFormat {\nClassMethod M() {\n Write $FNUMBER(1.5,5)\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        // `5` isn't a valid format code either, so this also gets the usual hard error.
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("implicitly converted to the string"))
        );
    }

    #[test]
    fn glvn_functions_accept_member_targets() {
        let workspace = Workspace::new(None, None);
        let uri = "GlvnTargets.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.GlvnTargets Extends %RegisteredObject {\nProperty Modified As %Boolean;\nMethod Foo() {\n if $Get(..Modified) {}\n if $Data(..Modified) {}\n set x = $Order(..Modified)\n set y = $Increment(..Modified)\n set z = $Query(..Modified)\n set w = $Name(..Modified)\n write x,y,z,w\n quit\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn data_target_is_updated_even_when_first_argument_is_not_local() {
        let workspace = Workspace::new(None, None);
        let uri = "DataTarget.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.DataTarget {\nClassMethod Foo(Output x) {\n do $Data(^Global,x)\n quit\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn hover_type_shows_inferred_local_variable_type() {
        let workspace = Workspace::new(None, None);
        let uri = "Hover2.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Hover2 {\nClassMethod Foo() As %Integer {\n set x = ..Bar()\n write x\n quit x\n}\nClassMethod Bar() As %Integer {\n quit 1\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri.clone());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        // Line 3 (` write x`): `x` is at character 7.
        let ty = workspace
            .hover_type(
                uri.clone(),
                bindings::Position {
                    line: 3,
                    character: 7,
                },
            )
            .expect("expected a hover type for `x`");
        assert!(ty.contains("Integer"), "{ty}");

        // The `Set` target itself (` set x = ...`, `x` at character 5) isn't covered.
        assert!(
            workspace
                .hover_type(
                    uri,
                    bindings::Position {
                        line: 2,
                        character: 5
                    }
                )
                .is_none()
        );
    }

    #[test]
    fn macro_expansion_shows_expanded_text() {
        let workspace = Workspace::new(None, None);
        let uri = "MacroHover.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n#define GREETING \"hi\"\n\n\tWrite $$$GREETING\n\tQuit\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri.clone());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        // Line 4 (`\tWrite $$$GREETING`): inside the `$$$GREETING` invocation.
        let expansion = workspace
            .macro_expansion(
                uri.clone(),
                bindings::Position {
                    line: 4,
                    character: 10,
                },
            )
            .expect("expected a macro expansion at that position");
        assert!(expansion.contains("hi"), "{expansion:?}");

        // Elsewhere on the same line, not inside the invocation.
        assert!(
            workspace
                .macro_expansion(
                    uri,
                    bindings::Position {
                        line: 4,
                        character: 1
                    }
                )
                .is_none()
        );
    }

    #[test]
    fn query_cls_and_query_mem_prefix_match() {
        let workspace = Workspace::new(None, None);
        workspace.open(
            "Test.Foo.cls".to_string(),
            "Class Test.Foo {\nClassMethod Bar() {\n}\nClassMethod Baz() {\n}\n}\n".to_string(),
        );
        workspace.open("Other.cls".to_string(), "Class Other {\n}\n".to_string());

        let classes = workspace.query_cls("Test.".to_string());
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].0, "Test.Foo.cls");
        assert_eq!(classes[0].1.name.content, "Test.Foo");

        let members = workspace.query_mem("Test.Foo".to_string(), "Ba".to_string());
        let mut names: Vec<&str> = members
            .iter()
            .map(|(_, m)| m.name.content.as_str())
            .collect();
        names.sort();
        assert_eq!(names, ["Bar", "Baz"]);
        assert!(members.iter().all(|(uri, _)| uri == "Test.Foo.cls"));

        assert!(workspace.query_cls("NoSuchClass".to_string()).is_empty());
    }

    #[test]
    fn document_symbol_returns_class_and_members() {
        let workspace = Workspace::new(None, None);
        let uri = "Test.Foo.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Foo {\nClassMethod Bar() {\n}\nProperty Baz;\n}\n".to_string(),
        );

        let document_symbol = workspace
            .document_symbol(uri)
            .expect("expected a document symbol");
        assert_eq!(document_symbol.root.name, "Test.Foo");
        assert_eq!(document_symbol.root.kind, bindings::SymbolKind::Class);
        let mut members: Vec<(&str, bindings::SymbolKind)> = document_symbol
            .members
            .iter()
            .map(|m| (m.name.as_str(), m.kind))
            .collect();
        members.sort_by_key(|(name, _)| *name);
        assert_eq!(
            members,
            [
                ("Bar", bindings::SymbolKind::Method),
                ("Baz", bindings::SymbolKind::Property),
            ]
        );

        assert!(
            workspace
                .document_symbol("NoSuchUri.cls".to_string())
                .is_none()
        );
    }

    #[test]
    fn workspace_symbol_finds_classes_and_members_by_prefix() {
        let workspace = Workspace::new(None, None);
        workspace.open(
            "Test.Foo.cls".to_string(),
            "Class Test.Foo {\nClassMethod Bar() {\n}\n}\n".to_string(),
        );
        workspace.open("Other.cls".to_string(), "Class Other {\n}\n".to_string());

        let symbols = workspace.workspace_symbol("Test.".to_string());
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Test.Foo", "Bar"]);
        assert!(symbols.iter().all(|s| s.location.uri == "Test.Foo.cls"));
    }

    #[test]
    fn document_symbol_returns_routine_and_labels() {
        let workspace = Workspace::new(None, None);
        let uri = "Test.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n\tQuit\n\nFoo\t;\n\tQuit\n\nBar\t;\n\tQuit\n".to_string(),
        );

        let routine_symbol = workspace
            .document_symbol(uri)
            .expect("expected a routine symbol");
        assert_eq!(routine_symbol.root.name, "Test");
        assert_eq!(routine_symbol.root.kind, bindings::SymbolKind::Module);
        let mut labels: Vec<(&str, bindings::SymbolKind)> = routine_symbol
            .members
            .iter()
            .map(|m| (m.name.as_str(), m.kind))
            .collect();
        labels.sort_by_key(|(name, _)| *name);
        assert_eq!(
            labels,
            [
                ("Bar", bindings::SymbolKind::Function),
                ("Foo", bindings::SymbolKind::Function),
            ]
        );
    }

    #[test]
    fn workspace_symbol_finds_routines_and_labels_by_prefix() {
        let workspace = Workspace::new(None, None);
        workspace.open(
            "Test.mac".to_string(),
            "ROUTINE Test\n\n\tQuit\n\nFoo\t;\n\tQuit\n".to_string(),
        );
        workspace.open(
            "Other.mac".to_string(),
            "ROUTINE Other\n\n\tQuit\n\nBar\t;\n\tQuit\n".to_string(),
        );

        let symbols = workspace.workspace_symbol("Test".to_string());
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Test", "Foo"]);
        assert!(symbols.iter().all(|s| s.location.uri == "Test.mac"));
    }

    #[test]
    fn workspace_symbol_parses_a_never_checked_matching_routine() {
        let workspace = Workspace::new(None, None);
        // Opened but never otherwise analyzed (no diagnostics/document_symbol/etc. called) --
        // a name match still gets full detail (its label included), same as a class: tier A
        // (the name) is always available for free, but a *match* is worth paying for tier B.
        workspace.open(
            "Matched.mac".to_string(),
            "ROUTINE MatchedRoutine\n\n\tQuit\n\nFoo\t;\n\tQuit\n".to_string(),
        );

        let symbols = workspace.workspace_symbol("Matched".to_string());
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["MatchedRoutine", "Foo"]);
    }

    #[test]
    fn hover_member_on_a_call_shows_the_callee_not_the_caller() {
        let workspace = Workspace::new(None, None);
        let uri = "HoverCallee.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.HoverCallee {\nClassMethod Foo() {\n do ..Bar()\n}\nClassMethod Bar() {\n}\n}\n"
                .to_string(),
        );
        assert!(
            workspace.diagnostics(uri.clone()).is_empty(),
            "{:?}",
            workspace.diagnostics(uri.clone())
        );
        // Character 7 on line 2 (` do ..Bar()`) is on "Bar", inside `Foo`'s own body.
        let member = workspace
            .hover_member(
                uri,
                bindings::Position {
                    line: 2,
                    character: 7,
                },
            )
            .expect("expected a member at that position");
        assert_eq!(member.name.content, "Bar");
    }

    #[test]
    fn hover_member_returns_member_at_position() {
        let workspace = Workspace::new(None, None);
        let uri = "Hover.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Hover {\nClassMethod Foo() {\n}\n}\n".to_string(),
        );
        let member = workspace
            .hover_member(
                uri.clone(),
                bindings::Position {
                    line: 1,
                    character: 5,
                },
            )
            .expect("expected a member at that position");
        assert_eq!(member.name.content, "Foo");
        assert!(
            workspace
                .hover_member(
                    uri,
                    bindings::Position {
                        line: 0,
                        character: 0,
                    },
                )
                .is_none()
        );
    }

    #[test]
    fn hover_renders_member_as_markdown() {
        let workspace = Workspace::new(None, None);
        let uri = "Hover.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Hover {\nClassMethod Foo() {\n}\n}\n".to_string(),
        );
        let markdown = workspace
            .hover(
                uri,
                bindings::Position {
                    line: 1,
                    character: 5,
                },
            )
            .expect("expected hover text at that position");
        assert!(markdown.contains("ClassMethod Foo"));
    }

    #[test]
    fn hover_shows_both_type_and_macro_expansion_when_both_apply() {
        let workspace = Workspace::new(None, None);
        let uri = "MacroArg.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n#define X x\n\n\tSet x = 1\n\tWrite $$$X\n\tQuit\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri.clone());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        // Line 5 (`\tWrite $$$X`): a whole-token macro that expands to a bare variable read,
        // so both `hover_type` (for `x`) and `macro_expansion` (for `$$$X`) apply here.
        let position = bindings::Position {
            line: 5,
            character: 9,
        };
        assert!(workspace.hover_type(uri.clone(), position).is_some());
        assert!(workspace.macro_expansion(uri.clone(), position).is_some());

        let hover = workspace
            .hover(uri, position)
            .expect("expected hover text at that position");
        assert!(hover.contains("DataType"), "{hover}");
        assert!(hover.contains("Expands to"), "{hover}");
    }

    #[test]
    fn definition_resolves_member_reference() {
        let workspace = Workspace::new(None, None);
        let base_uri = "Base.cls".to_string();
        let sub_uri = "Sub.cls".to_string();
        workspace.open(
            base_uri.clone(),
            "Class Test.Base {\nClassMethod Foo() {\n}\n}\n".to_string(),
        );
        workspace.open(
            sub_uri.clone(),
            "Class Test.Sub {\nClassMethod Bar() {\n do ##class(Test.Base).Foo()\n}\n}\n"
                .to_string(),
        );

        let location = workspace
            .definition(
                sub_uri,
                bindings::Position {
                    line: 2,
                    character: 24,
                },
            )
            .expect("expected a definition at that position");
        assert_eq!(location.uri, base_uri);
        assert_eq!(location.range.start.line, 1);
    }

    #[test]
    fn row_instance_percent_accesses_property_like_instance_percent() {
        let workspace = Workspace::new(None, None);
        let uri = "RPercent.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.RPercent {\nProperty P1 As %Numeric;\nMethod Get() As %Numeric {\n return r%P1\n}\nClassMethod ClassGet() As %Numeric {\n return r%P1\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri.clone());
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("instance-only"),
            "{diagnostics:?}"
        );
        assert_eq!(diagnostics[0].range.start.line, 6);

        // `r%P1` inside the instance method (line 3) should resolve like `..P1`/`i%P1` —
        // go-to-definition finds the property declaration.
        let location = workspace
            .definition(
                uri,
                bindings::Position {
                    line: 3,
                    character: 10,
                },
            )
            .expect("expected a definition for `r%P1`");
        assert_eq!(location.range.start.line, 1);
    }

    #[test]
    fn definition_resolves_bare_class_reference() {
        let workspace = Workspace::new(None, None);
        let base_uri = "Base.cls".to_string();
        let sub_uri = "Sub.cls".to_string();
        workspace.open(
            base_uri.clone(),
            "Class Test.Base {\nClassMethod Foo() {\n}\n}\n".to_string(),
        );
        workspace.open(
            sub_uri.clone(),
            "Class Test.Sub {\nClassMethod Bar() {\n do ##class(Test.Base).Foo()\n}\n}\n"
                .to_string(),
        );

        // Position inside "Base", within the `##class(Test.Base)` literal itself.
        let location = workspace
            .definition(
                sub_uri,
                bindings::Position {
                    line: 2,
                    character: 18,
                },
            )
            .expect("expected a definition at that position");
        assert_eq!(location.uri, base_uri);
        assert_eq!(location.range.start.line, 0);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn definition_resolves_a_class_reference_via_the_iris_hosts_own_uri() {
        struct ExternalClassIris;
        impl crate::iris::IrisClient for ExternalClassIris {
            fn get_cls(&self, cls: &str) -> Option<bindings::ClassInfo> {
                let zero = bindings::Position {
                    line: 0,
                    character: 0,
                };
                (cls == "Test.External").then(|| bindings::ClassInfo {
                    doc: String::new(),
                    name: bindings::NameInfo {
                        before: zero,
                        content: cls.to_string(),
                        after: zero,
                    },
                    extends: vec![],
                    deprecated: false,
                    members: vec![],
                    property_class_names: vec![],
                    includes: vec![],
                    is_datatype: Some(false),
                    uri: Some("mock:///External.cls".to_string()),
                })
            }
            fn get_inc(&self, _name: &str) -> Option<bindings::RoutineSource> {
                None
            }
            fn get_int_or_mac(&self, _name: &str) -> Option<bindings::RoutineSource> {
                None
            }
        }
        let workspace = Workspace::new_with_iris(Some(Box::new(ExternalClassIris)), None);
        let uri = "Sub.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Sub {\nClassMethod Bar() {\n do ##class(Test.External).Foo()\n}\n}\n"
                .to_string(),
        );
        let location = workspace
            .definition(
                uri,
                bindings::Position {
                    line: 2,
                    character: 18,
                },
            )
            .expect("expected a definition at that position");
        assert_eq!(location.uri, "mock:///External.cls");
    }

    #[test]
    fn definition_resolves_same_routine_label_call() {
        let workspace = Workspace::new(None, None);
        let uri = "Test.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n\tQuit\n\nMain()\n do Sub()\n\tQuit\n\nSub()\n\tQuit\n".to_string(),
        );

        let location = workspace
            .definition(
                uri,
                bindings::Position {
                    line: 5,
                    character: 5,
                },
            )
            .expect("expected a definition at that position");
        assert_eq!(location.range.start.line, 8);
    }

    #[test]
    fn cross_routine_label_call_is_type_checked_against_the_callee() {
        let workspace = Workspace::new(None, None);
        workspace.open(
            "Callee.mac".to_string(),
            "ROUTINE Callee\n\n\tQuit\n\nFoo()\n\tQuit\n".to_string(),
        );

        let uri = "Main.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Main\n\n\tDo Foo^Callee()\n\tQuit\n".to_string(),
        );
        assert!(workspace.diagnostics(uri).is_empty());

        // `Foo` takes no arguments -- calling it with one should be flagged, exactly like a
        // same-routine label call would be.
        let uri = "TooMany.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE TooMany\n\n\tDo Foo^Callee(1)\n\tQuit\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("Too many arguments"));

        // No such label in `Callee`.
        let uri = "NoSuchLabel.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE NoSuchLabel\n\n\tDo Nope^Callee()\n\tQuit\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Cannot find label 'Nope' in routine 'Callee'")
        );

        // No such routine open in the workspace, and no IRIS connection to fall back to.
        let uri = "NoSuchRoutine.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE NoSuchRoutine\n\n\tDo Foo^Missing()\n\tQuit\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Routine 'Missing' is not defined in the current workspace")
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn cross_routine_label_call_resolves_via_iris_when_not_in_the_workspace() {
        struct ExternalRoutineIris;
        impl crate::iris::IrisClient for ExternalRoutineIris {
            fn get_cls(&self, _cls: &str) -> Option<bindings::ClassInfo> {
                None
            }
            fn get_inc(&self, _name: &str) -> Option<bindings::RoutineSource> {
                None
            }
            fn get_int_or_mac(&self, name: &str) -> Option<bindings::RoutineSource> {
                (name == "External").then(|| bindings::RoutineSource {
                    text: "ROUTINE External\n\n\tQuit\n\nFoo()\n\tQuit\n".to_string(),
                    uri: None,
                })
            }
        }
        let workspace = Workspace::new_with_iris(Some(Box::new(ExternalRoutineIris)), None);
        let uri = "Main.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Main\n\n\tDo Foo^External(1)\n\tQuit\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("Too many arguments"));
    }

    #[test]
    fn definition_resolves_cross_routine_label_call() {
        let workspace = Workspace::new(None, None);
        let callee_uri = "Callee.mac".to_string();
        workspace.open(
            callee_uri.clone(),
            "ROUTINE Callee\n\n\tQuit\n\nFoo()\n\tQuit\n".to_string(),
        );
        let uri = "Main.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Main\n\n\tDo Foo^Callee()\n\tQuit\n".to_string(),
        );

        let location = workspace
            .definition(
                uri,
                bindings::Position {
                    line: 2,
                    character: 5,
                },
            )
            .expect("expected a definition at that position");
        assert_eq!(location.uri, callee_uri);
        assert_eq!(location.range.start.line, 4);
    }

    #[test]
    fn goto_does_not_force_a_return_value_contract() {
        let workspace = Workspace::new(None, None);
        let uri = "Test.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n\tQuit\n\nfoo(cond)\t;\n\tif cond {\n\t\tg bar\n\t}\n\tquit\n\nbar\t;\n\tquit\n"
                .to_string(),
        );

        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn definition_resolves_goto_target() {
        let workspace = Workspace::new(None, None);
        let uri = "Test.mac".to_string();
        // No `Quit` before the `Goto` — a bare fallthrough would be flagged, and `where` would
        // be flagged as unread unless `Goto` (like `Do`) is treated as sharing scope with its
        // target rather than as this flow just ending on its own.
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n\tQuit\n\ntables\t;\n\tn where s where=1\n\tg tref\n\ntref\t;\n\tQuit\n"
                .to_string(),
        );

        let diagnostics = workspace.diagnostics(uri.clone());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        let location = workspace
            .definition(
                uri,
                bindings::Position {
                    line: 6,
                    character: 3,
                },
            )
            .expect("expected a definition at that position");
        assert_eq!(location.range.start.line, 8);
    }

    #[test]
    fn new_twice_in_a_procedure_block_is_reported_as_a_no_op_each_time() {
        let workspace = Workspace::new(None, None);
        let uri = "NewTest.cls".to_string();
        // A `ClassMethod`'s `{ }` body is a procedure block, where every local is already
        // implicitly `New`'d on entry — so both `New x` statements are no-ops, not just the
        // second, and neither is the old `RedundantNew` ("discards its current value") kind,
        // which only applies outside a procedure block.
        workspace.open(
            uri.clone(),
            "Class Test.NewTest {\nClassMethod Foo() {\n new x\n new x\n quit\n}\n}\n".to_string(),
        );

        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        for (diagnostic, line) in diagnostics.iter().zip([2, 3]) {
            assert!(diagnostic.message.contains("implicitly NEW'd"));
            assert_eq!(diagnostic.range.start.line, line);
        }
    }

    #[test]
    fn new_on_a_special_variable_in_a_procedure_block_is_not_reported() {
        let workspace = Workspace::new(None, None);
        let uri = "NewSpecialVar.cls".to_string();
        // `$Namespace` (and any other special variable) isn't a local binding subject to the
        // procedure block's implicit `New` -- `New $Namespace` is always a deliberate, legitimate
        // save/restore, twice included, never a redundant re-`New`.
        workspace.open(
            uri.clone(),
            "Class Test.NewSpecialVar {\nClassMethod Foo() {\n new $Namespace\n new $Namespace\n write $Namespace\n quit\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn new_on_a_parameter_in_a_procedure_block_is_not_reported() {
        let workspace = Workspace::new(None, None);
        let uri = "NewTest.cls".to_string();
        // Unlike an ordinary local, re-`New`-ing a by-value parameter is a legitimate, common
        // pattern (discarding the caller's argument, forcing a fresh assignment before reuse),
        // not a no-op worth flagging -- see objectscript-typecheck-corpus's New.cls.
        workspace.open(
            uri.clone(),
            "Class Test.NewTest {\nClassMethod Foo(x) {\n new x\n quit\n}\n}\n".to_string(),
        );

        let diagnostics = workspace.diagnostics(uri);
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.message.contains("implicitly NEW'd")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn new_twice_outside_a_procedure_block_is_still_reported_as_redundant() {
        let workspace = Workspace::new(None, None);
        let uri = "NewTest.mac".to_string();
        // A label-based routine body isn't a procedure block -- `New` is the only thing that
        // scopes a variable there, so a second `New` on the same name is genuinely redundant
        // (the old `RedundantNew` diagnostic), not the procedure-block no-op case above.
        workspace.open(
            uri.clone(),
            "ROUTINE NewTest\n\n\tNew x\n\tNew x\n\tQuit\n".to_string(),
        );

        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("discarding its current value")
        );
        assert_eq!(diagnostics[0].range.start.line, 3);
        assert_eq!(
            diagnostics[0].related_information[0]
                .location
                .range
                .start
                .line,
            2
        );
    }

    #[test]
    fn definition_resolves_label_after_include() {
        let workspace = Workspace::new(None, None);
        let inc_uri = "Shared.inc".to_string();
        let mac_uri = "Test.mac".to_string();
        workspace.open(
            inc_uri,
            "ROUTINE Shared [Type=INC]\n// one\n// two\n// three\n".to_string(),
        );
        workspace.open(
            mac_uri.clone(),
            "ROUTINE Test\n\n#include Shared\n\tQuit\n\nMain()\n do Sub()\n\tQuit\n\nSub()\n\tQuit\n"
                .to_string(),
        );

        let diagnostics = workspace.diagnostics(mac_uri.clone());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        let location = workspace
            .definition(
                mac_uri.clone(),
                bindings::Position {
                    line: 6,
                    character: 5,
                },
            )
            .expect("expected a definition at that position");
        assert_eq!(location.uri, mac_uri);
        assert_eq!(location.range.start.line, 9);
    }

    #[test]
    fn references_finds_all_call_sites_from_either_end() {
        let workspace = Workspace::new(None, None);
        let base_uri = "Base.cls".to_string();
        let sub_uri = "Sub.cls".to_string();
        workspace.open(
            base_uri.clone(),
            "Class Test.Base {\nClassMethod Foo() {\n}\n}\n".to_string(),
        );
        workspace.open(
            sub_uri.clone(),
            "Class Test.Sub {\nClassMethod Bar() {\n do ##class(Test.Base).Foo()\n do ##class(Test.Base).Foo()\n}\n}\n"
                .to_string(),
        );

        let call_site = bindings::Position {
            line: 2,
            character: 24,
        };
        let definition = workspace
            .definition(sub_uri.clone(), call_site)
            .expect("expected a definition at that position");

        let from_call_site = workspace.references(sub_uri.clone(), call_site, true);
        assert_eq!(from_call_site.len(), 3, "{from_call_site:?}");
        assert!(from_call_site.contains(&definition));
        assert_eq!(
            from_call_site
                .iter()
                .filter(|location| location.uri == sub_uri)
                .count(),
            2
        );

        // Invoking from the declaration site itself finds the exact same set.
        let from_declaration =
            workspace.references(definition.uri.clone(), definition.range.start, true);
        let mut a = from_call_site;
        let mut b = from_declaration;
        a.sort_by_key(|l| (l.uri.clone(), l.range.start.line, l.range.start.character));
        b.sort_by_key(|l| (l.uri.clone(), l.range.start.line, l.range.start.character));
        assert_eq!(a, b);

        // Excluding the declaration drops exactly the one entry for it.
        let without_declaration = workspace.references(sub_uri, call_site, false);
        assert_eq!(without_declaration.len(), 2, "{without_declaration:?}");
        assert!(!without_declaration.contains(&definition));
    }

    #[test]
    fn definition_resolves_self_member_reference() {
        let workspace = Workspace::new(None, None);
        let uri = "Sub.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Sub {\nClassMethod Bar() {\n do ..Baz()\n}\nClassMethod Baz() {\n quit\n}\n}\n"
                .to_string(),
        );

        let location = workspace
            .definition(
                uri,
                bindings::Position {
                    line: 2,
                    character: 7,
                },
            )
            .expect("expected a definition at that position");
        assert_eq!(location.range.start.line, 4);
    }

    #[test]
    fn bare_throw_is_allowed() {
        let workspace = Workspace::new(None, None);
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nMethod M() {\n throw\n}\n}\n".to_string(),
        );
        assert!(workspace.diagnostics(uri).is_empty());
    }

    #[test]
    fn throw_expression_is_type_checked() {
        let workspace = Workspace::new(None, None);
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nMethod M() {\n throw undefinedVar\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("undefinedVar"));
    }

    #[test]
    fn code_after_throw_is_unreachable() {
        let workspace = Workspace::new(None, None);
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nMethod M() {\n throw\n quit\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("Unreachable"));
        assert_eq!(diagnostics[0].range.start.line, 3);
    }

    #[test]
    fn inherited_parameter_resolves_through_a_workspace_superclass() {
        let workspace = Workspace::new(None, None);
        workspace.open(
            "Base.cls".to_string(),
            "Class Test.Base Extends %RegisteredObject {\nParameter P = 1;\n}\n".to_string(),
        );
        let uri = "Sub.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Sub Extends Test.Base {\nClassMethod M() {\n write ..#P\n quit\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn a_failing_loop_body_inside_an_if_keeps_the_rest_checked() {
        let workspace = Workspace::new(None, None);
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nClassMethod Two(a, b) As %Integer {\n quit a+b\n}\nClassMethod M(x) {\n if x {\n  for i=1:1:3 {\n   do ..Two(i)\n  }\n }\n write undefinedVar\n quit\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("Too few arguments"))
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("undefinedVar"))
        );
    }

    #[test]
    fn a_failing_branch_does_not_make_what_follows_unreachable() {
        let workspace = Workspace::new(None, None);
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nClassMethod M(x) As %Integer {\n set t = 1\n if x {\n  write $$$NoSuchMacro\n  write t\n } else {\n  write $$$NoSuchMacro\n }\n quit 1\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert!(
            diagnostics
                .iter()
                .all(|d| d.message.contains("NoSuchMacro"))
        );
    }

    #[test]
    fn diagnostics_inside_a_loop_body_are_reported_once() {
        let workspace = Workspace::new(None, None);
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nClassMethod Two(a, b) As %Integer {\n quit a+b\n}\nClassMethod M() {\n for i=1:1:3 {\n  if i {\n   do ..Two(i)\n  }\n }\n quit\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("Too few arguments"));
    }

    #[test]
    fn byref_arguments_are_still_required() {
        let workspace = Workspace::new(None, None);
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nClassMethod F(a, ByRef b) As %Integer {\n quit a+$get(b)\n}\nClassMethod M() {\n write ..F(1)\n quit\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("'b' is required"));
    }

    #[test]
    fn unused_parameters_of_an_overriding_method_are_not_flagged() {
        let workspace = Workspace::new(None, None);
        workspace.open(
            "Base.cls".to_string(),
            "Class Test.Base Extends %RegisteredObject {\nMethod M(x As %String, Output y) As %Status {\n set y = x\n quit 1\n}\n}\n".to_string(),
        );
        let uri = "Sub.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Sub Extends Test.Base {\nMethod M(x As %String, Output y) As %Status {\n quit 1\n}\nMethod N(z As %String) As %Status {\n quit 1\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("'z'"));
    }

    #[test]
    fn system_include_macros_are_available_without_an_include() {
        let workspace = Workspace::new(None, None);
        workspace.open(
            "%systemInclude.inc".to_string(),
            "ROUTINE %systemInclude [Type=INC]\n#include %occInclude\n".to_string(),
        );
        workspace.open(
            "%occInclude.inc".to_string(),
            "ROUTINE %occInclude [Type=INC]\n#define AppTypePrivRoutine 4\n".to_string(),
        );
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nClassMethod M() As %Integer {\n quit $$$AppTypePrivRoutine\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn a_superclass_include_is_inherited() {
        let workspace = Workspace::new(None, None);
        workspace.open(
            "Foo.inc".to_string(),
            "ROUTINE Foo [Type=INC]\n#define FooMacro 1\n".to_string(),
        );
        workspace.open(
            "Base.cls".to_string(),
            "Include Foo\n\nClass Test.Base Extends %RegisteredObject {\n}\n".to_string(),
        );
        let uri = "Sub.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Sub Extends Test.Base {\nClassMethod M() As %Integer {\n quit $$$FooMacro\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn nested_parameterized_macros_expand() {
        let workspace = Workspace::new(None, None);
        let uri = "Nested.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Nested\n#define B(%x) (%x+1)\n#define A(%x) $$$B(%x)\n#define R(%c,%k) ^G(%c,%k)\n#define I(%ref,%c) %ref(%c,1)\n\twrite $$$A(1)\n\twrite $$$I($$$R,\"a\")\n\tquit\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn def1arg_binds_the_whole_argument_text() {
        let workspace = Workspace::new(None, None);
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nClassMethod M() As %List {\n#def1arg P(%args) $listbuild(%args)\n#def1arg E(%a) ##safeexpression(1)\n write $$$E(1)\n quit $$$P(1,2,3)\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn lowercase_continue_joins_macro_lines() {
        let workspace = Workspace::new(None, None);
        let uri = "Cont.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Cont\n#define M(%a) ##continue\n set %a=1 ##continue\n write %a\n\t$$$M(v)\n\tquit\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn object_handle_parameter_stands_for_the_class_itself() {
        // The mock resolves %RegisteredObject (with no members), so a miss is a hard error.
        let workspace = Workspace::new_with_iris(
            Some(Box::new(MockIrisClient {
                is_datatype: None,
                mem: None,
            })),
            None,
        );
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nProperty P As %String;\nClassMethod M(obj As %ObjectHandle) {\n set obj.P = 1\n write obj.Q\n quit\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Cannot find member 'Q' in Test.T"),
            "{diagnostics:?}"
        );
    }

    // Whether the levels agree depends on the run-time conditions, so they aren't tracked.
    #[test]
    fn transaction_levels_are_not_checked() {
        let workspace = Workspace::new(None, None);
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nClassMethod M(x) {\n if x {\n  tstart\n }\n if x {\n  tcommit\n }\n write undefinedVar\n quit\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("undefinedVar"));
    }

    #[test]
    fn quit_inside_try_exits_the_block() {
        let workspace = Workspace::new(None, None);
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nClassMethod M(x) As %Status {\n set sc = 1\n try {\n  if x quit\n  set sc = 2\n } catch e {\n  set sc = e.AsStatus()\n }\n quit sc\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn argumented_quit_inside_try_is_an_error() {
        let workspace = Workspace::new(None, None);
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nClassMethod M() As %Integer {\n try {\n  quit 1\n } catch e {\n  write e\n }\n quit 2\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("Quit with"));
    }

    #[test]
    fn null_oref_is_assignable_to_an_object_variable() {
        let workspace = Workspace::new(None, None);
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nClassMethod M(Output obj As Test.T) As %Status {\n set obj = \"\"\n quit 1\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn not_procedure_block_methods_may_new_their_locals() {
        let workspace = Workspace::new(None, None);
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T [ Not ProcedureBlock ] {\nClassMethod M() {\n new x\n set x = 1\n write x\n quit\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        let uri = "T2.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T2 {\nClassMethod M() {\n new x\n set x = 1\n write x\n quit\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("already NEW'd"));
    }

    #[test]
    fn a_trailing_comment_may_contain_a_quote() {
        let workspace = Workspace::new(None, None);
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nClassMethod M() {\n set x=1 ;double-quote \"\n write x\n quit\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn external_method(
        name: &str,
        normal: Vec<bindings::NormalArg>,
        t: Option<&str>,
    ) -> bindings::MemberInfo {
        let zero = bindings::Position {
            line: 0,
            character: 0,
        };
        bindings::MemberInfo {
            doc: String::new(),
            before: zero,
            name: bindings::NameInfo {
                before: zero,
                content: name.to_string(),
                after: zero,
            },
            aliases: vec![],
            deprecated: false,
            kind: bindings::MemberKind::Method(bindings::MethodInfo {
                normal,
                variadic: None,
                t: t.map(str::to_string),
                procedure_block: None,
                body: bindings::Range {
                    start: zero,
                    end: zero,
                },
            }),
            after: zero,
            uri: None,
            origin: None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn external_arg(name: &str, default: Option<&str>) -> bindings::NormalArg {
        bindings::NormalArg {
            mode: bindings::ArgMode::Default,
            name: name.to_string(),
            t: None,
            default: default.map(str::to_string),
            declared_at: None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn diagnostics_with_external_member(
        mem: bindings::MemberInfo,
        body: &str,
    ) -> Vec<bindings::Diagnostic> {
        let workspace = Workspace::new_with_iris(
            Some(Box::new(MockIrisClient {
                is_datatype: None,
                mem: Some(mem),
            })),
            None,
        );
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            format!(
                "Class Test.T Extends %RegisteredObject {{\nMethod M() {{\n{body}\n quit\n}}\n}}\n"
            ),
        );
        workspace.diagnostics(uri)
    }

    #[test]
    fn a_trailing_comment_on_a_preprocessor_condition_is_ignored() {
        let workspace = Workspace::new(None, None);
        let uri = "Cond.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Cond\n#if 0 /*=== Maintenance ===*/\n\twrite undefinedVar\n#endif\n\tquit\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn a_line_comment_start_inside_a_range_comment_is_not_a_line_comment() {
        let workspace = Workspace::new(None, None);
        let uri = "example.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE example\nf()\n    /* // */\n    /* ; */\n    quit\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    fn style_messages(workspace: &Workspace, uri: &str) -> Vec<String> {
        let mut messages: Vec<_> = workspace
            .diagnostics(uri.to_string())
            .into_iter()
            .map(|d| {
                format!(
                    "{}:{} {}",
                    d.range.start.line, d.range.start.character, d.message
                )
            })
            .collect();
        messages.sort();
        messages
    }

    #[test]
    fn command_keywords_follow_the_configured_style() {
        let style = style::parse(r#"{"command": {"form": "Long", "case": "Pascal"}}"#).unwrap();
        let workspace = Workspace::new(None, Some(style));
        let uri = "Style.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Style\nf(x)\n s y=x\n SET z=y\n Set x=z\n if x { w 1 } elseif y { W 2 } else { write 3 }\n try { do { s a=1 } WHILE x } catch e { w e }\n h\n".to_string(),
        );
        assert_eq!(
            style_messages(&workspace, &uri),
            [
                "2:1 Command keyword `s` should be written `Set`",
                "3:1 Command keyword `SET` should be written `Set`",
                "5:1 Command keyword `if` should be written `If`",
                "5:14 Command keyword `elseif` should be written `ElseIf`",
                "5:25 Command keyword `W` should be written `Write`",
                "5:31 Command keyword `else` should be written `Else`",
                "5:38 Command keyword `write` should be written `Write`",
                "5:8 Command keyword `w` should be written `Write`",
                "6:1 Command keyword `try` should be written `Try`",
                "6:12 Command keyword `s` should be written `Set`",
                "6:20 Command keyword `WHILE` should be written `While`",
                "6:30 Command keyword `catch` should be written `Catch`",
                "6:40 Command keyword `w` should be written `Write`",
                "6:7 Command keyword `do` should be written `Do`",
                "7:1 Command keyword `h` should be written `Halt`",
            ]
        );
    }

    #[test]
    fn functions_follow_the_configured_style() {
        let style = style::parse(
            r#"{"function": {"form": "Long", "case": "Pascal"}, "function_listbuild": {"form": "Short"}}"#,
        )
        .unwrap();
        let workspace = Workspace::new(None, Some(style));
        let uri = "F.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE F\nf(x)\n s y=$p(x,\",\")_$Length(x)\n if x { w $LISTBUILD(1),\"$e(x)\" }\n q\n".to_string(),
        );
        assert_eq!(
            style_messages(&workspace, &uri),
            [
                "2:6 Function `$p` should be written `$Piece`",
                "3:11 Function `$LISTBUILD` should be written `$LB`",
            ]
        );
    }

    #[test]
    fn set_targets_follow_the_configured_function_style() {
        let style = style::parse(r#"{"function": {"form": "Long", "case": "Pascal"}}"#).unwrap();
        let workspace = Workspace::new(None, Some(style));
        let uri = "S.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE S\nf(x,y)\n s $p(x,\",\",2)=1\n set $E(x,1,2)=\"ab\",$li(y,1)=3\n s ($P(x,1),$extract(y))=0\n s $bit(x,1)=1,$ZTRAP=\"err\",$x=0\n s:x $WE(y,1)=$wextract(x)\n q\n".to_string(),
        );
        let found: Vec<_> = style_messages(&workspace, &uri)
            .into_iter()
            .filter(|m| m.contains("Function"))
            .collect();
        assert_eq!(
            found,
            [
                "2:4 Function `$p` should be written `$Piece`",
                "3:21 Function `$li` should be written `$List`",
                "3:6 Function `$E` should be written `$Extract`",
                "4:13 Function `$extract` should be written `$Extract`",
                "4:5 Function `$P` should be written `$Piece`",
                "5:4 Function `$bit` should be written `$Bit`",
                "6:15 Function `$wextract` should be written `$WExtract`",
                "6:6 Function `$WE` should be written `$WExtract`",
            ]
        );
    }

    #[test]
    fn expression_methods_follow_the_configured_function_style() {
        let style = style::parse(r#"{"function": {"case": "Upper"}}"#).unwrap();
        let workspace = Workspace::new(None, Some(style));
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nClassMethod M(x) As %String [ CodeMode = expression ] {\n$Piece(x,\",\")\n}\n}\n".to_string(),
        );
        assert_eq!(
            style_messages(&workspace, &uri),
            ["2:1 Function `$Piece` should be written `$PIECE`"]
        );
    }

    #[test]
    fn method_body_commands_follow_the_configured_style() {
        let style = style::parse(
            r#"{"command": {"form": "Short"}, "command_quit": {"form": "Long", "case": "Upper"}}"#,
        )
        .unwrap();
        let workspace = Workspace::new(None, Some(style));
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nClassMethod M() {\n Set x=1\n Write x\n Quit\n}\n}\n".to_string(),
        );
        assert_eq!(
            style_messages(&workspace, &uri),
            [
                "2:1 Command keyword `Set` should be written `S`",
                "3:1 Command keyword `Write` should be written `W`",
                "4:1 Command keyword `Quit` should be written `QUIT`",
            ]
        );
    }

    #[test]
    fn a_failed_branch_ending_in_a_return_still_ends_its_path() {
        let workspace = Workspace::new(None, None);
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nClassMethod M(x) As %Integer {\n if x {\n  return 3\n } else {\n  return undefinedVar\n }\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("undefined"));
        assert_eq!(diagnostics[0].range.start.line, 5);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_collection_property_resolves_to_its_collection_class() {
        let mem = external_method("Count", vec![], Some("%Integer"));
        let workspace = Workspace::new_with_iris(
            Some(Box::new(MockIrisClient {
                is_datatype: None,
                mem: Some(mem),
            })),
            None,
        );
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T Extends %RegisteredObject {\nProperty L As list Of %String;\nMethod M() {\n write ..L.Count()\n quit\n}\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    // `..Open()` parses as one empty argument; it must count as none, so `id` is reported.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn an_empty_argument_list_is_no_arguments() {
        let mem = external_method(
            "Open",
            vec![
                external_arg("id", None),
                external_arg("concurrency", Some("-1")),
            ],
            None,
        );
        let diagnostics = diagnostics_with_external_member(mem, " do ..Open()");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("Too few arguments"));
    }

    #[test]
    fn documented_legacy_and_bit_functions_parse() {
        let workspace = Workspace::new(None, None);
        let uri = "Fns.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Fns\n\tSet b=$ZBITSTR(8,1),b=$ZBITSET(b,2,0),n=$ZBITCOUNT($ZBITAND(b,$ZBITNOT(b)))+$ZBITLEN(b)+$ZBITFIND(b,1)+$ZBITFIND(b,0,3)\n\tWrite $ISO(b),$LOCK(\"^x\"),$LOCK(\"^x\",\"OWNER\"),$NEXT(^G(\"\")),$N(^G(\"\")),$ZORDER(^G(1)),$ZO(^G(1)),$ZPREVIOUS(^G(\"\")),n,$ZBITXOR(b,$ZBITOR(b,b))\n\tQuit\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    // Every diagnostic carries its kind's code and a link to that kind's docs, including the ones
    // raised outside the checker proper, such as the unconfirmed-class hint.
    #[test]
    fn diagnostics_carry_their_kinds_code_and_docs_link() {
        let diagnostics = class_diagnostics(
            "Property P As Other.Missing;\nClassMethod M() {\n Write $$$NoSuchMacro\n}\n",
        );
        let find = |text: &str| {
            diagnostics
                .iter()
                .find(|d| d.message.contains(text))
                .unwrap_or_else(|| panic!("{diagnostics:?}"))
        };
        let cases = [
            (
                find("NoSuchMacro"),
                check::TypeErrorKind::UndefinedMacro {
                    name: String::new(),
                    suggestions: check::Suggestions(vec![]),
                },
                "UndefinedMacro",
            ),
            (
                find("Cannot confirm class"),
                check::TypeErrorKind::UnconfirmedClass {
                    lookup: check::ClassLookup {
                        cls: String::new(),
                        has_iris: false,
                    },
                },
                "UnconfirmedClass",
            ),
        ];
        for (diagnostic, kind, variant) in cases {
            assert_eq!(diagnostic.code, kind.code());
            assert_eq!(
                diagnostic.code_description,
                format!(
                    "https://docs.rs/ascot/{}/ascot/check/enum.TypeErrorKind.html#variant.{variant}",
                    env!("CARGO_PKG_VERSION")
                )
            );
        }
    }

    fn class_diagnostics(body: &str) -> Vec<bindings::Diagnostic> {
        let workspace = Workspace::new(None, None);
        let uri = "F.cls".to_string();
        workspace.open(
            uri.clone(),
            format!("Class Test.F Extends %RegisteredObject {{\n{body}\n}}\n"),
        );
        workspace.diagnostics(uri)
    }

    // `'$DATA(x, t)` refines exactly like `$DATA(x, t)` with the branches swapped, and the
    // postconditional / `&&` forms refine like a plain `If`.
    #[test]
    fn data_refinement_survives_negation_postconditionals_and_conjunctions() {
        let diagnostics = class_diagnostics(
            "ClassMethod Neg() As %String {\n Set:$R(2) x = 1\n If '$D(x,t) Quit \"\"\n Quit t\n}\n\
             ClassMethod NegGlobal() As %String {\n If '($D(^G(\"a\"),v)#2) Quit \"\"\n Quit v\n}\n\
             ClassMethod PC() As %String {\n Set out = \"\"\n For i=1:1:3 {\n  Continue:'($d(^G(i),tItem)#2)\n  Set out = out_tItem\n }\n Quit out\n}\n\
             ClassMethod And(a As %String) As %String {\n Set:$R(2) x = 1\n If (a=\"\")&&$D(x,t) {\n  Set a = t\n }\n Quit a\n}\n",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn is_object_of_get_proves_the_variable_defined() {
        let diagnostics = class_diagnostics(
            "ClassMethod M() As %String {\n If $IsObject($Get(obj)) Quit obj.Name\n Quit \"\"\n}\n",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn variadic_parameter_subscripts_are_readable() {
        let diagnostics = class_diagnostics(
            "ClassMethod M(args...) As %Integer {\n For i=1:1:args {\n  If $IsObject($Get(args(i))) Write args(i)\n }\n Quit 1\n}\n",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn throwing_macros_end_the_path() {
        let diagnostics = class_diagnostics(
            "ClassMethod A(sc As %Status) As %Status {\n #define ThrowStatus(%sc) Throw ##class(%Exception.StatusException).CreateFromStatus(%sc)\n $$$ThrowStatus(sc)\n}\n\
             ClassMethod B() As %Status {\n #define ThrowSQLCODE(%c,%m) Throw ##class(%Exception.SQL).CreateFromSQLCODE(%c,%m)\n $$$ThrowSQLCODE(-400,\"bad\")\n}\n",
        );
        let diagnostics = without_hints(diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn system_process_throw_error_never_returns() {
        for call in [
            " do ##class(%SYSTEM.Process).ThrowError(149)",
            " do $SYSTEM.Process.ThrowError(149)",
        ] {
            let mut mem = external_method("ThrowError", vec![external_arg("n", None)], None);
            if let bindings::MemberKind::Method(info) = mem.kind {
                mem.kind = bindings::MemberKind::ClassMethod(info);
            }
            let diagnostics = diagnostics_with_external_member(mem, call);
            assert!(
                diagnostics
                    .iter()
                    .any(|d| d.message.contains("Unreachable")),
                "{call}: {diagnostics:?}"
            );
        }
    }

    #[test]
    fn labels_without_commands_need_no_quit() {
        let workspace = Workspace::new(None, None);
        let uri = "Alias.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Alias\n\tQuit\nOld ; 11/02/4 history\n\t; more history\nReal\n\tWrite 1\n\tQuit\nLast ; nothing after\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    // Pending writes are tracked as a set: both sides of an `If` merge usually carry the same
    // ones, and a list doubling on every merge under nested loops is exponential (#42).
    #[test]
    fn nested_loops_with_many_branches_finish() {
        let ifs: String = (0..6)
            .map(|i| format!("    If x {{ Set v{i} = {i} }}\n"))
            .collect();
        let body = format!(
            " Set a = 1, b = 2, c = 3\n Set i = $O(^g(\"\"))\n While i '= \"\" {{\n  Set j = $O(^g(i,\"\"))\n  While j '= \"\" {{\n   Set k = $O(^g(i,j,\"\"))\n   While k '= \"\" {{\n{ifs}    Set k = $O(^g(i,j,k))\n   }}\n   Set j = $O(^g(i,j))\n  }}\n  Set i = $O(^g(i))\n }}\n Quit a+b+c"
        );
        let diagnostics =
            class_diagnostics(&format!("ClassMethod M(x) As %Integer {{\n{body}\n}}\n"));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    fn diagnostics_for(files: &[(&str, &str)]) -> Vec<bindings::Diagnostic> {
        let workspace = Workspace::new(None, None);
        for (uri, text) in files {
            workspace.open(uri.to_string(), text.to_string());
        }
        workspace.diagnostics(files[0].0.to_string())
    }

    // Without an IRIS connection, members of system classes only get "cannot confirm" hints.
    fn without_hints(diagnostics: Vec<bindings::Diagnostic>) -> Vec<bindings::Diagnostic> {
        diagnostics
            .into_iter()
            .filter(|d| d.severity != bindings::DiagnosticSeverity::Hint)
            .collect()
    }

    #[test]
    fn dim_default_null_oref_and_dim_in_one_branch() {
        let diagnostics = class_diagnostics(
            "ClassMethod M(x) As %String {\n #dim cDef As %Dictionary.CompiledClass = \"\"\n If x {\n  #dim it = x\n } ElseIf x = 2 {\n  #dim it = 2\n }\n Quit $Get(it)_$IsObject(cDef)\n}\nClassMethod N() As %RegisteredObject {\n Quit \"\"\n}\n",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn expanded_argument_covers_the_remaining_parameters() {
        let diagnostics = class_diagnostics(
            "ClassMethod Spread(a, b, c) As %Status {\n Quit a+b+c\n}\nClassMethod Caller(args...) As %Status {\n Set sc = ..Spread(1, args...)\n Set sc = sc + ..Spread(args...)\n Quit sc + ..Spread(1, 2, args...)\n}\n",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn multidimensional_property_nodes_are_not_calls() {
        let diagnostics = class_diagnostics(
            "Property Symbols As %String [ MultiDimensional ];\nProperty Plain As %String;\nMethod M(k) As %String {\n If $Data(..Symbols(k),sym)#2 Quit sym\n Quit ..Plain(1)\n}\n",
        );
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("not a procedure"),
            "{diagnostics:?}"
        );
    }

    // Unqualified class names resolve against the class's own package, its `Import`s, then `User`.
    #[test]
    fn unqualified_class_names_resolve_in_the_same_package() {
        let diagnostics = diagnostics_for(&[
            (
                "P.cls",
                "Import Other\nClass Pkg.Sub Extends Base {\nProperty Ref As Helper;\nMethod M() As %String {\n Set h = ##class(Helper).%New()\n Quit ..Base()_h.Name_..Ref.Name_##class(Far).X()\n}\n}\n",
            ),
            (
                "B.cls",
                "Class Pkg.Base Extends %RegisteredObject {\nMethod Base() As %String {\n Quit \"b\"\n}\n}\n",
            ),
            (
                "H.cls",
                "Class Pkg.Helper Extends %RegisteredObject {\nProperty Name As %String;\n}\n",
            ),
            (
                "F.cls",
                "Class Other.Far Extends %RegisteredObject {\nClassMethod X() As %String {\n Quit \"x\"\n}\n}\n",
            ),
        ]);
        let diagnostics = without_hints(diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    // Parsed, but reported by the checker.
    #[test]
    fn empty_class_keyword_value_is_reported() {
        let diagnostics = diagnostics_for(&[(
            "E.cls",
            "Class Test.E Extends %RegisteredObject [ CompileAfter = , System = 4 ]\n{\n}\n",
        )]);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Class keyword 'CompileAfter' has no value"),
            "{diagnostics:?}"
        );
        // Flagged at the keyword, not at the class name.
        let header = "Class Test.E Extends %RegisteredObject [ CompileAfter = , System = 4 ]";
        let start = header.find("CompileAfter").unwrap() as u32;
        assert_eq!(diagnostics[0].range.start.line, 0);
        assert_eq!(diagnostics[0].range.start.character, start);
        assert_eq!(
            diagnostics[0].range.end.character,
            start + "CompileAfter".len() as u32
        );
    }

    // With IRIS to ask, a `##class(...)` naming no class is reported at that expression, not
    // at whatever member is then accessed on it.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn missing_class_is_reported_at_the_class_expression() {
        let workspace = Workspace::new_with_iris(Some(Box::new(OccErrorsIris)), None);
        let uri = "M.cls".to_string();
        let line = " quit ##class(No.Such.Class).Method()";
        workspace.open(
            uri.clone(),
            format!("Class Test.M {{\nClassMethod M() As %Status {{\n{line}\n}}\n}}\n"),
        );
        let diagnostics = without_hints(workspace.diagnostics(uri));
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Cannot find class 'No.Such.Class'"),
            "{diagnostics:?}"
        );
        let start = line.find("##class").unwrap() as u32;
        assert_eq!(diagnostics[0].range.start.line, 2);
        assert_eq!(diagnostics[0].range.start.character, start);
        assert_eq!(
            diagnostics[0].range.end.character,
            start + "##class(No.Such.Class)".len() as u32
        );
    }

    #[test]
    fn argumentless_if_and_new_of_percent_variable() {
        let diagnostics = class_diagnostics(
            "ClassMethod M(x) As %String {\n New %pct\n Set %pct = 1\n i x=1\n i  s %pct = 2\n e  s %pct = 3\n Quit %pct\n}\n",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    // IRIS keeps a method body's `#define` in effect for the methods after it; that's an
    // anti-pattern ascot doesn't follow -- the later use is an undefined macro.
    #[test]
    fn method_body_defines_do_not_leak_into_later_methods() {
        let diagnostics = class_diagnostics(
            "ClassMethod A() As %String {\n #define REPORT(%x) Write %x\n $$$REPORT(1)\n Quit \"\"\n}\nClassMethod B() As %String {\n $$$REPORT(2)\n Quit \"\"\n}\n",
        );
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("macro 'REPORT' is undefined"),
            "{diagnostics:?}"
        );
        assert_eq!(diagnostics[0].range.start.line, 7, "{diagnostics:?}");
    }

    // A generator method's body writes code; its parameters belong to the generated method.
    #[test]
    fn generator_method_parameters_are_not_unread() {
        let diagnostics = class_diagnostics(
            "ClassMethod G(pFlag As %Integer = 0, Output pOut) As %Status [ CodeMode = generator ] {\n Do %code.WriteLine(\" Set pOut = pFlag\")\n Quit 1\n}\n",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    // `%`-variables are process-wide, of no known type or definedness.
    fn percent_variables_are_public() {
        let diagnostics = class_diagnostics(
            "Property Name As %String;\nMethod M() As %String {\n Set me = $Get(%this)\n Set %leaked = 1\n Quit me.Name_%this.Name_%request.Data\n}\n",
        );
        let diagnostics = without_hints(diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    // `list Of X(...)`'s type parameters are X's; an index may share a property's name (a
    // method may not: properties and methods are one namespace); a subscripted Set on an Output
    // parameter assigns it; a `many` relationship is a collection.
    #[test]
    fn collection_parameters_shared_member_names_and_output_subscripts() {
        let diagnostics = diagnostics_for(&[
            (
                "C.cls",
                "Class Test.C Extends %RegisteredObject {\nProperty Names As list Of %String(MAXLEN = 10);\nProperty Flag As %Boolean;\nIndex Flag On Flag;\nRelationship Kids As Test.Kid [ Cardinality = many, Inverse = Parent ];\nClassMethod Fill(Output pOut) As %Integer {\n Set pOut(1) = 1\n Quit 1\n}\nMethod M() As %Integer {\n Quit ..Kids.Count()_..Flag\n}\n}\n",
            ),
            (
                "K.cls",
                "Class Test.Kid Extends %RegisteredObject {\nRelationship Parent As Test.C [ Cardinality = one, Inverse = Kids ];\n}\n",
            ),
        ]);
        let diagnostics = without_hints(diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn piece_set_targets_define_the_string() {
        let diagnostics = class_diagnostics(
            "ClassMethod M() As %String {\n Set $Piece(out, \",\", 2) = \"b\"\n Set $Extract(more, 1) = \"m\"\n Quit out_more_$ZINCR(^c)_$MVV(2)\n}\n",
        );
        let diagnostics = without_hints(diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    // `list Of`/`array Of` properties are collections: element-typed methods are checked against
    // the element, the rest resolve on the collection class.
    #[test]
    fn collection_methods_are_typed_by_the_element() {
        let diagnostics = diagnostics_for(&[
            (
                "C.cls",
                "Class Test.C Extends %RegisteredObject {\nProperty Kids As list Of Test.Kid;\nProperty Names As array Of %String;\nMethod M(obj As Test.C) As %Integer {\n Set sc = ..Kids.Insert(..Names.GetAt(\"a\"))\n Set kid = obj.Kids.GetAt(1)\n Set sc = ..Names.SetAt(kid.Parent, \"k\")\n Quit ..Kids.Count()_..Names.Count()_sc\n}\n}\n",
            ),
            (
                "K.cls",
                "Class Test.Kid Extends %RegisteredObject {\nProperty Parent As %String;\n}\n",
            ),
        ]);
        let diagnostics = without_hints(diagnostics);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].range.start.line, 4, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("Test.Kid"),
            "{diagnostics:?}"
        );
    }

    // `..X` looks up plain properties, `..X(...)` methods and multidimensional properties: a
    // same-named pair across the two is fine (IRIS compiles it), within one it is a conflict.
    #[test]
    fn member_namespaces_follow_the_access_syntax() {
        let diagnostics = class_diagnostics(
            "Property Flag As %Boolean;\nMethod Flag() As %Integer {\n Quit ..Flag\n}\nProperty Data As %String [ MultiDimensional ];\nMethod Data(k) As %String {\n Quit ..Data(k)\n}\nProperty Dup As %String;\nProperty Dup As %Integer;\nProperty Root As %String [ MultiDimensional ];\nProperty Root As %Integer;\nMethod M(k) As %String {\n Quit ..Flag()_..Flag_..Data(k)_..Root_..Root(k)\n}\n",
        );
        let lines: Vec<u32> = diagnostics.iter().map(|d| d.range.start.line).collect();
        assert_eq!(lines, [6, 10, 12], "{diagnostics:?}");
    }

    #[test]
    fn legacy_commands_and_embedded_sql_are_checked() {
        let diagnostics = class_diagnostics(
            "Method Abs() As %Status [ Abstract ]\n{\n}\nClassMethod Io(dev, name) As %String\n{\n Open dev:(\"R\"):5 Use dev:(/IOT=\"UTF8\") Read line#100:2 Read *ch:1\n Close dev\n Xecute \"s name=1\", (\"s %z=$g(y)\", .name)\n New (dev, name)\n Set val = @name, @name@(1) = 2\n &sql(SELECT Name INTO :found FROM Sample.Person WHERE ID = :dev AND Age > :name)\n If SQLCODE Quit found_val\n New %session\n Kill (dev, line, ch, name)\n If dev Quit line_ch_val\n Set @name = 1\n If dev Quit val\n ZTrap \"OOPS\"\n}\nClassMethod Ends() As %String\n{\n If $Random(2) Halt\n ZQuit\n}\n",
        );
        let diagnostics = without_hints(diagnostics);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].range.start.line, 15, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("undefined: val"),
            "{diagnostics:?}"
        );
    }

    // A quoted member name is the name without its quotes; a `{$$$X}` parameter default doesn't
    // shift the body; a parameterless macro called with arguments keeps them; a by-reference
    // argument to an unknown callee may define subscripts; `$ZCONVERT`'s handle is written.
    #[test]
    fn quoted_members_header_defaults_macro_args_and_byref_to_unknown() {
        let diagnostics = class_diagnostics(
            "Property \"grant_types\" As %String;\nProperty Plain As %String;\nMethod M(x, y As %String = {$$$OK}) As %String\n{\n #define OK 1\n #define Rows ..Two\n Set ..\"grant_types\" = ..Plain\n Do $$$Rows(x, y)\n Do ##class(%SYS.Unknown).Fill(.arr)\n Set h = \"\"\n Quit ..\"grant_types\"_arr(1)_$zcvt(x, \"I\", \"UTF8\", h)\n}\nMethod Two(a, b) As %String\n{\n Quit a_b\n}\n",
        );
        let diagnostics = without_hints(diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    // `$CLASSMETHOD`/`$METHOD` take an `args...` expansion; `&javascript<` is `&js<`.
    #[test]
    fn expanded_arguments_in_reflection_calls_and_javascript_marker() {
        let diagnostics = class_diagnostics(
            "ClassMethod M(cls, name, obj, args...) As %String\n{\n Do $ClassMethod(cls, name, 1, args...)\n &javascript<var a = 1; alert(a)>\n Quit $Method(obj, name, args...)_$Property(obj, name)\n}\n",
        );
        let diagnostics = without_hints(diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    // A `;` inside `&html<...>`/`&js<...>` is content; `%`-variables are untracked.
    #[test]
    fn embedded_blocks_and_untracked_percent_variables() {
        let diagnostics = class_diagnostics(
            "ClassMethod M(x, %p) As %String\n{\n\t&html<<tr><th>&nbsp;</th></tr>>\n\t&js<var a = 1; alert(a)>\n If x {\n  New %session\n  Set %session = 1\n }\n &sql(SELECT Name INTO :%out FROM Sample.Person WHERE ID = :%p)\n Quit x_%session\n}\n",
        );
        let diagnostics = without_hints(diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn public_list_only_clashes_with_bound_private_variables() {
        let diagnostics = diagnostics_for(&[(
            "T.mac",
            "ROUTINE T\na(m) [mt] PUBLIC {\n d b(m)\n q 1\n}\nb(m) [ojf] PUBLIC {\n s ojf=m\n q\n}\nc(m) [mt] PUBLIC {\n s ojf=2\n d b(m)\n q ojf\n}\n",
        )]);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("requires the private variable 'ojf'"),
            "{diagnostics:?}"
        );
    }

    // Function arguments parse generically and `cos::fun_args!` converts them, so a wrong number
    // or kind of argument is reported by number at parse time, not as a low-level syntax error.
    #[test]
    fn function_arity_is_reported_with_the_extra_arguments() {
        let diagnostics = class_diagnostics(
            "ClassMethod A(a, b) As %String {\n Quit $ZBITLEN(a,b)\n}\nClassMethod B(a, b, c, d, e) As %String {\n Quit $ZCONVERT(a,b,c,d,e)\n}\nClassMethod C() As %String {\n Quit $RANDOM()\n}\nClassMethod D(a) As %String {\n Quit $ZCONVERT(a,\"U\")_$LENGTH(a,\"x\")_$RANDOM(5)_$VIEW(1,,3)_$NOW()_$LISTBUILD()_$LIST(a,1:2,3:4)\n}\nClassMethod E(a) As %String {\n Quit $LENGTH(a,1,2)\n}\nClassMethod F(a) As %String {\n Quit $ORDER(a+1)\n}\nClassMethod G(a) As %String {\n Quit $TEXT(a,1)\n}\nClassMethod H(a) As %String {\n Quit $CASE(a,1:2,:3,:4)\n}\n",
        );
        let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(messages.len(), 7, "{diagnostics:?}");
        assert_eq!(
            messages[3..],
            [
                "$LENGTH takes at most 2 arguments; extra: argument 3 (`2`)",
                "$ORDER argument 1 must be a variable or property",
                "$TEXT takes at most 1 argument; extra: argument 2 (`1`)",
                "$CASE's `:default` must be its last argument",
            ],
            "{diagnostics:?}"
        );
        assert!(
            messages[0].contains("$ZBITLEN takes at most 1 argument; extra: argument 2 (`b`)"),
            "{messages:?}"
        );
        assert!(
            messages[1].contains("$ZCONVERT takes at most 4 arguments; extra: argument 5 (`e`)"),
            "{messages:?}"
        );
        assert!(
            messages[2].contains("$RANDOM takes at least 1 argument, but 0 were given"),
            "{messages:?}"
        );
    }

    #[test]
    fn quoted_instance_variable_and_hash_comment_braces_parse() {
        let diagnostics = class_diagnostics(
            "Property Name As %String;\nMethod M(y) As %String {\n If y {\n  #; an unbalanced { in a comment\n  Set x = i%\"Name\"\n }\n Quit $Get(x)\n}\n",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    // Labels inside a method body are checked like a routine's: each on its own, so a `$ZTRAP`
    // handler, a `Goto` target (including a computed `Goto $CASE`) and a `Do`/`$$` subroutine
    // all work without modeling how they're entered.
    #[test]
    fn labels_inside_method_bodies_are_checked() {
        let diagnostics = class_diagnostics(
            "ClassMethod Trap(Data As %String) As %Status {\n s $zt=\"Error\"\n s ^x=Data\n q 1\nError s $zt=\"\"\n q $ze\n}\n\
             ClassMethod TrapGoto() As %Status {\n Set tSC=1\n Set $ZT=\"Trap\"\n do {\n  Set ^h=$ZF(-6,1,2)\n } while (0)\nExit\n Quit tSC\nTrap\n Set $ZT=\"\",tSC=$ze\n Goto Exit\n}\n\
             ClassMethod Computed(Progress As %Integer) As %String {\n Goto $Case(Progress,1:Initial,2:Called,:error)\nerror\n Quit \"error\"\nInitial\n If Progress = 1 Quit \"initial\"\n Quit \"other\"\nCalled\n Quit \"called\"\n}\n\
             ClassMethod Sub(dir As %String) As %Status {\n Set n=1\n Do step\n Set n=n+$$twice(n)\n Quit n\nstep\n Set ^x(dir)=n\n Quit 1\ntwice(x)\n Quit x*2\n}\n",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let diagnostics = class_diagnostics(
            "ClassMethod FallsOff() As %Status {\n Set $ZT=\"Oops\"\n Quit 1\nOops\n Set $ZT=\"\"\n}\nClassMethod Unknown() As %Status {\n Goto Nowhere\n}\n",
        );
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("Missing an explicit QUIT") && d.range.start.line == 5),
            "{diagnostics:?}"
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("Cannot find label 'Nowhere'")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn constant_folding_that_overflows_or_divides_by_zero_is_not_folded() {
        let diagnostics = class_diagnostics(
            "ClassMethod M(x) {\n Set a = 5 - 10\n Set b = 10 \\ 0\n Set c = 10 # 0\n Set d = 2 ** 40\n If ($D(x) - 5) { Write 1 }\n If (5 - 10) > 0 { Write 2 }\n Write a, b, c, d\n}\n",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn cyclic_extends_does_not_recurse_forever() {
        let diagnostics = without_hints(diagnostics_for(&[
            (
                "A.cls",
                "Class Test.A Extends Test.B {\nProperty P As %String;\nClassMethod M() {\n Set o = ..%New()\n Write o.P, ..Nope()\n}\n}\n",
            ),
            ("B.cls", "Class Test.B Extends Test.A {\n}\n"),
        ]));
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("Cannot find member 'Nope'"),
            "{diagnostics:?}"
        );

        // `Selfx` resolves relative to the class's own package: to itself.
        let diagnostics = without_hints(diagnostics_for(&[(
            "Selfx.cls",
            "Class Test.Selfx Extends Selfx {\nClassMethod M() {\n Do ..Nope()\n}\n}\n",
        )]));
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    }

    #[test]
    fn many_refinements_in_one_method_do_not_overflow_the_type_variable_counter() {
        let body: String = (0..300)
            .map(|i| format!("\tIf $D(x{i}) {{ Write {i} }}\n"))
            .collect();
        let diagnostics =
            diagnostics_for(&[("Many.mac", &format!("ROUTINE Many\n{body}\tQuit\n"))]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn recursive_macro_is_reported_instead_of_expanding_forever() {
        for src in [
            "ROUTINE Rec\n#define A $$$A\n\twrite $$$A\n\tquit\n",
            "ROUTINE Rec\n#define B(x) $$$B(x)\n\twrite $$$B(1)\n\tquit\n",
            "ROUTINE Rec\n#define C $$$D\n#define D $$$C\n\twrite $$$C\n\tquit\n",
        ] {
            let diagnostics = diagnostics_for(&[("Rec.mac", src)]);
            assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
            assert!(
                diagnostics[0].message.contains("expands recursively"),
                "{diagnostics:?}"
            );
            assert_eq!(
                diagnostics[0].range.start.line as usize,
                src.matches('\n').count() - 2
            );
        }
    }

    #[test]
    fn include_cycles_are_included_once() {
        let diagnostics = diagnostics_for(&[
            ("R.mac", "ROUTINE R\n#include A\n\twrite $$$X\n\tquit\n"),
            ("A.inc", "ROUTINE A [Type=INC]\n#include B\n#define X 1\n"),
            ("B.inc", "ROUTINE B [Type=INC]\n#include A\n#include B\n"),
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn hover_past_the_end_of_a_routine_is_none() {
        let workspace = Workspace::new(None, None);
        let uri = "Short.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Short\n#define X 1\n\twrite $$$X\n\tquit\n".to_string(),
        );
        let position = bindings::Position {
            line: 100,
            character: 0,
        };
        assert!(workspace.hover(uri.clone(), position).is_none());
        assert!(workspace.macro_expansion(uri, position).is_none());
    }

    // A `#dim` at column 0 (legal in a routine) is analyzed exactly like an indented one.
    #[test]
    fn dim_at_column_zero_in_a_routine_is_checked_in_place() {
        let body =
            "#dim x As %String\n\tWrite x\n\tQuit\nlbl\n#dim y As %Integer\n\tWrite y\n\tQuit\n";
        let workspace = Workspace::new(None, None);
        let col0 = "Col0.mac".to_string();
        let indented = "Indented.mac".to_string();
        workspace.open(col0.clone(), format!("ROUTINE Col0\n{body}"));
        workspace.open(
            indented.clone(),
            format!("ROUTINE Indented\n{}", body.replace("#dim", " #dim")),
        );
        let diagnostics = workspace.diagnostics(indented.clone());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(workspace.diagnostics(col0.clone()), diagnostics);
        for (line, character) in [(2, 7), (6, 7)] {
            let position = bindings::Position { line, character };
            let hover = workspace.hover_type(indented.clone(), position);
            assert!(hover.is_some(), "no hover at {line}:{character}");
            assert_eq!(workspace.hover_type(col0.clone(), position), hover);
        }
    }

    #[test]
    fn subscripted_data_is_not_decided_by_the_scalar() {
        let diagnostics = class_diagnostics(
            "ClassMethod M() {\n Set x = 1\n If $D(x(\"a\")) { Write \"sub\" } Else { Write \"none\" }\n Write x\n}\n",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn catch_block_sees_writes_from_the_try_block_as_maybe() {
        let diagnostics = class_diagnostics(
            "ClassMethod M() {\n Try { Set x = 1  Set y = $ZF(-100,\"\",\"ls\") } Catch ex { Write x, y }\n}\n",
        );
        assert!(!diagnostics.is_empty());
        for d in &diagnostics {
            assert_eq!(d.severity, bindings::DiagnosticSeverity::Warning, "{d:?}");
            assert!(d.message.contains("may be undefined"), "{d:?}");
        }
    }

    #[test]
    fn while_condition_is_reapplied_on_every_pass() {
        let diagnostics = class_diagnostics(
            "ClassMethod M() {\n Set x = 1\n While $D(x) { Write x  Kill x }\n}\n",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn continue_in_a_loop_reaches_the_code_after_it() {
        let diagnostics = class_diagnostics(
            "ClassMethod M() {\n Do { If $R(2) { Continue }  Set x = 1 } While $R(2)\n Write x\n}\n",
        );
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("may be undefined: x"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn new_outside_a_procedure_block_undefines_the_variable() {
        let diagnostics = diagnostics_for(&[(
            "NewX.mac",
            "ROUTINE NewX\n\tSet x = 1\n\tNew x\n\tWrite x\n\tQuit\n",
        )]);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.severity == bindings::DiagnosticSeverity::Error
                    && d.message.contains("undefined: x")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn integer_literal_too_big_for_a_constant_is_not_zero() {
        let diagnostics = class_diagnostics("ClassMethod M() {\n If 4294967296 { Write 1 }\n}\n");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn new_returns_an_instance_even_when_onnew_is_unresolvable() {
        // No `Extends` at all: every member is known, so a missing one is an error.
        let diagnostics = without_hints(diagnostics_for(&[(
            "Nw.cls",
            "Class Test.Nw {\nProperty P As %String;\nClassMethod M() {\n Set obj = ##class(Test.Nw).%New()\n Do obj.Nope()\n Write obj.P\n}\n}\n",
        )]));
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("Cannot find member 'Nope'"),
            "{diagnostics:?}"
        );
        // A system superclass outside the workspace: `Nope` can't be confirmed, but `%New` can.
        let diagnostics = class_diagnostics(
            "Property P As %String;\nClassMethod M() {\n Set obj = ..%New()\n Do obj.Nope()\n Write obj.P\n}\n",
        );
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].severity, bindings::DiagnosticSeverity::Hint);
        assert!(diagnostics[0].message.contains("'Nope'"), "{diagnostics:?}");
    }

    #[test]
    fn get_of_a_maybe_undefined_variable_merges_with_the_default() {
        let workspace = Workspace::new(None, None);
        let uri = "G.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.G {\nClassMethod G(a As Test.G) {\n If $R(2) { Kill a }\n Set b = $G(a)\n Write b\n}\n}\n".to_string(),
        );
        let hover = workspace
            .hover_type(
                uri,
                bindings::Position {
                    line: 4,
                    character: 7,
                },
            )
            .expect("hover on `b`");
        assert!(!hover.contains("String"), "{hover}");
    }

    // A routine sharing a class's name is unrelated to it: not a duplicate, and not a class to
    // read the duplicate's range from.
    #[test]
    fn a_routine_named_like_a_class_is_not_a_duplicate_class() {
        let diagnostics = diagnostics_for(&[
            (
                "Same.cls",
                "Class Test.Same {\nClassMethod M() {\n Quit\n}\n}\n",
            ),
            ("Same.mac", "ROUTINE Test.Same\n\tQuit\n"),
            ("Same.inc", "ROUTINE Test.Same [Type=INC]\n#define X 1\n"),
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    // A mode mismatch compiles, so it's a warning and the rest of the method is still checked.
    #[test]
    fn argument_mode_mismatch_does_not_stop_checking_the_method() {
        for call in ["Do ..ByVal(.x)", "Do ..Out(1)"] {
            let diagnostics = class_diagnostics(&format!(
                "ClassMethod ByVal(x) {{\n Quit\n}}\nClassMethod Out(Output o) {{\n Set o = 1\n}}\nClassMethod M() {{\n {call}\n Set y = 1\n}}\n"
            ));
            assert!(
                diagnostics
                    .iter()
                    .all(|d| d.severity == bindings::DiagnosticSeverity::Warning),
                "{call}: {diagnostics:?}"
            );
            assert!(
                diagnostics.iter().any(|d| d.message.contains("'y'")),
                "{call}: {diagnostics:?}"
            );
        }
    }

    #[test]
    fn similar_name_suggestions_do_not_depend_on_candidate_order() {
        let candidates = ["tORPHANPROCEDURE", "tORPHANprocedure", "TRUNCATE"];
        let forward = check::top_similar("tORPHANPROC", candidates.iter().copied(), 20);
        let backward = check::top_similar("tORPHANPROC", candidates.iter().rev().copied(), 20);
        assert_eq!(forward, backward);
        assert_eq!(forward.len(), 2, "{forward:?}");
    }

    #[test]
    fn routine_expansion_computed_for_symbols_still_tracks_its_includes() {
        let workspace = Workspace::new(None, None);
        workspace.open(
            "A.inc".to_string(),
            "ROUTINE A [Type=INC]\n#define X 1\n".to_string(),
        );
        workspace.open(
            "R.mac".to_string(),
            "ROUTINE R\n#include A\n\twrite $$$X\n\tquit\n".to_string(),
        );
        assert!(workspace.document_symbol("R.mac".to_string()).is_some());
        let affected = workspace.open(
            "A.inc".to_string(),
            "ROUTINE A [Type=INC]\n#define Y 1\n".to_string(),
        );
        assert!(affected.contains(&"R.mac".to_string()), "{affected:?}");
        let diagnostics = workspace.diagnostics("R.mac".to_string());
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("'X' is undefined"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn class_depends_on_transitively_included_files() {
        let workspace = Workspace::new(None, None);
        workspace.open(
            "B.inc".to_string(),
            "ROUTINE B [Type=INC]\n#define X 1\n".to_string(),
        );
        workspace.open(
            "A.inc".to_string(),
            "ROUTINE A [Type=INC]\n#include B\n".to_string(),
        );
        workspace.open(
            "C.cls".to_string(),
            "Include A\n\nClass Test.C {\nClassMethod M() {\n Write $$$X\n}\n}\n".to_string(),
        );
        assert!(workspace.diagnostics("C.cls".to_string()).is_empty());
        let affected = workspace.open(
            "B.inc".to_string(),
            "ROUTINE B [Type=INC]\n#define Y 1\n".to_string(),
        );
        assert!(affected.contains(&"C.cls".to_string()), "{affected:?}");
        let diagnostics = workspace.diagnostics("C.cls".to_string());
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("'X' is undefined"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn class_index_forgets_a_name_its_file_no_longer_declares() {
        let workspace = Workspace::new(None, None);
        workspace.open(
            "C.cls".to_string(),
            "Class Test.C {\nClassMethod M() {\n Do ##class(Test.Foo).Baz()\n}\n}\n".to_string(),
        );
        let foo = bindings::Position {
            line: 2,
            character: 15,
        };
        let definition = |ws: &Workspace| ws.definition("C.cls".to_string(), foo).map(|l| l.uri);
        workspace.open("A.cls".to_string(), "Class Test.Foo {\n}\n".to_string());
        assert_eq!(definition(&workspace), Some("A.cls".to_string()));
        // Unparseable for a while (mid-edit), then renamed: `Test.Foo` is nobody's.
        workspace.open("A.cls".to_string(), "Clas Test.Foo {\n".to_string());
        workspace.open("A.cls".to_string(), "Class Test.Bar {\n}\n".to_string());
        assert_eq!(definition(&workspace), None);
    }

    #[test]
    fn closing_one_of_two_same_named_classes_leaves_the_other_indexed() {
        let workspace = Workspace::new(None, None);
        workspace.open(
            "C.cls".to_string(),
            "Class Test.C {\nClassMethod M() {\n Do ##class(Test.Foo).Baz()\n}\n}\n".to_string(),
        );
        workspace.open("A.cls".to_string(), "Class Test.Foo {\n}\n".to_string());
        workspace.open("B.cls".to_string(), "Class Test.Foo {\n}\n".to_string());
        assert!(!workspace.diagnostics("B.cls".to_string()).is_empty());
        let affected = workspace.close("A.cls".to_string());
        assert!(affected.contains(&"B.cls".to_string()), "{affected:?}");
        assert!(workspace.diagnostics("B.cls".to_string()).is_empty());
        let foo = bindings::Position {
            line: 2,
            character: 15,
        };
        assert_eq!(
            workspace
                .definition("C.cls".to_string(), foo)
                .map(|l| l.uri),
            Some("B.cls".to_string())
        );
    }

    // `%flag` inside the pasted argument is the caller's variable, not `Has`'s second parameter.
    #[test]
    fn macro_arguments_are_not_rescanned_for_other_parameters() {
        let workspace = Workspace::new(None, None);
        let uri = "Sub.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Sub\n#define Wrap(%flag) $$Wrap^Other(%flag)\n#define Has(%flags,%flag) (%flags[%flag)\n\tWrite $$$Has($$$Wrap($g(%flag)),\"D\")\n\tQuit\n".to_string(),
        );
        let expansion = workspace
            .macro_expansion(
                uri,
                bindings::Position {
                    line: 3,
                    character: 8,
                },
            )
            .expect("expansion");
        assert_eq!(expansion.trim(), "($$Wrap^Other($g(%flag))[\"D\")");
    }

    fn routine_diagnostics(body: &str) -> Vec<bindings::Diagnostic> {
        diagnostics_for(&[("D.mac", &format!("ROUTINE D\n{body}\tQuit\n"))])
    }

    #[test]
    fn ifdef_and_ifndef_are_evaluated() {
        // Undefined `FOO`: `#ifdef` skips its branch, `#ifndef` keeps it.
        assert!(routine_diagnostics("#ifdef FOO\n\tWrite $$$UNDEFA\n#endif\n").is_empty());
        let d = routine_diagnostics("#ifndef FOO\n\tWrite $$$UNDEFB\n#endif\n");
        assert!(
            d.iter()
                .any(|d| d.message.contains("'UNDEFB' is undefined")),
            "{d:?}"
        );
        // Defined `FOO`: the other way round, and `#else` works for both.
        let d = routine_diagnostics(
            "#define FOO 1\n#ifdef FOO\n\tWrite $$$UNDEFC\n#else\n\tWrite $$$UNDEFD\n#endif\n",
        );
        assert!(
            d.iter()
                .any(|d| d.message.contains("'UNDEFC' is undefined")),
            "{d:?}"
        );
        assert!(d.iter().all(|d| !d.message.contains("UNDEFD")), "{d:?}");
        let d = routine_diagnostics(
            "#define FOO 1\n#ifndef FOO\n\tWrite $$$UNDEFE\n#else\n\tWrite $$$UNDEFF\n#endif\n",
        );
        assert!(
            d.iter()
                .any(|d| d.message.contains("'UNDEFF' is undefined")),
            "{d:?}"
        );
        assert!(d.iter().all(|d| !d.message.contains("UNDEFE")), "{d:?}");
    }

    #[test]
    fn undef_removes_a_definition() {
        let d = routine_diagnostics("#define X 1\n#undef X\n\tWrite $$$X\n");
        assert!(
            d.iter().any(|d| d.message.contains("'X' is undefined")),
            "{d:?}"
        );
    }

    // The `#ifdef`'s `#endif` must not end the enclosing skipped `#if 0` branch.
    #[test]
    fn ifdef_nested_in_a_skipped_branch_does_not_end_it() {
        let d = routine_diagnostics(
            "#if 0\n#ifdef FOO\n\tWrite $$$UNDEFA\n#endif\n\tWrite $$$UNDEFLEAK\n#endif\n",
        );
        assert!(d.is_empty(), "{d:?}");
    }

    // The "Expands to" step of a parse error inside a macro expansion names the file, so an LSP
    // client (which needs a real uri) shows it instead of dropping it.
    #[test]
    fn parse_error_in_an_expansion_relates_to_the_file_itself() {
        let uri = "Bad.mac";
        let diagnostics =
            diagnostics_for(&[(uri, "ROUTINE Bad\n#define BAD set =\n\t$$$BAD\n\tQuit\n")]);
        let expands_to = diagnostics
            .iter()
            .flat_map(|d| &d.related_information)
            .find(|info| info.message.contains("Expands to"))
            .unwrap_or_else(|| panic!("{diagnostics:?}"));
        assert_eq!(expands_to.location.uri, uri, "{diagnostics:?}");
    }

    #[test]
    fn diagnostic_in_a_class_method_expansion_shows_the_expansion() {
        let diagnostics = diagnostics_for(&[(
            "Demo.cls",
            "Class MyLib.Demo\n{\nClassMethod Run() As %String\n{\n#define CALL(%x) $$Fn^Missing(%x)\n    Quit $$$CALL(1)\n}\n}\n",
        )]);
        let expands_to = diagnostics
            .iter()
            .flat_map(|d| &d.related_information)
            .find(|info| info.message.contains("Expands to"))
            .unwrap_or_else(|| panic!("{diagnostics:?}"));
        assert_eq!(expands_to.message, "↳ Expands to: Quit $$Fn^Missing(1)");
        let range = expands_to.location.range;
        assert_eq!(
            (
                range.start.line,
                range.start.character,
                range.end.line,
                range.end.character
            ),
            (5, 4, 5, 19),
            "{diagnostics:?}"
        );
    }

    // IRIS answering nothing for a class says nothing about its parameters: no "'MAXLEN' is not
    // a parameter of %Library.String".
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn type_parameter_of_an_unresolvable_class_is_not_reported() {
        let workspace = Workspace::new_with_iris(Some(Box::new(OccErrorsIris)), None);
        let uri = "P.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.P Extends %RegisteredObject {\nProperty Name As %String(MAXLEN = 50);\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert!(
            diagnostics
                .iter()
                .all(|d| !d.message.contains("is not a parameter")),
            "{diagnostics:?}"
        );
    }

    // Both land right after the closing brace; the `end while` one used to be pushed one
    // character further, past the newline, onto the next line.
    #[test]
    fn loop_end_hints_sit_after_the_closing_brace() {
        let workspace = Workspace::new(None, None);
        let uri = "Loops.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Loops {\nClassMethod M() {\n Set i = 0\n While i < 3 {\n  Set i = i + 1\n }\n For j = 1:1:3 {\n  Set i = i + j\n }\n Write i\n}\n}\n".to_string(),
        );
        let ends: Vec<(String, u32, u32)> = workspace
            .inlay_hint(uri, bindings::Range::default())
            .into_iter()
            .filter(|h| h.label.starts_with("end "))
            .map(|h| (h.label, h.position.line, h.position.character))
            .collect();
        assert_eq!(
            ends,
            vec![
                ("end while".to_string(), 5, 2),
                ("end for".to_string(), 8, 2)
            ]
        );
    }

    // Go to definition on a class's `Include X` or a routine's `#include X` opens `X.inc`.
    #[test]
    fn include_names_have_a_definition() {
        let workspace = Workspace::new(None, None);
        let inc = "ROUTINE Test.Shared [Type=INC]\n#define X 1\n";
        let cls = "Include (Test.Other, Test.Shared)\n\nClass Test.C {\nClassMethod M() {\n Write $$$X\n}\n}\n";
        let mac = "ROUTINE R\n#include Test.Shared\n\tWrite $$$X\n\tQuit\n";
        for (uri, text) in [("Shared.inc", inc), ("C.cls", cls), ("R.mac", mac)] {
            workspace.open(uri.to_string(), text.to_string());
        }
        let expected = bindings::Location::new(
            "Shared.inc".to_string(),
            bindings::Range {
                start: bindings::Position {
                    line: 0,
                    character: 8,
                },
                end: bindings::Position {
                    line: 0,
                    character: 19,
                },
            },
        );
        let at = |uri: &str, line, character| {
            workspace.definition(uri.to_string(), bindings::Position { line, character })
        };
        assert_eq!(
            at("C.cls", 0, cls.find("Test.Shared").unwrap() as u32 + 3),
            Some(expected.clone())
        );
        // The other, unresolvable include has none.
        assert_eq!(
            at("C.cls", 0, cls.find("Test.Other").unwrap() as u32 + 3),
            None
        );
        assert_eq!(at("R.mac", 1, 12), Some(expected));
    }

    // `SET $LISTGET(a,,b)=list` assigns `a` and `b`; a variable's span is its own, not relative
    // to the call it was re-read from.
    #[test]
    fn set_listget_assigns_its_variables() {
        let diagnostics =
            class_diagnostics("ClassMethod M(list) {\n Set $lg(a,,b,c)=list\n Write a, b\n}\n");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("'c' is assigned but never read"),
            "{diagnostics:?}"
        );
        assert_eq!(
            (
                diagnostics[0].range.start.line,
                diagnostics[0].range.start.character,
                diagnostics[0].range.end.character
            ),
            (2, 14, 15)
        );
    }

    // `#define GetMsg $$$Inner ` (trailing space) called as `$$$GetMsg(x)`: the parameterless
    // macro hands its arguments on, as `$$$Inner (x)`, which must still be a call of `Inner`.
    #[test]
    fn parameterless_macro_with_trailing_space_hands_its_arguments_on() {
        let diagnostics = diagnostics_for(&[(
            "Fwd.mac",
            "ROUTINE Fwd\n#define Inner(%a,%b) (%a_%b)\n#define Fwd $$$Inner \n\tSet x = $$$Fwd(\"a\",\"b\")\n\tWrite x\n\tQuit\n",
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    // Labels inside a routine's procedure block are checked like a method body's, with the
    // routine's labels still in scope. They return for the block: once its first return has
    // established a value, a label's bare `Quit` is reported, even one only ever `Do`ne.
    #[test]
    fn labels_inside_a_routine_procedure_block_are_checked() {
        let diagnostics = diagnostics_for(&[(
            "Blk.mac",
            "ROUTINE Blk\n\tQuit\nOuter\n\tQuit 1\nProc(x) public {\n\tIf x Goto exitEarly\n\tDo inner\n\tSet y = $$Outer\n\tQuit y\nexitEarly\n\tQuit 0\ninner\n\tSet ^g = 1\n\tQuit\n}\n",
        )]);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Expected return value of type 'Any' but got nothing"),
            "{diagnostics:?}"
        );
        assert_eq!(diagnostics[0].range.start.line, 13, "{diagnostics:?}");
        let diagnostics = diagnostics_for(&[(
            "Bad.mac",
            "ROUTINE Bad\n\tQuit\nProc() public {\n\tGoto nowhere\n\tQuit\nexitEarly\n\tQuit\n}\n",
        )]);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Cannot find label 'nowhere'"),
            "{diagnostics:?}"
        );
        // A bare `Goto` can't leave the block (IRIS: <NOLINE>), though `Do` may call out of it
        // and a routine-qualified `Goto Outer^Out` may too.
        let diagnostics = diagnostics_for(&[(
            "Out.mac",
            "ROUTINE Out\n\tQuit\nOuter\n\tQuit\nProc() public {\n\tDo Outer\n\tGoto Outer\n\tGoto Outer^Out\n}\n",
        )]);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Cannot GOTO label 'Outer' from inside a procedure block"),
            "{diagnostics:?}"
        );
        assert_eq!(diagnostics[0].range.start.line, 6, "{diagnostics:?}");
    }

    // Every label inside a method returns for the method, so its `Quit` is held to the declared
    // return type -- also one only ever `Do`ne, whose bare `Quit` is then reported.
    #[test]
    fn labels_inside_a_method_return_for_the_method() {
        let diagnostics = class_diagnostics(
            "ClassMethod M(x) As %Status {\n If x Goto Done\n Do step\n Quit 1\nDone\n Quit\nstep\n Set ^x = 1\n Quit\n}\n",
        );
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        for (diagnostic, line) in diagnostics.iter().zip([6, 9]) {
            assert!(
                diagnostic
                    .message
                    .contains("Expected return value of type '%Status' but got nothing"),
                "{diagnostics:?}"
            );
            assert_eq!(diagnostic.range.start.line, line, "{diagnostics:?}");
        }
    }

    // With `ProcedureBlock = 0` a method is a subroutine: its labels have their own returns, not
    // the method's, and fall through into one another like a routine's.
    #[test]
    fn labels_inside_a_non_procedure_block_method_are_subroutines() {
        let diagnostics = class_diagnostics(
            "ClassMethod M(x) As %Status [ ProcedureBlock = 0 ] {\n Do step\n Quit 1\nstep\n Set ^x = 1\n Quit\n}\n",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let diagnostics = class_diagnostics(
            "ClassMethod M(x) As %Status [ ProcedureBlock = 0 ] {\n Quit 1\nA\n If x Quit\n Set y = 1\nB\n Quit y\n}\n",
        );
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("Unexpected return value"),
            "{diagnostics:?}"
        );
    }

    // A procedure block's return shape is whatever its first return establishes, and a label
    // it `Goto`es shares that.
    #[test]
    fn goto_target_label_inside_a_procedure_block_shares_its_return_shape() {
        let diagnostics = diagnostics_for(&[(
            "Pb.mac",
            "ROUTINE Pb\n\tQuit\nProc(x) public {\n\tIf x Quit 1\n\tGoto fin\nfin\n\tQuit\n}\n",
        )]);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Expected return value of type 'Any' but got nothing"),
            "{diagnostics:?}"
        );
    }

    // A routine-level label that falls off its end runs into the next label, which then
    // returns for both: their return shapes must agree. The fall-through itself isn't reported.
    #[test]
    fn label_falling_through_shares_the_next_labels_return_shape() {
        // Directly, and across an alias label (`B`, no commands) in between.
        for text in [
            "ROUTINE Ft\n\tQuit\nA(y)\n\tIf y Quit\n\tSet x = 1\nB\n\tQuit x\n",
            "ROUTINE Ft\n\tQuit\nA(y)\n\tIf y Quit\n\tSet x = 1\nB\nC\n\tQuit x\n",
        ] {
            let diagnostics = diagnostics_for(&[("Ft.mac", text)]);
            assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
            assert!(
                diagnostics[0].message.contains("Unexpected return value"),
                "{diagnostics:?}"
            );
        }
    }

    // A `$$$Call(args)` left as is (not a `#define`d macro) still has its arguments expanded. A
    // range ending inside it maps back to the source, not to the shorter expanded text.
    #[test]
    fn ranges_over_an_unexpanded_macro_call_with_expanded_arguments_map_to_the_source() {
        let line = " return $$$NotAMacro($$$LongCode,\"x\")";
        let diagnostics = class_diagnostics(&format!(
            "ClassMethod M() {{\n#define LongCode 5001\n{line}\n}}\n"
        ));
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        let range = diagnostics[0].range;
        assert!(
            diagnostics[0].message.contains("'NotAMacro' is undefined"),
            "{diagnostics:?}"
        );
        assert_eq!(
            (range.start.line, range.start.character),
            (3, 8),
            "{diagnostics:?}"
        );
        assert_eq!(range.end.character as usize, line.len(), "{diagnostics:?}");
    }
}
