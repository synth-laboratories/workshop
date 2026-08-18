import { expect, test } from "./browser.fixture";

test("a local session process exit clears stale Working and Stop state", async ({ page }) => {
	await page.addInitScript(() => {
		type Event = { sessionId: string; method: string; params: Record<string, unknown> };
		let listener: ((event: Event) => void) | undefined;
		(window as typeof window & { __emitSessionHealth?: (event: Event) => void }).__emitSessionHealth = (event) => listener?.(event);
		(window as typeof window & { synthLaguna?: unknown }).synthLaguna = {
			getStatus: async () => ({ phase: "ready", baseUrl: "http://127.0.0.1:7333", backend: "mlx_lm", loadedModel: "poolside/Laguna-XS-2.1-NVFP4-mlx", detail: "Laguna XS ready", memoryBytes: null, updatedAt: Date.now() }),
			onStatus: () => () => undefined,
			listModels: async () => []
		};
		(window as typeof window & { synthCodex?: unknown }).synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId: "detached-laguna", threadId: "laguna-thread", workspace: "/workspaces/default",
				model: "poolside/Laguna-XS-2.1-NVFP4-mlx", providerName: "local-laguna",
				providerTitle: "Laguna XS Responses", baseUrl: "http://127.0.0.1:7333/v1", status: "running"
			}],
			start: async () => ({ sessionId: "detached-laguna", threadId: "laguna-thread" }),
			startTurn: async () => ({ sessionId: "detached-laguna", threadId: "laguna-thread", turnId: "turn-1" }),
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: (next: (event: Event) => void) => { listener = next; return () => { listener = undefined; }; }
		};
	});
	await page.reload();
	await page.getByTestId("local-chat-detached-laguna").click();
	await page.evaluate(() => {
		(window as typeof window & { __emitSessionHealth: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void })
			.__emitSessionHealth({
				sessionId: "detached-laguna",
				method: "turn/started",
				params: { turnId: "turn-1" }
			});
	});
	await expect(page.getByTestId("model-working")).toBeVisible();

	await page.evaluate(() => {
		(window as typeof window & { __emitSessionHealth: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void })
			.__emitSessionHealth({
				sessionId: "detached-laguna",
				method: "session/unhealthy",
				params: { reason: "app_server_exited", message: "The local agent process exited before the turn completed." }
			});
	});

	await expect(page.getByTestId("model-working")).toHaveCount(0);
	await expect(page.getByRole("button", { name: "Stop generating" })).toHaveCount(0);
	await expect(page.getByTestId("chat-working-detached-laguna")).toHaveCount(0);
	// Scoped to the visible run summary: the transcript also mirrors this text
	// into an off-screen aria-live region.
	await expect(page.getByTestId("chat-transcript").locator(".run-summary"))
		.toContainText("Stopped because the local agent disconnected · send a message to reconnect");
});

test("a replayed terminal event overrides a stale running session record", async ({ page }) => {
	await page.addInitScript(() => {
		const sessionId = "stale-provider-failure";
		(window as typeof window & { synthCodex?: unknown }).synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId, threadId: "stale-provider-thread", workspace: "/workspaces/default",
				model: "openai/gpt-5.6-luna", providerName: "openrouter",
				providerTitle: "OpenRouter Responses", baseUrl: "https://openrouter.ai/api/v1",
				status: "running"
			}],
			start: async () => ({ sessionId, threadId: "stale-provider-thread" }),
			startTurn: async () => ({ sessionId, threadId: "stale-provider-thread", turnId: "turn-stale" }),
			interrupt: async () => undefined, close: async () => undefined,
			onEvent: () => () => undefined
		};
		const rows = [
			{ sequence: 1, sessionSequence: 1, kind: "message.created", payload: { messageId: "user-stale", role: "user", content: "hello" } },
			{ sequence: 2, sessionSequence: 2, kind: "run.started", payload: { runId: "turn-stale" } },
			{ sequence: 3, sessionSequence: 3, kind: "run.failed", payload: { runId: "turn-stale", message: "Missing provider credential" } }
		].map((row) => ({
			schemaVersion: "synth.desktop-app-event.v1" as const, eventId: `evt-${row.sequence}`,
			sessionId, source: "codex" as const, createdAt: "2026-08-10T20:00:00Z", ...row
		}));
		(window as typeof window & { synthCore?: unknown }).synthCore = {
			diagnostics: async () => ({ databasePath: "/tmp/core.sqlite3", schemaVersion: 1, integrityOk: true,
				contentStorePath: "/tmp/content", journalHead: 3, sessionCount: 1, runCount: 1, visualCount: 0, migrationComplete: true }),
			eventsAfter: async () => rows, sessionEventsAfter: async () => rows, onEvent: () => () => undefined
		};
	});
	await page.reload();
	await page.getByTestId("local-chat-stale-provider-failure").click();
	await expect(page.getByTestId("chat-transcript")).toContainText("Stopped with an error after");
	await expect(page.getByTestId("model-working")).toHaveCount(0);
	await expect(page.getByRole("button", { name: "Stop generating" })).toHaveCount(0);
});

