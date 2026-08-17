/**
 * `run_progress.v1` — the one algorithm-neutral projection chat reads.
 *
 * Chat is not taught GEPA, SFT, or eval event vocabularies. Each algorithm has
 * a `RunProgressAdapter` that turns its own projected slice into this shape,
 * and every presentation — the compact transcript card, the expanded dialog,
 * notifications, reports — reads only this.
 *
 * Three rules the types themselves enforce:
 *
 *   · A denominator is optional. `work.total` and `progress.fraction` are
 *     absent when no truthful denominator exists; there is no "assume 100".
 *   · Usage is a `CoveredMetric`, never a bare number. A missing value is
 *     `undefined` with `source: "unavailable"`, so nothing can render 0 for
 *     telemetry that was never reported.
 *   · An ETA is derived evidence with a written-down basis, not a promise. Its
 *     `state` can say `estimating` or `unavailable`, and callers must handle
 *     both rather than printing a number they do not have.
 */

export const RUN_PROGRESS_SCHEMA_VERSION = "run_progress.v1";

/** Workflows that share the projection. `go-ex` and `dag` runs are not offered in chat. */
export type RunKind = "eval" | "gepa" | "sft";

export type RunProgressStatus =
	| "queued"
	| "running"
	| "paused"
	| "completed"
	| "failed"
	| "cancelled";

export type RunProgressPhaseStatus = "pending" | "active" | "completed" | "skipped" | "failed";

export type RunProgressPhase = {
	id: string;
	label: string;
	status: RunProgressPhaseStatus;
	detail?: string;
	startedAt?: string;
	endedAt?: string;
};

/**
 * Bounded work in the unit the run actually counts. Every field is optional
 * because a producer that never reported a count has not reported zero.
 */
export type RunProgressWork = {
	completed?: number;
	active?: number;
	queued?: number;
	failed?: number;
	retried?: number;
	total?: number;
	/** "rollouts", "trials", "steps" — plural, lowercase, shown beside the count. */
	unit?: string;
};

export type RunProgressBar = {
	/** 0–1. Absent whenever `determinate` is false. */
	fraction?: number;
	/** What the bar measures, e.g. "campaign completion". Never reward quality. */
	semantics: string;
	determinate: boolean;
};

export type RunEtaState = "estimating" | "range" | "point" | "unavailable" | "paused";

export type RunEtaConfidence = "warming" | "low" | "medium" | "high";

export type RunEtaProjection = {
	state: RunEtaState;
	/** Point estimate. Present for `point`; the range midpoint for `range`. */
	remainingMs?: number;
	lowMs?: number;
	highMs?: number;
	confidence: RunEtaConfidence;
	/**
	 * Why this number exists, in one line, persisted so the dialog can explain
	 * the estimate instead of asserting it: "median of 7 rollouts completed in
	 * phase minibatch". Present even when the state is `unavailable`.
	 */
	basis: string;
	/** Comparable completed units the estimate was built from. */
	sampleCount: number;
	/** Set when `state` is `unavailable`, naming what is missing. */
	unavailableReason?: string;
};

export type CoveredMetricSource = "provider" | "container" | "derived" | "unavailable";

/**
 * A number plus who vouches for it and how much of the run it covers. A
 * `value` of `undefined` means unreported; it is never rendered as 0.
 */
export type CoveredMetric = {
	value?: number;
	/** Units that actually reported this metric. */
	observedUnits: number;
	/** Units expected to report it, when a denominator exists. */
	expectedUnits?: number;
	/** observed / expected, 0–1. Absent without a denominator. */
	coverage?: number;
	source: CoveredMetricSource;
};

export type RunUsageProjection = {
	costUsd: CoveredMetric;
	promptTokens: CoveredMetric;
	completionTokens: CoveredMetric;
	rollouts: CoveredMetric;
};

export type RunProgressCapabilities = {
	pause: boolean;
	resume: boolean;
	cancel: boolean;
};

