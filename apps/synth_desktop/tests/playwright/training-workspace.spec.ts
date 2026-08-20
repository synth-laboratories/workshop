import { expect, test } from "./browser.fixture";

test.beforeEach(async ({ page }) => {
	await page.addInitScript(() => {
		(window as any).synthTrainingModels = {
			listModels: async () => [],
			downloadModel: async (modelId: string) => new Promise(() => modelId),
			deleteModel: async () => undefined,
			onDownloadProgress: () => () => undefined
		};
		(window as any).synthOptimizers = {
			listAlgorithms: async () => [], list: async () => [], listRecipes: async () => [], listCloud: async () => [],
			hostedTrainingModels: async () => ({ revision: "fixture", models: [] }), searchSavedLoras: async () => ({ items: [], total: 0 }),
			onEvent: () => () => undefined
		};
	});
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByRole("button", { name: "Optimizers" }).click();
	await expect(page.getByTestId("training-workspace")).toBeVisible();
});

test("artifact lineage opens exact inference and terminal failed-load state", async ({ page }) => {
	const cispo = page.getByTestId("training-artifact-mlx-lora-cispo-b921");
	await expect(cispo.getByTestId("training-artifact-lineage")).toContainText("mlx-lora-sft-7f31");
	await cispo.getByRole("button", { name: "mlx-lora-cispo-b921" }).click();
	await page.getByTestId("training-artifact-detail").getByRole("button", { name: "Run inference" }).click();
	await expect(page.getByTestId("artifact-inference")).toContainText("mlx-lora-cispo-b921");
	await expect(page.getByTestId("artifact-inference")).toContainText("Qwen 3.5 0.8B");
	await page.getByRole("button", { name: "Failed-load state" }).click();
	await expect(page.getByTestId("artifact-failed-load")).toContainText("Base model revision mismatch");
});

test("setup install is confirmed and download can cancel and resume", async ({ page }) => {
	await page.getByTestId("training-tab-setup").click();
	await page.getByRole("button", { name: "Install managed copy" }).click();
	await expect(page.getByTestId("training-confirm-install")).toContainText("1.12 GB");
	await expect(page.getByTestId("training-confirm-install")).toContainText("8d4c2f1");
	await page.getByTestId("training-confirm-install").getByRole("button", { name: "Confirm" }).click();
	await expect(page.getByTestId("progress-qwen-3.5-0.8b-mlx")).toBeVisible();
	await page.getByRole("button", { name: "Cancel download" }).click();
	await expect(page.getByTestId("training-setup")).toHaveAttribute("data-install-state", "cancelled");
	await page.getByRole("button", { name: "Resume download" }).click();
	await expect(page.getByTestId("progress-qwen-3.5-0.8b-mlx")).toBeVisible();
});

test("CISPO plan requires an artifact and resolves bounded config", async ({ page }) => {
	await page.getByTestId("training-tab-train").click();
	await page.getByLabel("Recipe").selectOption("cispo");
	await expect(page.getByLabel("Parent artifact")).toHaveValue("mlx-lora-sft-7f31");
	await expect(page.getByTestId("training-resolved-config")).toContainText("120 steps · 20 min · 12 GB");
	await expect(page.getByTestId("training-resolved-config")).toContainText("Before · every 40 steps · final");
	await expect(page.getByTestId("training-resolved-config")).toContainText("Container tunnel · exact checkpoint");
	await page.getByLabel("Compute").selectOption("tinker");
	await expect(page.getByTestId("training-resolved-config")).toContainText("Hosted · Tinker");
	await page.getByRole("button", { name: "Review run" }).click();
	await expect(page.getByTestId("training-confirm-start")).toContainText("Managed artifacts");
});

test("CISPO run exposes algorithm-specific diagnostics and next artifact", async ({ page }) => {
	await page.getByTestId("training-tab-run").click();
	await expect(page.getByTestId("training-run-view")).toContainText("CISPO");
	await expect(page.getByTestId("training-cispo-diagnostics")).toContainText("Policy objective");
	const comparison = page.getByTestId("training-evaluation-comparison");
	await expect(comparison).toContainText("baseline");
	await expect(comparison).toContainText("checkpoint");
	await expect(comparison).toContainText("final");
	await expect(comparison).toContainText("+0.304 vs baseline");
	await expect(comparison).toContainText("2 checkpoints · 4 observations");
	await expect(comparison.getByRole("img", { name: "Reward across 4 evaluation observations" })).toBeVisible();
	await page.getByRole("button", { name: "Open artifact" }).click();
	await expect(page.getByTestId("training-artifact-detail")).toContainText("mlx-lora-cispo-b921");
});

test("artifact Eval and deletion expose exact identity and receipt warning", async ({ page }) => {
	await page.getByTestId("training-artifact-detail").getByRole("button", { name: "Evaluate" }).click();
	await expect(page.getByTestId("artifact-eval")).toContainText("GSM8K exact-match");
	await page.getByTestId("training-tab-artifacts").click();
	await page.getByTestId("training-artifact-detail").getByRole("button", { name: "Delete" }).click();
	await expect(page.getByTestId("training-confirm-delete")).toContainText("2 retained receipts");
});

test("retain CUA-1 training receipts", async ({ page }) => {
	const receiptRoot = "../../docs/receipts/2026-08-20/v0.7-training-ui";
	for (const viewport of [{ name: "wide", width: 1440, height: 900 }, { name: "narrow", width: 980, height: 760 }]) {
		await page.setViewportSize(viewport);
		await page.getByTestId("training-tab-artifacts").click();
		await page.screenshot({ path: `${receiptRoot}/${viewport.name}-artifacts.png`, fullPage: true });
		await page.getByTestId("training-tab-setup").click();
		await page.screenshot({ path: `${receiptRoot}/${viewport.name}-setup-ready.png`, fullPage: true });
		await page.getByRole("button", { name: "Show progress" }).click();
		await page.screenshot({ path: `${receiptRoot}/${viewport.name}-setup-installing.png`, fullPage: true });
		await page.getByTestId("training-tab-train").click();
		await page.getByLabel("Recipe").selectOption("cispo");
		await page.screenshot({ path: `${receiptRoot}/${viewport.name}-train-cispo.png`, fullPage: true });
		await page.getByTestId("training-tab-run").click();
		await page.screenshot({ path: `${receiptRoot}/${viewport.name}-run-cispo.png`, fullPage: true });
		await page.evaluate(() => document.documentElement.setAttribute("data-theme", "dark"));
		await page.getByTestId("training-tab-artifacts").click();
		await page.screenshot({ path: `${receiptRoot}/${viewport.name}-artifacts-dark.png`, fullPage: true });
		await page.evaluate(() => document.documentElement.setAttribute("data-theme", "light"));
	}
});
