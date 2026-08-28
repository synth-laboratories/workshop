import { expect, test } from "./browser.fixture";
import { BROWSER_MODEL_CATALOG } from "../../src/renderer/src/runtime/modelCatalog";

test("Synth Cloud hosted models appear under SYNTH CLOUD when api key is configured", async ({ page }) => {
	await page.addInitScript((catalog) => {
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
			modelCatalog: async () => catalog,
			refreshModelCatalog: async () => catalog,
			update: async () => { throw new Error("unused"); },
			listModelMultiAgent: async () => [],
			updateModelMultiAgent: async () => [],
			getWorkspaceAccess: async () => ({ allowedRoots: [] }),
			updateWorkspaceAccess: async () => ({ allowedRoots: [] })
		};
	}, BROWSER_MODEL_CATALOG);
	await page.reload();
	await page.getByTestId("titlebar").waitFor();

	await page.getByTestId("composer-model").click();
	await page.getByTestId("composer-model-access-api").click();
	const menu = page.getByTestId("composer-model-menu");
	await expect(menu).toBeVisible();
	await expect(menu).not.toContainText(/Muse Glimmer|GGUF|DFlash/i);

	const cloudGroup = menu.locator(".composer-model-group").filter({
		has: page.locator(".composer-model-group-label", { hasText: "Synth" })
	});
	await expect(cloudGroup.getByTestId("composer-model-option-synth-cloud-laguna-s")).toBeVisible();
	await expect(cloudGroup.getByTestId("composer-model-option-synth-cloud-laguna-s"))
		.toHaveText(/Laguna S 2\.1/);
	await expect(cloudGroup.getByTestId("composer-model-option-synth-cloud-laguna-s"))
		.not.toContainText("usage tracked");
	await expect(cloudGroup.getByTestId("composer-model-option-synth-cloud-laguna-s"))
		.not.toHaveAttribute("aria-disabled", "true");
	await expect(cloudGroup.getByTestId("composer-model-option-synth-cloud-laguna-xs-b200"))
		.toHaveText(/Laguna XS 2\.1.*B200/);
	await expect(cloudGroup.getByTestId("composer-model-option-synth-cloud-laguna-xs-h100"))
		.toHaveText(/Laguna XS 2\.1.*H100/);
	await expect(cloudGroup.getByTestId("composer-model-option-synth-cloud-muse-spark"))
		.toHaveText(/Muse Spark 1\.2/);
	await expect(cloudGroup.getByTestId("composer-model-option-synth-cloud-muse-spark"))
		.not.toHaveAttribute("aria-disabled", "true");

	// OpenRouter Laguna stays under its third-party provider, not Synth.
	const remoteGroup = menu.locator(".composer-model-group").filter({
		has: page.locator(".composer-model-group-label", { hasText: "OpenRouter" })
	});
	await expect(remoteGroup.getByTestId("composer-model-option-openrouter-laguna-s")).toBeVisible();
	await expect(remoteGroup.getByTestId("composer-model-option-openrouter-muse-spark")).toHaveText(/Muse Spark 1\.2/);
	await expect(remoteGroup.getByTestId("composer-model-option-openrouter-gemini-flash")).toHaveText(/Gemini 3\.7 Flash/);
	await expect(cloudGroup.getByTestId("composer-model-option-openrouter-laguna-s")).toHaveCount(0);

	await page.getByTestId("composer-model-option-synth-cloud-muse-spark").click();
	await expect(page.getByTestId("composer-model")).toHaveAccessibleName(/Muse Spark 1\.2/);
	await page.getByTestId("composer-model").click();
	await page.getByTestId("composer-model-access-api").click();
	const advanced = page.getByTestId("composer-model-advanced");
	await advanced.locator("summary").click();
	await expect(advanced).toContainText("Synth Cloud · usage tracked");
});

test("Synth Cloud Laguna S is gated when api key is missing", async ({ page }) => {
	// Default browser stub has apiKeyConfigured: false — do not override it.
	await page.getByTestId("composer-model").click();
	await page.getByTestId("composer-model-access-api").click();
	const option = page.getByTestId("composer-model-option-synth-cloud-laguna-s");
	await expect(option).toBeVisible();
	await expect(option.getByRole("option")).toHaveAttribute("aria-disabled", "true");
	await expect(option).toContainText("Configure Synth API key");

	await option.getByTestId("composer-model-configure-synth-api-key").click();
	await expect(page.getByTestId("settings-page")).toBeVisible();
	await expect(page.getByTestId("settings-account")).toBeVisible();
});

