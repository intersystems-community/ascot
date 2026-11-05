//! The `ascot` command-line binary: argument parsing (with `clap`) and the `analyze`, `parse`,
//! `declare`, `mcp` and `lsp` subcommands documented in README.md.
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
use ascot::diagnostic_filter::Filter;
use ascot::mcp::{DiagnosticsResult, FileDiagnostics};
use ascot::settings::{Diagnostics, Settings, StyleSetting};
use ascot::style;
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::process::exit;
use std::time::Instant;
use tygr::Grammar;

use ascot::{DocKind, IrisContext, RawAtelier};
use clap::{Args, Parser, Subcommand, ValueEnum};

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
    /// diagnostics of all of them. With --root, the workspace is every file under that directory
    /// instead, so cross-file references resolve, and the paths choose whose diagnostics are
    /// printed; one outside the root is skipped with a warning. Exits non-zero if any error or
    /// warning was found, or if a path doesn't exist; hints don't affect the exit code. For
    /// example:
    /// `ascot analyze --iris http://user:password@localhost:8080/iris --iris-namespace USER src/`.
    Analyze {
        #[command(flatten)]
        iris: IrisArgs,
        #[command(flatten)]
        style: StyleArgs,
        #[command(flatten)]
        filter: FilterArgs,
        #[command(flatten)]
        output: OutputArgs,
        /// Load every file under this directory, and print only the diagnostics of the paths
        #[arg(long, value_name = "DIR")]
        root: Option<PathBuf>,
        /// Files or directories to check
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
        /// [default: ~/.ascot/<host>-<port>-<namespace>.d.json]
        file: Option<PathBuf>,
    },
    /// Run the MCP server on stdio, for AI agents
    ///
    /// Exposes workspace analysis as tools an AI agent calls on demand. A workspace's IRIS
    /// connection is set by the `iris` and `iris_namespace` arguments of the `create_workspace`
    /// tool. For example: `claude mcp add ascot -- ascot mcp`.
    Mcp {
        /// The server shares the client's file system: `insert` reads a `file://` uri, a file or
        /// a whole directory, when no `src` is given
        #[arg(long)]
        shared_fs: bool,
    },
    /// Run the LSP server on stdio, for editors and AI agents
    ///
    /// The flags are defaults: the client's `initializationOptions` override them, with the same
    /// names, and a `workspace/didChangeConfiguration` changes them while the server runs.
    Lsp {
        #[command(flatten)]
        iris: IrisArgs,
        #[command(flatten)]
        style: StyleArgs,
        #[command(flatten)]
        filter: FilterArgs,
        /// The server shares the client's file system: the workspace roots are read from disk
        /// at startup, and a `style` setting may be a file's path
        #[arg(long)]
        shared_fs: bool,
    },
}

#[derive(Args)]
struct OutputArgs {
    /// How to print the diagnostics
    ///
    /// - `text`: one line per diagnostic, for reading.
    ///
    /// - `lsp`: JSON, the diagnostics of each file as LSP `Diagnostic` objects, as the language
    ///   server publishes them.
    ///
    /// - `sarif`: SARIF 2.1.0, for code scanning.
    ///
    /// The summary stays on stderr and the exit code is the same for all three.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Lsp,
    Sarif,
}

#[derive(Args)]
struct FilterArgs {
    #[arg(
        short = 'd',
        long,
        value_name = "VERB:TARGETS",
        help = "Which codes are reported, and at what severity",
        long_help = diagnostics_help()
    )]
    diagnostics: Vec<Filter>,
}

impl FilterArgs {
    fn filter(self) -> Filter {
        Filter(self.diagnostics.into_iter().flat_map(|f| f.0).collect())
    }
}

fn diagnostics_help() -> String {
    format!(
        "Which codes are reported, and at what severity\n\nRepeatable, or several in one value \
         separated by spaces, and applied in order, a later value overriding an earlier one; `on` \
         is the code's own severity. A target is a code, a \
         severity (every code currently reported at it), or `all`. For example `-d off:A5579`, \
         `-d off:hint,information`, `-d off:all -d on:A7095,A6851`, or `-d hint:A4471`. The exit \
         code follows what is reported.\n\n{}",
        ascot::diagnostic_filter::bnf()
    )
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
    /// An IRIS Community release, an IRIS instance, a declaration file from `ascot declare`, or
    /// `none`
    ///
    /// Resolves the classes, include files and routines outside the workspace. A release, the
    /// tag of its Community image such as 2026.2, or `latest` for the newest this ascot knows, is
    /// its pre-built declarations, downloaded into ~/.ascot the first time: `<image>:<tag>` for
    /// another image's, `<image>:<tag>:<namespace>` for a namespace other than %SYS, as
    /// irishealth-community:<tag>:HSLIB for IRIS for Health's HS and Ens classes. An instance is
    /// given as http://user:password@host:port[/prefix]. With `none`, nothing outside the workspace
    /// exists: every reference to such a class, routine or include is an error. [default: latest]
    #[arg(long, value_name = "RELEASE|URL|FILE|none")]
    iris: Option<String>,
    /// The namespace of the live instance to use [default: %SYS]
    ///
    /// A release names its own after its tag, and a declaration file carries its own.
    #[arg(long, value_name = "NS", requires = "iris")]
    iris_namespace: Option<String>,
}

