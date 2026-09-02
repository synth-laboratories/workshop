import { createRequire } from "node:module";
import { test, expect } from "./browser.fixture";
import type { Page } from "@playwright/test";

const require = createRequire(import.meta.url);
const desktopPackage = require("../../package.json") as { version: string };

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

test("bottom composer remains fully visible at supported window sizes", async ({ page }) => {
	for (const [width, height] of [[1280, 840], [960, 640], [1440, 900]] as const) {
		await page.setViewportSize({ width, height });
		await expect(page.getByTestId("composer")).toBeVisible();
		expectComposerInsideViewport(await readLayout(page));
	}
});

test("composer stays visible and anchored while its containing surface scrolls", async ({ page }) => {
	const transcript = page.getByTestId("landing-page");
	await expect(transcript).toBeVisible();
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

test("sidebar account and settings footer stays anchored to the bottom edge", async ({ page }) => {
	for (const [width, height] of [[1280, 840], [960, 640], [1440, 900]] as const) {
		await page.setViewportSize({ width, height });
		const geometry = await page.evaluate(() => {
			const sidebar = document.querySelector<HTMLElement>('[data-testid="sidebar"]')?.getBoundingClientRect();
			const footer = document.querySelector<HTMLElement>(".sidebar-footer")?.getBoundingClientRect();
			if (!sidebar || !footer) throw new Error("Sidebar footer invariant target is absent");
			return {
				bottomInset: sidebar.bottom - footer.bottom,
				footerTop: footer.top,
				sidebarTop: sidebar.top
			};
		});
		expect(geometry.bottomInset).toBeGreaterThanOrEqual(8);
		expect(geometry.bottomInset).toBeLessThanOrEqual(12);
		expect(geometry.footerTop).toBeGreaterThan(geometry.sidebarTop);
	}
});

test("sidebar seam is one hairline with an invisible resize hit target", async ({ page }) => {
	const seam = await page.evaluate(() => {
		const sidebar = document.querySelector<HTMLElement>('[data-testid="sidebar"]');
		const handle = document.querySelector<HTMLElement>('[data-testid="sidebar-resize-handle"]');
		if (!sidebar || !handle) throw new Error("Sidebar seam targets are absent");
		const sidebarBox = sidebar.getBoundingClientRect();
		const handleBox = handle.getBoundingClientRect();
		const sidebarStyle = getComputedStyle(sidebar);
		const handleLine = getComputedStyle(handle, "::after");
		return {
			borderWidth: Number.parseFloat(sidebarStyle.borderRightWidth),
			borderStyle: sidebarStyle.borderRightStyle,
			handleLineBackground: handleLine.backgroundColor,
			seamInsideHandle: sidebarBox.right >= handleBox.left && sidebarBox.right <= handleBox.right,
			handleWidth: handleBox.width
		};
	});
	expect(seam.borderWidth).toBe(1);
	expect(seam.borderStyle).toBe("solid");
	expect(seam.handleLineBackground).toBe("rgba(0, 0, 0, 0)");
	expect(seam.seamInsideHandle).toBe(true);
	expect(seam.handleWidth).toBeGreaterThanOrEqual(6);
});

test("titlebar always shows the package version", async ({ page }) => {
	const version = page.getByTestId("app-version");
	const expected = `v${desktopPackage.version}`;
	await expect(version).toBeVisible();
	await expect(version).toHaveText(expected);
	await expect(version).toHaveAttribute("aria-label", `Synth Desktop version ${desktopPackage.version}`);

	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-menu-settings").click();
	await expect(page.getByTestId("settings-page")).toBeVisible();
	await expect(version).toBeVisible();
	await expect(version).toHaveText(expected);
});

test("titlebar chrome stays trimmed to version and terminal controls", async ({ page }) => {
	await expect(page.getByRole("button", { name: "Show terminal" })).toBeVisible();
	await expect(page.getByTestId("runtime-status")).toHaveCount(0);
	await expect(page.getByTestId("open-account-settings")).toHaveCount(0);
	await expect(page.getByTestId("open-models-settings")).toHaveCount(0);
});

test("the window has generous drag surfaces without swallowing titlebar controls", async ({ page }) => {
	const dragRegions = page.locator("[data-tauri-drag-region]");
	await expect(dragRegions).toHaveCount(4);
	await expect(page.getByTestId("titlebar")).toHaveAttribute("data-tauri-drag-region", "");
	await expect(page.getByRole("group", { name: /chat tab$/ })).toHaveAttribute("data-tauri-drag-region", "");

	const regions = await page.evaluate(() => ({
		titlebar: getComputedStyle(document.querySelector<HTMLElement>('[data-testid="titlebar"]')!).getPropertyValue("-webkit-app-region"),
		tab: getComputedStyle(document.querySelector<HTMLElement>('.titlebar [role="group"]')!).getPropertyValue("-webkit-app-region"),
		close: getComputedStyle(document.querySelector<HTMLElement>('.tab-close')!).getPropertyValue("-webkit-app-region"),
		terminal: getComputedStyle(document.querySelector<HTMLElement>('[aria-label="Show terminal"]')!).getPropertyValue("-webkit-app-region")
	}));
	expect(regions.titlebar).toBe("drag");
	expect(regions.tab).toBe("drag");
	expect(regions.close).toBe("no-drag");
	expect(regions.terminal).toBe("no-drag");

	const visibleSidebarInset = await page.evaluate(() => {
		const titlebar = document.querySelector<HTMLElement>('[data-testid="titlebar"]')!.getBoundingClientRect();
		const tab = document.querySelector<HTMLElement>('.titlebar [role="group"]')!.getBoundingClientRect();
		return tab.left - titlebar.left;
	});
	expect(visibleSidebarInset).toBeLessThanOrEqual(12);

	const hiddenSidebarInset = await page.evaluate(() => {
		document.documentElement.classList.add("sidebar-hidden");
		const titlebar = document.querySelector<HTMLElement>('[data-testid="titlebar"]')!.getBoundingClientRect();
		const tab = document.querySelector<HTMLElement>('.titlebar [role="group"]')!.getBoundingClientRect();
		const inset = tab.left - titlebar.left;
		document.documentElement.classList.remove("sidebar-hidden");
		return inset;
	});
	expect(hiddenSidebarInset).toBeGreaterThanOrEqual(78);
});

test("terminal panel is discoverable and toggles without changing the active surface", async ({ page }) => {
	await expect(page.getByRole("button", { name: "Show terminal" })).toBeVisible();
	await page.keyboard.press("Meta+j");
	await expect(page.getByTestId("terminal-panel")).toBeVisible();
	await expect(page.getByText("Terminal is available in the desktop app.")).toBeVisible();
	await expect(page.getByTestId("landing-page")).toBeVisible();
	const placement = await page.evaluate(() => {
		const main = document.querySelector<HTMLElement>(".main-pane")!.getBoundingClientRect();
		const terminal = document.querySelector<HTMLElement>("[data-testid=terminal-panel]")!.getBoundingClientRect();
		const composer = document.querySelector<HTMLElement>("[data-testid=composer]")!.getBoundingClientRect();
		return {
			terminalFlushWithBottom: Math.abs(terminal.bottom - main.bottom) <= 1,
			composerClearsTerminal: composer.bottom <= terminal.top - 16
		};
	});
	expect(placement).toEqual({ terminalFlushWithBottom: true, composerClearsTerminal: true });
	await page.keyboard.press("Meta+j");
	await expect(page.getByTestId("terminal-panel")).toBeHidden();
});

test("model picker stays visible and clickable above the terminal at supported sizes", async ({ page }) => {
	for (const [width, height] of [[960, 640], [1024, 700], [1280, 840], [1440, 900]] as const) {
		await page.setViewportSize({ width, height });
		await page.getByRole("button", { name: "Show terminal" }).click();
		await expect(page.getByTestId("terminal-panel")).toBeVisible();
		await page.getByTestId("composer-model").click();
		await expect(page.getByTestId("composer-model-menu")).toBeVisible();

		const geometry = await page.evaluate(() => {
			const menu = document.querySelector<HTMLElement>("[data-testid=composer-model-menu]")!;
			const terminal = document.querySelector<HTMLElement>("[data-testid=terminal-panel]")!;
			const menuRect = menu.getBoundingClientRect();
			const terminalRect = terminal.getBoundingClientRect();
			const samples = [0.18, 0.5, 0.82].map((ratio) => {
				const node = document.elementFromPoint(
					menuRect.left + menuRect.width * ratio,
					menuRect.top + Math.min(18, menuRect.height / 2)
				);
				return Boolean(node && menu.contains(node));
			});
			return {
				insideViewport: menuRect.left >= 0 && menuRect.top >= 0 && menuRect.right <= window.innerWidth && menuRect.bottom <= window.innerHeight,
				clearsTerminal: menuRect.bottom <= terminalRect.top - 8,
				paintedOnTop: samples.every(Boolean),
				noHorizontalOverflow: document.documentElement.scrollWidth <= window.innerWidth + 1
			};
		});

		expect(geometry).toEqual({
			insideViewport: true,
			clearsTerminal: true,
			paintedOnTop: true,
			noHorizontalOverflow: true
		});
		await page.getByTestId("composer-model").click();
		await expect(page.getByTestId("composer-model-menu")).toBeHidden();
		await page.getByTestId("terminal-panel").getByRole("button", { name: "Hide terminal" }).click();
	}
});

test("a long prompt never hides the active turn beneath the composer", async ({ page }) => {
	await page.addInitScript(() => {
		const sessionId = "long-prompt-chat";
		const longPrompt = Array.from({ length: 48 }, (_, index) =>
			`${index + 1}. This is deliberately long CUA regression text that must remain available without becoming a screen-height message wall.`
		).join("\n");
		(window as typeof window & { synthCodex?: unknown }).synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId, threadId: "thread-long-prompt", workspace: "/workspaces/default",
				model: "poolside/Laguna-XS-2.1-NVFP4-mlx", providerName: "local-laguna",
				providerTitle: "Laguna XS", baseUrl: "http://127.0.0.1:7333/v1", status: "running",
				title: "Long prompt fuzz fixture"
			}],
			start: async () => ({ sessionId, threadId: "thread-long-prompt" }),
			startTurn: async () => ({ sessionId, threadId: "thread-long-prompt", turnId: "turn-long-prompt" }),
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: (listener: (event: { sessionId: string; method: string; params: Record<string, unknown> }) => void) => {
				const timer = window.setTimeout(() => listener({
					sessionId, method: "turn/started", params: { turnId: "turn-long-prompt" }
				}), 100);
				return () => window.clearTimeout(timer);
			}
		};
		const rows = [
			{ sequence: 1, sessionSequence: 1, kind: "message.created", payload: { messageId: "long-user-message", role: "user", content: longPrompt } },
			{ sequence: 2, sessionSequence: 2, kind: "run.started", payload: { runId: "turn-long-prompt" } }
		].map((row) => ({
			schemaVersion: "synth.desktop-app-event.v1" as const, eventId: `evt-${row.sequence}`,
			sessionId, source: "codex" as const, createdAt: "2026-08-09T23:00:00Z", ...row
		}));
		(window as typeof window & { synthCore?: unknown }).synthCore = {
			diagnostics: async () => ({ databasePath: "/tmp/core.sqlite3", schemaVersion: 1, integrityOk: true,
				contentStorePath: "/tmp/content", journalHead: 2, sessionCount: 1, runCount: 1, visualCount: 0, migrationComplete: true }),
			eventsAfter: async () => rows,
			sessionEventsAfter: async () => rows,
			onEvent: () => () => undefined
		};
	});
	await page.reload();
	await page.getByTestId("local-chat-long-prompt-chat").click();
	await expect(page.getByTestId("user-message-long-user-message")).toBeVisible();
	await expect(page.getByRole("button", { name: "Show full message" })).toBeVisible();
	await expect(page.getByTestId("model-working")).toBeVisible();

	const tailGeometry = async () => page.evaluate(() => {
		const transcript = document.querySelector<HTMLElement>(".chat-transcript-scroll")!;
		const composer = document.querySelector<HTMLElement>("[data-testid=composer]")!.getBoundingClientRect();
		const working = document.querySelector<HTMLElement>("[data-testid=model-working]")!.getBoundingClientRect();
		const bubble = document.querySelector<HTMLElement>("[data-testid=user-message-long-user-message]")!;
		return {
			collapsed: bubble.classList.contains("is-collapsed"),
			atTail: transcript.scrollHeight - transcript.scrollTop - transcript.clientHeight <= 2,
			scrollViewportClearsComposer: transcript.getBoundingClientRect().bottom <= composer.top - 8,
			workingClearsComposer: working.bottom <= composer.top - 8
		};
	});
	expect(await tailGeometry()).toEqual({
		collapsed: true,
		atTail: true,
		scrollViewportClearsComposer: true,
		workingClearsComposer: true
	});

	await page.getByRole("button", { name: "Show full message" }).click();
	await expect.poll(tailGeometry).toEqual({
		collapsed: false,
		atTail: true,
		scrollViewportClearsComposer: true,
		workingClearsComposer: true
	});
});

