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

import type { OptimizerRunStatus } from "../../generated/protocol";

export const RUN_PROGRESS_SCHEMA_VERSION = "run_progress.v1";

/** Workflows that share the projection. DAG remains a diagnostic-only legacy run. */
export type RunKind = "eval" | "gepa" | "go-ex" | "sft" | "cispo" | "environment";

export type RunProgressStatus =
	| "queued"
	| "running"
	| "paused"
	| "interrupted"
	| "completed"
	| "failed"
	| "cancelled"
	/** Terminal: the compute settled but its evidence did not. */
	| "degraded"
	/**
	 * A word this build does not know. Not a state a producer emits — it is
	 * what the renderer says when it is handed one. It used to say `running`,
	 * which is how a settled run kept a spinner turning forever.
	 */
	| "unknown";

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

export type CoveredMetricSource = "provider" | "proxy" | "container" | "derived" | "unavailable";

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
	/** Calls covered by a durable provider receipt, when this metric came from one. */
	receiptCalls?: number;
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
/**
 * Whether the durable projection can answer "how much work happened".
 *
 * `unavailable` is a real state and must render as words. A campaign that
 * finished ten of ten rollouts and lost its event history is not a campaign
 * that ran zero trials, and a card that prints "0 trials" for it is asserting
 * something no evidence supports.
 */
export type RunProgressEvidence = {
	state: "present" | "unavailable" | "degraded";
	/** One line the user can act on. Present whenever state is not `present`. */
	reason?: string;
	/** Where to look: cursor, missing event types, sealed manifest. */
	diagnostic?: string;
};

/**
 * What a search is allowed to conclude, sealed by the run's terminal manifest.
 *
 * A GEPA run reporting `Heldout 0.600` says nothing about whether the search
 * worked — that number is frequently the *seed's*, retained because no proposal
 * beat it. The verdict is the missing half, and it is computed once in the
 * service from the durable log rather than re-guessed per surface.
 */
export type RunProgressVerdict =
	| "measured_improvement"
	| "no_measured_improvement"
	| "inconclusive"
	| "failed";

export type RunProgressResult = {
	headline?: string;
	detail?: string;
	absentReason?: string;
	partial: boolean;
	/** Absent on algorithms that do not select, and on runs with no sealed manifest. */
	verdict?: RunProgressVerdict;
	/** One line of the evidence the verdict rests on, e.g. the uplift and its sample count. */
	verdictDetail?: string;
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
	/**
	 * Whether `work` and `progress` rest on durable evidence. Surfaces must
	 * check this before rendering a count: absent evidence is shown as
	 * "Progress unavailable", never as zero.
	 */
	evidence: RunProgressEvidence;
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
	/** Compact provider-proxy status. Never includes a key or capability token. */
	providerAccess?: {
		provider: string;
		status: string;
		suffix?: string;
		usedCalls: number;
		maxCalls: number;
		/** Null means the capability has no authoritative cost telemetry. */
		usedCostUsd: number | null;
		maxCostUsd: number | null;
		note?: string;
	};
	result?: RunProgressResult;
	/** Primary visual instance id for "Open visual", when the run published one. */
	fullVisualRef?: string;
	/** Durable event sequence this projection was computed at. */
	cursorSeq: number;
	/**
	 * Frozen at terminal transition (O-5). Live runs equal `cursorSeq`.
	 * Authoritative usage/result reduce only through this cursor.
	 */
	terminalCursor: number;
	/** Newest post-terminal enrichment sequence, when an enrichment lane exists. */
	enrichmentCursor?: number;
	/** Count of enrichment-lane events withheld from the authoritative projection. */
	enrichmentEventCount?: number;
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

export const RUN_KINDS: readonly RunKind[] = [
	"eval",
	"gepa",
	"go-ex",
	"sft",
	"cispo",
	"environment"
];

export function isRunKind(value: unknown): value is RunKind {
	return typeof value === "string" && (RUN_KINDS as readonly string[]).includes(value);
}

/**
 * Every status a producer can write, mapped onto the eight the product shows.
 *
 * The key type is the generated `OptimizerRunStatus` union, so this map is the
 * one place a producer status is interpreted and TypeScript refuses to compile
 * if Rust adds a status nothing here handles — or if this file invents one Rust
 * does not have. That check is the lock; there is no runtime list to drift.
 */
const PRODUCER_STATUS: Record<OptimizerRunStatus, RunProgressStatus> = {
	queued: "queued",
	validating: "queued",
	provisioning: "queued",
	starting: "queued",
	waiting_for_viewer: "queued",
	running: "running",
	// A cancel that has been accepted but not carried out is still burning
	// compute; it is not `cancelled` until the record says so.
	cancelling: "running",
	env_unreachable: "running",
	paused: "paused",
	degraded: "degraded",
	// The work ran; only its receipt is missing. Same surface as `degraded`.
	failed_evidence: "degraded",
	completed: "completed",
	failed: "failed",
	cancelled: "cancelled",
	interrupted: "interrupted",
	// Stopped before its own end, and not by its own error. `failed` would
	// blame the run for the machine, and `completed` would claim a result it
	// never reached.
	infrastructure_lost: "interrupted",
	cap_reached: "interrupted"
};

/**
 * Spellings older builds persisted into `payload_json` before migration 28
 * folded the column. Rust `OptimizerRunStatus::parse` accepts exactly these;
 * this list exists so a database written by an older build still reads, and it
 * shrinks as those rows are rewritten.
 *
 * `terminated`, `disconnected`, `stalled`, `pending` and `prepared` used to be
 * listed here too. No producer has ever written any of them as a run status —
 * they were consumer-only guesses, and they are gone.
 */
const LEGACY_PAYLOAD_ALIASES: Record<string, OptimizerRunStatus> = {
	succeeded: "completed",
	canceled: "cancelled",
	created: "queued",
	error: "failed",
	done: "failed",
	stopped: "failed",
	aborted: "failed"
};

const TERMINAL_PROGRESS_STATUSES: ReadonlySet<RunProgressStatus> = new Set<RunProgressStatus>([
	"completed",
	"failed",
	"cancelled",
	"interrupted",
	// A run whose evidence lane failed is finished, not still working. Leaving
	// it live would spin a card forever over compute that already stopped.
	"degraded"
]);

/** The producer word for a stored status, or `null` when nothing recognises it. */
function producerStatus(status: string | null | undefined): OptimizerRunStatus | null {
	const value = (status ?? "").toLowerCase();
	if (value in PRODUCER_STATUS) return value as OptimizerRunStatus;
	return LEGACY_PAYLOAD_ALIASES[value] ?? null;
}

/**
 * Has this run stopped? An unrecognised word is *not* terminal: the durable
 * record is the terminal authority, and a word this build cannot read is not
 * that authority saying "finished".
 */
export function isTerminalRunStatus(status: string | null | undefined): boolean {
	const producer = producerStatus(status);
	return producer !== null && TERMINAL_PROGRESS_STATUSES.has(PRODUCER_STATUS[producer]);
}

/**
 * Normalize a producer status onto the ones the product shows. Anything this
 * build does not recognise reads as `unknown` — never `running`, which is the
 * spelling that used to keep a finished run's card alive.
 */
export function normalizeRunStatus(status: string | null | undefined): RunProgressStatus {
	const producer = producerStatus(status);
	return producer === null ? "unknown" : PRODUCER_STATUS[producer];
}
