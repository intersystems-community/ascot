import * as fs from "node:fs";
import * as path from "node:path";
// `generated/ascot.js` is jco's own ESM output (real `export function instantiate`), so
// this CJS wrapper can't statically `import` it -- load it lazily instead. Doing so also
// keeps this file's own module-relative asset lookup (below) on native CJS `__dirname`,
// which survives being bundled into a consumer's build; ESM's `import.meta.url` doesn't --
// bundlers commonly substitute it with a literal path resolved at *build* time, which
// breaks the moment the built package runs on a different machine.
import type { Root } from "../generated/ascot.js" with { "resolution-mode": "import" };
import type {
	ClassInfo,
	RoutineInfo,
	Source,
	Style,
} from "../generated/interfaces/iris-ascot-common.js" with {
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

export class IrisContext {
	/** A class's own members and metadata. */
	async getCls(_cls: string): Promise<ClassInfo | undefined> {
		return undefined;
	}
	/** The text of `name.inc`. */
	async getInc(_name: string): Promise<Source | undefined> {
		return undefined;
	}
	/** The labels of routine `name`, `undefined` if there is no such routine. */
	async getRtn(_name: string): Promise<RoutineInfo | undefined> {
		return { labels: undefined, uri: undefined };
	}
}

/** The two Atelier REST API requests, for ascot to make `IrisContext`'s lookups with. A failed
 * request throws; ascot leaves what it was for unconfirmed. */
export class Atelier {
	/** The rows of `/action/query` as a JSON array, each an object keyed by column name. */
	async query(_sql: string, _params: string[]): Promise<string> {
		throw new Error("no IRIS connection");
	}
	/** The text of `/doc/{name}`, `undefined` if there is no such document. */
	async getDoc(_name: string): Promise<string | undefined> {
		throw new Error("no IRIS connection");
	}
}

const wasm: Promise<Root> = import("../generated/ascot.js").then(({ instantiate }) =>
	instantiate(
		(name: string) =>
			new WebAssembly.Module(fs.readFileSync(path.resolve(__dirname, "../lib", name))),
		{
			"iris:ascot/common": {},
			"iris:ascot/imported": { IrisContext, Atelier },
		} as never,
	),
);
wasm.catch(() => {}); // reported by createWorkspace instead

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

/** A workspace whose calls run one at a time, resolving what it doesn't contain through `iris`. */
export async function createWorkspace(
	iris?: IrisContext | Atelier,
	style?: Style,
): Promise<Workspace> {
	const root = await wasm;
	const env =
		iris instanceof Atelier
			? { tag: "atelier" as const, val: new Failing(iris) }
			: iris && { tag: "context" as const, val: iris };
	return serialize(new root.exported.Workspace(env, style));
}

// jco rethrows a thrown `Error` to the caller, and hands ascot only a thrown `{ payload }` as a
// request's failure; so what `atelier` throws becomes one.
class Failing extends Atelier {
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
