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

### Diagnostics

A private corpus of annotated ObjectScript pins what ascot reports on every file, hints and
all. It is not run in CI: unit tests are the only check a PR gets, so a PR that changes what
is reported should say so in its description, and the maintainer runs the corpus against it
and before every release.

### Pre-built declarations

`scripts/prebuilt-declarations.sh [RELEASE] [OUT]` writes the declaration file of an IRIS
Community release's `%SYS` namespace: it starts the release's Docker image, runs `ascot declare`
against it, adds the label names of the routines the image ships as object code only (read with
`ZLOAD` in an `iris session`, since no API lists them), composes `src/mask.json` into it, and
stops the container. The result is about 22 MB, 4 MB gzipped, and is not tracked here.

`src/mask.json` is ascot's set of corrections to what IRIS reports about its own classes, in the
declaration format: for each class, the headers of the members to replace, written in class
syntax, plus `drop` (members the compiler doesn't accept), `type` (what the class stands for) and
`generics` (the parameters that type members as `..#NAME`). A header may say `As This` for the
receiver's class. The script checks that each override keeps the member's kind and argument
names, so an override can only change types, modes, defaults and the return type. The same file
is embedded in ascot and applied over a live instance.

The generated files are published as assets of the `declarations` GitHub Release, a prerelease
on a fixed tag that is never GitHub's "latest" (which `releases/latest/download` must keep
resolving to an ascot build); its notes are `.github/declarations-release.md`. The assets are named
`iris-community-<release>.d.json.gz` with a `.sha256` beside each, which is where
`--iris <release>` downloads from. The `Declarations` workflow builds and uploads one
(`Run workflow`, with the release), or by hand:

```sh
scripts/prebuilt-declarations.sh 2026.2 iris-community-2026.2.d.json
gzip -9 iris-community-2026.2.d.json
shasum -a 256 iris-community-2026.2.d.json.gz > iris-community-2026.2.d.json.gz.sha256
gh release upload declarations iris-community-2026.2.d.json.gz{,.sha256} --clobber
```

Bump `DEFAULT_RELEASE` in `src/prebuilt.rs` when a new Community release's file is up. The
format's version, `DECLARATIONS_VERSION` in `src/iris.rs`, is semver: a file is readable while
its major part matches, so bump the major for a change old ascots can't read, the minor for an
addition, and re-upload every release's file after a major bump.

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
