#!/usr/bin/env bash
# Builds the pre-built declaration file of one namespace of a Community release: starts the
# release's Docker image, runs `ascot declare` against the namespace, adds the label names of the
# routines it ships as object code only (read with ZLOAD in an `iris session`, which no API offers),
# composes src/mask.json's corrections into it, then stops the container.
#
# Usage: scripts/prebuilt-declarations.sh [RELEASE] [OUT] [IMAGE] [NAMESPACE]
#   RELEASE    the image tag, default 2026.2
#   OUT        the file to write, default IMAGE-RELEASE.d.json, or IMAGE-RELEASE.NAMESPACE.d.json
#              for a namespace other than %SYS
#   IMAGE      the image, iris-community (default) or irishealth-community
#   NAMESPACE  the namespace to declare, default %SYS; IRIS for Health keeps its HS and Ens
#              classes in HSLIB
# Needs docker and python3; ASCOT names the binary to use (default: target/release/ascot, built
# if missing), PORT the host port for the container (default 52790).
set -euo pipefail
RELEASE=${1:-2026.2}
IMAGE_NAME=${3:-iris-community}
NAMESPACE=${4:-%SYS}
DEFAULT_OUT=$IMAGE_NAME-$RELEASE.d.json
[ "$NAMESPACE" = %SYS ] || DEFAULT_OUT=$IMAGE_NAME-$RELEASE.$NAMESPACE.d.json
OUT=${2:-$DEFAULT_OUT}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
ASCOT=${ASCOT:-$ROOT/target/release/ascot}
PORT=${PORT:-52790}
NAME=ascot-declare-$IMAGE_NAME-$RELEASE-${NAMESPACE//%/}
IMAGE=containers.intersystems.com/intersystems/$IMAGE_NAME:$RELEASE

[ -x "$ASCOT" ] || (cd "$ROOT" && cargo build --release)
docker run -d --rm --name "$NAME" -p "$PORT":52773 "$IMAGE" >/dev/null
trap 'docker stop "$NAME" >/dev/null' EXIT
echo "waiting for $IMAGE on port $PORT ..."
until [ "$(curl -s -o /dev/null -w '%{http_code}' "http://localhost:$PORT/api/atelier/")" != 000 ]; do sleep 2; done
# The image's passwords come expired, which the terminal below minds.
docker exec "$NAME" iris session IRIS -U %SYS '##class(Security.Users).UnExpireUserPasswords("*")' >/dev/null

"$ASCOT" declare --iris "http://_SYSTEM:SYS@localhost:$PORT" --iris-namespace "$NAMESPACE" "$OUT"

python3 - "$OUT" "$ROOT/src/mask.json" "$NAME" "$NAMESPACE" <<'PY'
import json, re, subprocess, sys
out, mask, name, namespace = sys.argv[1:5]
d = json.load(open(out))
mask = json.load(open(mask))
# A routine declared without labels has no source: ask the terminal for its object code's labels,
# clearing the routine buffer first so a failed ZLOAD doesn't report the previous routine's.
compiled = [r for r, info in d["routines"].items() if info["labels"] is None]
script = "".join(f'zr\nzl {r}\nw "{r}=",$lts($zu(135,5)),!\n' for r in compiled) + "h\n"
session = subprocess.run(["docker", "exec", "-i", name, "iris", "session", "IRIS", "-U", namespace],
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
# A mask member replaces the base member of its name, or is added when IRIS reports none (the
# `%Dispatch*` hooks of `%DynamicAbstractObject`). It may change only types, modes, defaults and
# the return type: a member of another kind or arity than IRIS reports is a mistake.
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
    return NS.get(kind, kind), name
# A line of an include file the mask replaces by its decided form: what IRIS decides at compile
# time and ascot can't (`#if $$$comClassDefined(...)`). An include of another namespace is skipped.
for doc, edits in mask.get("sources", {}).items():
    include = d["includes"].get(doc.removesuffix(".inc"))
    if include is None:
        continue
    lines = include["text"].split("\n")
    for before, after in edits:
        if before not in lines:
            sys.exit(f"mask.json: {doc} has no line {before!r}")
        lines = [after if line == before else line for line in lines]
    include["text"] = "\n".join(lines)
replaced = added = 0
missing = []
for cls, masked in mask["classes"].items():
    base = d["classes"].get(cls)
    if base is None:  # a class of another namespace
        missing.append(cls)
        continue
    members = {key(m["header"]): m for m in base["members"]}
    for m in masked["members"]:
        kind, mname, args = shape(m["header"])
        target = members.get(key(m["header"]))
        if target is None:
            miscased = [h for k, h in members.items() if k[0] == key(m["header"])[0] and k[1].lower() == mname.lower()]
            if miscased:
                sys.exit(f"mask.json: {cls}.{mname} differs from IRIS in case:\n  {miscased[0]['header']}\n  {m['header']}")
            base["members"].append(m)
            added += 1
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
if missing:
    print(f"mask.json: {len(missing)} class(es) are not in {namespace}, e.g. {missing[0]}", file=sys.stderr)
json.dump(d, open(out, "w"), separators=(",", ":"))
print(f"{named} of {len(compiled)} compiled-only routines got their label names; "
      f"{replaced} member(s) replaced, {added} added and {sum(len(c.get('drop', [])) for c in mask['classes'].values())} dropped by the mask")
PY
