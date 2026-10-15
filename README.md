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

Prebuilt `ascot` binaries are attached to each
[GitHub release](https://github.com/intersystems-community/ascot/releases).

- **Linux** (x86_64 and ARM64) and **Windows** (x86_64): download the archive
  for your platform from the release page.
- **macOS**: one universal binary covers Intel and Apple silicon. Download it
  with `curl` rather than a browser, since Gatekeeper refuses to run an
  unsigned binary that a browser download has quarantined:

  ```sh
  curl -L https://github.com/intersystems-community/ascot/releases/latest/download/ascot-cli-universal-apple-darwin.tar.gz | tar xz
  ```

  If you already downloaded it with a browser, clear the quarantine flag
  instead:

  ```sh
  xattr -d com.apple.quarantine ascot
  ```

### Rust

Not published yet. Once it is, the crate will live at
[crates.io/crates/ascot](https://crates.io/crates/ascot):

```sh
cargo install ascot
```

### JavaScript binding

A library for Node and other JS consumers (the one the InterSystems Language
Server uses); it does not provide an `ascot` executable. Not published yet.
Once it is, the package will live at
[npmjs.com/package/@intersystems-community/ascot](https://www.npmjs.com/package/@intersystems-community/ascot):

```sh
npm install @intersystems-community/ascot
```

## Usage

### `ascot` as a CLI tool

```sh
ascot analyze <PATH>...
```

Loads every `.cls`/`.mac`/`.int`/`.inc` file under each `PATH` into one
workspace and prints diagnostics for all of them. Exits non-zero if any error
or warning was found, or if a `PATH` doesn't exist.

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

### Connecting to IRIS

Every command can use a live IRIS instance, reached through the Atelier REST
API, to resolve the system and library classes and routines that aren't part
of the local workspace. Without a connection, such a class or routine is left
unconfirmed (a hint, not an error) rather than checked against real IRIS
metadata. The commands take the connection differently:

- `analyze`, `analyze-workspace` and `lsp` take `--iris <url>` and
  `--iris-namespace <namespace>` (default `%SYS`) on the command line. The URL
  carries the credentials and, if the instance is served under one, its path
  prefix (`/iris` below):

  ```sh
  ascot analyze --iris http://user:password@localhost:8080/iris --iris-namespace USER <PATH>...
  ```

  The `ascot-lsp` plugin starts `ascot lsp` with no flags, so it never connects
  to IRIS.

- `mcp` takes no flags. The connection is set per workspace, through the
  `iris` and `iris_namespace` (default `%SYS`) arguments of the
  `create_workspace` tool call.

Not every workflow has a live instance at hand, for example CI or analysis of
code exported from another system. If you need system and library code resolved
without a connection, we'd like to hear how you'd want to supply it: a
pre-exported metadata bundle, a snapshot cached from an earlier connection, or
something else. Please
[open a feature request](https://github.com/intersystems-community/ascot/issues/new?template=feature_request.yml)
describing your setup.

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
