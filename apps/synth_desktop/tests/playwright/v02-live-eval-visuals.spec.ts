/**
 * v0.2 live-eval visual families: fixture replay of Craftax, Harbor, and
 * dig.bench. Proves visual-first slot `stream`, missing ≠ 0, campaign
 * isolation, and no invented frames. Paid providers are not used.
 */
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "./browser.fixture";
import { installVisuals, liveVisual, metricValue, openVisual, streamBinding } from "./v02-helpers";

const visualsRoot = join(dirname(fileURLToPath(import.meta.url)), "../../../../visuals");

function loadEvents(rel: string): unknown[] {
	const parsed = JSON.parse(readFileSync(join(visualsRoot, rel), "utf8")) as { events?: unknown[] };
	return parsed.events ?? [];
}

function envelope(kind: string, sequence: number | null, payload: Record<string, unknown> = {}, extra: Record<string, unknown> = {}) {
	return {
		ts: "2026-08-13T13:00:00.000Z",
		kind,
		sequence,
		payload,
		...extra
	};
}

test("[v0.2] Craftax live visual replays fixture evidence and keeps missing usage missing", async ({ page }) => {
	const events = loadEvents("families/first_class_example_containers/live.craftax.v1/examples/events.json");
	await installVisuals(page, [liveVisual({
		id: "vis_v02_craftax",
		templateId: "live.craftax.v1",
		title: "Craftax seed 0",
		bindings: streamBinding(events)
	})]);
	const pane = await openVisual(page, "vis_v02_craftax");
	const viewer = pane.getByTestId("visual-live-craftax");
	await expect(viewer).toBeVisible();
	await expect(viewer).toContainText("You see a tree.", { timeout: 20_000 });
	await expect(viewer).toContainText("0.50");
	await expect(viewer).toContainText("not emitted", { timeout: 15_000 });
	await expect(viewer).not.toContainText("$0.00");
	await expect(viewer).not.toContainText("stream.subscribed");
});

test("[v0.2] Harbor live visual fails closed when reward.txt is missing", async ({ page }) => {
	const events = [
		envelope("stream.subscribed", null, { "stream.id": "stream_harbor_missing" }),
		envelope("trace.opened", 1, { rollout_id: "harbor_missing" }),
		envelope("trial.planned", 2, { instruction: "solve without a score file", trial_id: "trial_missing" }),
		envelope("trial.launched", 3, { sandbox: "env:harbor_public", trial_id: "trial_missing" }),
		envelope("verifier", 4, { script: "tests/test.sh", trial_id: "trial_missing" }),
		envelope("status", 5, { status: "completed" })
	];
	await installVisuals(page, [liveVisual({
		id: "vis_v02_harbor_missing",
		templateId: "live.harbor_eval.v1",
		title: "Harbor missing reward",
		bindings: streamBinding(events)
	})]);
	const pane = await openVisual(page, "vis_v02_harbor_missing");
	const viewer = pane.getByTestId("visual-live-harbor-eval");
	await expect(viewer).toBeVisible();
	await expect(viewer.getByTestId("harbor-trials")).toContainText("verified", { timeout: 20_000 });
	expect(await metricValue(viewer, "Reward")).toBe("—");
	expect(await metricValue(viewer, "reward.txt")).toBe("not yet");
	await expect(viewer).not.toContainText("$0.00");
});

test("[v0.2] dig.bench live visual is text-only and keeps incomplete reward null", async ({ page }) => {
	const events = loadEvents("families/first_class_example_containers/live.digbench.v1/examples/events.json");
	await installVisuals(page, [liveVisual({
		id: "vis_v02_digbench",
		templateId: "live.digbench.v1",
		title: "dig.bench P-1",
		bindings: streamBinding(events)
	})]);
	const pane = await openVisual(page, "vis_v02_digbench");
	const viewer = pane.getByTestId("visual-live-digbench");
	await expect(viewer).toBeVisible();
	await expect(viewer.getByTestId("digbench-observation")).toContainText("A locked door", { timeout: 25_000 });
	await expect(viewer.getByTestId("digbench-legal-actions")).toContainText("inspect");
	expect(await metricValue(viewer, "/reward")).toBe("pending");
	await expect(viewer.locator("img")).toHaveCount(0);
	await expect(viewer).not.toContainText("PNG");
	await expect(viewer).not.toContainText("0.00");
});

test("[v0.2] guessed /events bindings fail closed instead of rendering a live visual", async ({ page }) => {
	await installVisuals(page, [liveVisual({
		id: "vis_v02_guessed",
		templateId: "live.craftax.v1",
		title: "Guessed stream",
		bindings: {
			schemaVersion: "synth.visual-bindings.v1",
			slots: [{ slot: "stream", kind: "live_sse", source: "http://127.0.0.1:8298/events" }]
		}
	})]);
	const pane = await openVisual(page, "vis_v02_guessed");
	const invalid = pane.getByTestId("visual-invalid");
	await expect(invalid).toBeVisible();
	await expect(invalid).toContainText(/guessed|Refusing/);
	await expect(pane.getByTestId("visual-live-craftax")).toHaveCount(0);
});

test("[v0.2] two live visuals do not import each other's evidence", async ({ page }) => {
	const laneA = [
		envelope("stream.subscribed", null, { "stream.id": "stream_a" }, { run_id: "roll_a" }),
		envelope("observation", 1, { text: "ALPHA-ONLY observation" }, { run_id: "roll_a", lane: "roll_a" }),
		envelope("reward_signal", 2, { value: 4 }, { run_id: "roll_a", lane: "roll_a" })
	];
	const laneB = [
		envelope("stream.subscribed", null, { "stream.id": "stream_b" }, { run_id: "roll_b" }),
		envelope("observation", 1, { text: "BRAVO-ONLY observation" }, { run_id: "roll_b", lane: "roll_b" }),
		envelope("reward_signal", 2, { value: 1 }, { run_id: "roll_b", lane: "roll_b" })
	];
	await installVisuals(page, [
		liveVisual({
			id: "vis_v02_iso_a",
			templateId: "live.craftax.v1",
			title: "Lane A",
			bindings: streamBinding(laneA, { scope: { campaign_id: "camp_a", rollout_ids: ["roll_a"], selection: { initial_rollout_id: "roll_a" } } })
		}),
		liveVisual({
			id: "vis_v02_iso_b",
			templateId: "live.craftax.v1",
			title: "Lane B",
			bindings: streamBinding(laneB, { scope: { campaign_id: "camp_b", rollout_ids: ["roll_b"], selection: { initial_rollout_id: "roll_b" } } })
		})
	]);
	const pane = await openVisual(page, "vis_v02_iso_a");
	await expect(pane.getByTestId("visual-live-craftax")).toContainText("ALPHA-ONLY observation", { timeout: 20_000 });
	await expect(pane.getByTestId("visual-live-craftax")).not.toContainText("BRAVO-ONLY observation");
	await page.getByTestId("visuals-card-vis_v02_iso_b").getByRole("button", { name: "Open" }).click();
	await expect(page.getByTestId("visual-pane").getByTestId("visual-live-craftax")).toContainText("BRAVO-ONLY observation", { timeout: 20_000 });
	await expect(page.getByTestId("visual-pane").getByTestId("visual-live-craftax")).not.toContainText("ALPHA-ONLY observation");
});
