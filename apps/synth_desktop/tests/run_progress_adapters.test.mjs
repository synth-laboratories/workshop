/**
 * `run_progress.v1` adapters over the three workflows.
 *
 * The event vocabularies here are condensed from real runs — same types, same
 * field names — so the assertions are about the adapters, not about invented
 * producers. Each workflow is checked for the same four things:
 *
 *   1. the denominator is the run's own bounded work, never a quality score;
 *   2. missing telemetry stays missing;
 *   3. the ETA refuses when the evidence does not support one;
 *   4. a terminal run reports a result, or says honestly that there isn't one.
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
		// The renderer resolves family internals through the same alias Vite and
		// tsconfig use; esbuild needs it spelled out.
		alias: { "@synth/visual-templates": join(appRoot, "../../visuals/families") },
		outfile
	});
	return pathToFileURL(outfile).href;
}

const { projectRunProgress, runKindOf } = await import(
	bundle("src/renderer/src/runtime/runProgress/project.ts", "runProgressProject.mjs")
);
const { formatEta, formatWork, progressAriaText } = await import(
	bundle("src/renderer/src/runtime/runProgress/format.ts", "runProgressFormat2.mjs")
);
const { costSummary, metricExplanation } = await import(
	bundle("src/renderer/src/runtime/runProgress/usage.ts", "runProgressUsage.mjs")
);

const NOW = Date.UTC(2026, 7, 17, 12, 30, 0);
const at = (minute, second = 0) => new Date(Date.UTC(2026, 7, 17, 12, minute, second)).toISOString();

function snapshot(run, events, overrides = {}) {
	return {
		runId: run.id,
		state: "subscribed",
		run,
		events,
		cursor: events.at(-1)?.sequenceNumber ?? 0,
		gap: false,
		revision: 1,
		...overrides
	};
}

/* ── GEPA ─────────────────────────────────────────────────────────────── */

function gepaRun(overrides = {}) {
	return {
		id: "banking77_gepa_sol_med_45856f25",
		algorithmId: "gepa",
		status: "running",
		source: "local",
		objective: "Banking77",
		sessionRef: "sess-1",
		createdAt: at(0),
		startedAt: at(0),
		cursorSeq: 99,
		capabilities: { cancel: true, pause: true, resume: true, streamEvents: true },
		visualRefs: [{ kind: "visual", id: "visual-banking77" }],
		usage: {},
		...overrides
	};
}

function gepaEvents({ completed = 8, withCost = true, intervalSeconds = 30 } = {}) {
	const base = { optimizerRunId: "banking77_gepa_sol_med_45856f25", algorithmId: "gepa" };
	let seq = 0;
	const events = [
		{ ...base, sequenceNumber: ++seq, type: "gepa.run.started", occurredAt: at(0), delta: { state: "initializing", message: "GEPA run started" } },
		{ ...base, sequenceNumber: ++seq, type: "candidate.registered", occurredAt: at(0, 6), delta: { candidate_id: "gepa_seed", source: "seed", status: "registered" } },
		{
			...base, sequenceNumber: ++seq, type: "optimizer.limit.estimate_updated", occurredAt: at(0, 15),
			delta: {
				limits: [
					{ kind: "total_rollouts", max: 100, spent: completed, reserved: 0, hard: true },
					{ kind: "proposer_calls", max: 8, spent: 2, hard: true }
				],
				nearest_limit: { kind: "total_rollouts", max: 100, spent: completed }
			}
		},
		{
			...base, sequenceNumber: ++seq, type: "optimizer.state.transitioned", occurredAt: at(1),
			delta: { from: "ready", to: "rollout_running", trigger: "rollouts_started", details: { candidate_id: "gepa_seed", stage: "candidate_minibatch", rollout_count: 20 } }
		},
		{
			...base, sequenceNumber: ++seq, type: "optimizer.rollout_queue.updated", occurredAt: at(1, 5),
			delta: { active_workers: 4, semaphore_size: 4, queued_rollouts: 7 }
		}
	];
	for (let index = 0; index < completed; index += 1) {
		const second = index * intervalSeconds;
		events.push({
			...base,
			sequenceNumber: ++seq,
			type: "optimizer.evaluation_result.received",
			occurredAt: at(2 + Math.floor(second / 60), second % 60),
			delta: {
				candidate_id: "gepa_seed",
				rollout_id: `rollout_${index}`,
				stage: "candidate_minibatch",
				example_id: `train:${index}`,
				reward: 0.8
			},
			usageDelta: withCost
				? { cost_usd: 0.05, prompt_tokens: 400, completion_tokens: 120, rollouts: 1 }
				: { prompt_tokens: 400, completion_tokens: 120, rollouts: 1 }
		});
	}
	return events;
}