export type RunProgressMilestone = {
	label: string;
	detail?: string;
	occurredAt?: string;
	/** Durable event sequence the milestone came from; used for de-duplication. */
	sequence?: number;
};

/** One labelled fact for the dialog's algorithm-specific block. */
export type RunProgressDetail = {
	label: string;
	value: string;
	/** Where the value came from, when that is load-bearing. */
	note?: string;
};

/**
 * The terminal headline. `absent` is a first-class outcome: a run can finish
 * without having measured anything worth promoting, and saying so is correct.
 */
export type RunProgressResult = {
	headline?: string;
	detail?: string;
	absentReason?: string;
	partial: boolean;
};

export type RunProgressProjection = {
	schemaVersion: typeof RUN_PROGRESS_SCHEMA_VERSION;
	runId: string;
	runKind: RunKind;
	title: string;
	status: RunProgressStatus;
	terminal: boolean;
	phase: RunProgressPhase;
	/** The whole timeline, for the dialog. Always includes `phase`. */
	phases: RunProgressPhase[];
	work: RunProgressWork;
	progress?: RunProgressBar;
	timing: {
		startedAt?: string;
		elapsedMs?: number;
		/** Newest durable event timestamp; the basis for update-latency telemetry. */
		lastEventAt?: string;
		eta?: RunEtaProjection;
	};
	usage: RunUsageProjection;
	capabilities: RunProgressCapabilities;
	/** The single throughput/concurrency fact worth the compact card's one line. */
	throughput?: { label: string; detail?: string };
	/** Newest milestone; the same object as `milestones.at(-1)`. */
	milestone?: RunProgressMilestone;
	milestones: RunProgressMilestone[];
	/** Newest warning; the same string as `warnings[0]`. */
	warning?: string;
	warnings: string[];
	details: RunProgressDetail[];
	result?: RunProgressResult;
	/** Visual instance id for "Open full run", when the run published one. */
	fullVisualRef?: string;
	/** Durable event sequence this projection was computed at. */
	cursorSeq: number;
	/** The event history is known-incomplete; counts below are a floor, not a total. */
	stale: boolean;
};

/**
 * A control request. Intent is separate from observed state on purpose: a
 * pause the user asked for is not a paused run until the durable record says
 * so, and the UI must show the difference.
 */
export type RunControlIntent = {
	runId: string;
	action: "pause" | "resume" | "cancel";
	state: "requested" | "acknowledged" | "failed";
	requestedAt: number;
	error?: string;
};

/** What the transcript stores. Everything else is derived from the durable run. */
export type RunProgressTranscriptItem = {
	kind: "run_progress";
	runId: string;
	runKind: RunKind;
	createdAt: string;
};

export const RUN_KINDS: readonly RunKind[] = ["eval", "gepa", "sft"];

export function isRunKind(value: unknown): value is RunKind {
	return typeof value === "string" && (RUN_KINDS as readonly string[]).includes(value);
}

/** Terminal statuses as the durable record spells them, across producers. */
const TERMINAL_STATUSES = new Set([
	"completed",
	"succeeded",
	"failed",
	"terminated",
	"cancelled",
	"canceled"
]);

export function isTerminalRunStatus(status: string | null | undefined): boolean {
	return TERMINAL_STATUSES.has((status ?? "").toLowerCase());
}

/**
 * Normalize a producer status onto the six the product shows. Anything not
 * recognised while the run is live reads as `running` — the run record is the
 * terminal authority, so only its terminal spellings may end a card.
 */
export function normalizeRunStatus(status: string | null | undefined): RunProgressStatus {
	const value = (status ?? "").toLowerCase();
	if (value === "failed" || value === "terminated") return "failed";
	if (value === "cancelled" || value === "canceled") return "cancelled";
	if (value === "completed" || value === "succeeded") return "completed";
	if (value === "paused") return "paused";
	if (value === "queued" || value === "created" || value === "pending" || value === "prepared") {
		return "queued";
	}
	return "running";
}
