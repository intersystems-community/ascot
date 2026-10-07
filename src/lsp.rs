//! The LSP front end (`ascot lsp`): a stdio Language Server Protocol server over one `Workspace`.
//!
//! Translates between `lsp-types` and `bindings.rs` types and maps each request to a `Workspace`
//! method: `didOpen`/`didChange` re-run `open` and republish diagnostics; hover, definition,
//! references, document/workspace symbols and inlay hints call their namesakes. Only
//! `.cls`/`.mac`/`.int`/`.inc` documents are tracked. This is what the Claude Code `ascot-lsp`
//! plugin (`bindings/claude-lsp-plugin`) launches. The InterSystems Language Server does not use
//! it; it embeds the wasm build instead (see `bindings.rs`).

use std::path::{Path, PathBuf};

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    Diagnostic as LspDiagnostic, DiagnosticRelatedInformation as LspDiagnosticRelatedInformation,
    DiagnosticSeverity as LspDiagnosticSeverity, DidChangeTextDocumentParams,
    DidOpenTextDocumentParams, DocumentSymbol as LspDocumentSymbol, DocumentSymbolParams,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    HoverProviderCapability, InitializeParams, InlayHint as LspInlayHint,
    InlayHintLabel as LspInlayHintLabel, InlayHintParams, Location as LspLocation, MarkupContent,
    MarkupKind, OneOf, Position as LspPosition, PublishDiagnosticsParams, Range as LspRange,
    ReferenceParams, ServerCapabilities, SymbolInformation, SymbolKind as LspSymbolKind, SymbolTag,
    TextDocumentSyncCapability, TextDocumentSyncKind, Uri, WorkspaceSymbolParams,
};

use crate::{
    Workspace,
    atelier::IrisClient,
    bindings::{
        Diagnostic, DiagnosticSeverity, GuestWorkspace, InlayHint, Position, Range, SymbolInfo,
        SymbolKind,
    },
};

pub fn run(
    iris: Option<Box<dyn IrisClient>>,
) -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    let (connection, io_threads) = Connection::stdio();

    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        inlay_hint_provider: Some(OneOf::Left(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        ..Default::default()
    };
    let initialize_params = connection.initialize(serde_json::to_value(capabilities)?)?;
    let params: InitializeParams = serde_json::from_value(initialize_params)?;

    let workspace = Workspace::new_with_iris(iris);
    // Insert every `.cls`/`.mac`/`.int`/`.inc` file under the workspace root up front, so
    // cross-file references (`##class(...)`, `#include`) resolve correctly from the start —
    // otherwise a file that's opened on its own, without whatever it `#include`s also being
    // open, gets a real parse error instead of a diagnostic (the unresolved `#include` is left
    // as literal text in the expansion). This only runs once, at startup — a file added to the
    // workspace afterward needs to be opened individually to be picked up.
    for root in workspace_roots(&params) {
        open_directory(&workspace, &root);
    }

    main_loop(&connection, &workspace)?;
    // `io_threads`'s writer only stops once every `Sender<Message>` clone is dropped — including
    // `connection`'s own, so it must go out of scope before we can join.
    drop(connection);
    io_threads.join()?;
    Ok(())
}

// Every workspace root the client told us about — `workspaceFolders` (current) and `rootUri`
// (deprecated, but some clients only send this).
fn workspace_roots(params: &InitializeParams) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = params
        .workspace_folders
        .iter()
        .flatten()
        .filter_map(|folder| uri_to_path(&folder.uri))
        .collect();
    if roots.is_empty() {
        #[allow(deprecated)]
        let root_uri = params.root_uri.as_ref();
        roots.extend(root_uri.and_then(uri_to_path));
    }
    roots
}

fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    url::Url::parse(uri.as_str()).ok()?.to_file_path().ok()
}

// The workspace key for a document the client names. Clients spell `file:` uris differently
// from `url::Url::from_file_path` (VS Code sends `file:///c%3A/...` for `C:\...`), which is how
// the startup scan keys the same file -- so route every client uri through the path and back,
// and the two spellings meet. Anything that isn't a file path is used as-is.
fn workspace_key(uri: &Uri) -> String {
    uri_to_path(uri)
        .and_then(|path| url::Url::from_file_path(path).ok())
        .map(|url| url.to_string())
        .unwrap_or_else(|| uri.as_str().to_string())
}

fn open_directory(workspace: &Workspace, path: &Path) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            open_directory(workspace, &entry.path());
        }
    } else if path
        .extension()
        .is_some_and(|ext| matches!(ext.to_str(), Some("cls" | "mac" | "int" | "inc")))
        && let Ok(text) = std::fs::read_to_string(path)
        // Keyed the same way `textDocument/didOpen` would key it, so later opening the same
        // file from the editor updates this entry instead of creating a stale duplicate.
        && let Ok(uri) = url::Url::from_file_path(path)
    {
        workspace.open(uri.to_string(), text);
    }
}

