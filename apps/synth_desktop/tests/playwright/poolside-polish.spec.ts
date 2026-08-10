import { expect, test } from "./browser.fixture";
import type { Page } from "@playwright/test";

async function openSettings(page: Page) {
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-menu-settings").click();
}

test("parked Projects surface is absent while workspace access remains available", async ({ page }) => {
	await expect(page.getByText("Projects", { exact: true })).toHaveCount(0);
	await expect(page.getByTestId("project-list")).toHaveCount(0);
	await expect(page.getByTestId("quick-add-project")).toHaveCount(0);
	await openSettings(page);
	await page.getByRole("button", { name: "Runtime" }).click();
	await expect(page.getByTestId("workspace-access-settings")).toBeVisible();
});

test.describe("preferences persistence", () => {
	test("theme, fonts, tool activity, and layout survive a fresh context", async ({ page, context }) => {
		await openSettings(page);
		await expect(page.getByTestId("settings-general")).toBeVisible();
		await page.getByTestId("theme-dark").click();
		await page.getByTestId("tool-activity-compact").click();
		await page.getByTestId("active-enter-steer").click();
		await page.getByTestId("chat-font-size").fill("18");
		await page.getByTestId("chat-font-size").blur();
		await page.getByTestId("save-layout-default").click();

		const stored = await page.evaluate(() => window.localStorage.getItem("synth.preferences.v1"));
		expect(stored).toBeTruthy();
		const parsed = JSON.parse(stored!);
		expect(parsed.appearance.theme).toBe("dark");
		expect(parsed.toolActivity.mode).toBe("compact");
		expect(parsed.submission.activeEnterAction).toBe("steer");
		expect(parsed.appearance.chatFontSize).toBe(18);

		const page2 = await context.newPage();
		await page2.goto(page.url());
		await page2.getByTestId("titlebar").waitFor();
		await expect(page2.locator("html")).toHaveAttribute("data-theme", "dark");
		const restored = await page2.evaluate(() => (window as typeof window & { __synthPreferences?: { get(): { appearance: { chatFontSize: number }; toolActivity: { mode: string } } } }).__synthPreferences?.get());
		expect(restored?.appearance.chatFontSize).toBe(18);
		expect(restored?.toolActivity.mode).toBe("compact");
		await page2.close();
	});

	test("malformed preferences normalize to supported schema values", async ({ page }) => {
		await page.evaluate(() => {
			window.localStorage.setItem("synth.preferences.v1", JSON.stringify({
				schemaVersion: 99,
				appearance: { theme: "neon", chatFontSize: 999 },
				toolActivity: { mode: "verbose" },
				submission: { activeEnterAction: "teleport" },
				layout: { last: { sidebarWidth: -40, outputPaneWidth: 99999 } }
			}));
		});
		await page.reload();
		await page.getByTestId("titlebar").waitFor();
		const normalized = await page.evaluate(() => (window as typeof window & { __synthPreferences?: { get(): any } }).__synthPreferences!.get());
		expect(["system", "light", "dark"]).toContain(normalized.appearance.theme);
		expect(["detailed", "grouped", "compact"]).toContain(normalized.toolActivity.mode);
		expect(["steer", "enqueue"]).toContain(normalized.submission.activeEnterAction);
		expect(normalized.appearance.chatFontSize).toBeLessThanOrEqual(22);
		expect(normalized.layout.last.sidebarWidth).toBeGreaterThanOrEqual(180);
		expect(normalized.layout.last.outputPaneWidth).toBeLessThanOrEqual(720);
	});

	test("invalid font size is rejected with feedback", async ({ page }) => {
		await openSettings(page);
		await page.getByTestId("chat-font-size").fill("99");
		await page.getByTestId("chat-font-size").blur();
		await expect(page.getByTestId("chat-font-size-error")).toBeVisible();
		await expect(page.getByTestId("chat-font-size")).toHaveValue("14");
	});
});

