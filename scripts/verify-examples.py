#!/usr/bin/env python3
"""Load every example in the diagnostics' doc comments into IRIS and check what each message claims.

Usage: scripts/verify-examples.py --iris http://user:pass@host:port[/prefix] [--namespace USER]
                                  [--run "iris session IRIS -U USER"] [--only A1234,A5678]

An example is a ```objectscript block in a kind's doc comment in src/check/errors.rs: a class or
routine whose offending lines end in `//! A1234` (`tests/examples.rs` checks those). Here, an
error's example must be rejected by the compiler, unless its block says `raises=<ERROR> statement`:
then it must compile, and the statement, fed to `--run` (an `iris session`, through `docker exec -i`
when IRIS runs in a container), must raise that error. A warning's or hint's example, and a block
marked `compiles`, must compile; one marked `rejected` says IRIS refuses it although ascot only
warns or hints. Exits 1 on any disagreement.
"""
import argparse, base64, json, os, re, shlex, subprocess, sys, urllib.parse, urllib.request

ap = argparse.ArgumentParser()
ap.add_argument('--iris', required=True)
ap.add_argument('--namespace', default='USER')
ap.add_argument('--run', default='iris session IRIS -U USER')
ap.add_argument('--only', default=None, help='comma-separated codes; the rest are skipped')
args = ap.parse_args()
ROOT = os.path.realpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))

u = urllib.parse.urlsplit(args.iris)
AUTH = 'Basic ' + base64.b64encode(f'{urllib.parse.unquote(u.username)}:{urllib.parse.unquote(u.password)}'.encode()).decode()
BASE = f'{u.scheme}://{u.hostname}:{u.port}{u.path.rstrip("/")}/api/atelier/v1/{args.namespace}'

def call(method, path, body=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(BASE + path, data=data, method=method, headers={'Authorization': AUTH, 'Content-Type': 'application/json'})
    try:
        return json.load(urllib.request.urlopen(req, timeout=300))
    except urllib.error.HTTPError as e:
        return json.load(e)

# A refused PUT (a class with two members of one name) reports only in `result.status`.
def errors(resp):
    status = resp.get('result', {}).get('status', '') if isinstance(resp.get('result'), dict) else ''
    return [e['error'] for e in resp.get('status', {}).get('errors', [])] + \
        [l.strip() for l in resp.get('console', []) if l.startswith('ERROR') and not l.strip().endswith('.cls')] + \
        ([status.strip()] if status.startswith('ERROR') else [])

# The kinds: (code, severity, doc lines) from `type_error_kinds!`.
def kinds():
    docs, code, severity = [], None, None
    for line in open(os.path.join(ROOT, 'src/check/errors.rs'), encoding='utf-8'):
        s = line.strip()
        if s.startswith('///'):
            docs.append(s[3:].removeprefix(' ')); continue
        if m := re.match(r'#\[code = "([AS]\d+)"\]', s): code = m.group(1); continue
        if m := re.match(r'#\[severity = (\w+)\]', s): severity = m.group(1); continue
        if code and re.match(r'[A-Z]\w+', s):
            yield code, severity, docs
            docs, code, severity = [], None, None
        elif not s.startswith('#['):
            docs = [] if not code else docs

MARKER = re.compile(r'\s*//! (?:[AS]\d+ ?)+$')

# One kind's example: its documents (name, lines) and how IRIS should take them.
def example(docs):
    documents, info = [], []
    i = 0
    while i < len(docs):
        line = docs[i]; i += 1
        if not line.strip().startswith('```objectscript'): continue
        info.append(line.strip()[len('```objectscript'):].strip())
        lines = []
        while i < len(docs) and docs[i].strip() != '```':
            lines.append(MARKER.sub('', docs[i])); i += 1
        i += 1
        header = next(l for l in lines if l.strip()).split()
        ext = {'Class': 'cls', 'ROUTINE': 'mac'}[header[0]]
        if header[0] == 'ROUTINE' and '[Type=INC]' in lines[0]: ext = 'inc'
        if header[0] == 'ROUTINE' and '[Type=INT]' in lines[0]: ext = 'int'
        documents.append((f'{header[1]}.{ext}', lines))
    return documents, info

def expectation(severity, info):
    for inf in info:
        if m := re.match(r'raises=(<[A-Z ]+>)\s+(.+)$', inf): return 'raises', m.group(1), m.group(2)
    if any('rejected' in inf.split() for inf in info): return 'rejected', None, None
    if any('compiles' in inf.split() for inf in info) or severity != 'Error': return 'compiles', None, None
    return 'rejected', None, None

def run(statement):
    script = f' try {{ {statement} write "ASCOT-RAN",! }} catch e {{ write "ASCOT-RAISED ",e.Name,! }}\n halt\n'
    r = subprocess.run(shlex.split(args.run), input=script, capture_output=True, text=True, timeout=300)
    out = r.stdout + r.stderr
    if m := re.search(r'ASCOT-RAISED (<[^>]+>)', out): return m.group(1)
    if 'ASCOT-RAN' in out: return None
    return f'? ({out.strip()[-200:]})'

bad = total = 0
only = set(args.only.split(',')) if args.only else None
for code, severity, docs in kinds():
    if only and code not in only: continue
    documents, info = example(docs)
    if not documents: continue
    total += 1
    want, error, statement = expectation(severity, info)
    names = [n for n, _ in documents]
    try:
        errs = []
        for name, lines in documents:
            errs += errors(call('PUT', f'/doc/{name}?ignoreConflict=1', {'enc': False, 'content': lines}))
        if not errs: errs += errors(call('POST', '/action/compile?flags=cuk', names))
        raised = run(statement) if want == 'raises' and not errs else None
    finally:
        for name in names: call('DELETE', f'/doc/{name}')
    got = 'rejected: ' + ' | '.join(e.replace('&#39;', "'")[:160] for e in errs) if errs else 'compiles' + (f', raises {raised}' if raised else ', runs' if want == 'raises' else '')
    ok = (want == 'rejected') == bool(errs) and (want != 'raises' or raised == error)
    expected = {'rejected': 'rejected', 'compiles': 'compiles', 'raises': f'compiles, raises {error}'}[want]
    print(f'{"ok  " if ok else "BAD "} {code} {severity:7} expected {expected}; IRIS: {got}')
    bad += not ok
print(f'{total} examples, {bad} disagree with IRIS')
sys.exit(1 if bad else 0)
