/**
 * Composer model / effort chip decision tree for Codex app-server chats.
 *
 * BEFORE (rejected):
 *   model chip change ──► kick to landing ──► next send = brand new session
 *   effort chip change ──► next send uses new effort (OK)
 *
 * AFTER (compact on send only):
 *   model chip change
 *           │
 *           ▼
 *     pendingTarget = B     [no compact, no rebind, still chat A]
 *     session.target stays A until send
 *
 *   user──── send message M ────►
 *           │
 *           ├─ pendingTarget == session.target ?
 *           │         │
 *           │         ├─ yes ──► turn/start(M) on A
 *           │         │
 *           │         └─ no ──► compact(A)          [skip when empty thread]
 *           │                      │
 *           │                      ▼
 *           │                   rebind thread → B
 *           │                      │
 *           │                      ▼
 *           │                   session.target = B
 *           │                      │
 *           │                      ▼
 *           │                   turn/start(M) on B
 *           │                   (+ <model_switch>)
 *
 * Edge cases that change with “compact on send”:
 *   - Fiddle A→B→A without sending: no compact, no switch.
 *   - Fiddle A→B, then send: one compact on A, then B handles M.
 *   - Cancel / never send: zero cost.
 *   - Images pending + switch to text-only: validate on send against destination.
 *   - Turn already running: refuse send/switch until idle.
 *   - Empty thread, first message, chip already B: no compact; rebind then send.
 *   - Effort-only on send: no compact — only pass effort.
 */

export type ModelChipChangePlan = {
	/** Composer pending target only. session.target is unchanged. */
	pendingTargetId: string;
	/** Stay on the current chat; never kick to landing. */
	kickToLanding: false;
	compact: false;
	rebind: false;
};

export type EffortChipChangePlan = {
	/** Persist knob for the (pending) target; applied on next turn/start. */
	persistKnob: true;
	compact: false;
	rebind: false;
};

export type ComposerSendPlan =
	| {
			kind: "turn_start";
			/** Destination equals the bound session target. */
			targetId: string;
			compact: false;
			rebind: false;
	  }
	| {
			kind: "model_switch_then_turn";
			sourceTargetId: string;
			destinationTargetId: string;
			/**
			 * Compact on the source model before rebind when the thread has
			 * history. Empty threads skip compact (nothing to summarize).
			 */
			compact: boolean;
			rebind: true;
	  }
	| {
			kind: "block";
			reason: "turn_running" | "images_unsupported_on_destination";
			message: string;
	  };

export type PlanModelChipChangeInput = {
	nextTargetId: string;
};

/**
 * Model chip change updates pendingTarget only.
 * No compact, no rebind, no landing kick — session.target stays until send.
 */
export function planModelChipChange(input: PlanModelChipChangeInput): ModelChipChangePlan {
	return {
		pendingTargetId: input.nextTargetId,
		kickToLanding: false,
		compact: false,
		rebind: false
	};
}

/**
 * Effort chip change is always localStorage + next turn/start effort.
 * Never triggers compact or model rebind.
 */
export function planEffortChipChange(): EffortChipChangePlan {
	return { persistKnob: true, compact: false, rebind: false };
}

export type PlanComposerSendInput = {
	/** Composer chip selection (pendingTarget). */
	pendingTargetId: string;
	/** Bound thread target (session.target as UI id). */
	sessionTargetId: string;
	/** True when the session already has prior user/assistant/turn activity. */
	threadHasHistory: boolean;
	/** Session is mid-turn (streaming / running). */
	turnRunning: boolean;
	/** Composer has pending image attachments for this send. */
	hasPendingImages: boolean;
	/** Destination model accepts image input. */
	destinationSupportsImages: boolean;
};

/**
 * Send-time state machine. Compact runs only here, and only when the pending
 * model differs from the bound session model and the thread has history.
 */
export function planComposerSend(input: PlanComposerSendInput): ComposerSendPlan {
	const switching = input.pendingTargetId !== input.sessionTargetId;

	if (switching && input.turnRunning) {
		return {
			kind: "block",
			reason: "turn_running",
			message: "Wait for the current turn to finish before switching models."
		};
	}

	if (switching && input.hasPendingImages && !input.destinationSupportsImages) {
		return {
			kind: "block",
			reason: "images_unsupported_on_destination",
			message: "This model does not support image input. Remove the screenshots or choose a multimodal model before sending."
		};
	}

	if (!switching) {
		if (input.hasPendingImages && !input.destinationSupportsImages) {
			return {
				kind: "block",
				reason: "images_unsupported_on_destination",
				message: "This model does not support image input. The message was not sent."
			};
		}
		return {
			kind: "turn_start",
			targetId: input.sessionTargetId,
			compact: false,
			rebind: false
		};
	}

	return {
		kind: "model_switch_then_turn",
		sourceTargetId: input.sessionTargetId,
		destinationTargetId: input.pendingTargetId,
		compact: input.threadHasHistory,
		rebind: true
	};
}

/** Prior transcript activity that means compact has something to summarize. */
export function threadHasHistoryFromEvents(
	events: ReadonlyArray<{ eventKind: string; payload?: Record<string, unknown> }>
): boolean {
	return events.some((event) => {
		if (
			event.eventKind === "run.started"
			|| event.eventKind === "run.completed"
			|| event.eventKind === "run.failed"
			|| event.eventKind === "run.cancelled"
			|| event.eventKind === "message.completed"
			|| event.eventKind === "message.delta"
			|| event.eventKind === "agent.reasoning"
		) {
			return true;
		}
		if (event.eventKind === "message.created") {
			const role = event.payload?.role;
			return role === "user" || role === "assistant";
		}
		return false;
	});
}
