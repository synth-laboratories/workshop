import { expect, test } from "./browser.fixture";

test.beforeEach(async ({ page }) => {
	await page.addInitScript(() => {
		const now = "2026-08-09T16:00:00.000Z";
		let runs: any[] = [];
		const makeRun = (id: string, algorithmId = "gepa") => ({
			schemaVersion: "optimizer_run.v1",
			id,
			algorithmId,
			algorithmVersion: "1.0.0",
			status: "queued",
			source: "cloud",
			objective: "Banking77 intent prompt · bounded GEPA smoke",
			createdAt: now,
			startedAt: null,
			finishedAt: null,
			cursorSeq: 1,
			capabilities: { cancel: true, streamEvents: true, stateSlices: true, candidates: true, localSlotBinding: true },
			executionBindings: [{ kind: "container", id: "banking77-local", label: "Banking77 local container", status: "starting" }],
			inputRefs: [{ kind: "dataset", id: "banking77", role: "train" }],
			outputRefs: [],
			visualRefs: [{ kind: "visual", id: `visual-${id}` }],
			summary: { bestScore: null },
			usage: { costUsd: 0, rollouts: 0 }
		});
		(window as any).__optimizerCreateCount = 0;
		(window as any).__setOptimizerRuns = (next: any[]) => { runs = next; };
		(window as any).prompt = () => { throw new Error("window.prompt must not be used"); };
		(window as any).synthOptimizers = {
			listAlgorithms: async () => [{ id: "gepa", title: "GEPA", availability: "available" }],
			list: async () => runs,
			get: async (id: string) => runs.find((run) => run.id === id) ?? makeRun(id),
			create: async (request: any) => {
				const run = makeRun(request.id ?? "banking77-smoke"); runs = [run]; return run;
			},
			listRecipes: async () => [
				{ id: "gepa.banking77.smoke.v1", title: "Banking77 GEPA smoke", availability: "available", limits: { maxTotalRollouts: 8, maxCostUsd: 0.25 } },
				{ id: "sft.craftax.gpt-oss.smoke.v1", title: "Craftax GPT-OSS SFT smoke", availability: "available", limits: { maxTotalEnvironmentRollouts: 8, trainSteps: 4 } }
			],
			startRecipe: async (request: any) => {
				(window as any).__optimizerCreateCount += 1;
				(window as any).__optimizerCreateRequest = request;
				const isSft = request.recipeId === "sft.craftax.gpt-oss.smoke.v1";
				const run = makeRun(isSft ? "craftax_sft_cua_smoke" : "banking77_cua_smoke", isSft ? "sft" : "gepa");
				runs = [run];
				return run;
			},
			refresh: async (id: string) => runs.find((run) => run.id === id),
			eventsAfter: async (id: string) => [{
				schemaVersion: "optimizer_event.v1", eventId: `${id}:1`, type: "run.queued",
				sequenceNumber: 1, occurredAt: now, optimizerRunId: id, algorithmId: "gepa",
				delta: { status: "queued", message: "Banking77 smoke queued" }
			}],
			getState: async () => ({}), getStateBatch: async () => [],
			cancel: async (id: string) => runs.find((run) => run.id === id),
			pause: async (id: string) => runs.find((run) => run.id === id),
			resume: async (id: string) => runs.find((run) => run.id === id),
			openVisual: async (id: string) => runs.find((run) => run.id === id),
			importLocal: async () => { throw new Error("not used"); },
			reconcileCloud: async ({ optimizerRunId }: any) => runs.find((run) => run.id === optimizerRunId),
			listCloud: async () => [],
			onEvent: () => () => undefined
		};
		(window as any).synthVisuals = {
			get: async (visualId: string) => {
				const runId = visualId.replace(/^visual-/, "");
				return {
					schemaVersion: "synth.desktop-visual.v1", id: visualId, templateId: "optimizer.run.v1",
					title: "Banking77 GEPA smoke", status: "saved", createdAt: now, updatedAt: now,
					bindings: { schemaVersion: "synth.visual-bindings.v1", slots: [{ slot: "optimizer_run", kind: "optimizer_run", source: runId }] },
					metadata: {}
				};
			},
			onEvent: () => () => undefined,
			onShow: () => () => undefined
		};
	});
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByRole("button", { name: "Optimizers" }).click();
});

