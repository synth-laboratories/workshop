import { openOptimizer } from "./v02-helpers";
import { expect, test } from "./browser.fixture";

test.beforeEach(async ({ page }) => {
	await page.addInitScript(() => {
		(window as any).synthTrainingArtifacts = {
			list: async () => [],
			get: async () => { throw new Error("artifact not found"); },
			delete: async () => undefined,
			export: async () => { throw new Error("real export runtime unavailable"); },
			launchInference: async () => { throw new Error("real inference runtime unavailable"); },
			launchEval: async () => { throw new Error("real evaluation runtime unavailable"); }
		};
		(window as any).synthTrainingModels = {
			listModels: async () => [],
			downloadModel: async () => { throw new Error("model download unavailable"); },
			deleteModel: async () => undefined,
			onDownloadProgress: () => () => undefined
		};
		(window as any).synthInventory = {
			listContainers: async () => [{
				id: "ctr-alfworld-cleanroom",
				name: "ALFWorld cleanroom",
				taskFamily: "alfworld.text.v1",
				status: "ready"
			}],
			getContainer: async () => { throw new Error("container detail unavailable"); },
			registerContainer: async () => { throw new Error("registration unavailable"); },
			probeContainer: async () => { throw new Error("probe unavailable"); },
			reconcileContainer: async () => { throw new Error("reconcile unavailable"); },
			restartContainer: async () => { throw new Error("restart unavailable"); },
			listTraces: async () => [],
			listUsage: async () => [],
			counts: async () => ({ containers: 1, traces: 0, usage: 0 })
		};
		(window as any).synthOptimizers = {
			listAlgorithms: async () => [], list: async () => [], listRecipes: async () => [{
				id: "sft.banking77.nemotron-lightning.tinker.v1",
				title: "Banking77 Tinker SFT",
				availability: "available",
				limits: { trainingSteps: 30 }
			}], listCloud: async () => [],
			hostedTrainingModels: async () => ({ revision: "unavailable", models: [] }), searchSavedLoras: async () => ({ items: [], total: 0 }),
			startRecipe: async () => { throw new Error("native optimizer runtime unavailable"); },
			refresh: async () => ({ status: "idle" }), eventsAfter: async () => [],
			onEvent: () => () => undefined
		};
	});
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await openOptimizer(page);
	await page.getByTestId("optimizer-tab-launch").click();
	await expect(page.getByTestId("training-workspace")).toBeVisible();
});

test("empty artifact library never invents training results", async ({ page }) => {
	await page.getByTestId("training-tab-artifacts").click();
	await expect(page.getByTestId("training-artifact-library")).toContainText("Local artifacts");
	await expect(page.locator("article.training-artifact")).toHaveCount(0);
	await expect(page.getByTestId("training-artifact-detail")).toHaveCount(0);
});

test("setup fails closed when no real MLX runtime is installed", async ({ page }) => {
	await page.getByTestId("training-tab-setup").click();
	await expect(page.getByTestId("training-setup")).toHaveAttribute("data-install-state", "absent");
	await page.getByRole("button", { name: "Install managed copy" }).click();
	await expect(page.getByRole("alert")).toContainText("model download unavailable");
});

test("resolved config routes hosted launches through the native optimizer", async ({ page }) => {
	await page.getByTestId("training-tab-train").click();
	await page.getByLabel("Recipe").selectOption("sft");
	await expect(page.getByTestId("training-resolved-config")).toContainText("Before · checkpoints · final");
	await expect(page.getByTestId("training-resolved-config")).toContainText("Public Tinker service");
	await page.getByLabel("Compute").selectOption("tinker");
	await page.getByLabel("Dataset / workload").selectOption("ctr-alfworld-cleanroom");
	await expect(page.getByTestId("training-resolved-config")).toContainText("Hosted · Tinker");
	await expect(page.getByTestId("training-resolved-config")).toContainText("alfworld.text.v1");
	await page.getByRole("button", { name: "Start bounded run" }).click();
	await expect(page.getByTestId("training-run-failure")).toContainText("native optimizer runtime unavailable");
});

test("real unscored checkpoint evidence remains reviewable without inventing a plot", async ({ page }) => {
	await page.evaluate(() => {
		(window as any).synthOptimizers.startRecipe = async () => ({ id: "real-run", status: "running" });
		(window as any).synthOptimizers.refresh = async () => ({ id: "real-run", status: "completed" });
		(window as any).synthOptimizers.runSummary = async () => ({
			unchanged: false, projectionRevision: 1, tailCursor: 1,
			summary: {schemaVersion: "optimizer_run_summary.v1", runId: "real-run", algorithm: "sft", status: "completed", lifecycle: "terminal", projectionRevision: 1, asOfSequence: 1, tailCursor: 1, collections: [{collection: "evaluations", count: 1, latestRevision: 1}], work: {}, usage: {}, budget: {bytes: 512, limit: 65536, within: true}}
		});
		(window as any).synthOptimizers.runCollection = async (runId: string, collection: string) => ({
			runId, collection, projectionRevision: 1, asOfSequence: 1, total: 1, nextCursor: null, truncatedByBytes: false, limit: 100,
			rows: [{runId, collection, itemId: "eval-10", ordinal: 0, revision: 1, details: {phase: "checkpoint", step: 10, checkpointId: "ckpt-10", score: null, status: "completed", sampleCount: 2}}]
		});
	});
	await page.getByTestId("training-tab-train").click();
	await page.getByLabel("Compute").selectOption("tinker");
	await page.getByLabel("Dataset / workload").selectOption("ctr-alfworld-cleanroom");
	await page.getByRole("button", { name: "Start bounded run" }).click();
	const evidence = page.getByTestId("training-evaluation-comparison");
	await expect(evidence).toContainText("no scores returned");
	await evidence.getByRole("button", { name: "Review checkpoint evaluation at step 10" }).click();
	await expect(page.getByTestId("training-evaluation-dialog")).toContainText("ckpt-10");
	await expect(page.getByTestId("training-evaluation-dialog").getByRole("row", {name: /ckpt-10.*checkpoint.*2/})).toBeVisible();
});

test("a run cannot start until an exact training workload is advertised", async ({ page }) => {
	// Runs after the beforeEach init script, so it replaces that ready container.
	await page.addInitScript(() => {
		(window as any).synthInventory.listContainers = async () => [];
	});
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await openOptimizer(page);
	await page.getByTestId("optimizer-tab-launch").click();
	await page.getByTestId("training-tab-train").click();
	await expect(
		page.getByRole("alert").filter({ hasText: "No training workload" })
	).toBeVisible();
	await expect(page.getByRole("button", { name: "Start bounded run" })).toBeDisabled();
	await expect(page.getByTestId("training-resolved-config")).toContainText("No ready container");
});
