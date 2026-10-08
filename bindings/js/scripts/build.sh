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
(cd "$REPO_ROOT" && cargo build --target=wasm32-unknown-unknown --release --lib)
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
	--instantiation async \
	--async-mode jspi \
	--async-imports \
		"iris:ascot/imported#[method]iris-connection.get-cls" \
		"iris:ascot/imported#[method]iris-connection.get-inc" \
		"iris:ascot/imported#[method]iris-connection.get-int-or-mac" \
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
	--no-nodejs-compat \
	-o "$GEN"

# We always pass getCoreModule to instantiate(), so jco's fetch(import.meta.url)
# fallback is dead code. Drop it: its dynamic `new URL(..., import.meta.url)` makes
# bundlers glob the whole generated dir (including .d.ts) into a context module.
echo "stripping dead fetch fallback ..."
perl -0pi -e 's{^\s*if \(!getCoreModule\) getCoreModule = .*fetchCompile.*$}{    if (!getCoreModule) throw new Error("getCoreModule is required");}m' "$GEN/ascot.js"

# The core wasm modules are read via fs at runtime (not bundled as JS), so they live in
# lib/ alongside the original ascot.wasm rather than in the generated dir.
echo "moving core wasm to lib/ ..."
mv "$GEN"/*.core*.wasm lib/

echo "compiling TypeScript wrapper ..."
npx tsc -p tsconfig.json

echo "done."
echo "  glue (bundled):    $GEN/ascot.js + interfaces/"
echo "  core wasm (fs):    lib/*.wasm"
echo "  wrapper (bundled): dist/"
