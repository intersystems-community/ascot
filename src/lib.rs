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
//! let workspace = Workspace::new();
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
//! `lsp` and `mcp` (modules behind the features of those names) wrap the same [`Workspace`] as a
//! stdio LSP server and an MCP server, respectively; both are also available as `ascot lsp`/`ascot mcp`
//! from the CLI binary.
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
#[cfg(all(not(target_arch = "wasm32"), feature = "declare"))]
mod declare;
mod declared;
mod details;
pub mod diagnostic_filter;
#[cfg(all(not(target_arch = "wasm32"), feature = "network"))]
mod download;
pub mod format;
#[cfg(not(target_arch = "wasm32"))]
mod iris;
#[cfg(all(not(target_arch = "wasm32"), feature = "network"))]
mod iris_connection;
#[cfg(all(not(target_arch = "wasm32"), feature = "lsp"))]
pub mod lsp;
mod macro_expand;
mod mask;
#[cfg(all(not(target_arch = "wasm32"), feature = "mcp"))]
pub mod mcp;
pub mod reference;
#[cfg(not(target_arch = "wasm32"))]
mod release;
mod render;
pub mod report;
#[cfg(all(not(target_arch = "wasm32"), feature = "analyze"))]
pub mod sarif;
#[cfg(not(target_arch = "wasm32"))]
pub mod settings;
pub mod style;
pub use atelier::{Atelier, AtelierContext};
pub use declared::Declarations;
#[cfg(not(target_arch = "wasm32"))]
pub use iris::{
    DEFAULT_NAMESPACE as DEFAULT_IRIS_NAMESPACE, LATEST as LATEST_IRIS, NONE as NO_IRIS,
    open as open_iris, open_all as open_iris_all,
};
#[cfg(all(not(target_arch = "wasm32"), feature = "network"))]
pub use iris_connection::RawAtelier;
pub(crate) use macro_expand::{Definitions, MacroDefinitions, SourceMap};
#[cfg(not(target_arch = "wasm32"))]
pub use release::{BUNDLED_DECLARATIONS, BundledDeclarations, DEFAULT_RELEASE};
use tygr_objectscript::cos::ClassDefinitionHeader;
pub use tygr_objectscript::{cos, sql};

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    hash::Hash,
    sync::Arc,
};

use details::Positions;
use tygr::{OLC, Parse, Print};

