/**
 * `run_progress.v1` producer contract — the pin for Lanes B/C/D.
 *
 * Workshop chat never learns GEPA, eval, SFT, or environment event names.
 * Producers emit `optimizer_event.v1` envelopes; adapters project them into
 * one `run_progress.v1` shape. This module is the vocabulary those producers
 * must stay inside, and the fixture tests below it are how a drift is caught.
 *
 * Rules that travel with the pin:
 *
 *   · Missing usage/token/cost fields are omitted (or JSON `null`). They are
 *     never numeric zero. Zero is a reported value.
 *   · `sequenceNumber` is the durable identity. It does not restart after
 *     terminal; post-terminal enrichment rides `lane: "enrichment"` and a
 *     separate cursor.
 *   · `optimizer_terminal_cursor` / `summary.terminalCursor` freezes at the
 *     terminal event. Workshop projections reduce only through that cursor
 *     for authoritative usage, phase, progress, and result.
 */

export const EVENT_CONTRACT_PIN = "run_progress.event-contract.v1";
export const EVENT_SCHEMA_VERSION = "optimizer_event.v1";

export const WORKFLOW_FAMILIES = ["eval", "gepa", "sft", "environment"] as const;
export type WorkflowFamily = (typeof WORKFLOW_FAMILIES)[number];

/** Shared envelope every family event must carry. */
export const ENVELOPE_FIELDS = [
	"schemaVersion",
	"eventId",
	"type",
	"sequenceNumber",
	"occurredAt",
	"optimizerRunId",
	"algorithmId"
] as const;

/**
 * Usage keys a producer may report. Presence is coverage; absence is
 * unavailable. A key set to `null` is also unavailable, never a zero.
 */
export const USAGE_DELTA_KEYS = [
	"cost_usd",
	"costUsd",
	"prompt_tokens",
	"promptTokens",
	"completion_tokens",
	"completionTokens",
	"rollouts",
	"rollout_count",
	"rolloutCount"
] as const;

export const EVENT_LANES = ["terminal", "enrichment"] as const;
export type EventLane = (typeof EVENT_LANES)[number];

/**
 * Per-family event types the adapters reduce. Unknown types are ignored, not
 * fatal — a producer may emit extras — but a fixture for a family must include
 * the types that family uses to express phase, work, and terminality.
 */
export const FAMILY_EVENT_TYPES: Record<WorkflowFamily, readonly string[]> = {
	gepa: [
		"gepa.run.started",
		"gepa.run.finished",
		"candidate.registered",
		"optimizer.limit.estimate_updated",
		"optimizer.state.transitioned",
		"optimizer.rollout_queue.updated",
		"optimizer.evaluation_result.received",
		"optimizer.child_rollout.completed",
		"optimizer.child_rollout.failed",
		"optimizer.child_rollout.retried",
		"frontier.updated",
		"rollout.circuit_breaker.tripped",
		"optimizer.run.completed",
		"optimizer.run.failed",
		"optimizer.run.cancelled"
	],
	eval: [
		"eval.run.planned",
		"eval.seed_ledger.sealed",
		"eval.trial.queued",
		"eval.trial.started",
		"eval.trial.terminal",
		"eval.run.paused",
		"eval.selection.completed",
		"eval.selection.decided",
		"rollout.circuit_breaker.tripped"
	],
	sft: [
		"run.queued",
		"run.started",
		"sft.run.queued",
		"sft.run.started",
		"sft.dataset.validated",
		"sft.compute.updated",
		"sft.training.metrics",
		"sft.step.metrics",
		"training.metrics",
		"training.job.completed",
		"training.artifact.materialized",
		"training.terminal.mapped",
		"sft.checkpoint.ready",
		"sft.checkpoint.promoted",
		"sft.checkpoint_rollout.completed",
		"sft.checkpoint_rollout.failed",
		"sft.run.completed"
	],
	environment: [
		"environment.run.planned",
		"environment.run.started",
		"environment.episode.started",
		"environment.step.completed",
		"environment.episode.terminal",
		"environment.run.completed",
		"environment.run.failed",
		"container.task_info.loaded",
		"container.contract.verified",
		"container.rollout.start",
		"container.rollout.completed",
		"container.rollout.failed"
	]
};

