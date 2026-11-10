#!/usr/bin/env bash
# Confirms that every class and routine of a Community release's namespace parses:
# builds the namespace's declaration file (scripts/prebuilt-declarations.sh, whose mask decides the
# `#if`s IRIS decides at compile time), starts the release's image, exports the namespace's documents
# over the Atelier API, and runs `ascot analyze` on them against that file for the two syntax codes
# alone (A1727, a document that doesn't parse; A8073, a method body that doesn't), failing on any.
# What IRIS compiles, ascot must parse.
#
# Usage: scripts/parse-coverage.sh [RELEASE] [IMAGE] [NAMESPACE]
#   RELEASE    the image tag, default 2026.2
#   IMAGE      the image, iris-community (default) or irishealth-community
#   NAMESPACE  the namespace, default %SYS; IRIS for Health keeps its HS and Ens classes in HSLIB
# Needs docker and python3; ASCOT names the binary to use (default: target/release/ascot, built
# if missing), PORT the host port for the container (default 52791; the declaration build takes
# the next), OUT the directory to export into (default: a new temporary one, kept for a look at
# what failed).
set -euo pipefail
RELEASE=${1:-2026.2}
IMAGE_NAME=${2:-iris-community}
NAMESPACE=${3:-%SYS}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
ASCOT=${ASCOT:-$ROOT/target/release/ascot}
PORT=${PORT:-52791}
OUT=${OUT:-$(mktemp -d)/$IMAGE_NAME-$RELEASE-${NAMESPACE//%/}}
NAME=ascot-parse-$IMAGE_NAME-$RELEASE-${NAMESPACE//%/}
IMAGE=containers.intersystems.com/intersystems/$IMAGE_NAME:$RELEASE

[ -x "$ASCOT" ] || (cd "$ROOT" && cargo build --release)
DECLARATIONS=$(dirname "$OUT")/$IMAGE_NAME-$RELEASE-${NAMESPACE//%/}.d.json
mkdir -p "$(dirname "$OUT")"
# A rerun into the same OUT keeps the declarations it built.
[ -s "$DECLARATIONS" ] ||
  ASCOT=$ASCOT PORT=$((PORT + 1)) "$ROOT/scripts/prebuilt-declarations.sh" "$RELEASE" "$DECLARATIONS" "$IMAGE_NAME" "$NAMESPACE"

docker run -d --rm --name "$NAME" -p "$PORT":52773 "$IMAGE" >/dev/null
trap 'docker stop "$NAME" >/dev/null' EXIT
echo "waiting for $IMAGE on port $PORT ..."
until [ "$(curl -s -o /dev/null -w '%{http_code}' "http://localhost:$PORT/api/atelier/")" != 000 ]; do sleep 2; done
# The image's passwords come expired, which every request minds.
docker exec "$NAME" iris session IRIS -U %SYS '##class(Security.Users).UnExpireUserPasswords("*")' >/dev/null

mkdir -p "$OUT"
python3 - "http://localhost:$PORT" "$NAMESPACE" "$OUT" <<'PY'
import base64, concurrent.futures, json, os, sys, urllib.error, urllib.parse, urllib.request
base, namespace, out = sys.argv[1:4]
auth = "Basic " + base64.b64encode(b"_SYSTEM:SYS").decode()
def call(path, body=None):
    url = f"{base}/api/atelier/v1/{urllib.parse.quote(namespace)}{path}"
    data = json.dumps(body).encode() if body is not None else None
    headers = {"Authorization": auth, "Content-Type": "application/json"}
    return json.load(urllib.request.urlopen(urllib.request.Request(url, data, headers), timeout=300))["result"]
# The document list leaves out the hidden classes (`%Compiler.LG`, `%CPT`, a third of `%Library`),
# which the dictionary has; a deployed class has no source to parse. The routines come from the
# document list, without the compiler's own (a class's `.1.int`, a generator's `.G1.mac`), and
# without the include files: those the declaration file supplies, its mask deciding what IRIS
# decides at compile time, where a copy in the workspace would shadow it undecided.
classes = call("/action/query", {"query": "SELECT Name FROM %Dictionary.ClassDefinition WHERE Deployed = 0"})["content"]
names = sorted({row["Name"] + ".cls" for row in classes}
               | {d["name"] for d in call("/docnames/RTN/*")["content"]
                  if d["name"].lower().endswith((".mac", ".int"))})
def fetch(name):
    try:
        content = call("/doc/" + urllib.parse.quote(name)).get("content")
    except urllib.error.HTTPError as err:
        return f"{name}: HTTP {err.code}"
    if not isinstance(content, list):
        return f"{name}: no source"
    with open(os.path.join(out, name), "w") as f:
        f.write("\n".join(content))
    return None
with concurrent.futures.ThreadPoolExecutor(8) as pool:
    skipped = [reason for reason in pool.map(fetch, names) if reason]
print(f"{len(names) - len(skipped)} of {len(names)} documents of {namespace} exported to {out}")
for reason in skipped:
    print("skipped", reason)
PY

# The two syntax codes alone, so the exit code is theirs.
"$ASCOT" analyze --root "$OUT" --iris "$DECLARATIONS" -d off:all -d on:A1727,A8073 "$OUT"
