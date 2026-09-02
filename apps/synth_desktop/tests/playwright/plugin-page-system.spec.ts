import { expect, test } from "./browser.fixture";

test("plugin surfaces share the page header and tab system", async ({ page }) => {
	await page.getByTestId("titlebar").waitFor();
	const geometry: Array<{ name: string; pageX: number; headerY: number; backX: number; backY: number; titleX: number; titleY: number; tabsX: number; tabsY: number }> = [];
	for (const surface of [
		{ open: "open-visuals", page: "visuals-page", title: "Visuals" },
		{ open: "open-experiments", page: "experiments-workbench", title: "Experiments" },
		{ open: "open-inventory", page: "inventory-page", title: "Data" },
		{ open: "open-inference", page: "inference-page", title: "Inference" }
	] as const) {
		await page.getByTestId(surface.open).click();
		const pluginPage = page.getByTestId(surface.page);
		await expect(pluginPage).toHaveClass(/plugin-page/);
		await expect(pluginPage.locator(":scope > .plugin-page-head")).toContainText(surface.title);
		await expect(pluginPage.locator(":scope > .plugin-tabs")).toBeVisible();
		await expect(pluginPage.locator(":scope > .plugin-page-head .plugin-page-back")).toHaveText("← Back");

		const pageBox = await pluginPage.boundingBox();
		const headerBox = await pluginPage.locator(":scope > .plugin-page-head").boundingBox();
		const backBox = await pluginPage.locator(":scope > .plugin-page-head .plugin-page-back").boundingBox();
		const titleBox = await pluginPage.locator(":scope > .plugin-page-head .ws-title").boundingBox();
		const tabsBox = await pluginPage.locator(":scope > .plugin-tabs").boundingBox();
		expect(pageBox).not.toBeNull();
		expect(headerBox).not.toBeNull();
		expect(backBox).not.toBeNull();
		expect(titleBox).not.toBeNull();
		expect(tabsBox).not.toBeNull();
		geometry.push({
			name: surface.title,
			pageX: pageBox!.x,
			headerY: headerBox!.y,
			backX: backBox!.x,
			backY: backBox!.y,
			titleX: titleBox!.x,
			titleY: titleBox!.y,
			tabsX: tabsBox!.x,
			tabsY: tabsBox!.y
		});
	}

	const reference = geometry[0];
	for (const current of geometry.slice(1)) {
		for (const key of ["pageX", "headerY", "backX", "backY", "titleX", "titleY", "tabsX", "tabsY"] as const) {
			expect(Math.abs(current[key] - reference[key]), `${current.name} ${key} should align with ${reference.name}`).toBeLessThanOrEqual(1);
		}
	}
});

test("empty plugin registries use the shared empty-state treatment", async ({ page }) => {
	await page.getByTestId("titlebar").waitFor();
	await page.getByTestId("open-inventory").click();
	await expect(page.getByTestId("inventory-page").locator(".plugin-empty")).toContainText("No containers yet");
});

test("plugin Back returns to the recent chat instead of the previous plugin", async ({ page }) => {
	await page.addInitScript(() => {
		(window as typeof window & { synthCodex?: unknown }).synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId: "recent-chat",
				threadId: "recent-thread",
				workspace: "/workspaces/default",
				model: "poolside/Laguna-XS-2.1-NVFP4-mlx",
				providerName: "local-laguna",
				providerTitle: "Laguna XS Responses",
				baseUrl: "http://127.0.0.1:7333/v1",
				status: "ready",
				title: "Most recent conversation",
				approvalPolicy: "untrusted",
				sandbox: "workspace-write"
			}],
			start: async () => ({ sessionId: "recent-chat", threadId: "recent-thread" }),
			startTurn: async () => ({ sessionId: "recent-chat", threadId: "recent-thread", turnId: "turn-1" }),
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: () => () => undefined
		};
	});
	await page.reload();
	await page.getByTestId("local-chat-recent-chat").click();
	await expect(page.getByTestId("chat-transcript")).toBeVisible();

	await page.getByTestId("open-visuals").click();
	await page.getByTestId("open-inventory").click();
	await page.getByTestId("inventory-page").getByRole("button", { name: "← Back" }).click();

	await expect(page.getByTestId("chat-transcript")).toBeVisible();
	await expect(page.getByTestId("local-chat-recent-chat")).toHaveClass(/active/);
	await expect(page.getByTestId("visuals-page")).toHaveCount(0);
});