test("failed local recipes surface bounded stderr diagnostics and log paths", async ({ page }) => {
	await page.evaluate(() => {
		const failed = {
			schemaVersion: "optimizer_run.v1", id: "banking77_failed", algorithmId: "gepa", status: "failed", source: "local",
			objective: "Banking77 failed smoke", createdAt: "2026-08-09T16:00:00.000Z", cursorSeq: 3,
			capabilities: {}, executionBindings: [], inputRefs: [], outputRefs: [], visualRefs: [], usage: {},
			summary: { runDirectory: "/tmp/banking77_failed" },
			error: {
				message: "configuration error",
				stderrTail: "error: synth_optimizer_config_error: configuration error: gepa.rollout_estimated_cost_usd is required and must be positive when the corresponding hard limit is set",
				logPath: "/tmp/banking77_failed/workshop.stderr.log"
			}
		};
		(window as any).__setOptimizerRuns([failed]);
	});
	await page.getByTestId("optimizers-search").fill("failed");
	await expect(page.getByTestId("optimizer-diagnostic")).toContainText("Missing rollout cost estimate");
	await expect(page.getByTestId("optimizer-diagnostic")).toContainText("rejected this recipe before compute started");
	await expect(page.getByTestId("optimizer-diagnostic")).toContainText("gepa.rollout_estimated_cost_usd");
	await expect(page.getByTestId("optimizer-stderr-tail")).toContainText("configuration error");
	await expect(page.getByText("Show technical details")).toBeVisible();
	await expect(page.getByTestId("optimizer-diagnostic")).toContainText("workshop.stderr.log");
	await expect(page.getByTestId("optimizer-run-files")).toBeVisible();
});

test("native GEPA candidates, frontier, usage, and artifacts render in the visual", async ({ page }) => {
	await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
	await page.evaluate(() => {
		const run = {
			schemaVersion: "optimizer_run.v1", id: "banking77_rich", algorithmId: "gepa", status: "completed", source: "local",
			objective: "Banking77 rich smoke", createdAt: "2026-08-09T16:00:00.000Z", cursorSeq: 4,
			capabilities: { streamEvents: true }, executionBindings: [], inputRefs: [], outputRefs: [],
			visualRefs: [{ kind: "visual", id: "visual-banking77_rich" }], summary: { runDirectory: "/tmp/banking77_rich" },
			usage: { promptTokens: 100, completionTokens: 5, rollouts: 4 }
		};
		(window as any).__setOptimizerRuns([run]);
		(window as any).synthOptimizers.eventsAfter = async () => [
			{ type: "candidate.evaluated", sequenceNumber: 1, occurredAt: "2026-08-09T16:00:01Z", optimizerRunId: run.id, algorithmId: "gepa", item: { id: "cand_seed", status: "evaluated", raw: { values: { stage2_system: "Return exactly one Banking77 intent label." } } }, delta: { train_reward: 0.5, message: "Seed evaluated" } },
			{ type: "frontier.updated", sequenceNumber: 2, occurredAt: "2026-08-09T16:00:02Z", optimizerRunId: run.id, algorithmId: "gepa", snapshot: { bestScore: 0.5, cells: [{ candidateId: "cand_seed", quality: 0.5, costUsd: 0, accent: true }] }, delta: { message: "Frontier updated" } },
			{ type: "runtime.job.completed", sequenceNumber: 3, occurredAt: "2026-08-09T16:00:03Z", optimizerRunId: run.id, algorithmId: "gepa", usageDelta: { prompt_tokens: 100, completion_tokens: 5, rollouts: 4, wall_time_ms: 2500 }, delta: { message: "Rollouts completed" } },
			{ type: "optimizer.recipe.artifacts", sequenceNumber: 4, occurredAt: "2026-08-09T16:00:04Z", optimizerRunId: run.id, algorithmId: "gepa", artifactRefs: [{ kind: "manifest", id: "/tmp/result_manifest.json" }], delta: { status: "completed", message: "Artifacts persisted" } }
		];
	});
	await page.getByTestId("optimizers-search").fill("rich");
	await page.getByTestId("open-optimizer-visual").click();
	await expect(page.getByTestId("optimizer-candidate-cand_seed")).toContainText("0.50");
	await page.getByTestId("optimizer-candidate-cand_seed").click();
	await expect(page.getByTestId("gepa-pareto-frontier")).toContainText("cand_seed");
	await expect(page.getByLabel("Usage")).toContainText("4");
	await expect(page.getByLabel("Usage")).toContainText("105");
	await expect(page.getByLabel("Artifacts")).toContainText("result_manifest.json");
	await expect(page.getByTestId("optimizer-artifact-0")).toContainText("Result manifest");
	await expect(page.getByTestId("optimizer-artifact-0")).not.toContainText('{"kind"');
	await page.getByTestId("copy-artifact-path-0").click();
	await expect(page.getByTestId("copy-artifact-path-0")).toHaveText("Copied");
	await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe("/tmp/result_manifest.json");
	await expect(page.getByTestId("gepa-candidate-content")).toContainText("Return exactly one Banking77 intent label.");
	await page.getByTestId("copy-gepa-candidate").click();
	await expect(page.getByTestId("copy-gepa-candidate")).toHaveText("Copied");
	await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe("Return exactly one Banking77 intent label.");
	const downloadPromise = page.waitForEvent("download");
	await page.getByTestId("download-gepa-candidate").click();
	const download = await downloadPromise;
	expect(download.suggestedFilename()).toBe("cand_seed.json");
});

