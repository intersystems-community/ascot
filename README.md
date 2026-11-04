# Ascot

[![crates.io](https://img.shields.io/crates/v/ascot)](https://crates.io/crates/ascot)
[![npm](https://img.shields.io/npm/v/%40intersystems-community%2Fascot)](https://www.npmjs.com/package/@intersystems-community/ascot)
[![Claude Code plugins](https://img.shields.io/badge/Claude_Code-plugins-blue)](#claude-code-plugins)

Ties together knowledge from across your ObjectScript code base.

A type checker and code intelligence engine for InterSystems ObjectScript.
Use it as a [**CLI** tool](#ascot-as-a-cli-tool), from **AI agents** via
[MCP](#ascot-as-an-mcp-server) or [LSP](#ascot-as-an-lsp-server) protocols
(including [Claude Code plugins](#claude-code-plugins)), or via the
[**InterSystems Language Server**](https://github.com/intersystems/language-server/).

## Features

- **Diagnostics**: flow-sensitive type checking — undefined-variable reads,
  assigned-but-never-read locals, argument count/mode mismatches, missing
  `QUIT`/`RETURN`, inconsistent variable declarations across branches, and
  more.
- **Hover**: type and signature info, or macro expansion (`.mac`/`.int` only) on
  hover.
- **Go to Definition**/**Find References**: for classes, class members, and
  same-routine labels/gotos.
- **Document/Workspace Symbols**: navigate classes and class members within a
  file or across the workspace.
- **Inlay Hints**: inferred types, argument names, and parameter default
  values shown inline.

## Installation

### GitHub release

Prebuilt `ascot` binaries for Linux, macOS and Windows are attached to each
[GitHub release](https://github.com/intersystems-community/ascot/releases).

On macOS, Gatekeeper refuses to run the unsigned binary once a browser download
has quarantined it. [Homebrew](#homebrew-macos-and-linux) avoids this; otherwise
download it with `curl`:

```sh
curl -L https://github.com/intersystems-community/ascot/releases/latest/download/ascot-cli-universal-apple-darwin.tar.gz | tar xz
```

or clear the flag on a browser download:

```sh
xattr -d com.apple.quarantine path/to/ascot
```

### Homebrew (macOS and Linux)

From the
[InterSystems Community tap](https://github.com/intersystems-community/homebrew-tap):

```sh
brew trust intersystems-community/tap # if Homebrew 5.1.15 or later
brew install intersystems-community/tap/ascot
```

### Rust

From [crates.io/crates/ascot](https://crates.io/crates/ascot):

```sh
cargo install ascot
```

On Windows, building also needs the Visual Studio C++ build tools, which newer
rustup installers offer to set up; a build failing on `link.exe` or
`kernel32.lib` is missing them, see rustup's
[MSVC prerequisites](https://rust-lang.github.io/rustup/installation/windows-msvc.html).

### JavaScript binding

A library for Node 24+ and other JS consumers; it does not provide an `ascot`
executable. From
[npmjs.com/package/@intersystems-community/ascot](https://www.npmjs.com/package/@intersystems-community/ascot):

```sh
npm install @intersystems-community/ascot
```

## Usage

### `ascot` as a CLI tool

`ascot help` lists the commands, and `ascot help <COMMAND>` (or
`ascot <COMMAND> --help`) shows one's options; the IRIS and style ones are
described under [Advanced Usage](#advanced-usage).

```sh
ascot analyze <PATH>...
```

Loads every `.cls`/`.mac`/`.int`/`.inc` file under each `PATH` into one
workspace and prints diagnostics for all of them. Exits non-zero if any error
or warning was found, or if a `PATH` doesn't exist.
Both `analyze` commands also print JSON instead of text: `--format lsp` gives
the LSP diagnostic records per file, as the language server publishes and the
MCP `check` tool returns them, and `--format sarif` one SARIF 2.1.0 document
for code scanning. The summary stays on stderr and the exit code is the same.

```sh
ascot analyze-workspace <WORKSPACE-ROOT> <PATH>...
```

Loads every `.cls`/`.mac`/`.int`/`.inc` file under `WORKSPACE-ROOT` into a single
workspace, so cross-file references resolve, but only prints diagnostics for
files under the given `PATH`(s). Useful for checking a subset of a large
workspace without losing cross-file context. Each `PATH` must exist and be
inside `WORKSPACE-ROOT`; a nonexistent one is an error, and one outside the
root is skipped with a warning.

```sh
ascot parse FILE
```

Only parses `FILE` (no type checking) and prints the syntax error, if any.
Exits non-zero on a syntax error.

### `ascot` as an MCP server

```sh
ascot mcp
```

Starts the MCP server (stdio), exposing workspace analysis as tools an AI agent
calls on demand. Any MCP client that can launch a stdio server works: the
command is `ascot` (or its full path) and the single argument is `mcp`.

The `insert` tool takes each document's text, since the server can't assume it
shares a file system with the agent. When it does, start it with
`ascot mcp --shared-fs`: `insert` then reads a `file://` uri given without
`src` from disk, a single file or a whole directory of `.cls`/`.mac`/`.int`/`.inc`
files, and inserting a directory again re-reads it and drops the files no longer
there.

The tools and their parameters are documented on
[docs.rs](https://docs.rs/ascot/latest/ascot/mcp/struct.Server.html).
To try them by hand, the [MCP Inspector](https://github.com/modelcontextprotocol/inspector)
lists them and calls them from a browser:

```sh
npx @modelcontextprotocol/inspector ascot mcp
```

Claude Code:

```sh
claude mcp add ascot -- ascot mcp
```

OpenAI Codex CLI:

```sh
codex mcp add ascot -- ascot mcp
```

Cursor (`.cursor/mcp.json`), Claude Desktop (`claude_desktop_config.json`) and
Gemini CLI (`~/.gemini/settings.json`) share one shape:

```json
{
  "mcpServers": {
    "ascot": { "command": "ascot", "args": ["mcp"] }
  }
}
```

VS Code (`.vscode/mcp.json`):

```json
{
  "servers": {
    "ascot": { "type": "stdio", "command": "ascot", "args": ["mcp"] }
  }
}
```

### `ascot` as an LSP server

```sh
ascot lsp
```

Starts the LSP server (stdio).

### Claude Code plugins

This repository is a Claude Code plugin marketplace with two plugins, both
expecting `ascot` on `PATH`: `ascot-mcp` registers the MCP server above, and
`ascot-lsp` runs the LSP server and feeds its diagnostics back into the
conversation after every edit.

```sh
claude plugin marketplace add intersystems-community/ascot
claude plugin install ascot-mcp@ascot-plugins
claude plugin install ascot-lsp@ascot-plugins
```

## Advanced Usage

### Connecting to IRIS

The system and library classes and routines that aren't part of the local
workspace are resolved against IRIS, in one of three ways:

- **A release's pre-built declarations**, the default. `--iris 2026.2` names an
  IRIS Community release; ascot downloads that release's declaration file (about
  4 MB) from this repository's
  [`declarations` release](https://github.com/intersystems-community/ascot/releases/tag/declarations)
  the first time, says so, checks its checksum and keeps it in `~/.ascot/`.
  Without `--iris`, ascot uses the newest release it was built for, 2026.2. If
  the file isn't cached and can't be downloaded, ascot says so and goes on with
  no environment at all: every class, routine and include outside the workspace
  is then an error, the strictest reading. `ASCOT_DECLARATIONS_URL` points at
  another directory holding the assets, for a mirror.

  The pre-built files hold the classes as IRIS reports them, corrected by
  [`src/mask.json`](src/mask.json): constructors return their own class,
  collections their element type, optional arguments have their defaults.

- **A live instance**, reached through the Atelier REST API, for code that uses
  classes the Community image doesn't ship: `--iris <url>` and
  `--iris-namespace <namespace>` (default `%SYS`). The URL carries the
  credentials and, if the instance is served under one, its path prefix
  (`/iris` below):

  ```sh
  ascot analyze --iris http://user:password@localhost:8080/iris --iris-namespace USER src/
  ```

  An `https` instance's certificate is checked against the operating system's
  trust store, so one it trusts, such as a company CA's, works.

- **A declaration file** of your own, written by `ascot declare` (next section).

A host embedding the wasm build has the same choices through the `env` it
passes: its own lookups, an Atelier connection, or a declaration file's bytes,
gzipped or not, such as the pre-built asset it bundles.

`analyze`, `analyze-workspace` and `lsp` take these on the command line; `mcp`
takes them per workspace, through the `iris` and `iris_namespace` arguments of
the `create_workspace` tool call, with the same default.

#### Declaration files

Not every workflow has a live instance at hand, for example CI or analysis of
code exported from another system, and a live connection costs one round of
REST requests per class or routine looked up, every run. A declaration file
holds everything a connection would be asked for, fetched once:

```sh
ascot declare --iris http://user:password@localhost:8080/iris --iris-namespace USER user.d.json
```

Without the file argument, it writes to
`~/.ascot/<host>-<port>-<namespace>.d.json` and says so.

Wherever `--iris` (or the MCP `iris` argument) takes a URL, it also takes the
path of such a file, or a `file://` URL, and no REST request is made:

```sh
ascot analyze --iris user.d.json src/
```

`--iris-namespace` is then optional; if given, it must match the namespace the
file declares. The file holds every class's superclasses, members and their
signatures and types, as a live connection would report them (no method code),
the full source of every `.inc` file, and the labels and their arguments of every
routine, or, for a routine without source (e.g. only its OBJ), that it exists, along
with the instance's version.
Regenerate it after the instance's code changes.

### Style

A JSON file passed with `--style <file>` (or as the `style` of the MCP
`create_workspace` and the JavaScript `createWorkspace`) says how code should be
written; only what's set is checked, as a warning. For example, with

```json
{ "command": { "form": "Long", "case": "Pascal" }, "allow_quit_for_return": false }
```

```
$ ascot analyze --style style.json R.mac
warning: R.mac:3:2-3 - Command keyword `s` should be written `Set` [S2839]
warning: R.mac:4:2-8 - Use `Return` to leave a method or procedure; `Quit` is for loops and `Try` blocks [S4151]
```

Every rule is described in [STYLE.md](STYLE.md).

### Diagnostics Filter

`analyze`, `analyze-workspace` and `lsp` take `-d VERB:TARGETS` (long form
`--diagnostics`), repeatable, to choose which codes are reported and at what
severity; the MCP `create_workspace` call takes the same strings as its
`diagnostics` list. VERB is `off`, `on` (the code's own severity) or a severity
(`error`, `warning`, `information`, `hint`); TARGETS is a comma-separated list
of codes, severities (every code currently reported at one), or `all`. Values
apply in order, a later one overriding an earlier one:

```sh
ascot analyze -d off:A5579,A4043 src/            # drop two codes
ascot analyze -d off:hint,information src/       # errors and warnings only
ascot analyze -d off:all -d on:A7095,A6851 src/  # report only two
ascot analyze -d hint:A4471 src/                 # report one as a hint
```

The exit code follows what is reported, so a dropped or downgraded warning no
longer fails a build.

## Contributing

Please [open an issue](https://github.com/intersystems-community/ascot/issues/new/choose)
before starting a pull request. See [CONTRIBUTING.md](CONTRIBUTING.md) for the
development workflow, [GOVERNANCE.md](GOVERNANCE.md) for how the project is
run, and [SECURITY.md](SECURITY.md) for reporting vulnerabilities.

## License

This software is provided **AS-IS**, with no implied warranties, support, or
conditions of any kind.

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this work by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
