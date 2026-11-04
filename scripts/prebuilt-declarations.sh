#!/usr/bin/env bash
# Builds the pre-built declaration file of an IRIS Community release: starts the release's Docker
# image, runs `ascot declare` against its %SYS namespace, adds the label names of the routines it
# ships as object code only (read with ZLOAD in an `iris session`, which no API offers), composes
# src/mask.json's corrections into it, then stops the container.
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

python3 - "$OUT" "$ROOT/src/mask.json" "$NAME" <<'PY'
import json, re, subprocess, sys
out, mask, name = sys.argv[1:4]
d = json.load(open(out))
mask = json.load(open(mask))
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
for r, info in mask["routines"].items():
    known = {l["name"]: l for l in d["routines"].setdefault(r, {"labels": None, "uri": None})["labels"] or []}
    known.update((l["name"], l) for l in info["labels"])
    d["routines"][r]["labels"] = sorted(known.values(), key=lambda l: l["name"])
# A mask member replaces the base member of its name. It may change only types, modes, defaults
# and the return type: a member of another kind or arity than IRIS reports is a mistake.
KINDS = "ClassMethod|Method|ClientMethod|Property|Parameter|Relationship|Index|Query|Trigger|XData|Projection|ForeignKey"
def shape(header):
    kind, name, rest = re.match(rf'({KINDS}) ("[^"]*"|[^\s(]+)(.*)', header).groups()
    args = []
    if rest.startswith("("):
        depth, quoted, arg = 0, False, ""
        for ch in rest[1:]:
            quoted ^= ch == '"'
            if not quoted:
                depth += (ch in "({") - (ch in ")}")
                if depth < 0 or (ch == "," and depth == 0):
                    if arg.strip():
                        args.append(re.match(r"\s*(?:ByRef |Output )?(%?\w+)", arg).group(1))
                    arg = ""
                    if depth < 0:
                        break
                    continue
            arg += ch
    return kind, name.strip('"'), args
NS = {"ClassMethod": "Method", "ClientMethod": "Method", "Relationship": "Property"}
def key(header):
    kind, name, _ = shape(header)
    return NS.get(kind, kind), name.lower()
replaced = 0
for cls, masked in mask["classes"].items():
    base = d["classes"].get(cls)
    if base is None:
        sys.exit(f"mask.json: {cls} is not in the declaration")
    members = {key(m["header"]): m for m in base["members"]}
    for m in masked["members"]:
        kind, mname, args = shape(m["header"])
        target = members.get(key(m["header"]))
        if target is None:  # a member another release reports on this class
            print(f"mask.json: {cls}.{mname} is not in the declaration", file=sys.stderr)
            continue
        if shape(target["header"])[::2] != (kind, args):
            sys.exit(f"mask.json: {cls}.{mname} differs from IRIS in kind or arguments:\n  {target['header']}\n  {m['header']}")
        target["header"] = m["header"]
        replaced += 1
    names = {shape(m["header"])[1].lower() for m in base["members"]}
    for name in masked.get("drop", []):
        if name.lower() not in names:
            sys.exit(f"mask.json: {cls}.{name} is not in the declaration")
    base["members"] = [m for m in base["members"] if shape(m["header"])[1].lower() not in {n.lower() for n in masked.get("drop", [])}]
json.dump(d, open(out, "w"), separators=(",", ":"))
print(f"{named} of {len(compiled)} compiled-only routines got their label names; "
      f"{replaced} member(s) replaced and {sum(len(c.get('drop', [])) for c in mask['classes'].values())} dropped by the mask")
PY
