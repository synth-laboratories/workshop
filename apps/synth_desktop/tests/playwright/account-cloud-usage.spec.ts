import { expect, test } from "./browser.fixture";
import { formatUsd } from "../../src/renderer/src/runtime/accountView";

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

test("missing dollar amounts render UNKNOWN instead of zero", () => {
	expect(formatUsd(null)).toBe("UNKNOWN");
	expect(formatUsd(undefined)).toBe("UNKNOWN");
	expect(formatUsd(Number.NaN)).toBe("UNKNOWN");
	expect(formatUsd(0)).toBe("$0.00");
});

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
		const breakdown = (row: Record<string, unknown>) => ({
			provider: "openrouter",
			modelId: "unknown",
			requests: 0,
			inputTokens: 0,
			cachedInputTokens: null,
			nonCachedInputTokens: null,
			cacheWriteTokens: null,
			reasoningTokens: null,
			outputTokens: 0,
			totalTokens: 0,
			cacheHitRate: null,
			billedCostUsd: null,
			estimatedCostUsd: null,
			costSource: "none",
			decodeTpsP50: null,
			decodeTpsP95: null,
			endToEndTpsP50: null,
			endToEndTpsP95: null,
			ttftMsP50: null,
			ttftMsP95: null,
			perfSampleCount: 0,
			...row
		});
		const usageWindows: string[] = [];
		(window as unknown as { __usageWindows: string[] }).__usageWindows = usageWindows;
		window.synthUsage = {
			summary: async (usageWindow) => {
				usageWindows.push(usageWindow);
				if (usageWindow === "today") {
					return {
						window: usageWindow,
						totals: breakdown({ provider: "all", modelId: "all", requests: 1, inputTokens: 4000, outputTokens: 1000, totalTokens: 5000 }),
						models: [breakdown({ modelId: "openai/gpt-5.6-luna", requests: 1, inputTokens: 4000, outputTokens: 1000, totalTokens: 5000, estimatedCostUsd: 0.01, costSource: "tariff_estimate" })],
						generatedAt: "2026-08-10T12:00:00+00:00"
					};
				}
				return {
					window: usageWindow,
					totals: breakdown({
						provider: "all", modelId: "all", requests: 19,
						inputTokens: 170_000, cachedInputTokens: 80_000, nonCachedInputTokens: 90_000,
						cacheWriteTokens: 2_000, reasoningTokens: 1_500, outputTokens: 42_000,
						totalTokens: 212_000, cacheHitRate: 80_000 / 170_000,
						billedCostUsd: 0.42, estimatedCostUsd: 0.07, costSource: "provider_reported"
					}),
					models: [
						breakdown({
							modelId: "openai/gpt-5.6-luna", requests: 12,
							inputTokens: 120_000, cachedInputTokens: 80_000, nonCachedInputTokens: 40_000,
							cacheWriteTokens: 2_000, reasoningTokens: 1_500, outputTokens: 30_000,
							totalTokens: 150_000, cacheHitRate: 2 / 3,
							billedCostUsd: 0.42, costSource: "provider_reported",
							decodeTpsP50: 25, decodeTpsP95: 40, endToEndTpsP50: 18, endToEndTpsP95: 30,
							ttftMsP50: 800, ttftMsP95: 2_000, perfSampleCount: 12
						}),
						breakdown({
							modelId: "poolside/laguna-s-2.1", requests: 4,
							inputTokens: 40_000, outputTokens: 8_000, totalTokens: 48_000,
							estimatedCostUsd: 0.07, costSource: "tariff_estimate",
							endToEndTpsP50: 22, endToEndTpsP95: 28, perfSampleCount: 4
						}),
						breakdown({
							provider: "local-laguna", modelId: "poolside/Laguna-XS-2.1-NVFP4-mlx", requests: 3,
							inputTokens: 10_000, outputTokens: 4_000, totalTokens: 14_000,
							decodeTpsP50: 26, decodeTpsP95: 27, ttftMsP50: 350, ttftMsP95: 500, perfSampleCount: 3
						})
					],
					generatedAt: "2026-08-10T12:00:00+00:00"
				};
			}
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
	await expect(device.getByTestId("usage-total-tokens")).toHaveText("212,000");

	await page.getByTestId("usage-sheet-close").click();
	await expect(sheet).toBeHidden();
});

