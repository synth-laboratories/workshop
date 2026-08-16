/**
 * Composer steering state machine and error normalization.
 *
 * Steering is a turn-controller transition, not a property of whichever row
 * happens to hold DOM focus: the first Return queues a prompt during an active
 * turn, and the second Return promotes that prompt to an immediate steer from
 * wherever the keyboard is. Keeping the transition here — rather than on the
 * queued-prompt input — is what makes the gesture keyboard-accessible and
 * independent of clicking the queue.
 *
 * Errors are normalized before they can reach the composer. A raw runtime
 * rejection carries an internal session UUID, a transport body, or a bare
 * object; none of those belong in the core composer, and interpolating one
 * into JSX is how `[object Object]` reaches a user.
 */

/**
 * How long a queued prompt stays promotable by a second Return.
 *
 * This is a human gesture: the first Return has to render "Next turns" before
 * the person can decide to promote it. A stopwatch tuned to a synthetic double
 * press expires before a real user reacts, which is what forced them to click
 * the queued row and press Return twice more. The arm is instead ended by
 * something meaningful — the turn finishing, the prompt leaving the queue, or
 * new text being typed — and this window only bounds a forgotten arm.
 */
export const STEER_PROMOTION_WINDOW_MS = 30_000;

export type SteerErrorCode =
	| "steer_turn_finished"
	| "steer_unsupported"
	| "steer_empty"
	| "steer_unauthorized"
	| "steer_rejected"
	| "steer_unavailable";

export type SteerFailure = {
	/** Stable public code. Safe to show and to match on in tests. */
	code: SteerErrorCode;
	/** User-facing sentence: what happened and what to do next. */
	message: string;
	/** Full structured original, for Advanced diagnostics and logs only. */
	detail: string;
};

export type SteerState =
	| { phase: "idle" }
	| { phase: "armed"; promptId: string; text: string; armedAt: number }
	| { phase: "promoting"; promptId: string; text: string }
	| { phase: "failed"; promptId: string; failure: SteerFailure };

export type SteerEvent =
	| { type: "queued"; promptId: string; text: string; at: number }
	| {
			type: "return";
			composerText: string;
			at: number;
			repeat?: boolean;
			composing?: boolean;
	  }
	| { type: "acknowledged"; promptId: string }
	| { type: "rejected"; promptId: string; failure: SteerFailure }
	| { type: "queueReconciled"; promptIds: string[] }
	| { type: "turnEnded" }
	| { type: "disarm" };

export type SteerEffect = { kind: "promote"; promptId: string; text: string } | null;

export const IDLE_STEER_STATE: SteerState = { phase: "idle" };

export function armedPromptId(state: SteerState): string | null {
	return state.phase === "armed" ? state.promptId : null;
}

export function promotingPromptId(state: SteerState): string | null {
	return state.phase === "promoting" ? state.promptId : null;
}

export function steerFailure(state: SteerState): SteerFailure | null {
	return state.phase === "failed" ? state.failure : null;
}

/**
 * Advance the machine. The caller performs `effect` — never the reducer — so
 * one Return can produce at most one promotion request.
 */
export function reduceSteer(
	state: SteerState,
	event: SteerEvent
): { state: SteerState; effect: SteerEffect } {
	switch (event.type) {
		case "queued": {
			if (!event.text.trim()) return { state, effect: null };
			return {
				state: { phase: "armed", promptId: event.promptId, text: event.text, armedAt: event.at },
				effect: null
			};
		}
		case "return": {
			// Held Return repeats, and an IME commit press is a composition
			// boundary, not an instruction. Neither may deliver a steer.
			if (event.repeat || event.composing) return { state, effect: null };
			// A promotion is already in flight; a second press is the same
			// intent, not a second steer.
			if (state.phase === "promoting") return { state, effect: null };
			if (state.phase !== "armed") return { state, effect: null };
			if (event.at - state.armedAt > STEER_PROMOTION_WINDOW_MS) {
				return { state: IDLE_STEER_STATE, effect: null };
			}
			// React may not have committed the cleared composer before a fast
			// physical double Return. Accept the pre-commit value only when it
			// is exactly the prompt that was armed; anything newly typed is a
			// distinct prompt and must enqueue instead of hijacking the arm.
			const typed = event.composerText.trim();
			if (typed && typed !== state.text.trim()) return { state, effect: null };
			return {
				state: { phase: "promoting", promptId: state.promptId, text: state.text },
				effect: { kind: "promote", promptId: state.promptId, text: state.text }
			};
		}
		case "acknowledged": {
			// Only the backend's acknowledgement retires a prompt, and only the
			// prompt it acknowledged.
			if (state.phase === "promoting" && state.promptId !== event.promptId) {
				return { state, effect: null };
			}
			return { state: IDLE_STEER_STATE, effect: null };
		}
		case "rejected": {
			return {
				state: { phase: "failed", promptId: event.promptId, failure: event.failure },
				effect: null
			};
		}
		case "queueReconciled": {
			// A reconnect can replace the persisted queue. An armed prompt that
			// no longer exists cannot be promoted; a promotion already in flight
			// keeps waiting for its acknowledgement.
			if (state.phase === "armed" && !event.promptIds.includes(state.promptId)) {
				return { state: IDLE_STEER_STATE, effect: null };
			}
			if (state.phase === "failed" && !event.promptIds.includes(state.promptId)) {
				return { state: IDLE_STEER_STATE, effect: null };
			}
			return { state, effect: null };
		}
		case "turnEnded": {
			// There is nothing to steer; the prompt stays queued for the normal
			// next-turn path rather than being lost or double-delivered.
			if (state.phase === "promoting") return { state, effect: null };
			return { state: IDLE_STEER_STATE, effect: null };
		}
		case "disarm": {
			if (state.phase === "promoting") return { state, effect: null };
			return { state: IDLE_STEER_STATE, effect: null };
		}
	}
}

