import { expect, test } from "./browser.fixture";
import type { VisualRecord } from "@synth/runtime-protocol";

const chartId = "vis_chart_pane_acceptance";
const chartSpec = JSON.stringify({
	version: 1,
	title: "Craftax rollout",
	panels: [{ kind: "metrics", items: [{ label: "mean reward", value: "2.05" }] }]
});

function svg(title: string): string {
	return Buffer.from(`<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360" role="img" aria-label="${title}"><rect width="100%" height="100%" fill="#fff"/><text x="24" y="48" font-size="20">${title}</text><path d="M40 300 L220 180 L400 230 L600 90" fill="none" stroke="#b94712" stroke-width="4"/></svg>`).toString("base64");
}

const record = (revision: number): VisualRecord => ({
	schemaVersion: "synth.desktop-visual.v1",
	id: chartId,
	currentRevision: revision,
	title: revision === 1 ? "Craftax rollout" : "Craftax rollout · revision 2",
	templateId: "analysis.chart.v1",
	status: "saved",
	rendererKind: "chart",
	bindings: { schemaVersion: "synth.visual-bindings.v1", slots: [] },
	sessionId: "chart-pane-session",
	messageId: "chart-pane-message",
	runId: "opt_eval_16cb3bdbc3b5",
	traceId: null,
	parentVisualId: null,
	sourceAgentId: "playwright",
	sourceModel: "fixture",
	contentDigest: "sha256:chart-pane-fixture",
	previewDigest: "sha256:chart-pane-rendition",
	metadata: { renderStatus: "ready", visualKind: "chart", rendererVersion: "workshop-charts-svg.1" },
	createdAt: "2026-08-19T00:00:00Z",
	updatedAt: `2026-08-19T00:00:0${revision}Z`
});

test("chart pane renders the host rendition, revises in place, and reflows without overflow", async ({ page }) => {
	await page.addInitScript(({ first, second, spec, firstSvg, secondSvg }) => {
		let revision = 1;
		const listeners: Array<(event: { kind: string }) => void> = [];
		const current = () => revision === 1 ? first : second;
		(window as any).synthVisuals = {
			listTemplates: async () => [], getTemplate: async () => ({ id: "analysis.chart.v1", title: "Chart" }),
			list: async () => [current()], get: async () => current(), revisions: async () => [],
			content: async () => ({ base64: btoa(spec), mediaType: "application/vnd.synth.chart-spec+json" }),
			rendition: async () => ({ base64: revision === 1 ? firstSvg : secondSvg, mediaType: "image/svg+xml", format: "svg", theme: "light", sizeClass: "pane" }),
			render: async () => current(), listSeals: async () => [], annotations: async () => [],
			onEvent: (listener: (event: { kind: string }) => void) => { listeners.push(listener); return () => undefined; }
		};
		(window as any).__advanceChartRevision = () => { revision = 2; listeners.forEach((listener) => listener({ kind: "visual.updated" })); };
	}, { first: record(1), second: record(2), spec: chartSpec, firstSvg: svg("Craftax rollout"), secondSvg: svg("Craftax rollout revision 2") });
	await page.reload();
	await page.getByTestId("open-visuals").click();
	const chart = page.getByTestId("visual-chart");
	await expect(chart).toBeVisible();
	await expect(chart.locator("img")).toHaveAttribute("src", /^data:image\/svg\+xml;base64,/);
	await expect(chart.getByRole("button", { name: "Spec", exact: true })).toBeVisible();
	await expect(chart.getByRole("button", { name: "Copy spec" })).toBeVisible();
	await expect(chart.getByRole("button", { name: "Export SVG" })).toBeVisible();
	await expect(chart.getByRole("button", { name: "Retry" })).toBeVisible();

	await page.evaluate(() => (window as any).__advanceChartRevision());
	await expect(chart.locator("img")).toHaveAttribute("alt", "Craftax rollout · revision 2");

	await page.setViewportSize({ width: 390, height: 844 });
	await expect(chart).toBeVisible();
	const geometry = await page.evaluate(() => ({ scroll: document.documentElement.scrollWidth, width: window.innerWidth }));
	expect(geometry.scroll).toBeLessThanOrEqual(geometry.width + 1);
});