use crate::{
    bindings::{Diagnostic, InlayHint, Location, MemberInfo, Position, Style},
    check::WorkspaceEnv,
    cos::Routine,
    details::{ClassDetails, MemberDetails, MemberKindSpecificDetails, WithRange, with_range_name},
    macro_expand::Fidelity,
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

// The path of a uri reference, split as RFC 3986's appendix B does: the scheme and authority off
// the front, the query and fragment off the back, nothing validated. A file path passes through
// whole. VS Code's server folders give `isfs://host:namespace/Pkg/Cls.cls?generated=1`, whose
// authority is no host and port, so both the WHATWG `url` crate and a strict RFC 3986 parser
// refuse it.
fn uri_path(uri: &str) -> &str {
    let rest = match uri.split_once(':') {
        Some((scheme, rest)) if !scheme.is_empty() && !scheme.contains(['/', '?', '#']) => rest,
        _ => uri,
    };
    let rest = match rest.strip_prefix("//") {
        Some(authority) => authority
            .find(['/', '?', '#'])
            .map_or("", |at| &authority[at..]),
        None => rest,
    };
    rest.split(['?', '#']).next().unwrap_or(rest)
}

impl DocKind {
    // Purely a function of the extension of `uri`'s path -- known even for a uri that failed
    // tier A (or was never opened at all), unlike anything actually stored in `doc_cache`.
    pub fn from_uri(uri: &str) -> Option<Self> {
        let ext = uri_path(uri).rsplit('.').next()?.to_ascii_lowercase();
        match ext.as_str() {
            "cls" => Some(DocKind::Cls),
            "inc" => Some(DocKind::Inc),
            "mac" | "int" => Some(DocKind::Rtn),
            _ => None,
        }
    }

    fn syntax_scope(self) -> check::SyntaxScope {
        match self {
            DocKind::Cls => check::SyntaxScope::ClassDefinition,
            DocKind::Rtn => check::SyntaxScope::Routine,
            DocKind::Inc => check::SyntaxScope::IncludeFile,
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
    details: Option<Result<DocDetails, Vec<Diagnostic>>>,
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
#[derive(Clone, Default)]
struct CachedCheck {
    diagnostics: Vec<Diagnostic>,
    hints: Vec<InlayHint>,
    references: Vec<(bindings::Range, bindings::Location)>,
    hover_types: Vec<(bindings::Range, String)>,
    line_filters: diagnostic_filter::LineFilters,
}

#[derive(Default)]
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
    // Name -> uri, populated as a byproduct of tier A — lets `resolve_uri` be a plain O(1)
    // lookup once a class has been opened. A rename or removal is fixed up against `doc_cache`'s
    // own (old) entry directly, rather than keeping a uri -> name reverse index just for that.
    class_index: RefCell<HashMap<String, String>>,
    // Same idea as `class_index`, but for `.inc` header names — see `resolve_uri`.
    inc_index: RefCell<HashMap<String, String>>,
    // Same idea as `class_index`, but for `ROUTINE` header names — see `resolve_uri`.
    rtn_index: RefCell<HashMap<String, String>>,
    pub(crate) iris: Option<IrisHandle>,
    pub(crate) style: Option<style::Styles>,
    pub(crate) inlay_hints: bindings::InlayHintOptions,
    diagnostic_filter: diagnostic_filter::Filter,
    // %occErrors, resolved on first use: the error names a compile-time macro such as `$$$ERROR`
    // accepts in its arguments without any `Include`. Dropped whenever an include file changes.
    pub(crate) error_names: RefCell<Option<MacroDefinitions>>,
    // Includes fetched from IRIS, parsed once; `None` when IRIS has no such include.
    pub(crate) iris_includes: RefCell<HashMap<String, Option<macro_expand::Include>>>,
    // `CLASS_IMPLICIT_INCLUDES` expanded once, since every class gets the same result.
    pub(crate) implicit_definitions: RefCell<Option<(MacroDefinitions, HashSet<String>)>>,
}

/// What the checker asks about code outside the workspace. An `Err` says why a lookup couldn't be
/// made; `Ok(None)`, that IRIS has no such document.
pub trait IrisContext: Send + Sync {
    // `cls`'s own (not inherited) members, superclasses and class-level metadata.
    fn get_cls(&self, cls: &str) -> Result<Option<bindings::ClassInfo>, String>;
    // The full text of `name.inc`, code included.
    fn get_inc(&self, name: &str) -> Result<Option<bindings::Source>, String>;
    fn get_rtn(&self, name: &str) -> Result<Option<bindings::RoutineInfo>, String>;
    /// Whether the classes come with their SQL names (`SqlTableName`, `SqlFieldName`, `SqlProc`,
    /// ...), which a declaration file from before 1.1 leaves out.
    fn sql_names_declared(&self) -> bool {
        false
    }
    /// The class projected as the table or view `schema.table`, compared as SQL does, without
    /// regard to case; `Err` when the context cannot tell.
    fn find_cls_by_sql(&self, _schema: &str, _table: &str) -> Result<Option<String>, String> {
        Err("the context does not answer SQL names".to_string())
    }
    /// The class whose method or class query is the stored procedure `schema.name`.
    fn find_sql_proc(&self, _schema: &str, _name: &str) -> Result<Option<String>, String> {
        Err("the context does not answer SQL names".to_string())
    }
}

/// The SQL schema of a package: `User` is `SQLUser`, and dots are underscores.
pub fn sql_schema(package: &str) -> String {
    if package.eq_ignore_ascii_case("User") {
        "SQLUser".to_string()
    } else {
        package.replace('.', "_")
    }
}

type IrisHandle = Box<dyn IrisContext>;

/// A context several workspaces share, each holding an `Arc` of the one parsed set.
impl<T: IrisContext + ?Sized> IrisContext for std::sync::Arc<T> {
    fn get_cls(&self, cls: &str) -> Result<Option<bindings::ClassInfo>, String> {
        (**self).get_cls(cls)
    }

    fn get_inc(&self, name: &str) -> Result<Option<bindings::Source>, String> {
        (**self).get_inc(name)
    }

    fn get_rtn(&self, name: &str) -> Result<Option<bindings::RoutineInfo>, String> {
        (**self).get_rtn(name)
    }

    fn sql_names_declared(&self) -> bool {
        (**self).sql_names_declared()
    }

    fn find_cls_by_sql(&self, schema: &str, table: &str) -> Result<Option<String>, String> {
        (**self).find_cls_by_sql(schema, table)
    }

    fn find_sql_proc(&self, schema: &str, name: &str) -> Result<Option<String>, String> {
        (**self).find_sql_proc(schema, name)
    }
}

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

/// Whether `text`, the document at `uri`, is a routine IRIS generated (`[Type=INT,Generated]`, a
/// class's compiled code): compiler output nobody edits, analyzed for navigation, neither reported
/// on nor formatted.
pub fn generated(uri: &str, text: &str) -> bool {
    DocKind::from_uri(uri) == Some(DocKind::Rtn)
        && cos::RoutineHeader::parse_prefix(text).is_ok_and(|(header, _)| {
            (header.type_spec.as_ref()).is_some_and(|spec| spec.generated())
        })
}

impl WorkspaceStruct {
    pub(crate) fn generated(&self, uri: &str) -> bool {
        self.doc(uri)
            .is_some_and(|cell| generated(uri, &cell.borrow().text))
    }

    // Doubles as both open and edit; kind is inferred from `uri`'s extension. Returns the uris
    // (including this one) whose cached analysis is now stale and should be rechecked.
    fn open(&mut self, uri: String, text: String) -> Vec<String> {
        let Some(kind) = DocKind::from_uri(&uri) else {
            return vec![];
        };
        // The same text again stales nothing; the caller may still want this uri's diagnostics.
        if self
            .doc(&uri)
            .is_some_and(|cell| cell.borrow().text == text)
        {
            return vec![uri];
        }
        if kind == DocKind::Inc {
            self.forget_include_definitions();
        }
        let name = match kind {
            DocKind::Cls => ClassDefinitionHeader::parse_prefix(&text)
                .map(|(header, _)| with_range_name(&header.name, &OLC::new(&text))),
            // `.mac`/`.int`/`.inc` all share the same `ROUTINE Name [Type=...]` header grammar --
            // which of the two this is only matters once something asks for tier B (see
            // `ensure_details`), not here.
            DocKind::Inc | DocKind::Rtn => cos::RoutineHeader::parse_prefix(&text)
                .map(|(header, _)| with_range_name(&header.name, &OLC::new(&text))),
        };
        let entry = name
            .map_err(|err| tygr_error_to_diagnostics(err, &text, kind.syntax_scope()))
            .map(|name| DocCache {
                name,
                text,
                details: None,
            });
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
        if DocKind::from_uri(uri) == Some(DocKind::Inc) {
            self.forget_include_definitions();
        }
        let mut affected = self.invalidate(uri);
        if let Some(Ok(cell)) = self.doc_cache.remove(uri) {
            let name = cell.into_inner().name(uri);
            let kind = DocKind::from_uri(uri)
                .expect("a uri in doc_cache always has a recognized extension");
            affected.extend(self.unindex(kind, uri, &name));
            self.forget_deps(&(kind, name));
        }
        affected
    }

    fn name_of(&self, uri: &str) -> Option<Name> {
        Some((DocKind::from_uri(uri)?, self.doc(uri)?.borrow().name(uri)))
    }

    // The parsed document at `uri`: `None` for one that isn't open or whose header didn't parse.
    fn doc(&self, uri: &str) -> Option<&RefCell<DocCache>> {
        self.doc_cache.get(uri)?.as_ref().ok()
    }

    // Every parsed document: its uri, kind and cell.
    fn docs(&self) -> impl Iterator<Item = (&str, DocKind, &RefCell<DocCache>)> {
        self.doc_cache.iter().filter_map(|(uri, entry)| {
            let kind = DocKind::from_uri(uri)
                .expect("a uri in doc_cache always has a recognized extension");
            Some((uri.as_str(), kind, entry.as_ref().ok()?))
        })
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
        self.docs()
            .filter(|&(uri, kind, cell)| seen.contains(&(kind, cell.borrow().name(uri))))
            .map(|(uri, kind, cell)| {
                drop_derived(cell, kind == DocKind::Rtn);
                uri.to_string()
            })
            .collect()
    }

    fn forget_include_definitions(&self) {
        self.error_names.take();
        self.implicit_definitions.take();
    }

    // Everything built on the server is stale once it changes: checks, routine expansions (they
    // inline server includes) and the include caches. A style change stales only the checks.
    fn forget(&self, server: bool) -> Vec<String> {
        if server {
            self.forget_include_definitions();
            self.iris_includes.borrow_mut().clear();
        }
        self.docs()
            .map(|(uri, kind, cell)| {
                drop_derived(cell, server && kind == DocKind::Rtn);
                uri.to_string()
            })
            .collect()
    }

    // Only the edges from `name` to what it looked up: a name that looked `name` up keeps its edge.
    fn forget_deps(&self, name: &Name) {
        if let Some(deps) = self.depends_on.borrow_mut().remove(name) {
            let mut dependents = self.dependents.borrow_mut();
            for dep in deps {
                if let Some(set) = dependents.get_mut(&dep) {
                    set.remove(name);
                }
            }
        }
    }

    fn record_deps(&self, uri: &str, mut deps: HashSet<Name>) {
        let Some(name) = self.name_of(uri) else {
            return;
        };
        deps.remove(&name);
        self.forget_deps(&name);
        let mut dependents = self.dependents.borrow_mut();
        for dep in &deps {
            dependents
                .entry(dep.clone())
                .or_default()
                .insert(name.clone());
        }
        self.depends_on.borrow_mut().insert(name, deps);
    }

    // The include file `name` (a `ROUTINE name [Type=INC]` header) open in the workspace, at
    // its header name. An include fetched from IRIS has no document to point at.
    pub(crate) fn inc_definition(&self, name: &str) -> Option<Location> {
        let uri = self.resolve_uri(DocKind::Inc, name)?;
        let range = self.doc(&uri)?.borrow().name.range;
        Some(Location::new(uri, range))
    }

    // The whole declaration whose name a definition points at: a class member from its keyword
    // through its body, a routine's label through its body (a procedure block's `}`), a class or
    // an include file entire. `None` for a document outside the workspace, or a name no
    // declaration of the document's own carries (a nested label).
    fn declaration_range(&self, location: &Location) -> Option<bindings::Range> {
        let Location { uri, range } = location;
        if !self.doc_cache.contains_key(uri) {
            return None;
        }
        match DocKind::from_uri(uri)? {
            DocKind::Cls => {
                let details = self.cls_details(uri).ok()?;
                if details.name.range == *range {
                    return Some(full_range(&self.doc_text(uri)?));
                }
                (details.members.all())
                    .find(|member| member.it.name.range == *range)
                    .map(|member| member.range)
            }
            DocKind::Rtn => {
                let rtn = self.rtn_details(uri).ok()?;
                let olc = &rtn.map.expanded_olc;
                let (name, element) = (rtn.routine.subroutines.iter())
                    .flat_map(|subs| subs.items.iter())
                    .filter_map(|element| {
                        let details = details::routine_element_kind(olc, element)?;
                        Some((details.name.range, element))
                    })
                    .find(|(name, _)| {
                        rtn.map.map_lsp_range(*name).map(|(range, _)| range) == Some(*range)
                    })?;
                let end = olc.position(details::routine_element_end(element));
                let declaration = bindings::Range {
                    start: name.start,
                    end,
                };
                Some(rtn.map.map_lsp_range(declaration)?.0)
            }
            DocKind::Inc => Some(full_range(&self.doc_text(uri)?)),
        }
    }

    fn doc_text(&self, uri: &str) -> Option<String> {
        Some(self.doc(uri)?.borrow().text.clone())
    }

    // Every open class by name, with its uri.
    pub(crate) fn class_uris(&self) -> Vec<(String, String)> {
        (self.class_index.borrow().iter())
            .map(|(name, uri)| (name.clone(), uri.clone()))
            .collect()
    }

    pub(crate) fn cls_details(&self, uri: &str) -> Result<Arc<ClassDetails>, Vec<Diagnostic>> {
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

    // The open document of `kind` declaring `name`: a plain O(1) lookup, since `index_name` fills
    // the index the moment a document opens -- `None` just means no open document has that name.
    pub(crate) fn resolve_uri(&self, kind: DocKind, name: &str) -> Option<String> {
        self.index_for(kind).borrow().get(name).cloned()
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
        if let Some(old_name) = old_name.filter(|old| *old != name) {
            affected.extend(self.unindex(kind, uri, &old_name));
        }
        // `open` then invalidates by name, reaching a previous owner (checked before this one
        // existed, so unaware of the conflict) and everything that looked `name` up and missed.
        self.index_for(kind)
            .borrow_mut()
            .insert(name, uri.to_string());
        affected
    }

    // The other documents of `kind` (`uri` aside) declaring `name`. `index_for(kind)` only ever
    // remembers one owner per name (whichever claimed it last), so it can't answer this -- scan
    // `doc_cache`'s own tier-A names instead, same O(workspace) cost as a reverse index would be.
    fn others_named(&self, kind: DocKind, uri: &str, name: &str) -> impl Iterator<Item = String> {
        self.docs()
            .filter(move |&(other, other_kind, cell)| {
                other != uri && other_kind == kind && cell.borrow().name(other) == name
            })
            .map(|(other, ..)| other.to_string())
    }

    // Drops `uri`'s claim on `name`, handing it to another document of that kind still declaring
    // it, if any. Every such document's cached analysis (a duplicate-class error naming `uri`,
    // say) is stale, so they're all returned for rechecking.
    fn unindex(&self, kind: DocKind, uri: &str, name: &str) -> Vec<String> {
        let index = self.index_for(kind);
        if index.borrow().get(name).is_some_and(|owner| owner == uri) {
            index.borrow_mut().remove(name);
            if let Some(successor) = self.others_named(kind, uri, name).min() {
                index.borrow_mut().insert(name.to_string(), successor);
            }
        }
        self.invalidate_name(&(kind, name.to_string()))
    }

    fn duplicate_class_name_diagnostic(
        &self,
        uri: &str,
        range: bindings::Range,
    ) -> Option<Diagnostic> {
        let (_, name) = self.name_of(uri)?;
        let other_uri = self.others_named(DocKind::Cls, uri, &name).min()?;
        let doc = self.doc(&other_uri)?.borrow();
        let other = Location::new(other_uri, doc.name.range);
        Some(check::TypeError::duplicate_class_name(range, name, other).into())
    }

    pub(crate) fn rtn_details(&self, uri: &str) -> Result<Arc<RtnDetails>, Vec<Diagnostic>> {
        match self.ensure_details(uri)? {
            DocDetails::Rtn(details, _) => Ok(details),
            _ => panic!("rtn_details called on a uri that isn't an open routine"),
        }
    }

    // `.inc` files never reach the `expand_rtn` branch here -- they can't be macro-expanded on
    // their own (see `DocDetails::Inc`); `expand_rtn` resolves `#include`s by recursing back into
    // `doc_cache` (via `inc_parse`), but always for some *other* (`.inc`) uri's own cell -- never
    // this one -- so holding this cell borrowed for the whole call is safe.
    fn ensure_details(&self, uri: &str) -> Result<DocDetails, Vec<Diagnostic>> {
        let cell = self
            .doc_cache
            .get(uri)
            .expect("ensure_details called on a uri that isn't in the workspace")
            .as_ref()
            .map_err(|diagnostic| vec![diagnostic.clone()])?;
        let mut entry = cell.borrow_mut();
        let doc = &mut *entry;
        let text = doc.text.as_str();
        let fresh = doc.details.is_none();
        let kind =
            DocKind::from_uri(uri).expect("a uri in doc_cache always has a recognized extension");
        let syntax_error = |err| vec![tygr_error_to_diagnostics(err, text, kind.syntax_scope())];
        let details = doc
            .details
            .get_or_insert_with(|| match kind {
                DocKind::Cls => ClassDetails::try_from(text)
                    .map(|details| DocDetails::Cls(Arc::new(details), None))
                    .map_err(syntax_error),
                DocKind::Rtn => self
                    .expand_rtn(uri, text)
                    .map(|rtn| DocDetails::Rtn(Arc::new(rtn), None)),
                DocKind::Inc => cos::RoutineWithMacro::parse(text)
                    .map(|parsed| DocDetails::Inc(Arc::new(parsed)))
                    .map_err(syntax_error),
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

    // Every open document of `kind` whose header name starts with `prefix`. Tier A alone decides
    // the matches, so only they pay for a full (tier B) parse.
    fn uris_with_prefix(&self, kind: DocKind, prefix: &str) -> Vec<String> {
        self.docs()
            .filter(|&(_, k, cell)| k == kind && cell.borrow().name.it.starts_with(prefix))
            .map(|(uri, ..)| uri.to_string())
            .collect()
    }

    // The routine declared in `uri` and its labels, as a `textDocument/documentSymbol` outline.
    fn routine_symbol(&self, uri: &str) -> Option<bindings::DocumentSymbol> {
        let rtn = self.rtn_details(uri).ok()?;
        let text = self.doc_text(uri)?;
        let olc = &rtn.map.expanded_olc;
        let header_range = olc.span(0, text.lines().next().unwrap_or_default().len());
        let members = rtn
            .routine
            .subroutines
            .iter()
            .flat_map(|subs| subs.items.iter())
            .filter_map(|elem| details::routine_element_kind(olc, elem))
            .filter_map(|details| {
                let range = rtn.map.map_in_source(details.name.range)?;
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
                range: full_range(&text),
                selection_range: header_range,
            },
            members,
        })
    }

    // Same shape as `ensure_details`: check `uri`'s cached `CachedCheck` (embedded in whatever
    // `ensure_details` already returned -- no separate lookup needed) before computing a fresh
    // one from its tier-B payload.
    fn ensure_checked(&self, uri: &str) -> CachedCheck {
        if !self.doc_cache.contains_key(uri) {
            return CachedCheck::default();
        }
        let details = match self.ensure_details(uri) {
            Ok(details) => details,
            // A failed tier-B parse (see `ensure_details`) has no check slot to write into -- its
            // diagnostics are cheap to recompute from `cls_details`/`rtn_details`'s own cached
            // `Err` on every call, so there's no need to also cache them as a `CachedCheck`.
            Err(diagnostics) => {
                return CachedCheck {
                    diagnostics,
                    ..Default::default()
                };
            }
        };
        let mut result = match details {
            DocDetails::Cls(_, Some(check)) | DocDetails::Rtn(_, Some(check)) => return check,
            DocDetails::Cls(cls, None) => {
                let env = WorkspaceEnv::new(self);
                let text = self.doc_text(uri).unwrap_or_default();
                let olc = OLC::new(&text);
                let doc_env = check::DocumentEnv::new(&env, &olc, uri);
                let mut diagnostics = check::check_cls(&doc_env, &cls);
                diagnostics.extend(self.duplicate_class_name_diagnostic(uri, cls.name.range));
                diagnostics.extend(env.soft_diagnostics());
                if let Some(style) = &self.style {
                    let findings = format::findings_cls(&cls, &text, &olc, style, None);
                    diagnostics.extend(findings.into_iter().map(|f| f.diagnostic(&olc)));
                }
                let hints = doc_env.inlay_hints.into_inner();
                let hover_types = doc_env.hover_types.into_inner();
                let line_filters = doc_env.line_filters.into_inner().into_iter().collect();
                let references = env.references();
                self.record_deps(uri, env.touched_names());
                CachedCheck {
                    diagnostics,
                    hints,
                    references,
                    hover_types,
                    line_filters,
                }
            }
            DocDetails::Rtn(rtn, None) => {
                let env = WorkspaceEnv::new(self);
                let doc_env = check::DocumentEnv::new(&env, &rtn.map.expanded_olc, uri)
                    .with_source_map(&rtn.map);
                let mut diagnostics = check::check_rtn(&doc_env, &rtn.routine, &rtn.definitions);
                diagnostics.extend(env.soft_diagnostics());
                let mut diagnostics = rtn.map.relocate_diagnostics(diagnostics, uri);
                diagnostics.extend(rtn.map.unresolved.iter().cloned());
                // The style reads the routine as written, its macros unexpanded; when those hide
                // the syntax, the expanded routine, reported in the source's terms.
                if let Some(style) = &self.style {
                    let text = self.doc_text(uri).unwrap_or_default();
                    let map = &rtn.map;
                    diagnostics.extend(match cos::Routine::parse(&text) {
                        Ok(raw) => {
                            let olc = OLC::new(&text);
                            let found = format::findings_rtn(&raw, &text, style, None);
                            found.into_iter().map(|f| f.diagnostic(&olc)).collect()
                        }
                        Err(_) => {
                            let expanded = &map.expanded_text;
                            let found = format::findings_rtn(&rtn.routine, expanded, style, None);
                            let found = found.into_iter().map(|f| f.diagnostic(&map.expanded_olc));
                            map.relocate_diagnostics(found.collect(), uri)
                        }
                    });
                }
                // Hints only strictly inside source text; hovers and references at its edges too.
                let hints = doc_env
                    .inlay_hints
                    .into_inner()
                    .into_iter()
                    .filter_map(|hint| match rtn.map.map_position(hint.position)? {
                        (position, Fidelity::Exact) => Some(InlayHint { position, ..hint }),
                        _ => None,
                    })
                    .collect();
                let hover_types = doc_env
                    .hover_types
                    .into_inner()
                    .into_iter()
                    .filter_map(|(range, ty)| Some((rtn.map.map_in_source(range)?, ty)))
                    .collect();
                let mut references: Vec<_> = env
                    .references()
                    .into_iter()
                    .filter_map(|(range, mut location)| {
                        let range = rtn.map.map_in_source(range)?;
                        // A same-file label/procedure's `Location` is also in
                        // expanded-macro coordinates and needs relocating too.
                        if location.uri == uri {
                            location.range = rtn.map.map_in_source(location.range)?;
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
                let line_filters = (doc_env.line_filters.into_inner().into_iter())
                    .filter_map(|(range, filter)| Some((rtn.map.map_lsp_range(range)?.0, filter)))
                    .collect();
                let mut deps = env.touched_names();
                deps.extend(include_names(&rtn.map));
                self.record_deps(uri, deps);
                CachedCheck {
                    diagnostics,
                    hints,
                    references,
                    hover_types,
                    line_filters,
                }
            }
            // An include that parses has nothing to check on its own -- see `DocDetails::Inc`.
            DocDetails::Inc(_) => return CachedCheck::default(),
        };
        // Recorded twice over, as for an argument a macro body pastes twice.
        dedup(&mut result.diagnostics, |d| {
            (d.range, d.code.clone(), d.message.clone())
        });
        dedup(&mut result.hints, |hint| {
            (hint.position, hint.label.clone())
        });
        dedup(&mut result.references, Clone::clone);
        if let Some(cell) = self.doc(uri)
            && let Some(Ok(DocDetails::Cls(_, slot) | DocDetails::Rtn(_, slot))) =
                &mut cell.borrow_mut().details
        {
            *slot = Some(result.clone());
        }
        result
    }
}

// Keeps the first of the items alike in `key`.
fn dedup<T, K: Hash + Eq>(items: &mut Vec<T>, key: impl Fn(&T) -> K) {
    let mut seen = HashSet::new();
    items.retain(|item| seen.insert(key(item)));
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
    let workspace = WorkspaceStruct::default();
    let expanded = workspace.expand_rtn(uri.as_deref().unwrap_or_default(), text);
    let labels = expanded.ok().map(|rtn| {
        let mut labels: Vec<_> = check::labels_of(&rtn.map, &rtn.routine, uri.as_deref())
            .iter()
            .filter_map(|(name, label)| Some(label.procedure()?.to_label(name.clone())))
            .collect();
        labels.sort_by(|a, b| a.name.cmp(&b.name));
        labels
    });
    bindings::RoutineInfo { labels, uri }
}

// Drops what `cell` built on other documents: its check, and with `expansion` its expansion too
// (a routine's inlines its `#include`s) -- never a class's own parse, which only its own edit
// changes, nor an `.inc`'s, which is substituted in whoever includes it, not here.
fn drop_derived(cell: &RefCell<DocCache>, expansion: bool) {
    let mut cache = cell.borrow_mut();
    let details = &mut cache.details;
    match details {
        Some(Ok(DocDetails::Rtn(..))) if expansion => *details = None,
        Some(Ok(DocDetails::Rtn(_, check) | DocDetails::Cls(_, check))) => *check = None,
        _ => {}
    }
}

/// Sources asked in order. A document is the first source's that has it; a source that fails is
/// passed over, and its failure is the answer only when no later source has the document.
pub(crate) struct Chain(pub(crate) Vec<Box<dyn IrisContext>>);

impl Chain {
    fn first<T>(
        &self,
        get: impl Fn(&dyn IrisContext) -> Result<Option<T>, String>,
    ) -> Result<Option<T>, String> {
        let mut failed = None;
        for source in &self.0 {
            match get(source.as_ref()) {
                Ok(Some(found)) => return Ok(Some(found)),
                Ok(None) => {}
                Err(err) => {
                    failed.get_or_insert(err);
                }
            }
        }
        failed.map_or(Ok(None), Err)
    }
}

impl IrisContext for Chain {
    fn get_cls(&self, cls: &str) -> Result<Option<crate::bindings::ClassInfo>, String> {
        self.first(|source| source.get_cls(cls))
    }

    fn sql_names_declared(&self) -> bool {
        self.0.iter().all(|source| source.sql_names_declared())
    }

    fn find_cls_by_sql(&self, schema: &str, table: &str) -> Result<Option<String>, String> {
        self.first(|source| source.find_cls_by_sql(schema, table))
    }

    fn find_sql_proc(&self, schema: &str, name: &str) -> Result<Option<String>, String> {
        self.first(|source| source.find_sql_proc(schema, name))
    }

    fn get_inc(&self, name: &str) -> Result<Option<crate::bindings::Source>, String> {
        self.first(|source| source.get_inc(name))
    }

    fn get_rtn(&self, name: &str) -> Result<Option<crate::bindings::RoutineInfo>, String> {
        self.first(|source| source.get_rtn(name))
    }
}

// The sources as the host lists them, asked in order; a declaration file that doesn't read is
// the error, so the host learns of it rather than checking against nothing.
fn iris_handle(env: Vec<bindings::Env>) -> Result<Option<IrisHandle>, String> {
    let mut sources: Vec<IrisHandle> = Vec::new();
    for source in env {
        match source {
            bindings::Env::Declarations(bytes) => sources.push(Box::new(
                declared::Declarations::from_bytes(&bytes)
                    .map_err(|err| format!("declarations: {err}"))?,
            )),
            #[cfg(target_arch = "wasm32")]
            bindings::Env::Context(context) => sources.push(Box::new(context)),
            #[cfg(target_arch = "wasm32")]
            bindings::Env::Atelier(atelier) => {
                sources.push(Box::new(AtelierContext::from(atelier)))
            }
            // Never carries a real connection natively -- use `native` for that.
            #[cfg(not(target_arch = "wasm32"))]
            _ => {}
        }
    }
    Ok(match sources.len() {
        0 => None,
        1 => sources.pop(),
        _ => Some(Box::new(Chain(sources))),
    })
}

#[cfg(not(target_arch = "wasm32"))]
impl Workspace {
    /// [`GuestWorkspace::new`](bindings::GuestWorkspace::new), with a native IRIS client.
    pub fn native(iris: Option<Box<dyn IrisContext>>, style: Option<Style>) -> Self {
        Workspace(RefCell::new(WorkspaceStruct {
            iris,
            style: style.as_ref().map(style::Styles::new),
            ..Default::default()
        }))
    }

    /// Opens `path` if it is a `.cls`/`.mac`/`.int`/`.inc` file, or every such file under it if it
    /// is a directory, each keyed by its `file:` url. Returns the uris opened, and the uris whose
    /// analysis is now stale.
    pub fn open_directory(&self, path: &std::path::Path) -> (Vec<String>, Vec<String>) {
        let (mut opened, mut affected) = (vec![], vec![]);
        self.open_under(path, &mut opened, &mut affected);
        (opened, affected)
    }

    fn open_under(
        &self,
        path: &std::path::Path,
        opened: &mut Vec<String>,
        affected: &mut Vec<String>,
    ) {
        if path.is_dir() {
            for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
                self.open_under(&entry.path(), opened, affected);
            }
        } else if path.to_str().and_then(DocKind::from_uri).is_some()
            && let Ok(text) = std::fs::read_to_string(path)
            && let Ok(uri) = url::Url::from_file_path(path)
        {
            affected.extend(bindings::GuestWorkspace::open(self, uri.to_string(), text));
            opened.push(uri.to_string());
        }
    }
}

impl Workspace {
    /// Which codes `diagnostics` reports, and at what severity.
    pub fn set_diagnostic_filter(&self, filter: diagnostic_filter::Filter) {
        self.borrow_mut().diagnostic_filter = filter;
    }

    /// Every document's uri.
    pub fn uris(&self) -> Vec<String> {
        self.borrow().doc_cache.keys().cloned().collect()
    }

    /// [`GuestWorkspace::set_env`](bindings::GuestWorkspace::set_env) with the environment open:
    /// the documents stay, what was learned from the old one goes, and the uris returned are stale.
    pub fn set_iris(&self, iris: Option<IrisHandle>) -> Vec<String> {
        let mut this = self.borrow_mut();
        this.iris = iris;
        this.forget(true)
    }
}

impl bindings::GuestWorkspace for Workspace {
    fn new() -> Self {
        Workspace(RefCell::new(WorkspaceStruct::default()))
    }

    fn open(&self, uri: String, text: String) -> Vec<String> {
        self.borrow_mut().open(uri, text)
    }

    fn close(&self, uri: String) -> Vec<String> {
        self.borrow_mut().remove_uri(&uri)
    }

    fn set_env(&self, env: Vec<bindings::Env>) -> Result<Vec<String>, String> {
        Ok(self.set_iris(iris_handle(env)?))
    }

    fn set_style(&self, style: Option<Style>) -> Vec<String> {
        let mut this = self.borrow_mut();
        this.style = style.as_ref().map(style::Styles::new);
        this.forget(false)
    }

    fn set_inlay_hints(&self, options: bindings::InlayHintOptions) -> Vec<String> {
        let mut this = self.borrow_mut();
        this.inlay_hints = options;
        this.forget(false)
    }

    fn set_diagnostics(&self, diagnostics: Vec<String>) -> Result<Vec<String>, String> {
        self.set_diagnostic_filter(diagnostics.join(" ").parse()?);
        Ok(self.uris())
    }

    fn diagnostics(&self, uri: String) -> Vec<bindings::Diagnostic> {
        let this = self.borrow();
        if this.generated(&uri) {
            return vec![];
        }
        let filter = &this.diagnostic_filter;
        let checked = this.ensure_checked(&uri);
        (checked.diagnostics.into_iter())
            .filter_map(|diagnostic| filter.apply(&checked.line_filters, diagnostic))
            .collect()
    }

    fn inlay_hint(&self, uri: String, _range: bindings::Range) -> Vec<InlayHint> {
        let this = self.borrow();
        if this.generated(&uri) {
            return vec![];
        }
        this.ensure_checked(&uri).hints
    }

    fn format(
        &self,
        uri: String,
        options: bindings::FormattingOptions,
    ) -> Option<Vec<bindings::TextEdit>> {
        let this = self.borrow();
        let text = this.doc_text(&uri)?;
        if this.generated(&uri) {
            return Some(vec![]);
        }
        let Some(style) = &this.style else {
            return Some(vec![]);
        };
        let spaces = " ".repeat(options.tab_size as usize);
        let indent = if options.insert_spaces {
            spaces.as_str()
        } else {
            "\t"
        };
        let edits = format::edits(&text, &uri, style, indent)?;
        let olc = OLC::new(&text);
        Some(edits.into_iter().map(|edit| edit.text_edit(&olc)).collect())
    }

    fn code_actions(&self, uri: String, range: bindings::Range) -> Vec<bindings::CodeAction> {
        let this = self.borrow();
        if this.generated(&uri) {
            return vec![];
        }
        let Some((text, style)) = this.doc_text(&uri).zip(this.style.as_ref()) else {
            return vec![];
        };
        // No formatting options come with a code action request: the document's own indentation.
        let fixes = format::fixes(&text, &uri, style, format::indent_of(&text)).unwrap_or_default();
        let olc = OLC::new(&text);
        let (start, end) = (olc.offset(range.start), olc.offset(range.end));
        let filter = &this.diagnostic_filter;
        let lines = this.ensure_checked(&uri).line_filters;
        fixes
            .into_iter()
            .filter(|fix| fix.range.start <= end && start <= fix.range.end)
            .filter_map(|mut fix| {
                let edit = fix.edit.take()?.text_edit(&olc);
                let diagnostic = filter.apply(&lines, fix.diagnostic(&olc))?;
                Some(bindings::CodeAction {
                    title: diagnostic.message.clone(),
                    kind: Some("quickfix".to_string()),
                    diagnostics: vec![diagnostic],
                    edit: Some(bindings::WorkspaceEdit {
                        changes: vec![(uri.clone(), vec![edit])],
                    }),
                })
            })
            .collect()
    }

    // Where the reference at `position` is declared, if any — one of the go-to-definition
    // edges discovered as a byproduct of the last check, as a link: the name to select, and the
    // whole declaration to reveal (see `declaration_range`). Covers class references
    // (`##class(Name)`, `x.Foo`, `x.Foo()`), instance members (`i%Foo`), self-references
    // (`..Method`), label calls/gotos, in the same or another open routine, and
    // `Include`/`#include` names. Doesn't cover anything the checker resolves to no location
    // (e.g. `%New`, unmapped IRIS system classes), computed/indirect `Goto`, or anything in SQL
    // (which isn't type-checked yet).
    fn definition(
        &self,
        uri: String,
        position: bindings::Position,
    ) -> Option<bindings::LocationLink> {
        let this = self.borrow();
        let (range, location) = this
            .ensure_checked(&uri)
            .references
            .into_iter()
            .find(|(range, _)| position_in_range(position, *range))?;
        Some(bindings::LocationLink {
            origin_selection_range: Some(range),
            target_range: this.declaration_range(&location).unwrap_or(location.range),
            target_uri: location.uri,
            target_selection_range: location.range,
        })
    }

    // Every location across the workspace that refers to whatever's at `position` — the reverse
    // of `definition`. `position` may be on a reference itself (same lookup `definition` does)
    // or on the declaration site (found by checking every recorded reference's target instead of
    // its source), so this works whether invoked from a call site or from the declaration.
    fn references(
        &self,
        uri: String,
        position: bindings::Position,
        include_declaration: bool,
    ) -> Vec<bindings::Location> {
        let this = self.borrow();
        let all: Vec<(String, bindings::Range, bindings::Location)> = (this.doc_cache.keys())
            .flat_map(|doc_uri| {
                (this.ensure_checked(doc_uri).references.into_iter())
                    .map(move |(range, location)| (doc_uri.clone(), range, location))
            })
            .collect();
        let Some(target) = all
            .iter()
            .find(|(src_uri, range, _)| *src_uri == uri && position_in_range(position, *range))
            .or_else(|| {
                all.iter().find(|(_, _, location)| {
                    location.uri == uri && position_in_range(position, location.range)
                })
            })
            .map(|(_, _, location)| location.clone())
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
    fn hover(&self, uri: String, position: bindings::Position) -> Option<String> {
        if let Some(member) = self.hover_member(uri.clone(), position) {
            let this = self.borrow();
            let class = (member.uri.as_deref())
                .and_then(|uri| this.cls_details(uri).ok())
                .map(|details| details.name.it.clone());
            return Some(render::format_member_hover(&member, class.as_deref()));
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
    fn query_cls(&self, query: String) -> Vec<(String, bindings::ClassInfo)> {
        let this = self.borrow();
        this.uris_with_prefix(DocKind::Cls, &query)
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
    fn query_mem(&self, cls: String, query: String) -> Vec<(String, MemberInfo)> {
        self.query_cls(cls.clone())
            .into_iter()
            .filter(|(_, info)| info.name == cls)
            .flat_map(|(uri, info)| {
                info.members
                    .into_iter()
                    .filter(|member| member.name.starts_with(&query))
                    .map(move |member| (uri.clone(), member))
            })
            .collect()
    }

    // The class declared in `uri` and its members, or (in a `.mac`/`.int` routine) its labels,
    // as a `textDocument/documentSymbol` outline.
    fn document_symbol(&self, uri: String) -> Option<bindings::DocumentSymbol> {
        let this = self.borrow();
        this.doc_cache.get(&uri)?;
        match DocKind::from_uri(&uri)? {
            DocKind::Cls => {
                let details = this.cls_details(&uri).ok()?;
                let text = this.doc_text(&uri)?;
                Some(bindings::DocumentSymbol {
                    root: bindings::SymbolInfo {
                        name: details.name.to_string(),
                        kind: bindings::SymbolKind::Class,
                        deprecated: details.keywords.deprecated,
                        range: full_range(&text),
                        selection_range: details.name.range,
                    },
                    members: (details.members.in_order().into_iter())
                        .map(|member| member_symbol(member).0)
                        .collect(),
                })
            }
            DocKind::Rtn => this.routine_symbol(&uri),
            DocKind::Inc => None,
        }
    }

    // Classes and members, and routines and labels, across the workspace whose name starts with
    // `query`, as a `workspace/symbol` result. Tier A alone decides which classes/routines match
    // (see `uris_with_prefix`); only matches pay for the full (tier B) parse needed to list
    // their members/labels.
    fn workspace_symbol(&self, query: String) -> Vec<bindings::WorkspaceSymbol> {
        fn symbol_in(
            uri: &str,
            symbol: bindings::SymbolInfo,
            container_name: Option<String>,
        ) -> bindings::WorkspaceSymbol {
            bindings::WorkspaceSymbol {
                name: symbol.name,
                kind: symbol.kind,
                deprecated: symbol.deprecated,
                location: bindings::Location::new(uri.to_string(), symbol.selection_range),
                container_name,
            }
        }
        let query = query.as_bytes();
        let matches = |name: &str| {
            query.is_empty()
                || (name.as_bytes().windows(query.len())).any(|w| w.eq_ignore_ascii_case(query))
        };
        let this = self.borrow();
        let cls_symbols = this
            .uris_with_prefix(DocKind::Cls, "")
            .into_iter()
            .filter_map(|uri| Some((uri.clone(), this.cls_details(&uri).ok()?)))
            .flat_map(|(uri, details)| {
                let class_name = details.name.to_string();
                let cls_symbol = matches(&class_name).then(|| bindings::WorkspaceSymbol {
                    name: class_name.clone(),
                    kind: bindings::SymbolKind::Class,
                    deprecated: details.keywords.deprecated,
                    location: bindings::Location::new(uri.clone(), details.name.range),
                    container_name: None,
                });
                cls_symbol
                    .into_iter()
                    .chain(
                        (details.members.in_order().into_iter())
                            .filter(|member| matches(&member.it.name.it))
                            .map(move |member| {
                                let (symbol, keyword) = member_symbol(member);
                                symbol_in(&uri, symbol, Some(format!("{keyword} • {class_name}")))
                            }),
                    )
                    .collect::<Vec<_>>()
            });

        let rtn_symbols = this
            .uris_with_prefix(DocKind::Rtn, "")
            .into_iter()
            .flat_map(|uri| {
                let Some(symbol) = this.routine_symbol(&uri) else {
                    return vec![];
                };
                let routine = symbol.root.name.clone();
                (matches(&routine)
                    .then(|| symbol_in(&uri, symbol.root, None))
                    .into_iter())
                .chain(
                    (symbol.members.into_iter())
                        .filter(|label| matches(&label.name))
                        .map(|label| symbol_in(&uri, label, Some(format!("Label • {routine}")))),
                )
                .collect::<Vec<_>>()
            });

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
            if position >= body_start {
                let location = this
                    .ensure_checked(&uri)
                    .references
                    .into_iter()
                    .find(|(range, _)| position_in_range(position, *range))
                    .map(|(_, location)| location)?;
                if DocKind::from_uri(&location.uri) != Some(DocKind::Cls)
                    || !this.doc_cache.contains_key(&location.uri)
                {
                    return None;
                }
                let target = this.cls_details(&location.uri).ok()?;
                return target
                    .members
                    .all()
                    .find(|m| m.it.name.range == location.range)
                    .map(|m| MemberInfo {
                        uri: Some(location.uri.clone()),
                        ..MemberInfo::from(m)
                    });
            }
        }
        Some(MemberInfo {
            uri: Some(uri),
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

    // What the macro invocation at `position` expanded to, if any. `.mac`/`.int` only -- a
    // `.cls` method body's expansion isn't kept for hover, and a `.inc` file's own macros can't
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
    map.include_names
        .iter()
        .map(|name| (DocKind::Inc, name.clone()))
        .collect()
}

// A member as a `documentSymbol` entry, with the keyword it is declared with.
fn member_symbol(member: &WithRange<MemberDetails>) -> (bindings::SymbolInfo, &'static str) {
    let kind = bindings::MemberKind::from(&member.it.kind);
    let symbol = bindings::SymbolInfo {
        name: member.it.name.to_string(),
        kind: bindings::member_kind_to_symbol_kind(&kind),
        deprecated: member.it.deprecated,
        range: member.range,
        selection_range: member.it.name.range,
    };
    (symbol, bindings::member_keyword(&kind))
}

fn full_range(text: &str) -> bindings::Range {
    bindings::Range {
        start: Position::default(),
        end: OLC::new(text).position(text.len()),
    }
}

fn position_in_range(position: Position, range: bindings::Range) -> bool {
    (range.start..=range.end).contains(&position)
}

pub(crate) fn tygr_error_to_diagnostics(
    e: cos::Error,
    src: &str,
    scope: check::SyntaxScope,
) -> Diagnostic {
    let start = OLC::new(src).position(e.pos());
    let end = Position {
        character: start.character + 1,
        ..start
    };
    check::TypeError::syntax_error(bindings::Range { start, end }, &e, src, scope).into()
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::bindings::GuestWorkspace;

    use super::*;

    // `Test.Sub` calls `Test.Base`'s `Foo`, each in its own document.
    fn base_and_sub() -> (Workspace, String, String) {
        let workspace = Workspace::new();
        let (base_uri, sub_uri) = ("Base.cls".to_string(), "Sub.cls".to_string());
        workspace.open(
            base_uri.clone(),
            "Class Test.Base {\nClassMethod Foo() {\n}\n}\n".to_string(),
        );
        workspace.open(
            sub_uri.clone(),
            "Class Test.Sub {\nClassMethod Bar() {\n do ##class(Test.Base).Foo()\n}\n}\n"
                .to_string(),
        );
        (workspace, base_uri, sub_uri)
    }

    fn span(range: bindings::Range) -> ((u32, u32), (u32, u32)) {
        (
            (range.start.line, range.start.character),
            (range.end.line, range.end.character),
        )
    }

    const OPTIONS: bindings::FormattingOptions = bindings::FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
    };

    // A link's target as `references` lists it.
    fn target(link: &bindings::LocationLink) -> bindings::Location {
        bindings::Location::new(link.target_uri.clone(), link.target_selection_range)
    }

    #[track_caller]
    fn assert_clean(workspace: &Workspace, uri: &str) {
        let diagnostics = workspace.diagnostics(uri.to_string());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn dependency_invalidation() {
        let (workspace, base_uri, sub_uri) = base_and_sub();

        assert_clean(&workspace, &sub_uri);

        // Editing `Base` should invalidate `Sub` too, even though `Sub` wasn't touched.
        let affected: HashSet<String> = workspace
            .open(
                base_uri.clone(),
                "Class Test.Base {\nClassMethod Foo() {\n Quit\n}\n}\n".to_string(),
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
        let workspace = Workspace::new();
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

        assert_clean(&workspace, &mac_uri);

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
    fn a_server_folder_uri_keeps_its_kind_past_the_query() {
        let workspace = Workspace::new();
        let uri = "isfs://irishealth:ns/Pkg/Cls.cls?generated=1".to_string();
        let text = "Class Pkg.Cls {\nClassMethod Foo() {\n Write undefinedVar\n}\n}\n".to_string();
        assert_eq!(workspace.open(uri.clone(), text), vec![uri.clone()]);
        assert!(!workspace.diagnostics(uri).is_empty());
        for (uri, kind) in [
            ("file:///x/Y.mac#frag", Some(DocKind::Rtn)),
            ("isfs://srv:ns/?csp", None),
            ("C:\\dir\\Z.inc", Some(DocKind::Inc)),
            ("/Users/a:b/W.cls", Some(DocKind::Cls)),
            ("Base.cls", Some(DocKind::Cls)),
        ] {
            assert_eq!(DocKind::from_uri(uri), kind, "{uri}");
        }
    }

    #[test]
    fn opening_a_missing_include_later_invalidates_its_includers() {
        let workspace = Workspace::new();
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
                    .any(|d| d.message.contains("Cannot find include file 'Shared'")),
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
            assert_clean(&workspace, uri);
        }
    }

    #[test]
    fn opening_a_missing_class_later_invalidates_its_users() {
        let workspace = Workspace::new();
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
        assert_clean(&workspace, &sub_uri);
    }

    #[test]
    fn open_directory_keys_every_source_file_by_its_url() {
        let dir = std::env::temp_dir().join(format!("ascot-open-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("pkg")).unwrap();
        std::fs::write(dir.join("pkg/A.cls"), "Class Test.A {\n}\n").unwrap();
        std::fs::write(dir.join("R.mac"), "ROUTINE R\n Quit\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "skipped").unwrap();
        let workspace = Workspace::new();
        let (mut opened, affected) = workspace.open_directory(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        opened.sort();
        let url = |p: &str| url::Url::from_file_path(dir.join(p)).unwrap().to_string();
        assert_eq!(opened, [url("R.mac"), url("pkg/A.cls")]);
        assert_eq!(affected.len(), 2, "{affected:?}");
        assert_eq!(workspace.uris().len(), 2);
    }

    #[test]
    fn set_env_forgets_what_the_old_server_said() {
        let workspace = mock_iris(None, None);
        let uri = "Owner.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Owner {\nProperty P As App.NotOpen;\n}\n".to_string(),
        );
        assert!(workspace.diagnostics(uri.clone()).is_empty());
        assert_eq!(
            workspace.set_env(vec![]).unwrap(),
            std::slice::from_ref(&uri)
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("App.NotOpen"));
    }

    // The host hands over a declaration file's bytes, gzipped as the pre-built asset ships or not;
    // one another ascot wrote is no environment at all.
    #[test]
    fn a_declaration_file_passed_as_bytes_is_the_environment() {
        use std::io::Write;
        let file = |version: &str| {
            format!(
                r#"{{"version":"{version}","namespace":"%SYS","classes":{{"Demo.Base":{{"header":"Class Demo.Base","members":[{{"header":"ClassMethod Hello() As %String"}}]}}}},"includes":{{}},"routines":{{}}}}"#
            )
        };
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(file(declared::DECLARATIONS_VERSION).as_bytes())
            .unwrap();
        let source = "Class Test.Use {\nClassMethod M() As %String {\n Quit ##class(Demo.Base).Hello()\n}\n}\n";
        for bytes in [
            gz.finish().unwrap(),
            file(declared::DECLARATIONS_VERSION).into_bytes(),
        ] {
            let workspace = Workspace::new();
            workspace
                .set_env(vec![bindings::Env::Declarations(bytes)])
                .unwrap();
            workspace.open("Use.cls".to_string(), source.to_string());
            assert_clean(&workspace, "Use.cls");
        }
        // One another ascot wrote is refused.
        let incompatible = bindings::Env::Declarations(file("0.1.0").into_bytes());
        assert!(Workspace::new().set_env(vec![incompatible]).is_err());
    }

    #[test]
    fn without_iris_a_property_type_outside_the_workspace_is_an_error() {
        let workspace = Workspace::new();
        let uri = "Owner.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Owner {\nProperty P As App.NotOpen;\n}\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].severity, bindings::DiagnosticSeverity::Error);
        assert!(diagnostics[0].message.contains("App.NotOpen"));
    }

    struct MockIrisClient {
        is_datatype: Option<bool>,
        mem: Option<bindings::MemberInfo>,
    }

    impl crate::IrisContext for MockIrisClient {
        fn get_cls(&self, _cls: &str) -> Result<Option<bindings::ClassInfo>, String> {
            Ok(Some(bindings::ClassInfo {
                doc: String::new(),
                name: String::new(),
                members: self.mem.clone().into_iter().collect(),
                class_type: (self.is_datatype == Some(true))
                    .then_some(bindings::ClassType::Datatype),
                ..Default::default()
            }))
        }
        fn get_inc(&self, _name: &str) -> Result<Option<bindings::Source>, String> {
            Ok(None)
        }
        fn get_rtn(&self, _name: &str) -> Result<Option<bindings::RoutineInfo>, String> {
            Ok(None)
        }
    }

    fn mock_iris(is_datatype: Option<bool>, mem: Option<bindings::MemberInfo>) -> Workspace {
        let iris = MockIrisClient { is_datatype, mem };
        Workspace::native(Some(Box::new(iris)), None)
    }

    #[test]
    fn without_iris_a_superclass_outside_the_workspace_is_an_error_at_the_extends() {
        let workspace = Workspace::new();
        let uri = "Probe.cls".to_string();
        // With no IRIS there is no environment: `%RegisteredObject` doesn't exist, which is
        // reported where it is named; past it the hierarchy is unknown, so a member nobody
        // declares is unconfirmed rather than missing.
        workspace.open(
            uri.clone(),
            "Class Test.Probe Extends %RegisteredObject {\nMethod Foo() {\n do ..SomeInheritedMethod()\n quit\n}\n}\n".to_string(),
        );

        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert_eq!(diagnostics[0].severity, bindings::DiagnosticSeverity::Error);
        assert!(diagnostics[0].message.contains("%Library.RegisteredObject"));
        assert_eq!(diagnostics[0].range.start.line, 0);
        assert_eq!(
            diagnostics[1].severity,
            bindings::DiagnosticSeverity::Information
        );
        assert!(diagnostics[1].message.contains("SomeInheritedMethod"));
        assert_eq!(diagnostics[1].range.start.line, 2);
    }

    // The user-command alternative's failure is not appended to the command report: one line,
    // as before `ZNAME args` could be parsed, and `ZNAME args` itself still is.
    #[test]
    fn an_unrecognized_command_is_reported_in_one_line() {
        for (body, start) in [
            (
                "ClassMethod M(x) {\n Ydummyword x\n}\n",
                "`Ydummyword` must be a command; the commands are",
            ),
            (
                "ClassMethod M() {\n ZKill\n}\n",
                "`ZKill` must be followed by a well-formed argument",
            ),
        ] {
            let diagnostics = class_diagnostics(body);
            let error = diagnostics
                .iter()
                .find(|d| d.code == "A8073")
                .expect("A8073");
            assert!(error.message.contains(start), "{}", error.message);
            assert!(!error.message.contains('\n'), "{}", error.message);
        }
        let diagnostics = class_diagnostics("ClassMethod M(x) {\n ZFoo x\n Quit\n}\n");
        assert!(
            diagnostics.iter().any(|d| d.code == "A5362"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn with_an_environment_a_superclass_it_lacks_is_information_at_the_extends() {
        let empty = format!(
            r#"{{"version":"{}","namespace":"%SYS","classes":{{}},"includes":{{}},"routines":{{}}}}"#,
            declared::DECLARATIONS_VERSION
        );
        let workspace = Workspace::new();
        workspace
            .set_env(vec![bindings::Env::Declarations(empty.into_bytes())])
            .unwrap();
        let uri = "Probe.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Probe Extends Nope.Missing {\nMethod Foo() {\n do ..SomeInheritedMethod()\n quit\n}\n}\n".to_string(),
        );

        let diagnostics = workspace.diagnostics(uri);
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert!(
            diagnostics
                .iter()
                .all(|d| d.severity == bindings::DiagnosticSeverity::Information)
        );
        assert!(diagnostics[0].message.contains("Nope.Missing"));
        let range = diagnostics[0].range;
        assert_eq!(
            (range.start.character, range.end.character),
            (25, 37),
            "{range:?}"
        );
        assert!(diagnostics[1].message.contains("SomeInheritedMethod"));
    }

    #[test]
    fn a_generated_routine_is_navigation_only() {
        let workspace = styled(r#"{"command": {"case": "lower"}}"#);
        let generated = "Pkg.Cls.1.int".to_string();
        let text = "ROUTINE Pkg.Cls.1 [Type=INT,Generated]\n ;Pkg.Cls.1\nzFoo(a) methodimpl {\n Set unused = a\n Quit never }\nzBar() methodimpl {\n Do zFoo(1) }\n";
        workspace.open(generated.clone(), text.to_string());
        assert!(workspace.diagnostics(generated.clone()).is_empty());
        assert!(
            workspace
                .inlay_hint(generated.clone(), bindings::Range::default())
                .is_empty()
        );
        let set = || bindings::Range::point((3, 1).into());
        assert_eq!(workspace.format(generated.clone(), OPTIONS), Some(vec![]));
        assert!(workspace.code_actions(generated.clone(), set()).is_empty());
        let call = (6, 4).into();
        assert!(workspace.definition(generated, call).is_some());
        // The same code as a routine of its own is reported on.
        let edited = "Plain.int".to_string();
        let text = text
            .replace(",Generated", "")
            .replace("methodimpl", "public");
        workspace.open(edited.clone(), text);
        assert!(!workspace.diagnostics(edited.clone()).is_empty());
        assert!(
            !workspace
                .format(edited.clone(), OPTIONS)
                .unwrap()
                .is_empty()
        );
        assert_eq!(workspace.code_actions(edited, set()).len(), 1);
    }

    #[test]
    fn open_ignores_a_file_with_an_unrecognized_extension() {
        let workspace = Workspace::new();
        let uri = "notes.txt".to_string();
        workspace.open(
            uri.clone(),
            "this is not ObjectScript at all {{{".to_string(),
        );
        assert!(workspace.diagnostics(uri).is_empty());
    }

    #[test]
    fn diagnostics_and_inlay_hint_share_one_check() {
        let workspace = Workspace::new();
        let uri = "Shared.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Shared {\nClassMethod Foo() As %Integer {\n set x = ..Bar()\n write x\n quit x\n}\nClassMethod Bar() As %Integer {\n quit 1\n}\n}\n".to_string(),
        );
        let diagnostics_of = |uri: &str| {
            let mut diagnostics = workspace.diagnostics(uri.to_string());
            diagnostics.retain(|d| d.code != "A8061");
            diagnostics
        };
        let diagnostics = diagnostics_of(&uri);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let hints = workspace.inlay_hint(uri.clone(), bindings::Range::default());
        assert!(!hints.is_empty(), "expected an inferred-type hint for `x`");
        // Re-checking after an unrelated no-op read shouldn't change the result.
        assert_eq!(diagnostics_of(&uri), diagnostics);
        assert_eq!(workspace.inlay_hint(uri, bindings::Range::default()), hints);
    }

    #[test]
    fn type_hint_spells_the_type_as_the_inlay_hint_options_say() {
        let workspace = Workspace::new();
        let uri = "Spelling.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Spelling {\nClassMethod Foo() As %Integer {\n set x = ..Bar()\n quit x\n}\nClassMethod Bar() As %Integer {\n quit 1\n}\n}\n".to_string(),
        );
        let type_hint = || {
            workspace
                .inlay_hint(uri.clone(), bindings::Range::default())
                .into_iter()
                .find(|h| h.label.contains("Integer"))
                .map(|h| (h.label, h.padding_left, h.padding_right))
        };
        let spelled = |type_annotation| bindings::InlayHintOptions {
            type_annotation: Some(type_annotation),
        };
        assert_eq!(type_hint(), Some(("As %Integer".to_string(), true, true)));
        assert_eq!(
            workspace.set_inlay_hints(spelled(bindings::TypeAnnotation::Colon)),
            std::slice::from_ref(&uri)
        );
        assert_eq!(type_hint(), Some((": %Integer".to_string(), false, true)));
        workspace.set_inlay_hints(spelled(bindings::TypeAnnotation::As));
        assert_eq!(type_hint(), Some(("As %Integer".to_string(), true, true)));
        workspace.set_inlay_hints(bindings::InlayHintOptions::default());
        assert_eq!(type_hint(), Some(("As %Integer".to_string(), true, true)));
    }

    #[test]
    fn creates_hint_shows_non_new_variable_written_in_subroutine() {
        let workspace = Workspace::new();
        let uri = "CreatesTest.mac".to_string();
        // A legacy subroutine (no `Public(...)` clause) defaults every undeclared local to
        // shared/non-`New`'d — writing `foo` here silently leaks it into the caller's scope.
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n\tQuit\n\nSub\t;\n\tSet foo = 1\n\tQuit\n".to_string(),
        );

        assert_clean(&workspace, &uri);

        let hints = workspace.inlay_hint(uri, bindings::Range::default());
        assert!(hints.iter().any(|h| h.label.contains("[foo]")), "{hints:?}");
    }

    #[test]
    fn creates_hint_shows_non_new_variable_written_via_merge() {
        let workspace = Workspace::new();
        let uri = "CreatesTest3.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n\tQuit\n\nSub\t;\n\tSet bar = 1\n\tMerge foo = bar\n\tQuit\n"
                .to_string(),
        );

        assert_clean(&workspace, &uri);

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
        let workspace = Workspace::new();
        let uri = "CreatesTest2.cls".to_string();
        // A `ClassMethod` is private-by-default (unlike a legacy subroutine): an undeclared
        // local defaults to `New`'d, so writing one isn't a "creates" leak.
        workspace.open(
            uri.clone(),
            "Class Test.CreatesTest2 {\nClassMethod Foo() {\n set x = 1\n write x\n}\n}\n"
                .to_string(),
        );

        assert_clean(&workspace, &uri);

        let hints = workspace.inlay_hint(uri, bindings::Range::default());
        assert!(!hints.iter().any(|h| h.label.starts_with('[')), "{hints:?}");
    }

    // An argumentless `Do` with its dotted block, and a stand-alone `Else`, get a hint and are
    // skipped; the defect after each is still reported.
    #[test]
    fn checking_continues_past_an_unmodeled_legacy_construct() {
        let diagnostics = diagnostics_for(&[(
            "Legacy.mac",
            "ROUTINE Legacy\nDOTTED\n D\n . S X=1\n D MISSING\n Q\nTIMED\n L +^Legacy(1):5\n E  Q\n D MISSING\n Q\n",
        )]);
        let lines = |severity| -> Vec<u32> {
            diagnostics
                .iter()
                .filter(|d| d.severity == severity)
                .map(|d| d.range.start.line)
                .collect()
        };
        assert_eq!(
            lines(bindings::DiagnosticSeverity::Information),
            [2, 8],
            "{diagnostics:?}"
        );
        assert_eq!(
            lines(bindings::DiagnosticSeverity::Error),
            [4, 9],
            "{diagnostics:?}"
        );
        assert!(diagnostics.iter().all(|d| {
            d.severity != bindings::DiagnosticSeverity::Error || d.message.contains("MISSING")
        }));
    }

    #[test]
    fn hover_type_shows_inferred_local_variable_type() {
        let workspace = Workspace::new();
        let uri = "Hover2.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Hover2 {\nClassMethod Foo() As %Integer {\n set x = ..Bar()\n write x\n quit x\n}\nClassMethod Bar() As %Integer {\n quit 1\n}\n}\n".to_string(),
        );
        let diagnostics = workspace
            .diagnostics(uri.clone())
            .into_iter()
            .filter(|d| d.code != "A8061")
            .collect::<Vec<_>>();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        // Line 3 (` write x`): `x` is at character 7.
        let ty = workspace
            .hover_type(uri.clone(), (3, 7).into())
            .expect("expected a hover type for `x`");
        assert!(ty.contains("Integer"), "{ty}");

        // The `Set` target itself (` set x = ...`, `x` at character 5) isn't covered.
        assert!(workspace.hover_type(uri, (2, 5).into()).is_none());
    }

    #[test]
    fn quoted_argument_pasted_into_dollar_g_is_the_string_alone() {
        let workspace = Workspace::new();
        let uri = "QuotedArg.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n#define VALUE(%v) $g(%v)\n\n\tWrite $$$VALUE(\"x\")\n\tQuit\n"
                .to_string(),
        );
        assert_clean(&workspace, &uri);
        let expansion = workspace
            .macro_expansion(uri, (4, 10).into())
            .expect("expected a macro expansion at that position");
        assert!(
            expansion.contains("\"x\"") && !expansion.contains("$g("),
            "{expansion:?}"
        );
    }

    #[test]
    fn macro_expansion_shows_expanded_text() {
        let workspace = Workspace::new();
        let uri = "MacroHover.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n#define GREETING \"hi\"\n\n\tWrite $$$GREETING\n\tQuit\n".to_string(),
        );
        assert_clean(&workspace, &uri);

        // Line 4 (`\tWrite $$$GREETING`): inside the `$$$GREETING` invocation.
        let expansion = workspace
            .macro_expansion(uri.clone(), (4, 10).into())
            .expect("expected a macro expansion at that position");
        assert!(expansion.contains("hi"), "{expansion:?}");

        // Elsewhere on the same line, not inside the invocation.
        assert!(workspace.macro_expansion(uri, (4, 1).into()).is_none());
    }

    #[test]
    fn query_cls_and_query_mem_prefix_match() {
        let workspace = Workspace::new();
        workspace.open(
            "Test.Foo.cls".to_string(),
            "Class Test.Foo {\nClassMethod Bar() {\n}\nClassMethod Baz() {\n}\n}\n".to_string(),
        );
        workspace.open("Other.cls".to_string(), "Class Other {\n}\n".to_string());

        let classes = workspace.query_cls("Test.".to_string());
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].0, "Test.Foo.cls");
        assert_eq!(classes[0].1.name, "Test.Foo");

        let members = workspace.query_mem("Test.Foo".to_string(), "Ba".to_string());
        let mut names: Vec<&str> = members.iter().map(|(_, m)| m.name.as_str()).collect();
        names.sort();
        assert_eq!(names, ["Bar", "Baz"]);
        assert!(members.iter().all(|(uri, _)| uri == "Test.Foo.cls"));

        assert!(workspace.query_cls("NoSuchClass".to_string()).is_empty());
    }

    #[test]
    fn document_symbol_returns_class_and_members() {
        let workspace = Workspace::new();
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
    fn workspace_symbol_finds_classes_and_members_by_name() {
        let workspace = Workspace::new();
        workspace.open(
            "Test.Foo.cls".to_string(),
            "Class Test.Foo {\nClassMethod Bar() {\n}\n}\n".to_string(),
        );
        workspace.open("Other.cls".to_string(), "Class Other {\n}\n".to_string());

        let symbols = workspace.workspace_symbol("test.".to_string());
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Test.Foo"]);
        assert_eq!(symbols[0].container_name, None);
        let symbols = workspace.workspace_symbol("bar".to_string());
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Bar"]);
        assert_eq!(symbols[0].location.uri, "Test.Foo.cls");
        assert_eq!(
            symbols[0].container_name.as_deref(),
            Some("ClassMethod • Test.Foo")
        );
        assert_eq!(workspace.workspace_symbol(String::new()).len(), 3);
    }

    #[test]
    fn document_symbol_returns_routine_and_labels() {
        let workspace = Workspace::new();
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
    fn workspace_symbol_finds_routines_and_labels_by_name() {
        let workspace = Workspace::new();
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
        assert_eq!(names, ["Test"]);
        let symbols = workspace.workspace_symbol("foo".to_string());
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Foo"]);
        assert_eq!(symbols[0].location.uri, "Test.mac");
        assert_eq!(symbols[0].container_name.as_deref(), Some("Label • Test"));
    }

    #[test]
    fn workspace_symbol_parses_a_never_checked_matching_routine() {
        let workspace = Workspace::new();
        // Opened but never otherwise analyzed (no diagnostics/document_symbol/etc. called) --
        // a name match still gets full detail (its label included), same as a class: tier A
        // (the name) is always available for free, but a *match* is worth paying for tier B.
        workspace.open(
            "Matched.mac".to_string(),
            "ROUTINE MatchedRoutine\n\n\tQuit\n\nFoo\t;\n\tQuit\n".to_string(),
        );

        let symbols = workspace.workspace_symbol("o".to_string());
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["MatchedRoutine", "Foo"]);
    }

    #[test]
    fn hover_member_on_a_call_shows_the_callee_not_the_caller() {
        let workspace = Workspace::new();
        let uri = "HoverCallee.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.HoverCallee {\nClassMethod Foo() {\n do ..Bar()\n}\nClassMethod Bar() {\n}\n}\n"
                .to_string(),
        );
        assert_clean(&workspace, &uri);
        // Character 7 on line 2 (` do ..Bar()`) is on "Bar", inside `Foo`'s own body.
        let member = workspace
            .hover_member(uri, (2, 7).into())
            .expect("expected a member at that position");
        assert_eq!(member.name, "Bar");
    }

    #[test]
    fn hover_member_returns_member_at_position() {
        let workspace = Workspace::new();
        let uri = "Hover.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Hover {\nClassMethod Foo() {\n}\n}\n".to_string(),
        );
        let member = workspace
            .hover_member(uri.clone(), (1, 5).into())
            .expect("expected a member at that position");
        assert_eq!(member.name, "Foo");
        assert!(workspace.hover_member(uri, (0, 0).into()).is_none());
    }

    #[test]
    fn hover_renders_member_as_markdown() {
        let workspace = Workspace::new();
        let uri = "Hover.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Hover {\nClassMethod Foo() {\n}\n}\n".to_string(),
        );
        let markdown = workspace
            .hover(uri, (1, 5).into())
            .expect("expected hover text at that position");
        assert_eq!(markdown, "(**Test.Hover**) ClassMethod <u>**Foo**</u>()");
    }

    #[test]
    fn hover_shows_both_type_and_macro_expansion_when_both_apply() {
        let workspace = Workspace::new();
        let uri = "MacroArg.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n#define X(%a) %a\n\n\tSet x = 1\n\tWrite $$$X(x)\n\tQuit\n"
                .to_string(),
        );
        assert_clean(&workspace, &uri);

        // `x` is a macro argument: a hover type and a macro expansion both apply.
        let position = (5, 12).into();
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
        let (workspace, base_uri, sub_uri) = base_and_sub();

        let link = workspace
            .definition(sub_uri, (2, 24).into())
            .expect("expected a definition at that position");
        assert_eq!(link.target_uri, base_uri);
        // The name, `Foo` in `ClassMethod Foo() {`, is selected; the whole member is revealed.
        assert_eq!(span(link.target_selection_range), ((1, 12), (1, 15)));
        assert_eq!(span(link.target_range), ((1, 0), (2, 1)));
        assert_eq!(
            link.origin_selection_range.map(span),
            Some(((2, 23), (2, 26)))
        );
    }

    #[test]
    fn row_instance_percent_accesses_property_like_instance_percent() {
        let workspace = mock_iris(Some(true), None);
        let uri = "RPercent.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.RPercent {\nProperty P1 As %Numeric;\nMethod Get() As %Numeric {\n return r%P1\n}\nClassMethod ClassGet() As %Numeric {\n return r%P1\n}\n}\n"
                .to_string(),
        );
        let diagnostics = workspace.diagnostics(uri.clone());
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("instance member"),
            "{diagnostics:?}"
        );
        assert_eq!(diagnostics[0].range.start.line, 6);

        // `r%P1` inside the instance method (line 3) should resolve like `..P1`/`i%P1` —
        // go-to-definition finds the property declaration.
        let link = workspace
            .definition(uri, (3, 10).into())
            .expect("expected a definition for `r%P1`");
        assert_eq!(link.target_selection_range.start.line, 1);
    }

    #[test]
    fn definition_resolves_bare_class_reference() {
        let (workspace, base_uri, sub_uri) = base_and_sub();

        // Position inside "Base", within the `##class(Test.Base)` literal itself.
        let link = workspace
            .definition(sub_uri, (2, 18).into())
            .expect("expected a definition at that position");
        assert_eq!(link.target_uri, base_uri);
        assert_eq!(span(link.target_selection_range), ((0, 6), (0, 15)));
        assert_eq!(span(link.target_range), ((0, 0), (4, 0)));
    }

    #[test]
    fn definition_resolves_a_class_reference_via_the_iris_hosts_own_uri() {
        struct ExternalClassIris;
        impl crate::IrisContext for ExternalClassIris {
            fn get_cls(&self, cls: &str) -> Result<Option<bindings::ClassInfo>, String> {
                Ok((cls == "Test.External").then(|| bindings::ClassInfo {
                    doc: String::new(),
                    name: cls.to_string(),
                    uri: Some("mock:///External.cls".to_string()),
                    ..Default::default()
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
        let link = workspace
            .definition(uri, (2, 18).into())
            .expect("expected a definition at that position");
        assert_eq!(link.target_uri, "mock:///External.cls");
        // Nothing of a document outside the workspace is known beyond the host's uri.
        assert_eq!(link.target_range, link.target_selection_range);
    }

    #[test]
    fn definition_resolves_same_routine_label_call() {
        let workspace = Workspace::new();
        let uri = "Test.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\tDo Main()\n\tQuit\n\nMain()\n do Sub()\n\tQuit\n\nSub()\n\tQuit\n"
                .to_string(),
        );

        let link = workspace
            .definition(uri.clone(), (5, 5).into())
            .expect("expected a definition at that position");
        assert_eq!(span(link.target_selection_range), ((8, 0), (8, 3)));
        assert_eq!(span(link.target_range), ((8, 0), (9, 5)));
        // Not into the blank line before the next label.
        let link = workspace
            .definition(uri, (1, 5).into())
            .expect("expected a definition at that position");
        assert_eq!(span(link.target_selection_range), ((4, 0), (4, 4)));
        assert_eq!(span(link.target_range), ((4, 0), (6, 5)));
    }

    #[test]
    fn definition_resolves_procedure_block_label_call() {
        let workspace = Workspace::new();
        let uri = "Test.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\tDo Sub()\n\tQuit\n\nSub() {\n\tQuit\n}\n\nOther()\n\tQuit\n"
                .to_string(),
        );

        let link = workspace
            .definition(uri, (1, 5).into())
            .expect("expected a definition at that position");
        assert_eq!(span(link.target_selection_range), ((4, 0), (4, 3)));
        assert_eq!(span(link.target_range), ((4, 0), (6, 1)));
    }

    #[test]
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
                    "Compiled" => Some(bindings::RoutineInfo::default()),
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
                    Information,
                    "The labels of routine 'Compiled' aren't known, so calls into it can't be checked precisely"
                        .to_string()
                ),
                (
                    Error,
                    "Too many arguments: the callee takes no arguments".to_string()
                ),
            ]
        );
    }

    #[test]
    fn a_class_declaration_is_read_whole() {
        let details = ClassDetails::try_from(
            "/// Doc\nClass T.K Extends (%Persistent, %XML.Adaptor) [ Abstract, ClassType = persistent, ClientName = TK, CompileAfter = (A.B, C.D), DdlAllowed, Deprecated, Final, Inheritance = right, Language = tsql, Not NoExtent, Owner = _SYSTEM, Not ProcedureBlock, PropertyClass = (P.Q, R.S), SqlTableName = T_K, System = 3, Bogus, ClassType = Persistent, Inheritance = middle, CompileAfter = , Finalfoo, Abstract = 0 ]\n{\n}\n",
        )
        .unwrap();
        let k = &details.keywords;
        assert!(k.deprecated);
        assert_eq!(k.class_type, Some(bindings::ClassType::Persistent));
        assert_eq!(k.inheritance, Some(bindings::Inheritance::Right));
        assert_eq!(k.language, Some(bindings::Language::Tsql));
        assert_eq!(k.procedure_block, Some(false));
        assert_eq!(k.property_class, ["P.Q", "R.S"]);
        let issues: Vec<_> = details
            .keyword_issues
            .iter()
            .map(|issue| issue.it.entry.as_str())
            .collect();
        assert_eq!(
            issues,
            [
                "Bogus",
                "ClassType = Persistent",
                "Inheritance = middle",
                "CompileAfter = ",
                "Finalfoo",
                "Abstract = 0",
            ]
        );
    }

    #[test]
    fn an_index_declaration_is_read_whole() {
        let details = ClassDetails::try_from(
            "Class T.I Extends %Persistent\n{\nProperty Name As %String;\nProperty Age As %Integer;\nProperty Tags As list Of %String;\nIndex Full On (Name As Exact, Tags(ELEMENTS)) [ Data = (Name, Age), Condition = {Age > 5}, SqlName = FULL_IDX, Internal, Unique, Type = bitslice ];\nIndex Ext [ Extent ];\n}\n",
        )
        .unwrap();
        let bindings::MemberKind::Index(full) =
            bindings::MemberInfo::from(details.member(details::MemberNs::Index, "Full").unwrap())
                .kind
        else {
            panic!()
        };
        let key = |property: &str, subscript, collation: Option<&str>| bindings::IndexKey {
            property: property.to_string(),
            subscript,
            collation: collation.map(str::to_string),
        };
        assert_eq!(
            full.on.as_deref(),
            Some(
                &[
                    key("Name", None, Some("Exact")),
                    key("Tags", Some(bindings::IndexSubscript::Elements), None),
                ][..]
            )
        );
        assert_eq!(full.data, ["Name", "Age"]);
        assert_eq!(full.condition.as_deref(), Some("Age > 5"));
        assert_eq!(full.sql_name.as_deref(), Some("FULL_IDX"));
        assert_eq!(full.type_, Some(bindings::IndexType::Bitslice));
        assert!(full.internal && full.unique && !full.primary_key && !full.id_key && !full.extent);
        let bindings::MemberKind::Index(ext) =
            bindings::MemberInfo::from(details.member(details::MemberNs::Index, "Ext").unwrap())
                .kind
        else {
            panic!()
        };
        assert!(ext.on.is_none() && ext.extent && ext.type_.is_none() && ext.data.is_empty());
    }

    #[test]
    fn an_undecidable_if_is_information_and_takes_its_first_branch() {
        let diagnostics = diagnostics_for(&[(
            "If.mac",
            "ROUTINE If\n#define MODE \"OUTPUT\"\nMain()\n\t#if $$$MODE=\"OUTPUT\" ; which\n\tSet x = 1\n\t#else\n\tQuit\n\t#endif\n\tWrite x\n\tQuit\n",
        )]);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(
            diagnostics[0].severity,
            bindings::DiagnosticSeverity::Information
        );
        assert_eq!(
            diagnostics[0].message,
            "Cannot decide `#if $$$MODE=\"OUTPUT\"` (`\"OUTPUT\"=\"OUTPUT\"`) at compile time, so its branch is assumed taken"
        );
        assert_eq!(diagnostics[0].range.start.line, 3, "{diagnostics:?}");
    }

    #[test]
    fn a_failed_iris_lookup_is_information_about_why() {
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
                bindings::DiagnosticSeverity::Information,
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
        let workspace = Workspace::new();
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

        let link = workspace
            .definition(uri, (2, 5).into())
            .expect("expected a definition at that position");
        assert_eq!(link.target_uri, callee_uri);
        assert_eq!(span(link.target_selection_range), ((4, 0), (4, 3)));
        assert_eq!(span(link.target_range), ((4, 0), (5, 5)));
    }

    #[test]
    fn definition_resolves_cross_routine_label_after_include() {
        let workspace = Workspace::new();
        workspace.open(
            "Shared.inc".to_string(),
            "ROUTINE Shared [Type=INC]\n// one\n// two\n// three\n".to_string(),
        );
        let callee_uri = "Callee.mac".to_string();
        workspace.open(
            callee_uri.clone(),
            "ROUTINE Callee\n\n#include Shared\n\tQuit\n\nFoo()\n\tQuit\n".to_string(),
        );
        let uri = "Main.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Main\n\n\tDo Foo^Callee()\n\tQuit\n".to_string(),
        );

        let link = workspace
            .definition(uri, (2, 5).into())
            .expect("expected a definition at that position");
        assert_eq!(link.target_uri, callee_uri);
        // In the callee's own lines, not its expansion's, which the include lengthens.
        assert_eq!(span(link.target_selection_range), ((5, 0), (5, 3)));
        assert_eq!(span(link.target_range), ((5, 0), (6, 5)));
    }

    #[test]
    fn definition_resolves_goto_target() {
        let workspace = Workspace::new();
        let uri = "Test.mac".to_string();
        // No `Quit` before the `Goto` — a bare fallthrough would be flagged, and `where` would
        // be flagged as unread unless `Goto` (like `Do`) is treated as sharing scope with its
        // target rather than as this flow just ending on its own.
        workspace.open(
            uri.clone(),
            "ROUTINE Test\n\n\tQuit\n\ntables\t;\n\tn where s where=1\n\tg tref\n\ntref\t;\n\tQuit\n"
                .to_string(),
        );

        assert_clean(&workspace, &uri);

        let link = workspace
            .definition(uri, (6, 3).into())
            .expect("expected a definition at that position");
        assert_eq!(span(link.target_selection_range), ((8, 0), (8, 4)));
        assert_eq!(span(link.target_range), ((8, 0), (9, 5)));
    }

    #[test]
    fn definition_resolves_label_after_include() {
        let workspace = Workspace::new();
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

        assert_clean(&workspace, &mac_uri);

        let link = workspace
            .definition(mac_uri.clone(), (6, 5).into())
            .expect("expected a definition at that position");
        assert_eq!(link.target_uri, mac_uri);
        // In the routine's own lines, not the expansion's, which the include lengthens.
        assert_eq!(span(link.target_selection_range), ((9, 0), (9, 3)));
        assert_eq!(span(link.target_range), ((9, 0), (10, 5)));
    }

    #[test]
    fn references_finds_all_call_sites_from_either_end() {
        let workspace = Workspace::new();
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

        let call_site = (2, 24).into();
        let definition = target(
            &workspace
                .definition(sub_uri.clone(), call_site)
                .expect("expected a definition at that position"),
        );

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
    fn references_include_extends_and_declared_types() {
        let workspace = Workspace::new();
        let (base_uri, sub_uri) = ("Base.cls".to_string(), "Sub.cls".to_string());
        workspace.open(base_uri.clone(), "Class Test.Base {\n}\n".to_string());
        workspace.open(
            sub_uri.clone(),
            "Class Test.Sub Extends Test.Base {\nProperty P As Test.Base;\nProperty L As list Of Test.Base;\nMethod M(a As Test.Base) As Test.Base {\n}\n}\n"
                .to_string(),
        );
        let at = |line, character| bindings::Position { line, character };
        let spans = |locations: &[bindings::Location]| {
            let mut spans: Vec<_> = locations
                .iter()
                .map(|l| {
                    (
                        l.range.start.line,
                        l.range.start.character,
                        l.range.end.character,
                    )
                })
                .collect();
            spans.sort();
            spans
        };
        let from_header = workspace.references(base_uri.clone(), at(0, 8), false);
        assert!(
            from_header.iter().all(|l| l.uri == sub_uri),
            "{from_header:?}"
        );
        assert_eq!(
            spans(&from_header),
            [
                (0, 23, 32),
                (1, 14, 23),
                (2, 22, 31),
                (3, 14, 23),
                (3, 28, 37)
            ]
        );
        let from_extends = workspace.references(sub_uri.clone(), at(0, 25), false);
        assert_eq!(spans(&from_extends), spans(&from_header));
        let definition = target(
            &workspace
                .definition(sub_uri.clone(), at(0, 25))
                .expect("the Extends clause names the class"),
        );
        assert_eq!(definition.uri, base_uri);
        let with_declaration = workspace.references(sub_uri, at(0, 25), true);
        assert!(with_declaration.contains(&definition));
    }

    #[test]
    fn definition_resolves_self_member_reference() {
        let workspace = Workspace::new();
        let uri = "Sub.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.Sub {\nClassMethod Bar() {\n do ..Baz()\n}\nClassMethod Baz() {\n quit\n}\n}\n"
                .to_string(),
        );

        let link = workspace
            .definition(uri, (2, 7).into())
            .expect("expected a definition at that position");
        assert_eq!(link.target_selection_range.start.line, 4);
        assert_eq!(span(link.target_range), ((4, 0), (6, 1)));
    }

    // Every class gets one method: `%SQLGatewayConnection`'s `nextResultSet`, returning `t`.
    fn gateway_iris(t: &str) -> Workspace {
        mock_iris(
            None,
            Some(bindings::MemberInfo {
                doc: String::new(),
                name: "nextResultSet".to_string(),
                aliases: vec![],
                deprecated: false,
                final_: false,
                kind: bindings::MemberKind::Method(bindings::Signature {
                    normal: vec![],
                    variadic: None,
                    t: Some(t.to_string()),
                    procedure_block: None,
                    sql: None,
                }),
                uri: None,
                origin: Some("%Library.SQLGatewayConnection".to_string()),
            }),
        )
    }

    // Without an IRIS connection, members of system classes only get "cannot confirm" hints.
    fn errors_in(workspace: &Workspace, uri: &str, text: &str) -> Vec<bindings::Diagnostic> {
        workspace.open(uri.to_string(), text.to_string());
        let mut diagnostics = workspace.diagnostics(uri.to_string());
        diagnostics.retain(|d| d.severity != bindings::DiagnosticSeverity::Hint);
        diagnostics
    }

    // A declaration may type a method's result as `This`: the receiver's class.
    #[test]
    fn a_declared_this_is_the_receiver() {
        let workspace = gateway_iris("This");
        let diagnostics = errors_in(
            &workspace,
            "T.cls",
            "Class Test.T Extends %RegisteredObject {\nMethod M(c As %SQLGatewayConnection) As %String {\n Quit c.nextResultSet().Next()\n}\n}\n",
        );
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Cannot find member 'Next' in %SQLGatewayConnection"),
            "{diagnostics:?}"
        );
    }

    // A declaration's `= …` makes the argument optional without saying what it is when left out.
    // The mask also corrects the `%OnNew` that `%New` takes its arguments from.
    // Without its own `%OnNew`, `%New` takes its own arguments: an optional `initvalue`.
    #[test]
    fn an_unmasked_object_handle_stays_one() {
        let workspace = gateway_iris("%Library.ObjectHandle");
        let diagnostics = errors_in(
            &workspace,
            "T.cls",
            "Class Test.T Extends %RegisteredObject {\nMethod M(c As %SQLGatewayConnection) As %String {\n Set rs = c.nextResultSet()\n Quit rs.Next()\n}\n}\n",
        );
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("Cannot find member 'Next' in %ObjectHandle"),
            "{diagnostics:?}"
        );
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
    fn set_style_rechecks_every_document_against_the_new_style() {
        let style = style::parse(r#"{"command": {"form": "long", "case": "pascal"}}"#).unwrap();
        let workspace = Workspace::new();
        let uri = "Style.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Style\nf(x)\n s y=x\n Quit y\n".to_string(),
        );
        assert!(style_messages(&workspace, &uri).is_empty());
        assert_eq!(workspace.set_style(Some(style)), std::slice::from_ref(&uri));
        assert_eq!(
            style_messages(&workspace, &uri),
            ["2:1 Command keyword `s` should be written `Set`"]
        );
        assert_eq!(workspace.set_style(None), std::slice::from_ref(&uri));
        assert!(style_messages(&workspace, &uri).is_empty());
    }

    fn styled(json: &str) -> Workspace {
        Workspace::native(None, Some(style::parse(json).unwrap()))
    }

    #[test]
    fn command_keywords_follow_the_configured_style() {
        let workspace = styled(r#"{"command": {"form": "long", "case": "pascal"}}"#);
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
    fn a_style_finding_in_a_method_body_is_a_quick_fix() {
        let workspace = styled(r#"{"command": {"case": "pascal"}}"#);
        let uri = "Fix.cls".to_string();
        let text = "Class Test.Fix {\nClassMethod Foo() {\n set x = 1\n}\n}\n";
        workspace.open(uri.clone(), text.to_string());
        let at = |line, character| bindings::Range::point((line, character).into());
        let actions = workspace.code_actions(uri.clone(), at(2, 2));
        assert_eq!(actions.len(), 1, "{actions:?}");
        let action = &actions[0];
        assert_eq!(action.kind.as_deref(), Some("quickfix"));
        assert_eq!(action.diagnostics.len(), 1);
        assert_eq!(action.diagnostics[0].code, "S2839");
        assert_eq!(action.title, action.diagnostics[0].message);
        let changes = &action.edit.as_ref().unwrap().changes;
        assert_eq!(changes.len(), 1);
        let (edited, edits) = &changes[0];
        assert_eq!((edited.as_str(), edits.len()), (uri.as_str(), 1));
        let olc = OLC::new(text);
        let (start, end) = (
            olc.offset(edits[0].range.start),
            olc.offset(edits[0].range.end),
        );
        assert_eq!(
            (&text[start..end], edits[0].new_text.as_str()),
            ("set", "Set")
        );
        assert_eq!(workspace.format(uri.clone(), OPTIONS).as_ref(), Some(edits));
        assert!(workspace.code_actions(uri, at(1, 0)).is_empty());
    }

    #[test]
    fn a_quick_fix_indents_a_line_it_adds_as_the_document_does() {
        let workspace = styled(r#"{"openBrace": {"newlineAfter": true}}"#);
        let uri = "Fix.cls".to_string();
        let text = "Class Test.Fix {\nClassMethod Foo(x)\n{\n    If x { Set y = 1 }\n}\n}\n";
        workspace.open(uri.clone(), text.to_string());
        let actions = workspace.code_actions(uri.clone(), bindings::Range::point((3, 11).into()));
        assert_eq!(actions.len(), 1, "{actions:?}");
        assert_eq!(actions[0].diagnostics[0].code, "S4933");
        let edits = &actions[0].edit.as_ref().unwrap().changes[0].1;
        assert_eq!(edits[0].new_text, "\n        ");
        assert_eq!(workspace.format(uri, OPTIONS).as_ref(), Some(edits));
    }

    #[test]
    fn a_style_finding_outside_a_procedure_block_has_no_quick_fix() {
        let workspace = styled(r#"{"command": {"case": "pascal"}}"#);
        let uri = "Plain.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Plain\nf\n set x = 1\n Quit\n".to_string(),
        );
        let diagnostics = workspace.diagnostics(uri.clone());
        let found = diagnostics.iter().find(|d| d.code == "S2839");
        let found = found.unwrap_or_else(|| panic!("{diagnostics:?}"));
        assert!(workspace.code_actions(uri, found.range).is_empty());
    }

    #[test]
    fn a_brace_never_joins_a_line_ending_in_a_macro_call() {
        let workspace = styled(r#"{"closeBrace": {"newlineBefore": false}}"#);
        let uri = "Macro.cls".to_string();
        let text = "Class Test.Macro {\nClassMethod Foo(x)\n{\n If x {\n  Set y = 1\n }\n If x {\n  $$$Macro(1)\n }\n If x {\n  Set y = $$$Macro(1)\n }\n}\n}\n";
        workspace.open(uri.clone(), text.to_string());
        let diagnostics = workspace.diagnostics(uri.clone());
        let braces: Vec<_> = (diagnostics.iter())
            .filter(|d| d.code == "S3627")
            .map(|d| d.range.start.line)
            .collect();
        assert_eq!(braces, [5, 8, 11], "{diagnostics:?}");
        let edits = workspace.format(uri, OPTIONS).unwrap();
        assert_eq!(edits.len(), 1, "{edits:?}");
        assert_eq!(
            (span(edits[0].range), edits[0].new_text.as_str()),
            (((4, 11), (5, 1)), " ")
        );
    }

    #[test]
    fn functions_follow_the_configured_style() {
        let workspace = styled(
            r#"{"function": {"form": "long", "case": "pascal"}, "functionListbuild": {"form": "short"}}"#,
        );
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
        let workspace = styled(r#"{"function": {"form": "long", "case": "pascal"}}"#);
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
        let workspace = styled(r#"{"function": {"case": "upper"}}"#);
        let uri = "T.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.T {\nClassMethod M(x) As %String [ CodeMode = expression ] {\n$Piece(x,\",\")\n}\n}\n".to_string(),
        );
        let mut messages = style_messages(&workspace, &uri);
        messages.retain(|m| !m.contains("Cannot find class"));
        assert_eq!(
            messages,
            ["2:1 Function `$Piece` should be written `$PIECE`"]
        );
    }

    // Every diagnostic carries its kind's code and a link to that kind's docs, including the ones
    // raised outside the checker proper, such as a declared type's missing class.
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
                find("Cannot find class"),
                check::TypeErrorKind::CannotFindClass { cls: String::new() },
                "CannotFindClass",
            ),
        ];
        for (diagnostic, kind, _) in cases {
            assert_eq!(diagnostic.code, kind.code());
            assert_eq!(
                diagnostic.code_description,
                format!(
                    "https://intersystems-community.github.io/ascot/{}/{}",
                    env!("CARGO_PKG_VERSION"),
                    kind.code()
                )
            );
        }
    }

    fn class_diagnostics(body: &str) -> Vec<bindings::Diagnostic> {
        let text = format!("Class Test.F Extends %RegisteredObject {{\n{body}\n}}\n");
        diagnostics_for(&[("F.cls", &text)])
    }

    fn diagnostics_for(files: &[(&str, &str)]) -> Vec<bindings::Diagnostic> {
        let workspace = Workspace::new();
        for (uri, text) in files {
            workspace.open(uri.to_string(), text.to_string());
        }
        workspace.diagnostics(files[0].0.to_string())
    }

    // IRIS compiles methods in name order and keeps a body's `#define` for the ones after it:
    // B sees A's macro (with a warning), A doesn't see Z's.
    #[test]
    fn method_body_defines_reach_the_methods_compiled_after() {
        let mut diagnostics = class_diagnostics(
            "ClassMethod Z() As %String {\n #define LATE 1\n Quit $$$LATE\n}\nClassMethod A() As %String {\n #define REPORT(%x) Write %x\n $$$REPORT(1)\n Quit $$$LATE\n}\nClassMethod B() As %String {\n $$$REPORT(2)\n Quit \"\"\n}\n",
        );
        diagnostics.retain(|d| {
            d.message
                != "Cannot find class '%Library.String' in the workspace or on the IRIS instance"
        });
        // Two about the macros, and the superclass no environment can confirm.
        assert_eq!(diagnostics.len(), 3, "{diagnostics:?}");
        let undefined = diagnostics
            .iter()
            .find(|d| d.message.contains("'LATE' is undefined"));
        assert_eq!(
            undefined.map(|d| d.range.start.line),
            Some(8),
            "{diagnostics:?}"
        );
        let leaked = diagnostics
            .iter()
            .find(|d| {
                d.message
                    .contains("'REPORT' is defined in the body of method 'A'")
            })
            .expect("warning");
        assert_eq!(leaked.severity, bindings::DiagnosticSeverity::Warning);
        assert_eq!(leaked.range.start.line, 11, "{diagnostics:?}");
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
        let workspace = Workspace::new();
        let uri = "Short.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Short\n#define X 1\n\twrite $$$X\n\tquit\n".to_string(),
        );
        let position = (100, 0).into();
        assert!(workspace.hover(uri.clone(), position).is_none());
        assert!(workspace.macro_expansion(uri, position).is_none());
    }

    // A `#dim` at column 0 (legal in a routine) is analyzed exactly like an indented one.
    #[test]
    fn dim_at_column_zero_in_a_routine_is_checked_in_place() {
        let body =
            "#dim x As %String\n\tWrite x\n\tQuit\nlbl\n#dim y As %Integer\n\tWrite y\n\tQuit\n";
        let workspace = Workspace::new();
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
        let workspace = Workspace::new();
        let uri = "G.cls".to_string();
        workspace.open(
            uri.clone(),
            "Class Test.G {\nClassMethod G(a As Test.G) {\n If $R(2) { Kill a }\n Set b = $G(a)\n Write b\n}\n}\n".to_string(),
        );
        let hover = workspace
            .hover_type(uri, (4, 7).into())
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
        let workspace = Workspace::new();
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
        let workspace = Workspace::new();
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
        let workspace = Workspace::new();
        workspace.open(
            "C.cls".to_string(),
            "Class Test.C {\nClassMethod M() {\n Do ##class(Test.Foo).Baz()\n}\n}\n".to_string(),
        );
        let foo = (2, 15).into();
        let definition = |ws: &Workspace| {
            ws.definition("C.cls".to_string(), foo)
                .map(|l| l.target_uri)
        };
        workspace.open("A.cls".to_string(), "Class Test.Foo {\n}\n".to_string());
        assert_eq!(definition(&workspace), Some("A.cls".to_string()));
        // Unparseable for a while (mid-edit), then renamed: `Test.Foo` is nobody's.
        workspace.open("A.cls".to_string(), "Clas Test.Foo {\n".to_string());
        workspace.open("A.cls".to_string(), "Class Test.Bar {\n}\n".to_string());
        assert_eq!(definition(&workspace), None);
    }

    #[test]
    fn closing_one_of_two_same_named_classes_leaves_the_other_indexed() {
        let workspace = Workspace::new();
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
        let foo = (2, 15).into();
        assert_eq!(
            workspace
                .definition("C.cls".to_string(), foo)
                .map(|l| l.target_uri),
            Some("B.cls".to_string())
        );
    }

    // `%flag` inside the pasted argument is the caller's variable, not `Has`'s second parameter.
    #[test]
    fn macro_arguments_are_not_rescanned_for_other_parameters() {
        let workspace = Workspace::new();
        let uri = "Sub.mac".to_string();
        workspace.open(
            uri.clone(),
            "ROUTINE Sub\n#define Wrap(%flag) $$Wrap^Other(%flag)\n#define Has(%flags,%flag) (%flags[%flag)\n\tWrite $$$Has($$$Wrap($g(%flag)),\"D\")\n\tQuit\n".to_string(),
        );
        let expansion = workspace
            .macro_expansion(uri, (3, 8).into())
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
        let range = span(expands_to.location.range);
        assert_eq!(range, ((5, 4), (5, 19)), "{diagnostics:?}");
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
        let range = span(expands_to.location.range);
        assert_eq!(expands_to.location.uri, "Lib.inc", "{diagnostics:?}");
        assert_eq!(range, ((2, 4), (2, 19)), "{diagnostics:?}");
    }

    // Both land right after the closing brace; the `end while` one used to be pushed one
    // character further, past the newline, onto the next line.
    #[test]
    fn loop_end_hints_sit_after_the_closing_brace() {
        let workspace = Workspace::new();
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
        let workspace = Workspace::new();
        let inc = "ROUTINE Test.Shared [Type=INC]\n#define X 1\n";
        let cls = "Include (Test.Other, Test.Shared)\n\nClass Test.C {\nClassMethod M() {\n Write $$$X\n}\n}\n";
        let mac = "ROUTINE R\n#include Test.Shared\n\tWrite $$$X\n\tQuit\n";
        for (uri, text) in [("Shared.inc", inc), ("C.cls", cls), ("R.mac", mac)] {
            workspace.open(uri.to_string(), text.to_string());
        }
        let expected = bindings::Location::new(
            "Shared.inc".to_string(),
            bindings::Range {
                start: (0, 8).into(),
                end: (0, 19).into(),
            },
        );
        let at = |uri: &str, line, character| {
            workspace
                .definition(uri.to_string(), bindings::Position { line, character })
                .map(|link| {
                    assert_eq!(span(link.target_range), ((0, 0), (2, 0)));
                    target(&link)
                })
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
}