test("GEPA: the bar is the rollout budget, and its semantics say so", () => {
	const projection = projectRunProgress(snapshot(gepaRun(), gepaEvents()), NOW);
	assert.equal(projection.runKind, "gepa");
	assert.equal(projection.schemaVersion, "run_progress.v1");
	assert.equal(projection.work.total, 100);
	assert.equal(projection.work.completed, 8);
	assert.equal(projection.work.unit, "rollouts");
	assert.equal(projection.progress.determinate, true);
	assert.equal(projection.progress.semantics, "rollout budget spent");
	assert.equal(formatWork(projection), "8 / 100 rollouts");
	assert.match(progressAriaText(projection), /8% of rollout budget spent/);
});

test("GEPA: an incumbent score is a detail, never progress", () => {
	const events = [
		...gepaEvents(),
		{
			optimizerRunId: "banking77_gepa_sol_med_45856f25", algorithmId: "gepa",
			sequenceNumber: 200, type: "frontier.updated", occurredAt: at(6),
			delta: { generation: 1, best_candidate_id: "gepa_gen1_0", best_train_reward: 0.93, reason: "accepted" }
		}
	];
	const projection = projectRunProgress(snapshot(gepaRun(), events), NOW);
	assert.equal(projection.progress.fraction, 0.08, "a 0.93 reward must not read as 93% complete");
	assert.ok(projection.details.some((detail) => detail.label === "Best train reward"));
	assert.ok(
		projection.details.find((detail) => detail.label === "Best train reward").note.includes("not heldout"),
		"train evidence must be labelled as train evidence"
	);
});

test("GEPA: concurrency and queue depth reach the card's one throughput line", () => {
	const projection = projectRunProgress(snapshot(gepaRun(), gepaEvents()), NOW);
	assert.equal(projection.work.active, 4);
	assert.equal(projection.work.queued, 7);
	assert.ok(projection.throughput, "a run with timed completions reports throughput");
	assert.match(projection.throughput.label, /rollouts\/min/);
});

test("GEPA: a run whose rollouts reported no cost says unavailable, never $0.00", () => {
	const projection = projectRunProgress(snapshot(gepaRun(), gepaEvents({ withCost: false })), NOW);
	assert.equal(projection.usage.costUsd.value, undefined);
	assert.match(costSummary(projection.usage.costUsd), /^Cost unavailable/);
	assert.ok(projection.usage.promptTokens.value > 0, "token telemetry that was reported still shows");
});

test("GEPA: coverage is how much of the planned work has reported", () => {
	const projection = projectRunProgress(snapshot(gepaRun(), gepaEvents({ completed: 74 })), NOW);
	assert.equal(projection.usage.costUsd.observedUnits, 74);
	assert.equal(projection.usage.costUsd.expectedUnits, 100);
	assert.equal(projection.usage.costUsd.coverage, 0.74);
	assert.match(metricExplanation(projection.usage.costUsd, "rollout"), /74 of 100 rollouts reported it/);
});

test("GEPA: the ETA warms, then settles, on rollout evidence alone", () => {
	const warming = projectRunProgress(snapshot(gepaRun(), gepaEvents({ completed: 1 })), NOW);
	assert.equal(warming.timing.eta.state, "estimating");
	const settled = projectRunProgress(snapshot(gepaRun(), gepaEvents({ completed: 12 })), NOW);
	assert.ok(
		settled.timing.eta.state === "point" || settled.timing.eta.state === "range",
		`expected a usable estimate, got ${settled.timing.eta.state}`
	);
	assert.match(settled.timing.eta.basis, /phase minibatch/);
	assert.match(formatEta(settled.timing.eta), /remaining$/);
});

