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
	tier?: "free" | "starter" | "pro";
	metered?: boolean;
};

async function stubCloudAccount(page: import("@playwright/test").Page, options: StubOptions) {
	await page.addInitScript((stub) => {
		const opened: string[] = [];
		(window as unknown as { __billingOpened: string[] }).__billingOpened = opened;
		const tier = stub.tier ?? "pro";
		const upgradeTier = stub.state === "limited"
			? "pro"
			: tier === "free"
				? "starter"
				: tier === "starter"
					? "pro"
					: null;
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
				name: tier === "free" ? "Free" : tier === "starter" ? "Starter" : "Pro",
				tier,
				state: "active",
				metered: stub.metered ?? true,
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
				checkoutUrl: upgradeTier ? `https://example.test/usage?upgrade=${upgradeTier}` : null,
				portalUrl: "https://example.test/usage",
				upgradeTier
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
			update: async (request) => {
				Object.assign(base, request);
				return { ...base };
			},
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

test("the account menu supports keyboard traversal and restores trigger focus", async ({ page }) => {
	await stubCloudAccount(page, { state: "active", remainingUsd: 157.5, usedUsd: 42.5 });
	const trigger = page.getByTestId("account-menu-trigger");
	await trigger.focus();
	await page.keyboard.press("Enter");
	await expect(page.getByTestId("account-open-usage")).toBeFocused();
	await page.keyboard.press("ArrowDown");
	await expect(page.getByTestId("account-primary-action")).toBeFocused();
	await page.keyboard.press("Escape");
	await expect(page.getByTestId("account-menu")).toBeHidden();
	await expect(trigger).toBeFocused();
});

test("the usage sheet closes by Escape, backdrop, and button and restores focus", async ({ page }) => {
	await stubCloudAccount(page, { state: "active", remainingUsd: 157.5, usedUsd: 42.5 });
	const trigger = page.getByTestId("account-menu-trigger");
	const sheet = page.getByTestId("usage-sheet");
	const openUsage = async () => {
		await trigger.click();
		await page.getByTestId("account-open-usage").click();
		await expect(sheet).toBeVisible();
		await expect(page.getByTestId("usage-sheet-close")).toBeFocused();
	};

	await openUsage();
	await page.keyboard.press("Escape");
	await expect(sheet).toBeHidden();
	await expect(trigger).toBeFocused();

	await openUsage();
	await sheet.dispatchEvent("mousedown");
	await expect(sheet).toBeHidden();
	await expect(trigger).toBeFocused();

	await openUsage();
	await page.getByTestId("usage-sheet-close").click();
	await expect(sheet).toBeHidden();
	await expect(trigger).toBeFocused();
});

test("manage billing opens a hosted URL through the host, never in-app", async ({ page }) => {
	await stubCloudAccount(page, { state: "active", remainingUsd: 157.5, usedUsd: 42.5 });
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-primary-action").click();
	await expect
		.poll(async () => page.evaluate(() => (window as unknown as { __billingOpened: string[] }).__billingOpened))
		.toEqual(["manage"]);
});

test("an active free account offers the backend-issued upgrade action", async ({ page }) => {
	await stubCloudAccount(page, { state: "active", remainingUsd: 0, usedUsd: 0, tier: "free" });
	await page.getByTestId("account-menu-trigger").click();
	await expect(page.getByTestId("account-primary-action")).toContainText("Upgrade");
	await page.getByTestId("account-primary-action").click();
	await expect
		.poll(async () => page.evaluate(() => (window as unknown as { __billingOpened: string[] }).__billingOpened))
		.toEqual(["upgrade"]);
});

test("an unmetered account renders no dollar figures on account or usage surfaces", async ({ page }) => {
	await stubCloudAccount(page, {
		state: "active",
		remainingUsd: 0,
		usedUsd: 0,
		tier: "pro",
		metered: false
	});

	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-open-usage").click();
	const sheet = page.getByTestId("usage-sheet");
	await expect(sheet).toContainText("not metered in monthly dollars");
	await expect(sheet).not.toContainText("$");
	await page.getByTestId("usage-sheet-close").click();

	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("open-account-settings").click();
	const account = page.getByTestId("settings-account");
	await expect(account).toContainText("not metered in monthly dollars");
	await expect(account).not.toContainText("$");
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

test("Settings \u2192 Account leads with account facts and demotes connection config", async ({ page }) => {
	await stubCloudAccount(page, { state: "active", remainingUsd: 157.5, usedUsd: 42.5 });

	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("open-account-settings").click();
	const account = page.getByTestId("settings-account");
	await expect(account).toBeVisible();

	// Four user-facing sections, in order, before any endpoint field.
	await expect(page.getByTestId("account-page-profile")).toBeVisible();
	await expect(page.getByTestId("account-page-name")).toHaveText("ada");
	await expect(page.getByTestId("account-page-org")).toHaveText("Ada Labs");
	await expect(page.getByTestId("account-page-plan-name")).toHaveText("Pro");
	await expect(page.getByTestId("account-page-allowance")).toHaveText("$200.00");
	await expect(page.getByTestId("account-page-remaining")).toHaveText("$157.50");
	await expect(page.getByTestId("account-page-7d")).toHaveText("$1.20");
	await expect(page.getByTestId("account-page-devices")).toBeVisible();
	await expect(page.getByTestId("account-page-catalog")).toContainText("Starter");

	// Device usage is present but never merged into the cloud figures.
	await expect(page.getByTestId("account-page-usage")).toContainText("This device");
	await expect(page.getByTestId("account-page-usage")).toContainText("not your Synth Cloud allowance");

	// Connection config is reachable, but only behind the disclosure.
	await expect(page.getByTestId("backend-settings")).toBeHidden();
	await page.getByTestId("account-page-advanced").getByText("Advanced connection").click();
	await expect(page.getByTestId("backend-settings")).toBeVisible();
	await expect(page.getByTestId("backend-settings")).toContainText("Authenticated");

	// Billing still leaves the app through the host.
	await page.getByTestId("account-page-primary-action").click();
	await expect
		.poll(async () => page.evaluate(() => (window as unknown as { __billingOpened: string[] }).__billingOpened))
		.toEqual(["manage"]);
});

test("saving Advanced connection refreshes the Devices summary without a relaunch", async ({ page }) => {
	await stubCloudAccount(page, { state: "active", remainingUsd: 157.5, usedUsd: 42.5 });
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("open-account-settings").click();
	await expect(page.getByTestId("account-page-backend")).toHaveText("https://api.usesynth.ai");

	await page.getByTestId("account-page-advanced").getByText("Advanced connection").click();
	await page.getByLabel("Backend API").fill("http://127.0.0.1:8000");
	await page.getByRole("button", { name: "Save and reconnect" }).click();

	await expect(page.getByTestId("account-page-backend")).toHaveText("http://127.0.0.1:8000");
});

test("the Account page does not overflow at a narrow desktop width", async ({ page }) => {
	await page.setViewportSize({ width: 640, height: 800 });
	await stubCloudAccount(page, { state: "active", remainingUsd: 157.5, usedUsd: 42.5 });
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("open-account-settings").click();
	const account = page.getByTestId("settings-account");
	await expect(account).toBeVisible();
	expect(await account.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
});

test("a limited account shows the block and an upgrade path on the Account page", async ({ page }) => {
	await stubCloudAccount(page, { state: "limited", remainingUsd: 0, usedUsd: 200 });
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("open-account-settings").click();
	await expect(page.getByTestId("account-page-blocked")).toContainText("allowance is used up");
	await expect(page.getByTestId("account-page-primary-action")).toContainText("Upgrade");
	await expect(page.getByTestId("account-page-remaining")).toHaveText("$0.00");
});
