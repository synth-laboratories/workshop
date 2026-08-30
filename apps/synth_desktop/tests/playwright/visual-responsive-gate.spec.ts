/**
 * A10-style responsive visual gate: real rendered DOM measurements and
 * screenshots at 1440 / 1024 / 768 / 390, driven by the REAL Banking77 GEPA
 * runs (Sol + Luna) and a delta-heavy Craftax fixture. `noOverflow` here is
 * measured from the DOM, never asserted from metadata.
 */

import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { expect, test } from "./browser.fixture";

const RUNS_DIR = join(
	homedir(),
	".synth-desktop/instances/v02/gepa/runtime/gepa/runs"
);
const SOL_ID = "banking77_gepa_sol_med_45856f25";
const LUNA_ID = "banking77_gepa_luna_med_82f8136b";
const SHOT_DIR = join(import.meta.dirname, "../../test-results/visual-gate");

// The full QA matrix from docs/visuals_issues.md. 680 and 480 are the widths the
// live GEPA review actually found problems at, so a gate that skips them is not
// measuring the reported surface.
const VIEWPORTS = [
	{ width: 1440, height: 900 },
	{ width: 1024, height: 768 },
	{ width: 768, height: 1024 },
	{ width: 680, height: 900 },
	{ width: 480, height: 844 },
	{ width: 390, height: 844 }
] as const;

/** Widths that also get a 200% zoom pass. Screenshotting all six twice is not
 * worth the wall clock; these two cover the wide and compact layouts. */
const ZOOM_WIDTHS = new Set([1440, 768]);

function loadRunEvents(runId: string): unknown[] | null {
	const path = join(RUNS_DIR, runId, "events.optimizer.jsonl");
	if (!existsSync(path)) return null;
	return readFileSync(path, "utf8")
		.split("\n")
		.filter(Boolean)
		.map((line) => JSON.parse(line));
}

async function assertNoHorizontalOverflow(page: Page, label: string): Promise<void> {
	const metrics = await page.evaluate(() => {
		const root = document.scrollingElement ?? document.documentElement;
		const offenders: string[] = [];
		const limit = window.innerWidth + 1;
		for (const element of document.querySelectorAll("body *")) {
			const rect = element.getBoundingClientRect();
			if (rect.width > 0 && rect.right > limit + 8) {
				const probe = element as HTMLElement;
				offenders.push(`${probe.tagName.toLowerCase()}.${String(probe.className).slice(0, 60)} right=${Math.round(rect.right)}`);
				if (offenders.length >= 5) break;
			}
		}
		return {
			scrollWidth: root.scrollWidth,
			clientWidth: root.clientWidth,
			innerWidth: window.innerWidth,
			offenders
		};
	});
	expect(
		metrics.scrollWidth,
		`${label}: page scrollWidth ${metrics.scrollWidth} must not exceed clientWidth ${metrics.clientWidth}; offenders: ${metrics.offenders.join(" | ")}`
	).toBeLessThanOrEqual(metrics.clientWidth + 1);
}

/**
 * WCAG 1.4.4 reflow: at 200% zoom the layout must still not force horizontal
 * scrolling. Chromium's `zoom` reproduces browser zoom rather than merely
 * shrinking the viewport, so text scales too — which is the case that actually
 * breaks sticky headers and single-line receipts.
 */
async function assertNoOverflowAtDoubleZoom(page: Page, label: string): Promise<void> {
	await page.evaluate(() => {
		document.documentElement.style.zoom = "2";
	});
	await page.waitForTimeout(150);
	try {
		await assertNoHorizontalOverflow(page, `${label} @200% zoom`);
	} finally {
		await page.evaluate(() => {
			document.documentElement.style.zoom = "";
		});
		await page.waitForTimeout(150);
	}
}

