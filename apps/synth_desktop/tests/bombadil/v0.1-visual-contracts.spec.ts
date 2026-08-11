import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const visualContracts = extract((state: any) => {
	const document = state.document;
	const point = (element: HTMLElement | null) => {
		const box = element?.getBoundingClientRect();
		return box ? { x: box.left + box.width / 2, y: box.top + box.height / 2 } : null;
	};
	const titlebar = document.querySelector<HTMLElement>('[data-testid="titlebar"]');
	const actions = [...(titlebar?.querySelectorAll<HTMLElement>(".titlebar-actions > *") ?? [])];
	const account = document.querySelector<HTMLElement>('[data-testid="account-menu-trigger"]');
	const accountSettings = document.querySelector<HTMLElement>('[data-testid="account-menu-settings"], [data-testid="open-account-settings"]');
	const modelsButton = [...document.querySelectorAll<HTMLButtonElement>("button")]
		.find((button) => button.textContent?.trim() === "Models") ?? null;
	const providerCard = document.querySelector<HTMLElement>('[data-testid="authorized-models"]');
	const rows = [...(providerCard?.querySelectorAll<HTMLElement>(".authorized-model-row") ?? [])];
	const marks = [...(providerCard?.querySelectorAll<HTMLElement>(".authorized-model-mark") ?? [])];
	const slugs = [...(providerCard?.querySelectorAll<HTMLElement>(".authorized-model-identity code") ?? [])];
	const markBoxes = marks.map((mark) => mark.getBoundingClientRect());
	const cardBox = providerCard?.getBoundingClientRect();
	const sidebar = document.querySelector<HTMLElement>('[data-testid="sidebar"]');
	const sidebarBox = sidebar?.getBoundingClientRect();
	const sidebarHandle = document.querySelector<HTMLElement>('[data-testid="sidebar-resize-handle"]');
	const sidebarHandleBox = sidebarHandle?.getBoundingClientRect();
	const sidebarStyle = sidebar ? getComputedStyle(sidebar) : null;
	const sidebarHandleLine = sidebarHandle ? getComputedStyle(sidebarHandle, "::after") : null;
	const newConversation = document.querySelector<HTMLElement>('[data-testid="new-conversation"]');
	const search = document.querySelector<HTMLElement>('[data-testid="open-search"]');
	const newIcon = newConversation?.querySelector<HTMLElement>("svg");
	const searchIcon = search?.querySelector<HTMLElement>("svg");
	const newIconBox = newIcon?.getBoundingClientRect();
	const searchIconBox = searchIcon?.getBoundingClientRect();
	const titlebarBox = titlebar?.getBoundingClientRect();
	const tab = titlebar?.querySelector<HTMLElement>('[role="tab"]');
	const tabBox = tab?.getBoundingClientRect();
	const actionBoxes = actions.map((action) => action.getBoundingClientRect());
	const iconCenter = (box?: DOMRect) => box ? box.left + box.width / 2 : null;
	const newCenter = iconCenter(newIconBox);
	const searchCenter = iconCenter(searchIconBox);
	// Tauri's x:20 anchor places the 15px native light's center at 27.5
	// logical pixels. Keep the renderer navigation axis on that same line.
	const nativeTrafficLightCenterX = 27.5;
	return {
		accountPoint: point(account),
		accountSettingsPoint: point(accountSettings),
		modelsPoint: point(modelsButton),
		settingsVisible: Boolean(document.querySelector('[data-testid="settings-page"]')),
		providersVisible: Boolean(providerCard),
		forbiddenModelSurfaceAbsent: !/Muse Glimmer|muse-glimmer|GGUF|DFlash/i.test(document.body.textContent ?? ""),
		titlebarIsClean: Boolean(titlebar) &&
			!titlebar.querySelector('[data-testid="runtime-status"], [data-testid="titlebar-account"], [data-testid="titlebar-account-avatar"], [data-testid="titlebar-cloud-status"]') &&
			!actions.some((element) => !["app-version", "toggle-terminal", "toggle-inference-rail"].includes(element.dataset.testid ?? "")) &&
			![...titlebar.querySelectorAll("*")].some((element) => ["Local", "S"].includes(element.textContent?.trim() ?? "")),
		providerRowsComplete: !providerCard || rows.length === 4,
		providerMarksQuiet: !providerCard || (
			marks.length === 4 && markBoxes.every((box) => box.width <= 22 && box.height <= 22)
		),
		providerMarksAligned: markBoxes.length < 2 || (
			Math.max(...markBoxes.map((box) => box.left + box.width / 2)) -
			Math.min(...markBoxes.map((box) => box.left + box.width / 2)) <= 1
		),
		slugsSubordinate: !providerCard || (
			slugs.length === 4 && slugs.every((slug) => Number.parseFloat(getComputedStyle(slug).fontSize) <= 10)
		),
		providerCardContained: !cardBox || (
			cardBox.left >= 0 && cardBox.right <= state.window.innerWidth &&
			document.documentElement.scrollWidth <= state.window.innerWidth + 1
		),
		sidebarContained: !sidebarBox || (sidebarBox.left === 0 &&
			sidebarBox.top === 0 && sidebarBox.bottom <= state.window.innerHeight + 1 &&
			sidebarBox.width >= 220 && sidebarBox.width <= 420),
		sidebarSeamIsSingle: !sidebarBox || Boolean(
			sidebarHandleBox && sidebarStyle && sidebarHandleLine &&
			Number.parseFloat(sidebarStyle.borderRightWidth) === 1 &&
			sidebarStyle.borderRightStyle === "solid" &&
			sidebarHandleLine.backgroundColor === "rgba(0, 0, 0, 0)" &&
			sidebarBox.right >= sidebarHandleBox.left && sidebarBox.right <= sidebarHandleBox.right
		),
		navigationIconsAligned: !sidebarBox || (newCenter !== null && searchCenter !== null &&
			Math.abs(newCenter - searchCenter) <= 0.5),
		navigationIconsMatchTrafficLights: !sidebarBox || (
			newCenter !== null && searchCenter !== null &&
			Math.abs((newCenter - sidebarBox.left) - nativeTrafficLightCenterX) <= 1.5 &&
			Math.abs((searchCenter - sidebarBox.left) - nativeTrafficLightCenterX) <= 1.5
		),
		navigationRowsContained: !sidebarBox || (Boolean(newConversation && search) &&
			[newConversation!, search!].every((row) => {
				const box = row.getBoundingClientRect();
				return box.left >= sidebarBox!.left && box.right <= sidebarBox!.right && box.height >= 28;
			})),
		titlebarContained: !sidebarBox || (Boolean(titlebarBox) &&
			titlebarBox!.left >= sidebarBox.right && titlebarBox!.right <= state.window.innerWidth + 1),
		tabContained: !tabBox || !titlebarBox || (
			tabBox.left >= titlebarBox.left && tabBox.right <= titlebarBox.right &&
			tabBox.top >= titlebarBox.top && tabBox.bottom <= titlebarBox.bottom
		),
		titlebarActionsContained: !titlebarBox || actionBoxes.every((box) =>
			box.left >= titlebarBox.left && box.right <= titlebarBox.right &&
			box.top >= titlebarBox.top && box.bottom <= titlebarBox.bottom
		),
		shellHasNoHorizontalOverflow: document.documentElement.scrollWidth <= state.window.innerWidth + 1
	};
});

