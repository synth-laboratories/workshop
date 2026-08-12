/**
 * Wave 3 will wire this into a sessionStore reducer.
 * Wave 1 only lands the pure module so Rust Session authority can merge without
 * rewriting App.tsx.
 *
 * Contract: one writer for session status on the renderer side, mirroring
 * `SessionService::transition` on the host (`state_machines_have_explicit_transitions`).
 */

export type RuntimeEventLike = {
	kind?: string;
	sessionId?: string;
	[key: string]: unknown;
};

export type SessionStatusMirror =
	| "created"
	| "ready"
	| "running"
	| "interrupted"
	| "failed"
	| "closed";

export type SessionSlice = {
	id: string;
	status: SessionStatusMirror;
};

/**
 * Pure apply function. Today this is a no-op identity stub so Wave 3 can
 * replace the body without changing call sites.
 */
export function applyRuntimeEvent<T extends { sessions?: SessionSlice[] }>(
	state: T,
	_event: RuntimeEventLike,
): T {
	return state;
}
