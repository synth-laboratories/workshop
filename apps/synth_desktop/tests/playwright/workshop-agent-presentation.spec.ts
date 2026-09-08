import { expect, test } from "./browser.fixture";

test("external presentation opens the requested shared visual and preserves the latest request", async ({ page }) => {
	await page.addInitScript(() => {
		const record = (id: string) => ({
			schemaVersion: "synth.desktop-visual.v1", id, currentRevision: 1,
			title: `Shared ${id}`, templateId: "analysis.chart.v1", status: "saved",
			rendererKind: "chart", bindings: { schemaVersion: "synth.visual-bindings.v1", slots: [] },
			sessionId: null, messageId: null, runId: null, traceId: null, parentVisualId: null,
			sourceAgentId: "workshop-mcp", sourceModel: null, contentDigest: null, previewDigest: null,
			metadata: {}, createdAt: "2026-09-07T00:00:00Z", updatedAt: "2026-09-07T00:00:00Z"
		});
		(window as any).synthVisuals = {
			list: async () => [record("first"), record("second")],
			get: async (id: string) => {
				if (id === "missing") throw new Error("Visual not found");
				if (id === "first") await new Promise((resolve) => setTimeout(resolve, 300));
				return record(id);
			},
			listTemplates: async () => [], getTemplate: async () => ({ id: "analysis.chart.v1" }),
			listSeals: async () => [], annotations: async () => [], revisions: async () => [],
			content: async () => ({ base64: btoa('{"version":1,"panels":[]}'), mediaType: "application/json" }),
			rendition: async () => ({ base64: btoa('<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360"/>'), mediaType: "image/svg+xml", format: "svg" }),
			onEvent: () => () => undefined
		};
	});
	await page.reload();
	await page.evaluate(() => {
		const present = (visualId: string) => {
			const detail = { visualId, requestId: crypto.randomUUID() };
			(window as any).__workshopVisualPresentation = detail;
			window.dispatchEvent(new CustomEvent("workshop:visual-present", { detail }));
		};
		present("first");
		present("second");
	});
	await expect(page.getByTestId("visuals-page")).toBeVisible();
	await expect(page.getByRole("heading", { name: "Shared second", exact: true })).toBeVisible();
	await expect(page.getByRole("button", { name: "Show library", exact: true })).toBeVisible();
	await page.waitForTimeout(400);
	await expect(page.getByRole("heading", { name: "Shared second", exact: true })).toBeVisible();
	await page.getByRole("button", { name: "Show library", exact: true }).click();
	await page.getByRole("button", { name: "← Back", exact: true }).click();
	await expect(page.getByTestId("visuals-page")).not.toBeVisible();
	await page.getByTestId("open-visuals").click();
	await expect(page.getByTestId("visuals-page")).toBeVisible();
	await expect(page.getByRole("button", { name: "Show library", exact: true })).not.toBeVisible();
	await page.evaluate(() => window.dispatchEvent(new CustomEvent("workshop:visual-present", {
		detail: { visualId: "missing", requestId: crypto.randomUUID() }
	})));
	await expect(page.getByText("Visual not found", { exact: true })).toBeVisible();
});
