import { expect, test } from "./browser.fixture";

// Browser sign-in (device pairing) drives synthAccount; the fixture stubs the
// bridge so no real Workshop backend is involved.

test("browser sign-in pairs the device and flips the account to authenticated", async ({ page }) => {
	await page.addInitScript(() => {
		let polls = 0;
		let paired = false;
		window.synthAccount = {
			beginSignIn: async () => ({
				verificationUri: "https://www.usesynth.ai/signin?redirect_to=x",
				expiresAtEpochS: Math.floor(Date.now() / 1000) + 600
			}),
			pollSignIn: async () => {
				polls += 1;
				if (polls >= 2) {
					paired = true;
					return { status: "active" as const };
				}
				return { status: "pending" as const };
			},
			cancelSignIn: async () => undefined,
			signOut: async () => {
				paired = false;
				return { ...base, apiKeyConfigured: false };
			},
			getSummary: async () => (paired
				? {
					signedIn: true,
					state: "active" as const,
					accountId: "dev-local",
					displayName: "Synth Dev",
					environment: "dev" as const,
					source: "dev_seed" as const,
					plan: {
						name: "Synth Dev",
						metered: true,
						monthlyAllowanceUsd: 200,
						usedUsd: 12.5,
						remainingUsd: 187.5,
						resetsAt: "2026-09-01T00:00:00+00:00",
						source: "dev_seed" as const
					}
				}
				: { signedIn: false, state: "signed_out" as const, environment: "dev" as const })
		};
		const base = {
			configPath: "/tmp/config.toml",
			envFile: "/tmp/.env",
			profile: "prod",
			backendUrl: "https://api.usesynth.ai",
			apiKeyEnv: "SYNTH_API_KEY",
			workerKeyConfigured: false,
			openrouterApiKeyConfigured: false
		};
		window.synthConfig = {
			get: async () => ({
				...base,
				apiKeyConfigured: paired,
				...(paired ? { apiKeyFingerprint: "sk…f1e2", apiKeySource: "env file" } : {})
			}),
			update: async () => { throw new Error("unused"); },
			listModelMultiAgent: async () => [],
			updateModelMultiAgent: async () => [],
			getWorkspaceAccess: async () => ({ allowedRoots: [] }),
			updateWorkspaceAccess: async () => ({ allowedRoots: [] })
		};
	});
	await page.reload();
	await page.getByTestId("titlebar").waitFor();

	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-menu").getByTestId("open-account-settings").click();
	const signIn = page.getByTestId("account-sign-in");
	await expect(signIn.getByTestId("sign-in-begin")).toContainText("Sign in with browser");
	await expect(signIn).toContainText("creates your Synth account");

	await signIn.getByTestId("sign-in-begin").click();
	await expect(signIn.getByTestId("sign-in-status")).toContainText("Finish sign-in in your browser");

	// Two 4s poll ticks flip the stub to paired.
	await expect(page.getByTestId("backend-settings")).toContainText("Authenticated", { timeout: 15_000 });
	await expect(signIn.getByTestId("sign-in-status")).toContainText("Connected to Synth");
	await page.getByTestId("account-menu-trigger").click();
	await expect(page.getByTestId("account-menu")).toContainText("Synth Dev");
	await expect(page.getByTestId("account-menu")).toContainText("Usage remaining");
	await expect(page.getByTestId("account-menu")).toContainText("Settings");
	await expect(page.getByTestId("account-menu")).toContainText("Log out");
	await page.getByTestId("account-usage-toggle").click();
	await expect(page.getByTestId("account-plan-allowance")).toHaveText("$200.00 monthly");
	await expect(page.getByTestId("account-plan-used")).toHaveText("$12.50");
	await expect(page.getByTestId("account-plan-remaining")).toHaveText("$187.50");
	await expect(page.getByTestId("account-plan-resets")).not.toBeEmpty();
	// A dev/local plan is never presented as Synth Cloud truth.
	await expect(page.getByTestId("account-plan-dev-seed")).toContainText("Dev stand-in");
	await expect(page.getByTestId("account-usage")).toContainText("This device, this week");
	await page.getByTestId("account-menu-trigger").click();

	await signIn.getByTestId("account-sign-out").click();
	await expect(page.getByTestId("backend-settings")).toContainText("API key required");
	await expect(signIn.getByTestId("sign-in-status")).toContainText("creates your Synth account");
});

test("first run offers local use and Synth sign-in as equal choices", async ({ page }) => {
	await page.addInitScript(() => window.localStorage.removeItem("synth.accountChoiceMade"));
	await page.reload();
	const choices = page.getByTestId("first-run-account-choice");
	await expect(choices.getByRole("button", { name: /Continue locally/ })).toBeVisible();
	await expect(choices.getByRole("button", { name: /Sign in to Synth/ })).toBeVisible();
	await choices.getByRole("button", { name: /Continue locally/ }).click();
	await expect(choices).not.toBeVisible();
});

test("cancel during pairing returns to the idle sign-in affordance", async ({ page }) => {
	await page.addInitScript(() => {
		window.synthAccount = {
			beginSignIn: async () => ({
				verificationUri: "https://www.usesynth.ai/signin?redirect_to=x",
				expiresAtEpochS: Math.floor(Date.now() / 1000) + 600
			}),
			pollSignIn: async () => ({ status: "pending" as const }),
			cancelSignIn: async () => undefined,
			signOut: async () => { throw new Error("unused"); },
			getSummary: async () => ({ signedIn: false, state: "local_only" as const, environment: "dev" as const })
		};
	});
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-menu").getByTestId("open-account-settings").click();
	await page.getByTestId("sign-in-begin").click();
	await page.getByTestId("sign-in-cancel").click();
	await expect(page.getByTestId("sign-in-begin")).toBeVisible();
});
