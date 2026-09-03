/**
 * Composer steering, end to end in the renderer.
 *
 * The reported failure was a real-app one: a person types, presses Return,
 * *reads* "Next turns", then presses Return again — and by then the promotion
 * arm had already expired. These tests exercise that human-paced gesture, plus
 * the error hygiene the composer must keep when a steer is rejected.
 */
import { expect, test } from "./browser.fixture";
import type { Page } from "@playwright/test";

const SESSION_ID = "steering-session";
const SESSION_UUID = "7f3a1c92-1d4b-4e2a-9c7f-0b1d2e3f4a5b";

type SteerBehavior = "accept" | "reject-uuid" | "reject-object";

async function bootSteeringApp(page: Page, behavior: SteerBehavior) {
	await page.addInitScript(
		([sessionId, sessionUuid, mode]: [string, string, SteerBehavior]) => {
			const calls: Array<{ sessionId: string; text: string }> = [];
			const testWindow = window as typeof window & {
				__steerCalls?: typeof calls;
				synthCodex?: unknown;
				synthLaguna?: unknown;
			};
			testWindow.__steerCalls = calls;
			testWindow.synthLaguna = {
				getStatus: async () => ({
					phase: "ready",
					baseUrl: "http://127.0.0.1:7333",
					backend: "mlx_lm",
					loadedModel: "poolside/Laguna-XS-2.1-NVFP4-mlx",
					detail: "ready",
					memoryBytes: null,
					updatedAt: Date.now()
				}),
				onStatus: () => () => undefined,
				listModels: async () => [],
				chooseModelDirectory: async () => null,
				setModelDirectory: async () => undefined,
				clearModelDirectory: async () => undefined
			};
			testWindow.synthCodex = {
				defaultWorkspace: async () => "/workspaces/default",
				list: async () => [
					{
						sessionId,
						threadId: "thread-steering",
						workspace: "/workspaces/default",
						model: "poolside/Laguna-XS-2.1-NVFP4-mlx",
						providerName: "local-laguna",
						providerTitle: "Laguna XS",
						baseUrl: "http://127.0.0.1:7333",
						status: "running"
					}
				],
				start: async () => ({ sessionId, threadId: "thread-steering" }),
				startTurn: async () => ({ sessionId, threadId: "thread-steering", turnId: "turn-steering" }),
				steerTurn: async (id: string, text: string) => {
					calls.push({ sessionId: id, text });
					if (mode === "reject-uuid") {
						// Exactly what the Rust manager rejects with today.
						throw {
							code: "internal",
							message: `session ${sessionUuid} has no active turn to steer`
						};
					}
					if (mode === "reject-object") {
						// A rejection with no message at all: the shape that used to
						// reach JSX and render as [object Object].
						throw { transport: { body: { nested: { detail: {} } } } };
					}
				},
				interrupt: async () => undefined,
				close: async () => undefined,
				onEvent: (listener: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void) => {
					// A persisted `running` row is intentionally not enough to own a
					// live turn after crash recovery. Steering is available only after
					// the attached provider stream proves this process owns the turn.
					const timer = window.setTimeout(() => listener({
						sessionId,
						method: "turn/started",
						params: { turnId: "turn-steering" }
					}), 100);
					return () => window.clearTimeout(timer);
				}
			};
			window.localStorage.setItem(
				"synth.preferences.v1",
				JSON.stringify({ schemaVersion: 1, submission: { activeEnterAction: "enqueue" } })
			);
		},
		[SESSION_ID, SESSION_UUID, behavior] as [string, string, SteerBehavior]
	);
	await page.reload();
	await page.getByTestId(`local-chat-${SESSION_ID}`).click();
	const composer = page.getByTestId("composer-input");
	await expect(composer).toBeEnabled();
	await expect(page.getByRole("button", { name: "Stop generating" })).toBeVisible();
	return composer;
}

function steerCalls(page: Page) {
	return page.evaluate(
		() => (window as typeof window & { __steerCalls?: Array<{ sessionId: string; text: string }> }).__steerCalls ?? []
	);
}