test("OpenRouter models are gated and link directly to Account settings", async ({ page }) => {
	await page.getByTestId("composer-model").click();
	await page.getByTestId("composer-model-access-api").click();
	const option = page.getByTestId("composer-model-option-openrouter-luna");
	await expect(option.getByRole("option")).toHaveAttribute("aria-disabled", "true");
	await expect(option).toContainText("OpenRouter API key required");
	await option.getByTestId("composer-model-configure-openrouter-api-key").click();
	await expect(page.getByTestId("settings-account")).toBeVisible();
});

test("a removed OpenRouter key rejects the message before creating a session", async ({ page }) => {
	await page.addInitScript((catalog) => {
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
			modelCatalog: async () => catalog,
			refreshModelCatalog: async () => catalog,
			update: async () => { throw new Error("unused"); },
			listModelMultiAgent: async () => [], updateModelMultiAgent: async () => [],
			getWorkspaceAccess: async () => ({ allowedRoots: [] }),
			updateWorkspaceAccess: async () => ({ allowedRoots: [] })
		};
	}, BROWSER_MODEL_CATALOG);
	await page.reload();
	await page.getByTestId("composer-model").click();
	await page.getByTestId("composer-model-access-api").click();
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

test("configured Luna replaces a stale remembered chat and sends the first message", async ({ page }) => {
	await page.addInitScript((catalog) => {
		window.localStorage.setItem("synth.preferences.v1", JSON.stringify({
			schemaVersion: 4,
			layout: { last: { selectedConversationId: "missing-chat" } }
		}));
		const testWindow = window as typeof window & {
			__lunaStarts?: number;
			__lunaSends?: string[];
			synthCodex?: unknown;
		};
		testWindow.__lunaStarts = 0;
		testWindow.__lunaSends = [];
		testWindow.synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [],
			start: async () => {
				testWindow.__lunaStarts! += 1;
				throw new Error("first send must be owned by atomic sendTurn");
			},
			startTurn: async () => { throw new Error("sendTurn should own the first send"); },
			sendTurn: async (request: { sessionId: string }, prompt: string) => {
				testWindow.__lunaSends!.push(prompt);
				return { sessionId: request.sessionId, threadId: "luna-thread", turnId: "luna-turn" };
			},
			interrupt: async () => undefined, close: async () => undefined,
			onEvent: () => () => undefined
		};
		window.synthConfig = {
			get: async () => ({
				configPath: "/tmp/config.toml", envFile: "/tmp/.env", profile: "prod",
				backendUrl: "https://api.usesynth.ai", apiKeyEnv: "SYNTH_API_KEY",
				apiKeyConfigured: false, workerKeyConfigured: false,
				openrouterApiKeyConfigured: true
			}),
			modelCatalog: async () => catalog,
			refreshModelCatalog: async () => catalog,
			update: async () => { throw new Error("unused"); },
			listModelMultiAgent: async () => [], updateModelMultiAgent: async () => [],
			getWorkspaceAccess: async () => ({ allowedRoots: [] }),
			updateWorkspaceAccess: async () => ({ allowedRoots: [] })
		};
	}, BROWSER_MODEL_CATALOG);
	await page.reload();
	await expect(page.getByRole("group", { name: /chat tab$/ })).toBeVisible();
	await page.getByTestId("composer-model").click();
	await page.getByTestId("composer-model-access-api").click();
	await page.getByTestId("composer-model-option-openrouter-luna").click();
	await page.getByTestId("composer-input").fill("first Luna message");
	await page.getByTestId("composer-send").click();
	await expect(page.getByTestId("model-working")).toBeVisible();
	const calls = await page.evaluate(() => {
		const testWindow = window as typeof window & { __lunaStarts?: number; __lunaSends?: string[] };
		return { starts: testWindow.__lunaStarts, sends: testWindow.__lunaSends };
	});
	expect(calls).toEqual({ starts: 0, sends: ["first Luna message"] });
});
