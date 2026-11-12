import * as fs from "node:fs";
import type {
	ClassInfo,
	RoutineInfo,
	Source,
	Style,
} from "../generated/interfaces/iris-ascot-common.js";
// Written by scripts/build.sh: the bundled declarations, as a `new URL("./file", import.meta.url)`,
// the one form of asset reference a bundler resolves at build time and carries into its output.
import { file as declarations, release } from "../lib/declarations.js";
import { Atelier, IrisContext } from "./imports.js";

export type * from "../generated/interfaces/iris-ascot-common.js";
export { Atelier, IrisContext };

type Root = typeof import("../generated/ascot.js");
type RawWorkspace = InstanceType<Root["exported"]["Workspace"]>;

/** Every method of the wasm `Workspace`, as `createWorkspace` hands it out: all async, since
 * `serialize` below queues each call behind the previous one. */
export type Workspace = {
	[K in Exclude<keyof RawWorkspace, "setEnv">]: RawWorkspace[K] extends (
		...args: infer A
	) => infer R
		? (...args: A) => Promise<Awaited<R>>
		: RawWorkspace[K];
} & {
	/** Replaces the server; every uri comes back stale. Takes what `createWorkspace` takes. */
	setEnv(iris?: Env): Promise<string[]>;
};

/** Where ascot resolves what the workspace doesn't contain: `"latest"` (the default), the
 * pre-built declarations of the newest IRIS release this package knows, bundled with it; the
 * host's own lookups; an Atelier connection; a declaration file's bytes, gzipped or not; or
 * `"none"`, no environment at all, so that everything outside the workspace is an error. */
export type Env = "latest" | IrisContext | Atelier | Uint8Array | "none";

let bundled: Uint8Array | undefined;
function bundledDeclarations(): Uint8Array {
	bundled ??= fs.readFileSync(declarations);
	return bundled;
}

/** The IRIS release whose declarations `"latest"` stands for, e.g. `"2026.2"`. */
export const LATEST_RELEASE: string = release;

// jco's glue instantiates the wasm as it loads, from its own `new URL("./ascot.core.wasm",
// import.meta.url)`, with the classes above as the host; loaded lazily so that a failure is
// `createWorkspace`'s to report, not this module's.
const wasm: Promise<Root> = import("../generated/ascot.js");
wasm.catch(() => {});

// jco's jspi glue allows only one in-flight suspended call per component instance --
// overlapping calls (e.g. diagnostics() and hover() both awaiting a REST round trip)
// trip its exclusive-lock check. Every `Workspace` this package creates shares the same
// wasm instance, so this queue is module-wide, not per-Workspace.
let tail: Promise<void> = Promise.resolve();

function serialize(instance: RawWorkspace): Workspace {
	return new Proxy(instance, {
		get(target, prop, receiver) {
			const value = Reflect.get(target, prop, receiver);
			if (typeof value !== "function") return value;
			return async (...args: unknown[]) => {
				if (prop === "setEnv") args = [envOf((args[0] as Env | undefined) ?? "latest")];
				const prev = tail;
				let release!: () => void;
				tail = new Promise<void>((resolve) => (release = resolve));
				await prev;
				try {
					return await value.apply(target, args);
				} finally {
					release();
				}
			};
		},
	}) as unknown as Workspace;
}

/** A workspace whose calls run one at a time, resolving what it doesn't contain through `iris`. */
export async function createWorkspace(
	iris: Env = "latest",
	style?: Style,
	diagnostics?: string[],
): Promise<Workspace> {
	const root = await wasm;
	const workspace = serialize(new root.exported.Workspace(envOf(iris), style));
	if (diagnostics) await workspace.setDiagnostics(diagnostics);
	return workspace;
}

function envOf(iris: Env) {
	if (iris === "none") return undefined;
	if (iris === "latest") return { tag: "declarations" as const, val: bundledDeclarations() };
	if (iris instanceof Uint8Array) return { tag: "declarations" as const, val: iris };
	return iris instanceof Atelier
		? { tag: "atelier" as const, val: new FailingAtelier(iris) }
		: iris && { tag: "context" as const, val: new FailingContext(iris) };
}

// jco rethrows a thrown `Error` to the caller, and hands ascot only a thrown `{ payload }` as the
// failure; so what the host throws becomes one.
class FailingContext extends IrisContext {
	constructor(private readonly inner: IrisContext) {
		super();
	}
	override getCls(cls: string): Promise<ClassInfo | undefined> {
		return payload(() => this.inner.getCls(cls));
	}
	override getInc(name: string): Promise<Source | undefined> {
		return payload(() => this.inner.getInc(name));
	}
	override getRtn(name: string): Promise<RoutineInfo | undefined> {
		return payload(() => this.inner.getRtn(name));
	}
}

class FailingAtelier extends Atelier {
	constructor(private readonly inner: Atelier) {
		super();
	}
	override query(sql: string, params: string[]): Promise<string> {
		return payload(() => this.inner.query(sql, params));
	}
	override getDoc(name: string): Promise<string | undefined> {
		return payload(() => this.inner.getDoc(name));
	}
}

async function payload<T>(request: () => Promise<T>): Promise<T> {
	try {
		return await request();
	} catch (e) {
		throw { payload: e instanceof Error ? e.message : String(e) };
	}
}
