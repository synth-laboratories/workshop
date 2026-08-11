import { expect, test } from "./browser.fixture";

/**
 * External-download acceptance: what a brand-new user hits in the first five
 * minutes after installing the app. Fresh profile, no Synth account, no prior
 * preferences — the golden path must work locally and every paid surface must
 * degrade to a clear sign-in affordance instead of an error.
 */
test.describe("first-five-minutes golden path", () => {
	test.beforeEach(async ({ page }) => {
		await page.addInitScript(() => window.localStorage.clear());
		await page.reload();
		await page.getByTestId("titlebar").waitFor();
	});

	test("a fresh install boots to a usable landing with an explicit account choice", async ({ page }) => {
		const choices = page.getByTestId("first-run-account-choice");
		await expect(choices).toBeVisible();
		await expect(choices.getByRole("button", { name: /Continue locally/ })).toBeVisible();
		await expect(choices.getByRole("button", { name: /Sign in to Synth/ })).toBeVisible();
		// The shell is complete behind the choice: sidebar, titlebar, composer.
		await expect(page.getByTestId("sidebar")).toBeVisible();
		await expect(page.getByTestId("titlebar")).toBeVisible();
		await expect(page.getByTestId("composer")).toBeVisible();
		// Nothing in the first-run surface advertises Intern in v0.1.
		await expect(page.getByTestId("landing-page")).not.toContainText("Intern");
	});

	test("continue-locally reaches the composer and a working local-first model picker", async ({ page }) => {
		await page.getByTestId("first-run-account-choice")
			.getByRole("button", { name: /Continue locally/ }).click();
		await expect(page.getByTestId("first-run-account-choice")).not.toBeVisible();
		// Before any local weights exist the composer must explain itself, not error.
		const input = page.getByTestId("composer-input");
		await expect(input).toBeVisible();
		await expect(input).toHaveAttribute("aria-label", "Message composer");
		const placeholder = await input.getAttribute("placeholder");
		expect(placeholder, "composer guides the fresh user").toBeTruthy();
		// The model picker opens, offers the local target first, and hides Intern.
		await page.getByTestId("model-picker").click();
		const dropdown = page.getByTestId("model-dropdown");
		await expect(dropdown).toBeVisible();
		await expect(dropdown.getByTestId("model-option-local-laguna")).toBeVisible();
		await expect(dropdown.getByText("Intern · Live", { exact: true })).toHaveCount(0);
		await page.keyboard.press("Escape");
		await expect(dropdown).not.toBeVisible();
	});

	test("without an account, cloud targets ask for configuration instead of failing", async ({ page }) => {
		await page.getByTestId("first-run-account-choice")
			.getByRole("button", { name: /Continue locally/ }).click();
		await page.getByTestId("model-picker").click();
		const dropdown = page.getByTestId("model-dropdown");
		await expect(dropdown).toBeVisible();
		const cloudOption = dropdown.getByTestId("model-option-synth-cloud-laguna-s");
		await expect(cloudOption).toContainText("Synth API key required");
		await expect(cloudOption.getByTestId("model-configure-synth-api-key")).toBeVisible();
	});

	test("the account footer reports local mode and leads to sign-in, settings stay reachable", async ({ page }) => {
		await page.getByTestId("first-run-account-choice")
			.getByRole("button", { name: /Continue locally/ }).click();
		const trigger = page.getByTestId("account-menu-trigger");
		await expect(trigger).toBeVisible();
		await expect(page.getByTestId("settings")).toHaveCount(0);
		await expect(trigger).toContainText("Sign in to Synth");
		await expect(trigger).toContainText("Local mode");
		await trigger.click();
		const menu = page.getByTestId("account-menu");
		await expect(menu).toBeVisible();
		// Signed out: no Log out, no stale plan data.
		await expect(menu.getByTestId("account-log-out")).toHaveCount(0);
		await expect(menu).not.toContainText("$200");
		await menu.getByTestId("account-menu-settings").click();
		await expect(page.getByTestId("settings-page")).toBeVisible();
	});
});
