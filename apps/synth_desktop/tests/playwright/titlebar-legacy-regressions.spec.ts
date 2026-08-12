import { expect, test, type Page } from "./browser.fixture";

const LEGACY_TEST_IDS = [
	"runtime-status",
	"open-account-settings",
	"titlebar-account",
	"titlebar-account-avatar",
	"titlebar-cloud-status"
] as const;

async function expectLegacyAccountChromeAbsent(page: Page): Promise<void> {
	const titlebar = page.getByTestId("titlebar");
	await expect(titlebar).toBeVisible();
	await expect(titlebar.getByText("Local", { exact: true })).toHaveCount(0);
	await expect(titlebar.getByText("S", { exact: true })).toHaveCount(0);
	await expect(titlebar.getByRole("button", { name: /account menu/i })).toHaveCount(0);
	await expect(titlebar.getByRole("button", { name: /cloud/i })).toHaveCount(0);
	for (const testId of LEGACY_TEST_IDS) {
		await expect(page.getByTestId(testId)).toHaveCount(0);
	}

	const unexpectedActions = await titlebar.locator(".titlebar-actions > *").evaluateAll((elements) =>
		elements
			.map((element) => ({
				ariaLabel: element.getAttribute("aria-label"),
				testId: element.getAttribute("data-testid")
			}))
			.filter(({ testId }) =>
				testId !== "toggle-terminal" &&
				testId !== "toggle-inference-rail" &&
				testId !== "app-version"
			)
	);
	expect(unexpectedActions, "titlebar actions are limited to terminal and inference controls").toEqual([]);
}

test("legacy Local pill and account initial stay out of every supported viewport", async ({ page }) => {
	for (const [width, height] of [[900, 640], [1100, 700], [1440, 900]] as const) {
		await page.setViewportSize({ width, height });
		await expectLegacyAccountChromeAbsent(page);
	}
});

test("legacy account chrome stays absent through first-run and signed-out states", async ({ page }) => {
	await page.addInitScript(() => window.localStorage.clear());
	await page.reload();
	await expect(page.getByTestId("first-run-account-choice")).toBeVisible();
	await expectLegacyAccountChromeAbsent(page);

	await page.getByTestId("first-run-account-choice")
		.getByRole("button", { name: /Continue locally/ }).click();
	await expect(page.getByTestId("first-run-account-choice")).not.toBeVisible();
	await expectLegacyAccountChromeAbsent(page);
});

test("legacy account chrome stays absent from Settings and after returning", async ({ page }) => {
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-menu").getByTestId("account-menu-settings").click();
	await expect(page.getByTestId("settings-page")).toBeVisible();
	await expectLegacyAccountChromeAbsent(page);

	await page.getByRole("button", { name: /Back/ }).click();
	await expect(page.getByTestId("landing-page")).toBeVisible();
	await expectLegacyAccountChromeAbsent(page);
});

test("signed-in account data remains in the sidebar and never leaks into the titlebar", async ({ page }) => {
	await page.addInitScript(() => {
		window.synthAccount = {
			getSummary: async () => ({
				signedIn: true,
				state: "active" as const,
				displayName: "Synth Dev",
				email: "dev@usesynth.ai",
				environment: "prod" as const,
				source: "cloud" as const
			})
		};
	});
	await page.reload();
	await expect(page.getByTestId("account-menu-trigger")).toContainText("Synth Dev");
	await expectLegacyAccountChromeAbsent(page);
});

test("opening terminal and side-panel controls cannot restore legacy header controls", async ({ page }) => {
	await page.getByTestId("toggle-terminal").click();
	await expect(page.getByTestId("terminal-panel")).toBeVisible();
	await expectLegacyAccountChromeAbsent(page);
	await page.getByTestId("toggle-terminal").click();
	await expectLegacyAccountChromeAbsent(page);
});
