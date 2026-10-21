//! The text of `ascot help`, `ascot help <command>` and `ascot <command> --help`, and the usage
//! line printed when a command's arguments are wrong.

pub struct Command {
    pub name: &'static str,
    usage: &'static str,
    summary: &'static str,
    details: &'static str,
}

const IRIS_OPTIONS: &str = "\
Options:
  --iris <url|file>      An IRIS instance, as http://user:password@host:port[/prefix],
                         or a declaration file written by `ascot declare`. Resolves the
                         classes, include files and routines outside the workspace;
                         without it, they are left unconfirmed.
  --iris-namespace <ns>  The namespace to use (default %SYS). With a declaration file,
                         it must match the file's.
";

pub const COMMANDS: &[Command] = &[
    Command {
        name: "analyze",
        usage: "analyze [--iris <url|file>] [--iris-namespace <ns>] <path>...",
        summary: "Check every .cls/.mac/.int/.inc file under the given paths",
        details: "\
Loads every .cls/.mac/.int/.inc file under each <path> into one workspace and
prints the diagnostics of all of them. Exits non-zero if any error or warning was
found, or if a <path> doesn't exist; hints don't affect the exit code.
",
    },
    Command {
        name: "analyze-workspace",
        usage: "analyze-workspace [--iris <url|file>] [--iris-namespace <ns>] <root> <path>...",
        summary: "Check some paths, with a whole workspace loaded",
        details: "\
Loads every .cls/.mac/.int/.inc file under <root>, so cross-file references
resolve, but prints diagnostics only for the files under the given <path>s. Each
<path> must exist; one outside <root> is skipped with a warning. Exits like
`ascot analyze`.
",
    },
    Command {
        name: "parse",
        usage: "parse <file>",
        summary: "Report a file's syntax error, if any",
        details: "\
Parses <file> without type checking and prints its syntax error, if any. Exits
non-zero on a syntax error.
",
    },
    Command {
        name: "declare",
        usage: "declare --iris <url> [--iris-namespace <ns>] <file>",
        summary: "Save an IRIS namespace's declarations to a file",
        details: "\
Fetches everything the other commands would ask a live instance for and writes it
to <file>: every class's superclasses, members and their signatures and types, and
the source of every include file and routine. Pass <file> as --iris to analyze
without a live instance, for example in CI. Regenerate it after the instance's
code changes.

Options:
  --iris <url>           The IRIS instance, as http://user:password@host:port[/prefix].
  --iris-namespace <ns>  The namespace to declare (default %SYS).
",
    },
    Command {
        name: "mcp",
        usage: "mcp",
        summary: "Run the MCP server on stdio, for AI agents",
        details: "\
Runs the MCP server on stdio, exposing workspace analysis as tools an AI agent
calls on demand. Takes no options: a workspace's IRIS connection is set by the
`iris` and `iris_namespace` arguments of the `create_workspace` tool. For example:

  claude mcp add ascot -- ascot mcp
",
    },
    Command {
        name: "lsp",
        usage: "lsp [--iris <url|file>] [--iris-namespace <ns>]",
        summary: "Run the LSP server on stdio, for editors and AI agents",
        details: "\
Runs the LSP server on stdio, for editors and AI agents.
",
    },
];

pub fn find(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|cmd| cmd.name == name)
}

pub fn overview() -> String {
    let mut out = format!(
        "ascot {}: a type checker and code intelligence engine for InterSystems ObjectScript\n\n\
         Usage: ascot <command> [<args>]\n\nCommands:\n",
        env!("CARGO_PKG_VERSION")
    );
    let width = COMMANDS.iter().map(|cmd| cmd.name.len()).max().unwrap_or(0);
    for cmd in COMMANDS {
        out.push_str(&format!("  {:width$}  {}\n", cmd.name, cmd.summary));
    }
    out.push_str(&format!(
        "  {:width$}  Show this list, or a command's help\n\n\
         Run `ascot help <command>` for a command's options. More:\n\
         https://github.com/intersystems-community/ascot#usage\n",
        "help"
    ));
    out
}

impl Command {
    pub fn help(&self) -> String {
        let mut out = format!("Usage: ascot {}\n\n{}", self.usage, self.details);
        if self.usage.contains("<url|file>") {
            out.push('\n');
            out.push_str(IRIS_OPTIONS);
        }
        out
    }

    pub fn usage_error(&self) -> ! {
        eprintln!(
            "usage: ascot {}\nRun `ascot help {}` for details.",
            self.usage, self.name
        );
        std::process::exit(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_lists_every_command() {
        let overview = overview();
        for cmd in COMMANDS {
            assert!(
                overview.contains(&format!("  {}", cmd.name)),
                "{}",
                cmd.name
            );
        }
    }

    #[test]
    fn iris_options_appear_only_where_iris_is_optional() {
        assert!(
            find("analyze")
                .unwrap()
                .help()
                .contains("--iris-namespace <ns>  The namespace to use")
        );
        assert!(!find("parse").unwrap().help().contains("--iris"));
        assert!(!find("declare").unwrap().help().contains("<url|file>"));
    }
}
