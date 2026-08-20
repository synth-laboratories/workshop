import type { Page } from "@playwright/test";
import { expect, test } from "./browser.fixture";

async function openSettings(page: Page) {
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-menu-settings").click();
}

test("Settings → Secrets is write-only and never reveals a value", async ({ page }) => {
	await openSettings(page);
	await page.getByTestId("settings-nav-secrets").click();

	const pane = page.getByTestId("settings-secrets");
	await expect(pane).toBeVisible();
	const providers = ["openai", "anthropic", "openrouter", "tinker", "groq"];
	await expect(pane.getByText("Not registered")).toHaveCount(providers.length);
	for (const provider of providers) {
		await expect(page.getByTestId(`secrets-provider-${provider}`)).toBeVisible();
	}
	await expect(page.getByTestId("secrets-add")).toBeVisible();
	await expect(page.getByTestId("secrets-import")).toBeVisible();

	await expect(pane.getByRole("button", { name: /reveal|show value|copy/i })).toHaveCount(0);

	await page.getByTestId("secrets-add").click();
	const form = page.getByTestId("secrets-add-form");
	await expect(form).toBeVisible();
	const credential = page.getByTestId("secrets-credential-input");
	await expect(credential).toHaveAttribute("type", "password");
	await expect(credential).toHaveAttribute("autocomplete", "off");
	await credential.fill("sk-playwright-MUST-NOT-RENDER");
	await expect(credential).toHaveValue("sk-playwright-MUST-NOT-RENDER");
	await expect(form.locator("input[type='password']")).toHaveCount(1);
	await expect(pane.getByText("sk-playwright-MUST-NOT-RENDER")).toHaveCount(0);
	await expect(pane.getByRole("button", { name: /reveal|show|copy/i })).toHaveCount(0);
});
