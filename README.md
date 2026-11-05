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
`ascot <COMMAND> --help`) shows one's options; the IRIS, style and diagnostics
ones are the [settings](#settings) described under Advanced Usage.

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
calls on demand: `create_workspace` takes the [settings](#settings), `insert`
each document's text (or, with a [shared file system](#shared-file-system), its
`file://` uri). Any MCP client that can launch a stdio server works: the command
is `ascot` (or its full path) and the single argument is `mcp`.

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

Starts the LSP server (stdio). Its flags are the [settings](#settings) as
defaults: the client overrides them with `initializationOptions` and changes
them with `workspace/didChangeConfiguration`. A new `iris` re-checks the open
documents without re-reading them; a setting that can't be applied is reported
with `window/showMessage` and left as it was. With a
[shared file system](#shared-file-system), the workspace roots are loaded from
disk at startup.

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

### Settings

Which IRIS to ask, the style to check and the diagnostics to report are the same
three settings everywhere: `--iris`/`--iris-namespace`, `--style` and `-d` on
`analyze`, `analyze-workspace` and `lsp`; one object,
`{"iris": ..., "iris_namespace": ..., "style": ..., "diagnostics": ...}`, for
the MCP `create_workspace` and the LSP `initializationOptions` and
`workspace/didChangeConfiguration`, which the `ascot-lsp` plugin asks for when
enabled. A wasm host passes `iris` through `env` and `style` to
`createWorkspace`.

### Connecting to IRIS

Classes, routines and include files outside the workspace are resolved against
IRIS; `iris` says which:

- `--iris latest` (default): the newest declaration-only snapshot of IRIS this
  ascot knows, downloaded once (4 MB) from this repository's
  [`declarations` release](https://github.com/intersystems-community/ascot/releases/tag/declarations)
  into `~/.ascot`.
- `--iris <YYYY>.<N>`: the snapshot of that IRIS release.
- `--iris http://user:password@host:port[/prefix]` with `--iris-namespace`
  (default `%SYS`): a live connection to IRIS through the Atelier REST API.
- `--iris path/to/declarations.d.json`: a declaration file, a released one or
  one you [built yourself](#build-your-own-declaration-files). No REST
  requests, and no instance needed.
- `--iris none`: no environment, so every class, routine or include outside the
  workspace is an error.

#### Build Your Own Declaration Files

`ascot declare` saves what a live connection would be asked for, so CI and
machines without the instance can check against it:

```sh
ascot declare --iris http://user:password@localhost:8080/iris --iris-namespace USER path/to/declaration.d.json
```

Without the file argument it writes `~/.ascot/<host>-<port>-<namespace>.d.json`.

### Style

`style` is a JSON object (for `--style`, a file holding it) saying how code
should be written; only what's set is checked, as a warning. For example, with

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

`diagnostics` is `VERB:TARGETS` values, as a list or separated by spaces (`-d`,
long form `--diagnostics`, is also repeatable), choosing which codes are
reported and at what severity. VERB is `off`, `on` (the code's own severity) or
a severity (`error`, `warning`, `information`, `hint`); TARGETS is a
comma-separated list of codes, severities (every code currently reported at
one), or `all`. Values apply in order, a later one overriding an earlier one:

```sh
ascot analyze -d off:A5579,A4043 src/            # drop two codes
ascot analyze -d off:hint,information src/       # errors and warnings only
ascot analyze -d off:all -d on:A7095,A6851 src/  # report only two
ascot analyze -d hint:A4471 src/                 # report one as a hint
```

The exit code follows what is reported, so a dropped or downgraded warning no
longer fails a build.

### Shared file system

Neither server assumes it shares a file system with its client; `--shared-fs`
says it does. `mcp`'s `insert` then reads a `file://` uri given without `src`
from disk, a single file or a whole directory of `.cls`/`.mac`/`.int`/`.inc`
files, and inserting a directory again re-reads it and drops the files no longer
there. `lsp` then loads the workspace roots from disk at startup, so cross-file
references resolve before their files are opened, and its `style` may be a
file's path; `"shared_fs": true` in `initializationOptions` says the same.

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
