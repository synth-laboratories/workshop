/**
 * Read-path telemetry — what a visual actually cost to open.
 *
 * The experience budgets in `telemetry.ts` measure the *card*: time to first
 * progress, update latency, estimate coverage. These measure the *read* behind
 * it, which is a different question and the one the visual-data work was
 * accountable to:
 *
 *   · first paint — subscribe until the projection mounts an aggregate;
 *   · interactive — subscribe until raw evidence has finished hydrating;
 *   · stage attribution — projection, metadata, and evidence timed separately,
 *     so a slow open names its own owner instead of collapsing into
 *     "subscription stalled";
 *   · payload — event pages and events read before first paint, which must be
 *     zero once the aggregate stopped waiting for the journal;
 *   · probes — how many freshness checks ran and how many carried no payload,
 *     which is the whole claim of the conditional read;
 *   · replays — full reloads from sequence zero, and why.
 *
 * One record per run, flushed when it settles, so a two-hour run reports once.
 * Nothing is inferred: a run that never reached interactive reports no
 * interactive time rather than a guess.
 */

export type ReadPathTelemetry = {
	runId: string;
	/** Subscribe → the projection published and the visual could mount. */
	firstPaintMs?: number;
	/** Subscribe → raw evidence finished hydrating. */
	interactiveMs?: number;
	/** Slowest observed read of the durable projection. */
	projectionMaxMs?: number;
	/** Slowest observed evidence page. */
	evidencePageMaxMs?: number;
	/** Event pages read before first paint. Should be zero. */
	pagesBeforeFirstPaint: number;
	/** Events read before first paint. Should be zero. */
	eventsBeforeFirstPaint: number;
	pages: number;
	events: number;
	probes: number;
	/** Probes answered `unchanged`, carrying no payload. */
	probesUnchanged: number;
	/** Full reloads from sequence zero. */
	replays: number;
	replayReasons: string[];
	failures: number;
};

type Counters = {
	startedAtMs: number;
	firstPaintMs?: number;
	interactiveMs?: number;
	projectionMaxMs?: number;
	evidencePageMaxMs?: number;
	painted: boolean;
	pagesBeforeFirstPaint: number;
	eventsBeforeFirstPaint: number;
	pages: number;
	events: number;
	probes: number;
	probesUnchanged: number;
	replays: number;
	replayReasons: Set<string>;
	failures: number;
	flushed: boolean;
};

const counters = new Map<string, Counters>();
let sink: ((record: ReadPathTelemetry) => void) | null = null;

/** Install the reporter. Called once, from the renderer entry point. */
export function installReadPathTelemetry(next: (record: ReadPathTelemetry) => void): void {
	sink = next;
}

/** Tests only. */
export function resetReadPathTelemetry(): void {
	counters.clear();
}

/** Tests only: read counters without flushing them. */
export function peekReadPath(runId: string): ReadPathTelemetry | null {
	const counter = counters.get(runId);
	return counter ? project(runId, counter) : null;
}

function counterFor(runId: string, now: number): Counters {
	const existing = counters.get(runId);
	if (existing) return existing;
	const created: Counters = {
		startedAtMs: now,
		painted: false,
		pagesBeforeFirstPaint: 0,
		eventsBeforeFirstPaint: 0,
		pages: 0,
		events: 0,
		probes: 0,
		probesUnchanged: 0,
		replays: 0,
		replayReasons: new Set(),
		failures: 0,
		flushed: false
	};
	counters.set(runId, created);
	return created;
}

export function recordReadStarted(runId: string, now: number): void {
	counterFor(runId, now);
}

/** One stage of one load completed. */
export function recordStage(
	runId: string,
	stage: "projection" | "metadata" | "evidence_page",
	elapsedMs: number,
	now: number
): void {
	const counter = counterFor(runId, now);
	if (stage === "evidence_page") {
		counter.evidencePageMaxMs = Math.max(counter.evidencePageMaxMs ?? 0, elapsedMs);
		return;
	}
	counter.projectionMaxMs = Math.max(counter.projectionMaxMs ?? 0, elapsedMs);
}

/** The projection published; the visual can mount. */
export function recordFirstPaint(runId: string, now: number): void {
	const counter = counterFor(runId, now);
	if (counter.painted) return;
	counter.painted = true;
	counter.firstPaintMs = now - counter.startedAtMs;
}

/** Raw evidence finished hydrating. */
export function recordInteractive(runId: string, now: number): void {
	const counter = counterFor(runId, now);
	if (counter.interactiveMs != null) return;
	counter.interactiveMs = now - counter.startedAtMs;
}

export function recordEvidencePage(runId: string, events: number, now: number): void {
	const counter = counterFor(runId, now);
	counter.pages += 1;
	counter.events += events;
	// Pages read before the aggregate mounted are the regression this work
	// exists to prevent: they are what made optional detail a blocking
	// dependency, so they are counted separately rather than summed away.
	if (!counter.painted) {
		counter.pagesBeforeFirstPaint += 1;
		counter.eventsBeforeFirstPaint += events;
	}
}

export function recordProbe(runId: string, unchanged: boolean, now: number): void {
	const counter = counterFor(runId, now);
	counter.probes += 1;
	if (unchanged) counter.probesUnchanged += 1;
}

export function recordReplay(runId: string, reason: string, now: number): void {
	const counter = counterFor(runId, now);
	counter.replays += 1;
	counter.replayReasons.add(reason);
}

export function recordReadFailure(runId: string, now: number): void {
	counterFor(runId, now).failures += 1;
}

function project(runId: string, counter: Counters): ReadPathTelemetry {
	return {
		runId,
		...(counter.firstPaintMs != null ? { firstPaintMs: counter.firstPaintMs } : {}),
		...(counter.interactiveMs != null ? { interactiveMs: counter.interactiveMs } : {}),
		...(counter.projectionMaxMs != null ? { projectionMaxMs: counter.projectionMaxMs } : {}),
		...(counter.evidencePageMaxMs != null ? { evidencePageMaxMs: counter.evidencePageMaxMs } : {}),
		pagesBeforeFirstPaint: counter.pagesBeforeFirstPaint,
		eventsBeforeFirstPaint: counter.eventsBeforeFirstPaint,
		pages: counter.pages,
		events: counter.events,
		probes: counter.probes,
		probesUnchanged: counter.probesUnchanged,
		replays: counter.replays,
		replayReasons: [...counter.replayReasons].sort(),
		failures: counter.failures
	};
}

/**
 * Emit one record and stop counting for this run.
 *
 * Also drops the counter, so watching a long session does not accumulate one
 * small record per run for the life of the window.
 */
export function flushReadPathTelemetry(runId: string): ReadPathTelemetry | null {
	const counter = counters.get(runId);
	if (!counter || counter.flushed) return null;
	counter.flushed = true;
	const record = project(runId, counter);
	counters.delete(runId);
	try {
		sink?.(record);
	} catch {
		// Failing to record a measurement must never become a second failure.
	}
	return record;
}
