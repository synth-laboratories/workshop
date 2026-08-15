import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const visualsLayout = extract((state: any) => {
	const document = state.document;
	const page = document.querySelector<HTMLElement>('[data-testid="visuals-page"]');
	const grid = document.querySelector<HTMLElement>('[data-testid="visuals-grid"]');
	const preview = document.querySelector<HTMLElement>('[data-testid="visuals-preview"]');
	const splitter = document.querySelector<HTMLElement>('[data-testid="visuals-resize-handle"]');
	const open = document.querySelector<HTMLElement>('[data-testid="open-visuals"]');
	const rect = (element: HTMLElement | null) => element?.getBoundingClientRect() ?? null;
	const pageRect = rect(page);
	const gridRect = rect(grid);
	const previewRect = rect(preview);
	const splitterRect = rect(splitter);
	const openRect = rect(open);
	return {
		openPoint: openRect ? { x: openRect.left + openRect.width / 2, y: openRect.top + openRect.height / 2 } : null,
		pageVisible: Boolean(page),
		visible: Boolean(page && grid && preview),
		columnsUsable: !gridRect || !previewRect || (
			gridRect.width >= 360 && previewRect.width >= 360 && gridRect.right <= previewRect.left - 12
		),
		stackedUsable: !gridRect || !previewRect || (
			gridRect.width >= 360 && previewRect.width >= 360 && gridRect.bottom <= previewRect.top - 12
		),
		separatorVisible: Boolean(splitterRect && splitterRect.width > 0 && splitterRect.height > 0),
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
	return [
		{ SetViewport: { width: 960, height: 640 } },
		{ SetViewport: { width: 1172, height: 768 } },
		{ SetViewport: { width: 1280, height: 840 } }
	];
});

export const visual_library_split_view_is_exercised = eventually(() =>
	visualsLayout.current.visible
).within(8, "seconds");

/** CUA 2026-08-10: the preview was crushed into a narrow, clipped right rail. */
export const visual_library_keeps_usable_non_overlapping_panes = always(() =>
	!visualsLayout.current.visible || (
		(visualsLayout.current.columnsUsable
			|| (visualsLayout.current.stackedUsable && !visualsLayout.current.separatorVisible))
		&& visualsLayout.current.contained
		&& visualsLayout.current.noHorizontalOverflow
	)
);
