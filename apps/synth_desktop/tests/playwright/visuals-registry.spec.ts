import { expect, test } from "./browser.fixture";
import type { Page } from "@playwright/test";
import type { VisualRecord } from "@synth/runtime-protocol";
import { readFileSync } from "node:fs";

const sampleVisual: VisualRecord = {
	schemaVersion: "synth.desktop-visual.v1",
	id: "vis_test_reward",
	currentRevision: 1,
	title: "Reward breakdown",
	templateId: "reward.breakdown.v1",
	status: "saved",
	rendererKind: "template",
	bindings: { steps: [0, 1, 2], rewards: [0.1, 0.4, 0.9] },
	sessionId: null,
	messageId: null,
	runId: null,
	traceId: null,
	parentVisualId: null,
	sourceAgentId: "test",
	sourceModel: "laguna-xs-2.1",
	contentDigest: null,
	previewDigest: null,
	metadata: {},
	createdAt: "2026-08-08T20:00:02Z",
	updatedAt: "2026-08-08T20:00:02Z"
};

async function installVisualsFixture(page: Page, visuals: VisualRecord[] = [sampleVisual]): Promise<void> {
	await page.addInitScript((rows) => {
		const store = [...rows] as VisualRecord[];
		(window as typeof window & { synthVisuals?: unknown }).synthVisuals = {
			listTemplates: async () => [{ id: "reward.breakdown.v1", title: "Reward breakdown", genre: "reward" }],
			getTemplate: async (templateId: string) => ({ id: templateId, title: templateId }),
			list: async () => store,
			get: async (visualId: string) => {
				const hit = store.find((row) => row.id === visualId);
				if (!hit) throw new Error(`missing visual ${visualId}`);
				return hit;
			},
			revisions: async () => [],
			create: async (request: { templateId: string; title?: string }) => {
				const created = {
					...store[0],
					id: `vis_${store.length + 1}`,
					templateId: request.templateId,
					title: request.title ?? "New visual",
					status: "draft" as const,
					currentRevision: 1
				};
				store.unshift(created);
				return created;
			},
			update: async () => store[0],
			save: async () => ({ ...store[0], status: "saved" as const }),
			fork: async () => ({ ...store[0], id: "vis_fork", title: "Fork" }),
			archive: async () => ({ ...store[0], status: "archived" as const }),
			show: async (visualId: string) => {
				const hit = store.find((row) => row.id === visualId);
				if (!hit) throw new Error(`missing visual ${visualId}`);
				return hit;
			},
			onEvent: () => () => undefined,
			onShow: () => () => undefined
		};
	}, visuals);
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
}

test("Visuals library lists a saved visual by visual_id", async ({ page }) => {
	await installVisualsFixture(page);
	await page.getByTestId("open-visuals").click();
	await expect(page.getByTestId("visuals-page")).toBeVisible();
	await expect(page.getByTestId("visuals-card-vis_test_reward")).toBeVisible();
	await expect(page.getByTestId("visuals-card-vis_test_reward")).toContainText("Reward breakdown");
});

test("chat visual card, registry, and right pane resolve one visual_id", async ({ page }) => {
	await installVisualsFixture(page);
	await page.getByTestId("open-visuals").click();
	await page.getByTestId("visuals-card-vis_test_reward").getByRole("button", { name: "Open" }).click();
	await expect(page.getByTestId("visual-pane")).toBeVisible();
	await expect(page.getByTestId("visual-pane")).toContainText("Reward breakdown");
	await expect(page.getByTestId("visuals-preview")).toBeVisible();
});

test("Visuals page can create a draft visual from the registry", async ({ page }) => {
	await installVisualsFixture(page, []);
	await page.getByTestId("open-visuals").click();
	await page.getByTestId("visuals-new").click();
	await expect(page.getByTestId("visual-pane")).toBeVisible();
	await expect(page.getByTestId("visuals-grid")).toContainText("New visual");
});