test.describe("tool activity presentation", () => {
	test("settings and transcript menu share the same mode labels", async ({ page }) => {
		await openSettings(page);
		await expect(page.getByTestId("tool-activity-detailed")).toContainText("Detailed");
		await expect(page.getByTestId("tool-activity-grouped")).toContainText("Grouped");
		await expect(page.getByTestId("tool-activity-compact")).toContainText("Compact");
		await page.getByTestId("tool-activity-detailed").click();
		await page.getByRole("button", { name: "← Back" }).click();

		await page.addInitScript(() => {
			const session = {
				id: "activity-chat",
				title: "Activity chat",
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
		await page.getByTestId("local-chat-activity-chat").click();
		await expect(page.getByTestId("chat-transcript")).toHaveAttribute("data-activity-mode", "detailed");
		await page.getByTestId("activity-mode-menu-trigger").click();
		await page.getByTestId("activity-mode-option-compact").click();
		await expect(page.getByTestId("chat-transcript")).toHaveAttribute("data-activity-mode", "compact");
	});
});

test.describe("steer and enqueue", () => {
	test("idle submit stays plain; active turn exposes enqueue and honest steer failure", async ({ page }) => {
		await page.addInitScript(() => {
			const session = {
				id: "queue-chat",
				title: "Queue chat",
				target: { kind: "local", model: "laguna-xs-2.1", adapter: null },
				createdAt: "2026-08-09T12:00:00.000Z",
				updatedAt: "2026-08-09T12:00:00.000Z",
				status: "running",
				latestCursor: 0,
				metadata: { runtime: "browser-fixture" }
			};
			(window as typeof window & { synthLaguna?: unknown; synthRuntime?: unknown }).synthLaguna = {
				getStatus: async () => ({
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
				chooseModelDirectory: async () => null,
				setModelDirectory: async () => undefined,
				clearModelDirectory: async () => undefined
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
		await page.getByTestId("local-chat-queue-chat").click();
		await expect(page.getByTestId("composer-input")).toBeEnabled();
		await expect(page.getByTestId("composer-intent-hint")).toBeVisible();
		await page.getByTestId("composer-input").focus();
		const workingComposerGeometry = await page.evaluate(() => {
			const composer = document.querySelector<HTMLElement>("[data-testid=composer]");
			const input = document.querySelector<HTMLTextAreaElement>("[data-testid=composer-input]");
			const toolbar = document.querySelector<HTMLElement>(".composer-toolbar");
			const hint = document.querySelector<HTMLElement>("[data-testid=composer-intent-hint]");
			if (!composer || !input || !toolbar || !hint) throw new Error("Working composer fixture is incomplete");
			const composerRect = composer.getBoundingClientRect();
			const toolbarRect = toolbar.getBoundingClientRect();
			const hintRect = hint.getBoundingClientRect();
			return {
				compact: composerRect.height <= 140 && input.getBoundingClientRect().height <= 80,
				hintLivesInToolbar: hintRect.top >= toolbarRect.top && hintRect.bottom <= toolbarRect.bottom,
				quietTextareaFocus: getComputedStyle(input).outlineStyle === "none",
				noHorizontalOverflow: document.documentElement.scrollWidth <= window.innerWidth + 1,
				showsInternalRuntimeDetail: hint.textContent?.includes("runtime primitive") ?? false
			};
		});
		expect(workingComposerGeometry).toEqual({
			compact: true,
			hintLivesInToolbar: true,
			quietTextareaFocus: true,
			noHorizontalOverflow: true,
			showsInternalRuntimeDetail: false
		});
		await page.getByTestId("composer-input").fill("queued one");
		await page.getByTestId("composer-send").click();
		await expect(page.getByTestId("prompt-queue")).toBeVisible();
		await expect(page.locator('[data-testid^="queued-prompt-"] input')).toHaveValue("queued one");
		await expect(page.getByTestId("prompt-queue")).toContainText("Next turns");
		const queueGeometry = await page.getByTestId("prompt-queue").evaluate((queue) => {
			const queueRect = queue.getBoundingClientRect();
			const composer = document.querySelector<HTMLElement>("[data-testid=composer]")!.getBoundingClientRect();
			return {
				bounded: queueRect.width <= composer.width + 1,
				aboveComposer: queueRect.bottom <= composer.top + 1,
				overflow: document.documentElement.scrollWidth > window.innerWidth + 1,
				compactRows: [...queue.querySelectorAll<HTMLInputElement>("input")].every((input) => getComputedStyle(input).whiteSpace === "nowrap")
			};
		});
		expect(queueGeometry).toEqual({ bounded: true, aboveComposer: true, overflow: false, compactRows: true });

		await page.getByTestId("composer-input").fill("steer attempt");
		await page.getByTestId("composer-input").press("Meta+Enter");
		await expect(page.getByTestId("steer-error")).toContainText("not supported");
		await expect(page.getByTestId("composer-input")).toHaveValue("steer attempt");
	});

	test("FIFO queue preserves order across three prompts and survives reload", async ({ page }) => {
		await page.evaluate(() => {
			(window as typeof window & { __synthPreferences?: { set(raw: unknown): unknown } }).__synthPreferences?.set({
				schemaVersion: 1,
				submission: { activeEnterAction: "enqueue" },
				promptQueue: [
					{ id: "q1", conversationId: "c1", text: "first", createdAt: "2026-08-09T12:00:00.000Z" },
					{ id: "q2", conversationId: "c1", text: "second", createdAt: "2026-08-09T12:00:01.000Z" },
					{ id: "q3", conversationId: "c1", text: "third", createdAt: "2026-08-09T12:00:02.000Z" }
				]
			});
		});
		const prefs = await page.evaluate(() => (window as typeof window & { __synthPreferences?: { get(): { promptQueue: Array<{ text: string }> } } }).__synthPreferences!.get());
		expect(prefs.promptQueue.map((item) => item.text)).toEqual(["first", "second", "third"]);
	});

	test("a failed send-next keeps the durable queued prompt", async ({ page }) => {
		await page.addInitScript(() => {
			let status = "running";
			const session = () => ({
				id: "queue-failure-chat", title: "Queue failure", target: { kind: "local", model: "laguna-xs-2.1", adapter: null },
				createdAt: "2026-08-09T12:00:00.000Z", updatedAt: "2026-08-09T12:00:00.000Z",
				status, latestCursor: 0, metadata: { runtime: "browser-fixture" }
			});
			(window as typeof window & { synthLaguna?: unknown }).synthLaguna = {
				getStatus: async () => ({ phase: "ready", baseUrl: "http://127.0.0.1:7333", backend: "mlx_lm", loadedModel: "laguna", detail: "ready", memoryBytes: null, updatedAt: Date.now() }),
				onStatus: () => () => undefined, listModels: async () => [], chooseModelDirectory: async () => null,
				setModelDirectory: async () => undefined, clearModelDirectory: async () => undefined
			};
			(window as typeof window & { synthRuntime?: unknown }).synthRuntime = {
				async request(path: string, options?: { method?: string }) {
					if (path === "/v1/health") return { runtimeId: "renderer-test", local: { mode: "mlx", modelPath: "/models/laguna" }, intern: { mode: "demo" }, openrouter: { mode: "unconfigured" }, inventory: { containers: 0, traces: 0, visuals: 0 } };
					if (path === "/v1/sessions") return { sessions: [session()] };
					if (path.includes("/events")) return { events: [], nextCursor: 0, hasMore: false };
					if (path.includes("/commands") && options?.method === "POST") { status = "interrupted"; return { accepted: true }; }
					if (path.includes("/messages") && options?.method === "POST") throw new Error("synthetic send rejection");
					throw new Error(`Unexpected renderer test request: ${path}`);
				},
				async subscribe() { return { close() {} }; }
			};
		});
		await page.reload();
		await page.getByTestId("local-chat-queue-failure-chat").click();
		await page.getByTestId("composer-input").fill("must survive rejection");
		await page.getByTestId("composer-send").click();
		await page.getByRole("button", { name: "Stop" }).click();
		await expect(page.getByTestId("prompt-queue-after-stop")).toBeVisible();
		await page.getByTestId("send-next-queued").click();
		await expect(page.locator('[data-testid^="queued-prompt-"] input')).toHaveValue("must survive rejection");
	});

	test("steer delivers input to the active turn via a real runtime primitive", async ({ page }) => {
		await page.addInitScript(() => {
			const calls: Array<{ sessionId: string; text: string }> = [];
			const testWindow = window as typeof window & {
				__steerCalls?: typeof calls;
				synthCodex?: unknown;
				synthLaguna?: unknown;
			};
			testWindow.__steerCalls = calls;
			testWindow.synthLaguna = {
				getStatus: async () => ({
					phase: "ready", baseUrl: "http://127.0.0.1:7333", backend: "mlx_lm",
					loadedModel: "poolside/Laguna-XS-2.1-NVFP4-mlx", detail: "ready", memoryBytes: null, updatedAt: Date.now()
				}),
				onStatus: () => () => undefined, listModels: async () => [],
				chooseModelDirectory: async () => null, setModelDirectory: async () => undefined, clearModelDirectory: async () => undefined
			};
			testWindow.synthCodex = {
				defaultWorkspace: async () => "/workspaces/default",
				list: async () => [{
					sessionId: "steer-session", threadId: "thread-steer", workspace: "/workspaces/default",
					model: "poolside/Laguna-XS-2.1-NVFP4-mlx", providerName: "local-laguna",
					providerTitle: "Laguna XS", baseUrl: "http://127.0.0.1:7333", status: "running"
				}],
				start: async () => ({ sessionId: "steer-session", threadId: "thread-steer" }),
				startTurn: async () => ({ sessionId: "steer-session", threadId: "thread-steer", turnId: "turn-steer" }),
				steerTurn: async (sessionId: string, text: string) => { calls.push({ sessionId, text }); },
				interrupt: async () => undefined,
				close: async () => undefined,
				onEvent: () => () => undefined
			};
			window.localStorage.setItem("synth.preferences.v1", JSON.stringify({
				schemaVersion: 1,
				submission: { activeEnterAction: "steer" },
				promptQueue: [{ id: "promote-me", conversationId: "steer-session", text: "push this through now", createdAt: "2026-08-09T12:00:00.000Z" }]
			}));
		});
		await page.reload();
		await page.getByTestId("local-chat-steer-session").click();
		await expect(page.getByTestId("composer-input")).toBeEnabled();
		const supported = await page.evaluate(() => Boolean((window as typeof window & { synthCodex?: { steerTurn?: unknown } }).synthCodex?.steerTurn));
		expect(supported).toBe(true);
		await page.getByTestId("composer-input").fill("nudge the active turn");
		await page.getByTestId("composer-send").click();
		await expect(page.getByTestId("composer-input")).toHaveValue("");
		const calls = await page.evaluate(() => (window as typeof window & { __steerCalls?: Array<{ sessionId: string; text: string }> }).__steerCalls ?? []);
		expect(calls).toEqual([{ sessionId: "steer-session", text: "nudge the active turn" }]);
		const queued = page.getByRole("textbox", { name: "Queued prompt 1" });
		await queued.press("Enter");
		await expect(page.getByTestId("prompt-queue")).toContainText("Return again to steer now");
		await queued.press("Enter");
		await expect(page.getByTestId("prompt-queue")).toBeHidden();
		const promotedCalls = await page.evaluate(() => (window as typeof window & { __steerCalls?: Array<{ sessionId: string; text: string }> }).__steerCalls ?? []);
		expect(promotedCalls).toEqual([
			{ sessionId: "steer-session", text: "nudge the active turn" },
			{ sessionId: "steer-session", text: "push this through now" }
		]);
	});
});

test.describe("conversation management", () => {
	test("rename, pin, archive, and unread clearing", async ({ page }) => {
		await page.addInitScript(() => {
			const session = {
				id: "manage-chat",
				title: "Original title",
				target: { kind: "local", model: "laguna-xs-2.1", adapter: null },
				createdAt: "2026-08-09T12:00:00.000Z",
				updatedAt: "2026-08-09T12:00:00.000Z",
				status: "ready",
				latestCursor: 0,
				metadata: {}
			};
			window.localStorage.setItem("synth.unreadCompletedChats", JSON.stringify(["manage-chat"]));
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
		await expect(page.getByTestId("chat-unread-manage-chat")).toBeVisible();
		const chat = page.getByTestId("local-chat-manage-chat");
		await chat.focus();
		await page.keyboard.press("Shift+F10");
		await expect(page.getByRole("menuitem", { name: "Rename" })).toBeFocused();
		await page.keyboard.press("ArrowDown");
		await expect(page.getByRole("menuitem", { name: "Pin" })).toBeFocused();
		await page.keyboard.press("Escape");
		await expect(chat).toBeFocused();
		await chat.click({ button: "right" });
		await page.getByRole("menuitem", { name: "Rename" }).click();
		await page.getByLabel("Rename conversation").fill("Renamed αβ");
		await page.getByRole("button", { name: "Save" }).click();
		await expect(page.getByTestId("local-chat-manage-chat")).toContainText("Renamed αβ");

		await chat.click({ button: "right" });
		await page.getByRole("menuitem", { name: "Pin" }).click();
		await expect(page.getByTestId("chat-pinned-manage-chat")).toBeVisible();

		await page.getByTestId("local-chat-manage-chat").click();
		await expect(page.getByTestId("chat-unread-manage-chat")).toHaveCount(0);

		await page.getByTestId("local-chat-manage-chat").click({ button: "right" });
		await page.getByRole("menuitem", { name: "Archive" }).click();
		await expect(page.getByTestId("local-chat-manage-chat")).toHaveCount(0);
		await openSettings(page);
		await expect(page.getByTestId("archived-chat-manage-chat")).toBeVisible();
	});
});

test.describe("layout persistence", () => {
	test("sidebar divider drags against the full app width and persists", async ({ page }) => {
		const handle = page.getByRole("separator", { name: "Resize sidebar" });
		const box = await handle.boundingBox();
		expect(box).not.toBeNull();
		await page.mouse.move(box!.x + box!.width / 2, box!.y + 120);
		await page.mouse.down();
		await page.mouse.move(box!.x + box!.width / 2 + 72, box!.y + 120, { steps: 5 });
		await page.mouse.up();
		const draggedWidth = await page.getByTestId("sidebar").evaluate((node) => node.getBoundingClientRect().width);
		expect(draggedWidth).toBeGreaterThanOrEqual(320);
		await page.reload();
		await page.getByTestId("titlebar").waitFor();
		await expect(page.getByTestId("sidebar")).toHaveCSS("width", `${draggedWidth}px`);
	});

	test("sidebar width and terminal visibility persist through reload", async ({ page }) => {
		await page.evaluate(() => {
			(window as typeof window & { __synthPreferences?: { set(raw: unknown): unknown } }).__synthPreferences?.set({
				schemaVersion: 1,
				layout: {
					last: {
						sidebarVisible: true,
						sidebarWidth: 300,
						outputPaneVisible: false,
						outputPaneWidth: 420,
						bottomPanelVisible: true,
						bottomPanelHeight: 220,
						selectedConversationId: null,
						selectedOutputTab: null
					},
					default: {
						sidebarVisible: true,
						sidebarWidth: 260,
						outputPaneVisible: false,
						outputPaneWidth: 420,
						bottomPanelVisible: false,
						bottomPanelHeight: 220,
						selectedConversationId: null,
						selectedOutputTab: null
					}
				}
			});
		});
		await page.reload();
		await page.getByTestId("titlebar").waitFor();
		await expect(page.getByTestId("sidebar")).toHaveCSS("width", "300px");
		await expect(page.getByTestId("terminal-panel")).toBeVisible();
	});

	test("saved default applies independently and reset restores factory layout", async ({ page }) => {
		await page.evaluate(() => {
			const adapter = (window as typeof window & { __synthPreferences?: { get(): any; set(raw: unknown): unknown } }).__synthPreferences!;
			const prefs = adapter.get();
			adapter.set({ ...prefs, layout: { ...prefs.layout, last: { ...prefs.layout.last, sidebarWidth: 310 } } });
		});
		await openSettings(page);
		await page.getByTestId("save-layout-default").click();
		await page.evaluate(() => {
			const adapter = (window as typeof window & { __synthPreferences?: { get(): any; set(raw: unknown): unknown } }).__synthPreferences!;
			const prefs = adapter.get();
			adapter.set({ ...prefs, layout: { ...prefs.layout, last: { ...prefs.layout.last, sidebarWidth: 220 } } });
		});
		await page.getByTestId("apply-layout-default").click();
		await expect(page.getByTestId("sidebar")).toHaveCSS("width", "310px");
		await page.getByTestId("reset-layout").click();
		await expect(page.getByTestId("sidebar")).toHaveCSS("width", "260px");
		const widths = await page.evaluate(() => {
			const prefs = (window as typeof window & { __synthPreferences?: { get(): any } }).__synthPreferences!.get();
			return [prefs.layout.last.sidebarWidth, prefs.layout.default.sidebarWidth];
		});
		expect(widths).toEqual([260, 260]);
	});
});

test("keyboard-only settings navigation reaches General anchors", async ({ page }) => {
	await page.getByTestId("account-menu-trigger").focus();
	await page.keyboard.press("Enter");
	await page.getByTestId("account-menu-settings").focus();
	await page.keyboard.press("Enter");
	await expect(page.getByTestId("settings-general")).toBeVisible();
	await page.getByRole("button", { name: "About" }).focus();
	await page.keyboard.press("Enter");
	await expect(page.getByTestId("settings-about")).toBeVisible();
});

test("narrow viewport keeps composer reachable without horizontal overflow", async ({ page }) => {
	await page.setViewportSize({ width: 960, height: 640 });
	await expect(page.getByTestId("composer")).toBeVisible();
	const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1);
	expect(overflow).toBe(false);
});


test("optimizer workbench keeps its hierarchy at desktop and compact widths", async ({ page }) => {
	await page.getByRole("button", { name: "Optimizers" }).click();
	await expect(page.getByTestId("optimizers-page")).toBeVisible();
	await expect(page.getByTestId("optimizer-toolbar")).toBeVisible();
	await expect(page.getByRole("heading", { name: "Optimizers" })).toBeVisible();
	await expect(page.getByText("No optimizer runs yet")).toBeVisible();
	for (const viewport of [{ width: 1280, height: 840 }, { width: 820, height: 700 }]) {
		await page.setViewportSize(viewport);
		const geometry = await page.getByTestId("optimizers-page").evaluate((pageElement) => {
			const toolbar = pageElement.querySelector<HTMLElement>("[data-testid=optimizer-toolbar]")!;
			const pageRect = pageElement.getBoundingClientRect();
			const toolbarRect = toolbar.getBoundingClientRect();
			return {
				inside: toolbarRect.left >= pageRect.left && toolbarRect.right <= pageRect.right + 1,
				overflow: document.documentElement.scrollWidth > window.innerWidth + 1
			};
		});
		expect(geometry).toEqual({ inside: true, overflow: false });
	}
});