async function captureViewportSweep(page: Page, slug: string): Promise<void> {
	mkdirSync(SHOT_DIR, { recursive: true });
	const workspaceGeometry: Array<{ width: number; columns: number; sameRow: boolean }> = [];
	for (const viewport of VIEWPORTS) {
		await page.setViewportSize(viewport);
		// Let CSS breakpoints and any resize observers settle.
		await page.waitForTimeout(250);
		await assertNoHorizontalOverflow(page, `${slug} @ ${viewport.width}px`);
		if (ZOOM_WIDTHS.has(viewport.width)) {
			await assertNoOverflowAtDoubleZoom(page, `${slug} @ ${viewport.width}px`);
		}
		const canvas = page.locator(".sv-workspace-canvas").first();
		if (await canvas.count()) {
			workspaceGeometry.push(await canvas.evaluate((element, width) => {
				const children = [...element.children].slice(0, 2).map((child) => child.getBoundingClientRect());
				return {
					width,
					columns: getComputedStyle(element).gridTemplateColumns.split(" ").filter(Boolean).length,
					sameRow: children.length === 2 && Math.abs(children[0].top - children[1].top) <= 2
				};
			}, viewport.width));
		}
		await page.screenshot({
			path: join(SHOT_DIR, `${slug}-${viewport.width}.png`),
			fullPage: true
		});
	}
	if (workspaceGeometry.length) {
		const wide = workspaceGeometry.find((geometry) => geometry.width === 1440);
		const compact = workspaceGeometry.find((geometry) => geometry.width === 768);
		expect(wide, `${slug}: missing wide workspace geometry`).toMatchObject({ columns: 2, sameRow: true });
		expect(compact, `${slug}: missing compact workspace geometry`).toMatchObject({ columns: 1, sameRow: false });
	}
	await page.setViewportSize({ width: 1440, height: 900 });
}