test("an already-open pane rejects stale gets and reconciles a dropped final update on focus", async ({ page }) => {
	await page.addInitScript((base) => {
		let current = { ...base, id: "vis_race", title: "Revision 13", currentRevision: 13 };
		const listeners = new Set<(event: Record<string, unknown>) => void>();
		let releaseOld: ((value: VisualRecord) => void) | null = null;
		let gets = 0;
		(window as typeof window & { synthVisuals?: unknown }).synthVisuals = {
			listTemplates: async () => [],
			list: async () => [],
			get: async () => {
				gets += 1;
				if (gets === 1) return new Promise<VisualRecord>((resolve) => { releaseOld = resolve; });
				return current;
			},
			revisions: async () => [],
			onEvent: (next: (event: Record<string, unknown>) => void, attached?: () => void) => {
				listeners.add(next);
				queueMicrotask(() => attached?.());
				return () => { listeners.delete(next); };
			}
		};
		(window as typeof window & { __visualRace?: unknown }).__visualRace = {
			isAttached: () => listeners.size > 0,
			getCount: () => gets,
			show13: () => listeners.forEach((listener) => listener({ kind: "visual.show", payload: { visualId: "vis_race", revision: 13 } })),
			update14: () => {
				current = { ...current, title: "Revision 14", currentRevision: 14 };
				listeners.forEach((listener) => listener({ kind: "visual.updated", payload: { visualId: "vis_race", revision: 14 } }));
			},
			drop15: () => { current = { ...current, title: "Revision 15", currentRevision: 15 }; },
			release13: () => releaseOld?.({ ...current, title: "Revision 13", currentRevision: 13 }),
			emit15: () => listeners.forEach((listener) => listener({ kind: "visual.updated", payload: { visualId: "vis_race", revision: 15 } }))
		};
	}, sampleVisual);
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await expect.poll(() => page.evaluate(() => (window as typeof window & { __visualRace: { isAttached(): boolean } }).__visualRace.isAttached())).toBe(true);
	await page.getByTestId("open-visuals").click();
	await expect.poll(() => page.evaluate(() => (window as typeof window & { __visualRace: { isAttached(): boolean } }).__visualRace.isAttached())).toBe(true);
	await page.evaluate(() => (window as typeof window & { __visualRace: { show13(): void; update14(): void } }).__visualRace.show13());
	await page.evaluate(() => (window as typeof window & { __visualRace: { show13(): void; update14(): void } }).__visualRace.update14());
	await expect.poll(() => page.evaluate(() => (window as typeof window & { __visualRace: { getCount(): number } }).__visualRace.getCount())).toBe(2);
	await expect.poll(() => page.evaluate(() => window.__synthEval?.getState().openVisualId)).toBe("vis_race");
	await expect(page.getByTestId("visual-pane")).toContainText("Revision 14");
	await expect(page.getByTestId("visual-pane")).toContainText("rev 14");
	await page.evaluate(() => (window as typeof window & { __visualRace: { release13(): void } }).__visualRace.release13());
	await expect(page.getByTestId("visual-pane")).toContainText("Revision 14");

	await page.evaluate(() => {
		(window as typeof window & { __visualRace: { drop15(): void } }).__visualRace.drop15();
		window.dispatchEvent(new Event("focus"));
	});
	await expect(page.getByTestId("visual-pane")).toContainText("Revision 15");
	await page.getByRole("button", { name: "Close visual" }).click();
	await page.evaluate(() => (window as typeof window & { __visualRace: { emit15(): void } }).__visualRace.emit15());
	await expect(page.getByTestId("visual-pane")).toBeHidden();
});

test("an owned visual.show does not steal another chat's right pane", async ({ page }) => {
	await page.addInitScript((rows) => {
		const store = [...rows] as VisualRecord[];
		const listeners = new Set<(event: Record<string, unknown>) => void>();
		(window as typeof window & { synthVisuals?: unknown }).synthVisuals = {
			listTemplates: async () => [],
			list: async () => store,
			get: async (visualId: string) => {
				const hit = store.find((row) => row.id === visualId);
				if (!hit) throw new Error(`missing visual ${visualId}`);
				return hit;
			},
			revisions: async () => [],
			onEvent: (next: (event: Record<string, unknown>) => void, attached?: () => void) => {
				listeners.add(next);
				queueMicrotask(() => attached?.());
				return () => { listeners.delete(next); };
			}
		};
		(window as typeof window & { __visualOwner?: unknown }).__visualOwner = {
			show: (visualId: string, ownerSessionId?: string) => {
				listeners.forEach((listener) => listener({
					kind: "visual.show",
					payload: { visualId, revision: 1, ...(ownerSessionId ? { ownerSessionId } : {}) }
				}));
			}
		};
	}, [sampleVisual, { ...sampleVisual, id: "vis_healthbench", title: "HealthBench smoke" }]);
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await expect.poll(() => page.evaluate(() => (window as typeof window & { __visualOwner: { show(visualId: string, ownerSessionId?: string): void } }).__visualOwner !== undefined)).toBeTruthy();
	await page.evaluate(() => (window as typeof window & { __visualOwner: { show(visualId: string, ownerSessionId?: string): void } }).__visualOwner.show("vis_test_reward", "chat-banking77"));
	await expect.poll(() => page.evaluate(() => window.__synthEval?.getState().openVisualId ?? null)).toBe(null);
	await expect(page.getByTestId("visual-pane")).toBeHidden();
	await page.evaluate(() => (window as typeof window & { __visualOwner: { show(visualId: string, ownerSessionId?: string): void } }).__visualOwner.show("vis_healthbench"));
	await expect.poll(() => page.evaluate(() => window.__synthEval?.getState().openVisualId)).toBe("vis_healthbench");
	await expect(page.getByTestId("visual-pane")).toContainText("HealthBench smoke");
});

