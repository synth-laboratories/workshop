/**
 * Prototype compose.visual.v1: create a VisualRecord, open it in the Desktop
 * pane, and prove the advertised components share one host-owned ingest.
 */
import { expect, test } from "./browser.fixture";
import { installVisuals, liveVisual, openVisual } from "./v02-helpers";

const MARKER = "PROTOTYPE-REWARD-3.1";

function envelope(
	kind: string,
	sequence: number | null,
	payload: Record<string, unknown> = {}
) {
	return {
		ts: "2026-08-26T16:00:00.000Z",
		run_id: "proto_run",
		kind,
		sequence,
		payload
	};
}

function composeSpec(placements: unknown[]) {
	return {
		schemaVersion: "synth.visual.compose_spec.v1" as const,
		title: "Harbor smoke · live stream",
		placements
	};
}

function composeBindings(spec: unknown, events: unknown[]) {
	return {
		schemaVersion: "synth.visual-bindings.v1" as const,
		slots: [
			{ slot: "spec", kind: "inline" as const, schema: "synth.visual.compose_spec.v1", data: spec },
			{ slot: "stream", kind: "inline" as const, data: { events } }
		]
	};
}

const happyPlacements = [
	{
		id: "log",
		component: "event_stream.v1",
		slot: "stream",
		config: { includeKinds: ["rollout.finished"] }
	},
	{ id: "inspect", component: "detail_modal.v1", from: "log" }
];

const prototypeEvents = [
	envelope("stream.subscribed", null, { "stream.id": "stream_proto", ready: true }),
	envelope("run_started", 1, { suite: "harbor_smoke" }),
	envelope("rollout.finished", 2, { marker: MARKER, reward: 3.1 })
];

test("compose visual replays a bound stream, hides control envelopes, and opens an in-pane detail modal", async ({ page }) => {
	await installVisuals(page, [liveVisual({
		id: "vis_compose_proto",
		templateId: "compose.visual.v1",
		title: "Compose prototype",
		bindings: composeBindings(composeSpec(happyPlacements), prototypeEvents)
	})]);
	const pane = await openVisual(page, "vis_compose_proto");
	const viewer = pane.getByTestId("visual-compose");
	await expect(viewer).toBeVisible();
	await expect(viewer.getByTestId("compose-event-stream")).toBeVisible();
	await expect(viewer.getByRole("button", { name: /rollout\.finished/ })).toBeVisible({ timeout: 20_000 });
	await expect(viewer).not.toContainText("stream.subscribed");
	await expect(viewer).not.toContainText("run_started");
	await viewer.getByRole("button", { name: /rollout\.finished/ }).click();
	const modal = viewer.getByTestId("compose-detail-modal");
	await expect(modal).toBeVisible();
	await expect(modal.getByTestId("compose-detail-payload")).toContainText(MARKER);
	await viewer.getByTestId("compose-detail-close").click();
	await expect(modal).toHaveCount(0);
});

test("unknown compose component ids fail closed instead of rendering an event log", async ({ page }) => {
	await installVisuals(page, [liveVisual({
		id: "vis_compose_unknown",
		templateId: "compose.visual.v1",
		title: "Compose unknown component",
		bindings: composeBindings(
			composeSpec([{ id: "log", component: "not.a.thing.v1", slot: "stream" }]),
			prototypeEvents
		)
	})]);
	const pane = await openVisual(page, "vis_compose_unknown");
	const viewer = pane.getByTestId("visual-compose");
	await expect(viewer).toBeVisible();
	await expect(viewer.getByTestId("visual-compose-invalid")).toContainText(/Unknown compose component "not\.a\.thing\.v1"/);
	await expect(viewer.getByTestId("compose-event-stream")).toHaveCount(0);
	await expect(pane.getByTestId("visual-invalid")).toHaveCount(0);
});

const OPT_GEPA = "CUA-OPT-GEPA";
const OPT_SFT = "CUA-OPT-SFT";
const OPT_CISPO = "CUA-OPT-CISPO";

function optimizerEvent(
	type: string,
	sequence: number,
	algorithmId: string,
	delta: Record<string, unknown>
) {
	return {
		schema_version: "optimizer_event.v1",
		type,
		sequence_number: sequence,
		created_at: "2026-08-26T16:00:00.000Z",
		run_id: "opt_compose_proto",
		algorithm_id: algorithmId,
		delta
	};
}

