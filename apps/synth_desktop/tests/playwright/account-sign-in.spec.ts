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
	await page.getByRole("button", { name: /Back/ }).click();
	await page.getByTestId("account-menu-trigger").click();
	await expect(page.getByTestId("account-menu")).toContainText("Synth Dev");
	await expect(page.getByTestId("account-open-usage")).toContainText("Usage");
	await expect(page.getByTestId("account-menu")).toContainText("Settings");
	await expect(page.getByTestId("account-menu")).toContainText("Log out");
	await expect(page.getByTestId("account-menu-status-note")).toHaveCount(0);
	await expect(page.getByTestId("account-usage")).toHaveCount(0);
	await page.getByTestId("account-open-usage").click();
	// A dev stand-in is not an authoritative Synth Cloud plan: the sheet says
	// so plainly and never dresses it in allowance dollars.
	await expect(page.getByTestId("usage-sheet-dev-seed")).toContainText("Dev stand-in");
	await expect(page.getByTestId("usage-sheet-allowance")).toHaveCount(0);
	await expect(page.getByTestId("usage-sheet-used")).toHaveCount(0);
	await expect(page.getByTestId("usage-sheet-remaining")).toHaveCount(0);
	// The device dashboard is the real content of this sheet.
	await expect(page.getByTestId("usage-sheet-device")).toContainText("This device");
	await page.getByTestId("usage-sheet-close").click();

	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-menu").getByTestId("open-account-settings").click();
	const signedInSettings = page.getByTestId("account-sign-in");
	await signedInSettings.getByTestId("account-sign-out").click();
	await expect(page.getByTestId("backend-settings")).toContainText("API key required");
	await expect(signedInSettings.getByTestId("sign-in-status")).toContainText("creates your Synth account");
});

test("account offers a top-level write-only Synth API key path", async ({ page }) => {
	await page.addInitScript(() => {
		let configured = false;
		const base = {
			configPath: "/tmp/config.toml",
			envFile: "/tmp/.env",
			profile: "prod",
			backendUrl: "https://api.usesynth.ai",
			apiKeyEnv: "SYNTH_API_KEY",
			workerKeyConfigured: false,
			openrouterApiKeyConfigured: false
		};
		window.synthAccount = {
			beginSignIn: async () => { throw new Error("unused"); },
			pollSignIn: async () => ({ status: "pending" as const }),
			cancelSignIn: async () => undefined,
			signOut: async () => ({ ...base, apiKeyConfigured: false }),
			getSummary: async () => ({ signedIn: configured, state: configured ? "active" as const : "signed_out" as const, environment: "prod" as const })
		};
		window.synthConfig = {
			get: async () => ({ ...base, apiKeyConfigured: configured }),
			update: async (request) => {
				if (request.apiKey !== "sk_test_manual") throw new Error("API key was not forwarded");
				configured = true;
				return { ...base, apiKeyConfigured: true, apiKeyFingerprint: "sha256:abc1234", apiKeySource: "/tmp/.env" };
			},
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

	const profile = page.getByTestId("account-page-profile");
	const signIn = profile.getByTestId("account-sign-in");
	await expect(signIn.getByTestId("sign-in-begin")).toBeVisible();
	await expect(signIn.getByTestId("api-key-toggle")).toBeVisible();
	await signIn.getByTestId("api-key-toggle").click();
	await signIn.getByLabel("Synth API key").fill("sk_test_manual");
	await signIn.getByRole("button", { name: "Connect", exact: true }).click();

	await expect(signIn.getByTestId("account-sign-in-note")).toContainText("API key connected");
	await expect(signIn.getByLabel("Synth API key")).toHaveCount(0);
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

/*
 * Signed-out account menu, per HANDOFF_CLOUD_ACCOUNT_QA.md A3: no Log out, and
 * `Usage remaining` expands to an invitation rather than an invented `$0.00`.
 */
test("signed out, the menu offers sign-in and never a zero cloud allowance", async ({ page }) => {
	await page.addInitScript(() => {
		window.synthAccount = {
			beginSignIn: async () => ({ verificationUri: "https://example.test", expiresAtEpochS: 0 }),
			pollSignIn: async () => ({ status: "pending" as const }),
			cancelSignIn: async () => undefined,
			signOut: async () => { throw new Error("unused"); },
			getSummary: async () => ({ signedIn: false, state: "local_only" as const, environment: "dev" as const })
		};
	});
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByTestId("account-menu-trigger").click();

	const menu = page.getByTestId("account-menu");
	await expect(menu.getByTestId("account-log-out")).toHaveCount(0);
	await expect(menu.getByTestId("account-usage-remaining-value")).toHaveCount(0);

	await page.getByTestId("account-usage-remaining").click();
	const panel = page.getByTestId("account-allowance-panel");
	await expect(panel).toHaveText("Sign in to Synth to see a cloud allowance");
	await expect(panel).not.toContainText("$");
});