test("Visuals list splitter resizes, persists, keyboard-clamps, and disappears when stacked", async ({ page }) => {
	await page.setViewportSize({ width: 1280, height: 840 });
	await installVisualsFixture(page);
	await page.getByTestId("open-visuals").click();
	const splitter = page.getByTestId("visuals-resize-handle");
	await expect(splitter).toBeVisible();
	await expect(splitter).toHaveAttribute("role", "separator");
	await expect(splitter).toHaveAttribute("aria-orientation", "vertical");
	const before = Number(await splitter.getAttribute("aria-valuenow"));
	const box = await splitter.boundingBox();
	if (!box) throw new Error("Visuals splitter geometry unavailable");
	await page.mouse.move(box.x + box.width / 2, box.y + 80);
	await page.mouse.down();
	await page.mouse.move(box.x + 72, box.y + 80, { steps: 4 });
	await page.mouse.up();
	const dragged = Number(await splitter.getAttribute("aria-valuenow"));
	expect(dragged).toBeGreaterThan(before + 40);
	await splitter.focus();
	await page.keyboard.press("Shift+ArrowLeft");
	const keyboard = Number(await splitter.getAttribute("aria-valuenow"));
	expect(keyboard).toBe(dragged - 64);
	await page.reload();
	await page.getByTestId("open-visuals").click();
	await expect(page.getByTestId("visuals-resize-handle")).toHaveAttribute("aria-valuenow", String(keyboard));
	await page.setViewportSize({ width: 720, height: 840 });
	await expect(page.getByTestId("visuals-resize-handle")).toBeHidden();
	expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBe(true);
	await page.setViewportSize({ width: 1280, height: 840 });
	await expect(page.getByTestId("visuals-resize-handle")).toBeVisible();
	expect(Number(await page.getByTestId("visuals-resize-handle").getAttribute("aria-valuenow"))).toBe(keyboard);
});

test("Trace V5 inspector provides focus, full, evidence, and expandable output views", async ({ page }) => {
	const traceVisual: VisualRecord = {
		...sampleVisual,
		id: "tracevis_test",
		title: "Laguna · edit-json · Harbor",
		templateId: "trace.rollout_inspector.v1",
		traceId: "trace_test",
		bindings: {
			schemaVersion: "synth.visual-bindings.v1",
			slots: [{
				slot: "projection", kind: "inline", schema: "synth.trace-projection.rollout-inspector.v1",
				data: {
					schema_version: "synth.trace-projection.rollout-inspector.v1",
					trace_id: "trace_test", trace_digest: "sha256:abc123", evidence_digest: "sha256:evidence",
					visual: {
						run_id: "run_test", task_id: "harborcoding/edit-json", state: "sealed", visibility_ceiling: "private",
						summary: { visual_item_count: 4 }, usage: { requests: 1, prompt_tokens: 1200, completion_tokens: 80, cached_tokens: 400, provenance: "derived" },
						lanes: [{ lane_id: "lane-1", display_name: "harbor-codex", actor_kind: "agent", detail: { status: "completed", coverage: { tool_events: "complete", usage: "partial" } } }],
						items: [
							{ item_id: "model", kind: "model_call.started", sequence: 1, lane_id: "lane-1", occurred_at: "2026-08-09T10:00:00Z", status: "ok", detail: { call_index: 1 } },
							{ item_id: "message", kind: "codex.agent_message", sequence: 2, lane_id: "lane-1", occurred_at: "2026-08-09T10:00:01Z", status: "ok", detail: { native: { text: "I’ll update the configuration." } } },
							{ item_id: "tool", kind: "codex.command_finished", sequence: 3, lane_id: "lane-1", occurred_at: "2026-08-09T10:00:02Z", status: "ok", detail: { native: { command: "jq . config.json", aggregated_output: '{\n  "enabled": true\n}' } } },
							{ item_id: "judge", kind: "evidence.judgment", sequence: 4, occurred_at: "2026-08-09T10:00:03Z", status: "decisive", title: "pass", detail: { passed: true, score: 1, rationale: "Verifier passed." } }
						]
					}
				}
			}]
		}
	};
	await installVisualsFixture(page, [traceVisual]);
	await page.getByTestId("open-visuals").click();
	await page.getByTestId("visuals-card-tracevis_test").getByRole("button", { name: "Open" }).click();
	const pane = page.getByTestId("visual-pane");
	await expect(pane.getByTestId("visual-trace-rollout-inspector")).toBeVisible();
	await expect(pane).toContainText("I’ll update the configuration.");
	await expect(pane).not.toContainText("Model call 1");
	await pane.getByRole("button", { name: "full" }).click();
	await expect(pane).toContainText("Model call 1");
	await pane.getByRole("button", { name: "evidence" }).click();
	await expect(pane).toContainText("Verifier passed.");
	await pane.getByRole("button", { name: "metadata" }).click();
	await expect(pane).toContainText("sha256:abc123");
	await expect(pane).toContainText("tool_events · complete");
});

