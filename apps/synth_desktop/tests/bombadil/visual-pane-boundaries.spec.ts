import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const boundary = extract((state: any) => {
	const document = state.document;
	const point = (element: HTMLElement | null) => {
		const rect = element?.getBoundingClientRect();
		return rect ? { x: rect.left + rect.width / 2, y: rect.top + Math.min(rect.height / 2, 72) } : null;
	};
	const page = document.querySelector<HTMLElement>('[data-testid="visuals-page"]');
	const openLibrary = document.querySelector<HTMLElement>('[data-testid="open-visuals"]');
	const card = document.querySelector<HTMLElement>('[data-testid^="visuals-card-"]');
	const cardMain = card?.querySelector<HTMLElement>(".visuals-card-main") ?? null;
	const cardActions = card?.querySelector<HTMLElement>(".visuals-card-actions") ?? null;
	const openVisual = cardActions?.querySelector<HTMLElement>("button") ?? null;
	const splitter = document.querySelector<HTMLElement>('[aria-label="Resize visual pane"]');
	const visualPane = splitter?.nextElementSibling as HTMLElement | null;
	const cardMainRect = cardMain?.getBoundingClientRect();
	const cardActionsRect = cardActions?.getBoundingClientRect();
	const visualPaneRect = visualPane?.getBoundingClientRect();
	const splitterValue = Number(splitter?.getAttribute("aria-valuenow") ?? 0);
	return {
		pageVisible: Boolean(page),
		openLibraryPoint: point(openLibrary),
		openVisualPoint: point(openVisual),
		cardPresent: Boolean(card),
		cardUsesIntrinsicRowHeight: !card || getComputedStyle(card).alignSelf === "start",
		cardContentGrouped: !cardMainRect || !cardActionsRect || cardActionsRect.top - cardMainRect.bottom <= 1,
		splitterPoint: point(splitter),
		splitterVisible: Boolean(splitter),
		splitterValue,
		splitterVertical: splitter?.getAttribute("aria-orientation") === "vertical",
		splitterFocusable: splitter?.getAttribute("tabindex") === "0",
		splitterControlsPaneWidth: !visualPaneRect || splitterValue <= 420 || Math.abs(visualPaneRect.width - splitterValue) <= 2,
		noHorizontalOverflow: document.documentElement.scrollWidth <= state.window.innerWidth + 1
	};
});

export const open_visual_and_resize_its_boundary = actions(() => {
	if (!boundary.current.pageVisible && boundary.current.openLibraryPoint) return [{ Click: { name: "Open Visuals library", point: boundary.current.openLibraryPoint } }];
	if (!boundary.current.splitterVisible && boundary.current.openVisualPoint) return [{ Click: { name: "Open visual side pane", point: boundary.current.openVisualPoint } }];
	if (boundary.current.splitterPoint && boundary.current.splitterValue === 420) {
		return [{ Click: { name: "Focus visual pane splitter", point: boundary.current.splitterPoint } }, { PressKey: { code: 39 } }, { PressKey: { code: 39 } }];
	}
	return [{ SetViewport: { width: 1172, height: 768 } }];
});

export const visual_card_keeps_metadata_and_actions_together = always(() =>
	!boundary.current.cardPresent || (boundary.current.cardUsesIntrinsicRowHeight && boundary.current.cardContentGrouped && boundary.current.noHorizontalOverflow)
);

export const visual_boundary_is_an_accessible_resizable_separator = eventually(() =>
	boundary.current.splitterVisible
	&& boundary.current.splitterVertical
	&& boundary.current.splitterFocusable
	&& boundary.current.splitterValue > 420
	&& boundary.current.splitterControlsPaneWidth
).within(8, "seconds");
