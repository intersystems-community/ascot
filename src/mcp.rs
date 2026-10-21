//! The MCP front end (`ascot mcp`): a stdio Model Context Protocol server exposing workspace
//! analysis as tools for AI agents (Claude Code via the `ascot-mcp` plugin, or `claude mcp add`).
//!
//! Unlike the LSP server it holds many `Workspace`s, keyed by an id the agent chooses:
//! `create_workspace` (optionally with IRIS connection details, see `iris.rs`) and
//! `delete_workspace`; `insert`/`remove` documents; then `check`, `inlay_hint`, `hover`,
//! `definition` and `references`. Tool parameter and result schemas derive from the
//! `bindings.rs` types via `schemars`. Built on the `rmcp` crate; native builds only.

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
    iris,
};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct WorkspaceIdRequest {
    workspace: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CreateWorkspaceRequest {
    workspace: String,
    /// Optional IRIS connection: `scheme://user:pass@host:port`, or the path of a declaration
    /// file written by `ascot declare`. When set, lookups that can't be resolved from documents
    /// already in this workspace (e.g. a class that isn't open here) fall back to this IRIS
    /// instance via the Atelier REST API, or to the file.
    iris: Option<String>,
    /// IRIS namespace to query against. Only meaningful when `iris` is set. Defaults to `%SYS`
    /// for a URL; for a declaration file it must match the namespace it declares.
    iris_namespace: Option<String>,
    /// Workspace-wide style rules, e.g. `{"commands": {"form": "Long", "case": "Pascal"}}` to
    /// have every command keyword written as `Set`/`Quit`. Applies to every document.
    style: Option<crate::bindings::Style>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FileToInsert {
    uri: String,
    src: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct InsertRequest {
    workspace: String,
    files: Vec<FileToInsert>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UriRequest {
    workspace: String,
    uri: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UrisRequest {
    workspace: String,
    uris: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct InlayHintRequest {
    workspace: String,
    uri: String,
    range: Range,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PositionRequest {
    workspace: String,
    uri: String,
    position: Position,
}

// MCP requires `outputSchema` to describe an object at the top level, so results that are
// naturally an array or an enum get wrapped in a single-field struct rather than returned bare.

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct FileDiagnostics {
    uri: String,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct DiagnosticsResult {
    files: Vec<FileDiagnostics>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct InlayHintsResult {
    hints: Vec<InlayHint>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct InsertResult {
    affected: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct HoverResult {
    hover: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct DefinitionResult {
    location: Option<Location>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ReferencesResult {
    locations: Vec<Location>,
}

#[derive(Clone)]
pub struct AnalyzerMcpServer {
    workspaces: Arc<RwLock<HashMap<String, Arc<Mutex<Workspace>>>>>,
    tool_router: ToolRouter<Self>,
}

impl Default for AnalyzerMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalyzerMcpServer {
    pub fn new() -> Self {
        Self {
            workspaces: Arc::new(RwLock::new(HashMap::new())),
            tool_router: Self::tool_router(),
        }
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
impl AnalyzerMcpServer {
    #[tool(
        description = "Create a new empty workspace with the given id; fails if it already exists. \
                        Do this first, before inserting any ObjectScript source. Pass `iris` to \
                        also resolve out-of-workspace lookups against a real IRIS instance (or a \
                        declaration file `ascot declare` wrote)."
    )]
    async fn create_workspace(
        &self,
        Parameters(CreateWorkspaceRequest {
            workspace,
            iris,
            iris_namespace,
            style,
        }): Parameters<CreateWorkspaceRequest>,
    ) -> Result<String, String> {
        let mut workspaces = self.workspaces.write().await;
        if workspaces.contains_key(&workspace) {
            return Err(format!("workspace already exists: {workspace:?}"));
        }
        let iris = iris
            .map(|iris| iris::open(&iris, iris_namespace.as_deref()))
            .transpose()
            .map_err(|err| format!("iris: {err}"))?;
        workspaces.insert(
            workspace.clone(),
            Arc::new(Mutex::new(Workspace::new_with_iris(iris, style))),
        );
        Ok(format!("created workspace {workspace:?}"))
    }

    #[tool(
        description = "Delete a workspace and all its documents by id. Call this when done \
                          with a workspace to free its resources."
    )]
    async fn delete_workspace(
        &self,
        Parameters(WorkspaceIdRequest { workspace }): Parameters<WorkspaceIdRequest>,
    ) -> Result<String, String> {
        let mut workspaces = self.workspaces.write().await;
        if workspaces.remove(&workspace).is_none() {
            return Err(format!("no such workspace: {workspace:?}"));
        }
        Ok(format!("deleted workspace {workspace:?}"))
    }

    #[tool(
        description = "Add .cls/.mac/.int/.inc ObjectScript sources to a workspace, or update \
                        one already there. This only stores the raw text — nothing is parsed \
                        or checked here, so it won't report syntax errors; call check() on the \
                        returned `affected` uris to see up-to-date diagnostics. Insert every \
                        related file, not just the one you're editing — cross-class and \
                        cross-routine references (and the errors they can produce) are only \
                        resolved against what's in the workspace."
    )]
    async fn insert(
        &self,
        Parameters(InsertRequest { workspace, files }): Parameters<InsertRequest>,
    ) -> Result<Json<InsertResult>, String> {
        let workspace = self.get_workspace(&workspace).await?;
        let workspace = workspace.lock().await;
        let mut affected = std::collections::HashSet::new();
        for FileToInsert { uri, src } in files {
            affected.extend(workspace.open(uri, src));
        }
        Ok(Json(InsertResult {
            affected: affected.into_iter().collect(),
        }))
    }

    #[tool(
        description = "Remove a document from a workspace by URI, e.g. after a file is \
                        deleted or renamed on disk. Returns the uris whose cached analysis is \
                        now stale — call check() on them to see up-to-date diagnostics."
    )]
    async fn remove(
        &self,
        Parameters(UriRequest { workspace, uri }): Parameters<UriRequest>,
    ) -> Result<Json<InsertResult>, String> {
        let workspace = self.get_workspace(&workspace).await?;
        let workspace = workspace.lock().await;
        Ok(Json(InsertResult {
            affected: workspace.close(uri),
        }))
    }

    #[tool(
        description = "Run the real ObjectScript parser/type-checker on previously-inserted \
                        documents: syntax errors, undefined variables, class-context \
                        violations, argument/type mismatches, and more. Call this after every \
                        edit instead of guessing whether ObjectScript code is correct — it \
                        understands cross-class references that pattern-matching or grepping \
                        can't."
    )]
    async fn check(
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

    #[tool(
        description = "Get inferred inlay hints (e.g. inferred types) for a range in a \
                        previously-inserted document — use this to see what the checker \
                        inferred for an expression instead of guessing its type."
    )]
    async fn inlay_hint(
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

    #[tool(
        description = "Inspect a position in a previously-inserted document: the class member \
                        (property/method/parameter) declared there, if any. Otherwise, the \
                        inferred type of the local variable read there and/or what the macro \
                        invocation there expanded to — both together when both apply. Rendered \
                        as markdown. Prefer this over grepping when you need to inspect a class \
                        member or check a variable's inferred type."
    )]
    async fn hover(
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

    #[tool(
        description = "Go to definition: find where the reference at a position is declared \
                        (member/method access like `x.Foo`, `x.Foo()`, `i%Foo`). Doesn't cover \
                        a bare `##class(Name)` or `..Method` self-reference — those have no \
                        recorded declaration site yet."
    )]
    async fn definition(
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

    #[tool(
        description = "Find references: every location across the workspace that refers to \
                        whatever's declared or referenced at a position (the reverse of \
                        definition) — call this before renaming or removing a member/label to \
                        see every call site that would break. Includes the declaration site \
                        itself. Works whether `position` is on a call site or on the \
                        declaration."
    )]
    async fn references(
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
impl ServerHandler for AnalyzerMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
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
        )
    }
}
