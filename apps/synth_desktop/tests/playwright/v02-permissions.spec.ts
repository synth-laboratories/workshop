/**
 * v0.2 permission chip: Full system access is danger-full-access, not
 * workspace-write, and the menu copy must say so.
 */
import { expect, test } from "./browser.fixture";

test("[v0.2] Full system access is a distinct sandbox from workspace write", async ({ page }) => {
	await page.addInitScript(() => {
		(window as typeof window & { synthLaguna?: unknown }).synthLaguna = {
			getStatus: async () => ({
				phase: "ready",
				baseUrl: "http://127.0.0.1:7333",
				backend: "mlx_lm",
				loadedModel: "poolside/Laguna-XS-2.1-NVFP4-mlx",
				detail: "Laguna XS ready",
				memoryBytes: null,
				updatedAt: Date.now()
			}),
			onStatus: () => () => undefined,
			listModels: async () => []
		};
	});
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	const firstRun = page.getByTestId("first-run-account-choice");
	if (await firstRun.isVisible().catch(() => false)) {
		await firstRun.getByRole("button", { name: /Continue locally/ }).click();
	}
	const chip = page.getByTestId("approval-mode-select");
	await expect(chip).toBeEnabled();
	await chip.click();
	const menu = page.getByTestId("approval-mode-menu");
	await expect(menu).toBeVisible();
	await expect(menu.getByRole("option", { name: /Full system access/ })).toBeVisible();
	await expect(menu.getByRole("option", { name: /Full system access/ })).toContainText("unrestricted filesystem and network access");
	await expect(menu.getByRole("option", { name: /Workspace access/ })).toContainText("inside the workspace");
	await menu.getByRole("option", { name: /Full system access/ }).click();
	await expect(chip).toHaveAccessibleName(/Full system access/);
	await expect(chip).toContainText("Full");
	await expect(chip).not.toHaveAccessibleName(/Workspace access/);
});
