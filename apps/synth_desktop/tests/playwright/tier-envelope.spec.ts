import type { Page } from "@playwright/test";
import { expect, test } from "./browser.fixture";

async function openSettings(page: Page) {
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-menu-settings").click();
}

test("pre-release builds badge the titlebar top right", async ({ page }) => {
	// The dev server compiles the dev tier; stable/core bundles have this
	// element statically eliminated.
	const badge = page.getByTestId("titlebar-tier-badge");
	await expect(badge).toBeVisible();
	await expect(badge).toHaveText("dev");
});

test("Settings → Build shows the maturity envelope and the dev-server pre-release badge", async ({ page }) => {
	await openSettings(page);
	const card = page.getByTestId("settings-build-tier");
	await expect(card).toBeVisible();
	await expect(card.getByText(/runtime settings can only narrow it/i)).toBeVisible();
	// The Playwright fixture runs on the Vite dev server, which compiles the
	// dev tier; browser mode has no host, so the status reports the bundle.
	await expect(page.getByTestId("build-tier-status")).toContainText("dev");
	// The pre-release badge is a beta-tier bundled feature: present here, and
	// statically eliminated from stable/core production bundles.
	await expect(page.getByTestId("build-tier-badge")).toContainText("pre-release");
	await expect(page.getByTestId("build-tier-badge")).toContainText("dev");
});
