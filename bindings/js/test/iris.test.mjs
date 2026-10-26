// Checks the wasm binding against a live IRIS instance, given as `ASCOT_TEST_IRIS`
// (`http://user:password@host:port`); the live test is skipped when it's unset.
import assert from "node:assert/strict";
import { test } from "node:test";
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

test("a request that fails leaves what it was for taken on trust, saying why", async () => {
  const hint = (what) => [
    "hint",
    "A4471",
    `Cannot look up ${what} on the IRIS instance (IRIS is down), so it is taken on trust`,
  ];
  assert.deepEqual((await check(new DeadAtelier())).sort(), [
    hint("class '%Library.DynamicObject'"),
    hint("routine '%SYS.API'"),
  ]);
});