test("a failed turn hidden inside a completed envelope never renders as blank success", async ({ page }) => {
	await page.addInitScript(() => {
		type Event = { sessionId: string; method: string; params: Record<string, unknown> };
		let listener: ((event: Event) => void) | undefined;
		(window as typeof window & { __emitTerminalEvent?: (event: Event) => void }).__emitTerminalEvent = (event) => listener?.(event);
		(window as typeof window & { synthLaguna?: unknown }).synthLaguna = {
			getStatus: async () => ({ phase: "ready", baseUrl: "http://127.0.0.1:7333", backend: "mlx_lm", loadedModel: "poolside/Laguna-XS-2.1-NVFP4-mlx", detail: "Laguna XS ready", memoryBytes: null, updatedAt: Date.now() }),
			onStatus: () => () => undefined,
			listModels: async () => []
		};
		(window as typeof window & { synthCodex?: unknown }).synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId: "terminal-envelope", threadId: "terminal-thread", workspace: "/workspaces/default",
				model: "poolside/Laguna-XS-2.1-NVFP4-mlx", providerName: "local-laguna",
				providerTitle: "Laguna XS Responses", baseUrl: "http://127.0.0.1:7333/v1", status: "ready"
			}],
			start: async () => ({ sessionId: "terminal-envelope", threadId: "terminal-thread" }),
			startTurn: async () => ({ sessionId: "terminal-envelope", threadId: "terminal-thread", turnId: "turn-1" }),
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: (next: (event: Event) => void) => { listener = next; return () => { listener = undefined; }; }
		};
	});
	await page.reload();
	await page.getByTestId("local-chat-terminal-envelope").click();
	await page.evaluate(() => {
		const emit = (window as typeof window & { __emitTerminalEvent: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void }).__emitTerminalEvent;
		emit({ sessionId: "terminal-envelope", method: "turn/started", params: {} });
		emit({ sessionId: "terminal-envelope", method: "turn/completed", params: {
			turn: { status: "failed", error: { message: "The provider disconnected." } }
		} });
	});

	const transcript = page.getByTestId("chat-transcript");
	await expect(transcript).toContainText("The provider could not produce a response: The provider disconnected. Try again.");
	await expect(transcript).not.toContainText("Worked");
});

// A restored record may still claim `running`, but without a live ownership
// receipt it must not show Working or Stop. The next send reconnects and its
// rejection remains actionable.
test("a rejected turn start clears Working, keeps the typed text and retries", async ({ page }) => {
	await page.addInitScript(() => {
		type Event = { sessionId: string; method: string; params: Record<string, unknown> };
		const testWindow = window as typeof window & {
			__codexSendTurnCalls?: { prompt: string; clientMessageId?: string }[];
			__codexAllowSend?: boolean;
			synthLaguna?: unknown;
			synthCodex?: unknown;
		};
		testWindow.__codexSendTurnCalls = [];
		testWindow.__codexAllowSend = false;
		testWindow.synthLaguna = {
			getStatus: async () => ({ phase: "ready", baseUrl: "http://127.0.0.1:7333", backend: "mlx_lm", loadedModel: "poolside/Laguna-XS-2.1-NVFP4-mlx", detail: "Laguna XS ready", memoryBytes: null, updatedAt: Date.now() }),
			onStatus: () => () => undefined,
			listModels: async () => []
		};
		testWindow.synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId: "922c25f7-0000-4000-8000-000000000001", threadId: "laguna-thread",
				workspace: "/workspaces/default", model: "poolside/Laguna-XS-2.1-NVFP4-mlx",
				providerName: "local-laguna", providerTitle: "Laguna XS Responses",
				baseUrl: "http://127.0.0.1:7333/v1", status: "running",
				approvalPolicy: "untrusted", sandbox: "workspace-write"
			}],
			start: async () => ({ sessionId: "922c25f7-0000-4000-8000-000000000001", threadId: "laguna-thread" }),
			startTurn: async () => { throw new Error("startTurn must not be used once sendTurn exists"); },
			sendTurn: async (
				_request: unknown,
				prompt: string,
				_effort?: string,
				options?: { clientMessageId?: string }
			) => {
				testWindow.__codexSendTurnCalls!.push({
					prompt,
					clientMessageId: options?.clientMessageId
				});
				if (!testWindow.__codexAllowSend) {
					// Shape of the typed `codex_turn_send` rejection.
					throw {
						code: "codex_session_detached",
						message: "The local agent process disconnected before the turn started. Retry to reconnect.",
						sessionId: "922c25f7-0000-4000-8000-000000000001",
						detail: "SessionDetached"
					};
				}
				return { sessionId: "922c25f7-0000-4000-8000-000000000001", threadId: "laguna-thread", turnId: "turn-2" };
			},
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: () => () => undefined
		};
	});
	await page.reload();
	await page.getByTestId("local-chat-922c25f7-0000-4000-8000-000000000001").click();

	// The stale persisted status cannot resurrect controls for a dead worker.
	await expect(page.getByTestId("model-working")).toHaveCount(0);
	await expect(page.getByRole("button", { name: "Stop generating" })).toHaveCount(0);

	await page.getByTestId("composer-input").fill("summarize the lifecycle handoff");
	await page.getByTestId("composer-send").click();

	// Working and Stop are gone, and the composer is usable again.
	await expect(page.getByTestId("model-working")).toHaveCount(0);
	await expect(page.getByRole("button", { name: "Stop generating" })).toHaveCount(0);
	await expect(page.getByTestId("composer-input")).toBeEnabled();

	// The failure is explained without a raw session UUID.
	const retry = page.getByTestId("send-retry");
	await expect(retry).toBeVisible();
	await expect(retry).toContainText("The local agent process disconnected before the turn started. Retry to reconnect.");
	await expect(retry).not.toContainText("922c25f7");
	const retryGeometry = await retry.boundingBox();
	const composerGeometry = await page.getByTestId("composer").boundingBox();
	expect(retryGeometry, "retry status has measurable geometry").not.toBeNull();
	expect(composerGeometry, "composer has measurable geometry").not.toBeNull();
	const retryBottom = retryGeometry!.y + retryGeometry!.height;
	const retryRight = retryGeometry!.x + retryGeometry!.width;
	const composerRight = composerGeometry!.x + composerGeometry!.width;
	expect(retryBottom, "retry status is above the composer").toBeLessThanOrEqual(composerGeometry!.y);
	expect(retryGeometry!.x).toBeGreaterThanOrEqual(composerGeometry!.x);
	expect(retryRight).toBeLessThanOrEqual(composerRight);
	expect(retryBottom).toBeLessThanOrEqual((await page.viewportSize())!.height);

	// The typed text survived and is still in the transcript.
	await expect(page.getByTestId("chat-transcript")).toContainText("summarize the lifecycle handoff");

	await page.evaluate(() => {
		(window as typeof window & { __codexAllowSend?: boolean }).__codexAllowSend = true;
	});
	await page.getByTestId("send-retry-button").click();

	await expect(page.getByTestId("send-retry")).toHaveCount(0);
	await expect(page.getByTestId("model-working")).toBeVisible();
	// The same prompt was resent, and only once per attempt.
	const calls = await page.evaluate(() => (window as typeof window & { __codexSendTurnCalls?: { prompt: string; clientMessageId?: string }[] }).__codexSendTurnCalls ?? []);
	expect(calls.map((call) => call.prompt)).toEqual([
		"summarize the lifecycle handoff",
		"summarize the lifecycle handoff"
	]);
	// Optimistic id must be forwarded so Rust can journal the same bubble.
	expect(calls[0]?.clientMessageId).toMatch(/^user-/);
	expect(calls[1]?.clientMessageId).toBe(calls[0]?.clientMessageId);
	// The retry reuses the original message id, so the text is not duplicated.
	const transcript = await page.getByTestId("chat-transcript").innerText();
	expect(transcript.split("summarize the lifecycle handoff").length - 1).toBe(1);
});

