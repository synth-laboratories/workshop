import type { Page } from "@playwright/test";
import { expect, test } from "./browser.fixture";

async function openSettings(page: Page) {
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-menu-settings").click();
}

type LagunaPhase = "starting" | "loading" | "ready";

async function installLagunaFixture(page: Page, phase: LagunaPhase): Promise<void> {
	await page.addInitScript((initialPhase) => {
		let selectedPath = "/models/poolside/Laguna-XS-2.1-NVFP4-mlx";
		const hits = () => [{
			path: selectedPath,
			modelsRoot: "/models",
			modelId: "poolside/Laguna-XS-2.1-NVFP4-mlx",
			shardCount: 8,
			totalBytes: 21_600_000_000,
			selected: true
		}];
		(window as typeof window & { synthLaguna?: unknown }).synthLaguna = {
			getStatus: async () => ({
				phase: initialPhase,
				baseUrl: "http://127.0.0.1:7333",
				backend: "mlx_lm",
				loadedModel: initialPhase === "ready" ? "poolside/Laguna-XS-2.1-NVFP4-mlx" : null,
				detail: initialPhase === "ready" ? "Laguna XS ready" : `${initialPhase} Laguna XS`,
				memoryBytes: null,
				updatedAt: Date.now()
			}),
			onStatus: () => () => undefined,
			listModels: async () => hits(),
			chooseModelDirectory: async () => "/models/custom/Laguna-XS-2.1-NVFP4-mlx",
			setModelDirectory: async (path: string) => {
				selectedPath = path;
				return hits()[0];
			},
			clearModelDirectory: async () => undefined
		};
	}, phase);
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
}

async function installConfiguredOpenRouter(page: Page): Promise<void> {
	await page.addInitScript(() => {
		window.synthConfig = {
			get: async () => ({
				configPath: "/tmp/config.toml", envFile: "/tmp/.env", profile: "prod",
				backendUrl: "https://api.usesynth.ai", apiKeyEnv: "SYNTH_API_KEY",
				apiKeyConfigured: false, workerKeyConfigured: false,
				openrouterApiKeyConfigured: true
			}),
			update: async () => { throw new Error("unused"); },
			listModelMultiAgent: async () => [], updateModelMultiAgent: async () => [],
			getWorkspaceAccess: async () => ({ allowedRoots: [] }),
			updateWorkspaceAccess: async () => ({ allowedRoots: [] })
		};
	});
}

test("native Laguna readiness overrides missing legacy runtime health", async ({ page }) => {
	await installLagunaFixture(page, "ready");

	await expect(page.getByTestId("composer-input")).toBeEnabled();
	await expect(page.getByTestId("composer-input")).toHaveAttribute("placeholder", "Ask Laguna something…");
	await expect(page.getByTestId("composer-model")).toHaveAccessibleName(/Laguna XS 2\.1/);
	await expect(page.getByTestId("composer-model")).not.toHaveAccessibleName(/offline|starting/i);
	await expect(page.getByTestId("runtime-status")).toHaveCount(0);
});

for (const phase of ["starting", "loading"] as const) {
	test(`the model menu describes ${phase} without fake download progress`, async ({ page }) => {
		await installLagunaFixture(page, phase);
		await page.getByTestId("composer-model").click();

		const menu = page.getByTestId("composer-model-menu");
		await expect(menu).toBeVisible();
		await expect(menu).not.toContainText(/Downloading…?\s*0%/i);
		await expect(menu).toContainText(phase === "loading" ? "Loading local weights…" : "Connecting to local runtime…");
	});
}

test("a blocked local startup does not trap remote or cloud target selection", async ({ page }) => {
	await installConfiguredOpenRouter(page);
	await installLagunaFixture(page, "starting");
	await page.getByTestId("composer-model").click();

	const local = page.getByRole("option", { name: /Laguna XS 2\.1/ }).first();
	await expect(local).toBeDisabled();
	await page.getByTestId("composer-model-option-openrouter-luna").click();
	await expect(page.getByTestId("composer-model")).toHaveAccessibleName(/GPT 5\.6 Luna/);
	await expect(page.getByTestId("reasoning-effort-select")).toHaveAccessibleName("Reasoning effort: Medium");
	await expect(page.getByTestId("composer-input")).toBeEnabled();

	await page.getByTestId("composer-model").click();
	await page.getByTestId("composer-model-option-openrouter-laguna-s").click();
	await expect(page.getByTestId("composer-model")).toHaveAccessibleName(/Laguna S 2\.1/);
	await expect(page.getByTestId("composer-input")).toBeEnabled();
});

test("Settings exposes discovered models and accepts a chosen folder", async ({ page }) => {
	await installLagunaFixture(page, "ready");
	await openSettings(page);
	await page.getByTestId("settings-page").getByRole("button", { name: "Models" }).click();

	const locations = page.getByTestId("laguna-model-locations");
	await expect(locations).toBeVisible();
	await expect(locations).toContainText("/models/poolside/Laguna-XS-2.1-NVFP4-mlx");
	await expect(locations.getByText("In use")).toBeVisible();
	await locations.getByRole("button", { name: "Choose folder…" }).click();
	await expect(locations).toContainText("/models/custom/Laguna-XS-2.1-NVFP4-mlx");
});

test("Settings offers and completes a real model-download bridge when weights are absent", async ({ page }) => {
	await page.addInitScript(() => {
		let downloaded = false;
		const hit = {
			path: "/models/poolside/Laguna-XS-2.1-NVFP4-mlx",
			modelsRoot: "/models",
			modelId: "poolside/Laguna-XS-2.1-NVFP4-mlx",
			shardCount: 5,
			totalBytes: 21_600_000_000,
			selected: true
		};
		(window as typeof window & { synthLaguna?: unknown }).synthLaguna = {
			getStatus: async () => ({ phase: "ready", baseUrl: "http://127.0.0.1:7333", backend: "mlx_lm", loadedModel: null, detail: "ready", memoryBytes: 0, updatedAt: Date.now() }),
			onStatus: () => () => undefined,
			listModels: async () => downloaded ? [hit] : [],
			downloadModel: async () => { downloaded = true; return hit; },
			chooseModelDirectory: async () => null,
			setModelDirectory: async () => hit,
			clearModelDirectory: async () => undefined
		};
	});
	await page.reload();
	await openSettings(page);
	await page.getByTestId("settings-page").getByRole("button", { name: "Models" }).click();
	await page.getByTestId("settings-page").getByRole("button", { name: "Download", exact: true }).click();
	const locations = page.getByTestId("laguna-model-locations");
	await expect(locations).toContainText("/models/poolside/Laguna-XS-2.1-NVFP4-mlx");
	await expect(locations.getByText("In use")).toBeVisible();
});

test("Settings identifies the exact running desktop build", async ({ page }) => {
	await openSettings(page);
	await page.getByRole("button", { name: "About" }).click();
	const identity = page.getByTestId("about-build-identity");
	await expect(identity).toContainText("Synth Desktop · browser");
	await expect(identity).toContainText("source vite · build vite");
	await expect(page).toHaveTitle("Synth Desktop · browser");
});

test("Models lists only credentialed remote providers with pricing", async ({ page }) => {
	await page.addInitScript(() => {
		window.synthConfig = {
			get: async () => ({ configPath: "/tmp/config.toml", envFile: "/tmp/.env", profile: "prod", backendUrl: "https://api.usesynth.ai", apiKeyEnv: "SYNTH_API_KEY", apiKeyConfigured: true, workerKeyConfigured: false, openrouterApiKeyConfigured: true }),
			update: async () => { throw new Error("unused"); }, listModelMultiAgent: async () => [], updateModelMultiAgent: async () => [],
			getWorkspaceAccess: async () => ({ allowedRoots: [] }), updateWorkspaceAccess: async () => ({ allowedRoots: [] })
		};
		window.synthTariffs = {
			catalog: async () => [
				{ provider: "openrouter", modelId: "openai/gpt-5.6-luna", inputUsdPerM: 0.20, outputUsdPerM: 1.20, cachedInputUsdPerM: 0.02, cacheWriteUsdPerM: 0.25 },
				{ provider: "openrouter", modelId: "poolside/laguna-s-2.1", inputUsdPerM: 0.10, outputUsdPerM: 0.20, cachedInputUsdPerM: null, cacheWriteUsdPerM: null },
				{ provider: "openrouter", modelId: "meta/muse-spark-1.2", inputUsdPerM: 1.25, outputUsdPerM: 4.25, cachedInputUsdPerM: 0.15, cacheWriteUsdPerM: null }
			]
		};
	});
	await page.reload();
	await openSettings(page);
	await page.getByRole("button", { name: "Models" }).click();
	const models = page.getByTestId("authorized-models");
	const luna = models.getByTestId("authorized-model-openrouter-luna");
	await expect(luna).toContainText("$0.20");
	await expect(luna).toContainText("$1.20");
	await expect(luna).toContainText("Cached read / 1M$0.02");
	await expect(luna).toContainText("Cache write / 1M$0.25");
	await expect(models.getByTestId("authorized-model-openrouter-laguna-s")).toContainText("$0.20");
	await expect(models.getByTestId("authorized-model-openrouter-muse-spark")).toContainText("$4.25");
	await expect(models.getByTestId("authorized-model-synth-cloud-laguna-s")).toContainText("Plan");
	const marks = models.locator(".authorized-model-mark");
	await expect(marks).toHaveCount(5);
	const markBoxes = await marks.evaluateAll((elements) => elements.map((element) => {
		const box = element.getBoundingClientRect();
		return { width: box.width, height: box.height, centerX: box.left + box.width / 2 };
	}));
	for (const box of markBoxes) {
		expect(box.width, "authorized-provider logos stay visually quiet").toBeLessThanOrEqual(22);
		expect(box.height, "authorized-provider logos stay visually quiet").toBeLessThanOrEqual(22);
	}
	expect(Math.max(...markBoxes.map((box) => box.centerX)) - Math.min(...markBoxes.map((box) => box.centerX)), "provider marks share one centerline").toBeLessThanOrEqual(1);
	const slugStyles = await models.locator(".authorized-model-identity code").evaluateAll((elements) =>
		elements.map((element) => {
			const style = getComputedStyle(element);
			return { fontSize: Number.parseFloat(style.fontSize), family: style.fontFamily };
		})
	);
	expect(slugStyles).toHaveLength(5);
	for (const style of slugStyles) {
		expect(style.fontSize, "model slugs stay subordinate to provider labels").toBeLessThanOrEqual(10);
		expect(style.family).toMatch(/SFMono|Menlo|Monaco|Consolas|monospace/i);
	}
});

