//! The `ascot` command-line binary: argument parsing and the `analyze`, `analyze-workspace`,
//! `parse`, `declare`, `mcp` and `lsp` subcommands documented in README.md.
//!
//! A thin shell over the library in `lib.rs`. It walks directories for `.cls`/`.mac`/`.int`/`.inc`
//! files, loads them into one `Workspace` and prints its diagnostics (`analyze*`), or hands the
//! process over to `lsp.rs`/`mcp.rs` for the server modes. `--iris` connections are built with
//! `iris.rs`. None of this is part of the wasm/JS build (see `bindings.rs`).

use ascot::Workspace;
use ascot::bindings::GuestWorkspace;
use ascot::cos;
use ascot::cos::Routine;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::process::exit;
use std::time::Instant;
use tygr::Grammar;

use ascot::atelier::{AtelierConnection, DocKind};
use ascot::iris::IrisClient;

fn main() {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("analyze") => {
            let mut rest: Vec<String> = args[2..].to_vec();
            let iris = connect_iris(&mut rest);
            if rest.is_empty() {
                eprintln!("usage: analyze [--iris <url>] [--iris-namespace <ns>] <path>...");
                exit(1);
            }
            analyze(rest.iter().map(|s| Path::new(s.as_str())), iris);
        }
        Some("analyze-workspace") => {
            let mut rest: Vec<String> = args[2..].to_vec();
            let iris = connect_iris(&mut rest);
            if rest.len() < 2 {
                eprintln!(
                    "usage: analyze-workspace [--iris <url>] [--iris-namespace <ns>] <workspace-root> <path>..."
                );
                exit(1);
            }
            let root = PathBuf::from(&rest[0]);
            let paths: Vec<PathBuf> = rest[1..].iter().map(PathBuf::from).collect();
            analyze_workspace(&root, paths.iter().map(PathBuf::as_path), iris);
        }
        Some("parse") => {
            let Some(file_path) = args.get(2) else {
                eprintln!("usage: parse <file>");
                exit(1);
            };
            scan_file(file_path);
        }
        Some("declare") => {
            let mut rest: Vec<String> = args[2..].to_vec();
            let url = extract_flag(&mut rest, "--iris");
            let namespace = extract_flag(&mut rest, "--iris-namespace");
            let (Some(url), [out]) = (url, rest.as_slice()) else {
                eprintln!("usage: declare --iris <url> [--iris-namespace <ns>] <file>");
                exit(1);
            };
            declare(&url, namespace.as_deref(), Path::new(out));
        }
        Some("mcp") => {
            let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
            rt.block_on(run_mcp_server());
        }
        Some("lsp") => {
            let mut rest: Vec<String> = args[2..].to_vec();
            let iris = connect_iris(&mut rest);
            if let Err(err) = ascot::lsp::run(iris) {
                eprintln!("LSP server error: {err}");
                exit(1);
            }
        }
        _ => {
            eprintln!(
                "usage: parse <file> | analyze [--iris <url|file>] [--iris-namespace <ns>] <path>... | analyze-workspace [--iris <url|file>] [--iris-namespace <ns>] <root> <path>... | declare --iris <url> [--iris-namespace <ns>] <file> | mcp | lsp [--iris <url|file>] [--iris-namespace <ns>]"
            );
            exit(1);
        }
    }
}

/// Pulls `--flag <value>` out of `args` (in place), wherever it appears, leaving the rest.
fn extract_flag(args: &mut Vec<String>, flag: &str) -> Option<String> {
    let idx = args.iter().position(|a| a == flag)?;
    args.remove(idx);
    if idx >= args.len() {
        eprintln!("{flag} requires a value");
        exit(1);
    }
    Some(args.remove(idx))
}

