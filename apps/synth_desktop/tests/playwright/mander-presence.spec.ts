import { expect, test, type Page } from "./browser.fixture";

async function openSettings(page: Page) {
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-menu-settings").click();
}

test.describe("optional chat mascot", () => {
	test("defaults off in settings and is absent from a chat", async ({ page }) => {
		await page.addInitScript(() => {
			(window as typeof window & { synthCodex?: unknown }).synthCodex = {
				defaultWorkspace: async () => "/workspaces/default",
				list: async () => [{
					sessionId: "mascot-chat",
					threadId: "mascot-thread",
					workspace: "/workspaces/default",
					model: "poolside/Laguna-XS-2.1-NVFP4-mlx",
					providerName: "local-laguna",
					providerTitle: "Laguna XS Responses",
					baseUrl: "http://127.0.0.1:7333/v1",
					status: "ready",
					title: "Inspect Craftax rollouts",
					approvalPolicy: "untrusted",
					sandbox: "workspace-write"
				}],
				start: async () => ({ sessionId: "mascot-chat", threadId: "mascot-thread" }),
				startTurn: async () => ({ sessionId: "mascot-chat", threadId: "mascot-thread", turnId: "turn-1" }),
				interrupt: async () => undefined,
				close: async () => undefined,
				onEvent: () => () => undefined
			};
		});
		await page.reload();
		await openSettings(page);
		await expect(page.getByTestId("show-mascot-off")).toHaveAttribute("aria-checked", "true");
		await page.locator(".settings-back").click();
		await page.getByTestId("local-chat-mascot-chat").click();
		await expect(page.getByTestId("chat-transcript")).toBeVisible();
		await expect(page.getByTestId("mander-presence")).toHaveCount(0);
	});

	test("enabling the preference shows overlay emotion and summary", async ({ page }) => {
		await page.addInitScript(() => {
			window.localStorage.setItem("synth.preferences.v1", JSON.stringify({
				schemaVersion: 5,
				appearance: { showMascot: true }
			}));
			(window as typeof window & { synthCodex?: unknown }).synthCodex = {
				defaultWorkspace: async () => "/workspaces/default",
				list: async () => [{
					sessionId: "mascot-on",
					threadId: "mascot-thread",
					workspace: "/workspaces/default",
					model: "poolside/Laguna-XS-2.1-NVFP4-mlx",
					providerName: "local-laguna",
					providerTitle: "Laguna XS Responses",
					baseUrl: "http://127.0.0.1:7333/v1",
					status: "ready",
					title: "Inspect Craftax rollouts",
					presentationEmotion: "success",
					presentationSummary: "Reward curve flattened",
					approvalPolicy: "untrusted",
					sandbox: "workspace-write"
				}],
				start: async () => ({ sessionId: "mascot-on", threadId: "mascot-thread" }),
				startTurn: async () => ({ sessionId: "mascot-on", threadId: "mascot-thread", turnId: "turn-1" }),
				interrupt: async () => undefined,
				close: async () => undefined,
				onEvent: () => () => undefined
			};
		});
		await page.reload();
		await page.getByTestId("local-chat-mascot-on").click();
		await expect(page.getByTestId("mander-presence")).toBeVisible();
		await expect(page.getByTestId("mander-presence")).toHaveAttribute("data-mander-emotion", "success");
		await expect(page.getByTestId("mander")).toHaveAttribute("data-mander-state", "success");
		await expect(page.getByTestId("mander-presence-summary")).toHaveText("Reward curve flattened");
	});

	test("composer toolbar toggle shows the mascot on landing", async ({ page }) => {
		await expect(page.getByTestId("landing-page")).toBeVisible();
		await expect(page.getByTestId("composer-mascot")).toHaveAttribute("aria-pressed", "false");
		await expect(page.getByTestId("mander-presence")).toHaveCount(0);
		await page.getByTestId("composer-mascot").click();
		await expect(page.getByTestId("composer-mascot")).toHaveAttribute("aria-pressed", "true");
		await expect(page.getByTestId("mander-presence")).toBeVisible();
		await expect(page.getByTestId("mander")).toHaveAttribute("data-mander-state", "idle");
		const composerBox = await page.getByTestId("composer").boundingBox();
		const mascotBox = await page.getByTestId("mander-presence").boundingBox();
		expect(composerBox && mascotBox).toBeTruthy();
		expect(mascotBox!.x).toBeGreaterThan(composerBox!.x + composerBox!.width - 12);
		expect(mascotBox!.y).toBeLessThan(composerBox!.y + composerBox!.height / 3);
		const stored = await page.evaluate(() => window.localStorage.getItem("synth.preferences.v1"));
		expect(JSON.parse(stored!).appearance.showMascot).toBe(true);
	});

	test("composer toolbar toggle shows the mascot in the active chat", async ({ page }) => {
		await page.addInitScript(() => {
			(window as typeof window & { synthCodex?: unknown }).synthCodex = {
				defaultWorkspace: async () => "/workspaces/default",
				list: async () => [{
					sessionId: "mascot-toggle",
					threadId: "mascot-thread",
					workspace: "/workspaces/default",
					model: "poolside/Laguna-XS-2.1-NVFP4-mlx",
					providerName: "local-laguna",
					providerTitle: "Laguna XS Responses",
					baseUrl: "http://127.0.0.1:7333/v1",
					status: "ready",
					title: "Inspect Craftax rollouts",
					presentationEmotion: "thinking",
					presentationSummary: "Reading reward traces",
					approvalPolicy: "untrusted",
					sandbox: "workspace-write"
				}],
				start: async () => ({ sessionId: "mascot-toggle", threadId: "mascot-thread" }),
				startTurn: async () => ({ sessionId: "mascot-toggle", threadId: "mascot-thread", turnId: "turn-1" }),
				interrupt: async () => undefined,
				close: async () => undefined,
				onEvent: () => () => undefined
			};
		});
		await page.reload();
		await page.getByTestId("local-chat-mascot-toggle").click();
		await expect(page.getByTestId("composer-mascot")).toHaveAttribute("aria-pressed", "false");
		await expect(page.getByTestId("mander-presence")).toHaveCount(0);
		await page.getByTestId("composer-mascot").click();
		await expect(page.getByTestId("composer-mascot")).toHaveAttribute("aria-pressed", "true");
		await expect(page.getByTestId("mander-presence")).toBeVisible();
		await expect(page.getByTestId("mander-presence-summary")).toHaveText("Reading reward traces");
		const stored = await page.evaluate(() => window.localStorage.getItem("synth.preferences.v1"));
		expect(JSON.parse(stored!).appearance.showMascot).toBe(true);
	});

	test("mascot on persists through settings", async ({ page }) => {
		await openSettings(page);
		await page.getByTestId("show-mascot-on").click();
		await expect(page.getByTestId("show-mascot-on")).toHaveAttribute("aria-checked", "true");
		const stored = await page.evaluate(() => window.localStorage.getItem("synth.preferences.v1"));
		expect(JSON.parse(stored!).appearance.showMascot).toBe(true);
	});
});
