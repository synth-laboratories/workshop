import { test, expect } from "./electron.fixture";
import type { Page } from "@playwright/test";

type Layout = {
	viewport: { width: number; height: number };
	composer: { top: number; right: number; bottom: number; left: number; width: number; height: number };
	input: { top: number; right: number; bottom: number; left: number; width: number; height: number };
	visual: { left: number } | null;
};

async function readLayout(page: Page): Promise<Layout> {
	return page.evaluate(() => {
		const composer = document.querySelector<HTMLElement>('[data-testid="composer"]');
		const input = document.querySelector<HTMLElement>('[data-testid="composer-input"]');
		if (!composer || !input) throw new Error("Composer invariant target is absent");
		const composerRect = composer.getBoundingClientRect();
		const inputRect = input.getBoundingClientRect();
		const visualRect = document
			.querySelector<HTMLElement>('[data-testid="visual-pane"]')
			?.getBoundingClientRect();
		const rect = (value: DOMRect) => ({
			top: value.top,
			right: value.right,
			bottom: value.bottom,
			left: value.left,
			width: value.width,
			height: value.height
		});
		return {
			viewport: { width: window.innerWidth, height: window.innerHeight },
			composer: rect(composerRect),
			input: rect(inputRect),
			visual: visualRect ? { left: visualRect.left } : null
		};
	});
}

function expectComposerInsideViewport(layout: Layout): void {
	expect(layout.composer.left).toBeGreaterThanOrEqual(0);
	expect(layout.composer.right).toBeLessThanOrEqual(layout.viewport.width);
	expect(layout.composer.top).toBeGreaterThanOrEqual(0);
	expect(layout.composer.bottom).toBeLessThanOrEqual(layout.viewport.height - 8);
	expect(layout.composer.width).toBeGreaterThan(320);
	expect(layout.composer.height).toBeGreaterThan(80);
	expect(layout.input.width).toBeGreaterThan(240);
	expect(layout.input.bottom).toBeLessThanOrEqual(layout.composer.bottom);
	if (layout.visual) {
		expect(layout.composer.right).toBeLessThanOrEqual(layout.visual.left + 1);
	}
}

test("bottom composer remains fully visible at supported window sizes", async ({ electronApp, page }) => {
	for (const [width, height] of [[1280, 840], [960, 640], [1440, 900]] as const) {
		await electronApp.evaluate(({ BrowserWindow }, size) => {
			BrowserWindow.getAllWindows()[0]?.setSize(size.width, size.height);
		}, { width, height });
		await expect(page.getByTestId("composer")).toBeVisible();
		expectComposerInsideViewport(await readLayout(page));
	}
});

test("composer stays visible and anchored while the transcript scrolls", async ({ page }) => {
	const session = await page.evaluate(async () => {
		const api = (window as typeof window & {
			__synthEval: { invoke(action: string, args?: Record<string, unknown>): Promise<unknown> };
		}).__synthEval;
		return api.invoke("create_session", { targetId: "local-laguna" }) as Promise<{ id: string }>;
	});

	await page.evaluate(async (sessionId) => {
		const api = (window as typeof window & {
			__synthEval: { invoke(action: string, args?: Record<string, unknown>): Promise<unknown> };
		}).__synthEval;
		await api.invoke("send_message", {
			sessionId,
			body: "Craftax Rust: inspect rollout and reward attribution with tool activity."
		});
	}, session.id);

	const transcript = page.getByTestId("chat-transcript");
	await expect(transcript).toBeVisible();
	await transcript.evaluate((element) => { element.scrollTop = 0; });
	const before = await readLayout(page);
	await transcript.evaluate((element) => { element.scrollTop = element.scrollHeight; });
	const after = await readLayout(page);

	expectComposerInsideViewport(after);
	expect(after.composer.bottom).toBeCloseTo(before.composer.bottom, 0);
	expect(after.composer.left).toBeCloseTo(before.composer.left, 0);
	await expect(page.getByTestId("composer-input")).toBeVisible();
	await expect(page.getByTestId("composer-input")).toHaveAttribute("aria-label", "Message composer");
	await expect(page.getByTestId("composer-send")).toBeVisible();
});

test("landing shell has no horizontal overflow", async ({ page }) => {
	const geometry = await page.evaluate(() => ({
		documentWidth: document.documentElement.scrollWidth,
		viewportWidth: window.innerWidth,
		documentHeight: document.documentElement.scrollHeight,
		viewportHeight: window.innerHeight
	}));
	expect(geometry.documentWidth).toBeLessThanOrEqual(geometry.viewportWidth);
	expect(geometry.documentHeight).toBeLessThanOrEqual(geometry.viewportHeight);
	await expect(page.getByTestId("sidebar")).toBeVisible();
	await expect(page.getByTestId("titlebar")).toBeVisible();
});