export type ProtocolEvent = {
	schemaVersion?: string;
	eventId?: string;
	type: string;
	sequenceNumber: number;
	occurredAt: string;
	optimizerRunId: string;
	algorithmId: string;
	lane?: EventLane | string;
	delta?: Record<string, unknown>;
	snapshot?: Record<string, unknown>;
	usageDelta?: Record<string, number | null | undefined>;
	item?: Record<string, unknown>;
};

export type EnvelopeIssue = {
	path: string;
	message: string;
};

function isRecord(value: unknown): value is Record<string, unknown> {
	return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

export function eventLaneOf(event: { lane?: unknown; delta?: Record<string, unknown> }): EventLane {
	const raw = event.lane ?? event.delta?.lane;
	return raw === "enrichment" ? "enrichment" : "terminal";
}

/**
 * Validate one producer event against the envelope. Does not require the type
 * to be in `FAMILY_EVENT_TYPES` — extras are allowed — but it does refuse a
 * missing sequence, a fabricated usage zero that was never keyed, and a lane
 * other than the two this contract names.
 *
 * `usageDelta` keys that are present must be `number | null`. `null` is
 * unavailable. A missing key is unavailable. A numeric `0` is a reported zero.
 */
export function validateProtocolEvent(event: unknown): EnvelopeIssue[] {
	const issues: EnvelopeIssue[] = [];
	if (!isRecord(event)) {
		return [{ path: "", message: "event is not an object" }];
	}
	if (event.schemaVersion != null && event.schemaVersion !== EVENT_SCHEMA_VERSION) {
		issues.push({
			path: "schemaVersion",
			message: `expected ${EVENT_SCHEMA_VERSION}, got ${String(event.schemaVersion)}`
		});
	}
	for (const field of ["type", "occurredAt", "optimizerRunId", "algorithmId"] as const) {
		if (typeof event[field] !== "string" || (event[field] as string).length === 0) {
			issues.push({ path: field, message: `${field} must be a non-empty string` });
		}
	}
	const sequence = Number(event.sequenceNumber ?? event.sequence_number);
	if (!Number.isSafeInteger(sequence) || sequence < 1) {
		issues.push({ path: "sequenceNumber", message: "sequenceNumber must be a positive integer" });
	}
	if (event.lane != null && event.lane !== "terminal" && event.lane !== "enrichment") {
		issues.push({ path: "lane", message: `lane must be terminal or enrichment, got ${String(event.lane)}` });
	}
	if (event.usageDelta != null) {
		if (!isRecord(event.usageDelta)) {
			issues.push({ path: "usageDelta", message: "usageDelta must be an object when present" });
		} else {
			for (const [key, value] of Object.entries(event.usageDelta)) {
				if (value == null) continue;
				if (typeof value !== "number" || !Number.isFinite(value)) {
					issues.push({
						path: `usageDelta.${key}`,
						message: "usage fields are finite numbers or null; missing keys stay absent"
					});
				}
			}
		}
	}
	return issues;
}

export function validateProtocolStream(events: unknown[]): EnvelopeIssue[] {
	const issues: EnvelopeIssue[] = [];
	const seen = new Set<number>();
	let cursor = 0;
	for (const [index, event] of events.entries()) {
		for (const issue of validateProtocolEvent(event)) {
			issues.push({ path: `${index}.${issue.path}`.replace(/\.$/, ""), message: issue.message });
		}
		if (!isRecord(event)) continue;
		const sequence = Number(event.sequenceNumber ?? event.sequence_number);
		if (!Number.isSafeInteger(sequence) || sequence < 1) continue;
		if (seen.has(sequence)) {
			issues.push({ path: String(index), message: `duplicate sequence ${sequence}` });
		}
		seen.add(sequence);
		if (sequence > cursor + 1) {
			issues.push({
				path: String(index),
				message: `sequence hole at ${sequence}; last contiguous cursor was ${cursor}`
			});
		}
		if (sequence > cursor) cursor = sequence;
	}
	return issues;
}

export function isWorkflowFamily(value: unknown): value is WorkflowFamily {
	return typeof value === "string" && (WORKFLOW_FAMILIES as readonly string[]).includes(value);
}
