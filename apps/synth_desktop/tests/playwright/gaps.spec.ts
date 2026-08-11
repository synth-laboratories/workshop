import { expect, test } from "./browser.fixture";

/**
 * Remaining migration gaps. Flip each expected-failing case to a real assertion when the
 * corresponding Rust CoreRuntime / cloud slice lands.
 */
test.describe("coverage gaps", () => {
	test("CoreRuntime diagnostics are exposed through window.synthCore", async ({ page }) => {
		const diagnostics = await page.evaluate(async () => {
			if (!window.synthCore) throw new Error("synthCore bridge missing");
			return window.synthCore.diagnostics();
		});
		expect(diagnostics).toMatchObject({ integrityOk: true, migrationComplete: true });
		expect(diagnostics.databasePath).toBeTruthy();
	});

	test("runtime:event journal replay restores a Codex session after reload", async ({ page }) => {
		await page.addInitScript(() => {
			const sessionId = "journal-session";
			(window as typeof window & { synthCodex?: unknown }).synthCodex = {
				defaultWorkspace: async () => "/workspaces/default",
				list: async () => [{
					sessionId, threadId: "thread-journal", workspace: "/workspaces/default",
					model: "poolside/Laguna-XS-2.1-NVFP4-mlx", providerName: "local-laguna",
					providerTitle: "Laguna XS", baseUrl: "http://127.0.0.1:7333/v1", status: "ready"
				}],
				start: async () => ({ sessionId, threadId: "thread-journal" }),
				startTurn: async () => ({ sessionId, threadId: "thread-journal", turnId: "turn-journal" }),
				interrupt: async () => undefined,
				close: async () => undefined,
				onEvent: () => () => undefined
			};
			const rows = [
				{ sequence: 10, sessionSequence: 1, kind: "message.created", payload: { messageId: "user-1", role: "user", content: "persist this" } },
				{ sequence: 11, sessionSequence: 2, kind: "agentMessage/completed", payload: { messageId: "assistant-1", content: "Restored from the Rust journal." } }
			].map((row) => ({
				schemaVersion: "synth.desktop-app-event.v1" as const, eventId: `evt-${row.sequence}`,
				sessionId, source: "codex" as const, createdAt: "2026-08-08T23:00:00Z", ...row
			}));
			(window as typeof window & { synthCore?: unknown }).synthCore = {
				diagnostics: async () => ({ databasePath: "/tmp/core.sqlite3", schemaVersion: 1, integrityOk: true,
					contentStorePath: "/tmp/content", journalHead: 11, sessionCount: 1, runCount: 1, visualCount: 0, migrationComplete: true }),
				eventsAfter: async () => rows,
				sessionEventsAfter: async () => rows,
				onEvent: () => () => undefined
			};
		});
		await page.reload();
		await page.getByTestId("local-chat-journal-session").click();
		await expect(page.getByTestId("chat-transcript")).toContainText("persist this");
		await expect(page.getByTestId("chat-transcript")).toContainText("Restored from the Rust journal.");
	});

	test("MCP visual_manage create appears in originating chat without a second registry", async ({ page }) => {
		await page.addInitScript(() => {
			let listener: ((event: { sessionId: string; method: string; params: Record<string, unknown> }) => void) | undefined;
			const testWindow = window as typeof window & {
				__emitVisualCreateCodex?: typeof listener;
				synthCodex?: unknown;
				synthVisuals?: unknown;
				synthLaguna?: unknown;
			};
			testWindow.synthLaguna = {
				getStatus: async () => ({
					phase: "ready", baseUrl: "http://127.0.0.1:7333", backend: "mlx_lm",
					loadedModel: "poolside/Laguna-XS-2.1-NVFP4-mlx", detail: "ready", memoryBytes: null, updatedAt: Date.now()
				}),
				onStatus: () => () => undefined, listModels: async () => [],
				chooseModelDirectory: async () => null, setModelDirectory: async () => undefined, clearModelDirectory: async () => undefined
			};
			// Chat projection must open from structuredContent alone — registry hits fail the contract.
			testWindow.synthVisuals = {
				list: async () => { throw new Error("chat projection must not query the separate visuals registry"); },
				get: async () => { throw new Error("chat projection must not query the separate visuals registry"); },
				listTemplates: async () => [],
				getTemplate: async () => { throw new Error("unused"); },
				revisions: async () => [],
				create: async () => { throw new Error("unused"); },
				update: async () => { throw new Error("unused"); },
				save: async () => { throw new Error("unused"); },
				fork: async () => { throw new Error("unused"); },
				archive: async () => { throw new Error("unused"); },
				show: async () => { throw new Error("unused"); },
				onEvent: () => () => undefined,
				onShow: () => () => undefined
			};
			testWindow.synthCodex = {
				defaultWorkspace: async () => "/workspaces/default",
				list: async () => [{
					sessionId: "visual-create-session", threadId: "thread-visual-create", workspace: "/workspaces/default",
					model: "poolside/Laguna-XS-2.1-NVFP4-mlx", providerName: "local-laguna",
					providerTitle: "Laguna XS", baseUrl: "http://127.0.0.1:7333", status: "ready"
				}],
				start: async () => ({ sessionId: "visual-create-session", threadId: "thread-visual-create" }),
				startTurn: async () => ({ sessionId: "visual-create-session", threadId: "thread-visual-create", turnId: "turn-visual-create" }),
				interrupt: async () => undefined,
				close: async () => undefined,
				onEvent: (next: typeof listener) => {
					listener = next;
					testWindow.__emitVisualCreateCodex = next;
					return () => { listener = undefined; };
				}
			};
		});
		await page.reload();
		await page.getByTestId("local-chat-visual-create-session").click();
		await page.getByTestId("activity-mode-menu-trigger").click();
		await page.getByTestId("activity-mode-option-detailed").click();
		await page.evaluate(() => {
			const emit = (window as typeof window & { __emitVisualCreateCodex: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitVisualCreateCodex;
			const send = (method: string, params: Record<string, unknown>) => emit({ sessionId: "visual-create-session", method, params });
			send("turn/started", { turn: { id: "turn-visual-create" } });
			send("item/completed", {
				item: {
					id: "visual-manage-1",
					type: "mcpToolCall",
					server: "synth_visuals",
					tool: "visual_manage",
					status: "completed",
					durationMs: 12,
					arguments: { operation: "create", arguments: { template_id: "craftax.eval_matrix.v1", title: "Originating chat visual" } },
					result: {
						structuredContent: {
							visual: {
								id: "vis-originating-create",
								title: "Originating chat visual",
								templateId: "craftax.eval_matrix.v1",
								bindings: {
									schemaVersion: "synth.visual-bindings.v1",
									slots: [{
										slot: "matrix",
										kind: "inline",
										data: {
											title: "Originating chat visual",
											achievements: ["collect_wood"],
											points: [{ model: "Luna", achievements: 1, cost_usd: 0.01, accent: true, achievement_rates: { collect_wood: 1 } }]
										}
									}]
								},
								metadata: {}
							}
						}
					}
				}
			});
			send("turn/completed", { turn: { id: "turn-visual-create" } });
		});
		const transcript = page.getByTestId("chat-transcript");
		await expect(transcript.locator("code.mcp-activity-name").getByText("synth_visuals.visual_manage")).toBeVisible();
		const open = transcript.getByTestId("tool-visual-open-vis-originating-create");
		await expect(open).toBeVisible();
		await open.click();
		const visualPane = page.getByTestId("visual-pane");
		await expect(visualPane).toBeVisible();
		await expect(visualPane).toContainText("Originating chat visual");
		await expect(visualPane.getByTestId("visual-craftax-eval-matrix")).toBeVisible();
	});

	test("Intern is absent from every v0.1 navigation and setup surface", async ({ page }) => {
		await page.getByTestId("model-picker").click();
		const menu = page.getByTestId("model-dropdown");
		await expect(menu).toBeVisible();
		await expect(menu.getByText("Intern · Live", { exact: true })).toHaveCount(0);
		await expect(menu.getByText("Intern · Background", { exact: true })).toHaveCount(0);
		await expect(menu.getByTestId("model-option-local-laguna")).toBeVisible();
		await expect(page.getByTestId("cloud-list")).toHaveCount(0);
		await expect(page.getByTestId("new-sync-session")).toHaveCount(0);
		await expect(page.getByTestId("async-intern-pin")).toHaveCount(0);
		await expect(page.getByTestId("landing-page")).not.toContainText("Intern");
		await page.getByTestId("open-search").click();
		await expect(page.getByTestId("conversation-search")).not.toContainText("Intern");
	});

	test("Inventory traces/containers/usage are served from Rust storage", async ({ page }) => {
		await page.addInitScript(() => {
			const timestamp = "2026-08-08T23:00:00Z";
			window.synthInventory = {
				async listContainers() {
					return [{ id: "rust-container", name: "Rust container", location: "local", status: "ready", health: { ok: true }, metadata: {}, createdAt: timestamp, updatedAt: timestamp }];
				},
				async getContainer() { return (await this.listContainers())[0]; },
				async probeContainer() { return (await this.listContainers())[0]; },
				async listTraces() {
					return [{ id: "rust-trace", digest: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", title: "Rust trace", source: "local", metrics: [], metadata: {}, createdAt: timestamp }];
				},
				async getTrace() { return (await this.listTraces())[0]; },
				async listUsage() {
					return [{ id: "rust-usage", provider: "openrouter", model: "openai/gpt-5.6-luna", promptTokens: 2, completionTokens: 3, totalTokens: 5, createdAt: timestamp }];
				},
				async counts() { return { containers: 1, traces: 1, usage: 1 }; }
			};
			window.synthVisuals = {
				async list() { return []; }
			} as typeof window.synthVisuals;
		});
		await page.reload();
		await page.getByTestId("titlebar").waitFor();
		await page.getByTestId("open-inventory").click();
		await page.getByTestId("inventory-container-rust-container").waitFor();
		await page.getByTestId("inventory-tab-traces").click();
		await page.getByTestId("inventory-trace-rust-trace").waitFor();
		await expect(page.getByText("Filter traces", { exact: true })).toHaveCSS("position", "absolute");
		await page.getByTestId("filter-traces-container").selectOption("unassigned");
		await expect(page.getByTestId("inventory-trace-rust-trace")).toBeVisible();
		await page.getByTestId("filter-traces-model").selectOption("unknown");
		await expect(page.getByTestId("inventory-trace-rust-trace")).toContainText("Unknown model");
		await page.getByLabel("Trace source").selectOption("import");
		await expect(page.getByText("No traces match that filter.")).toBeVisible();
		await page.getByRole("button", { name: "Clear filters" }).click();
		await expect(page.getByTestId("inventory-trace-rust-trace")).toBeVisible();
		await page.getByTestId("inventory-tab-usage").click();
		await page.getByText("openai/gpt-5.6-luna").waitFor();
		await page.getByText("1 containers · 1 traces · 1 usage entries").waitFor();
	});
});
