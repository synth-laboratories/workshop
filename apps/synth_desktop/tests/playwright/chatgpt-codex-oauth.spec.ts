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
			ensureReady: async () => configured
				? ({ state: "ready" as const, action: "reauthenticate" as const, canUseModels: true, guidance: "Ready", configured, accountHint: "person@example.com" })
				: ({ state: "disconnected" as const, action: "connect" as const, canUseModels: false, guidance: "Connect", configured }),
			status: async () => configured
				? ({ state: "ready" as const, action: "reauthenticate" as const, canUseModels: true, guidance: "Ready", configured, accountHint: "person@example.com" })
				: ({ state: "disconnected" as const, action: "connect" as const, canUseModels: false, guidance: "Connect", configured }),
			disconnect: async () => ({ state: "disconnected" as const, action: "connect" as const, canUseModels: false, guidance: "Connect", configured: configured = false }),
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

test("the first ChatGPT message verifies readiness once and reaches atomic sendTurn", async ({ page }) => {
	await page.addInitScript(() => {
		const testWindow = window as typeof window & {
			__oauthReadinessChecks?: number;
			__chatgptSends?: string[];
		};
		testWindow.__oauthReadinessChecks = 0;
		testWindow.__chatgptSends = [];
		const ready = {
			state: "ready" as const,
			action: "reauthenticate" as const,
			canUseModels: true,
			guidance: "Ready",
			configured: true,
			accountHint: "person@example.com"
		};
		window.synthCodexOauth = {
			begin: async () => ({ authorizeUrl: "https://auth.example.test", mode: "manual" as const }),
			completeManual: async () => ready,
			ensureReady: async () => {
				testWindow.__oauthReadinessChecks! += 1;
				return ready;
			},
			status: async () => ready,
			disconnect: async () => ({ configured: false }),
			cancel: async () => undefined
		};
		window.synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [],
			start: async () => { throw new Error("sendTurn owns first-message startup"); },
			startTurn: async () => { throw new Error("sendTurn owns the first turn"); },
			sendTurn: async (request: { sessionId: string }, prompt: string) => {
				testWindow.__chatgptSends!.push(prompt);
				return { sessionId: request.sessionId, threadId: "chatgpt-thread", turnId: "chatgpt-turn" };
			},
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: () => () => undefined
		};
	});
	await page.reload();
	await page.getByTestId("composer-model").click();
	await page.getByTestId("composer-model-option-chatgpt-luna").click();
	const readinessBeforeSend = await page.evaluate(() => (
		window as typeof window & { __oauthReadinessChecks?: number }
	).__oauthReadinessChecks ?? 0);
	await page.getByTestId("composer-input").fill("first ChatGPT message");
	await page.getByTestId("composer-send").click();
	await expect(page.getByTestId("model-working")).toBeVisible();
	const calls = await page.evaluate((readinessAtSubmit) => {
		const testWindow = window as typeof window & {
			__oauthReadinessChecks?: number;
			__chatgptSends?: string[];
		};
		return {
			readinessDuringSend: (testWindow.__oauthReadinessChecks ?? 0) - readinessAtSubmit,
			sends: testWindow.__chatgptSends
		};
	}, readinessBeforeSend);
	expect(calls).toEqual({ readinessDuringSend: 1, sends: ["first ChatGPT message"] });
});