test("About offers the download page when a newer release exists, and stays quiet otherwise", async ({ page }) => {
	await page.addInitScript(() => {
		(window as unknown as { __updateOpens: number }).__updateOpens = 0;
		window.synthUpdates = {
			status: async () => ({
				currentVersion: "0.1.0",
				channel: "stable",
				latestVersion: "0.1.2",
				updateAvailable: true
			}),
			openDownload: async () => {
				(window as unknown as { __updateOpens: number }).__updateOpens += 1;
			}
		};
	});
	await page.reload();
	await openSettings(page);
	await page.getByRole("button", { name: "About" }).click();
	const identity = page.getByTestId("about-build-identity");
	await expect(identity).toContainText("· stable ·");
	const affordance = page.getByTestId("about-update-available");
	await expect(affordance).toHaveText("Update available · v0.1.2");
	await affordance.click();
	await expect
		.poll(() => page.evaluate(() => (window as unknown as { __updateOpens: number }).__updateOpens))
		.toBe(1);
});

test("About shows no update affordance when the release is current", async ({ page }) => {
	await openSettings(page);
	await page.getByRole("button", { name: "About" }).click();
	await expect(page.getByTestId("about-build-identity")).toBeVisible();
	await expect(page.getByTestId("about-update-available")).toHaveCount(0);
});

test("Settings can force and reset a model multi-agent preset", async ({ page }) => {
	await page.addInitScript(() => {
		const preset = {
			modelId: "laguna-xs-2.1",
			displayName: "Laguna XS 2.1",
			preset: "none" as const,
			effective: "none" as "none" | "v1" | "v2",
			overridden: false
		};
		(window as typeof window & { __multiAgentUpdates?: unknown[] }).__multiAgentUpdates = [];
		(window as typeof window & { synthConfig?: unknown }).synthConfig = {
			get: async () => { throw new Error("unused"); },
			update: async () => { throw new Error("unused"); },
			listModelMultiAgent: async () => [{ ...preset }],
			updateModelMultiAgent: async (request: { modelId: string; version?: "none" | "v1" | "v2" | null }) => {
				(window as typeof window & { __multiAgentUpdates: unknown[] }).__multiAgentUpdates.push(request);
				preset.effective = request.version ?? preset.preset;
				preset.overridden = request.version != null;
				return [{ ...preset }];
			}
		};
	});
	await installLagunaFixture(page, "ready");
	await openSettings(page);
	await page.getByTestId("settings-page").getByRole("button", { name: "Models" }).click();

	const controls = page.getByRole("group", { name: "Laguna XS 2.1 multi-agent compatibility" });
	const row = controls.locator("..");
	await expect(row).toContainText("[agents] enabled=false · [features] multi_agent=false · multi_agent_v2=false");
	await controls.getByRole("button", { name: "V1" }).click();
	await expect(row).toContainText("V1 namespaced collaboration tools");
	await expect(row).toContainText("V1 does not use V2 encrypted message or tool payloads");
	await expect(row).toContainText("[agents] enabled=true · [features] multi_agent=true · multi_agent_v2=false");
	await controls.getByRole("button", { name: "V2" }).click();
	await expect(row).toContainText("V2 direct collaboration tools, agent-message routing, and encrypted message/tool payloads");
	await expect(row).toContainText("[agents] enabled=true · [features] multi_agent=true · multi_agent_v2=true");
	await controls.getByRole("button", { name: "Reset" }).click();
	await expect(row).not.toContainText("Override exposes");
	expect(await page.evaluate(() => (window as typeof window & { __multiAgentUpdates: unknown[] }).__multiAgentUpdates)).toEqual([
		{ modelId: "laguna-xs-2.1", version: "v1" },
		{ modelId: "laguna-xs-2.1", version: "v2" },
		{ modelId: "laguna-xs-2.1", version: null }
	]);
});

