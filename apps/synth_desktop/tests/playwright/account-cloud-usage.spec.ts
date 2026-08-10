import { expect, test } from "./browser.fixture";

/*
 * Synth Cloud account snapshot in the shell.
 *
 * The host supplies one document; these tests assert the two rules the product
 * sketch calls out: cloud dollars and device usage are never blended, and an
 * exhausted cloud allowance blocks only billable cloud actions — local models
 * keep working.
 */

type StubOptions = {
	state: "active" | "limited";
	remainingUsd: number;
	usedUsd: number;
};

async function stubCloudAccount(page: import("@playwright/test").Page, options: StubOptions) {
	await page.addInitScript((stub) => {
		const opened: string[] = [];
		(window as unknown as { __billingOpened: string[] }).__billingOpened = opened;
		const summary = {
			signedIn: true,
			state: stub.state,
			environment: "prod" as const,
			source: "cloud" as const,
			accountId: "acct_1",
			displayName: "ada",
			email: "ada@example.com",
			organization: { id: "org_1", displayName: "Ada Labs", role: "owner" },
			plan: {
				name: "Pro",
				tier: "pro",
				state: "active",
				metered: true,
				monthlyAllowanceUsd: 200,
				usedUsd: stub.usedUsd,
				remainingUsd: stub.remainingUsd,
				resetsAt: "2026-09-01T00:00:00+00:00",
				source: "cloud" as const
			},
			cloudUsage: {
				today: { events: 2, costUsd: 0.15 },
				sevenDays: { events: 9, costUsd: 1.2 },
				thirtyDays: { events: 40, costUsd: 13 }
			},
			billing: {
				checkoutUrl: "https://example.test/usage?upgrade=pro",
				portalUrl: "https://example.test/usage",
				upgradeTier: "pro"
			},
			catalog: [
				{ tier: "starter", displayName: "Starter", priceUsd: 20, monthlyAllowanceUsd: 20 },
				{ tier: "pro", displayName: "Pro", priceUsd: 200, monthlyAllowanceUsd: 200 }
			],
			lastUpdated: "2026-08-10T12:00:00+00:00",
			stale: false
		};
		window.synthAccount = {
			beginSignIn: async () => ({ verificationUri: "https://example.test", expiresAtEpochS: 0 }),
			pollSignIn: async () => ({ status: "active" as const }),
			cancelSignIn: async () => undefined,
			signOut: async () => { throw new Error("unused"); },
			getSummary: async () => summary,
			refresh: async () => summary,
			openBilling: async (action: string) => {
				opened.push(action);
				return "https://example.test/usage";
			}
		};
		const base = {
			configPath: "/tmp/config.toml",
			envFile: "/tmp/.env",
			profile: "prod",
			backendUrl: "https://api.usesynth.ai",
			apiKeyEnv: "SYNTH_API_KEY",
			apiKeyConfigured: true,
			workerKeyConfigured: false,
			openrouterApiKeyConfigured: false
		};
		window.synthConfig = {
			get: async () => base,
			update: async () => { throw new Error("unused"); },
			listModelMultiAgent: async () => [],
			updateModelMultiAgent: async () => [],
			getWorkspaceAccess: async () => ({ allowedRoots: [] }),
			updateWorkspaceAccess: async () => ({ allowedRoots: [] })
		};
	}, options);
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
}

test("the usage sheet separates Synth Cloud from this device", async ({ page }) => {
	await stubCloudAccount(page, { state: "active", remainingUsd: 157.5, usedUsd: 42.5 });

	await page.getByTestId("account-menu-trigger").click();
	await expect(page.getByTestId("account-menu")).toContainText("ada");
	await expect(page.getByTestId("account-menu-subtitle")).toHaveText("Ada Labs");
	await page.getByTestId("account-open-usage").click();

	const sheet = page.getByTestId("usage-sheet");
	await expect(sheet).toBeVisible();

	const cloud = page.getByTestId("usage-sheet-cloud");
	await expect(cloud).toContainText("Synth Cloud");
	await expect(page.getByTestId("usage-sheet-plan-name")).toHaveText("Pro");
	await expect(page.getByTestId("usage-sheet-allowance")).toHaveText("$200.00");
	await expect(page.getByTestId("usage-sheet-used")).toHaveText("$42.50");
	await expect(page.getByTestId("usage-sheet-remaining")).toHaveText("$157.50");
	await expect(page.getByTestId("usage-sheet-today")).toHaveText("$0.15");
	await expect(page.getByTestId("usage-sheet-7d")).toHaveText("$1.20");
	await expect(page.getByTestId("usage-sheet-30d")).toHaveText("$13.00");
	await expect(page.getByTestId("usage-sheet-last-updated")).toContainText("Last updated");

	// The device section is separate and says so; it never merges into the
	// cloud totals above.
	const device = page.getByTestId("usage-sheet-device");
	await expect(device).toContainText("This device");
	await expect(device).toContainText("not your Synth Cloud allowance");
	await expect(device.getByTestId("usage-sheet-device-weekly-tokens")).toBeVisible();

	await page.getByTestId("usage-sheet-close").click();
	await expect(sheet).toBeHidden();
});

test("manage billing opens a hosted URL through the host, never in-app", async ({ page }) => {
	await stubCloudAccount(page, { state: "active", remainingUsd: 157.5, usedUsd: 42.5 });
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-primary-action").click();
	await expect
		.poll(async () => page.evaluate(() => (window as unknown as { __billingOpened: string[] }).__billingOpened))
		.toEqual(["manage"]);
});

test("an exhausted allowance blocks the cloud model and offers upgrade; local stays open", async ({ page }) => {
	await stubCloudAccount(page, { state: "limited", remainingUsd: 0, usedUsd: 200 });

	await page.getByTestId("account-menu-trigger").click();
	await expect(page.getByTestId("account-menu-blocked")).toContainText("allowance is used up");
	await expect(page.getByTestId("account-menu-blocked")).toContainText("Local models keep working");
	await expect(page.getByTestId("account-primary-action")).toContainText("Upgrade");
	await page.keyboard.press("Escape");

	await page.getByTestId("model-picker").click();
	await expect(page.getByTestId("model-option-allowance-blocked")).toContainText("allowance is used up");
	// The local target is untouched by a cloud billing state.
	await expect(page.getByTestId("model-option-local-laguna")).toBeVisible();
	await page.getByTestId("model-resolve-synth-billing").click();
	await expect(page.getByTestId("usage-sheet")).toBeVisible();
	await expect(page.getByTestId("usage-sheet-blocked")).toContainText("allowance is used up");
	await expect(page.getByTestId("usage-sheet-primary-action")).toContainText("Upgrade");
});