test("the device dashboard labels billing authority per provider and model", async ({ page }) => {
	await stubCloudAccount(page, { state: "active", remainingUsd: 157.5, usedUsd: 42.5 });
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-open-usage").click();
	const device = page.getByTestId("usage-sheet-device");

	// Totals keep settled and estimated money in separate labeled rows.
	await expect(device.getByTestId("usage-total-billed")).toHaveText("$0.42");
	await expect(device.getByTestId("usage-total-estimated")).toHaveText("$0.07");
	await expect(device.getByTestId("usage-total-cached")).toContainText("80,000");
	await expect(device.getByTestId("usage-total-cached")).toContainText("47% hit");

	// Settled OpenRouter money says billed, never estimated.
	const luna = device.getByTestId("usage-model-openrouter-openai-gpt-5-6-luna");
	await expect(luna).toContainText("OpenRouter · 12 requests");
	await expect(luna.getByTestId("usage-model-openrouter-openai-gpt-5-6-luna-cost")).toHaveText("$0.42 billed");
	await expect(luna).toContainText("cached 80,000 (67%)");
	const lunaPerf = luna.getByTestId("usage-model-openrouter-openai-gpt-5-6-luna-perf");
	await expect(lunaPerf).toContainText("decode 25 tok/s (p95 40 tok/s)");
	await expect(lunaPerf).toContainText("end-to-end 18 tok/s");
	await expect(lunaPerf).toContainText("TTFT 800 ms (p95 2.0 s)");
	await expect(lunaPerf).toContainText("12 samples");

	// Unsettled money is clearly an estimate, and unreported cache telemetry
	// reads unavailable — never zero.
	const lagunaS = device.getByTestId("usage-model-openrouter-poolside-laguna-s-2-1");
	await expect(lagunaS.getByTestId("usage-model-openrouter-poolside-laguna-s-2-1-cost")).toHaveText("$0.07 estimated");
	await expect(lagunaS).toContainText("cached unavailable");

	// Local runs carry no provider charge and never render $0.00.
	const local = device.getByTestId("usage-model-local-laguna-poolside-Laguna-XS-2-1-NVFP4-mlx");
	await expect(local.getByTestId("usage-model-local-laguna-poolside-Laguna-XS-2-1-NVFP4-mlx-cost"))
		.toHaveText("On-device · no provider charge");
	await expect(local).not.toContainText("$0.00");

	// The window control refetches natively per window.
	await device.getByTestId("usage-window-today").click();
	await expect(device.getByTestId("usage-total-tokens")).toHaveText("5,000");
	const windows = await page.evaluate(() => (window as unknown as { __usageWindows: string[] }).__usageWindows);
	expect(windows).toContain("7d");
	expect(windows).toContain("today");
});

