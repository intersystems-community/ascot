# tygr-objectscript

Grammars for InterSystems ObjectScript and the IRIS SQL embedded in it, written as
[`tygr`](https://crates.io/crates/tygr) grammar types: each `#[derive(Grammar)]` type is at
once the parser, the syntax tree node, and the printer, and can render itself as EBNF. This
is the parser behind [ascot](https://github.com/intersystems-community/ascot), published on
its own so other tools can reuse the grammar. Syntax only: type checking lives in `ascot`.

```sh
cargo add tygr-objectscript tygr
```

## Usage

```rust
use tygr::Grammar;
use tygr_objectscript::cos;

fn main() -> Result<(), tygr::Error> {
    let src = "Class Demo.Hello Extends %RegisteredObject\n{\n\n\
               ClassMethod Greet(name As %String) As %String\n{\n Quit \"Hello, \"_name\n}\n\n}\n";

    let cls = cos::ClassDefinition::parse(src)?;
    assert_eq!(cls.name.print(), "Demo.Hello");
    assert_eq!(cls.print(), src); // printing round-trips the source exactly

    // A method body is kept as raw text (it may be Python, SQL, or an expression), so parse
    // it separately as ObjectScript when its `Language`/`CodeMode` say it is one:
    for member in cls.member_list.members.seps.iter() {
        if let cos::Member::Method(method) = &**member {
            cos::ProcedureBody::scan(&method.body.prefixed.wrapped.print())?;
        }
    }

    // Routines (`.mac`/`.int`). `RoutineWithMacro` instead parses an `.inc` or a routine as a
    // sequence of `#` preprocessor lines and `$$$` macro calls, for macro expansion.
    cos::Routine::scan("ROUTINE Demo\n\tWrite \"hi\"\n\tQuit\n")
}
```

The modules:

- `cos`: class definitions (`ClassDefinition`, members, keywords), routines (`Routine`,
  labels, procedure blocks), the preprocessor (`RoutineWithMacro`, `#define`, `#include`,
  `#if`), and the command and expression language of method bodies (`ProcedureBody`,
  `Command`, `Expr`). Embedded `&sql(...)` hands off to `sql`.
- `sql`: the IRIS SQL statements that may appear in `&sql(...)`.

Every type offers `parse` (the tree), `scan` (well-formedness only), `print`
(back to source), and, through `tygr::GrammarRule`, `bnf_rule()` for its EBNF. Each
`tygr::Range<T>` in the tree carries the byte offsets of what it matched;
`tygr::OLC` maps those to line and column.

The crate also builds a small debugging binary, `tygr-objectscript <file>`, which parses one
class or routine, including every ObjectScript method body, and reports the first syntax
error of each.

## Coverage

The grammar is validated against a corpus of several thousand InterSystems system classes
and routines, and aims to accept everything the IRIS compiler does, including legacy forms
(dotted `Do` blocks, argumentless `If`, `$ZUTIL` calls). Method bodies in other languages
are kept as text and not parsed.

## License

MIT or Apache-2.0, at your option.
