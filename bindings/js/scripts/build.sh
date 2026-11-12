#!/usr/bin/env bash
# Builds the wasm core module from source, componentizes + transpiles it with jco, and
# compiles the hand-written TS wrapper on top. Everything this produces (generated/,
# lib/*.wasm, dist/) is gitignored -- run this before `npm pack`/`npm publish`, or point
# `npm link` at this directory for local development against another checkout.
set -euo pipefail

cd "$(dirname "$0")/.."

REPO_ROOT="../.."
CORE="lib/ascot.wasm"
COMPONENT="$(mktemp -t ascot.component.XXXXXX.wasm)"
GEN="generated"

trap 'rm -f "$COMPONENT"' EXIT

mkdir -p lib

echo "building $CORE from source ..."
# No features: the modes are native-only anyway, and the package is the checker alone.
(cd "$REPO_ROOT" && cargo build --target=wasm32-unknown-unknown --release --lib --no-default-features)
cp "$REPO_ROOT/target/wasm32-unknown-unknown/release/ascot.wasm" "$CORE"

echo "componentizing $CORE ..."
npx jco new "$CORE" -o "$COMPONENT"

echo "transpiling to $GEN ..."
rm -rf "$GEN"
mkdir -p "$GEN"
# --async-mode jspi is what lets the sync get-cls/get-inc/get-int-or-mac imports suspend on an async
# host (JS Promise Integration) instead of needing a worker. Every query export is listed as
# async: any of them may need a routine's macro expansion, which fetches `#include`s through
# get-inc, and a suspend inside an export not listed here throws `SuspendError`. open/close
# never reach an import (they only parse the header and cache), so they stay sync.
npx jco transpile "$COMPONENT" \
	--name ascot \
	--map "iris:ascot/imported=../dist/imports.js" \
	--async-mode jspi \
	--async-imports \
		"iris:ascot/imported#[method]iris-context.get-cls" \
		"iris:ascot/imported#[method]iris-context.get-inc" \
		"iris:ascot/imported#[method]iris-context.get-rtn" \
		"iris:ascot/imported#[method]atelier.query" \
		"iris:ascot/imported#[method]atelier.get-doc" \
	--async-exports \
		"iris:ascot/exported#[method]workspace.diagnostics" \
		"iris:ascot/exported#[method]workspace.inlay-hint" \
		"iris:ascot/exported#[method]workspace.definition" \
		"iris:ascot/exported#[method]workspace.references" \
		"iris:ascot/exported#[method]workspace.hover" \
		"iris:ascot/exported#[method]workspace.query-cls" \
		"iris:ascot/exported#[method]workspace.query-mem" \
		"iris:ascot/exported#[method]workspace.document-symbol" \
		"iris:ascot/exported#[method]workspace.workspace-symbol" \
	-o "$GEN"

# The pre-built declarations of the IRIS release this ascot defaults to, from the
# `declarations` GitHub Release, checksum verified: what `createWorkspace()` checks against
# when the host passes no env, as `--iris latest` does on the command line.
RELEASE=$(sed -n 's/^pub const DEFAULT_RELEASE: &str = "\(.*\)";/\1/p' "$REPO_ROOT/src/release.rs")
ASSET="iris-community-$RELEASE.d.json.gz"
echo "fetching $ASSET ..."
curl -sSLf --retry 3 --retry-all-errors "https://github.com/intersystems-community/ascot/releases/download/declarations/$ASSET" -o "lib/$ASSET"
curl -sSLf --retry 3 --retry-all-errors "https://github.com/intersystems-community/ascot/releases/download/declarations/$ASSET.sha256" -o "lib/$ASSET.sha256"
(cd lib && (sha256sum -c "$ASSET.sha256" 2>/dev/null || shasum -a 256 -c "$ASSET.sha256") >/dev/null)
rm "lib/$ASSET.sha256"
printf 'export const release = "%s";\nexport const file = new URL("./%s", import.meta.url);\n' "$RELEASE" "$ASSET" > lib/declarations.js
printf 'export const release: string;\nexport const file: URL;\n' > lib/declarations.d.ts

echo "compiling TypeScript wrapper ..."
npx tsc -p tsconfig.json

echo "done."
echo "  glue (bundled):    $GEN/ascot.js + interfaces/"
echo "  core wasm:         $GEN/ascot.core.wasm"
echo "  wrapper (bundled): dist/"