test("GEPA: a terminated run reports the breaker, drops the ETA, and keeps its counts", () => {
	const events = [
		...gepaEvents({ completed: 8 }),
		{
			optimizerRunId: "banking77_gepa_sol_med_45856f25", algorithmId: "gepa",
			sequenceNumber: 300, type: "rollout.circuit_breaker.tripped", occurredAt: at(9),
			delta: { rolling_failure_rate: 0.42, tolerance: 0.2, reason: "rollout_failures", message: "Rollout circuit breaker tripped" }
		}
	];
	const projection = projectRunProgress(
		snapshot(gepaRun({ status: "failed", finishedAt: at(9) }), events),
		NOW
	);
	assert.equal(projection.terminal, true);
	assert.equal(projection.status, "failed");
	assert.equal(projection.timing.eta, undefined, "a finished run has a wall time, not an estimate");
	assert.equal(projection.work.completed, 8);
	assert.match(projection.warning, /circuit breaker/i);
	assert.equal(projection.result.partial, true);
});

test("GEPA: a completed run without a heldout evaluation refuses to imply one", () => {
	const projection = projectRunProgress(
		snapshot(gepaRun({ status: "completed", finishedAt: at(9) }), gepaEvents({ completed: 100 })),
		NOW
	);
	assert.equal(projection.result.headline, undefined);
	assert.match(projection.result.absentReason, /no candidate was scored|heldout/);
});

/* ── Evaluation campaigns ─────────────────────────────────────────────── */

function evalRun(overrides = {}) {
	return {
		id: "eval_craftax_code_policy_9f31",
		algorithmId: "eval",
		status: "running",
		source: "local",
		objective: "Craftax code policy",
		sessionRef: "sess-1",
		createdAt: at(0),
		startedAt: at(0),
		cursorSeq: 40,
		capabilities: { cancel: true, pause: true, resume: true, streamEvents: true, candidates: true },
		visualRefs: [{ kind: "visual", id: "visual-eval" }],
		usage: {},
		...overrides
	};
}

function evalEvents({ completed = 6, retries = 0, planned = 10 } = {}) {
	const base = { optimizerRunId: "eval_craftax_code_policy_9f31", algorithmId: "eval" };
	let seq = 0;
	const events = [
		{
			...base, sequenceNumber: ++seq, type: "eval.run.planned", occurredAt: at(0),
			snapshot: {
				planned_trials: planned,
				parallelism: 3,
				global_capacity: 6,
				candidate_set_id: "cs_7ab21",
				manifest_digest: "sha256:aaaa",
				candidates: [
					{ id: "baseline", label: "Baseline", is_baseline: true },
					{ id: "candidate", label: "Candidate" }
				]
			}
		},
		{
			...base, sequenceNumber: ++seq, type: "eval.seed_ledger.sealed", occurredAt: at(0, 5),
			snapshot: { seedLedger: { screening: [0, 1, 2, 3, 4], confirmation: [], scenarios: ["default"] } }
		}
	];
	for (let index = 0; index < completed; index += 1) {
		const trialId = `trial_${index}`;
		const candidateId = index % 2 ? "candidate" : "baseline";
		events.push({
			...base, sequenceNumber: ++seq, type: "eval.trial.queued", occurredAt: at(1, index * 10),
			delta: { trial_id: trialId, candidate_id: candidateId, stage: "screen", seed: index }
		});
		events.push({
			...base, sequenceNumber: ++seq, type: "eval.trial.started", occurredAt: at(1, index * 10 + 1),
			delta: { trial_id: trialId, candidate_id: candidateId, stage: "screen", seed: index }
		});
		events.push({
			...base, sequenceNumber: ++seq, type: "eval.trial.terminal", occurredAt: at(1, index * 10 + 8),
			item: {
				kind: "trial",
				id: trialId,
				status: "completed",
				valid: true,
				candidateId,
				stage: "screen",
				seed: index,
				metrics: { reward: 0.5 + index * 0.05 },
				missingGates: [],
				missingArtifacts: []
			},
			delta: { message: `Trial ${trialId} completed` },
			usageDelta: { cost_usd: 0.02, rollouts: 1 }
		});
	}
	// A retry is a trial re-entering the queue; the eval producer has no retry
	// event type, so the projection has to derive it.
	for (let index = 0; index < retries; index += 1) {
		events.push({
			...base, sequenceNumber: ++seq, type: "eval.trial.queued", occurredAt: at(2, index * 5),
			delta: { trial_id: `trial_${index}`, candidate_id: "baseline", stage: "screen", seed: index }
		});
		events.push({
			...base, sequenceNumber: ++seq, type: "eval.trial.started", occurredAt: at(2, index * 5 + 1),
			delta: { trial_id: `trial_${index}`, candidate_id: "baseline", stage: "screen", seed: index }
		});
		events.push({
			...base, sequenceNumber: ++seq, type: "eval.trial.terminal", occurredAt: at(2, index * 5 + 8),
			item: {
				kind: "trial",
				id: `trial_${index}`,
				status: "completed",
				valid: true,
				candidateId: "baseline",
				stage: "screen",
				seed: index,
				metrics: { reward: 0.6 },
				missingGates: [],
				missingArtifacts: []
			}
		});
	}
	return events;
}