function optimizerBindings(spec: unknown, events: unknown[]) {
	return {
		schemaVersion: "synth.visual-bindings.v1" as const,
		slots: [
			{ slot: "spec", kind: "inline" as const, schema: "synth.visual.compose_spec.v1", data: spec },
			{ slot: "optimizer_run", kind: "inline" as const, schema: "optimizer_event.v1", data: { events } }
		]
	};
}

const optimizerPlacements = [
	{
		id: "log",
		component: "event_stream.v1",
		slot: "optimizer_run",
		input: "optimizer_run",
		config: { includeKinds: ["candidate.accepted", "sft.training.metrics", "cispo.clip.identity"] }
	},
	{ id: "inspect", component: "detail_modal.v1", from: "log" }
];

const optimizerEvents = [
	optimizerEvent("optimizer.visual.ready", 1, "gepa", { ready: true }),
	optimizerEvent("candidate.accepted", 2, "gepa", { marker: OPT_GEPA, candidate_id: "cand_live" }),
	optimizerEvent("sft.training.metrics", 3, "sft", { marker: OPT_SFT, step: 20, train_loss: 1.1 }),
	optimizerEvent("cispo.clip.identity", 4, "cispo", { marker: OPT_CISPO, clip: 0.2 })
];

test("compose visual replays optimizer_run events, hides ready receipts, and opens GEPA/SFT/CISPO detail", async ({ page }) => {
	await installVisuals(page, [liveVisual({
		id: "vis_compose_optimizer",
		templateId: "compose.visual.v1",
		title: "Compose optimizer_run",
		bindings: optimizerBindings(composeSpec(optimizerPlacements), optimizerEvents)
	})]);
	const pane = await openVisual(page, "vis_compose_optimizer");
	const viewer = pane.getByTestId("visual-compose");
	await expect(viewer).toBeVisible();
	await expect(viewer.getByTestId("compose-event-stream")).toBeVisible();
	await expect(viewer.getByRole("button", { name: /candidate\.accepted/ })).toBeVisible();
	await expect(viewer.getByRole("button", { name: /sft\.training\.metrics/ })).toBeVisible();
	await expect(viewer.getByRole("button", { name: /cispo\.clip\.identity/ })).toBeVisible();
	await expect(viewer).not.toContainText("optimizer.visual.ready");
	await expect(viewer).not.toContainText("rollout.finished");
	await viewer.getByRole("button", { name: /candidate\.accepted/ }).click();
	const modal = viewer.getByTestId("compose-detail-modal");
	await expect(modal).toBeVisible();
	await expect(modal.getByTestId("compose-detail-payload")).toContainText(OPT_GEPA);
	await viewer.getByTestId("compose-detail-close").click();
	await viewer.getByRole("button", { name: /sft\.training\.metrics/ }).click();
	await expect(viewer.getByTestId("compose-detail-payload")).toContainText(OPT_SFT);
	await viewer.getByTestId("compose-detail-close").click();
	await viewer.getByRole("button", { name: /cispo\.clip\.identity/ }).click();
	await expect(viewer.getByTestId("compose-detail-payload")).toContainText(OPT_CISPO);
});

test("compose visual requires a bound optimizer_run when a placement consumes it", async ({ page }) => {
	await installVisuals(page, [liveVisual({
		id: "vis_compose_optimizer_unbound",
		templateId: "compose.visual.v1",
		title: "Compose optimizer unbound",
		bindings: {
			schemaVersion: "synth.visual-bindings.v1" as const,
			slots: [
				{
					slot: "spec",
					kind: "inline" as const,
					schema: "synth.visual.compose_spec.v1",
					data: composeSpec(optimizerPlacements)
				}
			]
		}
	})]);
	const pane = await openVisual(page, "vis_compose_optimizer_unbound");
	const viewer = pane.getByTestId("visual-compose");
	await expect(viewer).toBeVisible();
	await expect(viewer.getByTestId("visual-compose-invalid")).toContainText(
		/Placement requires a bound optimizer_run input/
	);
	await expect(viewer.getByTestId("compose-event-stream")).toHaveCount(0);
});

