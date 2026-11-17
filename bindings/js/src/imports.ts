// What the wasm imports from its host: jco's glue takes these two classes from here (see
// `--map` in scripts/build.sh), and `createWorkspace` wraps a caller's subclass in them.
import type { ClassInfo, RoutineInfo, Source } from "../generated/interfaces/iris-ascot-common.js";

/** What ascot asks about code outside the workspace. A failed lookup throws; ascot then takes
 * what it looked for on trust and says why. */
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
 * request throws, like a failed lookup. */
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
