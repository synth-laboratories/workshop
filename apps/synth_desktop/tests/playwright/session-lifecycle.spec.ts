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
	await expect(page.getByText("Stopped because the local agent disconnected · send a message to reconnect")).toBeVisible();
});
