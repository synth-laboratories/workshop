import { expect, test } from "./browser.fixture";

/**
 * Remaining migration gaps. Flip each `test.fail` to a real assertion when the
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

	test.fail("MCP visual_create appears in originating chat without a second registry", async () => {
		throw new Error("Need end-to-end MCP → visuals IPC → chat projection dogfood");
	});

	test.fail("Intern Live mailbox events render without Python product runtime", async () => {
		throw new Error("Need Rust cloud/intern client + unified journal normalization");
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
		await page.getByTestId("runtime-status").waitFor();
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

	test.fail("legacy Python runtime.sqlite3 migrates with stable IDs and sequences", async () => {
		throw new Error("Need migration receipt tests against fixture databases");
	});
});
