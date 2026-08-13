import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const visualsLayout = extract((state: any) => {
	const document = state.document;
	const page = document.querySelector<HTMLElement>('[data-testid="visuals-page"]');
	const grid = document.querySelector<HTMLElement>('[data-testid="visuals-grid"]');
	const preview = document.querySelector<HTMLElement>('[data-testid="visuals-preview"]');
	const open = document.querySelector<HTMLElement>('[data-testid="open-visuals"]');
	const cards = [...document.querySelectorAll<HTMLElement>('[data-testid^="visuals-card-"]')];
	const shortCard = cards.find((card) => card.textContent?.includes("Laguna Prompt Trim Preinstall"));
	const longCard = cards.find((card) => card.textContent?.includes("deliberately taller wrapped title"));
	const shortActions = shortCard?.querySelector<HTMLElement>(".visuals-card-actions");
	const shortMain = shortCard?.querySelector<HTMLElement>(".visuals-card-main");
	const firstOpen = shortCard?.querySelector<HTMLElement>(".visuals-card-actions button");
	const splitter = document.querySelector<HTMLElement>('[aria-label="Resize visual pane"]');
	const sideVisual = document.querySelector<HTMLElement>('[data-testid="visual-pane"]');
	const rect = (element: HTMLElement | null) => element?.getBoundingClientRect() ?? null;
	const pageRect = rect(page);
	const gridRect = rect(grid);
	const previewRect = rect(preview);
	const openRect = rect(open);
	const shortCardRect = rect(shortCard ?? null);
	const longCardRect = rect(longCard ?? null);
	const shortActionsRect = rect(shortActions ?? null);
	const shortMainRect = rect(shortMain ?? null);
	const firstOpenRect = rect(firstOpen ?? null);
	const splitterRect = rect(splitter);
	return {
		openPoint: openRect ? { x: openRect.left + openRect.width / 2, y: openRect.top + openRect.height / 2 } : null,
		pageVisible: Boolean(page),
		artifactOpenPoint: firstOpenRect ? { x: firstOpenRect.left + firstOpenRect.width / 2, y: firstOpenRect.top + firstOpenRect.height / 2 } : null,
		splitterPoint: splitterRect ? { x: splitterRect.left + splitterRect.width / 2, y: splitterRect.top + Math.min(80, splitterRect.height / 2) } : null,
		splitterVisible: Boolean(splitterRect),
		splitterWidth: Number(splitter?.getAttribute("aria-valuenow") ?? 0),
		visible: Boolean(page && grid && preview),
		cardsSizeToContent: !shortCardRect || !longCardRect || shortCardRect.height < longCardRect.height,
		shortCardUsesIntrinsicRowHeight: !shortCard || getComputedStyle(shortCard).alignSelf === "start",
		shortCardActionsStayGrouped: !shortActionsRect || !shortMainRect || shortActionsRect.top - shortMainRect.bottom <= 1,
		columnsUsable: Boolean(sideVisual) || !gridRect || !previewRect || (
			gridRect.width >= 360 && previewRect.width >= 360 && gridRect.right <= previewRect.left - 12
		),
		contained: !pageRect || !gridRect || !previewRect || (
			gridRect.left >= pageRect.left && previewRect.right <= pageRect.right + 1
			&& previewRect.right <= state.window.innerWidth - 8
		),
		noHorizontalOverflow: document.documentElement.scrollWidth <= state.window.innerWidth + 1
	};
});

export const open_visual_library_and_fuzz_supported_widths = actions(() => {
	if (!visualsLayout.current.pageVisible && visualsLayout.current.openPoint) {
		return [{ Click: { name: "Open Visuals library", point: visualsLayout.current.openPoint } }];
	}
	if (!visualsLayout.current.splitterVisible && visualsLayout.current.artifactOpenPoint) {
		return [{ Click: { name: "Open visual side pane", point: visualsLayout.current.artifactOpenPoint } }];
	}
	if (visualsLayout.current.splitterPoint && visualsLayout.current.splitterWidth === 420) {
		return [
			{ Click: { name: "Focus visual pane splitter", point: visualsLayout.current.splitterPoint } },
			{ PressKey: { code: 37 } },
			{ PressKey: { code: 37 } }
		];
	}
	return [
		{ SetViewport: { width: 960, height: 640 } },
		{ SetViewport: { width: 1172, height: 768 } },
		{ SetViewport: { width: 1280, height: 840 } }
	];
});

export const visual_library_split_view_is_exercised = eventually(() =>
	visualsLayout.current.visible
).within(8, "seconds");

export const visual_cards_do_not_stretch_short_metadata_into_a_blank_gap = always(() =>
	!visualsLayout.current.visible || (
		visualsLayout.current.shortCardUsesIntrinsicRowHeight
		&& visualsLayout.current.shortCardActionsStayGrouped
	)
);

export const visual_pane_splitter_is_present_and_keyboard_resizable = eventually(() =>
	visualsLayout.current.splitterVisible && visualsLayout.current.splitterWidth > 420
).within(8, "seconds");

/** CUA 2026-08-10: the preview was crushed into a narrow, clipped right rail. */
export const visual_library_keeps_two_usable_non_overlapping_columns = always(() =>
	!visualsLayout.current.visible || (
		visualsLayout.current.columnsUsable
		&& visualsLayout.current.contained
		&& visualsLayout.current.noHorizontalOverflow
	)
);