test("CUA can review and explicitly start the bounded Banking77 GEPA recipe", async ({ page }) => {
	await expect(page.getByTestId("banking77-gepa-launch-card")).toContainText("at most 8 rollouts");
	expect(await page.evaluate(() => (window as any).__optimizerCreateCount)).toBe(0);

	await page.getByTestId("configure-banking77-gepa-smoke").click();
	const dialog = page.getByTestId("banking77-gepa-launch-dialog");
	await expect(dialog).toBeVisible();
	await expect(page.getByTestId("banking77-gepa-bounds")).toContainText("$0.25");
	await expect(page.getByTestId("start-banking77-gepa-smoke")).toBeDisabled();
	await page.getByTestId("confirm-banking77-gepa-cost").check();
	await page.getByTestId("start-banking77-gepa-smoke").click();

	await expect(dialog).toHaveCount(0);
	await expect(page.getByTestId("optimizer-run-banking77_cua_smoke")).toBeVisible();
	await expect(page.getByTestId("optimizer-execution-mode")).toContainText("Banking77 local container");
	const request = await page.evaluate(() => (window as any).__optimizerCreateRequest);
	expect(request.recipeId).toBe("gepa.banking77.smoke.v1");
	expect(request.openVisual).toBe(true);

	await expect(page.getByTestId("visual-pane")).toBeVisible();
	await expect(page.getByTestId("visual-optimizer-run")).toBeVisible();
	await expect(page.getByTestId("optimizer-event-log")).toContainText("run.queued");
});

test("CUA can review and explicitly start the bounded Craftax SFT recipe", async ({ page }) => {
	await expect(page.getByTestId("craftax-sft-launch-card")).toContainText("4 LoRA steps");
	expect(await page.evaluate(() => (window as any).__optimizerCreateCount)).toBe(0);

	await page.getByTestId("configure-craftax-sft-smoke").click();
	const dialog = page.getByTestId("craftax-sft-launch-dialog");
	await expect(dialog).toContainText("GPT-OSS-120B via Groq");
	await expect(dialog).toContainText("bounded by rollouts and steps, not by a dollar ceiling");
	await expect(page.getByTestId("start-craftax-sft-smoke")).toBeDisabled();
	await page.getByTestId("confirm-craftax-sft-compute").check();
	await page.getByTestId("start-craftax-sft-smoke").click();

	await expect(dialog).toHaveCount(0);
	await expect(page.getByTestId("optimizer-run-craftax_sft_cua_smoke")).toBeVisible();
	const request = await page.evaluate(() => (window as any).__optimizerCreateRequest);
	expect(request).toEqual({ recipeId: "sft.craftax.gpt-oss.smoke.v1", openVisual: true });
});

test("an unresolved live optimizer binding is honest and never renders GEPA demo candidates", async ({ page }) => {
	await page.evaluate(() => {
		(window as any).synthOptimizers.get = async () => { throw new Error("run is offline"); };
		(window as any).synthOptimizers.eventsAfter = async () => { throw new Error("run is offline"); };
		(window as any).synthOptimizers.startRecipe = async () => ({
			schemaVersion: "optimizer_run.v1", id: "banking77_offline", algorithmId: "gepa", status: "queued", source: "local_recipe",
			objective: "Banking77", createdAt: new Date().toISOString(), cursorSeq: 0, capabilities: {}, executionBindings: [],
			inputRefs: [], outputRefs: [], visualRefs: [{ kind: "visual", id: "visual-banking77_offline" }], summary: {}, usage: {}
		});
	});
	await page.getByTestId("configure-banking77-gepa-smoke").click();
	await page.getByTestId("confirm-banking77-gepa-cost").check();
	await page.getByTestId("start-banking77-gepa-smoke").click();
	await expect(page.getByTestId("optimizer-run-unavailable")).toContainText("run is offline");
	await expect(page.getByTestId("optimizer-candidate-cand_seed")).toHaveCount(0);
});
