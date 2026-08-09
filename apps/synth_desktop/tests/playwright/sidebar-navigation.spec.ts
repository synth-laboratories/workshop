import { expect, test } from "./browser.fixture";

test("Connectors opens a searchable MCP catalog", async ({ page }) => {
	await page.getByTestId("open-connectors").click();

	const catalog = page.getByTestId("connectors-page");
	await expect(catalog).toBeVisible();
	await expect(catalog).toContainText("MCP servers available to your agents");
	await expect(catalog.getByRole("button", { name: "Synth Containers, bundled" })).toBeVisible();

	await catalog.getByRole("searchbox", { name: "Search connectors" }).fill("github");
	await expect(catalog.getByRole("button", { name: "Configure GitHub" })).toBeVisible();
	await expect(catalog.getByRole("button", { name: "Configure Notion" })).toHaveCount(0);
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
	await page.getByTestId("runtime-status").waitFor();

	await page.keyboard.press("Meta+k");
	const search = page.getByTestId("conversation-search");
	await expect(search).toBeVisible();
	await search.getByRole("searchbox", { name: "Search conversations" }).fill("Craftax");
	await search.getByRole("option", { name: /Craftax rollout review/ }).click();
	await expect(page.getByTestId("conversation-search")).toHaveCount(0);
	await expect(page.getByTestId("chat-transcript")).toBeVisible();

	await page.getByTestId("open-search").click();
	await expect(page.getByTestId("conversation-search")).toBeVisible();
	await page.keyboard.press("Escape");
	await expect(page.getByTestId("conversation-search")).toHaveCount(0);
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
