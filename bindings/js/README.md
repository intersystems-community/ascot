# @intersystems-community/ascot

[ascot](https://github.com/intersystems-community/ascot) is a type checker and code
intelligence engine for InterSystems ObjectScript. This package is its WebAssembly build
for Node and other JavaScript hosts, with a small TypeScript wrapper. It is the engine
behind the InterSystems Language Server's ascot features; it does not include the `ascot`
command-line tool.

```sh
npm install @intersystems-community/ascot
```

## Usage

````ts
import { createWorkspace } from "@intersystems-community/ascot";

const ws = await createWorkspace();

// `open` is both open and edit: it stores the raw text (the kind comes from the uri's
// extension: .cls, .mac, .int, .inc) and returns the uris whose cached analysis is now
// stale, this one and anything that includes or references it.
await ws.open("file:///T.cls", `Class T Extends %RegisteredObject
{
ClassMethod M() As %Status
{
 Set x = 1
 Write x
 Quit
}
}
`);

await ws.diagnostics("file:///T.cls");
// [{ severity: "error", message: "Expected return value of type '%Status' but got nothing",
//    range: { start: { line: 6, character: 5 }, ... }, relatedInformation: [...],
//    code: "A6781", codeDescription: "https://docs.rs/ascot/.../enum.TypeErrorKind.html#variant.ReturnNothingForX" }]

await ws.hover("file:///T.cls", { line: 5, character: 7 }); // the `x` being written
// "```objectscript\n%DataType = 1\n```"
````

To provide declarations for documents that aren't in the workspace, extend `IrisConnection`,
override the lookups you can answer, and pass an instance:

```ts
import { IrisConnection, type ClassInfo } from "@intersystems-community/ascot";

class MyIris extends IrisConnection {
  override async getCls(cls: string): Promise<ClassInfo | undefined> {
    return undefined; // look `cls` up, e.g. over IRIS's REST API
  }
}

const connected = await createWorkspace(new MyIris());
```

Or extend `Atelier` with the two Atelier REST API requests, `query` (the rows of
`/action/query`, each a JSON string) and `getDoc` (the lines of `/doc/{name}`), and let ascot
make the lookups itself:

```ts
import { Atelier } from "@intersystems-community/ascot";

class MyAtelier extends Atelier {
  override async getDoc(name: string): Promise<string[] | undefined> {
    return undefined; // GET /doc/{name}, e.g. with your own credentials and session
  }
}

const connected = await createWorkspace(new MyAtelier());
```

`createWorkspace` takes an optional second argument, a `Style` applied to every document:

```ts
const strict = await createWorkspace(undefined, { command: { form: "long" }, function: { case: "pascal" }, commandDo: { form: "short" } });
```

Load every file of a project into one `Workspace` so cross-file references resolve, and
call `open` again on each edit. `close(uri)` removes a document; a closed editor tab is
usually not a removal, so keep the on-disk text open instead.

## API

`createWorkspace(iris?, style?)` returns a `Workspace` whose methods mirror the engine's
exports, all returning promises. Positions and ranges are zero-based line/character pairs,
as in the Language Server Protocol.

| Method                                          | Returns                                               |
| ----------------------------------------------- | ----------------------------------------------------- |
| `open(uri, text)`                               | uris whose analysis is now stale                      |
| `close(uri)`                                    | uris whose analysis is now stale                      |
| `diagnostics(uri)`                              | `Diagnostic[]`                                        |
| `inlayHint(uri, range)`                         | `InlayHint[]`                                         |
| `definition(uri, position)`                     | `Location \| undefined`                               |
| `references(uri, position, includeDeclaration)` | `Location[]`                                          |
| `hover(uri, position)`                          | markdown `string \| undefined`                        |
| `documentSymbol(uri)`                           | the class or routine and its members or labels        |
| `workspaceSymbol(query)`                        | classes and members across the workspace by prefix    |
| `queryCls(query)`                               | `[uri, ClassInfo][]` for classes by prefix            |
| `queryMem(cls, query)`                          | `[uri, MemberInfo][]` for a class's members by prefix |

All result types (`Diagnostic`, `Location`, `ClassInfo`, `MemberInfo`, ...) are exported
from the package.

Calls are serialized: one runs at a time, and the rest queue behind it, across every
`Workspace` created by this package, since they share one wasm instance.

## Bundling

The core wasm modules are read from this package's `lib/` directory at runtime, relative
to the wrapper's own location. If you bundle the JavaScript, copy `lib/*.core*.wasm` to a
`lib/` directory next to your bundle's output directory.

## License

MIT or Apache-2.0, at your option.
