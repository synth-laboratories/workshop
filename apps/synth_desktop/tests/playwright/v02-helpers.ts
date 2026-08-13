import type { Page } from "@playwright/test";
import type { VisualRecord } from "@synth/runtime-protocol";

export function liveVisual(overrides: Partial<VisualRecord> & Pick<VisualRecord, "id" | "templateId" | "title">): VisualRecord {
	return {
		schemaVersion: "synth.desktop-visual.v1",
		currentRevision: 1,
		status: "saved",
		rendererKind: "template",
		bindings: { schemaVersion: "synth.visual-bindings.v1", slots: [] },
		sessionId: null,
		messageId: null,
		runId: null,
		traceId: null,
		parentVisualId: null,
		sourceAgentId: "v02-gate",
		sourceModel: "v02-gate",
		contentDigest: null,
		previewDigest: null,
		metadata: {},
		createdAt: "2026-08-13T13:00:00Z",
		updatedAt: "2026-08-13T13:00:00Z",
		...overrides
	};
}

export function streamBinding(events: unknown[], extra: Record<string, unknown> = {}) {
	return {
		schemaVersion: "synth.visual-bindings.v1" as const,
		slots: [{
			slot: "stream",
			kind: "inline" as const,
			data: { events, ...extra }
		}]
	};
}

export async function installVisuals(page: Page, visuals: VisualRecord[]): Promise<void> {
	await page.addInitScript((rows) => {
		const store = [...rows] as VisualRecord[];
		(window as typeof window & { synthVisuals?: unknown }).synthVisuals = {
			listTemplates: async () => rows.map((row) => ({ id: row.templateId, title: row.title, genre: "live" })),
			getTemplate: async (templateId: string) => ({ id: templateId, title: templateId }),
			list: async () => store,
			get: async (visualId: string) => {
				const hit = store.find((row) => row.id === visualId);
				if (!hit) throw new Error(`missing visual ${visualId}`);
				return hit;
			},
			revisions: async () => [],
			create: async () => store[0],
			update: async () => store[0],
			save: async () => store[0],
			fork: async () => store[0],
			archive: async () => store[0],
			show: async (visualId: string) => {
				const hit = store.find((row) => row.id === visualId);
				if (!hit) throw new Error(`missing visual ${visualId}`);
				return hit;
			},
			onEvent: () => () => undefined,
			onShow: () => () => undefined
		};
	}, visuals);
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
}

export async function openVisual(page: Page, visualId: string) {
	await page.getByTestId("open-visuals").click();
	await page.getByTestId(`visuals-card-${visualId}`).getByRole("button", { name: "Open" }).click();
	return page.getByTestId("visual-pane");
}

export async function metricValue(pane: ReturnType<Page["getByTestId"]>, label: string): Promise<string> {
	return pane.evaluate((root, wanted) => {
		const labels: string[] = [];
		for (const metric of root.querySelectorAll(".sv-metric")) {
			const text = metric.querySelector("span")?.textContent?.trim() ?? "";
			labels.push(text);
			if (text.toLowerCase() === wanted.toLowerCase()) {
				return metric.querySelector("strong")?.textContent?.trim() ?? "";
			}
		}
		throw new Error(`metric "${wanted}" not found; saw ${labels.join(", ") || "(none)"}`);
	}, label);
}