test("the account menu supports keyboard traversal and restores trigger focus", async ({ page }) => {
	await stubCloudAccount(page, { state: "active", remainingUsd: 157.5, usedUsd: 42.5 });
	const trigger = page.getByTestId("account-menu-trigger");
	await trigger.focus();
	await page.keyboard.press("Enter");
	await expect(page.getByTestId("account-usage-remaining")).toBeFocused();
	await page.keyboard.press("ArrowDown");
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

/*
 * `Usage remaining` is an expandable summary above the `Usage` row, per
 * HANDOFF_CLOUD_ACCOUNT_QA.md A3/C2 and the target UX in
 * synth_cloud_api_usage.md. It summarizes Synth Cloud only; `Usage` opens the
 * sheet where cloud and device sit side by side without blending.
 */
test("the account menu expands Usage remaining above a separate Usage row", async ({ page }) => {
	await stubCloudAccount(page, { state: "active", remainingUsd: 157.5, usedUsd: 42.5 });
	await page.getByTestId("account-menu-trigger").click();

	const remaining = page.getByTestId("account-usage-remaining");
	await expect(remaining).toBeVisible();
	await expect(page.getByTestId("account-usage-remaining-value")).toHaveText("$157.50");
	await expect(remaining).toHaveAttribute("aria-expanded", "false");
	await expect(page.getByTestId("account-allowance-panel")).toBeHidden();

	await remaining.click();
	await expect(remaining).toHaveAttribute("aria-expanded", "true");
	const panel = page.getByTestId("account-allowance-panel");
	await expect(panel).toContainText("Pro");
	await expect(panel).toContainText("Monthly allowance");
	await expect(panel).toContainText("$200.00");
	await expect(panel).toContainText("Used this period");
	await expect(panel).toContainText("$42.50");
	await expect(panel).toContainText("Remaining");
	await expect(panel).toContainText("Resets");
	// Cloud only: device figures live in the sheet, one row further down.
	await expect(panel).not.toContainText("This device");

	// The separate Usage entry still opens the sheet.
	await page.getByTestId("account-open-usage").click();
	await expect(page.getByTestId("usage-sheet")).toBeVisible();
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
	// Cloud plan chrome invents no dollars for an unmetered account. Device
	// rows may still show real provider charges — that money is a different
	// pool and is asserted separately above.
	const cloud = page.getByTestId("usage-sheet-cloud");
	await expect(cloud).toContainText("not metered in monthly dollars");
	await expect(cloud).not.toContainText("$");
	await page.getByTestId("usage-sheet-close").click();

	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("open-account-settings").click();
	const account = page.getByTestId("settings-account");
	await expect(account).toContainText("not metered in monthly dollars");
	await expect(account).not.toContainText("$");
});

/*
 * Desktop's half of the Upgrade deep link. Desktop asks the host for a hosted
 * URL and opens it; the browser then lands on
 * `{web}/usage?upgrade=<tier>&source=desktop`, which the frontend consumes
 * (`src/lib/desktopUpgradeIntent.ts`). Desktop itself must never render a way
 * to type a card number.
 */
test("Upgrade leaves through the host with a tier, and no card field exists in Desktop", async ({ page }) => {
	await stubCloudAccount(page, { state: "limited", remainingUsd: 0, usedUsd: 200 });
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-primary-action").click();

	await expect
		.poll(async () => page.evaluate(() => (window as unknown as { __billingOpened: string[] }).__billingOpened))
		.toEqual(["upgrade"]);

	// Nothing in the shell collects payment details, on any surface.
	await expect(page.locator("input[autocomplete*='cc-']")).toHaveCount(0);
	await expect(page.locator("input[name*='card' i], input[placeholder*='card number' i]")).toHaveCount(0);
	await expect(page.locator("iframe[src*='stripe' i], iframe[src*='checkout' i]")).toHaveCount(0);
});

test("an exhausted allowance blocks the cloud model and offers upgrade; local stays open", async ({ page }) => {
	await stubCloudAccount(page, { state: "limited", remainingUsd: 0, usedUsd: 200 });

	await page.getByTestId("account-menu-trigger").click();
	await expect(page.getByTestId("account-menu-blocked")).toContainText("allowance is used up");
	await expect(page.getByTestId("account-menu-blocked")).toContainText("Local models keep working");
	await expect(page.getByTestId("account-primary-action")).toContainText("Upgrade");
	await page.keyboard.press("Escape");

	await page.getByTestId("model-picker").click();
	await expect(page.getByTestId("model-option-allowance-blocked").first()).toContainText("allowance is used up");
	await expect(page.getByTestId("model-option-allowance-blocked")).toHaveCount(2);
	// The local target is untouched by a cloud billing state.
	await expect(page.getByTestId("model-option-local-laguna")).toBeVisible();
	await page.getByTestId("model-resolve-synth-billing").first().click();
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