test.describe("composer steering", () => {
	test("a human-paced double Return promotes without touching the queue row", async ({ page }) => {
		const composer = await bootSteeringApp(page, "accept");
		await composer.fill("prefer the smaller batch");
		await composer.press("Enter");

		// First Return queues; the prompt is visible under Next turns.
		await expect(page.getByTestId("prompt-queue")).toContainText("Next turns");
		await expect(page.getByTestId("prompt-queue")).toContainText("Return again to steer now");
		await expect(page.getByTestId("composer-steer-hint")).toHaveText("Queued — Return again to steer");
		expect(await steerCalls(page)).toEqual([]);

		// The pause a person actually takes to read the queue used to expire the
		// arm, which is what forced them into the queued row.
		await page.waitForTimeout(4_000);
		await expect(composer).toBeFocused();
		await composer.press("Enter");

		await expect(page.getByTestId("prompt-queue")).toBeHidden();
		await expect(page.getByTestId("composer-steer-hint")).toBeHidden();
		await expect(page.getByTestId("steer-error")).toHaveCount(0);
		expect(await steerCalls(page)).toEqual([
			{ sessionId: SESSION_ID, text: "prefer the smaller batch" }
		]);
	});

	test("a held Return delivers one steer, and Shift+Return never steers", async ({ page }) => {
		const composer = await bootSteeringApp(page, "accept");
		await composer.fill("hold this thought");
		await composer.press("Enter");
		await expect(page.getByTestId("prompt-queue")).toBeVisible();

		// Shift+Return is a newline in the composer, never a promotion.
		await composer.press("Shift+Enter");
		expect(await steerCalls(page)).toEqual([]);
		await composer.fill("");

		await page.keyboard.down("Enter");
		await page.waitForTimeout(600);
		await page.keyboard.up("Enter");

		await expect(page.getByTestId("prompt-queue")).toBeHidden();
		expect(await steerCalls(page)).toEqual([{ sessionId: SESSION_ID, text: "hold this thought" }]);
	});

	test("a rejected steer keeps the prompt and never shows the session id", async ({ page }) => {
		const composer = await bootSteeringApp(page, "reject-uuid");
		await composer.fill("this one will be rejected");
		await composer.press("Enter");
		await expect(page.getByTestId("prompt-queue")).toBeVisible();
		await composer.press("Enter");

		const error = page.getByTestId("steer-error");
		await expect(error).toBeVisible();
		await expect(error).toHaveAttribute("data-steer-error-code", "steer_turn_finished");
		const text = (await error.textContent()) ?? "";
		expect(text).not.toContain(SESSION_UUID);
		expect(text).not.toContain("[object Object]");
		expect(text).toContain("stays queued");

		// The prompt is recoverable: it was never acknowledged, so it is still
		// in Next turns and still sends as the next turn.
		await expect(page.getByTestId("prompt-queue")).toBeVisible();
		await expect(page.locator('[data-testid^="queued-prompt-"] input')).toHaveValue(
			"this one will be rejected"
		);
		// And nothing anywhere on the page leaked the internal id.
		expect(await page.locator("body").innerText()).not.toContain(SESSION_UUID);
	});

	test("a structured object rejection never renders [object Object]", async ({ page }) => {
		const composer = await bootSteeringApp(page, "reject-object");
		await composer.fill("object rejection");
		await composer.press("Enter");
		await expect(page.getByTestId("prompt-queue")).toBeVisible();
		await composer.press("Enter");

		const error = page.getByTestId("steer-error");
		await expect(error).toBeVisible();
		await expect(error).toHaveAttribute("data-steer-error-code", "steer_unavailable");
		expect(await error.textContent()).not.toContain("[object Object]");
		expect(await page.locator("body").innerText()).not.toContain("[object Object]");
	});

	test("opening Advanced while a prompt is armed does not block the composer", async ({ page }) => {
		const composer = await bootSteeringApp(page, "accept");
		await composer.fill("keep typing while advanced opens");
		await composer.press("Enter");
		await expect(page.getByTestId("prompt-queue")).toBeVisible();

		const advanced = page.getByRole("button", { name: "Open advanced trace" });
		if (await advanced.count()) await advanced.first().click();

		// The composer stays interactive and the arm survives the panel change.
		await expect(composer).toBeEnabled();
		await composer.focus();
		await composer.press("Enter");
		await expect(page.getByTestId("prompt-queue")).toBeHidden();
		expect(await steerCalls(page)).toEqual([
			{ sessionId: SESSION_ID, text: "keep typing while advanced opens" }
		]);
	});
});
