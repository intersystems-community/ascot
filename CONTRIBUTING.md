# Contributing to Ascot

Open an issue before starting a pull request; PRs without one will be asked to
open it. Submit PRs from a fork.

## Development

```sh
cargo build
cargo test --all-targets
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

`ascot lsp` runs the stdio LSP server natively.

### npm package

The JS bindings in `bindings/js/` are published as
[`@intersystems-community/ascot`](https://www.npmjs.com/package/@intersystems-community/ascot);
the language server depends on them.

```sh
cd bindings/js
npm install   # first time only
npm run build
```

To test against another checkout without publishing, `npm run build && npm link`
here, then `npm link @intersystems-community/ascot` there.

### Releasing

Pushing a `v*.*.*` tag builds the `ascot` binary for Linux, macOS (Intel and
Apple silicon) and Windows, attaches them to a GitHub release, and publishes
the npm package at the version in `bindings/js/package.json`; bump that by hand
first.

## License

Contributions are dual licensed under [Apache-2.0](LICENSE-APACHE) and
[MIT](LICENSE-MIT), without any additional terms or conditions.