// Composer model-menu containment: while open, the
// dropdown must stay inside the viewport with an 8px inset, never overlap the
// composer, scroll internally when tall, and flip above a low trigger.
async function readPickerLayout(page: import("@playwright/test").Page) {
	return page.evaluate(() => {
		const picker = document.querySelector('[data-testid="composer-model-menu"]');
		const composer = document.querySelector('[data-testid="composer"]');
		if (!picker) return { open: false as const };
		const p = picker.getBoundingClientRect();
		const c = composer?.getBoundingClientRect() ?? null;
		const selected = picker.querySelector(".composer-model-option.selected");
		const s = selected?.getBoundingClientRect() ?? null;
		return {
			open: true as const,
			rect: { left: p.left, top: p.top, right: p.right, bottom: p.bottom },
			viewport: { width: window.innerWidth, height: window.innerHeight },
			overlapsComposer: Boolean(
				c && !(p.right <= c.left || p.left >= c.right || p.bottom <= c.top || p.top >= c.bottom)
			),
			scrollsInternally: picker.scrollHeight > picker.clientHeight
				? getComputedStyle(picker).overflowY === "auto"
				: true,
			selectedVisible: Boolean(s && s.top >= p.top - 1 && s.bottom <= p.bottom + 1),
			bodyOverflowX: document.documentElement.scrollWidth > window.innerWidth,
			placement: picker.getAttribute("data-placement")
		};
	});
}

