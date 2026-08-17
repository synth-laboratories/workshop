/**
 * Terminal cursor freeze and the enrichment lane — Workshop projection side
 * of Optimizers O-5 / O-11.
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

const { splitEventLanes } = await import(
	bundle("src/renderer/src/runtime/runProgress/lanes.ts", "runProgressLanes.mjs")
);
const { projectRunProgress, progressAgreement } = await import(
	bundle("src/renderer/src/runtime/runProgress/project.ts", "runProgressProjectLanes.mjs")
);

const NOW = Date.UTC(2026, 7, 17, 12, 30, 0);
const at = (minute, second = 0) => new Date(Date.UTC(2026, 7, 17, 12, minute, second)).toISOString();

function event(sequence, type, extra = {}) {
	return {
		schemaVersion: "optimizer_event.v1",
		eventId: `e${sequence}`,
		type,
		sequenceNumber: sequence,
		occurredAt: extra.occurredAt ?? at(0, sequence),
		optimizerRunId: "run-gepa",
		algorithmId: "gepa",
		...extra
	};
}

const terminalEvents = [
	event(1, "gepa.run.started"),
	event(2, "optimizer.limit.estimate_updated", {
		delta: { limits: [{ kind: "total_rollouts", max: 10, spent: 2, hard: true }] }
	}),
	event(3, "optimizer.evaluation_result.received", {
		occurredAt: at(1),
		delta: { rollout_id: "r0" },
		usageDelta: { cost_usd: 0.1, prompt_tokens: 20, completion_tokens: 5, rollouts: 1 }
	}),
	event(4, "optimizer.evaluation_result.received", {
		occurredAt: at(1, 10),
		delta: { rollout_id: "r1" },
		usageDelta: { cost_usd: 0.1, prompt_tokens: 20, completion_tokens: 5, rollouts: 1 }
	}),
	event(5, "gepa.run.finished", { occurredAt: at(2), delta: { message: "search finished" } })
];

function snapshot(run, events) {
	return {
		runId: run.id,
		state: run.status === "completed" ? "terminal" : "subscribed",
		run,
		events,
		cursor: events.at(-1)?.sequenceNumber ?? 0,
		gap: false,
		revision: 1
	};
}

function gepaRun(overrides = {}) {
	return {
		id: "run-gepa",
		algorithmId: "gepa",
		status: "completed",
		objective: "Banking77",
		createdAt: at(0),
		startedAt: at(0),
		finishedAt: at(2),
		cursorSeq: 5,
		summary: { terminalCursor: 5 },
		capabilities: {},
		usage: {},
		...overrides
	};
}

test("declared terminalCursor freezes the authoritative lane", () => {
	const lanes = splitEventLanes(gepaRun(), [
		...terminalEvents,
		event(6, "optimizer.usage.reconciled", {
			lane: "enrichment",
			occurredAt: at(3),
			usageDelta: { cost_usd: 9.99, prompt_tokens: 999, completion_tokens: 999, rollouts: 40 }
		})
	]);
	assert.equal(lanes.terminalCursor, 5);
	assert.equal(lanes.enrichmentCursor, 6);
	assert.equal(lanes.terminalEvents.length, 5);
	assert.equal(lanes.enrichmentEvents.length, 1);
});

test("late enrichment cannot rewrite terminal usage or result", () => {
	const before = projectRunProgress(snapshot(gepaRun(), terminalEvents), NOW);
	const after = projectRunProgress(
		snapshot(gepaRun({ cursorSeq: 7 }), [
			...terminalEvents,
			event(6, "optimizer.usage.reconciled", {
				lane: "enrichment",
				occurredAt: at(3),
				usageDelta: { cost_usd: 9.99, prompt_tokens: 999, completion_tokens: 999, rollouts: 40 }
			}),
			event(7, "viewer.ready", { lane: "enrichment", occurredAt: at(3, 5) })
		]),
		NOW
	);
	assert.deepEqual(progressAgreement(before), progressAgreement(after));
	assert.equal(before.usage.costUsd.value, after.usage.costUsd.value);
	assert.equal(after.enrichmentCursor, 7);
	assert.equal(after.enrichmentEventCount, 2);
	assert.equal(after.terminalCursor, 5);
	assert.notEqual(after.usage.costUsd.value, 9.99);
});

test("untagged events after finishedAt still ride the enrichment lane", () => {
	const lanes = splitEventLanes(gepaRun({ summary: {} }), [
		...terminalEvents,
		event(6, "optimizer.usage.reconciled", {
			occurredAt: at(4),
			usageDelta: { cost_usd: 4.2 }
		})
	]);
	assert.equal(lanes.terminalCursor, 5);
	assert.equal(lanes.enrichmentEvents.map((event) => event.sequenceNumber).join(","), "6");
});
