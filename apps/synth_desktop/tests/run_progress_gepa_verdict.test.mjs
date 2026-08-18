/**
 * What a finished GEPA search is allowed to claim.
 *
 * The defect these tests pin: a packaged Banking77 GEPA run spent 320 rollouts
 * on ten candidates, every proposal lost at the minibatch gate, the seed was
 * retained, and the card reported `Heldout 0.600`. That number is true and
 * says nothing — it is the *incumbent's* score, unchanged by the search — but
 * as a headline it reads exactly like a result the run produced.
 *
 * The rule: a heldout number never stands alone on a terminal search. The
 * verdict comes from the run's sealed terminal manifest, computed once in the
 * service from the durable event log, and an uplift is only ever shown with the
 * sample count that backs it.
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
	bundle("src/renderer/src/runtime/runProgress/project.ts", "runProgressGepaVerdictProject.mjs")
);

const NOW = Date.UTC(2026, 7, 17, 22, 55, 0);
const at = (minute, second) =>
	new Date(Date.UTC(2026, 7, 17, 22, minute, second)).toISOString();
const RUN_ID = "banking77_gepa_luna_med_959b2d7c";
const SEED = "gepa_1c284a9e221e";
const CHILD = "gepa_f5fb431d3f4f";

/** A sealed manifest as `terminal.rs` writes one for a GEPA run. */
function manifest(selection) {
	return {
		schemaVersion: "optimizer_terminal_manifest.v1",
		terminalStatus: "completed",
		terminalCursor: 1030,
		work: { planned: null, succeeded: 320, failed: 0, skipped: null, unit: "rollouts" },
		selection
	};
}

function gepaRun(terminalManifest) {
	return {
		id: RUN_ID,
		algorithmId: "gepa",
		status: "completed",
		source: "local",
		objective: "Banking77 intent prompt",
		sessionRef: "sess_gepa",
		createdAt: at(50, 20),
		startedAt: at(50, 20),
		finishedAt: at(53, 26),
		cursorSeq: 1030,
		capabilities: { cancel: true, pause: true, resume: true },
		visualRefs: [],
		summary: { recipeId: "gepa.banking77.luna.v1", terminalManifest },
		usage: {}
	};
}

const base = { optimizerRunId: RUN_ID, algorithmId: "gepa" };

/** Enough real GEPA history for the shared projection to build a `GepaState`. */
function history() {
	let seq = 0;
	const events = [
		{ ...base, sequenceNumber: ++seq, type: "optimizer.run.started", occurredAt: at(50, 20), delta: { status: "running" } },
		{
			...base,
			sequenceNumber: ++seq,
			type: "candidate.registered",
			occurredAt: at(50, 21),
			delta: { candidate_id: SEED, source: "seed", status: "registered" }
		}
	];
	for (let index = 0; index < 4; index += 1) {
		events.push({
			...base,
			sequenceNumber: ++seq,
			type: "optimizer.evaluation_result.received",
			occurredAt: at(51, index),
			delta: {
				candidate_id: SEED,
				stage: "seed_full_train",
				evaluation_id: `${SEED}:seed_full_train:${index}`,
				example_id: `train:${index}`,
				reward: 1,
				active_workers: 8,
				queued_rollouts: 0
			}
		});
	}
	events.push({
		...base,
		sequenceNumber: ++seq,
		type: "candidate.registered",
		occurredAt: at(52, 0),
		delta: { candidate_id: CHILD, parent_id: SEED, generation: 0, proposal_index: 0, source: "reflector:parent_variation" }
	});
	events.push({
		...base,
		sequenceNumber: ++seq,
		type: "heldout.completed",
		occurredAt: at(53, 20),
		delta: { candidate_id: SEED, heldout_reward: 0.6, train_reward: 0.765 }
	});
	events.push({
		...base,
		sequenceNumber: ++seq,
		type: "optimizer.run.completed",
		occurredAt: at(53, 26),
		delta: { status: "completed" }
	});
	return events;
}

function snapshot(run, events) {
	return {
		runId: run.id,
		state: "terminal",
		run,
		events,
		cursor: events.at(-1).sequenceNumber,
		gap: false,
		revision: 1
	};
}

test("a retained seed is reported as no improvement, not as a heldout score", () => {
	const run = gepaRun(
		manifest({
			seedCandidateId: SEED,
			selectedCandidateId: SEED,
			accepted: false,
			verdict: "no_measured_improvement",
			verdictDetail: {
				reason: "the seed candidate was retained; no proposal beat it",
				baselineHeldout: 0.6,
				baselineHeldoutSamples: 50,
				selectedHeldout: 0.6,
				selectedHeldoutSamples: 50,
				upliftAbsolute: null,
				proposalsRegistered: 10
			}
		})
	);
	const projection = projectRunProgress(snapshot(run, history()), NOW);
	assert.equal(projection.result.verdict, "no_measured_improvement");
	assert.match(projection.result.headline, /No improvement/);
	assert.match(projection.result.headline, /0\.600/, "the number is still shown, just not alone");
	assert.equal(
		projection.result.verdictDetail,
		"the seed candidate was retained; no proposal beat it",
		"with no uplift to state, the card states why there is none"
	);
});

test("an uplift is only shown alongside the samples that back it", () => {
	const run = gepaRun(
		manifest({
			seedCandidateId: SEED,
			selectedCandidateId: CHILD,
			accepted: true,
			verdict: "measured_improvement",
			verdictDetail: {
				baselineCandidateId: SEED,
				baselineHeldout: 0.6,
				baselineHeldoutSamples: 50,
				selectedCandidateId: CHILD,
				selectedHeldout: 0.72,
				selectedHeldoutSamples: 50,
				upliftAbsolute: 0.12
			}
		})
	);
	const projection = projectRunProgress(snapshot(run, history()), NOW);
	assert.equal(projection.result.verdict, "measured_improvement");
	assert.match(projection.result.headline, /Improved/);
	assert.equal(projection.result.verdictDetail, "+0.120 over baseline on 50 heldout samples");
});

test("a search with no sealed manifest claims no verdict at all", () => {
	// Nothing sealed means nothing settled. The card falls back to the bare
	// measurement rather than inventing an outcome for it.
	const projection = projectRunProgress(snapshot(gepaRun(undefined), history()), NOW);
	assert.equal(projection.result.verdict, undefined);
	assert.equal(projection.result.verdictDetail, undefined);
	assert.match(projection.result.headline, /Heldout 0\.600/);
});

test("a failed search carries its verdict too", () => {
	// Craftax GEPA's real ending: the seed evaluated, the proposer call timed
	// out. `failed` is a different statement from "no improvement".
	const run = gepaRun(
		manifest({
			seedCandidateId: SEED,
			selectedCandidateId: null,
			accepted: null,
			verdict: "failed",
			verdictDetail: {
				reason: "the search did not finish",
				failure: { failure: { failure_type: "timeout", retryable: true } }
			}
		})
	);
	run.status = "failed";
	const events = history();
	events.at(-1).delta = { status: "failed" };
	events.at(-1).type = "optimizer.run.failed";
	const projection = projectRunProgress(snapshot(run, events), NOW);
	assert.equal(projection.result.verdict, "failed");
	assert.equal(projection.result.verdictDetail, "the search did not finish");
});
