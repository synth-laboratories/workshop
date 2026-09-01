import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const visualsLayout = extract((state: any) => {
	const document = state.document;
	const page = document.querySelector<HTMLElement>('[data-testid="visuals-page"]');
	const grid = document.querySelector<HTMLElement>('[data-testid="visuals-grid"]');
	const preview = document.querySelector<HTMLElement>('[data-testid="visuals-preview"]');
	const splitter = document.querySelector<HTMLElement>('[data-testid="visuals-resize-handle"]');
	const previewHeader = document.querySelector<HTMLElement>('[data-testid="visuals-preview-header"]');
	const previewToolbar = document.querySelector<HTMLElement>('[data-testid="visuals-preview-toolbar"]');
	const previewContext = document.querySelector<HTMLElement>('[data-testid="visuals-preview-context"]');
	const provenance = document.querySelector<HTMLElement>('[data-testid="visual-add-to-report-identity"]');
	const previewOps = document.querySelector<HTMLElement>('[data-testid^="visual-ops-preview-"]');
	const open = document.querySelector<HTMLElement>('[data-testid="open-visuals"]')
		?? [...document.querySelectorAll<HTMLElement>("button")].find((button) => button.textContent?.trim() === "Visuals")
		?? null;
	const cards = [...document.querySelectorAll<HTMLElement>('[data-testid^="visuals-card-"]')];
	const firstCardControl = cards[0]?.querySelector<HTMLElement>(".visuals-card-main") ?? null;
	const shortCard = cards.find((card) => card.textContent?.includes("Laguna Prompt Trim Preinstall"));
	const longCard = cards.find((card) => card.textContent?.includes("deliberately taller wrapped title"));
	const shortActions = shortCard?.querySelector<HTMLElement>(".visuals-card-actions");
	const shortMain = shortCard?.querySelector<HTMLElement>(".visuals-card-main");
	const rect = (element: HTMLElement | null) => element?.getBoundingClientRect() ?? null;
	const fingerprint = (element: HTMLElement | null) => element ? {
		testId: element.dataset.testid ?? null,
		id: element.id || null,
		role: element.getAttribute("role"),
		accessibleName: element.getAttribute("aria-label"),
		tag: element.tagName.toLowerCase(),
		href: element.getAttribute("href"),
		nameAttr: element.getAttribute("name"),
		placeholder: element.getAttribute("placeholder"),
		inputType: element.getAttribute("type"),
		textContent: element.textContent?.trim() || null,
		structuralPath: null
	} : null;
	const pageRect = rect(page);
	const gridRect = rect(grid);
	const previewRect = rect(preview);
	const splitterRect = rect(splitter);
	const openRect = rect(open);
	const shortCardRect = rect(shortCard ?? null);
	const longCardRect = rect(longCard ?? null);
	const shortActionsRect = rect(shortActions ?? null);
	const shortMainRect = rect(shortMain ?? null);
	const firstCardRect = rect(firstCardControl);
	const headerRect = rect(previewHeader);
	const toolbarRect = rect(previewToolbar);
	const contextRect = rect(previewContext);
	const provenanceRect = rect(provenance);
	const previewOpsRect = rect(previewOps);
	const previewChildren = preview ? [...preview.children] as HTMLElement[] : [];
	const previewBody = previewChildren.find((child) => child !== previewHeader && child.getBoundingClientRect().height > 0) ?? null;
	const previewBodyRect = rect(previewBody);
	const controls = [
		...[...(previewToolbar?.querySelectorAll<HTMLElement>("button, select") ?? [])],
		...[...document.querySelectorAll<HTMLElement>(".visuals-card-actions button")]
	];
	const controlLabelsStayOnOneLine = controls.every((control) => {
		const style = getComputedStyle(control);
		const lineHeight = Number.parseFloat(style.lineHeight) || Number.parseFloat(style.fontSize) * 1.2;
		return control.getBoundingClientRect().width >= 40 && control.scrollHeight <= lineHeight * 1.8;
	});
	const maxTextBlockHeight = (element: HTMLElement | null, box: DOMRect | null) => {
		if (!element || !box) return true;
		const lineHeight = Number.parseFloat(getComputedStyle(element).lineHeight) || 18;
		return box.width >= 120 && box.height <= lineHeight * 5.2;
	};
	return {
		openPoint: openRect ? { x: openRect.left + openRect.width / 2, y: openRect.top + openRect.height / 2 } : null,
		openFingerprint: fingerprint(open),
		firstCardPoint: firstCardRect ? { x: firstCardRect.left + firstCardRect.width / 2, y: firstCardRect.top + Math.min(48, firstCardRect.height / 2) } : null,
		firstCardFingerprint: fingerprint(firstCardControl),
		pageVisible: Boolean(page),
		visible: Boolean(page && grid && preview),
		cardsSizeToContent: !shortCardRect || !longCardRect || shortCardRect.height < longCardRect.height,
		shortCardUsesIntrinsicRowHeight: !shortCard || getComputedStyle(shortCard).alignSelf === "start",
		shortCardActionsStayGrouped: !shortActionsRect || !shortMainRect || shortActionsRect.top - shortMainRect.bottom <= 1,
		columnsUsable: !gridRect || !previewRect || (
			gridRect.width >= 360 && previewRect.width >= 360 && (
				gridRect.right <= previewRect.left - 12
				|| gridRect.bottom <= previewRect.top - 12
			)
		),
		stackedUsable: !gridRect || !previewRect || (
			gridRect.width >= 360 && previewRect.width >= 360 && gridRect.bottom <= previewRect.top - 12
		),
		separatorVisible: Boolean(splitterRect && splitterRect.width > 0 && splitterRect.height > 0),
		previewHeaderBounded: !headerRect || headerRect.height <= 240,
		previewContextReadable: maxTextBlockHeight(provenance, provenanceRect)
			&& maxTextBlockHeight(previewOps, previewOpsRect),
		previewContentReachable: !headerRect || !previewBodyRect || (
			previewBodyRect.top >= headerRect.bottom - 1
			&& previewBodyRect.top - headerRect.bottom <= 80
		),
		previewRegionsContained: !previewRect || [headerRect, toolbarRect, contextRect].every((box) => !box || (
			box.left >= previewRect.left - 1 && box.right <= previewRect.right + 1
		)),
		controlLabelsStayOnOneLine,
		contained: !pageRect || !gridRect || !previewRect || (
			gridRect.left >= pageRect.left && previewRect.right <= pageRect.right + 1
			&& previewRect.right <= state.window.innerWidth - 8
		),
		noHorizontalOverflow: document.documentElement.scrollWidth <= state.window.innerWidth + 1
	};
});