fn main() {
    match Cli::parse().command {
        Command::Analyze {
            iris,
            style,
            filter,
            output,
            root,
            paths,
        } => analyze(
            root.as_deref(),
            paths.iter().map(PathBuf::as_path),
            iris.open(),
            style.load(),
            filter.filter(),
            output.format,
        ),
        Command::Parse { file } => scan_file(&file),
        Command::Declare {
            iris,
            iris_namespace,
            file,
        } => declare(&iris, iris_namespace.as_deref(), file.as_deref()),
        Command::Mcp { shared_fs } => {
            let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
            rt.block_on(run_mcp_server(shared_fs));
        }
        Command::Lsp {
            iris,
            style,
            filter,
            shared_fs,
        } => {
            let settings = Settings {
                iris: iris.iris,
                iris_namespace: iris.iris_namespace,
                style: style.load().map(StyleSetting::from),
                diagnostics: Some(Diagnostics::List(
                    filter.filter().0.iter().map(Grammar::print).collect(),
                )),
            };
            if let Err(err) = ascot::lsp::run(settings, shared_fs) {
                eprintln!("LSP server error: {err}");
                exit(1);
            }
        }
    }
}

impl IrisArgs {
    /// Opens the release's declarations, the connection or the declaration file; `none` is no
    /// environment. Whatever can't be opened or fetched ends the run.
    fn open(self) -> Option<Box<dyn IrisContext>> {
        let iris = (self.iris).unwrap_or_else(|| ascot::LATEST_IRIS.to_string());
        match ascot::open_iris(&iris, self.iris_namespace.as_deref()) {
            Ok(iris) => iris,
            Err(err) => {
                eprintln!("--iris {iris}: {err}");
                exit(1);
            }
        }
    }
}

impl StyleArgs {
    /// Reads and parses `--style`, if given. Like `--iris`, a file that can't be read or parsed is
    /// an error up front, not a silent fallback to no style.
    fn load(self) -> Option<Style> {
        let path = self.style.filter(|path| !path.as_os_str().is_empty())?;
        let fail = |err: String| -> ! {
            eprintln!("--style: {}: {err}", path.display());
            exit(1)
        };
        let text = fs::read_to_string(&path).unwrap_or_else(|err| fail(err.to_string()));
        Some(style::parse(&text).unwrap_or_else(|err| fail(err)))
    }
}