test("V1 collaboration events drive the first-class Subagents visual without treating idle as done", async ({ page }) => {
	await page.addInitScript(() => {
		type Event = { sessionId: string; method: string; params: Record<string, unknown> };
		let listener: ((event: Event) => void) | undefined;
		(window as typeof window & { __emitCodexEvent?: (event: Event) => void }).__emitCodexEvent = (event) => listener?.(event);
		(window as typeof window & { synthCodex?: unknown }).synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId: "subagent-session",
				threadId: "parent-thread",
				workspace: "/workspaces/default",
					model: "openai/gpt-5.6-luna",
				providerName: "openrouter",
				providerTitle: "OpenRouter",
				baseUrl: "https://openrouter.ai/api/v1",
				status: "ready"
			}],
			start: async () => ({ sessionId: "subagent-session", threadId: "parent-thread" }),
			startTurn: async () => ({ sessionId: "subagent-session", threadId: "parent-thread", turnId: "turn-1" }),
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: (next: (event: Event) => void) => { listener = next; return () => { listener = undefined; }; }
		};
	});
	await page.reload();
	await page.getByTestId("local-chat-subagent-session").click();

	await page.evaluate(() => {
		const emit = (window as typeof window & { __emitCodexEvent: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitCodexEvent;
		const send = (method: string, params: Record<string, unknown>) => emit({ sessionId: "subagent-session", method, params });
		send("agentMessage/completed", { messageId: "parent-message", content: "I’ll delegate the migration review." });
		send("item/started", { item: { id: "call-1", type: "collabAgentToolCall", tool: "spawnAgent", prompt: "Review migration safety. Check the runtime boundary." } });
		send("item/completed", { item: {
			id: "call-1", type: "collabAgentToolCall", tool: "spawnAgent", prompt: "Review migration safety. Check the runtime boundary.",
			receiverThreadIds: ["child-thread"], agentsStates: { "child-thread": { status: "running" } }
		} });
		send("thread/status/changed", { threadId: "child-thread", status: { type: "active" } });
	});

	const visual = page.getByTestId("visual-subagents");
	await expect(visual).toBeVisible();
	await expect(visual).toContainText("Working · 1");
	await expect(visual).toContainText("Review migration safety");
	await expect(page.getByText("Review migration safety started")).toBeVisible();

	await page.evaluate(() => {
		const emit = (window as typeof window & { __emitCodexEvent: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitCodexEvent;
		emit({ sessionId: "subagent-session", method: "agentMessage/completed", params: { threadId: "child-thread", messageId: "child-message", content: "Migration boundary is safe." } });
		emit({ sessionId: "subagent-session", method: "thread/status/changed", params: { threadId: "child-thread", status: { type: "idle" } } });
	});

	await expect(visual).toContainText("Working · 1");
	await expect(visual).toContainText("Completed · 0");
	await expect(page.getByTestId("chat-transcript")).not.toContainText("Migration boundary is safe.");

	await page.evaluate(() => {
		const emit = (window as typeof window & { __emitCodexEvent: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitCodexEvent;
		emit({ sessionId: "subagent-session", method: "item/completed", params: { item: {
			id: "wait-1", type: "collabAgentToolCall", tool: "wait", receiverThreadIds: ["child-thread"],
			agentsStates: { "child-thread": { status: "completed", message: "Migration boundary is safe." } }
		} } });
	});

	await expect(visual).toContainText("Working · 0");
	await expect(visual).toContainText("Completed · 1");
	await expect(visual).toContainText("Migration boundary is safe.");
	await expect(page.getByTestId("chat-transcript")).not.toContainText("Migration boundary is safe.");
});

test("V2 subAgentActivity and child turn lifecycle drive the same first-class Subagents visual", async ({ page }) => {
	await page.addInitScript(() => {
		type Event = { sessionId: string; method: string; params: Record<string, unknown> };
		let listener: ((event: Event) => void) | undefined;
		(window as typeof window & { __emitCodexV2Event?: (event: Event) => void }).__emitCodexV2Event = (event) => listener?.(event);
		(window as typeof window & { synthCodex?: unknown }).synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId: "subagent-v2-session", threadId: "parent-v2-thread", workspace: "/workspaces/default",
				model: "openai/gpt-5.6-terra", providerName: "openrouter", providerTitle: "OpenRouter",
				baseUrl: "https://openrouter.ai/api/v1", status: "ready"
			}],
			start: async () => ({ sessionId: "subagent-v2-session", threadId: "parent-v2-thread" }),
			startTurn: async () => ({ sessionId: "subagent-v2-session", threadId: "parent-v2-thread", turnId: "turn-1" }),
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: (next: (event: Event) => void) => { listener = next; return () => { listener = undefined; }; }
		};
	});
	await page.reload();
	await page.getByTestId("local-chat-subagent-v2-session").click();

	await page.evaluate(() => {
		const emit = (window as typeof window & { __emitCodexV2Event: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitCodexV2Event;
		const send = (method: string, params: Record<string, unknown>) => emit({ sessionId: "subagent-v2-session", method, params });
		send("agentMessage/completed", { messageId: "parent-v2-message", content: "I’ll delegate the README location audit." });
		send("item/started", { item: {
			id: "spawn-v2-1", type: "subAgentActivity", kind: "started", agentThreadId: "child-v2-thread", agentPath: "/root/readme_location"
		} });
		send("turn/started", { threadId: "child-v2-thread", turn: { id: "child-v2-turn-1" } });
	});

	const visual = page.getByTestId("visual-subagents");
	await expect(visual).toBeVisible();
	await expect(visual).toContainText("Working · 1");
	await expect(visual).toContainText("Readme Location");
	await expect(visual.getByText("Working", { exact: true })).toBeVisible();
	await expect(page.getByText("Readme Location started")).toBeVisible();

	await page.evaluate(() => {
		const emit = (window as typeof window & { __emitCodexV2Event: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitCodexV2Event;
		emit({ sessionId: "subagent-v2-session", method: "agentMessage/completed", params: { threadId: "child-v2-thread", messageId: "child-v2-message", content: "README location confirmed." } });
		emit({ sessionId: "subagent-v2-session", method: "thread/status/changed", params: { threadId: "child-v2-thread", status: { type: "idle" } } });
	});
	await expect(visual).toContainText("Working · 1");
	await expect(visual).toContainText("Completed · 0");
	await expect(page.getByTestId("chat-transcript")).not.toContainText("README location confirmed.");

	await page.evaluate(() => {
		const emit = (window as typeof window & { __emitCodexV2Event: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitCodexV2Event;
		emit({ sessionId: "subagent-v2-session", method: "turn/completed", params: { threadId: "child-v2-thread", turn: { status: "completed", lastAgentMessage: "README location confirmed." } } });
	});
	await expect(visual).toContainText("Working · 0");
	await expect(visual).toContainText("Completed · 1");
	await expect(visual).toContainText("README location confirmed.");
	await expect(page.getByTestId("chat-transcript")).not.toContainText("The provider ended the turn without a response");

	await page.evaluate(() => {
		const emit = (window as typeof window & { __emitCodexV2Event: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitCodexV2Event;
		emit({ sessionId: "subagent-v2-session", method: "item/started", params: { item: {
			id: "spawn-v2-wait", type: "subAgentActivity", kind: "started", agentThreadId: "child-v2-wait-thread", agentPath: "/root/runtime_audit"
		} } });
		emit({ sessionId: "subagent-v2-session", method: "agentMessage/completed", params: {
			threadId: "child-v2-wait-thread", messageId: "child-v2-wait-message", content: "Runtime audit complete."
		} });
		// Current Codex app-server V2 output can omit child ids/states here.
		emit({ sessionId: "subagent-v2-session", method: "item/completed", params: { item: {
			id: "wait-v2", type: "collabAgentToolCall", tool: "wait", status: "completed", receiverThreadIds: [], agentsStates: {}
		} } });
	});
	await expect(visual).toContainText("Completed · 2");
	await expect(visual).toContainText("Runtime audit complete.");

	await page.evaluate(() => {
		const emit = (window as typeof window & { __emitCodexV2Event: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitCodexV2Event;
		emit({ sessionId: "subagent-v2-session", method: "turn/started", params: { threadId: "child-v2-thread", turn: { id: "child-v2-turn-2" } } });
		emit({ sessionId: "subagent-v2-session", method: "turn/failed", params: { threadId: "child-v2-thread", error: { message: "Agent exceeded its task budget." } } });
	});
	await expect(visual).toContainText("Needs attention · 1");
	await expect(visual).toContainText("Agent exceeded its task budget.");
	await expect(page.getByTestId("chat-transcript")).not.toContainText("The provider could not produce a response");
});

test("Codex thread name updates rename the durable sidebar session", async ({ page }) => {
	await page.addInitScript(() => {
		type Event = { sessionId: string; method: string; params: Record<string, unknown> };
		let listener: ((event: Event) => void) | undefined;
		(window as typeof window & { __emitCodexTitle?: (event: Event) => void }).__emitCodexTitle = (event) => listener?.(event);
		(window as typeof window & { synthCodex?: unknown }).synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId: "titled-session", threadId: "titled-thread", workspace: "/workspaces/default",
				model: "openai/gpt-5.6-sol", providerName: "openrouter", providerTitle: "OpenRouter",
				baseUrl: "https://openrouter.ai/api/v1", status: "ready", title: "GPT 5.6 Sol"
			}],
			start: async () => ({ sessionId: "titled-session", threadId: "titled-thread" }),
			startTurn: async () => ({ sessionId: "titled-session", threadId: "titled-thread", turnId: "turn-1" }),
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: (next: (event: Event) => void) => { listener = next; return () => { listener = undefined; }; }
		};
	});
	await page.reload();
	const row = page.getByTestId("local-chat-titled-session");
	await expect(row).toContainText("GPT 5.6 Sol");
	await page.evaluate(() => {
		const emit = (window as typeof window & {
			__emitCodexTitle: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void;
		}).__emitCodexTitle;
		emit({
			sessionId: "titled-session",
			method: "thread/name/updated",
			params: { threadId: "titled-thread", threadName: "Run two Craftax rollouts" }
		});
	});
	await expect(row).toContainText("Run two Craftax rollouts");
});

test("projectless native Codex uses and retains the default workspace", async ({ page }) => {
	await page.addInitScript(() => {
		const calls: { defaults: number; starts: Array<Record<string, unknown>>; turns: Array<{ sessionId: string; prompt: string }> } = {
			defaults: 0,
			starts: [],
			turns: []
		};
		(window as typeof window & { __nativeCodexCalls?: typeof calls }).__nativeCodexCalls = calls;
		(window as typeof window & { synthCodex?: unknown }).synthCodex = {
			defaultWorkspace: async () => {
				calls.defaults += 1;
				return "/workspaces/default";
			},
			list: async () => [],
			start: async (request: Record<string, unknown>) => {
				calls.starts.push(request);
				return { sessionId: request.sessionId, threadId: "thread-projectless", turnId: null };
			},
			startTurn: async (sessionId: string, prompt: string) => {
				calls.turns.push({ sessionId, prompt });
				return { sessionId, threadId: "thread-projectless", turnId: "turn-1" };
			},
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: () => () => undefined
		};
	});
	await installLagunaFixture(page, "ready");
	await page.waitForFunction(() => Boolean(window.__synthEval));

	const session = await page.evaluate(async () => {
		if (!window.__synthEval) throw new Error("semantic API unavailable");
		return window.__synthEval.invoke("create_session", { targetId: "local-laguna" }) as Promise<{
			id: string;
			projectId: string | null;
			metadata: Record<string, unknown>;
		}>;
	});
	await expect(page.getByTestId("chat-transcript")).toBeVisible();
	await page.evaluate(async ({ sessionId }) => {
		if (!window.__synthEval) throw new Error("semantic API unavailable");
		await window.__synthEval.invoke("send_message", { sessionId, body: "projectless hello" });
	}, { sessionId: session.id });

	expect(session.projectId).toBeNull();
	expect(session.metadata).toMatchObject({
		runtime: "codex-app-server",
		workspace: "/workspaces/default"
	});
	const calls = await page.evaluate(() => (window as typeof window & {
		__nativeCodexCalls: { defaults: number; starts: Array<Record<string, unknown>>; turns: Array<{ sessionId: string; prompt: string }> };
	}).__nativeCodexCalls);
	expect(calls.defaults).toBeGreaterThan(0);
	// Sending defensively resumes the durable thread, so start may be called
	// again; every create/resume must retain the same projectless workspace.
	expect(calls.starts.length).toBeGreaterThanOrEqual(1);
	for (const start of calls.starts) {
		expect(start).toMatchObject({
			sessionId: session.id,
			workspace: "/workspaces/default",
			providerName: "local-laguna"
		});
	}
	expect(calls.turns).toEqual([{ sessionId: session.id, prompt: "projectless hello" }]);
});

test("Rust Inventory navigation never replaces native Codex sessions with legacy runtime sessions", async ({ page }) => {
	await page.addInitScript(() => {
		(window as typeof window & { synthCodex?: unknown }).synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId: "native-session", threadId: "native-thread", workspace: "/workspaces/default",
				model: "poolside/Laguna-XS-2.1-NVFP4-mlx", providerName: "local-laguna",
				providerTitle: "Laguna XS", baseUrl: "http://127.0.0.1:7333", status: "ready"
			}],
			start: async () => ({ sessionId: "native-session", threadId: "native-thread" }),
			startTurn: async () => ({ sessionId: "native-session", threadId: "native-thread", turnId: "turn" }),
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: () => () => undefined
		};
	});
	await installLagunaFixture(page, "ready");
	await expect(page.getByTestId("local-chat-native-session")).toBeVisible();
	await page.getByRole("button", { name: "Containers · Traces · Usage" }).click();
	await page.waitForTimeout(3_000);
	await expect(page.getByTestId("local-chat-native-session")).toBeVisible();
});

test("changing providers mid-chat stays in the thread and switches on send", async ({ page }) => {
	await installConfiguredOpenRouter(page);
	await page.addInitScript(() => {
		const starts: Array<Record<string, unknown>> = [];
		const turns: Array<{ sessionId: string; prompt: string; effort?: string }> = [];
		const listeners = new Set<(event: { sessionId: string; method: string; params: Record<string, unknown> }) => void>();
		(window as typeof window & { __providerStarts?: typeof starts }).__providerStarts = starts;
		(window as typeof window & { __providerTurns?: typeof turns }).__providerTurns = turns;
		(window as typeof window & { synthCodex?: unknown }).synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId: "bound-local", threadId: "local-thread", workspace: "/workspaces/default",
				model: "poolside/Laguna-XS-2.1-NVFP4-mlx", providerName: "local-laguna",
				providerTitle: "Laguna XS", baseUrl: "http://127.0.0.1:7333", status: "ready"
			}],
			start: async (request: Record<string, unknown>) => {
				starts.push(request);
				return { sessionId: request.sessionId, threadId: "local-thread" };
			},
			startTurn: async (sessionId: string, prompt: string, effort?: string) => {
				turns.push({ sessionId, prompt, effort });
				setTimeout(() => {
					for (const listener of listeners) {
						listener({ sessionId, method: "turn/completed", params: { turn: { status: "completed" } } });
					}
				}, 0);
				return { sessionId, threadId: "local-thread", turnId: `turn-${turns.length}` };
			},
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: (listener: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void) => {
				listeners.add(listener);
				return () => { listeners.delete(listener); };
			}
		};
	});
	await installLagunaFixture(page, "ready");
	await page.getByTestId("local-chat-bound-local").click();
	await expect(page.getByTestId("composer-model")).toHaveAccessibleName("Model: Laguna XS 2.1");
	await expect(page.getByTestId("reasoning-effort-select")).toHaveAccessibleName("Thinking: Max");
	await page.getByTestId("reasoning-effort-select").click();
	await expect(page.getByTestId("reasoning-effort-menu").getByRole("option")).toHaveCount(2);
	await page.getByTestId("reasoning-effort-menu").getByRole("option", { name: "Minimal", exact: true }).click();
	await page.getByTestId("composer-input").fill("hello Laguna");
	await page.getByTestId("composer-send").click();
	await expect.poll(() => page.evaluate(() =>
		(window as typeof window & { __providerTurns: Array<{ effort?: string }> }).__providerTurns.at(-1)?.effort
	)).toBe("none");
	await expect(page.getByRole("button", { name: "Stop generating" })).toHaveCount(0);
	await expect.poll(() => page.evaluate(() => localStorage.getItem("synth.models.local-laguna.reasoning"))).toBe("none");
	await page.getByTestId("composer-model").click();
	await page.getByTestId("composer-model-option-openrouter-luna").click();
	// Chip fiddle stays in the same chat; compact/rebind wait for send.
	await expect(page.getByTestId("chat-transcript")).toBeVisible();
	await expect(page.getByText("Start a new conversation using")).toHaveCount(0);
	await expect(page.getByTestId("composer-model")).toHaveAccessibleName("Model: GPT 5.6 Luna");
	await expect(page.getByTestId("reasoning-effort-select")).toHaveAccessibleName("Reasoning effort: Medium");
	await page.getByTestId("reasoning-effort-select").click();
	const effortMenu = page.getByTestId("reasoning-effort-menu");
	await expect(effortMenu.getByRole("option")).toHaveCount(5);
	await effortMenu.getByRole("option", { name: "High", exact: true }).click();
	await expect(page.getByTestId("reasoning-effort-select")).toHaveAccessibleName("Reasoning effort: High");
	await page.getByTestId("composer-input").fill("hello Luna");
	await page.getByTestId("composer-send").click();
	await expect.poll(() => page.evaluate(() => (window as typeof window & { __providerStarts: Array<Record<string, unknown>> }).__providerStarts.at(-1))).toMatchObject({
		sessionId: "bound-local",
		providerName: "openrouter",
		model: "openai/gpt-5.6-luna",
		threadId: "local-thread"
	});
	await expect.poll(() => page.evaluate(() => (window as typeof window & { __providerTurns: Array<{ sessionId: string; prompt: string; effort?: string }> }).__providerTurns.at(-1))).toMatchObject({ sessionId: "bound-local", prompt: "hello Luna", effort: "high" });
	await expect.poll(() => page.evaluate(() => localStorage.getItem("synth.reasoningEffort"))).toBe("high");

	await page.getByTestId("composer-model").click();
	await page.getByTestId("composer-model-option-openrouter-laguna-s").click();
	await expect(page.getByTestId("chat-transcript")).toBeVisible();
	await expect(page.getByTestId("composer-model")).toHaveAccessibleName("Model: Laguna S 2.1");
	await expect(page.getByTestId("reasoning-effort-select")).toHaveAccessibleName("Thinking: Max");
	await page.getByTestId("reasoning-effort-select").click();
	await page.getByTestId("reasoning-effort-menu").getByRole("option", { name: "None", exact: true }).click();
	await page.getByTestId("composer-input").fill("hello Laguna S");
	await page.getByTestId("composer-send").click();
	await expect.poll(() => page.evaluate(() => (window as typeof window & { __providerStarts: Array<Record<string, unknown>> }).__providerStarts.at(-1))).toMatchObject({
		sessionId: "bound-local",
		providerName: "openrouter",
		model: "poolside/laguna-s-2.1"
	});
	await expect.poll(() => page.evaluate(() => (window as typeof window & { __providerTurns: Array<{ prompt: string; effort?: string }> }).__providerTurns.at(-1))).toMatchObject({ prompt: "hello Laguna S", effort: "none" });
	await expect.poll(() => page.evaluate(() => localStorage.getItem("synth.models.openrouter-laguna-s.reasoning"))).toBe("none");
});

test("sidebar exposes the next local-model free time and countdown", async ({ page }) => {
	await page.addInitScript(() => {
		const freeAt = Date.now() + 235_000;
		(window as typeof window & { synthLaguna?: unknown }).synthLaguna = {
			getStatus: async () => ({
				phase: "ready",
				baseUrl: "http://127.0.0.1:7333",
				backend: "mlx_lm",
				loadedModel: "poolside/Laguna-XS-2.1-NVFP4-mlx",
				detail: "Laguna XS ready",
				memoryBytes: 16 * 1024 ** 3,
				idleSeconds: 65,
				idleUnloadAfterSeconds: 300,
				freeAt,
				updatedAt: Date.now()
			}),
			onStatus: () => () => undefined,
			listModels: async () => [],
			chooseModelDirectory: async () => null,
			setModelDirectory: async () => { throw new Error("not used"); },
			clearModelDirectory: async () => undefined
		};
	});
	await page.reload();

	const residency = page.getByTestId("model-residency");
	const summary = residency.getByRole("button");
	await expect(residency).toBeVisible();
	await expect(summary).toContainText("Laguna-XS-2.1-NVFP4");
	await expect(summary).toContainText("16.0 GB resident");
	await expect(summary).toHaveAccessibleName(/last prompt 1m ago/i);
	await expect(summary).toHaveAccessibleName(/Frees at .+ · in 3m 5[3-5]s/i);
	await summary.click();

	const details = page.getByTestId("model-residency-details");
	await expect(details).toBeVisible();
	await expect(details).toContainText("Last prompt");
	await expect(details).toContainText("1m ago");
	await expect(details).toContainText("Next free");
	await expect(details).toContainText(/Frees at .+ · in 3m 5[3-5]s/);
});

test("expired local model free schedule reports its time and waits for Laguna unload", async ({ page }) => {
	await page.addInitScript(() => {
		const freeAt = Date.now() - 1_000;
		(window as typeof window & { synthLaguna?: unknown }).synthLaguna = {
			getStatus: async () => ({
				phase: "ready",
				baseUrl: "http://127.0.0.1:7333",
				backend: "mlx_lm",
				loadedModel: "poolside/Laguna-XS-2.1-NVFP4-mlx",
				detail: "Laguna XS ready",
				memoryBytes: 16 * 1024 ** 3,
				idleSeconds: 900,
				idleUnloadAfterSeconds: 900,
				freeAt,
				updatedAt: Date.now()
			}),
			onStatus: () => () => undefined,
			listModels: async () => [],
			chooseModelDirectory: async () => null,
			setModelDirectory: async () => { throw new Error("not used"); },
			clearModelDirectory: async () => undefined
		};
	});
	await page.reload();

	const summary = page.getByTestId("model-residency").getByRole("button");
	await expect(summary).toHaveAccessibleName(/Free scheduled for .+ · awaiting unload/i);
	await expect(summary).not.toHaveAccessibleName(/Freeing memory/i);
});

test("resident model disappears immediately when Laguna reports automatic unload", async ({ page }) => {
	await page.addInitScript(() => {
		let listener: ((status: Record<string, unknown>) => void) | undefined;
		const ready = {
			phase: "ready", baseUrl: "http://127.0.0.1:7333", backend: "mlx_lm",
			loadedModel: "poolside/Laguna-XS-2.1-NVFP4-mlx", detail: "Laguna XS ready",
			memoryBytes: 16 * 1024 ** 3, idleSeconds: 1, idleUnloadAfterSeconds: 2,
			freeAt: Date.now() + 1_000, updatedAt: Date.now()
		};
		const testWindow = window as typeof window & {
			synthLaguna?: unknown;
			__emitLagunaUnloaded?: () => void;
		};
		testWindow.__emitLagunaUnloaded = () => listener?.({
			...ready, phase: "unloaded", loadedModel: null, memoryBytes: 0,
			idleSeconds: 2, freeAt: null, updatedAt: Date.now()
		});
		testWindow.synthLaguna = {
			getStatus: async () => ready,
			onStatus: (next: typeof listener) => { listener = next; return () => { listener = undefined; }; },
			listModels: async () => [], chooseModelDirectory: async () => null,
			setModelDirectory: async () => { throw new Error("not used"); },
			clearModelDirectory: async () => undefined
		};
	});
	await page.reload();
	await expect(page.getByTestId("model-residency")).toBeVisible();
	await page.evaluate(() => (window as typeof window & { __emitLagunaUnloaded: () => void }).__emitLagunaUnloaded());
	await expect(page.getByTestId("model-residency")).toBeHidden();
	await expect(page.getByText(/Frees automatically in now/i)).toHaveCount(0);
});

test("a cold local turn says Warming up until model residency is reported", async ({ page }) => {
	await page.addInitScript(() => {
		const testWindow = window as typeof window & { synthLaguna?: unknown; synthCodex?: unknown };
		testWindow.synthLaguna = {
			getStatus: async () => ({
				phase: "loading", baseUrl: "http://127.0.0.1:7333", backend: "mlx_lm",
				loadedModel: null, detail: "Loading Laguna XS…", memoryBytes: 0, updatedAt: Date.now()
			}),
			onStatus: () => () => undefined,
			listModels: async () => [], downloadModel: async () => { throw new Error("unused"); },
			chooseModelDirectory: async () => null,
			setModelDirectory: async () => { throw new Error("unused"); },
			clearModelDirectory: async () => undefined
		};
		testWindow.synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId: "cold-session", threadId: "thread-cold", workspace: "/workspaces/default",
				model: "poolside/Laguna-XS-2.1-NVFP4-mlx", providerName: "local-laguna",
				providerTitle: "Laguna XS", baseUrl: "http://127.0.0.1:7333", status: "running"
			}],
			start: async () => { throw new Error("unused"); },
			startTurn: async () => { throw new Error("unused"); },
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: () => () => undefined
		};
	});
	await page.reload();
	await page.getByTestId("local-chat-cold-session").click();
	await expect(page.getByTestId("model-working")).toContainText("Warming up…");
	await expect(page.getByTestId("model-working")).not.toContainText("Working…");
	await expect(page.getByRole("button", { name: "Stop generating" })).toBeVisible();
});

