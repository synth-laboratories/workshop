import { expect, test } from "./browser.fixture";
import { openOptimizer } from "./v02-helpers";

test("install refreshes recipe admission without restarting the app", async ({ page }) => {
	await page.addInitScript(() => {
		let installed = false;
		const status = () => ({
			schemaVersion: "synth.plugin-status.v1", pluginId: "optimizers", enabled: true,
			phase: installed ? "ready" : "not_installed",
			installedVersion: installed ? "0.2.20" : null, selectedVersion: "0.2.20",
			releaseChannel: "official", catalogVersion: "0.2.20",
			service: { phase: installed ? "ready" : "stopped", activeRuns: 0 }, algorithms: [], templates: []
		});
		(window as any).synthPlugins = {
			list: async () => [status()], status: async () => status(),
			manage: async () => { installed = true; return { operation: "install", outcome: "ok", retained: [] }; }
		};
		(window as any).synthOptimizers = {
			list: async () => [], listAlgorithms: async () => [], onEvent: () => () => undefined,
			listCloud: async () => [], searchSavedLoras: async () => ({ items: [], total: 0 }),
			listRecipes: async () => [{ id: "eval.install.refresh", algorithmId: "eval", title: "Install refresh proof", availability: installed ? "available" : "unavailable", availabilityReason: installed ? null : "Runtime not installed" }]
		};
	});
	await page.reload();
	await openOptimizer(page);
	await page.getByTestId("optimizer-tab-launch").click();
	await expect(page.getByTestId("optimizer-eval-availability-eval.install.refresh")).toHaveText("unavailable");
	await page.getByRole("button", { name: "Install and start Optimizers", exact: true }).click();
	await expect(page.getByTestId("optimizer-eval-availability-eval.install.refresh")).toHaveText("available");
	await expect(page.getByTestId("optimizer-eval-recipe-eval.install.refresh").getByRole("button", { name: "Set up run" })).toBeEnabled();
});