export const open_visual_library_at_the_configured_supported_width = actions(() => {
	if (!visualsLayout.current.pageVisible && visualsLayout.current.openPoint && visualsLayout.current.openFingerprint) {
		return [{ Click: { fingerprint: visualsLayout.current.openFingerprint, point: visualsLayout.current.openPoint } }];
	}
	if (visualsLayout.current.pageVisible && !visualsLayout.current.visible && visualsLayout.current.firstCardPoint && visualsLayout.current.firstCardFingerprint) {
		return [{ Click: { fingerprint: visualsLayout.current.firstCardFingerprint, point: visualsLayout.current.firstCardPoint } }];
	}
	// Bombadil 0.7.2's generated SetViewport action currently fails its own
	// Rust deserializer (missing `fingerprint`). The runner supplies a bounded
	// viewport instead; keep exploring through an idempotent real control.
	return visualsLayout.current.firstCardPoint && visualsLayout.current.firstCardFingerprint
		? [{ Click: { fingerprint: visualsLayout.current.firstCardFingerprint, point: visualsLayout.current.firstCardPoint } }]
		: ["Wait"];
});

export const visual_library_split_view_is_exercised = eventually(() =>
	visualsLayout.current.visible
).within(8, "seconds");

export const visual_cards_do_not_stretch_short_metadata_into_a_blank_gap = always(() =>
	!visualsLayout.current.visible || (
		visualsLayout.current.cardsSizeToContent
		&& visualsLayout.current.shortCardUsesIntrinsicRowHeight
		&& visualsLayout.current.shortCardActionsStayGrouped
	)
);

/** CUA 2026-08-10: the preview was crushed into a narrow, clipped right rail. */
export const visual_library_keeps_usable_non_overlapping_panes = always(() =>
	!visualsLayout.current.visible || (
		(visualsLayout.current.columnsUsable
			|| (visualsLayout.current.stackedUsable && !visualsLayout.current.separatorVisible))
		&& visualsLayout.current.contained
		&& visualsLayout.current.noHorizontalOverflow
	)
);

/** CUA 2026-09-01: a long session/run/trace identity collapsed to one glyph
 * per line, made the preview header page-height, and pushed the visual itself
 * off-screen. Card actions independently shrank until their labels stacked. */
export const visual_preview_metadata_never_becomes_a_vertical_glyph_column = always(() =>
	!visualsLayout.current.visible || (
		visualsLayout.current.previewHeaderBounded
		&& visualsLayout.current.previewContextReadable
		&& visualsLayout.current.previewRegionsContained
	)
);

export const visual_preview_content_stays_reachable_below_its_header = always(() =>
	!visualsLayout.current.visible || visualsLayout.current.previewContentReachable
);

export const visual_library_controls_never_wrap_their_labels_vertically = always(() =>
	!visualsLayout.current.visible || visualsLayout.current.controlLabelsStayOnOneLine
);
