/**
 * Experience-budget telemetry for run progress.
 *
 * Four things determine whether this feature is actually working, and none of
 * them are visible from a screenshot:
 *
 *   · time to first progress — how long a card shows "Loading run…";
 *   · update latency — how stale the newest number on screen is;
 *   · estimate coverage — what share of a live run had a usable ETA rather
 *     than "Estimating…" or "Unavailable";
 *   · stale cards — how often history was incomplete.
 *
 * The counters are per run and flushed once, when the run reaches a terminal
 * state, so a two-hour GEPA run produces one record rather than a stream of
 * them. Nothing here is inferred: a run that never left "Estimating…" reports
 * zero coverage, which is the honest reading.
 */

import type { RunEtaState } from "./types";

export type RunProgressTelemetry = {
	runId: string;
	runKind: string;
	/** Subscribe → first snapshot carrying a run record. */
	timeToFirstProgressMs?: number;
	/** Worst observed newest-event-to-render delay. */
	worstUpdateLatencyMs?: number;
	/** Share of live samples that offered a point or range estimate, 0–1. */
	estimateCoverage: number;
	samples: number;
	staleSamples: number;
};

type Counters = {
	runKind: string;
	startedAtMs: number;
	firstProgressMs?: number;
	worstLatencyMs?: number;
	samples: number;
	withEstimate: number;
	staleSamples: number;
	flushed: boolean;
};

const counters = new Map<string, Counters>();
let sink: ((record: RunProgressTelemetry) => void) | null = null;

/** Install the reporter. Called once, from the renderer entry point. */
export function installRunProgressTelemetry(next: (record: RunProgressTelemetry) => void): void {
	sink = next;
}

/** Tests only. */
export function resetRunProgressTelemetry(): void {
	counters.clear();
}

function counterFor(runId: string, runKind: string, now: number): Counters {
	const existing = counters.get(runId);
	if (existing) return existing;
	const created: Counters = {
		runKind,
		startedAtMs: now,
		samples: 0,
		withEstimate: 0,
		staleSamples: 0,
		flushed: false
	};
	counters.set(runId, created);
	return created;
}

/** Called when a card begins watching a run, before any snapshot has arrived. */
export function recordSubscribed(runId: string, runKind: string, now: number): void {
	counterFor(runId, runKind, now);
}

/**
 * Called on every projection a live card renders. `latencyMs` is the delay
 * between the newest durable event and this render; omit it when the run has
 * emitted nothing yet.
 */
export function recordSample(
	runId: string,
	runKind: string,
	options: { etaState?: RunEtaState; stale: boolean; latencyMs?: number; now: number }
): void {
	const counter = counterFor(runId, runKind, options.now);
	if (counter.firstProgressMs == null) {
		counter.firstProgressMs = Math.max(0, options.now - counter.startedAtMs);
	}
	counter.samples += 1;
	if (options.etaState === "point" || options.etaState === "range") counter.withEstimate += 1;
	if (options.stale) counter.staleSamples += 1;
	if (options.latencyMs != null && Number.isFinite(options.latencyMs) && options.latencyMs >= 0) {
		counter.worstLatencyMs = Math.max(counter.worstLatencyMs ?? 0, options.latencyMs);
	}
}

/**
 * Flush a run's counters. Idempotent: a card that re-renders after the run went
 * terminal does not emit a second record.
 */
export function flushRunTelemetry(runId: string): RunProgressTelemetry | null {
	const counter = counters.get(runId);
	if (!counter || counter.flushed) return null;
	counter.flushed = true;
	const record: RunProgressTelemetry = {
		runId,
		runKind: counter.runKind,
		...(counter.firstProgressMs != null ? { timeToFirstProgressMs: counter.firstProgressMs } : {}),
		...(counter.worstLatencyMs != null ? { worstUpdateLatencyMs: counter.worstLatencyMs } : {}),
		estimateCoverage: counter.samples > 0 ? counter.withEstimate / counter.samples : 0,
		samples: counter.samples,
		staleSamples: counter.staleSamples
	};
	try {
		sink?.(record);
	} catch {
		// Recording a measurement must never become a failure of its own.
	}
	return record;
}

/** The current counters for a run, for tests and diagnostics. */
export function runTelemetrySnapshot(runId: string): RunProgressTelemetry | null {
	const counter = counters.get(runId);
	if (!counter) return null;
	return {
		runId,
		runKind: counter.runKind,
		...(counter.firstProgressMs != null ? { timeToFirstProgressMs: counter.firstProgressMs } : {}),
		...(counter.worstLatencyMs != null ? { worstUpdateLatencyMs: counter.worstLatencyMs } : {}),
		estimateCoverage: counter.samples > 0 ? counter.withEstimate / counter.samples : 0,
		samples: counter.samples,
		staleSamples: counter.staleSamples
	};
}