const UUID = /\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/gi;
const LONG_HEX = /\b[0-9a-f]{16,}\b/gi;

/** Internal identifiers are diagnostics, never user-facing copy. */
export function redactIdentifiers(text: string): string {
	return text.replace(UUID, "…").replace(LONG_HEX, "…");
}

function describe(reason: unknown): string {
	if (reason instanceof Error) return reason.stack ?? `${reason.name}: ${reason.message}`;
	if (typeof reason === "string") return reason;
	try {
		return JSON.stringify(reason) ?? String(reason);
	} catch {
		return Object.prototype.toString.call(reason);
	}
}

function rawMessage(reason: unknown): string {
	if (reason instanceof Error) return reason.message;
	if (typeof reason === "string") return reason;
	if (reason && typeof reason === "object") {
		const value = reason as { message?: unknown; error?: unknown; detail?: unknown };
		if (typeof value.message === "string") return value.message;
		const nested = value.error;
		if (typeof nested === "string") return nested;
		if (nested && typeof nested === "object" && typeof (nested as { message?: unknown }).message === "string") {
			return (nested as { message: string }).message;
		}
		if (typeof value.detail === "string") return value.detail;
	}
	return "";
}

function transportCode(reason: unknown): string {
	if (reason && typeof reason === "object") {
		const value = reason as { code?: unknown; error?: unknown };
		if (typeof value.code === "string") return value.code;
		const nested = value.error;
		if (nested && typeof nested === "object" && typeof (nested as { code?: unknown }).code === "string") {
			return (nested as { code: string }).code;
		}
	}
	return "";
}

export const STEER_UNSUPPORTED: SteerFailure = {
	code: "steer_unsupported",
	message:
		"Steering is not supported by this runtime. The prompt stays queued and sends as the next turn.",
	detail: "runtime bridge does not expose steerTurn"
};

const STEER_ERROR_CODES: readonly string[] = [
	"steer_turn_finished",
	"steer_unsupported",
	"steer_empty",
	"steer_unauthorized",
	"steer_rejected",
	"steer_unavailable"
];

function isSteerFailure(reason: unknown): reason is SteerFailure {
	if (!reason || typeof reason !== "object") return false;
	const value = reason as { code?: unknown; message?: unknown; detail?: unknown };
	return (
		typeof value.code === "string" &&
		STEER_ERROR_CODES.includes(value.code) &&
		typeof value.message === "string" &&
		typeof value.detail === "string"
	);
}

/**
 * Turn any rejection into a stable public code plus one actionable sentence.
 * The original survives in `detail` for engineering inspection; it never
 * reaches the composer.
 */
export function normalizeSteerFailure(reason: unknown): SteerFailure {
	// Already normalized upstream — re-wrapping would bury the public code.
	if (isSteerFailure(reason)) return reason;
	const detail = describe(reason);
	const message = rawMessage(reason);
	const code = transportCode(reason);
	if (/has no active turn to steer|no active turn|turn (has )?(already )?finished/i.test(message)) {
		return {
			code: "steer_turn_finished",
			message: "That turn finished before the steer arrived. The prompt stays queued and sends as the next turn.",
			detail
		};
	}
	if (/steer text must not be empty|must not be empty/i.test(message)) {
		return {
			code: "steer_empty",
			message: "An empty prompt cannot steer a turn. Add some text and try again.",
			detail
		};
	}
	if (code === "unauthorized" || /unauthorized|not signed in|re-?authenticate/i.test(message)) {
		return {
			code: "steer_unauthorized",
			message: "This runtime rejected the steer as unauthorized. Reconnect the account in Settings, then try again.",
			detail
		};
	}
	if (!message.trim()) {
		return {
			code: "steer_unavailable",
			message: "The steer could not be delivered. The prompt stays queued — try again, or let it send as the next turn.",
			detail
		};
	}
	return {
		code: "steer_rejected",
		// Redacted so an internal id embedded in a runtime message cannot reach
		// the composer through the message path either.
		message: `The active turn rejected steering: ${redactIdentifiers(message.trim())}`,
		detail
	};
}

/** One line of user-facing copy. Always a string, never an interpolated object. */
export function steerFailureMessage(failure: SteerFailure): string {
	return failure.message;
}
