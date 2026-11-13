# Style rules

A style is a JSON file passed with `--style <file>` to `analyze`, `format` and `lsp`; the MCP `create_workspace` call and the JavaScript `createWorkspace` take the same record directly. Its schema is the `style` record in [ascot.wit](src/wit/ascot.wit). Every key is optional, and only what's set is checked; what's written otherwise is reported as a warning whose code starts with `S`, so `-d off:S2839` or a severity filter sets style apart from the checker's own diagnostics. `ascot format`, and the LSP formatting request, rewrite what the `command` keyword rules and the `open_brace`/`close_brace` rules report, inside procedure blocks and method bodies; the other rules are reported alone.

```json
{
  "command": { "form": "long", "case": "pascal", "allowPostconditionOnArgument": false },
  "commandDo": { "form": "short", "allowPostconditionOnArgument": true },
  "function": { "form": "long", "case": "pascal" },
  "functionListbuild": { "form": "short" },
  "allowQuitForReturn": false,
  "conditionAnd": "double-ampersand",
  "maxNewPerBlock": 1,
  "maxCommandsPerLine": 1,
  "requireDescription": { "minBodyLines": 5 },
  "openBrace": { "newlineBefore": false, "newlineAfter": true },
  "closeBrace": { "newlineBefore": true, "newlineAfter": false },
  "spaceAfterComma": true
}
```

## Keywords

`command` applies to every command keyword, and `command<Name>` (the full name, capitalized: `commandDo`, `commandTstart`) completes it field by field for one; `function` and `function<Name>` do the same for intrinsic functions, without the `$`.

| Field | Values | Reports |
|---|---|---|
| `form` | `long`, `short` | `s` under `long`, `Set` under `short`; a keyword with no short form is never reported. |
| `case` | `lower`, `upper`, `pascal` | `SET` under `pascal`. `pascal` spells a keyword as the documentation does (`TStart`, `ZWrite`) and an abbreviation from its full name (`Li` for `List`, `LB` for `ListBuild`). |
| `allowPostconditionOnArgument` (commands only) | `true`, `false` | Under `false`, a postconditional on an argument (`Do ^R:x`, `Goto L:x`, `Xecute c:x`) rather than on the keyword (`Do:x ^R`). |

With `form` alone, any casing passes; with `case` alone, the form written is kept.

## Code shape

| Key | Values | Reports |
|---|---|---|
| `allowQuitForReturn` | `true`, `false` | Under `false`, a `Quit` that leaves the method or procedure, where `Return` belongs. A `Quit` that leaves a loop or a `Try` block is what `Quit` is for. |
| `conditionAnd` | `double-ampersand`, `comma` | Conditions of `If`, `ElseIf` and `While` joined the other way: `If a, b` under `double-ampersand`, `If a && b` under `comma`. |
| `maxNewPerBlock` | a count | Each `New` past the count in one block or routine body. A nested `{ }` block counts its own. |
| `maxCommandsPerLine` | a count | Each command past the count on one line. A brace-less `If`, `Else` or `For` and the commands it governs count as one. |
| `requireDescription.minBodyLines` | a count | A method, or a routine label with a parameter list, whose body has at least that many lines and no description: `///` lines right above the method, comment lines right above the label. `0` asks it of every one; private methods are exempt. |
| `openBrace.newlineBefore` | `true`, `false` | A line break missing or present between a command and its block's `{`: `true` wants the `{` on its own line, `false` wants `If x {`. |
| `openBrace.newlineAfter` | `true`, `false` | Likewise between a block's `{` and its first command: `true` wants the body to start on the next line. |
| `closeBrace.newlineBefore` | `true`, `false` | Likewise between a block's last command and its `}`: `true` wants the `}` on its own line. |
| `closeBrace.newlineAfter` | `true`, `false` | Likewise between a block's `}` and an `Else`, `ElseIf` or `Catch` that follows: `false` wants `} Else {`, `true` wants the keyword on the next line. |
| `spaceAfterComma` | `true`, `false` | A list's comma spaced the other way: `true` wants one space after and none before (`$lb(1, 2)`, `If a, b`), `false` none on either side (`$lb(1,2)`). Two commas touching omit an argument and are left alone. |
