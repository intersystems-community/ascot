#!/usr/bin/env bash
# Builds the pre-built declaration file of an IRIS Community release: starts the release's Docker
# image, runs `ascot declare` against its %SYS namespace, adds the label names of the routines it
# ships as object code only (read with ZLOAD in an `iris session`, which no API offers) and the
# curated signatures in prebuilt-labels.json, then stops the container.
#
# Usage: scripts/prebuilt-declarations.sh [RELEASE] [OUT]
#   RELEASE  the iris-community image tag, default 2026.2
#   OUT      the file to write, default iris-community-RELEASE.d.json
# Needs docker and python3; ASCOT names the binary to use (default: target/release/ascot, built
# if missing), PORT the host port for the container (default 52790).
set -euo pipefail
RELEASE=${1:-2026.2}
OUT=${2:-iris-community-$RELEASE.d.json}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
ASCOT=${ASCOT:-$ROOT/target/release/ascot}
PORT=${PORT:-52790}
NAME=ascot-declare-$RELEASE
IMAGE=containers.intersystems.com/intersystems/iris-community:$RELEASE

[ -x "$ASCOT" ] || (cd "$ROOT" && cargo build --release)
docker run -d --rm --name "$NAME" -p "$PORT":52773 "$IMAGE" >/dev/null
trap 'docker stop "$NAME" >/dev/null' EXIT
echo "waiting for $IMAGE on port $PORT ..."
until [ "$(curl -s -o /dev/null -w '%{http_code}' "http://localhost:$PORT/api/atelier/")" != 000 ]; do sleep 2; done
# The image's passwords come expired, which the terminal below minds.
docker exec "$NAME" iris session IRIS -U %SYS '##class(Security.Users).UnExpireUserPasswords("*")' >/dev/null

"$ASCOT" declare --iris "http://_SYSTEM:SYS@localhost:$PORT" "$OUT"

python3 - "$OUT" "$ROOT/scripts/prebuilt-labels.json" "$NAME" <<'PY'
import json, subprocess, sys
out, overlay, name = sys.argv[1:4]
d = json.load(open(out))
# A routine declared without labels has no source: ask the terminal for its object code's labels,
# clearing the routine buffer first so a failed ZLOAD doesn't report the previous routine's.
compiled = [r for r, info in d["routines"].items() if info["labels"] is None]
script = "".join(f'zr\nzl {r}\nw "{r}=",$lts($zu(135,5)),!\n' for r in compiled) + "h\n"
session = subprocess.run(["docker", "exec", "-i", name, "iris", "session", "IRIS", "-U", "%SYS"],
                         input=script, capture_output=True, text=True)
named = 0
for line in session.stdout.splitlines():
    r, sep, labels = line.partition("=")
    if sep and d["routines"].get(r, {}).get("labels") is None and labels:
        d["routines"][r]["labels"] = [{"name": l, "signature": None} for l in labels.split(",")]
        named += 1
for r, labels in json.load(open(overlay)).items():
    info = d["routines"].setdefault(r, {"labels": None, "uri": None})
    known = {l["name"]: l for l in info["labels"] or []}
    for l, signature in labels.items():
        known[l] = {"name": l, "signature": signature}
    info["labels"] = sorted(known.values(), key=lambda l: l["name"])
json.dump(d, open(out, "w"), separators=(",", ":"))
print(f"{named} of {len(compiled)} compiled-only routines got their label names; "
      f"{sum(map(len, json.load(open(overlay)).values()))} curated signatures merged")
PY
