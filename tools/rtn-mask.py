#!/usr/bin/env python3
"""Regenerates the routine entries of src/mask.json: the signatures of the %SYS routine labels that system include
macros call, for instances that ship those routines without source. Only label headers are kept,
never code or default values. Hand-written `returns` survive regeneration.

Usage: tools/rtn-mask.py http://user:pass@localhost:8080/iris
"""

import base64, json, pathlib, re, sys, urllib.parse, urllib.request

url = urllib.parse.urlsplit(sys.argv[1])
base = f"{url.scheme}://{url.hostname}:{url.port}{url.path}/api/atelier/v8/%25SYS/"
auth = "Basic " + base64.b64encode(f"{url.username}:{url.password}".encode()).decode()


def request(path, body=None):
    req = urllib.request.Request(base + path, json.dumps(body).encode() if body else None)
    req.add_header("Authorization", auth)
    req.add_header("Content-Type", "application/json")
    try:
        return json.load(urllib.request.urlopen(req))["result"]["content"]
    except urllib.error.HTTPError:
        return None


def doc(name):
    return request("doc/" + urllib.parse.quote(name)) or []


def names(spec):
    query = "SELECT Name FROM %Library.RoutineMgr_StudioOpenDialog(?,1,1,1,1,1,0)"
    return [r["Name"] for r in request("action/query", {"query": query, "parameters": [spec]})]


def split_top(s, sep):
    parts, depth, quoted, start = [], 0, False, 0
    for i, c in enumerate(s):
        if c == '"':
            quoted = not quoted
        elif not quoted and c in "([":
            depth += 1
        elif not quoted and c in ")]":
            depth -= 1
        elif not quoted and depth == 0 and c == sep:
            parts.append(s[start:i])
            start = i + 1
    return parts + [s[start:]]


def closing(s, i):
    depth, quoted = 0, False
    for j in range(i, len(s)):
        if s[j] == '"':
            quoted = not quoted
        elif not quoted and s[j] in "([":
            depth += 1
        elif not quoted and s[j] in ")]":
            depth -= 1
            if depth == 0:
                return j
    return None


def header(line):
    if m := re.match(r"(%?\w+)(?=[ \t;]|$)", line):
        return m[1], {"args": []}
    m = re.match(r"(%?\w+)\(", line)
    if not m or (end := closing(line, m.end() - 1)) is None:
        return None
    args = []
    for param in filter(None, (p.strip() for p in split_top(line[m.end() : end], ","))):
        name, _, default = param.partition("=")
        args.append(name.strip() + ("=" if default or param.endswith("=") else ""))
    sig = {"args": args}
    rest = line[end + 1 :].lstrip()
    if rest.startswith("[") and (close := closing(rest, 0)) is not None:
        publics, rest = rest[1:close], rest[close + 1 :].lstrip()
    else:
        publics = None
    procedure = re.match(r"((public|private|methodimpl)\b\s*)*\{", rest, re.I)
    if procedure:
        sig["publics"] = [p.strip() for p in split_top(publics, ",") if p.strip()] if publics else []
    return m[1], sig


refs = {}
for inc in names("*.inc"):
    macro = False
    for line in doc(inc):
        macro = macro or re.match(r"\s*#(define|def1arg)\b", line, re.I)
        if macro:
            for label, rtn in re.findall(r"(%?\w+)\^(%[\w.]+)", line):
                refs.setdefault(rtn, set()).add(label)
        macro = macro and line.rstrip().lower().endswith("##continue")

def fmt(v):
    if isinstance(v, dict):
        return "{ " + ", ".join(f"{json.dumps(k)}: {fmt(x)}" for k, x in v.items()) + " }"
    return json.dumps(v)


path = pathlib.Path(__file__).parent.parent / "src/mask.json"
mask = json.loads(path.read_text())
old = {k: mask.pop(k) for k in list(mask) if not k.endswith(".cls")}
for rtn in sorted(refs):
    name = next((rtn + ext for ext in (".int", ".mac") if doc(rtn + ext)), None)
    labels = dict(filter(None, map(header, doc(name) if name else [])))
    for label in sorted(refs[rtn] & labels.keys()):
        sig = labels[label]
        if returns := old.get(name, {}).get(label, {}).get("returns"):
            sig["returns"] = returns
        mask.setdefault(name, {})[label] = sig
path.write_text(
    "{\n"
    + ",\n".join(
        f" {json.dumps(key)}: {{\n" + ",\n".join(f"  {json.dumps(k)}: {fmt(v)}" for k, v in members.items()) + "\n }"
        for key, members in mask.items()
    )
    + "\n}\n"
)
routines = [v for k, v in mask.items() if not k.endswith(".cls")]
print(len(routines), "routines,", sum(map(len, routines)), "labels")
