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
//! - `atelier.rs`: the `IrisContext` ascot makes out of the two Atelier REST API requests, a
//!   host's or its own connection's (native only).
//! - `iris.rs`: declaration files, answering `IrisContext` offline (native only).

mod atelier;
pub mod bindings;
pub mod check;
mod details;
#[cfg(not(target_arch = "wasm32"))]
mod iris;
#[cfg(not(target_arch = "wasm32"))]
pub mod lsp;
mod macro_expand;
mod mask;
#[cfg(not(target_arch = "wasm32"))]
pub mod mcp;
mod render;
pub mod style;
#[cfg(not(target_arch = "wasm32"))]
pub use atelier::RawAtelier;
pub use atelier::{Atelier, AtelierContext};
#[cfg(not(target_arch = "wasm32"))]
pub use iris::{DEFAULT_NAMESPACE as DEFAULT_IRIS_NAMESPACE, open as open_iris};
pub(crate) use macro_expand::MacroDefinitions;
pub(crate) use macro_expand::SourceMap;
use tygr_objectscript::cos::ClassDefinitionHeader;
pub use tygr_objectscript::{cos, sql};

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::Arc,
};

use tygr::{Grammar, OLC};

use crate::{
    bindings::{Diagnostic, InlayHint, Location, MemberInfo, NameInfo, Position, Style},
    check::WorkspaceEnv,
    cos::Routine,
    details::{ClassDetails, MemberKindSpecificDetails, WithRange, with_range_name},
};

// The only thing that decides how a document is analyzed: `.cls` files are checked as classes;
// `.inc` files are never checked standalone, only pulled in via another file's `#include`; `.mac`
// and `.int` routines are otherwise identical for our purposes, so share one variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    pub(crate) style: Option<style::KeywordStyles>,
}

/// What the checker asks about code outside the workspace. An `Err` says why a lookup couldn't be
/// made; `Ok(None)`, that IRIS has no such document.
pub trait IrisContext: Send + Sync {
    // `cls`'s own (not inherited) members, superclasses and class-level metadata.
    fn get_cls(&self, cls: &str) -> Result<Option<bindings::ClassInfo>, String>;
    // The full text of `name.inc`, code included.
    fn get_inc(&self, name: &str) -> Result<Option<bindings::Source>, String>;
    fn get_rtn(&self, name: &str) -> Result<Option<bindings::RoutineInfo>, String>;
}

type IrisHandle = Box<dyn IrisContext>;

#[cfg(target_arch = "wasm32")]
impl IrisContext for bindings::IrisContext {
    fn get_cls(&self, cls: &str) -> Result<Option<bindings::ClassInfo>, String> {
        bindings::IrisContext::get_cls(self, cls)
    }

    fn get_inc(&self, name: &str) -> Result<Option<bindings::Source>, String> {
        bindings::IrisContext::get_inc(self, name)
    }

    fn get_rtn(&self, name: &str) -> Result<Option<bindings::RoutineInfo>, String> {
        bindings::IrisContext::get_rtn(self, name)
    }
}

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

