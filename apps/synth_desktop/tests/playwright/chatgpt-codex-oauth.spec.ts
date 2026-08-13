import { expect, test } from "./browser.fixture";

test("ChatGPT subscription card connects, shows allowance copy, and disconnects", async ({ page }) => {
	await page.addInitScript(() => {
		let configured = false;
		window.synthCodexOauth = {
			begin: async () => {
				configured = true;
				return { authorizeUrl: "https://auth.example.test/authorize", mode: "auto" as const };
			},
			completeManual: async () => {
				configured = true;
				return { configured, accountHint: "person@example.com" };
			},
			status: async () => ({ configured, accountHint: configured ? "person@example.com" : null }),
			disconnect: async () => ({ configured: configured = false }),
			cancel: async () => undefined
		};
	});
	await page.reload();
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("open-account-settings").click();
	await page.getByRole("button", { name: "Models", exact: true }).click();

	const card = page.getByTestId("chatgpt-codex-subscription");
	await expect(card).toContainText("local personal use");
	await expect(card).toContainText("not API credits");
	await card.getByTestId("codex-oauth-connect").click();
	await expect(card.getByTestId("codex-oauth-status")).toHaveText("Connected");
	await expect(card).toContainText("person@example.com");
	await expect(card.getByTestId("codex-oauth-authorized-models")).toContainText("plan allowance");
	await card.getByTestId("codex-oauth-disconnect").click();
	await expect(card.getByTestId("codex-oauth-status")).toHaveText("Not connected");
});

test("subscription targets are grouped and gated without OAuth", async ({ page }) => {
	await page.addInitScript(() => {
		window.synthCodexOauth = {
			begin: async () => ({ authorizeUrl: "https://auth.example.test", mode: "manual" as const }),
			completeManual: async () => ({ configured: true }),
			status: async () => ({ configured: false }),
			disconnect: async () => ({ configured: false }),
			cancel: async () => undefined
		};
	});
	await page.reload();
	await page.getByTestId("composer-model").click();
	const menu = page.getByTestId("composer-model-menu");
	await expect(menu).toContainText("ChatGPT · subscription");
	const option = menu.getByTestId("composer-model-option-chatgpt-luna");
	await expect(option.getByRole("option")).toHaveAttribute("aria-disabled", "true");
	await expect(option).toContainText("Connect in Settings → Models");
});

test("subscription UI is available by default", async ({ page }) => {
	await page.addInitScript(() => {
		window.synthCodexOauth = {
			begin: async () => ({ authorizeUrl: "https://auth.example.test", mode: "manual" as const }),
			completeManual: async () => ({ configured: false }),
			status: async () => ({ configured: false }),
			disconnect: async () => ({ configured: false }),
			cancel: async () => undefined
		};
	});
	await page.reload();
	await page.getByTestId("composer-model").click();
	await expect(page.getByTestId("composer-model-menu")).toContainText("ChatGPT · subscription");
});
