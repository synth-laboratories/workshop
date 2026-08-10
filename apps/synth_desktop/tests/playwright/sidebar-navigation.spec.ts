import { expect, test } from "./browser.fixture";

test("sidebar omits the deprecated Connectors catalog", async ({ page }) => {
	await expect(page.getByTestId("open-connectors")).toHaveCount(0);
});

test("Search and the Command-K shortcut find and open conversations", async ({ page }) => {
	await page.addInitScript(() => {
		const session = {
			id: "searchable-chat",
			title: "Craftax rollout review",
			target: { kind: "local", model: "laguna-xs-2.1", adapter: null },
			createdAt: "2026-08-09T12:00:00.000Z",
			updatedAt: "2026-08-09T12:00:00.000Z",
			status: "ready",
			latestCursor: 0,
			metadata: {}
		};
		(window as typeof window & { synthRuntime?: unknown }).synthRuntime = {
			async request(path: string) {
				if (path === "/v1/health") return {
					runtimeId: "renderer-test", local: { mode: "unavailable", modelPath: null },
					intern: { mode: "demo" }, openrouter: { mode: "unconfigured" },
					inventory: { containers: 0, traces: 0, visuals: 0 }
				};
				if (path === "/v1/sessions") return { sessions: [session] };
				if (path === "/v1/projects") return { projects: [] };
				if (path.includes("/events")) return { events: [], nextCursor: 0, hasMore: false };
				throw new Error(`Unexpected renderer test request: ${path}`);
			},
			async subscribe() { return { close() {} }; }
		};
	});
	await page.reload();
	await page.getByTestId("titlebar").waitFor();

	await page.keyboard.press("Meta+k");
	const search = page.getByTestId("conversation-search");
	await expect(search).toBeVisible();
	await search.getByRole("searchbox", { name: "Search conversations" }).fill("Craftax");
	await search.getByRole("option", { name: /Craftax rollout review/ }).click();
	await expect(page.getByTestId("conversation-search")).toHaveCount(0);
	await expect(page.getByTestId("chat-transcript")).toBeVisible();
	const outputsTrigger = page.getByTestId("resource-shelf-trigger");
	const outputsPanel = page.getByTestId("resource-shelf");
	await expect(outputsTrigger).toBeVisible();
	await expect(outputsTrigger).toHaveAttribute("aria-expanded", "true");
	await expect(outputsPanel).toBeVisible();
	await expect(outputsPanel.getByTestId("resource-shelf-empty")).toContainText("No outputs yet");
	await outputsPanel.getByRole("button", { name: "Close outputs panel" }).click();
	await expect(outputsPanel).toHaveCount(0);
	await expect(outputsTrigger).toHaveAttribute("aria-expanded", "false");
	await outputsTrigger.click();
	await expect(outputsPanel).toBeVisible();

	await page.getByTestId("open-search").click();
	await expect(page.getByTestId("conversation-search")).toBeVisible();
	// The shortcut is a true toggle: a search opened with the sidebar must not trap
	// the user behind a modal if they reach for its advertised shortcut again.
	await page.keyboard.press("Meta+k");
	await expect(page.getByTestId("conversation-search")).toHaveCount(0);

	await page.getByTestId("open-search").click();
	await expect(page.getByTestId("conversation-search")).toBeVisible();
	await page.keyboard.press("Escape");
	await expect(page.getByTestId("conversation-search")).toHaveCount(0);
	await expect(page.getByTestId("open-search")).toBeFocused();

	await page.getByTestId("open-search").click();
	await expect(page.getByTestId("conversation-search")).toBeVisible();
	await page.getByTestId("search-scrim").click({ position: { x: 4, y: 4 } });
	await expect(page.getByTestId("conversation-search")).toHaveCount(0);

	await page.getByTestId("open-search").click();
	await expect(page.getByTestId("conversation-search")).toBeVisible();
	await page.getByRole("button", { name: "Close search" }).click();
	await expect(page.getByTestId("conversation-search")).toHaveCount(0);
});