test("native Codex deltas form one readable message with working and stop state", async ({ page }) => {
	await page.addInitScript(() => {
		let listener: ((event: { sessionId: string; method: string; params: Record<string, unknown> }) => void) | undefined;
		let interrupts = 0;
		const testWindow = window as typeof window & {
			__emitConversationCodex?: typeof listener;
			__conversationInterrupts?: () => number;
			synthCodex?: unknown;
		};
		testWindow.__conversationInterrupts = () => interrupts;
		testWindow.synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId: "stream-session", threadId: "thread-stream", workspace: "/workspaces/default",
				model: "poolside/Laguna-XS-2.1-NVFP4-mlx", providerName: "local-laguna",
				providerTitle: "Laguna XS", baseUrl: "http://127.0.0.1:7333", status: "ready"
			}],
			start: async () => ({ sessionId: "stream-session", threadId: "thread-stream" }),
			startTurn: async () => ({ sessionId: "stream-session", threadId: "thread-stream", turnId: "turn-stream" }),
			interrupt: async () => { interrupts += 1; },
			close: async () => undefined,
			onEvent: (next: typeof listener) => {
				listener = next;
				testWindow.__emitConversationCodex = next;
				return () => { listener = undefined; };
			}
		};
	});
	await installLagunaFixture(page, "ready");
	await page.getByTestId("local-chat-stream-session").click();

	await page.evaluate(() => {
		const emit = (window as typeof window & { __emitConversationCodex: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitConversationCodex;
		const send = (method: string, params: Record<string, unknown>) => emit({ sessionId: "stream-session", method, params });
		send("turn/started", { turn: { id: "turn-stream" } });
		// Real app-server deltas can arrive without any stable message/item id.
		send("item/agentMessage/delta", { delta: "Fragmented draft\n" });
		send("item/agentMessage/delta", { delta: "still streaming." });
		// The local Responses bridge may assign an envelope/item id per token.
		// Those ids must not turn one assistant response into many block rows.
		send("item/agentMessage/delta", { itemId: "token-envelope-1", delta: " One" });
		send("item/agentMessage/delta", { itemId: "token-envelope-2", delta: " response." });
		// Commentary is a preamble: a tool that follows it must render below it,
		// never be hoisted above the text when activity is grouped.
		send("item/started", { item: { id: "post-preamble-command", type: "commandExecution", command: "pwd" } });
		send("item/reasoning/delta", { delta: "Checking the relevant " });
		send("item/reasoning/delta", { delta: "renderer state." });
		send("remoteControl/status/changed", { status: "connected" });
		send("app-server/stderr", { line: "model-metadata: raw transport noise" });
		send("account/rateLimits/updated", { limit: 100 });
	});

	const transcript = page.getByTestId("chat-transcript");
	await expect(transcript.locator(".local-assistant")).toHaveCount(1);
	const assistantText = transcript.locator(".local-assistant p");
	await expect(assistantText).toHaveText("Fragmented draft\nstill streaming. One response.");
	await expect(assistantText).toHaveCSS("white-space", "pre-wrap");
	await expect(page.getByTestId("model-working")).toContainText("Working…");
	await expect(page.getByRole("button", { name: "Stop generating" })).toBeVisible();
	const postPreambleCommand = transcript.locator(".command-activity").filter({ hasText: "pwd" });
	await expect(postPreambleCommand).toBeVisible();
	expect((await postPreambleCommand.boundingBox())!.y).toBeGreaterThan((await assistantText.boundingBox())!.y);
	const thought = transcript.getByRole("button", { name: /Thought/ });
	await expect(thought).toBeVisible();
	await expect(thought).toHaveAttribute("aria-expanded", "false");
	const thoughtDisclosure = thought.locator("..");
	await expect(thoughtDisclosure).toHaveCSS("border-top-width", "0px");
	await expect(thoughtDisclosure).toHaveCSS("padding-top", "0px");
	await expect(thoughtDisclosure.locator(".local-activity-wave")).toHaveCount(0);
	expect((await thoughtDisclosure.boundingBox())?.height ?? Number.POSITIVE_INFINITY).toBeLessThanOrEqual(48);
	await expect(transcript.getByTestId(/activity-detail-/)).toHaveCount(0);
	await thought.click();
	await expect(thought).toHaveAttribute("aria-expanded", "true");
	await expect(transcript).toContainText("Checking the relevant renderer state.");
	await expect(thoughtDisclosure.locator(".local-activity-detail")).toHaveCSS("font-family", /-apple-system|BlinkMacSystemFont|system-ui/);

	await page.evaluate(() => {
		const emit = (window as typeof window & { __emitConversationCodex: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitConversationCodex;
		emit({
			sessionId: "stream-session",
			method: "item/completed",
			params: { item: { id: "stable-final-id", type: "agentMessage", text: "One correct final answer.\nWith preserved spacing." } }
		});
		emit({
			sessionId: "stream-session",
			method: "agentMessage/completed",
			params: {
				messageId: "alternate-final-envelope-id",
				content: "One correct final answer.\nWith preserved spacing."
			}
		});
	});
	await expect(transcript.locator(".local-assistant")).toHaveCount(2);
	await expect(transcript.locator(".local-assistant p").last()).toHaveText("One correct final answer.\nWith preserved spacing.");
	await expect(transcript.locator(".local-assistant p").first()).toContainText("Fragmented draft");
	await expect(transcript).not.toContainText("remoteControl/status/changed");
	await expect(transcript).not.toContainText("model-metadata");
	await expect(transcript).not.toContainText("account/rateLimits/updated");
	await expect(thought).toBeVisible();
	await expect(transcript).toContainText("Checking the relevant renderer state.");

	await page.evaluate(() => {
		(window as typeof window & { __emitConversationCodex: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitConversationCodex({
			sessionId: "stream-session", method: "turn/completed", params: { turn: { id: "turn-stream" } }
		});
	});
	await expect(page.getByTestId("model-working")).toBeHidden();
	await transcript.locator(".chat-transcript-scroll").evaluate((element) => {
		element.scrollTop = element.scrollHeight;
	});
	const composerBoundary = await page.evaluate(() => {
		const lastTurn = document.querySelector<HTMLElement>(".chat-transcript-inner > .local-turn:last-of-type");
		const composer = document.querySelector<HTMLElement>('[data-testid="composer"]');
		if (!lastTurn || !composer) throw new Error("Transcript boundary targets are absent");
		return {
			lastTurnBottom: lastTurn.getBoundingClientRect().bottom,
			composerTop: composer.getBoundingClientRect().top
		};
	});
	expect(composerBoundary.lastTurnBottom).toBeLessThanOrEqual(composerBoundary.composerTop - 12);

	await page.getByTestId("composer-input").fill("Second request must stay above its response");
	await page.getByTestId("composer-send").click();
	await expect(transcript.locator(".local-turn-user").last()).toContainText("Second request must stay above its response");

	await page.evaluate(() => {
		const emit = (window as typeof window & { __emitConversationCodex: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitConversationCodex;
		// Providers can emit activity and assistant deltas before (or without) a
		// turn/started notification. The local user event must still split turns.
		emit({ sessionId: "stream-session", method: "item/started", params: {
			item: { id: "second-command", type: "commandExecution", command: "pwd" }
		} });
		const send = (delta: string) => emit({
			sessionId: "stream-session",
			method: "item/agentMessage/delta",
			params: { itemId: "stable-stream-id", delta }
		});
		// Some transports replay cumulative envelopes for the same stable item.
		send("Replacement stream");
		send("Replacement stream with one paragraph.");
		send("Replacement stream with one paragraph.");
	});
	await expect(transcript.locator(".local-assistant")).toHaveCount(3);
	await expect(transcript.locator(".local-assistant p").last()).toHaveText("Replacement stream with one paragraph.");
	const finalTurnOrder = await transcript.locator(".local-turn").evaluateAll((turns) => turns.slice(-2).map((turn) => ({
		role: turn.classList.contains("local-turn-user") ? "user" : turn.classList.contains("local-turn-assistant") ? "assistant" : "system",
		text: turn.textContent ?? ""
	})));
	expect(finalTurnOrder.map((turn) => turn.role)).toEqual(["user", "assistant"]);
	expect(finalTurnOrder[0].text).toContain("Second request must stay above its response");
	expect(finalTurnOrder[1].text).toContain("Replacement stream with one paragraph.");
	const secondUser = transcript.locator(".local-turn-user").last();
	const secondCommand = transcript.locator(".command-activity").last();
	await expect(secondCommand).toContainText("pwd");
	expect((await secondCommand.boundingBox())!.y).toBeGreaterThan((await secondUser.boundingBox())!.y);
	await page.getByRole("button", { name: "Stop generating" }).click();
	expect(await page.evaluate(() => (window as typeof window & { __conversationInterrupts: () => number }).__conversationInterrupts())).toBe(1);
	await expect(page.getByTestId("workbench-side-panel")).toBeVisible();
	await expect(page.getByTestId("inference-panel")).toBeVisible();
	await expect(page.getByTestId("workbench-side-tab-inference")).toHaveAttribute("aria-selected", "true");
	await page.getByTestId("toggle-inference-rail").click();
	await expect(page.getByTestId("workbench-side-panel")).toBeHidden();
	await page.getByTestId("toggle-inference-rail").click();
	await expect(page.getByTestId("workbench-side-panel")).toBeVisible();
	const inferenceGeometry = await page.getByTestId("inference-panel").evaluate((panel) => {
		const rail = panel.parentElement!.getBoundingClientRect();
		const panelRect = panel.getBoundingClientRect();
		const composer = document.querySelector<HTMLElement>("[data-testid=composer]")!.getBoundingClientRect();
		return {
			contained: panelRect.left >= rail.left && panelRect.right <= rail.right + 1,
			hasInset: panelRect.left - rail.left >= 8 && rail.right - panelRect.right >= 8,
			composerClearsRail: composer.right <= rail.left + 1,
			overflow: document.documentElement.scrollWidth > window.innerWidth + 1
		};
	});
	expect(inferenceGeometry).toEqual({ contained: true, hasInset: true, composerClearsRail: true, overflow: false });
});

test("closed-model reasoning renders only a provider summary disclosure", async ({ page }) => {
	await page.addInitScript(() => {
		let listener: ((event: { sessionId: string; method: string; params: Record<string, unknown> }) => void) | undefined;
		const testWindow = window as typeof window & {
			__emitSummaryCodex?: typeof listener;
			synthCodex?: unknown;
		};
		testWindow.synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId: "summary-session", threadId: "thread-summary", workspace: "/workspaces/default",
				model: "openai/gpt-5.6-luna", providerName: "openrouter",
				providerTitle: "OpenRouter Responses", baseUrl: "https://openrouter.ai/api/v1", status: "ready"
			}],
			start: async () => ({ sessionId: "summary-session", threadId: "thread-summary" }),
			startTurn: async () => ({ sessionId: "summary-session", threadId: "thread-summary", turnId: "turn-summary" }),
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: (next: typeof listener) => {
				listener = next;
				testWindow.__emitSummaryCodex = next;
				return () => { listener = undefined; };
			}
		};
	});
	await installLagunaFixture(page, "ready");
	await page.getByTestId("local-chat-summary-session").click();
	await page.evaluate(() => {
		const emit = (window as typeof window & { __emitSummaryCodex: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitSummaryCodex;
		emit({ sessionId: "summary-session", method: "item/reasoning/delta", params: { delta: "Checked the workspace boundary." } });
	});
	const transcript = page.getByTestId("chat-transcript");
	const summary = transcript.getByRole("button", { name: /Reasoning summary/ });
	await expect(summary).toBeVisible();
	await expect(summary).toContainText("Reasoning summary");
	await expect(transcript.getByRole("button", { name: /Thought/ })).toHaveCount(0);
	await expect(transcript.getByTestId(/activity-detail-/)).toHaveCount(0);
	await summary.click();
	await expect(transcript).toContainText("Checked the workspace boundary.");
});

test("manual XS compaction resumes the thread and renders success without an empty-turn error", async ({ page }) => {
	await page.addInitScript(() => {
		let listener: ((event: { sessionId: string; method: string; params: Record<string, unknown> }) => void) | undefined;
		const testWindow = window as typeof window & {
			__emitCompactCodex?: typeof listener;
			__compactRequest?: Record<string, unknown>;
			synthCodex?: unknown;
		};
		testWindow.synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId: "compact-session", threadId: "thread-compact", workspace: "/workspaces/default",
				model: "poolside/Laguna-XS-2.1-NVFP4-mlx", providerName: "local-laguna",
				providerTitle: "Laguna XS", baseUrl: "http://127.0.0.1:7333", status: "ready"
			}],
			start: async () => ({ sessionId: "compact-session", threadId: "thread-compact" }),
			startTurn: async () => ({ sessionId: "compact-session", threadId: "thread-compact", turnId: "turn-compact" }),
			compact: async (request: Record<string, unknown>) => { testWindow.__compactRequest = request; },
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: (next: typeof listener) => {
				listener = next;
				testWindow.__emitCompactCodex = next;
				return () => { listener = undefined; };
			}
		};
	});
	await installLagunaFixture(page, "ready");
	await page.getByTestId("local-chat-compact-session").click();
	await page.evaluate(() => {
		const emit = (window as typeof window & { __emitCompactCodex: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitCompactCodex;
		emit({ sessionId: "compact-session", method: "item/completed", params: { item: { id: "answer-before-compact", type: "agentMessage", text: "BEFORE_COMPACT_OK" } } });
	});
	await page.getByTestId("composer-input").fill("/compact");
	await page.getByRole("option", { name: /Compact context/ }).click();
	await expect.poll(() => page.evaluate(() => Boolean((window as typeof window & { __compactRequest?: unknown }).__compactRequest))).toBe(true);
	expect(await page.evaluate(() => (window as typeof window & { __compactRequest: { threadId?: string } }).__compactRequest.threadId)).toBe("thread-compact");

	await page.evaluate(() => {
		const emit = (window as typeof window & { __emitCompactCodex: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitCompactCodex;
		emit({ sessionId: "compact-session", method: "turn/started", params: { turn: { id: "turn-compact" } } });
		emit({ sessionId: "compact-session", method: "item/completed", params: { item: { id: "compact-item", type: "contextCompaction" } } });
		emit({ sessionId: "compact-session", method: "thread/compacted", params: { threadId: "thread-compact" } });
		emit({ sessionId: "compact-session", method: "turn/completed", params: { turn: { id: "turn-compact", status: "completed" } } });
	});
	const transcript = page.getByTestId("chat-transcript");
	await expect(transcript).toContainText("Context compacted");
	await expect(transcript.locator(".context-compaction-divider")).toHaveCount(1);
	await expect(transcript).not.toContainText("The provider ended the turn without a response");
	const responseBox = await transcript.locator(".local-assistant", { hasText: "BEFORE_COMPACT_OK" }).boundingBox();
	const markerBox = await transcript.locator(".context-compaction-divider").boundingBox();
	expect(responseBox).not.toBeNull();
	expect(markerBox).not.toBeNull();
	expect(markerBox!.y).toBeGreaterThan(responseBox!.y + responseBox!.height - 1);
});

test("model-switch compaction renders above the continued turn's tool calls", async ({ page }) => {
	await page.addInitScript(() => {
		let listener: ((event: { sessionId: string; method: string; params: Record<string, unknown> }) => void) | undefined;
		const testWindow = window as typeof window & {
			__emitSwitchCompactCodex?: typeof listener;
			synthCodex?: unknown;
		};
		testWindow.synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId: "switch-compact-session", threadId: "thread-switch-compact", workspace: "/workspaces/default",
				model: "poolside/laguna-s-2.1", providerName: "openrouter",
				providerTitle: "Laguna S", baseUrl: "https://openrouter.ai/api/v1", status: "ready"
			}],
			start: async () => ({ sessionId: "switch-compact-session", threadId: "thread-switch-compact" }),
			startTurn: async () => ({ sessionId: "switch-compact-session", threadId: "thread-switch-compact", turnId: "turn-after-switch" }),
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: (next: typeof listener) => {
				listener = next;
				testWindow.__emitSwitchCompactCodex = next;
				return () => { listener = undefined; };
			}
		};
	});
	await installLagunaFixture(page, "ready");
	await page.getByTestId("local-chat-switch-compact-session").click();
	await page.getByTestId("activity-mode-menu-trigger").click();
	await page.getByTestId("activity-mode-option-detailed").click();
	await page.evaluate(() => {
		const emit = (window as typeof window & { __emitSwitchCompactCodex: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitSwitchCompactCodex;
		const send = (method: string, params: Record<string, unknown>) => emit({ sessionId: "switch-compact-session", method, params });
		send("turn/started", { turn: { id: "turn-after-switch" } });
		send("thread/tokenUsage/updated", {
			threadId: "thread-switch-compact",
			tokenUsage: { last: { totalTokens: 271_840 }, total: { totalTokens: 271_840 }, modelContextWindow: 258_400 }
		});
		send("thread/compacted", { threadId: "thread-switch-compact", source: "model_switch" });
		send("thread/tokenUsage/updated", {
			threadId: "thread-switch-compact",
			tokenUsage: { last: { totalTokens: 37_492 }, total: { totalTokens: 37_492 }, modelContextWindow: 258_400 }
		});
		send("thread/tokenUsage/updated", {
			threadId: "thread-switch-compact",
			tokenUsage: { last: { totalTokens: 7_385 }, total: { totalTokens: 7_385 }, modelContextWindow: 258_400 }
		});
		send("item/started", {
			item: {
				id: "probe-1",
				type: "mcpToolCall",
				server: "synth_containers",
				tool: "container_probe",
				status: "inProgress",
				arguments: { container_id: "craftax-local" }
			}
		});
		send("item/completed", {
			item: { id: "answer-after-switch", type: "agentMessage", text: "Picking up after the model switch." }
		});
	});
	const transcript = page.getByTestId("chat-transcript");
	await expect(transcript).toContainText("Model switch - context compacted");
	const compactToggle = transcript.getByTestId(/activity-toggle-context-compaction-/);
	await expect(compactToggle).toBeVisible();
	await compactToggle.click();
	await expect(transcript.getByTestId(/activity-detail-context-compaction-/)).toHaveText("0.27M → 0.01M");
	await expect(transcript.locator("code.mcp-activity-name").getByText("synth_containers.container_probe")).toBeVisible();
	await expect(transcript).toContainText("Picking up after the model switch.");
	const markerBox = await transcript.locator(".context-compaction-divider").boundingBox();
	const toolBox = await transcript.locator("code.mcp-activity-name").getByText("synth_containers.container_probe").boundingBox();
	const responseBox = await transcript.locator(".local-assistant", { hasText: "Picking up after the model switch." }).boundingBox();
	expect(markerBox).not.toBeNull();
	expect(toolBox).not.toBeNull();
	expect(responseBox).not.toBeNull();
	expect(toolBox!.y).toBeGreaterThan(markerBox!.y + markerBox!.height - 1);
	expect(responseBox!.y).toBeGreaterThan(toolBox!.y);
});

test("native Codex tool use renders safe Poolside-style rows and a compact run summary", async ({ page }) => {
	await page.addInitScript(() => {
		let listener: ((event: { sessionId: string; method: string; params: Record<string, unknown> }) => void) | undefined;
		const testWindow = window as typeof window & {
			__emitToolCodex?: typeof listener;
			synthCodex?: unknown;
			synthInventory?: unknown;
		};
		const craftax = {
			id: "craftax-local", name: "Craftax Rust", location: "local", status: "ready",
			baseUrl: "http://127.0.0.1:8098", taskFamily: "craftax-singleplayer",
			lastRolloutId: "rollout-latest", health: { payload: { sessions: 2 } },
			metadata: {
				info: { lane: "rust", capabilities: ["rollout", "checkpoint", "task_catalog", "task_info"], action_names: ["noop", "left", "right", "do"] },
				taskCatalog: {
					tasks: [{ task_id: "manual", name: "Craftax single-player", description: "Explore and survive.", default: true }],
					instances: [
						{ task_instance_id: "craftax:train:1", task_id: "manual", split: "train", metadata: { output_label: "collect_wood", seed: 1 } },
						{ task_instance_id: "craftax:test:2", task_id: "manual", split: "test", metadata: { output_label: "place_table", seed: 2 } }
					]
				},
				taskInfo: { task: { task_id: "manual", name: "Craftax single-player" }, objective: "Advance through the technology tree.", output_space: { kind: "interactive_action" }, metrics: { primary: "achievements_unlocked" }, metadata: { labels: ["collect_wood", "place_table"] } },
				program: { program_id: "craftax_policy", modules: [{ module_id: "policy" }] },
				dataset: { dataset_id: "craftax_scenarios", splits: { train: 10, test: 5 } }
			},
			createdAt: "2026-08-09T10:00:00Z", updatedAt: "2026-08-09T10:00:00Z"
		};
		testWindow.synthInventory = {
			getContainer: async () => craftax,
			probeContainer: async () => craftax
		};
		testWindow.synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId: "tool-session", threadId: "thread-tools", workspace: "/workspaces/default",
				model: "poolside/Laguna-XS-2.1-NVFP4-mlx", providerName: "local-laguna",
				providerTitle: "Laguna XS", baseUrl: "http://127.0.0.1:7333", status: "ready"
			}],
			start: async () => ({ sessionId: "tool-session", threadId: "thread-tools" }),
			startTurn: async () => ({ sessionId: "tool-session", threadId: "thread-tools", turnId: "turn-tools" }),
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: (next: typeof listener) => {
				listener = next;
				testWindow.__emitToolCodex = next;
				return () => { listener = undefined; };
			}
		};
	});
	await installLagunaFixture(page, "ready");
	await page.getByTestId("local-chat-tool-session").click();
	await page.getByTestId("activity-mode-menu-trigger").click();
	await page.getByTestId("activity-mode-option-detailed").click();

	await page.evaluate(() => {
		const emit = (window as typeof window & { __emitToolCodex: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitToolCodex;
		const send = (method: string, params: Record<string, unknown>) => emit({ sessionId: "tool-session", method, params });
		send("turn/started", { turn: { id: "turn-tools" } });
		send("item/started", { item: { id: "container-1", type: "mcpToolCall", server: "synth_containers", tool: "container_probe", status: "inProgress", arguments: { container_id: "craftax-local", api_key: "MUST_NOT_RENDER" } } });
	});

	const transcript = page.getByTestId("chat-transcript");
	// Tool rows appear while the run is active, before an assistant message exists.
	await expect(transcript.locator("code.mcp-activity-name").getByText("synth_containers.container_probe")).toBeVisible();
	await expect(transcript.getByText("Running")).toBeVisible();
	await expect(transcript).toContainText("container id craftax-local");
	await expect(transcript).not.toContainText("MUST_NOT_RENDER");
	const containerOpen = transcript.getByTestId("tool-container-open-craftax-local");
	await expect(containerOpen).toBeVisible();
	await expect(transcript.getByTestId("resource-shelf-trigger")).toContainText("Outputs");
	const toolbarGeometry = await transcript.getByTestId("transcript-toolbar").evaluate((toolbar) => {
		const activity = toolbar.querySelector<HTMLElement>("[data-testid=activity-mode-menu-trigger]")!.getBoundingClientRect();
		const outputs = toolbar.querySelector<HTMLElement>("[data-testid=resource-shelf-trigger]")!.getBoundingClientRect();
		const bounds = toolbar.getBoundingClientRect();
		return { separated: activity.right + 4 <= outputs.left, contained: activity.top >= bounds.top && outputs.bottom <= bounds.bottom };
	});
	expect(toolbarGeometry).toEqual({ separated: true, contained: true });
	await expect(page.getByTestId("resource-shelf")).toHaveCount(0);
	await containerOpen.click();
	const containerPane = page.getByTestId("container-pane");
	await expect(containerPane).toBeVisible();
	await expect(containerPane).toContainText("Craftax Rust");
	await expect(containerPane).toContainText("craftax-singleplayer");
	await expect(containerPane).toContainText("2 active sessions");
	await expect(containerPane.getByRole("button", { name: /Craftax single-player/ })).toBeVisible();
	await expect(containerPane).toContainText("Advance through the technology tree.");
	await expect(containerPane).toContainText("achievements_unlocked");
	await expect(containerPane).toContainText("2 of 2 instances");
	await containerPane.getByRole("textbox", { name: "Filter task instances" }).fill("split:test output_label:table");
	await expect(containerPane).toContainText("1 of 2 instances");
	const instanceList = containerPane.locator(".container-instance-list");
	await expect(instanceList).toContainText("craftax:test:2");
	await expect(instanceList).not.toContainText("craftax:train:1");
	await containerPane.getByRole("textbox", { name: "Filter task instances" }).fill("split = 'train' AND metadata.output_label LIKE 'collect%'");
	await expect(containerPane).toContainText("1 of 2 instances");
	await expect(instanceList).toContainText("craftax:train:1");
	await expect(instanceList).not.toContainText("craftax:test:2");
	await containerPane.getByRole("combobox", { name: "Query field" }).selectOption("metadata.output_label");
	await expect(containerPane.getByRole("button", { name: "collect_wood" })).toBeVisible();
	await expect(containerPane).toContainText("Program contract");
	await expect(containerPane).toContainText("Dataset contract");
	await containerPane.getByRole("textbox", { name: "Query container metadata" }).fill("labels:wood");
	await expect(containerPane).toContainText("collect_wood");
	await containerPane.getByTestId("container-pane-expand").click();
	await expect(page.locator(".workbench")).toHaveClass(/container-expanded/);

	await page.evaluate(() => {
		const emit = (window as typeof window & { __emitToolCodex: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitToolCodex;
		const send = (method: string, params: Record<string, unknown>) => emit({ sessionId: "tool-session", method, params });
		send("item/agentMessage/delta", { delta: "I inspected the relevant files." });
		send("item/reasoning/delta", { delta: "Checking the first command." });
		send("item/completed", { item: { id: "container-1", type: "mcpToolCall", server: "synth_containers", tool: "container_probe", status: "completed", durationMs: 14, arguments: { container_id: "craftax-local" }, result: { secret: "RAW_RESULT_SECRET" } } });
		send("item/started", { item: { id: "visual-1", type: "dynamicToolCall", server: "synth_visuals", tool: "visual_create", status: "inProgress", arguments: { template_id: "craftax.rollout.v1", title: "Craftax rollout", props: { token: "HIDDEN_PROP" } } } });
		send("item/completed", { item: { id: "visual-1", type: "dynamicToolCall", server: "synth_visuals", tool: "visual_create", status: "failed", durationMs: 2, arguments: { template_id: "craftax.rollout.v1", title: "Craftax rollout" }, error: "RAW_ERROR_SECRET" } });
		send("item/completed", { item: { id: "visual-2", type: "mcpToolCall", server: "synth_visuals", tool: "visual_create", status: "completed", durationMs: 8, arguments: { template_id: "craftax.eval_matrix.v1", title: "Reward comparison" }, result: { structuredContent: { visual: { id: "vis-reward-comparison", title: "Reward comparison", templateId: "craftax.eval_matrix.v1", bindings: { schemaVersion: "synth.visual-bindings.v1", slots: [{ slot: "matrix", kind: "inline", data: { title: "Reward comparison", achievements: ["collect_wood"], points: [{ model: "Luna", achievements: 1, cost_usd: 0.01, accent: true, achievement_rates: { collect_wood: 1 } }] } }] }, metadata: {} } } } } });
		send("item/started", { item: { id: "cmd-1", type: "commandExecution", command: "OPENROUTER_API_KEY=super-secret-value rg -n renderer src" } });
		send("item/completed", { item: { id: "cmd-1", type: "commandExecution", command: "OPENROUTER_API_KEY=super-secret-value rg -n renderer src", aggregatedOutput: "raw command output must stay hidden" } });
		send("item/started", { item: { id: "read-1", type: "mcpToolCall", tool: "read_file", arguments: { path: "/work/src/App.tsx" } } });
		send("item/completed", { item: { id: "read-1", type: "mcpToolCall", tool: "read_file", arguments: { path: "/work/src/App.tsx" }, output: "file contents must stay hidden" } });
		send("item/completed", { item: { id: "search-1", type: "mcpToolCall", tool: "web_search", arguments: { query: "Codex app-server events" } } });
		send("item/completed", { item: { id: "unsafe-1", type: "mcpToolCall", tool: "dump_environment", output: "DO_NOT_RENDER_SECRET" } });
		send("item/agentMessage/delta", { itemId: "post-tools-preamble", delta: "I am continuing after the tools." });
		send("turn/completed", { turn: { id: "turn-tools" } });
	});

	await expect(transcript.getByText("Run Shell Command")).toBeVisible();
	const preamble = transcript.locator(".local-assistant p").filter({ hasText: "I inspected the relevant files." });
	const preambleTurn = preamble.locator("xpath=ancestor::div[contains(@class, 'local-turn')]");
	const postPreambleTool = preambleTurn.locator(".command-activity").filter({ hasText: "OPENROUTER_API_KEY=[redacted]" });
	expect((await postPreambleTool.boundingBox())!.y).toBeGreaterThan((await preamble.boundingBox())!.y);
	const laterPreamble = transcript.locator(".local-assistant p").filter({ hasText: "I am continuing after the tools." });
	await expect(laterPreamble).toBeVisible();
	expect((await laterPreamble.boundingBox())!.y).toBeGreaterThan((await postPreambleTool.boundingBox())!.y);
	await expect(transcript.getByText(/OPENROUTER_API_KEY=\[redacted\] rg/)).toBeVisible();
	await expect(transcript.getByText("App.tsx")).toBeVisible();
	await expect(transcript.getByText("Searched the web")).toBeVisible();
	await expect(transcript.locator("code.mcp-activity-name").getByText("synth_containers.container_probe")).toBeVisible();
	await expect(transcript.locator("code.mcp-activity-name").getByText("synth_visuals.visual_create")).toHaveCount(2);
	await expect(transcript.getByText("Completed")).toHaveCount(2);
	await expect(transcript.getByText("Failed")).toBeVisible();
	await expect(transcript).toContainText("template id craftax.rollout.v1 · title Craftax rollout · 2ms");
	await transcript.getByTestId("resource-shelf-trigger").click();
	const resourceShelf = page.getByTestId("resource-shelf");
	await expect(resourceShelf).toContainText("Containers");
	await expect(resourceShelf).toContainText("Visuals");
	await expect(resourceShelf).toContainText("Reward comparison");
	await page.getByRole("button", { name: "Close side panel" }).click();
	const visualOpen = transcript.getByTestId("tool-visual-open-vis-reward-comparison");
	await expect(visualOpen).toBeVisible();
	await visualOpen.click();
	const visualPane = page.getByTestId("visual-pane");
	await expect(visualPane).toBeVisible();
	await expect(visualPane).toContainText("Reward comparison");
	await expect(visualPane.getByTestId("visual-craftax-eval-matrix")).toBeVisible();
	await page.getByTestId("activity-mode-menu-trigger").click();
	await page.getByTestId("activity-mode-option-grouped").click();
	const groupedWithContext = transcript.locator(".activity-group").first();
	await groupedWithContext.locator(".activity-group-toggle").click();
	const contextualStep = groupedWithContext.locator(".activity-group-step.has-context").first();
	expect((await contextualStep.locator(".activity-group-action").boundingBox())!.y)
		.toBeGreaterThanOrEqual((await contextualStep.locator(".activity-group-context").boundingBox())!.y);
	await expect(transcript.getByText(/Worked .*ran 1 command, read 1 file, searched once, used 4 tools/)).toBeVisible();
	await expect(transcript).not.toContainText("super-secret-value");
	await expect(transcript).not.toContainText("raw command output");
	await expect(transcript).not.toContainText("file contents must stay hidden");
	await expect(transcript).not.toContainText("DO_NOT_RENDER_SECRET");
	await expect(transcript).not.toContainText("RAW_RESULT_SECRET");
	await expect(transcript).not.toContainText("RAW_ERROR_SECRET");
	await expect(transcript).not.toContainText("HIDDEN_PROP");
	await expect(page.getByTestId("model-working")).toBeHidden();
});

test("approval modes configure new native sessions and pending requests resolve inline", async ({ page }) => {
	await page.addInitScript(() => {
		let listener: ((event: { sessionId: string; method: string; params: Record<string, unknown> }) => void) | undefined;
		let started: Record<string, unknown> | undefined;
		const decisions: Array<{ sessionId: string; approvalId: string; decision: string }> = [];
		const testWindow = window as typeof window & {
			__approvalStarted?: () => Record<string, unknown> | undefined;
			__approvalDecisions?: () => typeof decisions;
			__emitApproval?: typeof listener;
			synthCodex?: unknown;
		};
		testWindow.__approvalStarted = () => started;
		testWindow.__approvalDecisions = () => decisions;
		testWindow.synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [],
			start: async (request: Record<string, unknown>) => {
				started = request;
				return { sessionId: request.sessionId, threadId: "thread-approval" };
			},
			startTurn: async (sessionId: string) => ({ sessionId, threadId: "thread-approval", turnId: "turn-approval" }),
			interrupt: async () => undefined,
			resolveApproval: async (sessionId: string, approvalId: string, decision: string) => {
				decisions.push({ sessionId, approvalId, decision });
				listener?.({ sessionId, method: decision === "reject" ? "approval.rejected" : "approval.granted", params: { approvalId, decision } });
			},
			close: async () => undefined,
			onEvent: (next: typeof listener) => { listener = next; testWindow.__emitApproval = next; return () => { listener = undefined; }; }
		};
	});
	await installLagunaFixture(page, "ready");

	await page.getByTestId("approval-mode-select").click();
	const menu = page.getByTestId("approval-mode-menu");
	await expect(menu.getByRole("option")).toHaveCount(6);
	await expect(menu).toContainText("Always ask");
	await expect(menu).toContainText("Ask for risky actions");
	await expect(menu).toContainText("Full system access");
	await menu.getByRole("option", { name: /Ask for risky actions/ }).click();
	await menu.getByRole("option", { name: /Full system access/ }).click();
	await page.getByTestId("composer-input").fill("check approvals");
	await page.getByTestId("composer-send").click();
	await expect(page.getByTestId("approval-mode-select")).toHaveText("RiskyFull");
	await expect(page.getByTestId("approval-mode-select")).toHaveAttribute("aria-label", "Permissions: Ask for risky actions; Full system access");

	const started = await page.evaluate(() => (window as typeof window & { __approvalStarted: () => Record<string, unknown> }).__approvalStarted());
	expect(started?.approvalPolicy).toBe("on-request");
	expect(started?.sandbox).toBe("danger-full-access");
	const sessionId = String(started?.sessionId);
	await page.evaluate((id) => {
		(window as typeof window & { __emitApproval: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitApproval({
			sessionId: id,
			method: "approval.requested",
			params: { approvalId: "approval-1", kind: "shell_command", detail: "Run a shell command in /workspaces/default", scope: "/workspaces/default", alwaysSupported: true }
		});
	}, sessionId);

	const card = page.getByTestId(/approval-activity-/);
	await expect(card).toContainText("Run a shell command in /workspaces/default");
	await expect(card.getByRole("button", { name: "Approve once" })).toBeVisible();
	await expect(card.getByRole("button", { name: "Always allow for this session" })).toBeVisible();
	await expect(card.getByRole("button", { name: "Reject" })).toBeVisible();
	await card.getByRole("button", { name: "Approve once" }).click();
	await expect(card).toBeHidden();
	expect(await page.evaluate(() => (window as typeof window & { __approvalDecisions: () => unknown[] }).__approvalDecisions())).toEqual([
		{ sessionId, approvalId: "approval-1", decision: "once" }
	]);
});

test("a recent folder can create and attach to a conversation from the landing composer", async ({ page }) => {
	await page.addInitScript(() => {
		let createdSessionId = "";
		let pickerSessionId = "";
		const scope = (sessionId: string) => ({
			sessionId,
			workspace: "/workspaces/default",
			attachments: pickerSessionId ? [{ path: "/Users/test/Documents/GitHub", access: "read_write", source: "native_picker", createdAt: new Date().toISOString() }] : [],
			revision: pickerSessionId ? 2 : 1,
			boundRevision: 1,
			bindingStatus: pickerSessionId ? "pending" : "active"
		});
		const testWindow = window as typeof window & {
			__workspacePickerSession?: () => string;
			synthCodex?: unknown;
			synthWorkspaceScope?: unknown;
		};
		testWindow.__workspacePickerSession = () => pickerSessionId;
		testWindow.synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [],
			start: async (request: { sessionId: string }) => {
				createdSessionId = request.sessionId;
				return { sessionId: request.sessionId, threadId: "thread-workspace" };
			},
			close: async () => undefined,
			onEvent: () => () => undefined
		};
		testWindow.synthWorkspaceScope = {
			get: async (sessionId: string) => scope(sessionId),
			listGrants: async () => [],
			listRecentFolders: async () => ["/Users/test/Documents/GitHub"],
			chooseAndAttach: async (sessionId: string) => {
				if (sessionId !== createdSessionId) throw new Error("picker used the wrong conversation");
				pickerSessionId = sessionId;
				return scope(sessionId);
			},
			attachRecent: async (sessionId: string) => {
				pickerSessionId = sessionId;
				return scope(sessionId);
			},
			removeAttachment: async (sessionId: string) => scope(sessionId)
		};
	});
	await installLagunaFixture(page, "ready");

	await page.getByTestId("composer-slash-btn").click();
	await page.getByTestId("slash-command-item-workspace").click();
	const addFolder = page.getByTestId("workspace-scope-menu").getByRole("menuitem", { name: "Add folder…" });
	await expect(addFolder).toBeEnabled();
	const recentFolder = page.getByRole("menuitem", { name: /Documents\/GitHub/ });
	await expect(recentFolder).toBeVisible();
	await recentFolder.click();

	await expect.poll(() => page.evaluate(() => (window as typeof window & { __workspacePickerSession: () => string }).__workspacePickerSession())).not.toBe("");
	await expect(page.getByTestId("workspace-attachment")).toContainText("GitHub");
});
