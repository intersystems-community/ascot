# Style rules

A style is a JSON file passed with `--style <file>` to `analyze`, `analyze-workspace` and `lsp`; the MCP `create_workspace` call and the JavaScript `createWorkspace` take the same record directly. Its schema is the `style` record in [ascot.wit](src/wit/ascot.wit). Every key is optional, and only what's set is checked; what's written otherwise is reported as a warning whose code starts with `S`, so `-d off:S2839` or a severity filter sets style apart from the checker's own diagnostics.

```json
{
  "command": { "form": "Long", "case": "Pascal", "allow_postcondition_on_argument": false },
  "command_do": { "form": "Short", "allow_postcondition_on_argument": true },
  "function": { "form": "Long", "case": "Pascal" },
  "function_listbuild": { "form": "Short" },
  "allow_quit_for_return": false,
  "condition_and": "DoubleAmpersand",
  "max_new_per_block": 1,
  "max_commands_per_line": 1,
  "open_brace": { "newline_before": false, "newline_after": true },
  "close_brace": { "newline_before": true, "newline_after": false }
}
```

## Keywords

`command` applies to every command keyword, and `command_<name>` (the full name, lowercased: `command_do`, `command_tstart`) completes it field by field for one; `function` and `function_<name>` do the same for intrinsic functions, without the `$`.

| Field | Values | Reports |
|---|---|---|
| `form` | `Long`, `Short` | `s` under `Long`, `Set` under `Short`; a keyword with no short form is never reported. |
| `case` | `Lower`, `Upper`, `Pascal` | `SET` under `Pascal`. `Pascal` spells a keyword as the documentation does (`TStart`, `ZWrite`) and an abbreviation from its full name (`Li` for `List`, `LB` for `ListBuild`). |
| `allow_postcondition_on_argument` (commands only) | `true`, `false` | Under `false`, a postconditional on an argument (`Do ^R:x`, `Goto L:x`, `Xecute c:x`) rather than on the keyword (`Do:x ^R`). |

With `form` alone, any casing passes; with `case` alone, the form written is kept.

## Code shape

| Key | Values | Reports |
|---|---|---|
| `allow_quit_for_return` | `true`, `false` | Under `false`, a `Quit` that leaves the method or procedure, where `Return` belongs. A `Quit` that leaves a loop or a `Try` block is what `Quit` is for. |
| `condition_and` | `DoubleAmpersand`, `Comma` | Conditions of `If`, `ElseIf` and `While` joined the other way: `If a, b` under `DoubleAmpersand`, `If a && b` under `Comma`. |
| `max_new_per_block` | a count | Each `New` past the count in one block or routine body. A nested `{ }` block counts its own. |
| `max_commands_per_line` | a count | Each command past the count on one line. A brace-less `If`, `Else` or `For` and the commands it governs count as one. |
| `open_brace.newline_before` | `true`, `false` | A line break missing or present between a command and its block's `{`: `true` wants the `{` on its own line, `false` wants `If x {`. |
| `open_brace.newline_after` | `true`, `false` | Likewise between a block's `{` and its first command: `true` wants the body to start on the next line. |
| `close_brace.newline_before` | `true`, `false` | Likewise between a block's last command and its `}`: `true` wants the `}` on its own line. |
| `close_brace.newline_after` | `true`, `false` | Likewise between a block's `}` and an `Else`, `ElseIf` or `Catch` that follows: `false` wants `} Else {`, `true` wants the keyword on the next line. |
