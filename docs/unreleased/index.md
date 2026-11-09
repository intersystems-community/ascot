# ascot diagnostics

Every diagnostic ascot reports, from the checker's own definitions (`ascot diagnostics`). A code's page gives its severity and message, when it fires, what IRIS does with such code, what to change, and an example, in which a line ending in `//! A1234` is where that diagnostic lands. A code can be dropped or re-ranked with `-d`, as in `-d off:A1234`; see the README's Diagnostics Filter.

## Errors

| Code | Message |
| --- | --- |
| [A1073](A1073.md) | Cannot GOTO label '{label}' from inside a procedure block: it is outside the block |
| [A1727](A1727.md) | {err} |
| [A2357](A2357.md) | '{name}' is not a parameter of {class}{hint} |
| [A2716](A2716.md) | Too many arguments to the macro '{name}', which takes {expected} |
| [A2797](A2797.md) | The macro '{name}' is undefined{suggestions} |
| [A3119](A3119.md) | The member name {name} is defined twice |
| [A3139](A3139.md) | Cannot find member '{mem}' in {cls}{suggestions}{dynamic} |
| [A3286](A3286.md) | '{action}' is not a valid $ZSTRIP action: {reason} |
| [A3485](A3485.md) | {function} can't take '{name}', which is private to this procedure block |
| [A3850](A3850.md) | ${name} is not a recognized function{suggestions} |
| [A3942](A3942.md) | Property '{name}' is MultiDimensional, so `{name}()` names no node: give it a subscript |
| [A4025](A4025.md) | The procedure call requires the private variable '{name}' to be public |
| [A4304](A4304.md) | {what} is not allowed in {context} (IRIS: {iris}) |
| [A4439](A4439.md) | Cannot find routine '{routine}' in the workspace or on the IRIS instance |
| [A4652](A4652.md) | {function} takes only a plain local variable as its handle |
| [A4963](A4963.md) | Quit with {actual} inside control structure |
| [A4983](A4983.md) | Variable '{name}' is already declared |
| [A5090](A5090.md) | The macro '{name}' expands recursively without end |
| [A5268](A5268.md) | {function} takes no by-reference argument |
| [A5431](A5431.md) | A Caché Direct callback, `_name` or `_form!control`, raises <_CALLBACK SYNTAX> in IRIS, which has no client to call |
| [A5523](A5523.md) | Too many arguments: the callee takes {takes}, and this one is extra |
| [A5789](A5789.md) | CONTINUE is not within a loop |
| [A5967](A5967.md) | {source} is incompatible with {target} |
| [A6127](A6127.md) | Not enough arguments to the macro '{name}', which takes {expected} |
| [A6319](A6319.md) | Parameter '{name}' is a constant: it takes no arguments and can't be assigned |
| [A6587](A6587.md) | Invalid macro argument: {reason} |
| [A6781](A6781.md) | Expected return value of type '{wanted}' but got nothing |
| [A6851](A6851.md) | Cannot find label '{label}' in {routine} |
| [A6866](A6866.md) | Cannot NEW the private variable '{name}' in a procedure block, where every local variable is already NEW'd on entry |
| [A7095](A7095.md) | This value is undefined{expr} |
| [A7139](A7139.md) | $CASE's `:default` must be its last argument |
| [A7342](A7342.md) | Argument {position} of {function} can't be left empty |
| [A7461](A7461.md) | Method '{method}' does not exist in any superclass of {cls}, so ##super has nothing to call |
| [A7604](A7604.md) | Label '{label}' has no formal parameter list, so $$ cannot call it (IRIS: <PARAMETER>); `{label}()` would make it a function |
| [A7831](A7831.md) | Include file '{name}' is not defined in the current workspace |
| [A7895](A7895.md) | '{format}' is not a valid $FNUMBER format: {reason} |
| [A8061](A8061.md) | Cannot find class '{cls}' in the workspace or on the IRIS instance |
| [A8073](A8073.md) | {err} |
| [A8496](A8496.md) | Property '{name}' is not MultiDimensional, so it takes no subscripts |
| [A8820](A8820.md) | SET $VECTOR needs its third argument, the type |
| [A8892](A8892.md) | Variable '{name}' is {ty1} in one branch but {ty2} in another |
| [A8914](A8914.md) | Cannot reference instance-only member '{name}' in a class method |
| [A9180](A9180.md) | Cannot assign {source} to '{name}' declared as {target} |
| [A9322](A9322.md) | '{entry}' is not {kind} keyword IRIS accepts |
| [A9399](A9399.md) | ${name} is not a recognized special variable{suggestions} |

## Warnings