fn main_loop(
    connection: &Connection,
    workspace: &Workspace,
) -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                let response = handle_request(workspace, req);
                connection.sender.send(Message::Response(response))?;
            }
            Message::Notification(not) => handle_notification(connection, workspace, not)?,
            Message::Response(_) => {}
        }
    }
    Ok(())
}

fn handle_request(workspace: &Workspace, req: Request) -> Response {
    match req.method.as_str() {
        "textDocument/inlayHint" => respond(req.id, req.params, |params: InlayHintParams| {
            let uri = workspace_key(&params.text_document.uri);
            let hints = workspace
                .inlay_hint(uri, params.range.into())
                .into_iter()
                .map(to_lsp_inlay_hint)
                .collect::<Vec<_>>();
            Some(hints)
        }),
        "textDocument/definition" => respond(req.id, req.params, |params: GotoDefinitionParams| {
            let text_document_position = params.text_document_position_params;
            let uri = workspace_key(&text_document_position.text_document.uri);
            let position = text_document_position.position.into();
            let location = workspace.definition(uri, position)?;
            let lsp_location = LspLocation {
                uri: location.uri.parse().ok()?,
                range: to_lsp_range(location.range),
            };
            Some(GotoDefinitionResponse::Scalar(lsp_location))
        }),
        "textDocument/references" => respond(req.id, req.params, |params: ReferenceParams| {
            let text_document_position = params.text_document_position;
            let uri = workspace_key(&text_document_position.text_document.uri);
            let position = text_document_position.position.into();
            let locations = workspace
                .references(uri, position, params.context.include_declaration)
                .into_iter()
                .filter_map(|location| {
                    Some(LspLocation {
                        uri: location.uri.parse().ok()?,
                        range: to_lsp_range(location.range),
                    })
                })
                .collect::<Vec<_>>();
            Some(locations)
        }),
        "textDocument/hover" => respond(req.id, req.params, |params: HoverParams| {
            let text_document_position = params.text_document_position_params;
            let uri = workspace_key(&text_document_position.text_document.uri);
            let position = text_document_position.position.into();
            let value = workspace.hover(uri, position)?;
            Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value,
                }),
                range: None,
            })
        }),
        "textDocument/documentSymbol" => {
            respond(req.id, req.params, |params: DocumentSymbolParams| {
                let uri = workspace_key(&params.text_document.uri);
                let document_symbol = workspace.document_symbol(uri)?;
                let children = document_symbol
                    .members
                    .iter()
                    .map(to_lsp_document_symbol)
                    .collect();
                Some(vec![LspDocumentSymbol {
                    children: Some(children),
                    ..to_lsp_document_symbol(&document_symbol.root)
                }])
            })
        }
        "workspace/symbol" => respond(req.id, req.params, |params: WorkspaceSymbolParams| {
            let symbols = workspace
                .workspace_symbol(params.query)
                .into_iter()
                .filter_map(|symbol| {
                    Some(SymbolInformation {
                        name: symbol.name,
                        kind: to_lsp_symbol_kind(symbol.kind),
                        tags: symbol.deprecated.then(|| vec![SymbolTag::DEPRECATED]),
                        #[allow(deprecated)]
                        deprecated: None,
                        location: LspLocation {
                            uri: symbol.location.uri.parse().ok()?,
                            range: to_lsp_range(symbol.location.range),
                        },
                        container_name: None,
                    })
                })
                .collect::<Vec<_>>();
            Some(symbols)
        }),
        _ => Response::new_err(
            req.id,
            ErrorCode::MethodNotFound as i32,
            format!("unsupported method: {}", req.method),
        ),
    }
}

// Deserializes `params` as `P`, runs `f`, and wraps whatever it returns (including `None`, which
// serializes to `null` — the correct "no result" response for e.g. an unresolved definition) as
// a successful response. A deserialization failure becomes a `InvalidParams` error response.
fn respond<P: serde::de::DeserializeOwned, R: serde::Serialize>(
    id: RequestId,
    params: serde_json::Value,
    f: impl FnOnce(P) -> Option<R>,
) -> Response {
    match serde_json::from_value::<P>(params) {
        Ok(params) => Response::new_ok(id, f(params)),
        Err(err) => Response::new_err(id, ErrorCode::InvalidParams as i32, err.to_string()),
    }
}

fn handle_notification(
    connection: &Connection,
    workspace: &Workspace,
    not: Notification,
) -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    match not.method.as_str() {
        "textDocument/didOpen" => {
            let params: DidOpenTextDocumentParams = serde_json::from_value(not.params)?;
            let uri = workspace_key(&params.text_document.uri);
            let affected = workspace.open(uri, params.text_document.text);
            publish_all(connection, workspace, affected)?;
        }
        "textDocument/didChange" => {
            let params: DidChangeTextDocumentParams = serde_json::from_value(not.params)?;
            let uri = workspace_key(&params.text_document.uri);
            // Whole-document sync: the (only) content change is the full new text.
            if let Some(change) = params.content_changes.into_iter().last() {
                let affected = workspace.open(uri, change.text);
                publish_all(connection, workspace, affected)?;
            }
        }
        // Leave the document in the workspace on close — other still-open files may reference
        // it, and a close isn't a delete (there's no watched-files support to tell those apart).
        "textDocument/didClose" => {}
        _ => {}
    }
    Ok(())
}

