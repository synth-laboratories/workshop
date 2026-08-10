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

// The exact screenshot state: a restored record still claims `running`, so the
// transcript shows Working with a live Stop, and the very next send is rejected
// because the owning app-server is already gone.
test("a rejected turn start clears Working, keeps the typed text and retries", async ({ page }) => {
	await page.addInitScript(() => {
		type Event = { sessionId: string; method: string; params: Record<string, unknown> };
		const testWindow = window as typeof window & {
			__codexSendTurnCalls?: { prompt: string }[];
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
			sendTurn: async (_request: unknown, prompt: string) => {
				testWindow.__codexSendTurnCalls!.push({ prompt });
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

	// Broken state as screenshotted.
	await expect(page.getByTestId("model-working")).toBeVisible();
	await expect(page.getByRole("button", { name: "Stop generating" })).toBeVisible();
	// Active-turn input now honestly enqueues. Stop the stale restored run before
	// exercising the rejected fresh turn path.
	await page.getByRole("button", { name: "Stop generating" }).click();
	await expect(page.getByTestId("model-working")).toHaveCount(0);

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

	// The typed text survived and is still in the transcript.
	await expect(page.getByTestId("chat-transcript")).toContainText("summarize the lifecycle handoff");

	await page.evaluate(() => {
		(window as typeof window & { __codexAllowSend?: boolean }).__codexAllowSend = true;
	});
	await page.getByTestId("send-retry-button").click();

	await expect(page.getByTestId("send-retry")).toHaveCount(0);
	await expect(page.getByTestId("model-working")).toBeVisible();
	// The same prompt was resent, and only once per attempt.
	const calls = await page.evaluate(() => (window as typeof window & { __codexSendTurnCalls?: { prompt: string }[] }).__codexSendTurnCalls ?? []);
	expect(calls.map((call) => call.prompt)).toEqual([
		"summarize the lifecycle handoff",
		"summarize the lifecycle handoff"
	]);
	// The retry reuses the original message id, so the text is not duplicated.
	const transcript = await page.getByTestId("chat-transcript").innerText();
	expect(transcript.split("summarize the lifecycle handoff").length - 1).toBe(1);
});