| Code | Message |
| --- | --- |
| [A1203](A1203.md) | Unreachable: every path before this statement has quit, returned or thrown |
| [A1368](A1368.md) | Not in a class context; this relies on `$this` being set when it runs |
| [A1817](A1817.md) | Variable '{name}' is already NEW'd; this NEW resets it to undefined, discarding its current value — `Kill {name}` says that explicitly |
| [A3303](A3303.md) | Cannot find routine '{routine}' in this workspace, with no IRIS connection to check it against |
| [A3717](A3717.md) | Class '{name}' is also declared in {other_uri} |
| [A3904](A3904.md) | The macro '{name}' is defined in the body of method '{method}'; IRIS keeps it for the methods compiled after it (alphabetically), but it belongs in an include file or at class level |
| [A4344](A4344.md) | An error here re-enters the handler {var} still names; set {var} to "" first |
| [A4418](A4418.md) | {call} {replacement} |
| [A4476](A4476.md) | Output parameter '{name}' is never assigned; its caller will get an undefined value |
| [A4556](A4556.md) | This call returns {class}, which Do discards -- consider Set to capture it, or checking it directly |
| [A4718](A4718.md) | Cannot find member '{mem}' in {cls}, an abstract class: the value is a subclass's, which may have it{suggestions} |
| [A5156](A5156.md) | ${function} is documented to take at most {documented} argument(s), but was given {given}; IRIS tolerates the extra |
| [A5579](A5579.md) | Passing {actual} argument to '{name}', {wanted} parameter |
| [A5718](A5718.md) | Variable '{name}' is assigned but never read |
| [A6194](A6194.md) | {what} is not documented, though IRIS compiles it |
| [A6203](A6203.md) | '{entry}' is not a class keyword as written, though IRIS compiles it |
| [A6274](A6274.md) | Assigning {source}, an abstract class, to '{name}' declared as {target}: the value is a subclass's, which may be one |
| [A6613](A6613.md) | ZQUIT wants an integer of 1 or more; {value} acts as a bare ZQUIT and clears the whole stack |
| [A6705](A6705.md) | Too few arguments: '{next_arg}' is required. Only arguments with default values and `Output` arguments are considered optional. |
| [A6826](A6826.md) | This '{ty}' value may be undefined{expr} |
| [A7155](A7155.md) | ObjectScript has no operator precedence and evaluates strictly left-to-right; this may not compute what you expect |
| [A7408](A7408.md) | Label '{label}' is inside nested braces; ascot only understands labels at the top level of a method or routine body, so the checks after it may be inaccurate or incomplete |
| [A7548](A7548.md) | Missing an explicit QUIT or RETURN; execution falls through to whatever follows |
| [A7731](A7731.md) | {value} doesn't match the pattern of {what} |
| [A8139](A8139.md) | {source} is abstract, so this value is a subclass's, which may be the {target} it is taken for |
| [A8317](A8317.md) | '{meta_type}' is not a recognized parameter type; available: {available} |
| [A8456](A8456.md) | This condition is always {value} |
| [A8997](A8997.md) | Unexpected return value of type '{actual}' |
| [A9421](A9421.md) | {value} is a number here, implicitly converted to the string "{value}" |
| [S2839](S2839.md) | {kind} `{found}` should be written `{expected}` |
| [S3627](S3627.md) | Put {wanted} before `}}` |
| [S3918](S3918.md) | The style allows {max} `New` per block; this is one more |
| [S4151](S4151.md) | Use `Return` to leave a method or procedure; `Quit` is for loops and `Try` blocks |
| [S4933](S4933.md) | Put {wanted} after `{{` |
| [S5260](S5260.md) | The style allows {max} command(s) per line; this is one more |
| [S5583](S5583.md) | Put {wanted} |
| [S6307](S6307.md) | Put the postconditional on the command keyword, not on its argument |
| [S7126](S7126.md) | Describe this {kind}: its body is {lines} lines |
| [S7442](S7442.md) | Join these conditions with `{wanted}` |
| [S8215](S8215.md) | Put {wanted} before `{{` |
| [S9048](S9048.md) | Put {wanted} between `}}` and `{keyword}` |

## Information

| Code | Message |
| --- | --- |
| [A4043](A4043.md) | Type checking not implemented for '{what}' |

## Hints

| Code | Message |
| --- | --- |
| [A1054](A1054.md) | Cannot {lookup} |
| [A1765](A1765.md) | A $ETRAP that only jumps to a label is what `Set $ZTrap = "{label}"` does; $ZTRAP is scoped to this frame, where $ETRAP is inherited by callees |
| [A3794](A3794.md) | The labels of routine '{routine}' aren't known, so calls into it can't be checked precisely |
| [A3913](A3913.md) | {what} is not checked |
| [A4471](A4471.md) | Cannot look up {lookup} on the IRIS instance ({reason}), so it is taken on trust |
| [A5213](A5213.md) | The label is confirmed, but not where its offset lands: the call is checked as untyped and the offset is not checked |
| [A5362](A5362.md) | '{name}' is a user-defined command (^%ZLANGC00), so the call is not checked |
| [A5540](A5540.md) | Cannot decide `#if {condition}`{expansion} at compile time, so its first branch is assumed |
| [A5780](A5780.md) | ZQUIT unwinds here only because this procedure block has no private variables; one with any fails with <COMMAND>, so its error handling may not be what you expect |
| [A5847](A5847.md) | {call} is not documented for IRIS; $ZUTIL was superseded by class methods and special variables |
| [A6037](A6037.md) | ${name} is not documented for IRIS, so the call is not checked |
| [A7762](A7762.md) | '{name}' is never read before it is assigned; `Output` fits better than `ByRef` |
| [A7781](A7781.md) | '{name}' is read before it is assigned; `ByRef` fits better than `Output` |
| [A9360](A9360.md) | Cannot confirm '{mem}' exists on {on}{cls} — its class hierarchy includes {unseen} |

