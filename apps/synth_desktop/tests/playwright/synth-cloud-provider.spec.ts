import { expect, test } from "./browser.fixture";

test("Synth Cloud Laguna S appears under SYNTH CLOUD when api key is configured", async ({ page }) => {
	await page.addInitScript(() => {
		window.synthConfig = {
			get: async () => ({
				configPath: "/tmp/config.toml",
				envFile: "/tmp/.env",
				profile: "prod",
				backendUrl: "https://api.usesynth.ai",
				apiKeyEnv: "SYNTH_API_KEY",
				apiKeyConfigured: true,
				apiKeyFingerprint: "sk_dev…0001",
				apiKeySource: "env_file",
				workerKeyConfigured: false,
				openrouterApiKeyConfigured: false
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

	await page.getByTestId("composer-model").click();
	const menu = page.getByTestId("composer-model-menu");
	await expect(menu).toBeVisible();

	const cloudGroup = menu.locator(".composer-model-group").filter({
		has: page.locator(".composer-model-group-label", { hasText: "Synth Cloud" })
	});
	await expect(cloudGroup.getByTestId("composer-model-option-synth-cloud-laguna-s")).toBeVisible();
	await expect(cloudGroup.getByTestId("composer-model-option-synth-cloud-laguna-s"))
		.toHaveText(/Laguna S 2\.1/);
	await expect(cloudGroup.getByTestId("composer-model-option-synth-cloud-laguna-s"))
		.not.toContainText("usage tracked");
	await expect(cloudGroup.getByTestId("composer-model-option-synth-cloud-laguna-s"))
		.not.toHaveAttribute("aria-disabled", "true");

	// OpenRouter Laguna stays under Remote · OpenRouter, not Synth Cloud.
	const remoteGroup = menu.locator(".composer-model-group").filter({
		has: page.locator(".composer-model-group-label", { hasText: "Remote · OpenRouter" })
	});
	await expect(remoteGroup.getByTestId("composer-model-option-openrouter-laguna-s")).toBeVisible();
	await expect(cloudGroup.getByTestId("composer-model-option-openrouter-laguna-s")).toHaveCount(0);

	await page.getByTestId("composer-model-option-synth-cloud-laguna-s").click();
	await expect(page.getByTestId("composer-model")).toHaveAccessibleName(/Laguna S 2\.1/);
	await page.getByTestId("composer-model").click();
	const advanced = page.getByTestId("composer-model-advanced");
	await advanced.locator("summary").click();
	await expect(advanced).toContainText("Synth Cloud · usage tracked");
});

test("Synth Cloud Laguna S is gated when api key is missing", async ({ page }) => {
	// Default browser stub has apiKeyConfigured: false — do not override it.
	await page.getByTestId("composer-model").click();
	const option = page.getByTestId("composer-model-option-synth-cloud-laguna-s");
	await expect(option).toBeVisible();
	await expect(option.getByRole("option")).toHaveAttribute("aria-disabled", "true");
	await expect(option).toContainText("Configure Synth API key");

	await page.getByTestId("composer-model-configure-synth-api-key").click();
	await expect(page.getByTestId("settings-page")).toBeVisible();
	await expect(page.getByTestId("settings-account")).toBeVisible();
});

test("OpenRouter models are gated and link directly to Account settings", async ({ page }) => {
	await page.getByTestId("composer-model").click();
	const option = page.getByTestId("composer-model-option-openrouter-luna");
	await expect(option.getByRole("option")).toHaveAttribute("aria-disabled", "true");
	await expect(option).toContainText("OpenRouter API key required");
	await option.getByTestId("composer-model-configure-openrouter-api-key").click();
	await expect(page.getByTestId("settings-account")).toBeVisible();
});

test("a removed OpenRouter key rejects the message before creating a session", async ({ page }) => {
	await page.addInitScript(() => {
		let configured = true;
		const testWindow = window as typeof window & {
			__setOpenRouterConfigured?: (value: boolean) => void;
		};
		testWindow.__setOpenRouterConfigured = (value) => { configured = value; };
		window.synthConfig = {
			get: async () => ({
				configPath: "/tmp/config.toml", envFile: "/tmp/.env", profile: "prod",
				backendUrl: "https://api.usesynth.ai", apiKeyEnv: "SYNTH_API_KEY",
				apiKeyConfigured: false, workerKeyConfigured: false,
				openrouterApiKeyConfigured: configured
			}),
			update: async () => { throw new Error("unused"); },
			listModelMultiAgent: async () => [], updateModelMultiAgent: async () => [],
			getWorkspaceAccess: async () => ({ allowedRoots: [] }),
			updateWorkspaceAccess: async () => ({ allowedRoots: [] })
		};
	});
	await page.reload();
	await page.getByTestId("composer-model").click();
	await page.getByTestId("composer-model-option-openrouter-luna").click();
	await expect(page.getByTestId("composer-input")).toBeEnabled();

	await page.evaluate(() => {
		const testWindow = window as typeof window & {
			__setOpenRouterConfigured: (value: boolean) => void;
			__openRouterSessionCreates?: number;
		};
		testWindow.__setOpenRouterConfigured(false);
		testWindow.__openRouterSessionCreates = 0;
		const runtime = window.synthRuntime!;
		const request = runtime.request.bind(runtime);
		runtime.request = async (path, options) => {
			if (path === "/v1/sessions" && options?.method === "POST") {
				testWindow.__openRouterSessionCreates! += 1;
			}
			return request(path, options);
		};
	});
	await page.getByTestId("composer-input").fill("must not leave this device");
	await page.getByTestId("composer-send").click();

	await expect(page.getByTestId("settings-account")).toBeVisible();
	expect(await page.evaluate(() => (window as typeof window & { __openRouterSessionCreates?: number }).__openRouterSessionCreates)).toBe(0);
});