/// Pulls `--iris`/`--iris-namespace` out of `args` and opens the connection or declaration file, if
/// `--iris` was given. `--iris-namespace` is ignored (with a warning) if `--iris` is absent.
fn connect_iris(args: &mut Vec<String>) -> Option<Box<dyn IrisClient>> {
    let iris = extract_flag(args, "--iris");
    let namespace = extract_flag(args, "--iris-namespace");
    let iris = iris.or_else(|| {
        if namespace.is_some() {
            eprintln!("--iris-namespace has no effect without --iris");
        }
        None
    })?;
    match ascot::iris::open(&iris, namespace.as_deref()) {
        Ok(iris) => Some(iris),
        Err(err) => {
            eprintln!("--iris: {err}");
            exit(1);
        }
    }
}

/// Writes the declarations of the namespace at `url` to `out`, for a later `--iris <out>`.
fn declare(url: &str, namespace: Option<&str>, out: &Path) {
    let namespace = namespace.unwrap_or(ascot::iris::DEFAULT_NAMESPACE);
    let conn = match AtelierConnection::connect(url, namespace).and_then(|conn| {
        conn.check()?;
        Ok(conn)
    }) {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!("--iris: {err}");
            exit(1);
        }
    };
    let started = Instant::now();
    let mut bars = std::io::stderr().is_terminal().then(ProgressBars::default);
    let declarations = conn.declarations(|kind, done, total| {
        if let Some(bars) = &mut bars {
            bars.update(kind, done, total);
        }
    });
    match declarations.and_then(|decls| decls.save(out).map(|()| decls)) {
        Ok(decls) => eprintln!(
            "{} class(es), {} include file(s), {} routine(s) of namespace {} written to {} in {}s",
            decls.classes.len(),
            decls.includes.len(),
            decls.routines.len(),
            decls.namespace,
            out.display(),
            started.elapsed().as_secs()
        ),
        Err(err) => {
            eprintln!("declare: {err}");
            exit(1);
        }
    }
}

async fn run_mcp_server() {
    use ascot::mcp::AnalyzerMcpServer;
    use rmcp::ServiceExt;

    let server = match AnalyzerMcpServer::new()
        .serve(rmcp::transport::stdio())
        .await
    {
        Ok(server) => server,
        Err(err) => {
            eprintln!("failed to start MCP server: {err:?}");
            exit(1);
        }
    };
    if let Err(err) = server.waiting().await {
        eprintln!("MCP server error: {err:?}");
        exit(1);
    }
}

fn scan_file(file_path: &str) {
    let content = match fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            process::exit(1);
        }
    };
    let result = if file_path.ends_with(".cls") {
        cos::ClassDefinition::scan(&content)
    } else {
        Routine::scan(&content)
    };
    if let Err(err) = result {
        eprintln!("{err}");
        exit(1);
    }
}

/// Add every `.cls`/`.mac`/`.int`/`.inc` file under each of `roots` to a workspace and report
/// all diagnostics. A root that doesn't exist is an error, not an empty result: a typo'd path in
/// a CI step must not pass forever.
fn analyze<'a>(roots: impl Iterator<Item = &'a Path>, iris: Option<Box<dyn IrisClient>>) {
    let mut files = Vec::new();
    for root in roots {
        if !root.exists() {
            eprintln!("Error: {} does not exist", root.display());
            exit(1);
        }
        collect_doc(root, &mut files);
    }
    files.sort();

    let workspace = Workspace::new_with_iris(iris);
    insert_files(&workspace, &files);

    let count = report_diagnostics(&workspace, &files);
    eprintln!("{} diagnostic(s) across {} file(s)", count, files.len());
    if count > 0 {
        exit(1);
    }
}

