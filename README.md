# Ascot

Ties together knowledge from across your ObjectScript code base.

A type checker and code intelligence engine for InterSystems ObjectScript.
Use it as an **MCP** server, an **LSP** server, a **CLI** tool, or via the
[**InterSystems Language Server**](https://github.com/intersystems/language-server/).

## Features

- **Diagnostics**: flow-sensitive type checking — undefined-variable reads,
  assigned-but-never-read locals, argument count/mode mismatches, missing
  `QUIT`/`RETURN`, inconsistent variable declarations across branches, and
  more.
- **Hover**: type and signature info, or macro expansion (`.mac`/`.int`/`.inc`
  only) on hover.
- **Go to Definition**/**Find References**: for classes, class members, and
  same-routine labels/gotos.
- **Document/Workspace Symbols**: navigate classes and class members within a
  file or across the workspace.
- **Inlay Hints**: inferred types, argument names, and parameter default
  values shown inline.

## Usage

### Use as an MCP server

```sh
ascot mcp
```

Starts the MCP server (stdio). To register it with Claude Code:

```sh
claude mcp add ascot -- /path/to/ascot mcp
```

### Use as an LSP server

```sh
ascot lsp
```

Starts the LSP server (stdio), providing the features listed above to any
LSP client.

### Use as a CLI tool

```sh
ascot analyze [PATH] ...
```

Loads every `.cls`/`.mac`/`.int`/`.inc` file under each `PATH` into one
workspace and prints diagnostics for all of them. Exits non-zero if any error
or warning was found, or if a `PATH` doesn't exist.

```sh
ascot analyze-workspace [WORKSPACE-ROOT] [PATH] ...
```

Loads every `.cls`/`.mac`/`.int`/`.inc` file under `WORKSPACE-ROOT` into a single
workspace, so cross-file references resolve, but only prints diagnostics for
files under the given `PATH`(s). Useful for checking a subset of a large
workspace without losing cross-file context. Each `PATH` must be inside
`WORKSPACE-ROOT`; paths that aren't are skipped with a warning.

```sh
ascot parse FILE
```

Only parses `FILE` (no type checking) and prints the syntax error, if any.
Exits non-zero on a syntax error.

### Connecting to IRIS

Every command can use a live IRIS instance, reached through the Atelier REST
API, to resolve the system and library classes and routines that aren't part
of the local workspace. Without a connection, such a class or routine is left
unconfirmed (a hint, not an error) rather than checked against real IRIS
metadata. The commands take the connection differently:

- `analyze`, `analyze-workspace` and `lsp` take `--iris <url>` and
  `--iris-namespace <namespace>` on the command line. The URL carries the
  credentials and, if the instance is served under one, its path prefix
  (`/iris` below):

  ```sh
  ascot analyze --iris http://user:password@localhost:8080/iris --iris-namespace USER [PATH] ...
  ```

- `mcp` takes no flags. The connection is set per workspace, through the
  `iris` and `iris_namespace` arguments of the `create_workspace` tool call.

## Contributing

Please [open an issue](../../issues/new/choose) before starting a pull request.
See [CONTRIBUTING.md](CONTRIBUTING.md) for the development workflow.

## License

This software is provided **AS-IS**, with no implied warranties, support, or
conditions of any kind.

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this work by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
