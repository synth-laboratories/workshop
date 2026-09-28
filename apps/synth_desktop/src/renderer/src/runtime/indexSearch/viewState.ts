/**
 * UI state for the Index search surface, kept as a pure reducer so the
 * error-to-state mapping and the 429 countdown are testable without React.
 * The state is serializable and never contains a search token.
 */
import type { IndexSearchEnvelope, IndexSearchError, IndexSearchMode } from "./types.ts";

export type IndexSearchViewState =
	| { phase: "idle" }
	| { phase: "searching"; mode: IndexSearchMode; searchId: string | null; pollAttempt: number }
	| { phase: "complete"; envelope: IndexSearchEnvelope }
	| { phase: "rate_limited"; scope: string | null; retryAtMs: number | null; detail: string }
	| { phase: "unavailable"; code: string; detail: string }
	| { phase: "disabled"; detail: string }
	| { phase: "too_large"; detail: string }
	| { phase: "cancelled" }
	| { phase: "failed"; code: string; detail: string };

/** 503s fail closed: the surface shows why and offers no retry loop. */
const UNAVAILABLE_CODES: ReadonlySet<string> = new Set(["index_public_budget_exhausted", "index_rate_store_unavailable", "monitor_unavailable"]);

export function viewStateFromError(error: IndexSearchError, nowMs: number): IndexSearchViewState {
	switch (error.code) {
		case "index_public_rate_limited":
			return {
				phase: "rate_limited",
				scope: error.scope,
				retryAtMs: error.retryAfterS === null ? null : nowMs + Math.max(0, error.retryAfterS) * 1000,
				detail: error.detail
			};
		case "index_public_search_disabled":
			return { phase: "disabled", detail: error.detail };
		case "index_request_too_large":
			return { phase: "too_large", detail: error.detail };
		case "index_search_cancelled":
			return { phase: "cancelled" };
		default:
			if (UNAVAILABLE_CODES.has(error.code)) return { phase: "unavailable", code: error.code, detail: error.detail };
			return { phase: "failed", code: error.code, detail: error.detail };
	}
}

/** Whole seconds until a rate limit lifts; 0 once it has. `null` when the backend gave no Retry-After. */
export function rateLimitSecondsRemaining(state: IndexSearchViewState, nowMs: number): number | null {
	if (state.phase !== "rate_limited") return null;
	if (state.retryAtMs === null) return null;
	return Math.max(0, Math.ceil((state.retryAtMs - nowMs) / 1000));
}

/** Whether the operator may submit a new search from this state. */
export function canSubmit(state: IndexSearchViewState, nowMs: number): boolean {
	switch (state.phase) {
		case "searching":
		case "disabled":
		case "unavailable":
			return false;
		case "rate_limited":
			return rateLimitSecondsRemaining(state, nowMs) === 0;
		default:
			return true;
	}
}
