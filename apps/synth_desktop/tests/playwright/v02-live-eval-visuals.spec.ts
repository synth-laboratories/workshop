/**
 * v0.2 live-eval visual families: fixture replay of Craftax and Harbor.
 * Proves visual-first slot `stream`, missing ≠ 0, campaign
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
	const paneBody = pane.locator(".visual-pane-body");
	await paneBody.evaluate((element) => {
		Object.assign((element as HTMLElement).style, {
			alignSelf: "flex-end",
			flex: "0 0 340px",
			width: "340px",
			maxWidth: "340px"
		});
	});
	await expect(viewer.locator(".cv-surfaces")).toHaveCSS("display", "grid");
	const overviewColumns = await viewer.locator(".cv-overview-grid").evaluate((element) =>
		getComputedStyle(element).gridTemplateColumns.split(" ").filter(Boolean).length
	);
	expect(overviewColumns).toBe(1);
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

test("[v0.2] guessed /events bindings fail closed instead of rendering a live visual", async ({ page }) => {
	await installVisuals(page, [liveVisual({
		id: "vis_v02_guessed",
		templateId: "live.craftax.v1",
		title: "Guessed stream",
		bindings: {
			schemaVersion: "synth.visual-bindings.v1",
			inputs: [{ input: "stream", kind: "live_sse", source: "http://127.0.0.1:8298/events" }]
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

test("[v0.2] live.eval_stream.v1 shortcut pane mounts advertised compose landmarks", async ({ page }) => {
	const events = [
		envelope("stream.subscribed", null, { "stream.id": "stream_eval_shortcut", ready: true }),
		envelope("run_started", 1, { suite: "eval_acceptance" }),
		envelope("rollout.finished", 2, { marker: "EVAL-REWARD-3.1", reward: 3.1 }),
		envelope("run_finished", 3, { status: "completed", mean_reward: 3.1 })
	];
	await installVisuals(page, [liveVisual({
		id: "vis_v02_eval_stream",
		templateId: "live.eval_stream.v1",
		title: "Eval stream shortcut",
		bindings: {
			schemaVersion: "synth.visual-bindings.v1",
			inputs: [{ input: "stream", kind: "inline", data: { events } }]
		}
	})]);
	const pane = await openVisual(page, "vis_v02_eval_stream");
	const viewer = pane.getByTestId("visual-live-eval-stream");
	await expect(viewer).toBeVisible();
	await expect(viewer.getByTestId("compose-metrics")).toBeVisible();
	await expect(viewer.getByTestId("compose-event-stream")).toBeVisible();
	await expect(viewer.getByTestId("compose-scrubber")).toBeVisible();
	await expect(viewer.getByRole("button", { name: /rollout\.finished/ })).toBeVisible({ timeout: 20_000 });
	await expect(viewer.getByTestId("compose-metrics-count")).toHaveText("3", { timeout: 20_000 });
	await expect(viewer.getByTestId("compose-metrics-scalar")).toContainText("3.1");
	const slider = viewer.getByTestId("compose-scrubber-slider");
	await expect(slider).toBeVisible();
	await slider.fill("2");
	await expect(viewer.getByTestId("compose-scrubber-sequence")).toHaveText("2");
	await viewer.getByRole("button", { name: /rollout\.finished/ }).click();
	const modal = viewer.getByTestId("compose-detail-modal");
	await expect(modal).toBeVisible();
	await expect(modal.getByTestId("compose-detail-payload")).toContainText("EVAL-REWARD-3.1");
});