// A routine's labels read from its source; unknown if it doesn't parse. Its `#include`s aren't
// resolved, so this suits an `.int`, whose macros are already expanded.
pub(crate) fn routine_info(text: &str, uri: Option<String>) -> bindings::RoutineInfo {
    let workspace = build_workspace_struct(None, None);
    let expanded = workspace.expand_rtn(uri.as_deref().unwrap_or_default(), text);
    let labels = expanded.ok().map(|(routine, map, _)| {
        let mut labels: Vec<_> = check::labels_of(&map.expanded_olc, &routine, uri.as_deref())
            .iter()
            .map(|(name, label)| label.to_label(name.clone()))
            .collect();
        labels.sort_by(|a, b| a.name.cmp(&b.name));
        labels
    });
    bindings::RoutineInfo { labels, uri }
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
        style: style.as_ref().map(style::KeywordStyles::new),
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Workspace {
    /// [`GuestWorkspace::new`](bindings::GuestWorkspace::new), with a native IRIS client.
    pub fn native(iris: Option<Box<dyn IrisContext>>, style: Option<Style>) -> Self {
        Workspace(RefCell::new(build_workspace_struct(iris, style)))
    }
}

impl bindings::GuestWorkspace for Workspace {
    #[allow(async_fn_in_trait)]
    fn new(env: Option<bindings::Env>, style: Option<Style>) -> Self {
        #[cfg(target_arch = "wasm32")]
        let iris = env.map(|env| -> IrisHandle {
            match env {
                bindings::Env::Context(context) => Box::new(context),
                bindings::Env::Atelier(atelier) => Box::new(AtelierContext::from(atelier)),
            }
        });
        #[cfg(not(target_arch = "wasm32"))]
        let iris: Option<IrisHandle> = {
            // Never carries a real connection natively -- use `native` for that.
            let _ = env;
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
                        .in_order()
                        .into_iter()
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
            .all()
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
                    .all()
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

impl bindings::Guest for bindings::Component {
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

    #[cfg(not(target_arch = "wasm32"))]
    struct OccErrorsIris;

    #[cfg(not(target_arch = "wasm32"))]
    impl crate::IrisContext for OccErrorsIris {
        fn get_cls(&self, _cls: &str) -> Result<Option<bindings::ClassInfo>, String> {
            Ok(None)
        }
        fn get_rtn(&self, _name: &str) -> Result<Option<bindings::RoutineInfo>, String> {
            Ok(None)
        }
        fn get_inc(&self, name: &str) -> Result<Option<bindings::Source>, String> {
            Ok((name == "%occErrors").then(|| bindings::Source {
                text: "ROUTINE Errors [Type=INC]\n#define GeneralError 5001\n".to_string(),
                uri: None,
            }))
        }
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn class_gets_occ_errors_macros_via_iris_without_an_explicit_include() {
        let workspace = Workspace::native(Some(Box::new(OccErrorsIris)), None);
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
    fn cannot_find_member_always_suggests_percent_get_prefixed_methods() {
        // A dynamic-property-style getter's own name (e.g. `%GetData`) rarely resembles the
        // property a caller mistakenly wrote instead (`AtEnd`) -- so this must not depend on
        // `top_similar`'s ranking to surface it.
        struct GetterIris;
        impl crate::IrisContext for GetterIris {
            fn get_cls(&self, cls: &str) -> Result<Option<bindings::ClassInfo>, String> {
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
                Ok((cls == "%Fake.Getter").then(|| bindings::ClassInfo {
                    doc: String::new(),
                    name: bindings::NameInfo {
                        before: zero,
                        content: cls.to_string(),
                        after: zero,
                    },
                    extends: vec![],
                    inheritance_right: false,
                    storage: None,
                    deprecated: false,
                    members: vec![member("%GetData"), member("Close")],
                    property_class_names: vec![],
                    includes: vec![],
                    is_datatype: Some(false),
                    uri: None,
                }))
            }
            fn get_inc(&self, _name: &str) -> Result<Option<bindings::Source>, String> {
                Ok(None)
            }
            fn get_rtn(&self, _name: &str) -> Result<Option<bindings::RoutineInfo>, String> {
                Ok(None)
            }
        }
        let workspace = Workspace::native(Some(Box::new(GetterIris)), None);
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

    #[cfg(not(target_arch = "wasm32"))]
    struct MockIrisClient {
        is_datatype: Option<bool>,
        mem: Option<bindings::MemberInfo>,
    }

    #[cfg(not(target_arch = "wasm32"))]
    impl crate::IrisContext for MockIrisClient {
        fn get_cls(&self, _cls: &str) -> Result<Option<bindings::ClassInfo>, String> {
            let zero = bindings::Position {
                line: 0,
                character: 0,
            };
            Ok(Some(bindings::ClassInfo {
                doc: String::new(),
                name: bindings::NameInfo {
                    before: zero,
                    content: String::new(),
                    after: zero,
                },
                extends: vec![],
                inheritance_right: false,
                storage: None,
                deprecated: false,
                members: self.mem.clone().into_iter().collect(),
                property_class_names: vec![],
                includes: vec![],
                is_datatype: self.is_datatype,
                uri: None,
            }))
        }
        fn get_inc(&self, _name: &str) -> Result<Option<bindings::Source>, String> {
            Ok(None)
        }
        fn get_rtn(&self, _name: &str) -> Result<Option<bindings::RoutineInfo>, String> {
            Ok(None)
        }
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
        let workspace = Workspace::native(
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
    fn open_ignores_a_file_with_an_unrecognized_extension() {
        let workspace = Workspace::new(None, None);
        let uri = "notes.txt".to_string();
        workspace.open(
            uri.clone(),
            "this is not ObjectScript at all {{{".to_string(),
        );
        assert!(workspace.diagnostics(uri).is_empty());
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
        impl crate::IrisContext for ExternalClassIris {
            fn get_cls(&self, cls: &str) -> Result<Option<bindings::ClassInfo>, String> {
                let zero = bindings::Position {
                    line: 0,
                    character: 0,
                };
                Ok((cls == "Test.External").then(|| bindings::ClassInfo {
                    doc: String::new(),
                    name: bindings::NameInfo {
                        before: zero,
                        content: cls.to_string(),
                        after: zero,
                    },
                    extends: vec![],
                    inheritance_right: false,
                    storage: None,
                    deprecated: false,
                    members: vec![],
                    property_class_names: vec![],
                    includes: vec![],
                    is_datatype: Some(false),
                    uri: Some("mock:///External.cls".to_string()),
                }))
            }
            fn get_inc(&self, _name: &str) -> Result<Option<bindings::Source>, String> {
                Ok(None)
            }
            fn get_rtn(&self, _name: &str) -> Result<Option<bindings::RoutineInfo>, String> {
                Ok(None)
            }
        }
        let workspace = Workspace::native(Some(Box::new(ExternalClassIris)), None);
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
    #[cfg(not(target_arch = "wasm32"))]
    fn cross_routine_label_call_resolves_via_iris_when_not_in_the_workspace() {
        struct ExternalRoutineIris;
        impl crate::IrisContext for ExternalRoutineIris {
            fn get_cls(&self, _cls: &str) -> Result<Option<bindings::ClassInfo>, String> {
                Ok(None)
            }
            fn get_inc(&self, _name: &str) -> Result<Option<bindings::Source>, String> {
                Ok(None)
            }
            fn get_rtn(&self, name: &str) -> Result<Option<bindings::RoutineInfo>, String> {
                Ok(match name {
                    "External" => Some(crate::routine_info(
                        "ROUTINE External\n\n\tQuit\n\nFoo()\n\tQuit\n",
                        None,
                    )),
                    "Compiled" => Some(bindings::RoutineInfo {
                        labels: None,
                        uri: None,
                    }),
                    _ => None,
                })
            }
        }
        let workspace = Workspace::native(Some(Box::new(ExternalRoutineIris)), None);
        let uri = "Main.mac".to_string();
        // The hard error last, since it stops checking the rest of the body.
        workspace.open(
            uri.clone(),
            "ROUTINE Main\n\n\tDo Foo^Compiled(1)\n\tDo Foo^Typo()\n\tDo Foo^External(1)\n\tQuit\n"
                .to_string(),
        );
        let mut diagnostics: Vec<_> = workspace
            .diagnostics(uri)
            .into_iter()
            .map(|d| (d.severity, d.message))
            .collect();
        diagnostics.sort_by(|a, b| a.1.cmp(&b.1));
        use bindings::DiagnosticSeverity::*;
        assert_eq!(
            diagnostics,
            [
                (
                    Error,
                    "Cannot find routine 'Typo' in the workspace or on the IRIS instance".to_string()
                ),
                (
                    Hint,
                    "The labels of routine 'Compiled' aren't known, so calls into it can't be checked precisely"
                        .to_string()
                ),
                (Error, "Too many arguments".to_string()),
            ]
        );
    }

    #[test]
    fn a_failed_iris_lookup_is_a_hint_about_why() {
        struct DeadIris;
        impl crate::IrisContext for DeadIris {
            fn get_cls(&self, _cls: &str) -> Result<Option<bindings::ClassInfo>, String> {
                Err("connection refused".into())
            }
            fn get_inc(&self, _name: &str) -> Result<Option<bindings::Source>, String> {
                Err("connection refused".into())
            }
            fn get_rtn(&self, _name: &str) -> Result<Option<bindings::RoutineInfo>, String> {
                Err("connection refused".into())
            }
        }
        let workspace = Workspace::native(Some(Box::new(DeadIris)), None);
        let uri = "Main.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Main\n#include NoSuchInc\n\tDo Foo^External(1)\n\tWrite ##class(Some.Class).Foo()\n\tQuit\n"
                .to_string(),
        );
        let mut diagnostics: Vec<_> = workspace
            .diagnostics(uri)
            .into_iter()
            .map(|d| (d.severity, d.code, d.message))
            .collect();
        diagnostics.sort_by(|a, b| a.2.cmp(&b.2));
        let expected = |what: &str| {
            (
                bindings::DiagnosticSeverity::Hint,
                "A4471".to_string(),
                format!(
                    "Cannot look up {what} on the IRIS instance (connection refused), so it is taken on trust"
                ),
            )
        };
        assert_eq!(
            diagnostics,
            [
                expected("class 'Some.Class'"),
                expected("include file 'NoSuchInc'"),
                expected("routine 'External'"),
            ]
        );
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

    // The mock gives every class this one member, declared by `origin`.
    #[cfg(not(target_arch = "wasm32"))]
    fn masked_iris(name: &str, origin: &str, args: Vec<bindings::NormalArg>) -> Workspace {
        let mut mem = external_method(name, args, Some("%Library.ObjectHandle"));
        mem.origin = Some(origin.to_string());
        if name.starts_with('%')
            && let bindings::MemberKind::Method(m) = mem.kind
        {
            mem.kind = bindings::MemberKind::ClassMethod(m);
        }
        Workspace::native(
            Some(Box::new(MockIrisClient {
                is_datatype: None,
                mem: Some(mem),
            })),
            None,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn errors_in(workspace: &Workspace, uri: &str, text: &str) -> Vec<bindings::Diagnostic> {
        workspace.open(uri.to_string(), text.to_string());
        without_hints(workspace.diagnostics(uri.to_string()))
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn only_the_mask_says_this() {
        let mut mem = external_method("getNextResultSet", vec![], Some("This"));
        mem.origin = Some("%Library.SQLGatewayConnection".to_string());
        let workspace = Workspace::native(
            Some(Box::new(MockIrisClient {
                is_datatype: None,
                mem: Some(mem),
            })),
            None,
        );
        let diagnostics = errors_in(
            &workspace,
            "T.cls",
            "Class Test.T Extends %RegisteredObject {\nMethod M(c As %SQLGatewayConnection) As %String {\n Quit c.getNextResultSet().Next()\n}\n}\n",
        );
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Cannot find member 'Next' in This"),
            "{diagnostics:?}"
        );
    }

    // The mask also corrects the `%OnNew` that `%New` takes its arguments from.
    #[cfg(not(target_arch = "wasm32"))]
    // Without its own `%OnNew`, `%New` takes its own arguments: an optional `initvalue`.
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn an_unmasked_object_handle_stays_one() {
        let workspace = masked_iris("getNextResultSet", "%Library.SQLGatewayConnection", vec![]);
        let diagnostics = errors_in(
            &workspace,
            "T.cls",
            "Class Test.T Extends %RegisteredObject {\nMethod M(c As %SQLGatewayConnection) As %String {\n Set rs = c.getNextResultSet()\n Quit rs.Next()\n}\n}\n",
        );
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Cannot find member 'Next' in %ObjectHandle"),
            "{diagnostics:?}"
        );
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

    #[cfg(not(target_arch = "wasm32"))]
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

    #[test]
    fn expansion_in_an_included_file_is_located_there() {
        let diagnostics = diagnostics_for(&[
            ("Demo.mac", "ROUTINE Demo [Type=MAC]\nRun()\n#include Lib\n"),
            (
                "Lib.inc",
                "ROUTINE Lib [Type=INC]\n#define CALL(%x) $$Fn^Missing(%x)\n    Quit $$$CALL(1)\n",
            ),
        ]);
        let expands_to = diagnostics
            .iter()
            .flat_map(|d| &d.related_information)
            .find(|info| info.message.contains("Expands to"))
            .unwrap_or_else(|| panic!("{diagnostics:?}"));
        let range = expands_to.location.range;
        assert_eq!(expands_to.location.uri, "Lib.inc", "{diagnostics:?}");
        assert_eq!(
            (
                range.start.line,
                range.start.character,
                range.end.line,
                range.end.character
            ),
            (2, 4, 2, 19),
            "{diagnostics:?}"
        );
    }

    // IRIS answering nothing for a class says nothing about its parameters: no "'MAXLEN' is not
    // a parameter of %Library.String".
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn type_parameter_of_an_unresolvable_class_is_not_reported() {
        let workspace = Workspace::native(Some(Box::new(OccErrorsIris)), None);
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
}
