import type { Page } from "@playwright/test";
import { expect, test } from "./browser.fixture";

async function openSettings(page: Page) {
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-menu-settings").click();
}

test("Settings → Privacy can opt out of optional product analytics", async ({ page }) => {
	await openSettings(page);
	await expect(page.getByTestId("settings-general")).toBeVisible();
	const privacy = page.getByTestId("settings-privacy");
	await expect(privacy).toBeVisible();
	await expect(page.getByTestId("telemetry-policy-version")).toContainText("workshop.product-telemetry.v2");
	await expect(privacy.getByText(/prompts, traces, filenames, or secret values/i)).toBeVisible();
	await expect(privacy.getByText(/Sign out deletes optional events/i)).toBeVisible();
	await expect(page.getByTestId("telemetry-policy-version")).toContainText("90-day optional retention");
	// Honest consent bookkeeping: before any choice the card says so.
	await expect(page.getByTestId("telemetry-consent-status")).toContainText("Not asked yet");
	await page.getByTestId("telemetry-optional-off").click();
	await expect(page.getByTestId("telemetry-optional-off")).toHaveAttribute("aria-checked", "true");
	await expect(page.getByTestId("telemetry-consent-status")).toContainText("Sharing declined");
	await page.getByTestId("telemetry-optional-on").click();
	await expect(page.getByTestId("telemetry-optional-on")).toHaveAttribute("aria-checked", "true");
	await expect(page.getByTestId("telemetry-consent-status")).toContainText("Sharing allowed");
	// Transparency view is one click and never errors.
	await page.getByTestId("telemetry-view-events").click();
	await expect(page.getByTestId("telemetry-event-list")).toBeVisible();
});

test("first run asks for basic telemetry once and records the answer", async ({ page }) => {
	const ask = page.getByTestId("telemetry-consent-ask");
	await expect(ask).toBeVisible();
	await expect(ask).toContainText("never prompts");
	await ask.getByTestId("telemetry-consent-decline").click();
	await expect(ask).not.toBeVisible();
	// The answer is host state: Settings reflects it and the ask stays gone.
	await openSettings(page);
	await expect(page.getByTestId("telemetry-consent-status")).toContainText("Sharing declined");
	await expect(page.getByTestId("telemetry-optional-off")).toHaveAttribute("aria-checked", "true");
});

test("Settings → Secrets and Privacy never render a credential canary", async ({ page }) => {
	await openSettings(page);
	await page.getByTestId("settings-nav-secrets").click();
	await page.getByTestId("secrets-add").click();
	const canary = "sk-playwright-theme6-MUST-NOT-LEAK";
	await page.getByTestId("secrets-credential-input").fill(canary);
	await expect(page.getByTestId("settings-page").getByText(canary)).toHaveCount(0);
	await page.getByTestId("settings-nav-general").click();
	await expect(page.getByTestId("settings-privacy")).toBeVisible();
	await expect(page.getByTestId("settings-page").getByText(canary)).toHaveCount(0);
});
