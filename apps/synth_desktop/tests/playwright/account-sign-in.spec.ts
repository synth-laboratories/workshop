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
			}
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
	await page.getByTestId("runtime-status").waitFor();

	await page.getByTestId("open-account-settings").click();
	const signIn = page.getByTestId("account-sign-in");
	await expect(signIn.getByTestId("sign-in-begin")).toContainText("Sign in with browser");
	await expect(signIn).toContainText("creates your Synth account");

	await signIn.getByTestId("sign-in-begin").click();
	await expect(signIn.getByTestId("sign-in-status")).toContainText("Finish sign-in in your browser");

	// Two 4s poll ticks flip the stub to paired.
	await expect(page.getByTestId("backend-settings")).toContainText("Authenticated", { timeout: 15_000 });
	await expect(signIn.getByTestId("sign-in-status")).toContainText("Connected to Synth");

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
			signOut: async () => { throw new Error("unused"); }
		};
	});
	await page.reload();
	await page.getByTestId("runtime-status").waitFor();
	await page.getByTestId("open-account-settings").click();
	await page.getByTestId("sign-in-begin").click();
	await page.getByTestId("sign-in-cancel").click();
	await expect(page.getByTestId("sign-in-begin")).toBeVisible();
});
