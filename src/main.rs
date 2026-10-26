//! The `ascot` command-line binary: argument parsing (with `clap`) and the `analyze`,
//! `analyze-workspace`, `parse`, `declare`, `mcp` and `lsp` subcommands documented in README.md.
//!
//! A thin shell over the library in `lib.rs`. It walks directories for `.cls`/`.mac`/`.int`/`.inc`
//! files, loads them into one `Workspace` and prints its diagnostics (`analyze*`), or hands the
//! process over to `lsp.rs`/`mcp.rs` for the server modes. `--iris` connections are built with
//! `iris.rs`. None of this is part of the wasm/JS build (see `bindings.rs`).

use ascot::Workspace;
use ascot::bindings::GuestWorkspace;
use ascot::bindings::Style;
use ascot::cos;
use ascot::cos::Routine;
use ascot::style;
use std::collections::HashSet;
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::process::exit;
use std::time::Instant;
use tygr::Grammar;

use ascot::{AtelierConnection, DocKind, IrisClient};
use clap::{Args, Parser, Subcommand};

/// A type checker and code intelligence engine for InterSystems ObjectScript.
#[derive(Parser)]
#[command(
    version,
    arg_required_else_help = true,
    after_help = "More: https://github.com/intersystems-community/ascot#usage"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check every .cls/.mac/.int/.inc file under the given paths
    ///
    /// Loads every .cls/.mac/.int/.inc file under each path into one workspace and prints the
    /// diagnostics of all of them. Exits non-zero if any error or warning was found, or if a path
    /// doesn't exist; hints don't affect the exit code. For example:
    /// `ascot analyze --iris http://user:password@localhost:8080/iris --iris-namespace USER src/`.
    Analyze {
        #[command(flatten)]
        iris: IrisArgs,
        #[command(flatten)]
        style: StyleArgs,
        /// Files or directories to check
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Check some paths, with a whole workspace loaded
    ///
    /// Loads every .cls/.mac/.int/.inc file under the root, so cross-file references resolve, but
    /// prints diagnostics only for the files under the given paths. Each path must exist; one
    /// outside the root is skipped with a warning. Exits like `ascot analyze`.
    AnalyzeWorkspace {
        #[command(flatten)]
        iris: IrisArgs,
        #[command(flatten)]
        style: StyleArgs,
        /// The directory whose files are all loaded
        root: PathBuf,
        /// Files or directories under the root to check
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Report a file's syntax error, if any
    ///
    /// Parses the file without type checking and prints its syntax error, if any. Exits non-zero
    /// on a syntax error.
    Parse {
        /// A .cls, .mac, .int or .inc file
        file: PathBuf,
    },
    /// Save an IRIS namespace's declarations to a file
    ///
    /// Fetches everything the other commands would ask a live instance for and writes it to the
    /// file: every class's superclasses, members and their signatures and types, and the source of
    /// every include file and routine. Pass the file as --iris to analyze without a live instance,
    /// for example in CI. Regenerate it after the instance's code changes.
    Declare {
        /// The IRIS instance, as http://user:password@host:port[/prefix]
        #[arg(long, value_name = "URL")]
        iris: String,
        /// The namespace to declare [default: %SYS]
        #[arg(long, value_name = "NS")]
        iris_namespace: Option<String>,
        /// The declaration file to write, for example user.d.json
        file: PathBuf,
    },
    /// Run the MCP server on stdio, for AI agents
    ///
    /// Exposes workspace analysis as tools an AI agent calls on demand. Takes no options: a
    /// workspace's IRIS connection is set by the `iris` and `iris_namespace` arguments of the
    /// `create_workspace` tool. For example: `claude mcp add ascot -- ascot mcp`.
    Mcp,
    /// Run the LSP server on stdio, for editors and AI agents
    Lsp {
        #[command(flatten)]
        iris: IrisArgs,
        #[command(flatten)]
        style: StyleArgs,
    },
}

#[derive(Args)]
struct StyleArgs {
    /// A JSON file of style rules, such as how command keywords are spelled
    ///
    /// Its shape is the `style` record in src/wit/ascot.wit. Without it, no style is checked.
    #[arg(long, value_name = "FILE")]
    style: Option<PathBuf>,
}

#[derive(Args)]
struct IrisArgs {
    /// An IRIS instance, or a declaration file from `ascot declare`
    ///
    /// The instance is given as http://user:password@host:port[/prefix]. Resolves the classes,
    /// include files and routines outside the workspace; without it, they are left unconfirmed.
    #[arg(long, value_name = "URL|FILE")]
    iris: Option<String>,
    /// The namespace to use [default: %SYS]
    ///
    /// With a declaration file, it must match the file's.
    #[arg(long, value_name = "NS", requires = "iris")]
    iris_namespace: Option<String>,
}

fn main() {
    match Cli::parse().command {
        Command::Analyze { iris, style, paths } => analyze(
            paths.iter().map(PathBuf::as_path),
            iris.open(),
            style.load(),
        ),
        Command::AnalyzeWorkspace {
            iris,
            style,
            root,
            paths,
        } => analyze_workspace(
            &root,
            paths.iter().map(PathBuf::as_path),
            iris.open(),
            style.load(),
        ),
        Command::Parse { file } => scan_file(&file),
        Command::Declare {
            iris,
            iris_namespace,
            file,
        } => declare(&iris, iris_namespace.as_deref(), &file),
        Command::Mcp => {
            let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
            rt.block_on(run_mcp_server());
        }
        Command::Lsp { iris, style } => {
            if let Err(err) = ascot::lsp::run(iris.open(), style.load()) {
                eprintln!("LSP server error: {err}");
                exit(1);
            }
        }
    }
}

impl IrisArgs {
    /// Opens the connection or declaration file, if `--iris` was given.
    fn open(self) -> Option<Box<dyn IrisClient>> {
        match ascot::open_iris(&self.iris?, self.iris_namespace.as_deref()) {
            Ok(iris) => Some(iris),
            Err(err) => {
                eprintln!("--iris: {err}");
                exit(1);
            }
        }
    }
}

impl StyleArgs {
    /// Reads and parses `--style`, if given. Like `--iris`, a file that can't be read or parsed is
    /// an error up front, not a silent fallback to no style.
    fn load(self) -> Option<Style> {
        let path = self.style?;
        let fail = |err: String| -> ! {
            eprintln!("--style: {}: {err}", path.display());
            exit(1)
        };
        let text = fs::read_to_string(&path).unwrap_or_else(|err| fail(err.to_string()));
        Some(style::parse(&text).unwrap_or_else(|err| fail(err)))
    }
}

/// Writes the declarations of the namespace at `url` to `out`, for a later `--iris <out>`.
fn declare(url: &str, namespace: Option<&str>, out: &Path) {
    let namespace = namespace.unwrap_or(ascot::DEFAULT_IRIS_NAMESPACE);
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
    use ascot::mcp::Server;
    use rmcp::ServiceExt;

    let server = match Server::new().serve(rmcp::transport::stdio()).await {
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

fn scan_file(file_path: &Path) {
    let content = match fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            process::exit(1);
        }
    };
    let result = if file_path.extension().is_some_and(|ext| ext == "cls") {
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
fn analyze<'a>(
    roots: impl Iterator<Item = &'a Path>,
    iris: Option<Box<dyn IrisClient>>,
    style: Option<Style>,
) {
    let mut files = Vec::new();
    for root in roots {
        if !root.exists() {
            eprintln!("Error: {} does not exist", root.display());
            exit(1);
        }
        collect_doc(root, &mut files);
    }
    files.sort();

    let workspace = Workspace::native(iris, style);
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
    style: Option<Style>,
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

    let workspace = Workspace::native(iris, style);
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
