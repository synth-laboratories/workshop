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
