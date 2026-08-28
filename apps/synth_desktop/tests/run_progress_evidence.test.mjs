/**
 * Progress evidence: what the card is allowed to say when it has none.
 *
 * The defect these tests pin: a Banking77 baseline eval ran ten of ten
 * rollouts in ~1.5s, and the transcript card reported `0 trials`. The event
 * history had been lost, so the projection reduced over nothing — and an
 * adapter that turned "nothing" into `completed: 0` presented a total loss of
 * evidence as a measured result.
 *
 * The rule: a count is rendered only when something proves it. Otherwise the
 * card says "Progress unavailable" and carries a diagnostic naming what is
 * missing. Once a run seals a terminal manifest, its counts come from there and
 * a later poll cannot restate them.
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

const { projectRunProgress } = await import(
	bundle("src/renderer/src/runtime/runProgress/project.ts", "runProgressEvidenceProject.mjs")
);
const { formatWork, progressUnavailableLine, statusLabel } = await import(
	bundle("src/renderer/src/runtime/runProgress/format.ts", "runProgressEvidenceFormat.mjs")
);
const { isTerminalRunStatus } = await import(
	bundle("src/renderer/src/runtime/runProgress/types.ts", "runProgressEvidenceTypes.mjs")
);

test("every closed optimizer status disables terminal lifecycle controls", () => {
	for (const status of [
		"completed",
		"failed",
		"cancelled",
		"degraded",
		"failed_evidence",
		"infrastructure_lost",
		"cap_reached"
	]) {
		assert.equal(isTerminalRunStatus(status), true, status);
	}
	for (const status of ["queued", "starting", "running", "paused", "cancelling"]) {
		assert.equal(isTerminalRunStatus(status), false, status);
	}
});

const NOW = Date.UTC(2026, 7, 17, 21, 37, 0);
const at = (second) => new Date(Date.UTC(2026, 7, 17, 21, 36, second)).toISOString();
const RUN_ID = "opt_eval_banking77_81d51f81b59f";

function evalRun(overrides = {}) {
	return {
		id: RUN_ID,
		algorithmId: "eval",
		status: "completed",
		source: "local",
		objective: "Banking77 baseline eval",
		sessionRef: "sess_banking77_eval",
		createdAt: at(56),
		startedAt: at(56),
		finishedAt: at(57),
		cursorSeq: 1,
		capabilities: { cancel: true, pause: true, resume: true },
		visualRefs: [{ kind: "visual", id: "vis_7fc27280fde04974bd0e88cc1cc67ee5" }],
		summary: { recipeId: "eval.banking77.baseline.v1", meanReward: 1 },
		usage: {},
		...overrides
	};
}

function snapshot(run, events, overrides = {}) {
	return {
		runId: run.id,
		state: "terminal",
		run,
		events,
		cursor: events.at(-1)?.sequenceNumber ?? 0,
		gap: false,
		revision: 1,
		...overrides
	};
}

const base = { optimizerRunId: RUN_ID, algorithmId: "eval" };

function fullEvalHistory() {
	let seq = 0;
	const events = [
		{ ...base, sequenceNumber: ++seq, type: "optimizer.run.started", occurredAt: at(56), delta: { status: "running" } },
		{
			...base,
			sequenceNumber: ++seq,
			type: "eval.run.planned",
			occurredAt: at(56),
			snapshot: {
				planned_trials: 10,
				parallelism: 10,
				candidates: [{ id: "banking77_gpt_4_1_nano", label: "banking77_gpt_4_1_nano" }]
			}
		}
	];
	for (let index = 0; index < 10; index += 1) {
		events.push({
			...base,
			sequenceNumber: ++seq,
			type: "eval.trial.queued",
			occurredAt: at(56),
			delta: {
				trial_id: `trial:banking77:${index}`,
				candidate_id: "banking77_gpt_4_1_nano",
				seed: index,
				scenario: "banking77",
				stage: "screen"
			}
		});
	}
	for (let index = 0; index < 10; index += 1) {
		events.push({
			...base,
			sequenceNumber: ++seq,
			type: "eval.trial.terminal",
			occurredAt: at(57),
			item: {
				kind: "trial",
				id: `trial:banking77:${index}`,
				status: "evaluated",
				valid: true,
				candidateId: "banking77_gpt_4_1_nano",
				stage: "screen",
				seed: index,
				scenario: "banking77",
				metrics: { reward: 1 }
			},
			usageDelta: { prompt_tokens: 624, completion_tokens: 4, rollouts: 1 }
		});
	}
	events.push({
		...base,
		sequenceNumber: ++seq,
		type: "optimizer.run.completed",
		occurredAt: at(57),
		delta: { status: "completed" }
	});
	return events;
}

test("a successful campaign with no durable history says so instead of reporting zero", () => {
	// The packaged reproduction exactly: run record says completed, the log has
	// only `optimizer.run.started`.
	const events = [
		{ ...base, sequenceNumber: 1, type: "optimizer.run.started", occurredAt: at(56), delta: { status: "running" } }
	];
	const projection = projectRunProgress(snapshot(evalRun(), events), NOW);
	assert.equal(projection.work.completed, undefined, "no evidence must not become a count");
	assert.equal(formatWork(projection), null, "the work line is withheld, not zeroed");
	assert.equal(projection.evidence.state, "unavailable");
	const line = progressUnavailableLine(projection);
	assert.match(line, /Progress unavailable/);
	assert.match(projection.evidence.diagnostic, /cursor/, "the diagnostic names where to look");
	assert.ok(
		projection.warnings.some((warning) => /no progress evidence/.test(warning)),
		"the missing evidence is surfaced as a warning"
	);
});

test("a campaign with its whole history reports the trials it proved", () => {
	const events = fullEvalHistory();
	const run = evalRun({ cursorSeq: events.length });
	const projection = projectRunProgress(snapshot(run, events), NOW);
	assert.equal(projection.evidence.state, "present");
	assert.equal(projection.work.completed, 10);
	assert.equal(projection.work.total, 10);
	assert.equal(formatWork(projection), "10 / 10 trials");
	assert.equal(progressUnavailableLine(projection), null);
});

test("terminal counts are frozen at the sealed manifest, not at a later cursor", () => {
	// A post-terminal reconcile has advanced the run's cursor past the manifest.
	// The card must still report what the run ended with.
	const events = fullEvalHistory();
	const run = evalRun({
		cursorSeq: events.length + 5,
		summary: {
			recipeId: "eval.banking77.baseline.v1",
			meanReward: 1,
			terminalManifest: {
				terminalStatus: "completed",
				terminalCursor: events.length,
				work: { planned: 10, succeeded: 10, failed: 0, skipped: 0, unit: "trials" }
			}
		}
	});
	// Only the first half of the history has been read back so far.
	const projection = projectRunProgress(snapshot(run, events.slice(0, 6)), NOW);
	assert.equal(projection.work.completed, 10, "the manifest, not the partial replay, is authority");
	assert.equal(projection.work.total, 10);
});

test("a manifest that measured nothing still refuses to invent zeroes", () => {
	const run = evalRun({
		summary: {
			recipeId: "eval.banking77.baseline.v1",
			terminalManifest: {
				terminalStatus: "completed",
				terminalCursor: 1,
				work: { planned: null, succeeded: null, failed: null, skipped: null, unit: "trials" }
			}
		}
	});
	const events = [
		{ ...base, sequenceNumber: 1, type: "optimizer.run.started", occurredAt: at(56), delta: { status: "running" } }
	];
	const projection = projectRunProgress(snapshot(run, events), NOW);
	assert.equal(projection.work.completed, undefined);
	assert.equal(projection.evidence.state, "unavailable");
});

test("a run whose evidence lane failed reads as degraded, with the stage that failed", () => {
	const run = evalRun({
		status: "degraded",
		summary: {
			recipeId: "eval.banking77.baseline.v1",
			evidenceDegradation: {
				stage: "progress_projection",
				reason: "visual registry refused the update",
				retryable: true
			}
		}
	});
	const projection = projectRunProgress(snapshot(run, []), NOW);
	assert.equal(projection.status, "degraded");
	assert.equal(projection.terminal, true, "a degraded run is finished, not still working");
	assert.equal(statusLabel(projection.status), "Evidence unavailable");
	assert.equal(projection.evidence.state, "degraded");
	assert.match(projection.evidence.diagnostic, /progress_projection/);
	assert.equal(projection.work.completed, undefined);
});

test("an incomplete replay is a floor, not a total, and says which", () => {
	const events = fullEvalHistory().slice(0, 8);
	const run = evalRun({ status: "running", finishedAt: null, cursorSeq: 23 });
	const projection = projectRunProgress(snapshot(run, events, { gap: true, state: "stale" }), NOW);
	assert.equal(projection.stale, true);
	assert.ok(
		projection.warnings.some((warning) => /floor/.test(warning)),
		"a holed history is named as a floor"
	);
});

test("a live run that has not reported yet is not an evidence failure", () => {
	const run = evalRun({ status: "running", finishedAt: null, cursorSeq: 1 });
	const events = [
		{ ...base, sequenceNumber: 1, type: "optimizer.run.started", occurredAt: at(56), delta: { status: "running" } }
	];
	const projection = projectRunProgress(snapshot(run, events, { state: "subscribed" }), NOW);
	assert.equal(projection.evidence.state, "present", "not yet measured is not the same as lost");
	assert.equal(projection.work.completed, undefined, "and it still reports no count");
});
