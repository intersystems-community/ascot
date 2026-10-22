import * as fs from "node:fs";
import * as path from "node:path";
// `generated/ascot.js` is jco's own ESM output (real `export function instantiate`), so
// this CJS wrapper can't statically `import` it -- load it lazily instead. Doing so also
// keeps this file's own module-relative asset lookup (below) on native CJS `__dirname`,
// which survives being bundled into a consumer's build; ESM's `import.meta.url` doesn't --
// bundlers commonly substitute it with a literal path resolved at *build* time, which
// breaks the moment the built package runs on a different machine.
import type { Root } from "../generated/ascot.js" with { "resolution-mode": "import" };
import type { ClassInfo, RoutineSource, Style } from "../generated/interfaces/iris-ascot-common.js" with {
	"resolution-mode": "import",
};

export type * from "../generated/interfaces/iris-ascot-common.js";

type RawWorkspace = InstanceType<Root["exported"]["Workspace"]>;

/** Every method of the wasm `Workspace`, as `createWorkspace` hands it out: all async, since
 * `serialize` below queues each call behind the previous one. */
export type Workspace = {
	[K in keyof RawWorkspace]: RawWorkspace[K] extends (...args: infer A) => infer R
		? (...args: A) => Promise<Awaited<R>>
		: RawWorkspace[K];
};

/** The shape your `imported.iris-connection` implementation must satisfy. */
export interface Imported {
	getCls(cls: string): Promise<ClassInfo | undefined>;
	getInc(name: string): Promise<RoutineSource | undefined>;
	// `{ tag: "none" }` if there is no such routine; `val: undefined` if its source isn't available.
	getIntOrMac(name: string): Promise<{ tag: "none" } | { tag: "some"; val: RoutineSource | undefined }>;
}

// jco needs the actual `IrisConnection` class (not just an instance) up front, to bind
// resource-identity checks (`instanceof`) at instantiation time -- so the wasm module is
// loaded lazily on the first `createWorkspace` call, using that call's own argument's
// class. Every later call must be constructed from that same class; in practice callers
// only ever have one `Imported` implementation, so this holds.
let wasm: Promise<Root> | undefined;

async function loadWasm(IrisConnection: abstract new (...args: never[]) => Imported): Promise<Root> {
	return (wasm ??= import("../generated/ascot.js").then(({ instantiate }) =>
		instantiate(
			(name: string) => new WebAssembly.Module(fs.readFileSync(path.resolve(__dirname, "../lib", name))),
			{
				"iris:ascot/common": {},
				"iris:ascot/imported": { IrisConnection },
			} as never,
		),
	));
}

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

/**
 * A `Workspace` backed by `imported`, with every method pre-serialized so overlapping
 * calls (e.g. from two documents' diagnostics resolving concurrently) can't trip ascot's
 * internal `RefCell` borrows or jco's single-in-flight-call constraint.
 */
export async function createWorkspace(imported: Imported, style?: Style): Promise<Workspace> {
	const IrisConnection = Object.getPrototypeOf(imported).constructor;
	const root = await loadWasm(IrisConnection);
	return serialize(new root.exported.Workspace(imported, style));
}
