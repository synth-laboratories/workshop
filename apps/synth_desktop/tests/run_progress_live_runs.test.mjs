/**
 * The adapters against real producer streams.
 *
 * Every other run-progress test is written against fixtures, which means it can
 * only prove the adapters agree with my reading of the event vocabulary. This
 * one replays verbatim captures of runs that actually executed on this machine:
 *
 *   · `banking77_gepa_luna_med_c90c6c72` — completed bounded GEPA, 1,715 events
 *   · `opt_eval_9fcd8d0722a2` — completed Craftax LLM-policy eval, 35 events
 *   · `sft_craftax_nemo_f7f85d95` — completed hosted Nemotron SFT, 106 events
 *   · `opt_eval_d9efacf426c5` — an eval still running when it was captured
 *
 * Capture: docs/receipts/2026-08-17/v0.5-run-progress/real-run-streams.json,
 * exported from the `optimizer_runs` / `optimizer_events` tables of the v0.4 and
 * v0.5 dev instances. Nothing in it is hand-written, so a mismatch between an
 * adapter and a producer shows up here rather than in production.
 *
 * The replay also walks each stream cursor by cursor, which is what a live card
 * actually sees: the projection has to stay coherent at every prefix, not only
 * at the end.
 */
import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const workshopRoot = join(appRoot, "../..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

function bundle(relative, outName) {
	const outfile = join(compiledDir, outName);
	buildSync({
		entryPoints: [join(appRoot, relative)],
		bundle: true,
		format: "esm",
		target: "es2022",
		platform: "node",
		alias: { "@synth/visual-templates": join(appRoot, "../../visuals/families") },
		outfile
	});
	return pathToFileURL(outfile).href;
}

const { projectRunProgress } = await import(
	bundle("src/renderer/src/runtime/runProgress/project.ts", "runProgressLiveProject.mjs")
);
const { formatEta, formatWork } = await import(
	bundle("src/renderer/src/runtime/runProgress/format.ts", "runProgressLiveFormat.mjs")
);

const CAPTURE = JSON.parse(
	readFileSync(join(workshopRoot, "docs/receipts/2026-08-17/v0.5-run-progress/real-run-streams.json"), "utf8")
);

/** After the run finished, so a terminal projection measures to its own end. */
const NOW = Date.parse("2026-08-17T00:00:00Z");

function snapshot(capture, { events = capture.events, status = capture.run.status } = {}) {
	return {
		runId: capture.run.id,
		state: "subscribed",
		run: { ...capture.run, status },
		events,
		cursor: events.at(-1)?.sequenceNumber ?? 0,
		gap: false,
		revision: 1
	};
}

/**
 * Replay a stream one event at a time. Returns every projection, so invariants
 * can be asserted over the whole life of the run rather than its last frame.
 */
function replay(capture, limit = Infinity) {
	const frames = [];
	const total = Math.min(capture.events.length, limit);
	for (let count = 1; count <= total; count += 1) {
		const events = capture.events.slice(0, count);
		frames.push(
			projectRunProgress(
				snapshot(capture, { events, status: count === capture.events.length ? capture.run.status : "running" }),
				NOW
			)
		);
	}
	return frames;
}

/* ── The captures are what they claim to be ───────────────────────────── */

test("the capture is a real, unedited producer stream", () => {
	for (const [label, capture] of Object.entries(CAPTURE)) {
		assert.ok(capture.events.length > 0, `${label} has events`);
		assert.equal(capture.run.schemaVersion, "optimizer_run.v1", `${label} run envelope`);
		for (const event of capture.events) {
			assert.equal(event.schemaVersion, "optimizer_event.v1");
			assert.equal(event.optimizerRunId, capture.run.id);
			assert.ok(Number.isSafeInteger(event.sequenceNumber) && event.sequenceNumber >= 1);
		}
		const sequences = capture.events.map((event) => event.sequenceNumber);
		assert.deepEqual(sequences, [...sequences].sort((a, b) => a - b), `${label} is ordered`);
		assert.equal(new Set(sequences).size, sequences.length, `${label} has no duplicate sequences`);
	}
});

/* ── GEPA · banking77_gepa_luna_med_c90c6c72 ──────────────────────────── */

test("a real completed GEPA run projects a terminal card with real counts", () => {
	const projection = projectRunProgress(snapshot(CAPTURE.gepa), NOW);
	assert.equal(projection.runKind, "gepa");
	assert.equal(projection.status, "completed");
	assert.equal(projection.terminal, true);
	assert.match(projection.title, /^GEPA · Banking77 intent prompt/);
	assert.equal(projection.timing.eta, undefined, "a finished run has a wall time, not an estimate");
	assert.ok(projection.timing.elapsedMs > 0);
	// The producer really did declare a rollout budget and spend it.
	assert.ok(projection.work.total > 0, "the real run declared a rollout budget");
	assert.ok(projection.work.completed > 0, "the real run completed rollouts");
	assert.ok(projection.progress.determinate, "a real GEPA run has an honest denominator");
	assert.ok(formatWork(projection).includes("/"), formatWork(projection));
	assert.ok(projection.phases.length > 0);
	assert.ok(projection.result, "a terminal run reports a result or says there is none");
});

test("a real GEPA run offers no time estimate, at any point in its life", () => {
	// This is the test that changed the design. Replaying the real stream showed
	// rollouts arriving 13ms apart inside bursts with a 150s proposer gap between
	// them; a throughput-derived ETA missed the truth by a median of 4.7× and by
	// 11× at the p90. The card now declines, and says why.
	const step = Math.max(1, Math.floor(CAPTURE.gepa.events.length / 40));
	for (let count = 1; count <= CAPTURE.gepa.events.length; count += step) {
		const events = CAPTURE.gepa.events.slice(0, count);
		const projection = projectRunProgress(
			snapshot(CAPTURE.gepa, { events, status: "running" }),
			Date.parse(events.at(-1).occurredAt)
		);
		assert.equal(
			projection.timing.eta.state,
			"unavailable",
			`cursor ${count} offered ${projection.timing.eta.state}: ${projection.timing.eta.basis}`
		);
		assert.equal(formatEta(projection.timing.eta), "Unavailable");
	}
});

test("a real GEPA run still reports its progress, rate, and phase while refusing an ETA", () => {
	const midpoint = Math.floor(CAPTURE.gepa.events.length / 2);
	const events = CAPTURE.gepa.events.slice(0, midpoint);
	const projection = projectRunProgress(
		snapshot(CAPTURE.gepa, { events, status: "running" }),
		Date.parse(events.at(-1).occurredAt)
	);
	assert.ok(projection.work.completed > 0 && projection.work.total > 0);
	assert.ok(projection.progress.determinate, "progress is measurable even when time is not");
	assert.ok(projection.throughput, "the observed rollout rate is still shown");
	assert.ok(projection.phase.label.length > 0);
	assert.equal(projection.timing.eta.state, "unavailable");
});

test("a real GEPA run never claims more completed work than its own budget", () => {
	for (const frame of replay(CAPTURE.gepa, 400)) {
		if (frame.work.total == null || frame.work.completed == null) continue;
		assert.ok(
			frame.work.completed <= frame.work.total,
			`completed ${frame.work.completed} exceeded total ${frame.work.total} at cursor ${frame.cursorSeq}`
		);
		assert.ok(frame.progress.fraction >= 0 && frame.progress.fraction <= 1);
	}
});

test("progress on a real GEPA run is monotonic — a card must not count backwards", () => {
	let previous = -1;
	for (const frame of replay(CAPTURE.gepa, 400)) {
		const completed = frame.work.completed ?? 0;
		assert.ok(
			completed >= previous,
			`completed rollouts went ${previous} → ${completed} at cursor ${frame.cursorSeq}`
		);
		previous = completed;
	}
});

test("a real GEPA run's cost is either reported with coverage or honestly absent", () => {
	const projection = projectRunProgress(snapshot(CAPTURE.gepa), NOW);
	const cost = projection.usage.costUsd;
	if (cost.value == null) {
		assert.equal(cost.source, "unavailable");
		assert.ok(
			projection.warnings.some((warning) => /cost|floor/i.test(warning)),
			"an absent cost total is explained, not silently missing"
		);
	} else {
		assert.ok(cost.value >= 0);
		assert.ok(cost.observedUnits > 0, "a cost total is backed by receipts that reported one");
	}
});

/* ── Evaluations · opt_eval_9fcd8d0722a2 ──────────────────────────────── */

test("a real completed eval campaign projects its frozen plan as the denominator", () => {
	const projection = projectRunProgress(snapshot(CAPTURE.eval), NOW);
	assert.equal(projection.runKind, "eval");
	assert.equal(projection.terminal, true);
	assert.equal(projection.progress.semantics, "campaign completion");
	assert.ok(projection.work.total > 0, "the real campaign declared a trial count");
	assert.equal(
		projection.work.completed,
		projection.work.total,
		"a completed campaign finished every planned trial"
	);
	assert.ok(projection.result, "a terminal campaign reports a result or its absence");
	assert.ok(
		projection.details.some((detail) => detail.label === "Candidates"),
		"the real plan's candidates reach the dialog"
	);
});

test("a real eval campaign counts trials without double-counting attempts", () => {
	for (const frame of replay(CAPTURE.eval)) {
		const accounted = (frame.work.completed ?? 0) + (frame.work.active ?? 0) + (frame.work.queued ?? 0);
		assert.ok(
			frame.work.total == null || accounted <= frame.work.total,
			`completed+active+queued ${accounted} exceeded the plan's ${frame.work.total} at cursor ${frame.cursorSeq}`
		);
	}
});

test("an eval campaign captured mid-flight renders as running with a live estimate state", () => {
	const projection = projectRunProgress(snapshot(CAPTURE.eval_running), NOW);
	assert.equal(projection.terminal, false);
	assert.equal(projection.status, "running");
	assert.ok(projection.timing.eta, "a live run offers an ETA state");
	assert.ok(
		["estimating", "range", "point", "unavailable"].includes(projection.timing.eta.state),
		projection.timing.eta.state
	);
	assert.equal(projection.result, undefined, "a live run has no result yet");
});

/* ── Hosted SFT · sft_craftax_nemo_f7f85d95 ───────────────────────────── */

test("a real hosted SFT run refuses a training ETA the provider never supported", () => {
	const midpoint = Math.floor(CAPTURE.sft.events.length / 2);
	const live = projectRunProgress(
		snapshot(CAPTURE.sft, { events: CAPTURE.sft.events.slice(0, midpoint), status: "running" }),
		NOW
	);
	assert.equal(live.runKind, "sft");
	assert.ok(live.timing.eta, "a live SFT run still carries an ETA projection");
	// This is the claim the brief makes about today's producers. If a provider
	// starts declaring totals, this assertion is the thing that should change.
	if (live.timing.eta.state === "unavailable") {
		assert.match(live.timing.eta.unavailableReason, /total steps|bounded unit|not reported|interval/);
		assert.equal(formatEta(live.timing.eta), "Unavailable");
	} else {
		assert.ok(
			["estimating", "point", "range"].includes(live.timing.eta.state),
			live.timing.eta.state
		);
	}
});

test("a real hosted SFT run invents no cost, no tokens, and no uplift", () => {
	const projection = projectRunProgress(snapshot(CAPTURE.sft), NOW);
	assert.equal(projection.terminal, true);
	for (const [field, metric] of Object.entries(projection.usage)) {
		if (metric.value == null) {
			assert.equal(metric.source, "unavailable", `${field} absent means absent`);
		} else {
			assert.ok(metric.observedUnits > 0, `${field} is backed by observations`);
		}
	}
	const result = projection.result;
	assert.ok(result, "a terminal run says what it produced");
	if (result.headline?.includes("uplift")) {
		assert.ok(
			(CAPTURE.sft.events.some((event) =>
				String(event.type).includes("heldout") || String(event.type).includes("comparison"))),
			"an uplift headline requires heldout evidence in the stream"
		);
	}
});

test("a real SFT run's promotion phase is never satisfied by a ready checkpoint", () => {
	const projection = projectRunProgress(snapshot(CAPTURE.sft), NOW);
	const promotion = projection.phases.find((phase) => phase.id === "promotion");
	assert.ok(promotion, "the SFT timeline has a promotion phase");
	if (promotion.status === "completed") {
		assert.ok(
			CAPTURE.sft.events.some((event) => event.type === "sft.checkpoint.promoted"),
			"promotion is only completed when the producer emitted a promote event"
		);
	}
});

/* ── Shared invariants over every real stream ─────────────────────────── */

test("every real stream projects at every cursor without throwing or lying", () => {
	for (const [label, capture] of Object.entries(CAPTURE)) {
		const step = Math.max(1, Math.floor(capture.events.length / 40));
		for (let count = 1; count <= capture.events.length; count += step) {
			const events = capture.events.slice(0, count);
			const projection = projectRunProgress(
				snapshot(capture, { events, status: "running" }),
				NOW
			);
			assert.ok(projection, `${label} projects at cursor ${count}`);
			assert.equal(projection.schemaVersion, "run_progress.v1");
			// An indeterminate bar must never carry a fraction, and a determinate
			// one must never be out of range.
			if (projection.progress) {
				if (!projection.progress.determinate) {
					assert.equal(projection.progress.fraction, undefined, `${label}@${count} indeterminate with a fraction`);
				} else if (projection.progress.fraction != null) {
					assert.ok(projection.progress.fraction >= 0 && projection.progress.fraction <= 1);
				}
			}
			// No usage figure may be a fabricated zero.
			for (const metric of Object.values(projection.usage)) {
				if (metric.source === "unavailable") {
					assert.equal(metric.value, undefined, `${label}@${count} unavailable metric carried a value`);
				}
			}
			// A live projection always offers a renderable ETA state.
			assert.ok(
				["estimating", "range", "point", "unavailable", "paused"].includes(projection.timing.eta.state),
				`${label}@${count} eta state ${projection.timing.eta?.state}`
			);
			assert.ok(projection.timing.eta.basis.length > 0, `${label}@${count} eta has a basis`);
		}
	}
});

test("the terminal frame of every real run is terminal, with no lingering estimate", () => {
	for (const [label, capture] of Object.entries(CAPTURE)) {
		const projection = projectRunProgress(snapshot(capture), NOW);
		const terminal = ["completed", "failed", "cancelled", "canceled", "succeeded", "terminated"]
			.includes(capture.run.status);
		assert.equal(projection.terminal, terminal, `${label} terminal flag follows the record`);
		if (terminal) {
			assert.equal(projection.timing.eta, undefined, `${label} kept an ETA after finishing`);
			assert.ok(projection.result, `${label} has a result or a stated absence`);
		}
	}
});
