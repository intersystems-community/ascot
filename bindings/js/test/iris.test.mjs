// Checks the wasm binding against a live IRIS instance, given as `ASCOT_TEST_IRIS`
// (`http://user:password@host:port`); the live test is skipped when it's unset.
import assert from "node:assert/strict";
import { test } from "node:test";
import { gzipSync } from "node:zlib";
import ascot from "../dist/index.js";

const { Atelier, createWorkspace } = ascot;

// The two Atelier REST API requests over `fetch`, in the `%SYS` namespace.
class FetchAtelier extends Atelier {
  constructor(url) {
    super();
    const { username, password, origin, pathname } = new URL(url);
    this.base = `${origin}${pathname.replace(/\/$/, "")}/api/atelier/v8/%25SYS`;
    this.headers = {
      Authorization: `Basic ${Buffer.from(`${decodeURIComponent(username)}:${decodeURIComponent(password)}`).toString("base64")}`,
      "Content-Type": "application/json",
    };
  }

  async query(sql, params) {
    const response = await fetch(`${this.base}/action/query`, {
      method: "POST",
      headers: this.headers,
      body: JSON.stringify({ query: sql, parameters: params }),
    });
    if (!response.ok) throw new Error(`query: HTTP ${response.status}`);
    const body = await response.json();
    return JSON.stringify(body.result?.content ?? []);
  }

  async getDoc(name) {
    const response = await fetch(`${this.base}/doc/${encodeURIComponent(name)}`, {
      headers: this.headers,
    });
    if (response.status === 404) return undefined;
    if (!response.ok) throw new Error(`doc: HTTP ${response.status}`);
    const lines = (await response.json()).result?.content ?? [];
    return lines.length ? lines.join("\n") : undefined;
  }
}

class DeadAtelier extends Atelier {
  async query() {
    throw new Error("IRIS is down");
  }
  async getDoc() {
    throw new Error("IRIS is down");
  }
}

const uri = "Demo.Check.cls";
const text = [
  "Class Demo.Check",
  "{",
  "ClassMethod Run()",
  "{",
  " Set o = ##class(%DynamicObject).%New()",
  ' Write o.%Get("a")',
  " Do %SYSAPI^%SYS.API",
  " Do NoSuchLabel^%SYS.API",
  "}",
  "}",
  "",
].join("\n");

async function check(atelier) {
  const workspace = await createWorkspace(atelier);
  await workspace.open(uri, text);
  return (await workspace.diagnostics(uri)).map((d) => [d.severity, d.code, d.message]);
}

test(
  "system classes and routines are checked through a host's Atelier",
  {
    skip: !process.env.ASCOT_TEST_IRIS && "ASCOT_TEST_IRIS is unset",
  },
  async () => {
    assert.deepEqual(await check(new FetchAtelier(process.env.ASCOT_TEST_IRIS)), [
      ["error", "A6851", "Cannot find label 'NoSuchLabel' in routine '%SYS.API'"],
    ]);
  },
);

// Without an env, the bundled pre-built declarations of the newest IRIS release stand in for a
// server; `"none"` is no environment at all, and a class outside the workspace is an error.
test("the bundled declarations are the default; none is the strictest check", async () => {
  const source = [
    "Class Demo.Status",
    "{",
    "ClassMethod M() As %Status",
    "{",
    " Quit $$$OK",
    "}",
    "}",
    "",
  ].join("\n");
  const run = async (...env) => {
    const workspace = await createWorkspace(...env);
    await workspace.open("Demo.Status.cls", source);
    return (await workspace.diagnostics("Demo.Status.cls")).map((d) => [d.severity, d.code]);
  };
  assert.match(ascot.LATEST_RELEASE, /^\d{4}\.\d+$/);
  assert.deepEqual(await run(), []);
  assert.deepEqual(await run("latest"), []);
  assert.deepEqual(await run("none"), [["error", "A2797"]]);
});

// A declaration file's bytes stand in for a server too, gzipped as the asset ships or not.
test("a declaration file's bytes are the environment; none is the strictest check", async () => {
  const declarations = JSON.stringify({
    version: "1.0.0",
    namespace: "%SYS",
    classes: {
      "Demo.Lib": {
        header: "Class Demo.Lib",
        members: [{ header: "ClassMethod Hello() As %String" }],
      },
    },
    includes: {},
    routines: {},
  });
  const source = [
    "Class Demo.Use",
    "{",
    "ClassMethod M() As %String",
    "{",
    " Quit ##class(Demo.Lib).Hello()",
    "}",
    "}",
    "",
  ].join("\n");
  const run = async (env) => {
    const workspace = await createWorkspace(env);
    await workspace.open("Demo.Use.cls", source);
    return (await workspace.diagnostics("Demo.Use.cls")).map((d) => [d.severity, d.code]);
  };
  assert.deepEqual(await run(new Uint8Array(gzipSync(declarations))), []);
  assert.deepEqual(await run(new TextEncoder().encode(declarations)), []);
  assert.deepEqual(await run("none"), [["error", "A8061"]]);
});

test("a request that fails leaves what it was for taken on trust, saying why", async () => {
  const hint = (what) => [
    "hint",
    "A4471",
    `Cannot look up ${what} on the IRIS instance (IRIS is down), so it is taken on trust`,
  ];
  // Once per call into the routine: a failed lookup isn't cached.
  assert.deepEqual((await check(new DeadAtelier())).sort(), [
    hint("class '%Library.DynamicObject'"),
    hint("routine '%SYS.API'"),
    hint("routine '%SYS.API'"),
  ]);
});
