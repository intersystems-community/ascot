# Contributing to Ascot

Open an issue before starting a pull request; PRs without one will be asked to
open it. Submit PRs from a fork.

Maintainers may open a PR from a branch without an issue for internal changes
(CI, tests, tooling); label it `internal` to leave it out of the release notes.

## Development

```sh
cargo build
cargo test --all-targets
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

`ascot lsp` runs the stdio LSP server natively.

### Against IRIS

`tests/iris.rs` and the JavaScript binding's `npm test` run against the instance in
`ASCOT_TEST_IRIS`, and do nothing without it. To run one in Docker (its default passwords
come expired):

```sh
docker run -d --name iris -p 52773:52773 containers.intersystems.com/intersystems/iris-community:2026.1
docker exec iris iris session IRIS -U %SYS '##class(Security.Users).UnExpireUserPasswords("*")'
ASCOT_TEST_IRIS=http://_SYSTEM:SYS@localhost:52773 cargo test --test iris
(cd bindings/js && ASCOT_TEST_IRIS=http://_SYSTEM:SYS@localhost:52773 npm test)
```

### npm package

The JS bindings in `bindings/js/` are published as
[`@intersystems-community/ascot`](https://www.npmjs.com/package/@intersystems-community/ascot);
the language server depends on them.

```sh
cd bindings/js
npm install   # first time only
npm run build
npm run format
```

To test against another checkout without publishing, `npm run build && npm link`
here, then `npm link @intersystems-community/ascot` there.

### Releasing

Pushing a `v*.*.*` tag builds the `ascot` binary for Linux, macOS (Intel and
Apple silicon) and Windows, attaches them to a GitHub release, publishes the
`tygr-objectscript` and `ascot` crates to crates.io at the versions in their
`Cargo.toml`s, and publishes the npm package at the version in
`bindings/js/package.json`; bump those by hand first. crates.io rejects a
version that is already published, so every tagged release needs a new version
of both crates.

## License

Contributions are dual licensed under [Apache-2.0](LICENSE-APACHE) and
[MIT](LICENSE-MIT), without any additional terms or conditions.