test("model picker stays contained at normal and short window sizes", async ({ page }) => {
	for (const [width, height] of [[1728, 1117], [1100, 700], [960, 640]] as const) {
		await page.setViewportSize({ width, height });
		await page.getByTestId("composer-model").click();
		await expect(page.getByTestId("composer-model-menu")).toBeVisible();
		const layout = await readPickerLayout(page);
		if (!layout.open) throw new Error("model dropdown did not open");
		expect(layout.rect.left, `left inset at ${width}x${height}`).toBeGreaterThanOrEqual(8);
		expect(layout.rect.top, `top inset at ${width}x${height}`).toBeGreaterThanOrEqual(8);
		expect(layout.rect.right, `right inset at ${width}x${height}`).toBeLessThanOrEqual(width - 8);
		expect(layout.rect.bottom, `bottom inset at ${width}x${height}`).toBeLessThanOrEqual(height - 8);
		expect(layout.selectedVisible, `selected visible at ${width}x${height}`).toBe(true);
		expect(layout.bodyOverflowX, `horizontal overflow at ${width}x${height}`).toBe(false);
		// The first level is intentionally limited to the three access methods.
		await expect(page.getByTestId("composer-model-access-local")).toBeVisible();
		await expect(page.getByTestId("composer-model-access-api")).toBeVisible();
		await expect(page.getByTestId("composer-model-access-chatgpt")).toBeVisible();
		await page.getByTestId("composer-model-access-local").click();
		await expect(page.getByTestId("composer-model-option-local-laguna")).toBeVisible();
		await page.keyboard.press("Escape");
		await expect(page.getByTestId("composer-model-menu")).not.toBeVisible();
	}
});

test("opening and closing the model picker never moves the composer", async ({ page }) => {
	await page.setViewportSize({ width: 1280, height: 840 });
	const before = await readLayout(page);
	await page.getByTestId("composer-model").click();
	await expect(page.getByTestId("composer-model-menu")).toBeVisible();
	const during = await readLayout(page);
	await page.keyboard.press("Escape");
	const after = await readLayout(page);
	expect(during.composer.top).toBeCloseTo(before.composer.top, 0);
	expect(after.composer.top).toBeCloseTo(before.composer.top, 0);
	expect(after.composer.left).toBeCloseTo(before.composer.left, 0);
});
