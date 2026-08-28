/**
 * The Craftax viewer against a real ten-lane producer stream.
 *
 * The fixture is a verbatim capture of `GET /rollouts/{id}/events` for ten
 * canonical seeds (0–9), run live on 2026-08-16 through the Rust gold engine
 * with the ReAct `luna_low` policy on ChatGPT authentication. Nothing in it is
 * hand-written, so this is the projection contract measured against evidence a
 * pool actually emitted — not against a fixture shaped to make it pass.
 *
 * Capture: docs/receipts/2026-08-16/v0.4-evidence-contract/ten-lane-producer-stream.json
 * Producer contract: gamebench containers/react/LIVE_EVAL_CONTRACT.md
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { emptyLiveIngest, ingestLiveEnvelopeBatch } from "../runtime/liveStream.ts";
import {
	craftaxEventLane,
	craftaxRewardValue,
	projectCraftaxSemanticTrace,
	projectCraftaxViewer,
	scopeCraftaxEvents
} from "../families/first_class_example_containers/live.craftax.v1/projectCraftax.ts";

const here = dirname(fileURLToPath(import.meta.url));
const capture = JSON.parse(
	readFileSync(
		join(here, "../../docs/receipts/2026-08-16/v0.4-evidence-contract/ten-lane-producer-stream.json"),
		"utf8"
	)
);

const ROLLOUT_IDS = capture.seeds.map((seed) => `canonical-seed-${seed}`);
/** Sealed outcome rewards from the same run's receipt. */
const SEALED_REWARDS = {
	"canonical-seed-0": 2.0,
	"canonical-seed-1": 2.0,
	"canonical-seed-2": 2.0,
	"canonical-seed-3": 2.6,
	"canonical-seed-4": 1.6,
	"canonical-seed-5": 1.8,
	"canonical-seed-6": 2.0,
	"canonical-seed-7": 0.8,
	"canonical-seed-8": 1.0,
	"canonical-seed-9": 1.6
};

/** Ingest exactly as `useLiveEvalStreams` does when ten poll authorities land. */
const ingested = ingestLiveEnvelopeBatch(emptyLiveIngest(), capture.events);
const scoped = scopeCraftaxEvents(ingested.events, ROLLOUT_IDS);

test("ten rollout-local poll authorities ingest without a duplicate-envelope conflict", () => {
	// Every lane restarts at sequence 1, so ten envelopes legitimately share
	// `event_id: "1"`. Treating that as global identity is what collapsed ten
	// lanes into one.
	assert.deepEqual(ingested.conflicts, []);
	// Ten lanes, ten independent sequence spaces, each with its own evidence
	// high-water mark. That this capture has no holes in any of them is
	// asserted where the gap scan lives — `stream_fold.rs`, over this same
	// capture, through `fixtures/live_fold_golden.json`.
	assert.equal(ingested.lastSequenceByScope.size, ROLLOUT_IDS.length);
	// The producer declares a per-rollout `stream_id`, which outranks the lane
	// as the scope: one scope per lane either way, named by the stream.
	assert.deepEqual(
		[...ingested.lastSequenceByScope.keys()].sort(),
		ROLLOUT_IDS.map((id) => `stream:${id}`).sort()
	);
	assert.equal(ingested.ready, true, "stream.subscribed arrived on every lane");
	assert.equal(ingested.events.length, capture.events.length - ROLLOUT_IDS.length);
});

test("the viewer resolves ten distinct lanes", () => {
	const projection = projectCraftaxViewer(scoped);
	assert.equal(projection.lanes.length, 10);
	assert.deepEqual([...projection.lanes].sort(), [...ROLLOUT_IDS].sort());
	assert.equal(new Set(scoped.map(craftaxEventLane)).size, 10);
});

test("each lane projects its own real reward, and zero-delta steps stay evidence", () => {
	for (const rolloutId of ROLLOUT_IDS) {
		const projection = projectCraftaxViewer(scoped, rolloutId);
		assert.equal(projection.selectedLane, rolloutId);
		// The viewer accumulates reward_signal deltas; that must reproduce the
		// sealed outcome reward exactly.
		assert.ok(
			Math.abs(projection.reward - SEALED_REWARDS[rolloutId]) < 1e-9,
			`${rolloutId}: projected ${projection.reward}, sealed ${SEALED_REWARDS[rolloutId]}`
		);
		const values = projection.rewardSignals.map((event) => craftaxRewardValue(event.payload));
		assert.equal(values.length, 20, `${rolloutId} publishes one reward per executed step`);
		assert.ok(values.every((value) => value != null), "no reward signal is missing a value");
		assert.ok(values.some((value) => value === 0), "zero-earning steps are still reported");
	}
	// The lanes are not all the same number; the seeds really did diverge.
	assert.ok(new Set(Object.values(SEALED_REWARDS)).size >= 5);
});