test("dense search results scroll inside the dialog instead of clipping its last row", async ({ page }) => {
	await page.addInitScript(() => {
		const sessions = Array.from({ length: 24 }, (_, index) => ({
			id: `search-density-${index + 1}`,
			title: `Search density fixture ${index + 1}`,
			target: { kind: "local", model: "laguna-xs-2.1", adapter: null },
			createdAt: "2026-08-09T12:00:00.000Z",
			updatedAt: `2026-08-09T12:${String(index).padStart(2, "0")}:00.000Z`,
			status: "ready",
			latestCursor: 0,
			metadata: {}
		}));
		(window as typeof window & { synthRuntime?: unknown }).synthRuntime = {
			async request(path: string) {
				if (path === "/v1/health") return {
					runtimeId: "renderer-test", local: { mode: "unavailable", modelPath: null },
					intern: { mode: "demo" }, openrouter: { mode: "unconfigured" },
					inventory: { containers: 0, traces: 0, visuals: 0 }
				};
				if (path === "/v1/sessions") return { sessions };
				if (path === "/v1/projects") return { projects: [] };
				if (path.includes("/events")) return { events: [], nextCursor: 0, hasMore: false };
				throw new Error(`Unexpected renderer test request: ${path}`);
			},
			async subscribe() { return { close() {} }; }
		};
	});
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByTestId("open-search").click();

	const dialog = page.getByTestId("conversation-search");
	const results = dialog.locator(".conversation-results");
	const finalResult = dialog.getByRole("option", { name: /Search density fixture 24/ });
	await finalResult.scrollIntoViewIfNeeded();
	const geometry = await page.evaluate(() => {
		const dialogNode = document.querySelector<HTMLElement>("[data-testid=conversation-search]");
		const resultsNode = document.querySelector<HTMLElement>(".conversation-results");
		const finalNode = [...document.querySelectorAll<HTMLElement>('[role="option"]')]
			.find((node) => node.textContent?.includes("Search density fixture 24"));
		if (!dialogNode || !resultsNode || !finalNode) return null;
		const dialogRect = dialogNode.getBoundingClientRect();
		const resultsRect = resultsNode.getBoundingClientRect();
		const finalRect = finalNode.getBoundingClientRect();
		return {
			dialogBottom: dialogRect.bottom,
			resultsBottom: resultsRect.bottom,
			finalBottom: finalRect.bottom,
			finalTop: finalRect.top,
			resultsTop: resultsRect.top,
			scrolls: resultsNode.scrollHeight > resultsNode.clientHeight,
			noHorizontalOverflow: document.documentElement.scrollWidth <= window.innerWidth + 1
		};
	});
	expect(geometry).not.toBeNull();
	expect(geometry?.scrolls).toBe(true);
	expect(geometry?.finalTop).toBeGreaterThanOrEqual(geometry?.resultsTop ?? 0);
	expect(geometry?.finalBottom).toBeLessThanOrEqual((geometry?.resultsBottom ?? 0) - 6);
	expect(geometry?.resultsBottom).toBeLessThanOrEqual(geometry?.dialogBottom ?? 0);
	expect(geometry?.noHorizontalOverflow).toBe(true);

	await finalResult.click();
	await expect(dialog).toHaveCount(0);
	await expect(page.getByTestId("chat-transcript")).toBeVisible();
});