/// Writes the declarations of the namespace at `url` to `out`, for a later `--iris <out>`.
fn declare(url: &str, namespace: Option<&str>, out: Option<&Path>) {
    let namespace = namespace.unwrap_or(ascot::DEFAULT_IRIS_NAMESPACE);
    let conn = match RawAtelier::connect(url, namespace) {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!("--iris: {err}");
            exit(1);
        }
    };
    let out = match out {
        Some(out) => out.to_path_buf(),
        None => match default_declaration_file(url, namespace) {
            Ok(path) => {
                eprintln!("declare: no file given; writing to {}", path.display());
                path
            }
            Err(err) => {
                eprintln!("declare: {err}");
                exit(1);
            }
        },
    };
    let started = Instant::now();
    let mut bars = std::io::stderr().is_terminal().then(ProgressBars::default);
    let declarations = conn.declarations(|kind, done, total| {
        if let Some(bars) = &mut bars {
            bars.update(kind, done, total);
        }
    });
    match declarations.and_then(|decls| decls.save(&out).map(|()| decls)) {
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

/// `~/.ascot/<host>-<port>[-<prefix>]-<namespace>.d.json`, its directory created.
fn default_declaration_file(url: &str, namespace: &str) -> Result<PathBuf, String> {
    let dir = std::env::home_dir()
        .ok_or("cannot find the home directory; give the file to write")?
        .join(".ascot");
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let parsed = url::Url::parse(url).map_err(|e| e.to_string())?;
    let mut parts = vec![parsed.host_str().unwrap_or("iris").to_string()];
    parts.extend(parsed.port_or_known_default().map(|port| port.to_string()));
    parts.extend(
        parsed
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    );
    parts.push(namespace.to_string());
    let name: String = (parts.join("-").chars())
        .map(|c| {
            if c.is_alphanumeric() || ".-_%".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    Ok(dir.join(format!("{name}.d.json")))
}

async fn run_mcp_server(shared_fs: bool) {
    use ascot::mcp::Server;
    use rmcp::ServiceExt;

    let server = match Server::new(shared_fs).serve(rmcp::transport::stdio()).await {
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

/// Add every `.cls`/`.mac`/`.int`/`.inc` file under each of `paths` to a workspace and report
/// all diagnostics; under a `root`, the workspace is every file under it and `paths` choose which
/// are reported. A path that doesn't exist is an error, not an empty result: a typo'd path in a
/// CI step must not pass forever.
fn analyze<'a>(
    root: Option<&Path>,
    paths: impl Iterator<Item = &'a Path>,
    iris: Option<Box<dyn IrisContext>>,
    style: Option<Style>,
    filter: Filter,
    format: Format,
) {
    // Under a root, the paths are looked up among its files, so both are made canonical.
    let canonical = |path: &Path| {
        fs::canonicalize(path).unwrap_or_else(|e| {
            eprintln!("Error resolving {}: {}", path.display(), e);
            exit(1)
        })
    };
    let root = root.map(canonical);
    let mut files = Vec::new();
    for path in paths {
        if root.is_some() {
            collect_doc(&canonical(path), &mut files);
        } else if path.exists() {
            collect_doc(path, &mut files);
        } else {
            eprintln!("Error: {} does not exist", path.display());
            exit(1);
        }
    }
    files.sort();
    files.dedup();
    let loaded = match &root {
        Some(root) => {
            let mut all = Vec::new();
            collect_doc(root, &mut all);
            all.sort();
            files.retain(|file| {
                let under_root = all.binary_search(file).is_ok();
                if !under_root {
                    eprintln!(
                        "Warning: {} is not under workspace root {}; skipping",
                        file.display(),
                        root.display()
                    );
                }
                under_root
            });
            all
        }
        None => files.clone(),
    };

    let workspace = Workspace::native(iris, style);
    workspace.set_diagnostic_filter(filter);
    insert_files(&workspace, &loaded);

    let count = report_diagnostics(&workspace, &files, format);
    eprintln!("{} diagnostic(s) across {} file(s)", count, files.len());
    // `exit` runs no destructors; dropping the workspace ends its IRIS session.
    drop(workspace);
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
fn report_diagnostics(workspace: &Workspace, files: &[PathBuf], format: Format) -> usize {
    use ascot::bindings::DiagnosticSeverity::*;
    let files: Vec<FileDiagnostics> = files
        .iter()
        .map(|file| {
            let uri = file.display().to_string();
            let diagnostics = workspace.diagnostics(uri.clone());
            FileDiagnostics { uri, diagnostics }
        })
        .collect();
    let count = files
        .iter()
        .flat_map(|file| &file.diagnostics)
        .filter(|diagnostic| matches!(diagnostic.severity, Error | Warning))
        .count();
    match format {
        Format::Text => {
            let color = std::io::stdout().is_terminal();
            for file in &files {
                for diagnostic in &file.diagnostics {
                    // Colored on a terminal; a plain severity prefix otherwise, for scripts.
                    let (on, off) = match (diagnostic.severity, color) {
                        (Error, true) => ("\x1b[31m", "\x1b[0m"),
                        (Warning, true) => ("\x1b[38;5;214m", "\x1b[0m"),
                        (Information | Hint, true) => ("\x1b[2m", "\x1b[0m"),
                        (Error, false) => ("error: ", ""),
                        (Warning, false) => ("warning: ", ""),
                        (Information | Hint, false) => ("hint: ", ""),
                    };
                    print!("{}{}{}{}", on, file.uri, diagnostic.describe(), off);
                }
            }
        }
        Format::Lsp => {
            serde_json::to_writer_pretty(std::io::stdout(), &DiagnosticsResult { files })
                .expect("writing to stdout");
            println!();
        }
        Format::Sarif => println!("{}", files.into_iter().collect::<ascot::sarif::Log>()),
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