test("frame replay is populated from emitted producer frames", () => {
	for (const rolloutId of ROLLOUT_IDS) {
		const projection = projectCraftaxViewer(scoped, rolloutId);
		assert.equal(projection.frameEvents.length, 21, `${rolloutId} frame count`);
		assert.equal(projection.frameUnavailable, false);
		assert.equal(
			projection.frameUrl,
			`/rollouts/${rolloutId}/frames/20.png`,
			`${rolloutId} shows the last emitted frame`
		);
		// Relative, so it resolves against the stream origin the viewer reached.
		assert.ok(projection.frameEvents.every((event) => event.payload.format === "png"));
		assert.ok(
			projection.frameEvents.every((event) => String(event.payload.digest).startsWith("sha256:"))
		);
	}
});

test("the policy panel shows real provider, model, actions, and authority", () => {
	for (const rolloutId of ROLLOUT_IDS) {
		const { policy, traceEvents } = projectCraftaxViewer(scoped, rolloutId);
		assert.equal(policy.provider, "chatgpt-codex");
		assert.equal(policy.model, "gpt-5.6-luna");
		assert.ok(policy.actions.length > 0, `${rolloutId} has a selected action plan`);
		assert.equal(policy.actionAuthority, "model");
		assert.equal(policy.fallback, false);
		assert.equal(policy.parseError, undefined);
		assert.ok(policy.assistant, "the model's own output is present");
		// Four span kinds per call, and at least two calls per rollout.
		assert.ok(traceEvents.length >= 8, `${rolloutId} policy span count ${traceEvents.length}`);
	}
});

test("usage stays honestly absent on the Codex subscription lane", () => {
	for (const rolloutId of ROLLOUT_IDS) {
		const { policy } = projectCraftaxViewer(scoped, rolloutId);
		// The subscription lane reports no token accounting, so the viewer must
		// show nothing rather than a fabricated zero.
		assert.deepEqual(policy.usage, {}, `${rolloutId} invented usage`);
	}
});

test("each lane reaches terminal and links its own Trace V5", () => {
	const traceIds = new Set();
	for (const rolloutId of ROLLOUT_IDS) {
		const projection = projectCraftaxViewer(scoped, rolloutId);
		assert.equal(projection.terminal, true, `${rolloutId} is terminal`);
		const reconciled = projection.laneEvents.filter((event) => event.kind === "trace.reconciled");
		assert.equal(reconciled.length, 1);
		assert.equal(reconciled[0].payload.authority, "trace_v5");
		traceIds.add(String(reconciled[0].payload.trace_id));
		const terminal = projection.laneEvents.at(-1);
		assert.equal(terminal.kind, "eval.run.terminal");
		assert.equal(terminal.payload.status, "completed");
	}
	assert.equal(traceIds.size, 10, "ten immutable trace identities, one per rollout");
});

test("the semantic trace groups the run into policy calls and environment steps", () => {
	const projection = projectCraftaxViewer(scoped, "canonical-seed-3");
	const items = projectCraftaxSemanticTrace(projection.laneEvents);
	const policyCalls = items.filter((item) => item.category === "policy");
	const steps = items.filter((item) => item.kind === "environment.step");
	assert.ok(policyCalls.length >= 2);
	assert.equal(steps.length, 20);
	// A policy call renders the observation it answered and the model's output.
	assert.ok(policyCalls[0].interaction?.input, "policy call shows its observation");
	assert.equal(policyCalls[0].interaction?.responseType, "text");
	assert.ok(policyCalls[0].label.includes("gpt-5.6-luna"));
	// Achievements really were unlocked in this lane.
	assert.ok(projection.achievements.includes("defeat_zombie"));
});

test("replaying at any cursor never fabricates evidence the producer had not emitted", () => {
	const lane = projectCraftaxViewer(scoped, "canonical-seed-7");
	const early = projectCraftaxViewer(scoped, "canonical-seed-7", 2);
	assert.ok(early.visibleEvents.length < lane.visibleEvents.length);
	// Before any reward arrived the lane shows no reward, not a zero borrowed
	// from the terminal envelope.
	assert.equal(early.rewardSignals.length, 0);
	assert.equal(early.reward, undefined);
	assert.equal(early.terminal, false);
	assert.equal(early.policy.actions.length, 0);
});