test("eval: the frozen plan is the denominator and the bar is campaign completion", () => {
	const projection = projectRunProgress(snapshot(evalRun(), evalEvents()), NOW);
	assert.equal(projection.runKind, "eval");
	assert.equal(projection.work.total, 10);
	assert.equal(projection.work.completed, 6);
	assert.equal(projection.progress.semantics, "campaign completion");
	assert.equal(formatWork(projection), "6 / 10 trials");
	assert.equal(projection.details.find((detail) => detail.label === "Candidate set").value, "cs_7ab21");
});

test("eval: a retried trial is counted as a retry, not as extra completed work", () => {
	const clean = projectRunProgress(snapshot(evalRun(), evalEvents({ completed: 6 })), NOW);
	const retried = projectRunProgress(snapshot(evalRun(), evalEvents({ completed: 6, retries: 2 })), NOW);
	assert.equal(clean.work.completed, retried.work.completed, "a retry must not inflate completion");
	assert.equal(retried.work.retried, 2);
});

test("eval: a paused campaign is paused, and its estimate freezes", () => {
	const events = [
		...evalEvents({ completed: 4 }),
		{
			optimizerRunId: "eval_craftax_code_policy_9f31", algorithmId: "eval",
			sequenceNumber: 900, type: "eval.run.paused", occurredAt: at(3),
			delta: { paused: true }
		}
	];
	const projection = projectRunProgress(snapshot(evalRun({ status: "paused" }), events), NOW);
	assert.equal(projection.status, "paused");
	assert.equal(projection.timing.eta.state, "paused");
	assert.equal(formatEta(projection.timing.eta), "Paused");
});

test("eval: a campaign with no plan count gets an indeterminate bar and no ETA number", () => {
	const projection = projectRunProgress(snapshot(evalRun(), evalEvents({ planned: 0 })), NOW);
	assert.equal(projection.progress.determinate, false);
	assert.equal(projection.progress.fraction, undefined);
	assert.equal(projection.timing.eta.state, "unavailable");
	assert.match(projection.timing.eta.unavailableReason, /declared no trial count/);
	assert.match(progressAriaText(projection), /progress not measurable/);
});

test("eval: a finished campaign with no champion says so instead of showing a winner", () => {
	const events = [
		...evalEvents({ completed: 10 }),
		{
			optimizerRunId: "eval_craftax_code_policy_9f31", algorithmId: "eval",
			sequenceNumber: 800, type: "eval.selection.completed", occurredAt: at(5),
			snapshot: {
				selection: {
					status: "no_champion",
					winner_id: null,
					baseline_id: "baseline",
					primary_metric: "reward",
					lift: 0.004,
					min_lift: 0.05,
					reason: "lift below the promotion floor"
				}
			}
		}
	];
	const projection = projectRunProgress(
		snapshot(evalRun({ status: "completed", finishedAt: at(5) }), events),
		NOW
	);
	assert.equal(projection.terminal, true);
	assert.equal(projection.result.headline, undefined);
	assert.match(projection.result.absentReason, /lift below the promotion floor/);
});

/* ── Hosted SFT ───────────────────────────────────────────────────────── */