test("sidebar starts compact while retaining a working conversation and a reversible full history", async ({ page }) => {
	await page.addInitScript(() => {
		const sessions = Array.from({ length: 14 }, (_, index) => ({
			id: `sidebar-chat-${index}`,
			title: `Fuzzed conversation ${index + 1}`,
			target: { kind: "local", model: "laguna-xs-2.1", adapter: null },
			createdAt: "2026-08-09T12:00:00.000Z",
			updatedAt: `2026-08-09T12:${String(index).padStart(2, "0")}:00.000Z`,
			status: index === 13 ? "running" : "ready",
			latestCursor: 0,
			metadata: {}
		}));
		(window as typeof window & { synthRuntime?: unknown }).synthRuntime = {
			async request(path: string) {
				if (path === "/v1/health") return {
					runtimeId: "renderer-test", local: { mode: "mlx", modelPath: "/models/laguna" },
					intern: { mode: "demo" }, openrouter: { mode: "unconfigured" },
					inventory: { containers: 0, traces: 0, visuals: 0 }
				};
				if (path === "/v1/sessions") return { sessions };
				if (path === "/v1/projects") return { projects: [] };
				if (path.includes("/events")) return { events: [], nextCursor: 0, hasMore: false };
				throw new Error(`Unexpected renderer test request: ${path}`);
			},
			async subscribe() { return { close() {} }; }
		};
	});
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await expect(page.getByTestId("chat-list").locator(".chat-item .item-icon")).toHaveCount(0);
	await expect(page.getByTestId("local-chat-sidebar-chat-13")).toBeVisible();
	const working = page.getByTestId("chat-working-sidebar-chat-13");
	await expect(working).toBeVisible();
	await expect(working).toHaveAccessibleName("Working");
	const markerGeometry = await working.evaluate((marker) => {
		const row = marker.closest<HTMLElement>(".chat-item")!.getBoundingClientRect();
		const rect = marker.getBoundingClientRect();
		const style = getComputedStyle(marker);
		return { width: style.width, height: style.height, contained: rect.left >= row.left && rect.right <= row.right - 8 };
	});
	expect(markerGeometry).toEqual({ width: "15px", height: "15px", contained: true });
	await expect(page.locator('[data-testid^="local-chat-sidebar-chat-"]')).toHaveCount(10);
	await expect(page.getByTestId("sidebar-show-all-chats")).toContainText("Show 4 more");

	await page.getByTestId("sidebar-show-all-chats").click();
	await expect(page.locator('[data-testid^="local-chat-sidebar-chat-"]')).toHaveCount(14);
	await page.getByTestId("sidebar-show-fewer-chats").click();
	await expect(page.locator('[data-testid^="local-chat-sidebar-chat-"]')).toHaveCount(10);
	await expect(page.getByTestId("local-chat-sidebar-chat-13")).toBeVisible();
});

test("a daemon-reported decode rate stays with its one active local conversation", async ({ page }) => {
	await page.addInitScript(() => {
		const snapshot = {
			model: "poolside/Laguna-XS-2.1-NVFP4-mlx",
			resident: true,
			residentBytes: 21_000_000_000,
			queueDepth: 0,
			queueCapacity: 8,
			active: {
				generationId: "active-generation",
				phase: "decode",
				queuedAt: 1,
				startedAt: 2,
				firstTokenAt: 3,
				lastTokenAt: 4,
				promptTokens: 12,
				cachedTokens: 0,
				outputTokens: 8,
				cacheHitRatio: 0,
				prefillTokensPerSecond: null,
				decodeTokensPerSecond: 31.7,
				elapsedMs: 1200
			},
			rolling: {
				requestsCompleted: 0, requestsFailed: 0, requestsCancelled: 0,
				inputTokens: 0, outputTokens: 0, cachedTokens: 0,
				ttftP50Ms: null, ttftP95Ms: null, decodeTpsP50: null,
				decodeTpsP95: null, latencyP50Ms: null, latencyP95Ms: null
			}
		};
		(window as typeof window & { __SYNTH_TEST_INFERENCE_TRANSPORT__?: unknown }).__SYNTH_TEST_INFERENCE_TRANSPORT__ = {
			snapshot: async () => snapshot,
			subscribe: () => () => undefined,
			unload: async () => ({ released: true, conflict: false, detail: null })
		};
		const session = {
			id: "decoded-local-chat", title: "Measured local generation",
			target: { kind: "local", model: "laguna-xs-2.1", adapter: null },
			createdAt: "2026-08-09T12:00:00.000Z", updatedAt: "2026-08-09T12:00:00.000Z",
			status: "running", latestCursor: 0, metadata: {}
		};
		(window as typeof window & { synthRuntime?: unknown }).synthRuntime = {
			async request(path: string) {
				if (path === "/v1/health") return {
					runtimeId: "renderer-test", local: { mode: "mlx", modelPath: "/models/laguna" },
					intern: { mode: "demo" }, openrouter: { mode: "unconfigured" },
					inventory: { containers: 0, traces: 0, visuals: 0 }
				};
				if (path === "/v1/sessions") return { sessions: [session] };
				if (path === "/v1/projects") return { projects: [] };
				if (path.includes("/events")) return { events: [], nextCursor: 0, hasMore: false };
				throw new Error(`Unexpected renderer test request: ${path}`);
			},
			async subscribe() { return { close() {} }; }
		};
	});
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByTestId("local-chat-decoded-local-chat").click();

	const status = page.getByTestId("chat-working-decoded-local-chat");
	const rate = page.getByTestId("chat-working-rate-decoded-local-chat");
	await expect(status).toHaveAccessibleName("Working · 31.7 tok/s");
	await expect(rate).toHaveText("31.7 tok/s");
	const geometry = await status.evaluate((element) => {
		const row = element.closest<HTMLElement>(".chat-item")!.getBoundingClientRect();
		const rect = element.getBoundingClientRect();
		return { contained: rect.left >= row.left && rect.right <= row.right };
	});
	expect(geometry).toEqual({ contained: true });
});