fn publish_all(
    connection: &Connection,
    workspace: &Workspace,
    uris: Vec<String>,
) -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    for uri in uris {
        let Ok(lsp_uri) = uri.parse::<Uri>() else {
            continue;
        };
        let diagnostics = workspace
            .diagnostics(uri)
            .into_iter()
            .map(to_lsp_diagnostic)
            .collect();
        let params = PublishDiagnosticsParams {
            uri: lsp_uri,
            diagnostics,
            version: None,
        };
        let notification = Notification::new("textDocument/publishDiagnostics".to_string(), params);
        connection
            .sender
            .send(Message::Notification(notification))?;
    }
    Ok(())
}

fn to_lsp_position(position: Position) -> LspPosition {
    LspPosition {
        line: position.line,
        character: position.character,
    }
}

fn to_lsp_range(range: Range) -> LspRange {
    LspRange {
        start: to_lsp_position(range.start),
        end: to_lsp_position(range.end),
    }
}

fn to_lsp_document_symbol(info: &SymbolInfo) -> LspDocumentSymbol {
    #[allow(deprecated)]
    LspDocumentSymbol {
        name: info.name.clone(),
        detail: None,
        kind: to_lsp_symbol_kind(info.kind),
        tags: info.deprecated.then(|| vec![SymbolTag::DEPRECATED]),
        deprecated: None,
        range: to_lsp_range(info.range),
        selection_range: to_lsp_range(info.selection_range),
        children: None,
    }
}

fn to_lsp_symbol_kind(kind: SymbolKind) -> LspSymbolKind {
    match kind {
        SymbolKind::Class => LspSymbolKind::CLASS,
        SymbolKind::Module => LspSymbolKind::MODULE,
        SymbolKind::Method => LspSymbolKind::METHOD,
        SymbolKind::Property => LspSymbolKind::PROPERTY,
        SymbolKind::Interface => LspSymbolKind::INTERFACE,
        SymbolKind::Function => LspSymbolKind::FUNCTION,
        SymbolKind::Constant => LspSymbolKind::CONSTANT,
        SymbolKind::Array => LspSymbolKind::ARRAY,
        SymbolKind::Object => LspSymbolKind::OBJECT,
        SymbolKind::Key => LspSymbolKind::KEY,
        SymbolKind::Struct => LspSymbolKind::STRUCT,
        SymbolKind::Event => LspSymbolKind::EVENT,
    }
}

impl From<LspPosition> for Position {
    fn from(position: LspPosition) -> Self {
        Self {
            line: position.line,
            character: position.character,
        }
    }
}

impl From<LspRange> for Range {
    fn from(range: LspRange) -> Self {
        Self {
            start: range.start.into(),
            end: range.end.into(),
        }
    }
}

fn to_lsp_inlay_hint(hint: InlayHint) -> LspInlayHint {
    LspInlayHint {
        position: to_lsp_position(hint.position),
        label: LspInlayHintLabel::String(hint.label),
        kind: None,
        text_edits: None,
        tooltip: None,
        padding_left: Some(hint.padding_left),
        padding_right: Some(hint.padding_right),
        data: None,
    }
}

fn to_lsp_diagnostic(diagnostic: Diagnostic) -> LspDiagnostic {
    let related_information = diagnostic
        .related_information
        .into_iter()
        .filter_map(|info| {
            Some(LspDiagnosticRelatedInformation {
                location: LspLocation {
                    uri: info.location.uri.parse().ok()?,
                    range: to_lsp_range(info.location.range),
                },
                message: info.message,
            })
        })
        .collect::<Vec<_>>();
    LspDiagnostic {
        range: to_lsp_range(diagnostic.range),
        severity: Some(match diagnostic.severity {
            DiagnosticSeverity::Error => LspDiagnosticSeverity::ERROR,
            DiagnosticSeverity::Warning => LspDiagnosticSeverity::WARNING,
            DiagnosticSeverity::Information => LspDiagnosticSeverity::INFORMATION,
            DiagnosticSeverity::Hint => LspDiagnosticSeverity::HINT,
        }),
        code: None,
        code_description: None,
        source: Some("ascot".to_string()),
        message: diagnostic.message,
        related_information: (!related_information.is_empty()).then_some(related_information),
        tags: None,
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_and_scan_spellings_of_a_file_uri_share_one_key() {
        let client: Uri = "file:///c%3A/dir/Foo.cls".parse().unwrap();
        let scan = url::Url::from_file_path(uri_to_path(&client).unwrap()).unwrap();
        assert_eq!(workspace_key(&client), scan.to_string());
        assert!(!workspace_key(&client).contains("%3A"));
    }

    #[test]
    fn a_non_file_uri_is_its_own_key() {
        let uri: Uri = "untitled:Untitled-1".parse().unwrap();
        assert_eq!(workspace_key(&uri), "untitled:Untitled-1");
    }
}