function sftRun(overrides = {}) {
	return {
		id: "sft_craftax_nemotron_c41f",
		algorithmId: "sft",
		status: "running",
		source: "hosted",
		objective: "Craftax policy",
		sessionRef: "sess-1",
		createdAt: at(0),
		startedAt: at(2),
		cursorSeq: 30,
		capabilities: { cancel: true, pause: true, resume: true, streamEvents: true, checkpoints: true },
		visualRefs: [{ kind: "visual", id: "visual-sft" }],
		usage: {},
		...overrides
	};
}

function sftEvents({ steps = 4, declareTotal = false, promote = false } = {}) {
	const base = { optimizerRunId: "sft_craftax_nemotron_c41f", algorithmId: "sft" };
	let seq = 0;
	const events = [
		{ ...base, sequenceNumber: ++seq, type: "run.queued", occurredAt: at(0), delta: { status: "queued", message: "Waiting for an accelerator" } },
		{ ...base, sequenceNumber: ++seq, type: "run.started", occurredAt: at(2), delta: { status: "running" } },
		{
			...base, sequenceNumber: ++seq, type: "sft.dataset.validated", occurredAt: at(2, 10),
			snapshot: { splits: { train: { count: 30_000, digest: "sha256:abc" }, val: { count: 2_000 } } }
		}
	];
	if (declareTotal) {
		events.push({
			...base, sequenceNumber: ++seq, type: "sft.compute.updated", occurredAt: at(2, 15),
			snapshot: { total_steps: 1_000, accelerator: "H100" }
		});
	}
	for (let index = 1; index <= steps; index += 1) {
		events.push({
			...base, sequenceNumber: ++seq, type: "sft.training.metrics", occurredAt: at(3, index * 10),
			delta: { step: index * 100, epoch: index * 0.4, train_loss: 1.4 - index * 0.1, learning_rate: 0.0002 }
		});
	}
	if (promote) {
		events.push({
			...base, sequenceNumber: ++seq, type: "sft.checkpoint.ready", occurredAt: at(5),
			item: { kind: "checkpoint", id: "ckpt_400", status: "ready", raw: { step: 400, ready: true } }
		});
		events.push({
			...base, sequenceNumber: ++seq, type: "sft.checkpoint.promoted", occurredAt: at(5, 30),
			item: { kind: "checkpoint", id: "ckpt_400", status: "promoted", raw: { step: 400, promoted: true } }
		});
	}
	return events;
}

test("SFT: training without a declared total refuses an ETA in the producer's words", () => {
	const projection = projectRunProgress(snapshot(sftRun(), sftEvents()), NOW);
	assert.equal(projection.runKind, "sft");
	assert.equal(projection.phase.id, "training");
	assert.equal(projection.progress.determinate, false);
	assert.equal(projection.timing.eta.state, "unavailable");
	assert.equal(projection.timing.eta.unavailableReason, "provider did not declare total steps");
	assert.equal(formatEta(projection.timing.eta), "Unavailable");
	assert.match(
		projection.details.find((detail) => detail.label === "Training").value,
		/step 400 · epoch 1.6 · train loss 1\.0/
	);
});

test("SFT: a declared step total makes the estimate available on step evidence", () => {
	const projection = projectRunProgress(snapshot(sftRun(), sftEvents({ steps: 9, declareTotal: true })), NOW);
	assert.equal(projection.work.total, 1_000);
	assert.equal(projection.work.completed, 900);
	assert.equal(projection.progress.determinate, true);
	assert.ok(
		projection.timing.eta.state === "point" || projection.timing.eta.state === "range",
		`expected a usable estimate, got ${projection.timing.eta.state}`
	);
	assert.match(projection.timing.eta.basis, /phase training/);
});

test("SFT: queue time is displayed and excluded from the training estimate", () => {
	const projection = projectRunProgress(snapshot(sftRun(), sftEvents({ steps: 5, declareTotal: true })), NOW);
	const queued = projection.details.find((detail) => detail.label === "Queued for");
	assert.equal(queued.value, "2m");
	assert.match(queued.note, /excluded from the training estimate/);
	// Five step records 10s apart → 10s per 100 steps → 500 remaining steps ≈ 50s.
	assert.equal(projection.timing.eta.remainingMs, 10_000 * 5);
});

