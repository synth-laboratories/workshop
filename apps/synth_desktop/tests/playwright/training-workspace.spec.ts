import { expect, test } from "./browser.fixture";

test.beforeEach(async ({ page }) => {
	await page.addInitScript(() => {
		const withArtifact = localStorage.getItem("training-workspace-artifact-fixture") === "1";
		const artifact = {
			schemaVersion: "training.artifact.v1", id: "sft-parent", adapterKind: "mlx-lora.v1",
			baseModelId: "Qwen/Qwen3.5-0.8B", producingRunId: "sft-run", producingAlgorithm: "sft",
			datasetDigest: "sha256:dataset", configDigest: "sha256:config", digest: "sha256:adapter",
			path: "/managed/sft-parent", sizeBytes: 1024, integrity: "verified", compatibleInference: ["mlx"],
			createdAt: "2026-08-20T00:00:00Z"
		};
		(window as any).__trainingWorkspaceCalls = [];
		(window as any).synthTrainingArtifacts = {
			list: async () => withArtifact ? [artifact] : [],
			get: async (id: string) => { if (withArtifact && id === artifact.id) return artifact; throw new Error("artifact not found"); },
			launchInference: async (request: unknown) => {
				if (!withArtifact) throw new Error("real inference runtime unavailable");
				(window as any).__trainingWorkspaceCalls.push({ kind: "inference", request });
				return { artifactId: artifact.id, policySnapshotId: artifact.id, reply: "ok", baseModelId: artifact.baseModelId, producingRunId: artifact.producingRunId };
			}
		};
		(window as any).synthTrainingModels = {
			listModels: async () => [],
			downloadModel: async () => { throw new Error("model download unavailable"); },
			deleteModel: async () => undefined,
			onDownloadProgress: () => () => undefined
		};
		(window as any).synthOptimizers = {
			listAlgorithms: async () => [], list: async () => [], listRecipes: async () => [], listCloud: async () => [],
			hostedTrainingModels: async () => ({ revision: "unavailable", models: [] }), searchSavedLoras: async () => ({ items: [], total: 0 }),
			startRecipe: async (request: unknown) => {
				if (!withArtifact) throw new Error("native optimizer runtime unavailable");
				(window as any).__trainingWorkspaceCalls.push({ kind: "recipe", request });
				return { id: "bounded-run", status: "running" };
			},
			refresh: async () => ({ status: "idle" }), eventsAfter: async () => [],
			onEvent: () => () => undefined
		};
	});
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByRole("button", { name: "Optimizers" }).click();
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
	await expect(page.getByTestId("training-resolved-config")).toContainText("Container tunnel · exact checkpoint");
	await page.getByLabel("Compute").selectOption("tinker");
	await expect(page.getByTestId("training-resolved-config")).toContainText("Hosted · Tinker");
	await page.getByRole("button", { name: "Review run" }).click();
	await expect(page.getByTestId("training-run-failure")).toContainText("native optimizer runtime unavailable");
});

test("real unscored checkpoint evidence remains reviewable without inventing a plot", async ({ page }) => {
	await page.evaluate(() => {
		(window as any).synthOptimizers.startRecipe = async () => ({ id: "real-run", status: "running" });
		(window as any).synthOptimizers.refresh = async () => ({ id: "real-run", status: "completed" });
		(window as any).synthOptimizers.eventsAfter = async () => [{
			type: "training.evaluation.completed",
			delta: { evaluation: { phase: "checkpoint", step: 10, checkpoint_id: "ckpt-10", score: null, status: "completed", detail: { scored: 0, total: 2 } } }
		}];
	});
	await page.getByTestId("training-tab-train").click();
	await page.getByLabel("Compute").selectOption("tinker");
	await page.getByRole("button", { name: "Review run" }).click();
	const evidence = page.getByTestId("training-evaluation-comparison");
	await expect(evidence).toContainText("no scores returned");
	await evidence.getByRole("button", { name: "Review checkpoint evaluation at step 10" }).click();
	await expect(page.getByTestId("training-evaluation-dialog")).toContainText("ckpt-10");
	await expect(page.getByTestId("training-evaluation-dialog")).toContainText('"scored": 0');
});

test("bounded hosted SFT launch retains the selected model and dataset shard", async ({ page }) => {
	await page.evaluate(() => localStorage.setItem("training-workspace-artifact-fixture", "1"));
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByRole("button", { name: "Optimizers" }).click();
	await page.getByTestId("training-tab-train").click();
	await page.getByLabel("Compute").selectOption("tinker");
	await expect(page.getByLabel("Dataset")).toHaveValue("Banking77 · train_a");
	await page.getByRole("button", { name: "Review run" }).click();
	const calls = await page.evaluate(() => (window as any).__trainingWorkspaceCalls);
	expect(calls[0]).toEqual({
		kind: "recipe",
		request: {
			recipeId: "sft.banking77.nemotron-lightning.tinker.v1", openVisual: false,
			baseModel: "nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16",
			datasetShard: "train_a"
		}
	});
});

test("bounded CISPO launch retains the selected SFT parent", async ({ page }) => {
	await page.evaluate(() => localStorage.setItem("training-workspace-artifact-fixture", "1"));
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByRole("button", { name: "Optimizers" }).click();
	await page.getByTestId("training-tab-train").click();
	await page.getByLabel("Recipe").selectOption("cispo");
	await expect(page.getByLabel("Dataset")).toHaveValue("Banking77 rollout target");
	await expect(page.getByLabel("Parent artifact")).toHaveValue("sft-parent");
	await page.getByRole("button", { name: "Review run" }).click();
	const calls = await page.evaluate(() => (window as any).__trainingWorkspaceCalls);
	expect(calls[0]).toEqual({
		kind: "recipe",
		request: {
			recipeId: "cispo.banking77.mlx.v1", openVisual: false,
			baseModel: "Qwen/Qwen3.5-0.8B",
			trainingArtifactId: "sft-parent"
		}
	});
});

test("artifact actions explicitly confirm inference and start bounded Eval", async ({ page }) => {
	await page.evaluate(() => localStorage.setItem("training-workspace-artifact-fixture", "1"));
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByRole("button", { name: "Optimizers" }).click();
	await expect(page.getByTestId("training-artifact-detail")).toContainText("sft-parent");
	await expect(page.getByRole("button", { name: "Delete" })).toHaveCount(0);

	await page.getByRole("button", { name: "Run inference" }).click();
	await page.getByRole("button", { name: "Start inference" }).click();
	await page.getByRole("button", { name: "← Artifacts" }).click();
	await page.getByRole("button", { name: "Evaluate" }).click();
	await page.getByRole("button", { name: "Start Eval" }).click();

	const calls = await page.evaluate(() => (window as any).__trainingWorkspaceCalls);
	expect(calls).toContainEqual({ kind: "inference", request: { id: "sft-parent", confirm: true } });
	expect(calls).toContainEqual({
		kind: "recipe",
		request: { recipeId: "eval.mlx.local-policy.smoke.v1", trainingArtifactId: "sft-parent", openVisual: true }
	});
});
