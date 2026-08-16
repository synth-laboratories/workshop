import type { ArtifactRef } from "../types/landing";

export type VisualRevisionState = {
	id: string | null;
	artifact: ArtifactRef | null;
	acceptedRevision: number;
	requestedRevision: number;
	generation: number;
	loading: boolean;
	error: string | null;
};

export const EMPTY_VISUAL_REVISION_STATE: VisualRevisionState = {
	id: null,
	artifact: null,
	acceptedRevision: -1,
	requestedRevision: -1,
	generation: 0,
	loading: false,
	error: null
};

function revision(artifact: ArtifactRef | null | undefined): number {
	return typeof artifact?.revision === "number" ? artifact.revision : -1;
}

/** Select the newest materialized record for one visual. Equal revisions favor
 * the authoritative reconciler record, which is passed first by callers. */
export function newestVisualArtifact(
	id: string | null,
	...candidates: Array<ArtifactRef | null | undefined>
): ArtifactRef | null {
	if (!id) return null;
	return candidates
		.filter((candidate): candidate is ArtifactRef => candidate?.id === id)
		.reduce<ArtifactRef | null>((best, candidate) =>
			!best || revision(candidate) > revision(best) ? candidate : best, null);
}

function stableJson(value: unknown): string {
	if (Array.isArray(value)) return `[${value.map(stableJson).join(",")}]`;
	if (value && typeof value === "object") {
		return `{${Object.entries(value as Record<string, unknown>)
			.sort(([left], [right]) => left.localeCompare(right))
			.map(([key, child]) => `${JSON.stringify(key)}:${stableJson(child)}`)
			.join(",")}}`;
	}
	return JSON.stringify(value) ?? "undefined";
}

/** Small deterministic identity for transport authority. Metadata and revision
 * are intentionally excluded so cosmetic updates preserve viewer interaction. */
export function bindingAuthorityKey(bindings: ArtifactRef["bindings"]): string {
	const text = stableJson(bindings ?? null);
	let hash = 0x811c9dc5;
	for (let index = 0; index < text.length; index += 1) {
		hash ^= text.charCodeAt(index);
		hash = Math.imul(hash, 0x01000193);
	}
	return (hash >>> 0).toString(16).padStart(8, "0");
}

export type VisualRevisionAction =
	| { type: "select"; id: string; artifact?: ArtifactRef | null }
	| { type: "close" }
	| { type: "request"; id: string; minimumRevision?: number; generation: number }
	| { type: "resolve"; id: string; artifact: ArtifactRef; generation: number }
	| { type: "fail"; id: string; generation: number; error: string }
	| { type: "accept"; id: string; artifact: ArtifactRef };

export function visualRevisionReducer(
	state: VisualRevisionState,
	action: VisualRevisionAction
): VisualRevisionState {
	switch (action.type) {
		case "close":
			return { ...EMPTY_VISUAL_REVISION_STATE, generation: state.generation };
		case "select": {
			if (state.id === action.id) {
				if (!action.artifact || revision(action.artifact) < state.acceptedRevision) return state;
				return {
					...state,
					artifact: action.artifact,
					acceptedRevision: revision(action.artifact),
					error: null
				};
			}
			return {
				...EMPTY_VISUAL_REVISION_STATE,
				id: action.id,
				artifact: action.artifact ?? null,
				acceptedRevision: revision(action.artifact),
				generation: state.generation
			};
		}
		case "request":
			if (state.id !== action.id || action.generation < state.generation) return state;
			return {
				...state,
				generation: action.generation,
				requestedRevision: Math.max(state.requestedRevision, action.minimumRevision ?? -1),
				loading: true,
				error: null
			};
		case "accept":
			if (state.id !== action.id || revision(action.artifact) < state.acceptedRevision) return state;
			return {
				...state,
				artifact: action.artifact,
				acceptedRevision: revision(action.artifact),
				error: null
			};
		case "resolve":
			if (
				state.id !== action.id ||
				action.generation !== state.generation ||
				revision(action.artifact) < state.acceptedRevision
			) return state;
			return {
				...state,
				artifact: action.artifact,
				acceptedRevision: revision(action.artifact),
				requestedRevision: Math.max(state.requestedRevision, revision(action.artifact)),
				loading: false,
				error: null
			};
		case "fail":
			if (state.id !== action.id || action.generation !== state.generation) return state;
			return { ...state, loading: false, error: action.error };
	}
}