test("SFT: usage the provider never reported stays unavailable", () => {
	const projection = projectRunProgress(snapshot(sftRun(), sftEvents()), NOW);
	assert.equal(projection.usage.costUsd.value, undefined);
	assert.equal(projection.usage.costUsd.source, "unavailable");
	assert.equal(projection.usage.promptTokens.value, undefined);
	assert.match(costSummary(projection.usage.costUsd, "step"), /producer emitted no cost telemetry/);
});

test("SFT: a promoted checkpoint without a heldout comparison claims no uplift", () => {
	const projection = projectRunProgress(
		snapshot(sftRun({ status: "completed", finishedAt: at(6) }), sftEvents({ steps: 4, promote: true })),
		NOW
	);
	assert.equal(projection.terminal, true);
	assert.equal(projection.result.headline, "Promoted ckpt_400");
	assert.match(projection.result.absentReason, /no paired heldout comparison/);
	assert.ok(projection.warnings.some((warning) => /without a paired heldout comparison/.test(warning)));
});

test("SFT: 'ready' is never presented as promotion", () => {
	const events = sftEvents({ steps: 4 }).concat([
		{
			optimizerRunId: "sft_craftax_nemotron_c41f", algorithmId: "sft",
			sequenceNumber: 500, type: "sft.checkpoint.ready", occurredAt: at(5),
			item: { kind: "checkpoint", id: "ckpt_400", status: "ready", raw: { step: 400, ready: true } }
		}
	]);
	const projection = projectRunProgress(
		snapshot(sftRun({ status: "completed", finishedAt: at(6) }), events),
		NOW
	);
	const promotion = projection.phases.find((phase) => phase.id === "promotion");
	assert.equal(promotion.status, "skipped");
	assert.match(projection.result.absentReason, /none was promoted/);
	assert.equal(projection.result.headline, undefined);
});

/* ── Environment workflows ────────────────────────────────────────────── */

function environmentRun(overrides = {}) {
	return {
		id: "env_craftax_seed7_qa",
		algorithmId: "environment",
		status: "running",
		source: "local",
		objective: "Craftax classic",
		sessionRef: "sess-1",
		createdAt: at(0),
		startedAt: at(0),
		cursorSeq: 20,
		capabilities: { cancel: true },
		visualRefs: [{ kind: "visual", id: "visual-craftax" }],
		usage: {},
		...overrides
	};
}

function environmentEvents({ steps = 6, maxSteps = 20, withCost = true, reward = undefined } = {}) {
	const base = { schemaVersion: "optimizer_event.v1", optimizerRunId: "env_craftax_seed7_qa", algorithmId: "environment" };
	let seq = 0;
	const events = [
		{
			...base, eventId: "e1", sequenceNumber: ++seq, type: "environment.run.planned", occurredAt: at(0),
			snapshot: { max_steps: maxSteps, planned_episodes: 1, seed: 7, runtime_family: "craftax" }
		},
		{
			...base, eventId: "e2", sequenceNumber: ++seq, type: "container.task_info.loaded", occurredAt: at(0, 2),
			delta: { task_name: "Craftax-Classic-v1", runtime_family: "craftax" }
		},
		{
			...base, eventId: "e3", sequenceNumber: ++seq, type: "environment.run.started", occurredAt: at(0, 4),
			delta: { status: "running" }
		},
		{
			...base, eventId: "e4", sequenceNumber: ++seq, type: "environment.episode.started", occurredAt: at(0, 5),
			delta: { episode_id: "ep_0", seed: 7 }
		},
		{
			...base, eventId: "e5", sequenceNumber: ++seq, type: "container.rollout.start", occurredAt: at(0, 6),
			delta: { rollout_id: "rollout_env_0" }
		}
	];
	for (let index = 1; index <= steps; index += 1) {
		events.push({
			...base,
			eventId: `step-${index}`,
			sequenceNumber: ++seq,
			type: "environment.step.completed",
			occurredAt: at(1, index * 5),
			delta: { episode_id: "ep_0", step: index, action: index % 2 ? "move_right" : "do" },
			usageDelta: withCost
				? { cost_usd: 0.001, prompt_tokens: 80, completion_tokens: 12 }
				: { prompt_tokens: 80, completion_tokens: 12 }
		});
	}
	if (reward !== undefined) {
		events.push({
			...base, eventId: "term", sequenceNumber: ++seq, type: "environment.episode.terminal", occurredAt: at(2),
			delta: { episode_id: "ep_0", status: "completed", reward },
			usageDelta: withCost ? { cost_usd: 0.001, rollouts: 1 } : { rollouts: 1 }
		});
		events.push({
			...base, eventId: "done", sequenceNumber: ++seq, type: "container.rollout.completed", occurredAt: at(2, 1),
			delta: { rollout_id: "rollout_env_0" }
		});
	}
	return events;
}

