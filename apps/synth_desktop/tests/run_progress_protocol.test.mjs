/**
 * Protocol fixture tests for every workflow family that `run_progress.v1`
 * accepts. This is the event-contract pin Lanes B/C/D produce against:
 * envelope, sequence, omitted-not-zero usage, and a projection that does not
 * fabricate missing telemetry.
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

const {
	EVENT_CONTRACT_PIN,
	EVENT_SCHEMA_VERSION,
	FAMILY_EVENT_TYPES,
	WORKFLOW_FAMILIES,
	validateProtocolEvent,
	validateProtocolStream
} = await import(bundle("src/renderer/src/runtime/runProgress/protocol.ts", "runProgressProtocol.mjs"));
const { projectRunProgress, progressAgreement } = await import(
	bundle("src/renderer/src/runtime/runProgress/project.ts", "runProgressProjectProtocol.mjs")
);
const { costSummary } = await import(
	bundle("src/renderer/src/runtime/runProgress/usage.ts", "runProgressUsageProtocol.mjs")
);

const NOW = Date.UTC(2026, 7, 17, 12, 30, 0);
const at = (minute, second = 0) => new Date(Date.UTC(2026, 7, 17, 12, minute, second)).toISOString();

function envelope(family, sequence, type, extra = {}) {
	return {
		schemaVersion: EVENT_SCHEMA_VERSION,
		eventId: `${family}:${sequence}`,
		type,
		sequenceNumber: sequence,
		occurredAt: at(0, Math.min(59, sequence)),
		optimizerRunId: `run-${family}`,
		algorithmId: family,
		...extra
	};
}

function snapshot(run, events) {
	return {
		runId: run.id,
		state: "subscribed",
		run,
		events,
		cursor: events.at(-1)?.sequenceNumber ?? 0,
		gap: false,
		revision: 1
	};
}

function runRecord(family, overrides = {}) {
	return {
		id: `run-${family}`,
		algorithmId: family,
		status: "running",
		objective: family,
		sessionRef: "sess-1",
		createdAt: at(0),
		startedAt: at(0),
		cursorSeq: 8,
		capabilities: { cancel: true },
		usage: {},
		...overrides
	};
}

const FAMILY_FIXTURES = {
	gepa: [
		envelope("gepa", 1, "gepa.run.started", { delta: { message: "GEPA run started" } }),
		envelope("gepa", 2, "optimizer.limit.estimate_updated", {
			delta: { limits: [{ kind: "total_rollouts", max: 10, spent: 2, hard: true }] }
		}),
		envelope("gepa", 3, "optimizer.evaluation_result.received", {
			occurredAt: at(1),
			delta: { rollout_id: "r0", reward: 0.4 },
			usageDelta: { prompt_tokens: 10, completion_tokens: 4, rollouts: 1 }
		}),
		envelope("gepa", 4, "optimizer.evaluation_result.received", {
			occurredAt: at(1, 20),
			delta: { rollout_id: "r1", reward: 0.5 },
			usageDelta: { prompt_tokens: 10, completion_tokens: 4, rollouts: 1 }
		})
	],
	eval: [
		envelope("eval", 1, "eval.run.planned", { snapshot: { planned_trials: 4, parallelism: 2 } }),
		envelope("eval", 2, "eval.trial.queued", { delta: { trial_id: "t0" } }),
		envelope("eval", 3, "eval.trial.started", { delta: { trial_id: "t0" } }),
		envelope("eval", 4, "eval.trial.terminal", {
			occurredAt: at(1),
			item: { kind: "trial", id: "t0", status: "completed", valid: true, candidateId: "baseline", stage: "screen" },
			usageDelta: { prompt_tokens: 8, completion_tokens: 3 }
		})
	],
	sft: [
		envelope("sft", 1, "run.queued"),
		envelope("sft", 2, "run.started", { occurredAt: at(1) }),
		envelope("sft", 3, "sft.training.metrics", {
			occurredAt: at(2),
			delta: { step: 10, train_loss: 1.2 }
		}),
		envelope("sft", 4, "sft.training.metrics", {
			occurredAt: at(2, 20),
			delta: { step: 20, train_loss: 1.1 }
		})
	],
	environment: [
		envelope("environment", 1, "environment.run.planned", { snapshot: { max_steps: 8, seed: 3 } }),
		envelope("environment", 2, "environment.run.started"),
		envelope("environment", 3, "environment.episode.started", { delta: { episode_id: "ep0" } }),
		envelope("environment", 4, "environment.step.completed", {
			occurredAt: at(1),
			delta: { step: 1, action: "do" },
			usageDelta: { prompt_tokens: 40, completion_tokens: 6 }
		}),
		envelope("environment", 5, "environment.step.completed", {
			occurredAt: at(1, 10),
			delta: { step: 2, action: "do" },
			usageDelta: { prompt_tokens: 40, completion_tokens: 6 }
		})
	]
};

test("the event-contract pin is stable for consumer lanes", () => {
	assert.equal(EVENT_CONTRACT_PIN, "run_progress.event-contract.v1");
	assert.deepEqual([...WORKFLOW_FAMILIES], ["eval", "gepa", "sft", "environment"]);
	for (const family of WORKFLOW_FAMILIES) {
		assert.ok(FAMILY_EVENT_TYPES[family].length > 0, `${family} must declare its event types`);
	}
});

for (const family of WORKFLOW_FAMILIES) {
	test(`${family}: protocol fixture validates and projects without fabricating usage`, () => {
		const events = FAMILY_FIXTURES[family];
		assert.deepEqual(validateProtocolStream(events), []);
		for (const event of events) {
			assert.ok(
				FAMILY_EVENT_TYPES[family].includes(event.type),
				`${family} fixture used undeclared type ${event.type}`
			);
			assert.equal(validateProtocolEvent(event).length, 0);
			if (event.usageDelta) {
				assert.equal(Object.prototype.hasOwnProperty.call(event.usageDelta, "cost_usd"), false);
				assert.equal(Object.prototype.hasOwnProperty.call(event.usageDelta, "costUsd"), false);
			}
		}
		const projection = projectRunProgress(snapshot(runRecord(family), events), NOW);
		assert.equal(projection.runKind, family);
		assert.equal(projection.schemaVersion, "run_progress.v1");
		assert.equal(projection.usage.costUsd.value, undefined);
		assert.equal(projection.usage.costUsd.source, "unavailable");
		assert.doesNotMatch(costSummary(projection.usage.costUsd), /\$0/);
		const agreement = progressAgreement(projection);
		assert.equal(agreement.costUsd, null);
		assert.equal(typeof agreement.phaseId, "string");
		assert.equal(agreement.status, "running");
	});
}

test("a usage key set to null is unavailable, not a reported zero", () => {
	const events = [
		envelope("gepa", 1, "gepa.run.started"),
		envelope("gepa", 2, "optimizer.evaluation_result.received", {
			usageDelta: { cost_usd: null, prompt_tokens: 10, completion_tokens: 2, rollouts: 1 }
		})
	];
	assert.deepEqual(validateProtocolStream(events), []);
	const projection = projectRunProgress(snapshot(runRecord("gepa"), events), NOW);
	assert.equal(projection.usage.costUsd.value, undefined);
	assert.notEqual(projection.usage.promptTokens.value, 0);
	assert.ok(projection.usage.promptTokens.value == null || projection.usage.promptTokens.value > 0);
});

test("a fabricated numeric zero is distinguishable from an omitted field", () => {
	const omitted = envelope("sft", 1, "sft.training.metrics", { delta: { step: 1 } });
	const reportedZero = envelope("sft", 2, "sft.training.metrics", {
		delta: { step: 2 },
		usageDelta: { cost_usd: 0 }
	});
	assert.equal(Object.prototype.hasOwnProperty.call(omitted, "usageDelta"), false);
	assert.equal(reportedZero.usageDelta.cost_usd, 0);
	const projection = projectRunProgress(
		snapshot(runRecord("sft"), [omitted, reportedZero]),
		NOW
	);
	assert.equal(projection.usage.costUsd.value, 0);
	assert.equal(projection.usage.promptTokens.value, undefined);
});
