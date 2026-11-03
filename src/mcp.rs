//! The MCP front end (`ascot mcp`): a stdio Model Context Protocol server exposing workspace
//! analysis as tools for AI agents (Claude Code via the `ascot-mcp` plugin, or `claude mcp add`).
//!
//! Unlike the LSP server it holds many `Workspace`s, keyed by an id the agent chooses:
//! `create_workspace` (optionally with IRIS connection details, see `iris.rs`) and
//! `delete_workspace`; `insert`/`remove` documents; then `check`, `inlay_hint`, `hover`,
//! `definition` and `references`. Each tool is a method of [`Server`], whose doc
//! comment is also the description agents get from `tools/list`; parameter and result schemas
//! derive from these types and the `bindings.rs` ones via `schemars`, field docs included. Built
//! on the `rmcp` crate; native builds only.

use std::{collections::HashMap, sync::Arc};

use rmcp::{
    ServerHandler,
    handler::server::{
        router::tool::ToolRouter,
        wrapper::{Json, Parameters},
    },
    model::{ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::{
    Workspace,
    bindings::{Diagnostic, GuestWorkspace, InlayHint, Location, Position, Range},
    iris, style,
};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WorkspaceIdRequest {
    /// The id given to `create_workspace`.
    pub workspace: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateWorkspaceRequest {
    /// An id of your choice; every other tool names the workspace by it.
    pub workspace: String,
    /// Optional IRIS connection: `scheme://user:pass@host:port`, or the path of a declaration
    /// file written by `ascot declare`. When set, lookups that can't be resolved from documents
    /// already in this workspace (e.g. a class that isn't open here) fall back to this IRIS
    /// instance via the Atelier REST API, or to the file.
    pub iris: Option<String>,
    /// IRIS namespace to query against. Only meaningful when `iris` is set. Defaults to `%SYS`
    /// for a URL; for a declaration file it must match the namespace it declares.
    pub iris_namespace: Option<String>,
    /// Workspace-wide style rules, e.g. `{"command": {"form": "Long"}, "command_do": {"form": "Short"}}` to
    /// have every command keyword but `Do`'s written in full; `function` and `function_<name>` do
    /// the same for intrinsic functions. Applies to every document. The keys are documented at
    /// https://github.com/intersystems-community/ascot#style.
    #[schemars(with = "Option<serde_json::Map<String, serde_json::Value>>")]
    pub style: Option<serde_json::Value>,
    /// Which codes are reported, and at what severity: `VERB:TARGETS` strings applied in order, a
    /// later one overriding an earlier one. VERB is `off`, `on` (at the code's own severity) or a
    /// severity (`error`, `warning`, `information`, `hint`); TARGETS is a comma-separated list of
    /// codes, or `all`. For example `["off:A5579,A4043"]`, or `["off:all", "on:A7095,A6851"]`.
    pub diagnostics: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FileToInsert {
    /// Names the document, e.g. its file path; the extension (.cls, .mac, .int or .inc) decides
    /// how it is parsed.
    pub uri: String,
    /// The document's full text. When the server was started with `--shared-fs`, omit it to read
    /// `uri`, a `file://` url, from disk instead: a file is read as a document, a directory loads
    /// every .cls/.mac/.int/.inc file under it and drops those it no longer holds.
    pub src: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InsertRequest {
    /// The id given to `create_workspace`.
    pub workspace: String,
    /// The documents to add or update.
    pub files: Vec<FileToInsert>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UriRequest {
    /// The id given to `create_workspace`.
    pub workspace: String,
    /// A document's uri, as given to `insert`.
    pub uri: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UrisRequest {
    /// The id given to `create_workspace`.
    pub workspace: String,
    /// The documents to check, by the uris given to `insert`.
    pub uris: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InlayHintRequest {
    /// The id given to `create_workspace`.
    pub workspace: String,
    /// A document's uri, as given to `insert`.
    pub uri: String,
    /// The zero-based line/character range to get hints for.
    pub range: Range,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PositionRequest {
    /// The id given to `create_workspace`.
    pub workspace: String,
    /// A document's uri, as given to `insert`.
    pub uri: String,
    /// A zero-based line/character position in the document.
    pub position: Position,
}

// MCP requires `outputSchema` to describe an object at the top level, so results that are
// naturally an array or an enum get wrapped in a single-field struct rather than returned bare.

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct FileDiagnostics {
    pub uri: String,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DiagnosticsResult {
    pub files: Vec<FileDiagnostics>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct InlayHintsResult {
    pub hints: Vec<InlayHint>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct InsertResult {
    pub affected: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct HoverResult {
    pub hover: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DefinitionResult {
    pub location: Option<Location>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReferencesResult {
    pub locations: Vec<Location>,
}

#[derive(Clone)]
pub struct Server {
    workspaces: Arc<RwLock<HashMap<String, Arc<Mutex<Workspace>>>>>,
    tool_router: ToolRouter<Self>,
    // Whether `insert` may read a uri from disk: only when the server was told it shares the
    // client's file system, since over MCP that is nothing to assume.
    shared_fs: bool,
}

impl Default for Server {
    fn default() -> Self {
        Self::new(false)
    }
}

impl Server {
    pub fn new(shared_fs: bool) -> Self {
        Self {
            workspaces: Arc::new(RwLock::new(HashMap::new())),
            tool_router: Self::tool_router(),
            shared_fs,
        }
    }

    // Reads `uri` from disk into `workspace`: a file, or a directory whose documents are all
    // (re)read, dropping the ones it no longer holds. Returns the uris now stale.
    fn open_from_disk(&self, workspace: &Workspace, uri: &str) -> Result<Vec<String>, String> {
        if !self.shared_fs {
            return Err(format!(
                "{uri}: no `src` given, and the server can only read a uri from disk when started with --shared-fs"
            ));
        }
        let path = url::Url::parse(uri)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .filter(|path| path.exists())
            .ok_or_else(|| {
                format!("{uri}: not the file:// url of an existing file or directory")
            })?;
        let (opened, mut affected) = workspace.open_directory(&path);
        if path.is_dir() {
            let opened: std::collections::HashSet<String> = opened.into_iter().collect();
            let prefix = format!(
                "{}/",
                url::Url::from_file_path(&path)
                    .map_err(|()| uri.to_string())?
                    .as_str()
                    .trim_end_matches('/')
            );
            for stale in workspace.uris() {
                if stale.starts_with(&prefix) && !opened.contains(&stale) {
                    affected.extend(workspace.close(stale));
                }
            }
        }
        Ok(affected)
    }

    async fn get_workspace(&self, id: &str) -> Result<Arc<Mutex<Workspace>>, String> {
        self.workspaces
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| format!("no such workspace: {id:?}"))
    }
}

#[tool_router(router = tool_router)]
impl Server {
    /// Create a new empty workspace with the given id; fails if it already exists. Do this first,
    /// before inserting any ObjectScript source. Pass `iris` to also resolve out-of-workspace
    /// lookups against a real IRIS instance (or a declaration file `ascot declare` wrote).
    #[tool]
    pub async fn create_workspace(
        &self,
        Parameters(CreateWorkspaceRequest {
            workspace,
            iris,
            iris_namespace,
            style,
            diagnostics,
        }): Parameters<CreateWorkspaceRequest>,
    ) -> Result<String, String> {
        let mut workspaces = self.workspaces.write().await;
        if workspaces.contains_key(&workspace) {
            return Err(format!("workspace already exists: {workspace:?}"));
        }
        let style = style
            .map(style::from_value)
            .transpose()
            .map_err(|err| format!("style: {err}"))?;
        let filter = diagnostics
            .unwrap_or_default()
            .iter()
            .map(|batch| batch.parse())
            .collect::<Result<_, _>>()
            .map(crate::diagnostic_filter::Filter)
            .map_err(|err| format!("diagnostics: {err}"))?;
        let iris = iris
            .map(|iris| iris::open(&iris, iris_namespace.as_deref()))
            .transpose()
            .map_err(|err| format!("iris: {err}"))?;
        workspaces.insert(
            workspace.clone(),
            Arc::new(Mutex::new({
                let native = Workspace::native(iris, style);
                native.set_diagnostic_filter(filter);
                native
            })),
        );
        Ok(format!("created workspace {workspace:?}"))
    }

    /// Delete a workspace and all its documents by id. Call this when done with a workspace to free
    /// its resources.
    #[tool]
    pub async fn delete_workspace(
        &self,
        Parameters(WorkspaceIdRequest { workspace }): Parameters<WorkspaceIdRequest>,
    ) -> Result<String, String> {
        let mut workspaces = self.workspaces.write().await;
        if workspaces.remove(&workspace).is_none() {
            return Err(format!("no such workspace: {workspace:?}"));
        }
        Ok(format!("deleted workspace {workspace:?}"))
    }

    /// Add .cls/.mac/.int/.inc ObjectScript sources to a workspace, or update one already there.
    /// This only stores the raw text — nothing is parsed or checked here, so it won't report syntax
    /// errors; call check() on the returned `affected` uris to see up-to-date diagnostics. Insert
    /// every related file, not just the one you're editing — cross-class and cross-routine
    /// references (and the errors they can produce) are only resolved against what's in the
    /// workspace.
    #[tool]
    pub async fn insert(
        &self,
        Parameters(InsertRequest { workspace, files }): Parameters<InsertRequest>,
    ) -> Result<Json<InsertResult>, String> {
        let workspace = self.get_workspace(&workspace).await?;
        let workspace = workspace.lock().await;
        let mut affected = std::collections::HashSet::new();
        for FileToInsert { uri, src } in files {
            affected.extend(match src {
                Some(src) => workspace.open(uri, src),
                None => self.open_from_disk(&workspace, &uri)?,
            });
        }
        Ok(Json(InsertResult {
            affected: affected.into_iter().collect(),
        }))
    }

    /// Remove a document from a workspace by URI, e.g. after a file is deleted or renamed on disk.
    /// Returns the uris whose cached analysis is now stale — call check() on them to see up-to-date
    /// diagnostics.
    #[tool]
    pub async fn remove(
        &self,
        Parameters(UriRequest { workspace, uri }): Parameters<UriRequest>,
    ) -> Result<Json<InsertResult>, String> {
        let workspace = self.get_workspace(&workspace).await?;
        let workspace = workspace.lock().await;
        Ok(Json(InsertResult {
            affected: workspace.close(uri),
        }))
    }

    /// Run the real ObjectScript parser/type-checker on previously-inserted documents: syntax
    /// errors, undefined variables, class-context violations, argument/type mismatches, and more.
    /// Call this after every edit instead of guessing whether ObjectScript code is correct — it
    /// understands cross-class references that pattern-matching or grepping can't.
    #[tool]
    pub async fn check(
        &self,
        Parameters(UrisRequest { workspace, uris }): Parameters<UrisRequest>,
    ) -> Result<Json<DiagnosticsResult>, String> {
        let workspace = self.get_workspace(&workspace).await?;
        let workspace = workspace.lock().await;
        let files = uris
            .into_iter()
            .map(|uri| {
                let diagnostics = workspace.diagnostics(uri.clone());
                FileDiagnostics { uri, diagnostics }
            })
            .collect();
        Ok(Json(DiagnosticsResult { files }))
    }

    /// Get inferred inlay hints (e.g. inferred types) for a range in a previously-inserted document
    /// — use this to see what the checker inferred for an expression instead of guessing its type.
    #[tool]
    pub async fn inlay_hint(
        &self,
        Parameters(InlayHintRequest {
            workspace,
            uri,
            range,
        }): Parameters<InlayHintRequest>,
    ) -> Result<Json<InlayHintsResult>, String> {
        let workspace = self.get_workspace(&workspace).await?;
        let workspace = workspace.lock().await;
        Ok(Json(InlayHintsResult {
            hints: workspace.inlay_hint(uri, range),
        }))
    }

    /// Inspect a position in a previously-inserted document: the class member
    /// (property/method/parameter) declared there, if any. Otherwise, the inferred type of the
    /// local variable read there and/or what the macro invocation there expanded to — both together
    /// when both apply. Rendered as markdown. Prefer this over grepping when you need to inspect a
    /// class member or check a variable's inferred type.
    #[tool]
    pub async fn hover(
        &self,
        Parameters(PositionRequest {
            workspace,
            uri,
            position,
        }): Parameters<PositionRequest>,
    ) -> Result<Json<HoverResult>, String> {
        let workspace = self.get_workspace(&workspace).await?;
        let workspace = workspace.lock().await;
        Ok(Json(HoverResult {
            hover: workspace.hover(uri, position),
        }))
    }

    /// Go to definition: find where the reference at a position is declared (member/method access
    /// like `x.Foo`, `x.Foo()`, `i%Foo`). Doesn't cover a bare `##class(Name)` or `..Method`
    /// self-reference — those have no recorded declaration site yet.
    #[tool]
    pub async fn definition(
        &self,
        Parameters(PositionRequest {
            workspace,
            uri,
            position,
        }): Parameters<PositionRequest>,
    ) -> Result<Json<DefinitionResult>, String> {
        let workspace = self.get_workspace(&workspace).await?;
        let workspace = workspace.lock().await;
        Ok(Json(DefinitionResult {
            location: workspace.definition(uri, position),
        }))
    }

    /// Find references: every location across the workspace that refers to whatever's declared or
    /// referenced at a position (the reverse of definition) — call this before renaming or removing
    /// a member/label to see every call site that would break. Includes the declaration site
    /// itself. Works whether `position` is on a call site or on the declaration.
    #[tool]
    pub async fn references(
        &self,
        Parameters(PositionRequest {
            workspace,
            uri,
            position,
        }): Parameters<PositionRequest>,
    ) -> Result<Json<ReferencesResult>, String> {
        let workspace = self.get_workspace(&workspace).await?;
        let workspace = workspace.lock().await;
        Ok(Json(ReferencesResult {
            locations: workspace.references(uri, position, true),
        }))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        let mut instructions = String::from(
            "Real ObjectScript/COS parser and type-checker. Use this instead of guessing at \
             ObjectScript syntax, semantics, or types, or hand-rolling regex/grep checks over \
             .cls/.mac/.int/.inc source — it understands the language's grammar, class \
             hierarchy, and cross-file references, and will catch mistakes a text-level read \
             won't.\n\n\
             Workflow: call create_workspace(workspace) once, insert() every .cls/.mac/.int/.inc \
             file relevant to the task (not just the one being edited — undefined-reference and \
             type errors depend on the whole workspace), then call check(uris) after every edit \
             to catch syntax errors, undefined variables, class-context violations, and type \
             mismatches before treating the code as correct. Use hover instead of grepping to \
             look up a class member's definition. Call delete_workspace when done.",
        );
        if self.shared_fs {
            instructions.push_str(
                "\n\nThis server shares your file system: insert() a file:// uri with no `src` to \
                 read the file, or a directory's to load every file under it.",
            );
        }
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(instructions)
    }
}