test.describe("GEPA workspace on the real Sol run", () => {
	const solEvents = loadRunEvents(SOL_ID);
	const lunaEvents = loadRunEvents(LUNA_ID);
	test.skip(!solEvents || !lunaEvents, "real Banking77 GEPA run artifacts are not on this machine");

	test.beforeEach(async ({ page }) => {
		await page.addInitScript(
			({ sol, luna, solId, lunaId }) => {
				const makeRun = (id: string, events: unknown[]) => ({
					schemaVersion: "optimizer_run.v1",
					id,
					algorithmId: "gepa",
					algorithmVersion: "1.0.0",
					status: "completed",
					source: "local",
					objective: `Banking77 GEPA · ${id.includes("sol") ? "Sol" : "Luna"} medium`,
					createdAt: "2026-08-12T20:57:00.000Z",
					startedAt: "2026-08-12T20:57:00.000Z",
					finishedAt: "2026-08-12T21:01:30.000Z",
					cursorSeq: events.length,
					capabilities: { streamEvents: true },
					executionBindings: [],
					inputRefs: [],
					outputRefs: [],
					visualRefs: [{ kind: "visual", id: `visual-${id}` }],
					summary: {},
					usage: { rollouts: 140 }
				});
				const byId: Record<string, unknown[]> = { [solId]: sol, [lunaId]: luna };
				const runs = [makeRun(solId, sol), makeRun(lunaId, luna)];
				(window as any).synthOptimizers = {
					listAlgorithms: async () => [{ id: "gepa", title: "GEPA", availability: "available" }],
					listRecipes: async () => [],
					list: async () => runs,
					get: async (id: string) => runs.find((run: any) => run.id === id),
					refresh: async (id: string) => runs.find((run: any) => run.id === id),
					eventsAfter: async (id: string, afterSeq = 0, limit = 500) =>
						(byId[id] ?? [])
							.filter((event: any) => Number(event.sequence_number) > afterSeq)
							.slice(0, limit ?? 500),
					getState: async () => ({}),
					getStateBatch: async () => [],
					cancel: async () => runs[0],
					pause: async () => runs[0],
					resume: async () => runs[0],
					openVisual: async (id: string) => runs.find((run: any) => run.id === id),
					importLocal: async () => { throw new Error("not used"); },
					reconcileCloud: async () => runs[0],
					listCloud: async () => [],
					create: async () => runs[0],
					startRecipe: async () => runs[0],
					onEvent: () => () => undefined
				};
				(window as any).synthVisuals = {
					get: async (visualId: string) => ({
						schemaVersion: "synth.desktop-visual.v1",
						id: visualId,
						templateId: "optimizer.gepa.live.v1",
						title: "Banking77 GEPA · Sol",
						status: "saved",
						createdAt: "2026-08-12T21:02:00.000Z",
						updatedAt: "2026-08-12T21:02:00.000Z",
						bindings: {
							schemaVersion: "synth.visual-bindings.v1",
							slots: [{ slot: "optimizer_run", kind: "optimizer_run", source: visualId.replace(/^visual-/, "") }]
						},
						metadata: {}
					}),
					onEvent: () => () => undefined,
					onShow: () => () => undefined
				};
			},
			{ sol: solEvents, luna: lunaEvents, solId: SOL_ID, lunaId: LUNA_ID }
		);
		await page.reload();
		await page.getByTestId("titlebar").waitFor();
		await page.getByRole("button", { name: "Optimizers" }).click();
		await page.getByTestId(`optimizer-run-${SOL_ID}`).click();
		await page.getByTestId("open-optimizer-visual").click();
		await expect(page.getByTestId("gepa-workspace")).toBeVisible();
		await expect(page.getByTestId("workspace-status")).toContainText("Completed");
	});

	test("completed run renders truthfully and holds every breakpoint in split view", async ({ page }) => {
		const workspace = page.getByTestId("gepa-workspace");
		await expect(workspace).not.toContainText("Following live");
		await expect(page.getByTestId("workspace-headline")).toContainText("Search complete");
		await expect(page.getByTestId("gepa-run-header")).toContainText("140 / 240");
		await expect(page.getByTestId("gepa-run-header")).toContainText("gpt-5.6-sol");
		await expect(page.getByTestId("gepa-pareto-frontier")).toBeVisible();
		await expect(page.getByTestId("gepa-comparison")).toContainText("gpt-5.6-luna");
		await captureViewportSweep(page, "gepa-split");
	});

	test("expanded workspace inspects candidate, evaluations, and trace at every breakpoint", async ({ page }) => {
		await page.getByTestId("toggle-visual-expand").click();
		await expect(page.getByTestId("gepa-workbench-controls")).toBeVisible();
		await page.getByTestId("gepa-candidate-sort").selectOption("score");
		await page.getByTestId("gepa-sort-direction").selectOption("desc");
		await page.getByTestId("optimizer-candidate-gepa_d2b4f5433ce8").click();
		await expect(page.getByTestId("gepa-linked-selection")).toContainText("gepa_d2b4f5433ce8");
		await expect(page.getByTestId("gepa-selected-candidate")).toContainText("Rejected at the minibatch gate");
		await expect(page.getByTestId("gepa-candidate-content")).toBeVisible();
		await expect.poll(() => page.evaluate((runId) => window.localStorage.getItem(`synth.optimizer.gepa.presentation.v1:${runId}`), SOL_ID)).toContain('"sort":"score"');
		await page.getByTestId("eval-filter-failures").click();
		await expect(page.getByTestId("gepa-child-evaluations")).toBeVisible();
		await expect(page.getByTestId("inspect-proposer-trace-0")).toContainText("Reflection context assembled");
		await captureViewportSweep(page, "gepa-expanded");
	});
});

