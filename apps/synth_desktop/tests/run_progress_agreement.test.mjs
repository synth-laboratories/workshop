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

const { projectRunProgress, progressAgreement, visualProgressFacts, splitSnapshotEvents } = await import(
	bundle("src/renderer/src/runtime/runProgress/project.ts", "runProgressProjectAgree.mjs")
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

test("card and right-pane agreement views are identical for a live GEPA run", () => {
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
	assert.deepEqual(visualProgressFacts("gepa", visual, card), progressAgreement(card));
	assert.equal(progressAgreement(card).completed, 4);
	assert.equal(progressAgreement(card).total, 20);
	assert.equal(progressAgreement(card).costUsd, 0.2);
	assert.equal(lanes.enrichmentEvents.length, 0);
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