test("a host-backed user event collapses onto the optimistic submission", async ({ page }) => {
	await page.addInitScript(() => {
		type Event = { sessionId: string; method: string; params: Record<string, unknown> };
		let listener: ((event: Event) => void) | undefined;
		const sessionId = "single-user-message";
		(window as typeof window & { synthLaguna?: unknown }).synthLaguna = {
			getStatus: async () => ({ phase: "ready", baseUrl: "http://127.0.0.1:7333", backend: "mlx_lm", loadedModel: "poolside/Laguna-XS-2.1-NVFP4-mlx", detail: "Laguna XS ready", memoryBytes: null, updatedAt: Date.now() }),
			onStatus: () => () => undefined,
			listModels: async () => []
		};
		(window as typeof window & { synthCodex?: unknown }).synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId, threadId: "single-message-thread", workspace: "/workspaces/default",
				model: "poolside/Laguna-XS-2.1-NVFP4-mlx", providerName: "local-laguna",
				providerTitle: "Laguna XS Responses", baseUrl: "http://127.0.0.1:7333/v1",
				status: "ready", approvalPolicy: "untrusted", sandbox: "workspace-write"
			}],
			start: async () => ({ sessionId, threadId: "single-message-thread" }),
			startTurn: async () => { throw new Error("sendTurn owns this submission"); },
			sendTurn: async (
				_request: unknown,
				prompt: string,
				_effort: string | undefined,
				options: { clientMessageId?: string } | undefined
			) => {
				listener?.({
					sessionId,
					method: "message.created",
					params: { messageId: options?.clientMessageId, role: "user", content: prompt }
				});
				return { sessionId, threadId: "single-message-thread", turnId: "turn-single-message" };
			},
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: (next: (event: Event) => void) => { listener = next; return () => { listener = undefined; }; }
		};
	});
	await page.reload();
	await page.getByTestId("local-chat-single-user-message").click();
	await page.getByTestId("composer-input").fill("render this submission once");
	await page.getByTestId("composer-send").click();

	const transcript = page.getByTestId("chat-transcript");
	await expect(transcript.getByText("render this submission once", { exact: true })).toHaveCount(1);
	await expect(transcript.locator('[data-testid^="user-message-user-"]')).toHaveCount(1);
});