export const navigate_to_credentialed_models_and_resize = actions(() => {
	if (!visualContracts.current.settingsVisible && !visualContracts.current.accountSettingsPoint && visualContracts.current.accountPoint) {
		return [{ Click: { name: "Open account menu", point: visualContracts.current.accountPoint } }];
	}
	if (!visualContracts.current.settingsVisible && visualContracts.current.accountSettingsPoint) {
		return [{ Click: { name: "Open Settings", point: visualContracts.current.accountSettingsPoint } }];
	}
	if (!visualContracts.current.providersVisible && visualContracts.current.modelsPoint) {
		return [{ Click: { name: "Open Models settings", point: visualContracts.current.modelsPoint } }];
	}
	return [
		{ SetViewport: { width: 900, height: 640 } },
		{ SetViewport: { width: 1100, height: 700 } },
		{ SetViewport: { width: 1440, height: 900 } }
	];
});

export const credentialed_model_surface_is_exercised = eventually(() =>
	visualContracts.current.providersVisible
).within(8, "seconds");

export const legacy_titlebar_chrome_never_returns = always(() =>
	visualContracts.current.titlebarIsClean
);

export const v01_never_exposes_local_muse_glimmer_or_legacy_model_formats = always(() =>
	visualContracts.current.forbiddenModelSurfaceAbsent
);

export const authorized_provider_rows_remain_complete = always(() =>
	visualContracts.current.providerRowsComplete
);

export const authorized_provider_marks_remain_small_and_aligned = always(() =>
	visualContracts.current.providerMarksQuiet && visualContracts.current.providerMarksAligned
);

export const model_slugs_remain_subordinate = always(() =>
	visualContracts.current.slugsSubordinate
);

export const authorized_provider_card_never_overflows = always(() =>
	visualContracts.current.providerCardContained
);

export const sidebar_stays_inside_the_window = always(() =>
	visualContracts.current.sidebarContained
);

export const sidebar_and_resize_target_never_double_paint_the_seam = always(() =>
	visualContracts.current.sidebarSeamIsSingle
);

export const primary_navigation_icons_share_one_native_anchor = always(() =>
	visualContracts.current.navigationIconsAligned && visualContracts.current.navigationIconsMatchTrafficLights
);

export const primary_navigation_rows_stay_inside_the_sidebar = always(() =>
	visualContracts.current.navigationRowsContained
);

export const titlebar_stays_on_the_content_side_of_the_divider = always(() =>
	visualContracts.current.titlebarContained
);

export const titlebar_tab_and_actions_stay_inside_the_titlebar = always(() =>
	visualContracts.current.tabContained && visualContracts.current.titlebarActionsContained
);

export const shell_never_grows_a_horizontal_scrollbar = always(() =>
	visualContracts.current.shellHasNoHorizontalOverflow
);
