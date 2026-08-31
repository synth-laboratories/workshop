import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const geometry = extract((state: any) => {
	const document = state.document;
	const rect = (selector: string) =>
		document.querySelector<HTMLElement>(selector)?.getBoundingClientRect() ?? null;
	const point = (value: DOMRect | null) => value
		? { x: value.left + value.width / 2, y: value.top + value.height / 2 }
		: null;
	const fixture = [...document.querySelectorAll<HTMLElement>('[data-testid^="local-chat-"]')]
		.find((element) => element.textContent?.includes("Bombadil visual alignment"));
	const chat = rect('[data-testid="chat-transcript"]');
	const composer = rect('[data-testid="composer"]');
	const dock = rect('[data-testid="composer-dock"]');
	const panel = rect('[data-testid="workbench-side-panel"]');
	const handle = rect('.workbench.with-side-panel > [data-testid="pane-resize-handle"]');
	const handleElement = document.querySelector<HTMLElement>('.workbench.with-side-panel > [data-testid="pane-resize-handle"]');
	const transcriptCollapsed = handleElement?.dataset.primaryCollapsed === "true";
	const outputsTrigger = rect('[data-testid="resource-shelf-trigger"]');
	const tolerance = 4;
	const center = (value: DOMRect | null) => value ? value.left + value.width / 2 : null;
	const chatCenter = center(chat);
	const composerCenter = center(composer);
	return {
		bodyText: document.body.innerText.slice(0, 240),
		bootErrors: (state.window as any).__bombadilBootErrors ?? [],
		fixturePoint: point(fixture?.getBoundingClientRect() ?? null),
		outputsPoint: point(outputsTrigger),
		handlePoint: point(handle),
		transcriptCollapsed,
		panelOpen: Boolean(panel && handle),
		panelWidth: panel?.width ?? null,
		chatRect: chat && { left: chat.left, right: chat.right, width: chat.width },
		composerRect: composer && { left: composer.left, right: composer.right, width: composer.width },
		dockRect: dock && { left: dock.left, right: dock.right, width: dock.width },
		dockInlineStyle: document.querySelector<HTMLElement>('[data-testid="composer-dock"]')?.getAttribute("style") ?? null,
		livePanelWidth: getComputedStyle(document.querySelector<HTMLElement>(".main-pane")!).getPropertyValue("--live-side-panel-width"),
		panelWide: Boolean(panel && panel.width >= 500),
		panelNarrow: Boolean(panel && panel.width <= 360),
		composerInsideTranscript: transcriptCollapsed || !chat || !composer || (
			composer.left >= chat.left - tolerance && composer.right <= chat.right + tolerance
		),
		composerCenteredOnTranscript: transcriptCollapsed ||
			chatCenter === null || composerCenter === null ||
			Math.abs(chatCenter - composerCenter) <= tolerance,
		dockInsideTranscript: transcriptCollapsed || !chat || !dock || (
			dock.left >= chat.left - tolerance && dock.right <= chat.right + tolerance
		),
		noHorizontalOverflow: document.documentElement.scrollWidth <= state.window.innerWidth + 1
	};
});

/** Open Outputs, then alternate the real pointer separator across both extremes. */
let reachedWide = false;
let reachedNarrow = false;
let observedCollapsed = false;
let overdragPhase: "range" | "maximize-panel" | "minimize-panel" | "done" = "range";
export const open_outputs_and_drag_the_shared_boundary = actions(() => {
	if (!geometry.current.panelOpen && geometry.current.fixturePoint && !geometry.current.outputsPoint) {
		return [{ Click: { name: "Open side-panel drag fixture", point: geometry.current.fixturePoint } }];
	}
	if (!geometry.current.panelOpen && geometry.current.outputsPoint) {
		return [{ Click: { name: "Open unified Outputs dock", point: geometry.current.outputsPoint } }];
	}
	if (!geometry.current.handlePoint || geometry.current.panelWidth === null) return ["Wait"];
	observedCollapsed ||= geometry.current.transcriptCollapsed;
	reachedWide ||= geometry.current.panelWide;
	reachedNarrow ||= geometry.current.panelNarrow;
	const from = geometry.current.handlePoint;
	if (reachedWide && reachedNarrow && overdragPhase === "range") overdragPhase = "maximize-panel";
	if (overdragPhase === "maximize-panel") {
		overdragPhase = "minimize-panel";
		return [{ MouseDrag: { from, to: { x: 8, y: from.y }, steps: 18, delayMillis: 12 } }];
	}
	if (overdragPhase === "minimize-panel") {
		overdragPhase = "done";
		return [{ MouseDrag: { from, to: { x: 1272, y: from.y }, steps: 18, delayMillis: 12 } }];
	}
	if (overdragPhase === "done") return ["Wait"];
	const to = geometry.current.panelWide
		? { x: from.x + 300, y: from.y }
		: { x: from.x - 220, y: from.y };
	return [{ MouseDrag: { from, to, steps: 12, delayMillis: 12 } }];
});

export const unified_dock_opens = eventually(() => geometry.current.panelOpen)
	.within(8, "seconds");

export const drag_reaches_a_wide_panel = eventually(() => geometry.current.panelWide)
	.within(12, "seconds");

export const drag_reaches_a_narrow_panel = eventually(() => geometry.current.panelNarrow)
	.within(12, "seconds");

export const overdrag_collapses_the_transcript = eventually(() => observedCollapsed)
	.within(12, "seconds");

export const composer_never_crosses_the_transcript_boundary = always(() =>
	geometry.current.composerInsideTranscript && geometry.current.dockInsideTranscript
);

export const composer_keeps_the_transcript_centerline_during_drag = always(() =>
	geometry.current.composerCenteredOnTranscript
);

export const dragging_never_creates_horizontal_overflow = always(() =>
	geometry.current.noHorizontalOverflow
);
