import type { Page } from "@playwright/test";
import { expect, test } from "./browser.fixture";

async function openSettings(page: Page) {
	await page.getByTestId("account-footer-trigger").click();
	await page.getByTestId("settings").click();
}

async function enableComposer(page: Page): Promise<void> {
	await page.addInitScript(() => {
		(window as typeof window & { synthLaguna?: unknown }).synthLaguna = {
			getStatus: async () => ({
				phase: "ready",
				baseUrl: "http://127.0.0.1:7333",
				backend: "mlx_lm",
				loadedModel: "poolside/Laguna-XS-2.1-NVFP4-mlx",
				detail: "Laguna XS ready",
				memoryBytes: null,
				updatedAt: Date.now()
			}),
			reload: async () => ({
				phase: "ready",
				baseUrl: "http://127.0.0.1:7333",
				backend: "mlx_lm",
				loadedModel: "poolside/Laguna-XS-2.1-NVFP4-mlx",
				detail: "Laguna XS ready",
				memoryBytes: null,
				updatedAt: Date.now()
			}),
			onStatus: () => () => undefined,
			listModels: async () => [],
			downloadModel: async () => { throw new Error("unused"); },
			chooseModelDirectory: async () => null,
			setModelDirectory: async () => { throw new Error("unused"); },
			clearModelDirectory: async () => undefined
		};
	});
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await expect(page.getByTestId("composer-input")).toBeEnabled();
}

test("slash button opens command menu and /new returns to landing", async ({ page }) => {
	await enableComposer(page);

	await page.getByTestId("composer-slash-btn").click();
	const menu = page.getByTestId("slash-command-menu");
	await expect(menu).toBeVisible();
	await expect(menu.getByTestId("slash-command-item-new")).toBeVisible();
	await expect(menu.getByTestId("slash-command-item-mode")).toBeVisible();
	await expect(menu.getByTestId("slash-command-item-model")).toBeVisible();
	await expect(menu.getByTestId("slash-command-item-compact")).toContainText("Compact context");

	await menu.getByTestId("slash-command-item-new").click();
	await expect(page.getByTestId("landing-page")).toBeVisible();
});

test("typing /compact offers ad-hoc context compaction", async ({ page }) => {
	await enableComposer(page);

	await page.getByTestId("composer-input").fill("/compact");
	const menu = page.getByTestId("slash-command-menu");
	await expect(menu.getByTestId("slash-command-item-compact")).toBeVisible();
	await expect(menu.getByRole("option")).toHaveCount(1);
});

test("typing / filters slash menu and selecting a skill attaches a chip", async ({ page }) => {
	await page.addInitScript(() => {
		(window as typeof window & { synthSkills?: { list(): Promise<Array<{ id: string; name: string; description: string }>> } }).synthSkills = {
			list: async () => [{
				id: "use-synth-containers",
				name: "use-synth-containers",
				description: "Synth container discovery and Trace V5 evidence."
			}]
		};
	});
	await enableComposer(page);

	await page.getByTestId("composer-input").fill("/use");
	const menu = page.getByTestId("slash-command-menu");
	await expect(menu).toBeVisible();
	await expect(menu.getByTestId("slash-command-item-use-synth-containers")).toBeVisible();
	await menu.getByTestId("slash-command-item-use-synth-containers").click();
	await expect(page.getByTestId("composer-skill-chip")).toContainText("use-synth-containers");
	await expect(page.getByTestId("composer-input")).toHaveValue("");
});

test("Settings Voice lists Whisper models and download selects one", async ({ page }) => {
	await page.addInitScript(() => {
		let selected: string | null = null;
		const catalog = [
			{ id: "tiny", title: "Whisper Tiny", recommended: false, multilingual: true, downloadBytes: 74_000_000, modelsRoot: "/tmp/whisper" },
			{ id: "base", title: "Whisper Base", recommended: true, multilingual: true, downloadBytes: 141_000_000, modelsRoot: "/tmp/whisper" },
			{ id: "small", title: "Whisper Small", recommended: false, multilingual: true, downloadBytes: 465_000_000, modelsRoot: "/tmp/whisper" },
			{ id: "large-v3-turbo", title: "Whisper Large v3 Turbo", recommended: false, multilingual: true, downloadBytes: 1_549_000_000, modelsRoot: "/tmp/whisper" }
		];
		const hits = () => catalog.map((item) => ({
			...item,
			installedBytes: selected === item.id ? item.downloadBytes : null,
			path: selected === item.id ? `/tmp/whisper/${item.id}` : null,
			selected: selected === item.id
		}));
		(window as typeof window & { synthWhisper?: unknown }).synthWhisper = {
			listModels: async () => hits(),
			downloadModel: async (id: string) => {
				selected = id;
				return hits().find((hit) => hit.id === id)!;
			},
			setSelected: async (id: string) => { selected = id; },
			clearModel: async (id: string) => { if (selected === id) selected = null; },
			transcribe: async () => "hello from whisper",
			transcribeAudio: async () => "hello from whisper"
		};
	});
	await page.reload();
	await page.getByTestId("titlebar").waitFor();

	await openSettings(page);
	await page.getByRole("button", { name: "Voice", exact: true }).click();
	const voice = page.getByTestId("voice-recognition-settings");
	await expect(voice).toBeVisible();
	await expect(voice.getByTestId("whisper-model-base")).toContainText("Recommended");
	await voice.getByTestId("download-whisper-base").click();
	await expect(voice.getByTestId("whisper-model-base")).toContainText(/In use|Installed|Delete/i);
});

test("mic without a Whisper model opens Voice settings", async ({ page }) => {
	await page.addInitScript(() => {
		(window as typeof window & { synthWhisper?: unknown }).synthWhisper = {
			listModels: async () => [{
				id: "base",
				title: "Whisper Base",
				recommended: true,
				multilingual: true,
				downloadBytes: 141_000_000,
				installedBytes: null,
				path: null,
				selected: false,
				modelsRoot: "/tmp/whisper"
			}],
			downloadModel: async () => { throw new Error("unused"); },
			setSelected: async () => undefined,
			clearModel: async () => undefined,
			transcribe: async () => "",
			transcribeAudio: async () => ""
		};
	});
	await enableComposer(page);

	await page.getByTestId("composer-mic").click();
	await expect(page.getByTestId("settings-page")).toBeVisible();
	await expect(page.getByTestId("voice-recognition-settings")).toBeVisible();
});
