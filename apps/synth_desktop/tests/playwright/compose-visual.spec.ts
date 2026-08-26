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
