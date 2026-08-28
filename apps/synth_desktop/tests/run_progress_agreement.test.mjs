/**
 * Compact card, expanded dialog, and right-pane visual must read the same
 * phase, progress, usage, and terminal result from one projection.
 */
import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
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

const { projectRunProgress, progressAgreement, splitSnapshotEvents } = await import(
	bundle("src/renderer/src/runtime/runProgress/project.ts", "runProgressProjectAgree.mjs")
);
const { canonicalEvalState, evalAggregateFromSurface } = await import(
	bundle("src/renderer/src/runtime/evalAggregate.ts", "evalAggregateDesktop.mjs")
);
const { evalAggregateV1 } = await import(
	bundle("../../visuals/runtime/evalAggregate.ts", "evalAggregateVisual.mjs")
);
const { projectAtCursor } = await import(
	bundle(
		"../../visuals/families/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts",
		"runProgressVisualProject.mjs"
	)
);

const NOW = Date.UTC(2026, 7, 17, 12, 30, 0);
const at = (minute, second = 0) => new Date(Date.UTC(2026, 7, 17, 12, minute, second)).toISOString();

function snapshot(run, events, state = "subscribed") {
	return {
		runId: run.id,
		state,
		run,
		events,
		cursor: events.at(-1)?.sequenceNumber ?? 0,
		gap: false,
		revision: 1
	};
}

function gepaEvents() {
	const base = { optimizerRunId: "agree-gepa", algorithmId: "gepa" };
	let seq = 0;
	const events = [
		{ ...base, sequenceNumber: ++seq, type: "gepa.run.started", occurredAt: at(0), delta: { message: "started" } },
		{
			...base, sequenceNumber: ++seq, type: "optimizer.limit.estimate_updated", occurredAt: at(0, 10),
			delta: { limits: [{ kind: "total_rollouts", max: 20, spent: 4, hard: true }] }
		}
	];
	for (let index = 0; index < 4; index += 1) {
		events.push({
			...base,
			sequenceNumber: ++seq,
			type: "optimizer.evaluation_result.received",
			occurredAt: at(1, index * 10),
			delta: { rollout_id: `r${index}`, reward: 0.5 },
			usageDelta: { cost_usd: 0.05, prompt_tokens: 100, completion_tokens: 20, rollouts: 1 }
		});
	}
	return events;
}

test("card and right-pane reducers agree on real live GEPA facts", () => {
	const run = {
		id: "agree-gepa",
		algorithmId: "gepa",
		status: "running",
		objective: "Banking77",
		startedAt: at(0),
		cursorSeq: 6,
		capabilities: {},
		usage: {}
	};
	const events = gepaEvents();
	const card = projectRunProgress(snapshot(run, events), NOW);
	const lanes = splitSnapshotEvents(run, events);
	const visual = projectAtCursor({
		id: run.id,
		algorithmId: run.algorithmId,
		status: run.status,
		cursorSeq: run.cursorSeq,
		usage: run.usage
	}, lanes.terminalEvents);
	assert.equal(visual.summary.status, card.status);
	assert.equal(visual.gepa.rolloutsCompleted, card.work.completed);
	assert.equal(visual.gepa.limits[0].max, card.work.total);
	assert.equal(visual.usage.costUsd, card.usage.costUsd.value);
	assert.equal(progressAgreement(card).completed, 4);
	assert.equal(progressAgreement(card).total, 20);
	assert.equal(progressAgreement(card).costUsd, 0.2);
	assert.equal(lanes.enrichmentEvents.length, 0);
});

test("chat, experiment, inspector, and workbench consume one eval aggregate revision", () => {
	const aggregate = {
		schemaVersion: "eval.aggregate.v1",
		runId: "agree-eval",
		asOfSequence: 41,
		projectionRevision: 41,
		lifecycle: "terminal",
		work: { planned: 2, succeeded: 1, failed: 1, cancelled: 0, unit: "trials", fixedDenominator: true },
		evidence: { completeness: "partial", reason: "one evaluator measurement missing", refs: [] },
		selection: "promotion_not_applicable",
		meanReward: 0.75,
		scoredTrials: 1,
		evaluatorEvidence: 1,
		traceCount: 1,
		evidenceRefCount: 2
	};
	const runViewV2 = {
		algorithm: "eval",
		header: { runId: "agree-eval", asOfSequence: 41, projectionRevision: 41 },
		projection: {},
		aggregate,
		result: null
	};
	const experimentBinding = {
		aggregate: structuredClone(aggregate),
		// A failed raw row deliberately carries a large number. Surfaces must
		// never recalculate the aggregate from it.
		rollouts: [{ status: "failed", reward: 999 }]
	};

	const chat = canonicalEvalState(runViewV2, "agree-eval").aggregate;
	const inspector = evalAggregateFromSurface(runViewV2, "agree-eval");
	const experiment = evalAggregateFromSurface(experimentBinding, "agree-eval");
	const workbench = evalAggregateV1(runViewV2.aggregate, "agree-eval");

	assert.deepEqual(inspector, chat);
	assert.deepEqual(experiment, chat);
	assert.deepEqual(workbench, chat);
	assert.equal(experiment.meanReward, 0.75);
	assert.equal(experiment.scoredTrials, 1);
});

test("eval surfaces fail closed on identity or revision disagreement", () => {
	const mismatched = {
		algorithm: "eval",
		header: { runId: "agree-eval", asOfSequence: 12, projectionRevision: 12 },
		projection: {},
		aggregate: {
			schemaVersion: "eval.aggregate.v1",
			runId: "agree-eval",
			asOfSequence: 13,
			projectionRevision: 13
		},
		result: null
	};
	assert.throws(
		() => canonicalEvalState(mismatched, "agree-eval"),
		/eval aggregate revision does not match/
	);
	assert.throws(
		() => evalAggregateFromSurface(mismatched.aggregate, "another-run"),
		/does not carry a revisioned aggregate/
	);
});

test("a disconnect overlays Interrupted on both surfaces without dropping counts", () => {
	const run = {
		id: "agree-gepa",
		algorithmId: "gepa",
		status: "running",
		objective: "Banking77",
		startedAt: at(0),
		cursorSeq: 6,
		capabilities: {},
		usage: {}
	};
	const live = projectRunProgress(snapshot(run, gepaEvents()), NOW);
	const interrupted = projectRunProgress(
		snapshot(run, gepaEvents(), "interrupted"),
		NOW
	);
	assert.equal(interrupted.status, "interrupted");
	assert.equal(interrupted.work.completed, live.work.completed);
	assert.equal(interrupted.usage.costUsd.value, live.usage.costUsd.value);
	assert.equal(interrupted.timing.eta, undefined);
	assert.deepEqual(
		{ ...progressAgreement(interrupted), status: "running" },
		{ ...progressAgreement(live), status: "running" }
	);
});