test("compose visual refuses eval traces bound onto optimizer_run", async ({ page }) => {
	await installVisuals(page, [liveVisual({
		id: "vis_compose_optimizer_flatten",
		templateId: "compose.visual.v1",
		title: "Compose optimizer flatten",
		bindings: optimizerBindings(composeSpec(optimizerPlacements), prototypeEvents)
	})]);
	const pane = await openVisual(page, "vis_compose_optimizer_flatten");
	const viewer = pane.getByTestId("visual-compose");
	await expect(viewer).toBeVisible();
	await expect(viewer.getByTestId("visual-compose-invalid")).toContainText(/does not flatten eval traces/);
	await expect(viewer.getByTestId("compose-event-stream")).toHaveCount(0);
	await expect(viewer).not.toContainText("rollout.finished");
});

const laterPlacements = [
	{ id: "strip", component: "metrics.v1", input: "stream" },
	{ id: "playhead", component: "scrubber.v1", input: "stream" },
	{ id: "candidates", component: "candidate_inspector.v1", input: "optimizer_run" }
];

function kitBindings(spec: unknown, streamEvents: unknown[], optimizerRows: unknown[]) {
	const rows = [
		{ input: "spec", slot: "spec", kind: "inline" as const, schema: "synth.visual.compose_spec.v1", data: spec },
		{ input: "stream", slot: "stream", kind: "inline" as const, data: { events: streamEvents } },
		{ input: "optimizer_run", slot: "optimizer_run", kind: "inline" as const, schema: "optimizer_event.v1", data: { events: optimizerRows } }
	];
	return {
		schemaVersion: "synth.visual-bindings.v1" as const,
		inputs: rows,
		slots: rows
	};
}

test("compose metrics strip counts fixture events and scrubber can select sequence", async ({ page }) => {
	await installVisuals(page, [liveVisual({
		id: "vis_compose_later_stream",
		templateId: "compose.visual.v1",
		title: "Compose later stream",
		bindings: kitBindings(
			composeSpec(laterPlacements),
			prototypeEvents,
			[optimizerEvent("cispo.clip.identity", 4, "cispo", { clip: 0.2 })]
		)
	})]);
	const pane = await openVisual(page, "vis_compose_later_stream");
	const viewer = pane.getByTestId("visual-compose");
	await expect(viewer).toBeVisible();
	await expect(viewer.getByTestId("compose-metrics")).toBeVisible();
	await expect(viewer.getByTestId("compose-metrics-count")).toHaveText("2", { timeout: 20_000 });
	await expect(viewer.getByTestId("compose-metrics-scalar")).toContainText("3.1");
	const slider = viewer.getByTestId("compose-scrubber-slider");
	await expect(slider).toBeVisible();
	await slider.fill("2");
	await expect(viewer.getByTestId("compose-scrubber-sequence")).toHaveText("2");
	await expect(viewer.getByTestId("compose-candidate-inspector-empty")).toBeVisible();
	await expect(viewer.getByTestId("compose-candidate-inspector")).not.toContainText("cand_");
});

test("compose candidate inspector lists candidate.accepted identity from optimizer_run", async ({ page }) => {
	await installVisuals(page, [liveVisual({
		id: "vis_compose_later_candidates",
		templateId: "compose.visual.v1",
		title: "Compose later candidates",
		bindings: optimizerBindings(
			composeSpec([
				{ id: "candidates", component: "candidate_inspector.v1", input: "optimizer_run" }
			]),
			optimizerEvents
		)
	})]);
	const pane = await openVisual(page, "vis_compose_later_candidates");
	const viewer = pane.getByTestId("visual-compose");
	await expect(viewer).toBeVisible();
	await expect(viewer.getByTestId("compose-candidate-inspector")).toBeVisible();
	await expect(viewer.getByTestId("compose-candidate-cand_live")).toBeVisible();
	await expect(viewer.getByTestId("compose-candidate-inspector-empty")).toHaveCount(0);
});

test("candidate_inspector on stream fails closed", async ({ page }) => {
	await installVisuals(page, [liveVisual({
		id: "vis_compose_inspector_stream",
		templateId: "compose.visual.v1",
		title: "Compose inspector stream",
		bindings: composeBindings(
			composeSpec([{ id: "candidates", component: "candidate_inspector.v1", input: "stream" }]),
			prototypeEvents
		)
	})]);
	const pane = await openVisual(page, "vis_compose_inspector_stream");
	const viewer = pane.getByTestId("visual-compose");
	await expect(viewer).toBeVisible();
	await expect(viewer.getByTestId("visual-compose-invalid")).toContainText(/must consume input "optimizer_run"/);
	await expect(viewer.getByTestId("compose-candidate-inspector")).toHaveCount(0);
});