/// Add every `.cls`/`.mac`/`.int`/`.inc` file under `root` to a workspace (so cross-file
/// references resolve), but only report diagnostics for files under the given `paths`.
fn analyze_workspace<'a>(
    root: &Path,
    paths: impl Iterator<Item = &'a Path>,
    iris: Option<Box<dyn IrisClient>>,
) {
    let root = match fs::canonicalize(root) {
        Ok(root) => root,
        Err(e) => {
            eprintln!("Error resolving workspace root {}: {}", root.display(), e);
            exit(1);
        }
    };

    let mut all_files = Vec::new();
    collect_doc(&root, &mut all_files);
    all_files.sort();

    let mut diagnose_files = Vec::new();
    for path in paths {
        match fs::canonicalize(path) {
            Ok(path) => collect_doc(&path, &mut diagnose_files),
            Err(e) => {
                eprintln!("Error resolving {}: {}", path.display(), e);
                exit(1);
            }
        }
    }
    diagnose_files.sort();
    diagnose_files.dedup();

    let workspace = Workspace::new_with_iris(iris);
    insert_files(&workspace, &all_files);

    let all_files_set: HashSet<&PathBuf> = all_files.iter().collect();
    diagnose_files.retain(|file| {
        let under_root = all_files_set.contains(file);
        if !under_root {
            eprintln!(
                "Warning: {} is not under workspace root {}; skipping",
                file.display(),
                root.display()
            );
        }
        under_root
    });

    let count = report_diagnostics(&workspace, &diagnose_files);
    eprintln!(
        "{} diagnostic(s) across {} file(s) (workspace: {} file(s))",
        count,
        diagnose_files.len(),
        all_files.len()
    );
    if count > 0 {
        exit(1);
    }
}

/// One bar per kind of document `declare` fetches, redrawn in place on stderr.
#[derive(Default)]
struct ProgressBars {
    counts: [(usize, usize); 3],
    drawn: bool,
}

impl ProgressBars {
    const LABELS: [&str; 3] = ["classes", "include files", "routines"];
    const WIDTH: usize = 30;

    fn update(&mut self, kind: DocKind, done: usize, total: usize) {
        self.counts[kind as usize] = (done, total);
        if self.drawn {
            eprint!("\x1b[{}A", Self::LABELS.len());
        }
        for (label, (done, total)) in Self::LABELS.iter().zip(self.counts) {
            let filled = (done * Self::WIDTH).checked_div(total).unwrap_or(0);
            eprintln!(
                "\x1b[K{label:<13} [{}{}] {done}/{total}",
                "#".repeat(filled),
                "-".repeat(Self::WIDTH - filled)
            );
        }
        self.drawn = true;
    }
}

fn insert_files(workspace: &Workspace, files: &[PathBuf]) {
    for file in files {
        let src = match fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading {}: {}", file.display(), e);
                continue;
            }
        };
        let uri = file.display().to_string();
        workspace.open(uri, src);
    }
}

// Prints every diagnostic, but only `Error`/`Warning` count toward the returned total (which
// decides the exit code) -- a hint is worth seeing, not worth failing a build over.
fn report_diagnostics(workspace: &Workspace, files: &[PathBuf]) -> usize {
    let color = std::io::stdout().is_terminal();
    let mut count = 0;
    for file in files {
        let uri = file.display().to_string();
        for diagnostic in workspace.diagnostics(uri.clone()) {
            use ascot::bindings::DiagnosticSeverity::*;
            // Colored on a terminal; a plain severity prefix otherwise, for scripts.
            let (on, off) = match (diagnostic.severity, color) {
                (Error, true) => ("\x1b[31m", "\x1b[0m"),
                (Warning, true) => ("\x1b[38;5;214m", "\x1b[0m"),
                (Information | Hint, true) => ("\x1b[2m", "\x1b[0m"),
                (Error, false) => ("error: ", ""),
                (Warning, false) => ("warning: ", ""),
                (Information | Hint, false) => ("hint: ", ""),
            };
            if matches!(diagnostic.severity, Error | Warning) {
                count += 1;
            }
            print!("{}{}{}{}", on, uri, diagnostic.describe(), off);
        }
    }
    let _ = std::io::stdout().flush();
    count
}

fn collect_doc(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(e) => {
                eprintln!("Error reading {}: {}", path.display(), e);
                return;
            }
        };
        for entry in entries.flatten() {
            collect_doc(&entry.path(), out);
        }
    } else if path
        .extension()
        .is_some_and(|ext| matches!(ext.to_str(), Some("cls" | "mac" | "int" | "inc")))
    {
        out.push(path.to_path_buf());
    }
}