test("reversible navigation does not retain abandoned DOM", async ({ page }) => {
	const baseline = await page.evaluate(() => document.querySelectorAll("*").length);

	for (let attempt = 0; attempt < 4; attempt += 1) {
		await page.getByTestId("open-search").click();
		await page.keyboard.press("Escape");
		await expect(page.getByTestId("conversation-search")).toHaveCount(0);

		await page.getByTestId("account-footer-trigger").click();
		await page.getByTestId("settings").click();
		await page.getByTestId("settings-page").getByRole("button", { name: "← Back" }).click();
		await expect(page.getByTestId("landing-page")).toBeVisible();

	}

	const after = await page.evaluate(() => document.querySelectorAll("*").length);
	expect(after).toBeLessThanOrEqual(baseline + 24);
});

test("chat rows distinguish working from finished-unviewed and clear unread on open", async ({ page }) => {
	await page.addInitScript(() => {
		type Event = { sessionId: string; method: string; params: Record<string, unknown> };
		let listener: ((event: Event) => void) | undefined;
		(window as typeof window & { __emitSidebarStatus?: (event: Event) => void }).__emitSidebarStatus = (event) => listener?.(event);
		(window as typeof window & { synthCodex?: unknown }).synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId: "background-chat", threadId: "background-thread", workspace: "/workspaces/default",
				model: "openai/gpt-5.6-luna", providerName: "openrouter", providerTitle: "OpenRouter",
				baseUrl: "https://openrouter.ai/api/v1", status: "ready", title: "Background review"
			}],
			start: async () => ({ sessionId: "background-chat", threadId: "background-thread" }),
			startTurn: async () => ({ sessionId: "background-chat", threadId: "background-thread", turnId: "turn-1" }),
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: (next: (event: Event) => void) => { listener = next; return () => { listener = undefined; }; }
		};
	});
	await page.reload();
	await page.getByTestId("local-chat-background-chat").waitFor();

	await page.evaluate(() => {
		(window as typeof window & { __emitSidebarStatus: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitSidebarStatus({
			sessionId: "background-chat", method: "turn/started", params: { turnId: "turn-1" }
		});
	});
	await expect(page.getByTestId("chat-working-background-chat")).toBeVisible();
	await expect(page.getByTestId("chat-unread-background-chat")).toHaveCount(0);

	await page.evaluate(() => {
		(window as typeof window & { __emitSidebarStatus: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitSidebarStatus({
			sessionId: "background-chat", method: "turn/completed", params: { turnId: "turn-1" }
		});
	});
	await expect(page.getByTestId("chat-working-background-chat")).toHaveCount(0);
	await expect(page.getByTestId("chat-unread-background-chat")).toBeVisible();
	await expect(page.getByTestId("chat-unread-background-chat")).toHaveAccessibleName("Finished, unviewed");
	expect(await page.evaluate(() => JSON.parse(localStorage.getItem("synth.unreadCompletedChats") ?? "[]"))).toContain("background-chat");

	await page.getByTestId("local-chat-background-chat").click();
	await expect(page.getByTestId("chat-unread-background-chat")).toHaveCount(0);
	expect(await page.evaluate(() => JSON.parse(localStorage.getItem("synth.unreadCompletedChats") ?? "[]"))).not.toContain("background-chat");
});