test.describe("Craftax semantic viewer", () => {
	test("folds deltas, keeps hierarchy, and holds every breakpoint", async ({ page }) => {
		// Keep the synthetic replay fast; this gate measures the terminal visual,
		// not transport pacing.
		test.setTimeout(180_000);
		const lane = "rollout_craftax_gate_2026_08_12";
		const comparisonLane = "rollout_craftax_gate_2026_08_12_b";
		const events: unknown[] = [];
		let seq = 0;
		const pushLane = (runId: string, kind: string, payload: Record<string, unknown> = {}) => {
			seq += 1;
			events.push({
				kind,
				sequence: seq,
				occurred_at: new Date(Date.UTC(2026, 7, 12, 20, 0, 0, seq * 15)).toISOString(),
				run_id: runId,
				payload
			});
		};
		const push = (kind: string, payload: Record<string, unknown> = {}) => pushLane(lane, kind, payload);
		push("trace.opened");
		push("observation", { readout: { env_steps: 0, observation_text: "Forest clearing", inventory: { health: 9, food: 8, drink: 7, energy: 9, wood: 2 } } });
		push("span.policy.opened", { call: { provider: "openrouter", model: "gpt-5.6-luna" } });
		for (let index = 0; index < 30; index += 1) {
			push("span.policy.data", { delta: true, channel: "reasoning", text: `token${index} ` });
		}
		push("span.policy.data", { channel: "summary", model: "gpt-5.6-luna", tool_arguments: '{"actions":["up","left"]}', usage: { prompt_tokens: 1200, completion_tokens: 260, total_tokens: 1460, cost_usd: 0.000502 } });
		push("span.policy.plan", { actions: ["up", "left"] });
		push("span.policy.closed", { length: 2 });
		push("reward_signal", { value: 1.0 });
		push("span.step.closed", { step: 0, action: "up" });
		push("frame", { step: 0, text: "frame 0" });
		push("achievement_unlocked", { achievement: "collect_wood" });
		push("span.step.closed", { step: 1, action: "left" });
		const call1FrameIndex = events.length;
		push("frame", { step: 1, text: "frame 1" });
		push("span.policy.opened", { call_number: 2, call: { provider: "openrouter", model: "gpt-5.6-luna" } });
		push("span.policy.data", { channel: "summary", reasoning: "second-call-reasoning", tool_arguments: '{"actions":["down"]}', usage: { total_tokens: 120 } });
		push("span.policy.closed", { length: 1 });
		push("span.step.closed", { step: 2, action: "down" });
		const call2FrameIndex = events.length;
		push("frame", { step: 2, text: "frame 2" });
		push("trace.reconciled", { digest: "d".repeat(64) });
		pushLane(comparisonLane, "trace.opened");
		pushLane(comparisonLane, "snapshot", { step: 0, total_reward: 0 });
		pushLane(comparisonLane, "snapshot", { step: 2, total_reward: 2, achievements: { collect_stone: 1 } });
		pushLane(comparisonLane, "trace.reconciled", { digest: "e".repeat(64) });

		const visual = {
			schemaVersion: "synth.desktop-visual.v1",
			id: "vis_craftax_gate",
			currentRevision: 1,
			title: "Craftax responsive gate",
			templateId: "live.craftax.v1",
			status: "saved",
			rendererKind: "template",
			bindings: {
				schemaVersion: "synth.visual-bindings.v1",
				slots: [{
					slot: "stream",
					kind: "inline",
					data: {
						events,
						replay_ms: 1,
						scope: { campaign_id: "campaign_gate", rollout_ids: [lane, comparisonLane], selection: { initial_rollout_id: lane } }
					}
				}]
			},
			sessionId: null,
			messageId: null,
			runId: null,
			traceId: null,
			parentVisualId: null,
			sourceAgentId: "gate",
			sourceModel: "gate",
			contentDigest: null,
			previewDigest: null,
			metadata: {},
			createdAt: "2026-08-12T21:00:00Z",
			updatedAt: "2026-08-12T21:00:00Z"
		};
		await page.addInitScript((record) => {
			(window as any).synthVisuals = {
				listTemplates: async () => [{ id: "live.craftax.v1", title: "Craftax live", genre: "live" }],
				getTemplate: async (templateId: string) => ({ id: templateId, title: templateId }),
				list: async () => [record],
				get: async () => record,
				revisions: async () => [],
				create: async () => record,
				update: async () => record,
				save: async () => record,
				fork: async () => record,
				archive: async () => record,
				show: async () => record,
				onEvent: () => () => undefined,
				onShow: () => () => undefined
			};
		}, visual);
		await page.reload();
		await page.getByTestId("titlebar").waitFor();
		await page.getByTestId("open-visuals").click();
		await page.getByTestId("visuals-card-vis_craftax_gate").getByRole("button", { name: "Open" }).click();
		// The template also renders in the gallery preview; measure the pane instance.
		const viewer = page.getByTestId("visual-pane").getByTestId("visual-live-craftax");
		await expect(viewer).toBeVisible();
		// Fixture replay is interval-based; assert the terminal contract rather
		// than an older copy label that no longer names the transport state.
		await expect(viewer).toHaveAttribute("data-visual-terminal", "true", { timeout: 90_000 });
		await expect(viewer.locator(".cv-topbar h2")).toHaveCSS("color", "rgb(244, 238, 230)");
		await expect(viewer.locator(".cv-topbar .cv-eyebrow")).toHaveCSS("color", "rgb(255, 106, 42)");
		const aggregateTimeline = viewer.getByTestId("craftax-aggregate-timeline");
		expect(await viewer.evaluate((root) => {
			const overview = root.querySelector('[data-visual-landmark="run-overview"]');
			const aggregate = root.querySelector('[data-testid="craftax-aggregate-timeline"]');
			return Boolean(overview && aggregate && (aggregate.compareDocumentPosition(overview) & Node.DOCUMENT_POSITION_FOLLOWING));
		}), "aggregate outcomes should precede the run overview near the top").toBe(true);
		await expect(aggregateTimeline.locator(".cv-rollout-line")).toHaveCount(2);
		await expect(aggregateTimeline.locator(".cv-achievement-marker")).toHaveCount(2);
		await expect(aggregateTimeline).toContainText("🪵");
		mkdirSync(SHOT_DIR, { recursive: true });
		await aggregateTimeline.screenshot({ path: join(SHOT_DIR, "craftax-aggregate-timeline.png") });

		// One folded policy-call row, not thirty token rows.
		const traceButtons = viewer.locator(".cv-trace li button");
		const rowCount = await traceButtons.count();
		expect(rowCount, `trace rows should be folded, got ${rowCount}`).toBeLessThan(20);
		// Transcript renders one normalized call card while preserving every
		// delta under expandable Trace V5 evidence.
		await viewer.getByRole("button", { name: "Agent transcript", exact: true }).click();
		await expect(viewer.locator(".cv-call-list > li")).toHaveCount(2);
		await expect(viewer.getByRole("heading", { name: "Agent transcript" })).toBeVisible();
		await viewer.getByRole("button", { name: "Focus", exact: true }).click();
		await expect(viewer.locator(".cv-call-list button[aria-current=true]")).toContainText("Call 1");
		await expect(viewer.getByText("Raw Trace V5 evidence (34 envelopes)")).toHaveCount(1);
		await expect(viewer).toContainText("Step 0");
		await expect(viewer).toContainText("collect_wood");

		await viewer.getByRole("button", { name: "Replay", exact: true }).click();
		const frameCallPanel = viewer.getByTestId("craftax-frame-call-panel");
		await expect(frameCallPanel).toBeVisible();
		await expect(frameCallPanel).toContainText("Call 2");
		await expect(frameCallPanel).toContainText("second-call-reasoning");
		const rawEventSlider = viewer.getByRole("slider", { name: "Replay selected rollout by raw event" });
		await rawEventSlider.fill(String(call1FrameIndex));
		await expect(frameCallPanel).toContainText("Call 1");
		await expect(frameCallPanel).toContainText("Policy reasoning");
		await expect(frameCallPanel).toContainText("token0");
		await expect(frameCallPanel).toContainText("Tool calls");
		await expect(frameCallPanel).toContainText('"actions":["up","left"]');
		await rawEventSlider.fill(String(call2FrameIndex));
		await expect(frameCallPanel).toContainText("Call 2");
		await expect(frameCallPanel).toContainText('"actions":["down"]');
		const selectedAchievementTimeline = viewer.getByTestId("craftax-selected-achievement-timeline");
		await expect(selectedAchievementTimeline.locator(".cv-selected-achievement-marker")).toHaveCount(1);
		await expect(selectedAchievementTimeline).toContainText("🪵");
		await expect(selectedAchievementTimeline).toContainText("collect wood");
		await page.getByTestId("toggle-visual-expand").click();
		await captureViewportSweep(page, "craftax");
		await aggregateTimeline.screenshot({ path: join(SHOT_DIR, "craftax-aggregate-timeline-wide.png") });
	});
});