test("Trace V5 inspector renders canonical Craftax rewards, usage, achievements, and aligned actions", async ({ page }) => {
	const actions = (names: string[]) => names.map((action, index) => ({
		step: index + 1,
		action,
		transition: "applied",
		reason: "policy_plan"
	}));
	const projectionPath = process.env.SYNTH_CRAFTAX_PROJECTION;
	const realProjection = projectionPath
		? (JSON.parse(readFileSync(projectionPath, "utf8")) as { payload: Record<string, unknown> }).payload
		: null;
	const craftaxVisual: VisualRecord = {
		...sampleVisual,
		id: "tracevis_craftax",
		title: "Craftax · Luna low vs high · canonical V5",
		templateId: "trace.rollout_inspector.v1",
		traceId: "trace_craftax",
		bindings: {
			schemaVersion: "synth.visual-bindings.v1",
			slots: [{
				slot: "projection", kind: "inline", schema: "synth.trace-projection.rollout-inspector.v1",
				data: realProjection ?? {
					schema_version: "synth.trace-projection.rollout-inspector.v1",
					trace_id: "trace_craftax", trace_digest: "sha256:craftax",
					visual: {
						state: "sealed", visibility_ceiling: "private",
						summary: {
							visual_item_count: 68,
							craftax: {
								schema_version: "synth.trace-extension.craftax.v1", paired: true,
								rollouts: [
									{ lane: "high", rollout_id: "high-1", model: "gpt-5.6-luna", reasoning_effort: "high", reward: 4, env_steps: 3, usage: { total_tokens: 23227, calls: 4, estimated_usd: 0.00965712 }, achievements: [{ step: 1, name: "collect_wood" }, { step: 3, name: "make_wood_pickaxe" }], actions: actions(["MOVE_NORTH", "DO", "MAKE_WOOD_PICKAXE"]) },
									{ lane: "low", rollout_id: "low-1", model: "gpt-5.6-luna", reasoning_effort: "low", reward: 3, env_steps: 3, usage: { total_tokens: 18275, calls: 4, estimated_usd: 0.00381172 }, achievements: [{ step: 1, name: "collect_wood" }], actions: actions(["MOVE_NORTH", "MOVE_WEST", "PLACE_TABLE"]) }
								]
							}
						},
						usage: { requests: 8, prompt_tokens: 32573, completion_tokens: 8929, cached_tokens: 20892, cost_usd: 0.01346884, provenance: "partial" },
						lanes: [
							{ lane_id: "high", display_name: "gpt-5.6-luna · high", actor_kind: "agent", detail: { status: "completed", coverage: { model_calls: "complete", environment_events: "complete", usage: "complete" } } },
							{ lane_id: "low", display_name: "gpt-5.6-luna · low", actor_kind: "agent", detail: { status: "completed", coverage: { model_calls: "complete", environment_events: "complete", usage: "complete" } } }
						],
						items: []
					}
				}
			}]
		}
	};
	await installVisualsFixture(page, [craftaxVisual]);
	await page.getByTestId("open-visuals").click();
	await page.getByTestId("visuals-card-tracevis_craftax").getByRole("button", { name: "Open" }).click();
	const comparison = page.getByTestId("visual-pane").getByTestId("craftax-policy-comparison");
	await expect(comparison).toBeVisible();
	await expect(comparison).toContainText("4");
	await expect(comparison).toContainText("23,227");
	await expect(comparison).toContainText("$0.0097");
	await expect(comparison).toContainText("make_wood_pickaxe");
	await expect(comparison).toContainText("Aligned action trace");
	await expect(comparison).toContainText("wood pickaxe");
	if (process.env.SYNTH_CRAFTAX_SCREENSHOT) {
		await page.screenshot({ path: process.env.SYNTH_CRAFTAX_SCREENSHOT, fullPage: true });
	}
});