test("environment: the bar is declared steps, never reward", () => {
	const projection = projectRunProgress(snapshot(environmentRun(), environmentEvents({ steps: 6, maxSteps: 20 })), NOW);
	assert.equal(projection.runKind, "environment");
	assert.equal(projection.work.total, 20);
	assert.equal(projection.work.completed, 6);
	assert.equal(projection.work.unit, "steps");
	assert.equal(projection.progress.semantics, "environment steps");
	assert.equal(projection.progress.fraction, 0.3);
	assert.equal(formatWork(projection), "6 / 20 steps");
});

test("environment: missing cost stays unavailable, never $0.00", () => {
	const projection = projectRunProgress(
		snapshot(environmentRun(), environmentEvents({ withCost: false })),
		NOW
	);
	assert.equal(projection.usage.costUsd.value, undefined);
	assert.equal(projection.usage.costUsd.source, "unavailable");
	assert.match(costSummary(projection.usage.costUsd, "step"), /^Cost unavailable/);
	assert.ok(projection.usage.promptTokens.value > 0);
});

test("environment: no step denominator withholds ETA rather than inventing one", () => {
	const projection = projectRunProgress(
		snapshot(environmentRun(), environmentEvents({ maxSteps: 0 })),
		NOW
	);
	assert.equal(projection.progress.determinate, false);
	assert.equal(projection.timing.eta.state, "unavailable");
	assert.match(projection.timing.eta.unavailableReason, /no step or episode count/);
});

test("environment: a sealed episode reports reward as a result, not as progress", () => {
	const projection = projectRunProgress(
		snapshot(
			environmentRun({ status: "completed", finishedAt: at(2, 1) }),
			environmentEvents({ steps: 20, maxSteps: 20, reward: 4.5 })
		),
		NOW
	);
	assert.equal(projection.terminal, true);
	assert.equal(projection.timing.eta, undefined);
	assert.equal(projection.result.headline, "Reward 4.5");
	assert.equal(projection.progress.fraction, 1);
});

/* ── Shared contract ──────────────────────────────────────────────────── */

test("only the four carded workflows are offered a card", () => {
	assert.equal(runKindOf("gepa"), "gepa");
	assert.equal(runKindOf("eval"), "eval");
	assert.equal(runKindOf("sft"), "sft");
	assert.equal(runKindOf("environment"), "environment");
	assert.equal(runKindOf("go-ex"), null);
	assert.equal(runKindOf("dag.behavior"), null);
	assert.equal(
		projectRunProgress(snapshot({ id: "goex_1", algorithmId: "go-ex", status: "running" }, []), NOW),
		null
	);
});

test("a stale history warns that counts are a floor", () => {
	const projection = projectRunProgress(
		snapshot(gepaRun(), gepaEvents({ completed: 5 }), { state: "stale", gap: true }),
		NOW
	);
	assert.equal(projection.stale, true);
	assert.match(projection.warning, /incomplete; counts are a floor/);
});

test("capabilities are advertised, not assumed", () => {
	const projection = projectRunProgress(
		snapshot(gepaRun({ capabilities: { cancel: true } }), gepaEvents({ completed: 2 })),
		NOW
	);
	assert.deepEqual(projection.capabilities, { pause: false, resume: false, cancel: true });
});

test("elapsed time on a terminal run comes from the record, not from now", () => {
	const projection = projectRunProgress(
		snapshot(gepaRun({ status: "completed", startedAt: at(0), finishedAt: at(4) }), gepaEvents({ completed: 4 })),
		NOW
	);
	assert.equal(projection.timing.elapsedMs, 4 * 60_000);
	const later = projectRunProgress(
		snapshot(gepaRun({ status: "completed", startedAt: at(0), finishedAt: at(4) }), gepaEvents({ completed: 4 })),
		NOW + 3_600_000
	);
	assert.equal(later.timing.elapsedMs, projection.timing.elapsedMs);
});
