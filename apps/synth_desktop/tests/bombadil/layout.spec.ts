import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const layout = extract((state: any) => {
	const document = state.document;
	const viewport = state.window;
	const composer = document.querySelector<HTMLElement>('[data-testid="composer"]');
	const input = document.querySelector<HTMLElement>('[data-testid="composer-input"]');
	const transcriptScroll = document.querySelector<HTMLElement>(".chat-transcript-scroll");
	const visual = document.querySelector<HTMLElement>('[data-testid="visual-pane"]');
	const runtimeStatus = document.querySelector<HTMLElement>('[data-testid="runtime-status"]');
	const expected = Boolean(
		document.querySelector('[data-testid="landing-page"], [data-testid="chat-transcript"]')
	);
	const composerRect = composer?.getBoundingClientRect();
	const inputRect = input?.getBoundingClientRect();
	const transcriptPaddingBottom = transcriptScroll
		? Number.parseFloat(getComputedStyle(transcriptScroll).paddingBottom)
		: null;
	const visualRect = visual?.getBoundingClientRect();
	const runtimeStatusRect = runtimeStatus?.getBoundingClientRect();
	const subagents = document.querySelector<HTMLElement>('[data-testid="visual-subagents"]');
	const subagentGroups = subagents?.querySelectorAll(".subagents-group") ?? [];
	const subagentRows = subagents?.querySelectorAll<HTMLElement>(".subagent-row") ?? [];
	const chatRows = document.querySelectorAll<HTMLElement>('[data-testid^="local-chat-"]');
	return {
		initialized: Boolean(document.querySelector(".app-shell")),
		expected,
		present: Boolean(composer && input),
		insideViewport: Boolean(
			composerRect &&
			composerRect.left >= 0 &&
			composerRect.top >= 0 &&
			composerRect.right <= viewport.innerWidth &&
			composerRect.bottom <= viewport.innerHeight - 8
		),
		usable: Boolean(
			composerRect && inputRect &&
			composerRect.width >= 320 && composerRect.height >= 80 &&
			inputRect.width >= 240 && inputRect.height >= 40
		),
		avoidsVisual: Boolean(
			!visualRect || !composerRect || composerRect.right <= visualRect.left + 1
		),
		clearsTranscript: Boolean(
			!transcriptScroll || !composerRect ||
			(transcriptPaddingBottom !== null && transcriptPaddingBottom >= viewport.innerHeight - composerRect.top + 12)
		),
		noHorizontalOverflow:
			document.documentElement.scrollWidth <= viewport.innerWidth + 1,
		shellVisible: Boolean(
			document.querySelector('[data-testid="sidebar"]') &&
			document.querySelector('[data-testid="titlebar"]')
		),
		runtimeStatusCompact: Boolean(
			runtimeStatus && runtimeStatusRect && runtimeStatusRect.width <= 90 &&
			!/(Laguna·|\bOR\b|Intern|\d+\/\d+)/.test(runtimeStatus.textContent ?? "")
		),
		subagentsValid: !subagents || (
			subagentGroups.length === 2 &&
			[...subagentRows].every((row) => ["active", "done", "failed"].includes(row.dataset.status ?? ""))
		),
		chatIndicatorsValid: [...chatRows].every((row) =>
			row.querySelectorAll(".chat-working-indicator, .chat-unread-indicator").length <= 1
		)
	};
});

const visualAlignment = extract((state: any) => {
	const document = state.document;
	const rect = (selector: string) =>
		document.querySelector<HTMLElement>(selector)?.getBoundingClientRect() ?? null;
	const centerX = (value: DOMRect | null) => value ? value.left + value.width / 2 : null;
	const chat = rect('[data-testid="chat-transcript"]');
	const transcript = rect(".chat-transcript-inner");
	const composer = rect('[data-testid="composer"]');
	const trigger = rect('[data-testid="resource-shelf-trigger"]');
	const shelf = rect('[data-testid="resource-shelf"]');
	const triggerElement = document.querySelector<HTMLElement>('[data-testid="resource-shelf-trigger"]');
	const fixtureChat = [...document.querySelectorAll<HTMLElement>('[data-testid^="local-chat-"]')]
		.find((element) => element.textContent?.includes("Bombadil visual alignment"));
	const fixtureChatRect = fixtureChat?.getBoundingClientRect() ?? null;
	const triggerCenter = centerX(trigger);
	const transcriptCenter = centerX(transcript);
	const composerCenter = centerX(composer);
	const tolerance = 3;
	return {
		fixtureChatPoint: fixtureChatRect
			? { x: fixtureChatRect.left + fixtureChatRect.width / 2, y: fixtureChatRect.top + fixtureChatRect.height / 2 }
			: null,
		triggerPoint: trigger
			? { x: trigger.left + trigger.width / 2, y: trigger.top + trigger.height / 2 }
			: null,
		shelfOpen: Boolean(shelf),
		triggerStateMatchesPanel:
			!triggerElement || triggerElement.getAttribute("aria-expanded") === String(Boolean(shelf)),
		triggerInsideChat: !trigger || !chat || (
			trigger.left >= chat.left && trigger.right <= chat.right &&
			trigger.top >= chat.top && trigger.bottom <= chat.bottom
		),
		shelfInsideChat: !shelf || !chat || (
			shelf.left >= chat.left && shelf.right <= chat.right &&
			shelf.top >= chat.top && shelf.bottom <= chat.bottom
		),
		shelfAlignedToTrigger: !shelf || !trigger || (
			Math.abs(shelf.right - trigger.right) <= tolerance &&
			shelf.top >= trigger.bottom + 4
		),
		shelfClearsComposer: !shelf || !composer || shelf.bottom <= composer.top - 12,
		composerAlignedToTranscript:
			composerCenter === null || transcriptCenter === null ||
			Math.abs(composerCenter - transcriptCenter) <= tolerance,
		triggerAlignedToChatEdge: triggerCenter === null || !chat || (
			chat.right - trigger.right >= 8 && chat.right - trigger.right <= 24
		)
	};
});

/* A layered menu is only correct when it is usable, not merely mounted. */
const composerLayers = extract((state: any) => {
	const document = state.document;
	const viewport = state.window;
	const point = (selector: string) => {
		const element = document.querySelector<HTMLElement>(selector);
		const rect = element?.getBoundingClientRect();
		return rect ? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 } : null;
	};
	const menu = document.querySelector<HTMLElement>("[data-testid=composer-model-menu]");
	const terminal = document.querySelector<HTMLElement>("[data-testid=terminal-panel]");
	const menuRect = menu?.getBoundingClientRect();
	const terminalRect = terminal?.getBoundingClientRect();
	const paintedOnTop = !menu || !menuRect || [0.18, 0.5, 0.82].every((ratio) => {
		const node = document.elementFromPoint(
			menuRect.left + menuRect.width * ratio,
			menuRect.top + Math.min(18, menuRect.height / 2)
		);
		return Boolean(node && menu.contains(node));
	});
	return {
		showTerminalPoint: point('[aria-label="Show terminal"]'),
		modelPoint: point('[data-testid="composer-model"]'),
		terminalOpen: Boolean(terminal),
		menuOpen: Boolean(menu),
		menuInsideViewport: !menuRect || (
			menuRect.left >= 0 && menuRect.top >= 0 &&
			menuRect.right <= viewport.innerWidth && menuRect.bottom <= viewport.innerHeight
		),
		menuClearsTerminal: !menuRect || !terminalRect || menuRect.bottom <= terminalRect.top - 8,
		paintedOnTop
	};
});

const runtimeErrors = extract((state: any) => ({
	uncaught: state.errors?.uncaughtExceptions?.length ?? 0,
	consoleErrors: (state.console ?? []).filter((entry: { level?: string }) => entry.level === "error").length
}));

const accountControl = extract((state: any) => ({
	present: Boolean(state.document.querySelector('[data-testid="open-account-settings"]')),
	stubVisible: state.document.body.textContent?.includes("Account — stub") ?? false
}));

const primaryNavigation = extract((state: any) => {
	const connectors = state.document.querySelector<HTMLElement>('[data-testid="open-connectors"]');
	const search = state.document.querySelector<HTMLElement>('[data-testid="open-search"]');
	const connectorsRect = connectors?.getBoundingClientRect();
	const searchRect = search?.getBoundingClientRect();
	const connectorsPage = state.document.querySelector<HTMLElement>('[data-testid="connectors-page"]');
	const searchDialog = state.document.querySelector<HTMLElement>('[data-testid="conversation-search"]');
	const connectorsPageRect = connectorsPage?.getBoundingClientRect();
	const searchDialogRect = searchDialog?.getBoundingClientRect();
	return {
		initialized: Boolean(state.document.querySelector(".app-shell")),
		connectorsPoint: connectorsRect ? { x: connectorsRect.left + connectorsRect.width / 2, y: connectorsRect.top + connectorsRect.height / 2 } : null,
		searchPoint: searchRect ? { x: searchRect.left + searchRect.width / 2, y: searchRect.top + searchRect.height / 2 } : null,
		connectorsVisible: Boolean(state.document.querySelector('[data-testid="connectors-page"]')),
		searchVisible: Boolean(state.document.querySelector('[data-testid="conversation-search"]')),
		controlsUsable: Boolean(connectorsRect && searchRect && connectorsRect.width >= 120 && connectorsRect.height >= 28 && searchRect.width >= 120 && searchRect.height >= 28),
		connectorsFit: !connectorsPageRect || (connectorsPageRect.left >= 0 && connectorsPageRect.right <= state.window.innerWidth && connectorsPageRect.bottom <= state.window.innerHeight),
		searchFits: !searchDialogRect || (searchDialogRect.left >= 0 && searchDialogRect.right <= state.window.innerWidth && searchDialogRect.bottom <= state.window.innerHeight)
	};
});

const searchDismissal = extract((state: any) => {
	const search = state.document.querySelector<HTMLElement>('[data-testid="open-search"]');
	const rect = search?.getBoundingClientRect();
	return {
		searchPoint: rect ? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 } : null,
		searchVisible: Boolean(state.document.querySelector('[data-testid="conversation-search"]')),
		lastActionWasEscape:
			typeof state.lastAction === "object" &&
			state.lastAction !== null &&
			"PressKey" in state.lastAction &&
			state.lastAction.PressKey.code === 27,
		focusRestoredToTrigger: state.document.activeElement === search
	};
});

const interactionSafety = extract((state: any) => {
	const document = state.document;
	const viewport = state.window;
	const modals = [...document.querySelectorAll<HTMLElement>('[role="dialog"][aria-modal="true"]')];
	const modalIsUsable = modals.every((modal) => {
		const rect = modal.getBoundingClientRect();
		return Boolean(
			modal.getAttribute("aria-label") &&
			modal.contains(document.activeElement) &&
			modal.querySelector('button[aria-label^="Close"]') &&
			modal.getAttribute("aria-keyshortcuts")?.includes("Escape") &&
			rect.left >= 0 && rect.top >= 0 && rect.right <= viewport.innerWidth && rect.bottom <= viewport.innerHeight
		);
	});
	const allExpandedControls = [...document.querySelectorAll<HTMLElement>('[aria-expanded]')];
	const expandedControlsHaveContracts = allExpandedControls.every((control) =>
		Boolean(control.getAttribute("aria-controls"))
	);
	const expandedControls = allExpandedControls.filter((control) => control.getAttribute("aria-expanded") === "true");
	const expandedControlsAreBound = expandedControls.every((control) => {
		const id = control.getAttribute("aria-controls");
		const target = id ? document.getElementById(id) : null;
		const rect = target?.getBoundingClientRect();
		return Boolean(target && rect && rect.width > 0 && rect.height > 0 && rect.bottom > 0 && rect.top < viewport.innerHeight);
	});
	return {
		atMostOneModal: modals.length <= 1,
		modalIsUsable,
		expandedControlsHaveContracts,
		expandedControlsAreBound
	};
});

const backNavigation = extract((state: any) => {
	const document = state.document;
	const pointFor = (element: HTMLElement | null) => {
		const rect = element?.getBoundingClientRect();
		return rect ? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 } : null;
	};
	const routes = [
		{ id: "settings", page: '[data-testid="settings-page"]', entry: '[data-testid="settings"]', exit: '.desk-back' },
		{ id: "connectors", page: '[data-testid="connectors-page"]', entry: '[data-testid="open-connectors"]', exit: '.page-back' },
		{ id: "visuals", page: '[data-testid="visuals-page"]', entry: '[data-testid="open-visuals"]', exit: '.ghost-button' },
		{ id: "inventory", page: '[data-testid="inventory-page"]', entry: '[data-testid="open-inventory"]', exit: '.desk-back' },
		{ id: "cloud", page: '[data-testid="cloud-desk"]', entry: null, exit: '.desk-back' }
	] as const;
	const active = routes.find((route) => document.querySelector(route.page));
	const exit = active
		? document.querySelector<HTMLElement>(`${active.page} ${active.exit}`)
		: null;
	const lastClickName = typeof state.lastAction === "object" && state.lastAction !== null && "Click" in state.lastAction
		? state.lastAction.Click.name
		: null;
	const lastRouteExit = typeof lastClickName === "string" && lastClickName.startsWith("Return from ")
		? lastClickName.slice("Return from ".length)
		: null;
	const searchDialog = document.querySelector<HTMLElement>('[data-testid="conversation-search"]');
	return {
		searchVisible: Boolean(searchDialog),
		activeRoute: active?.id ?? null,
		activeRouteHasExit: !active || Boolean(exit),
		activeRouteExitPoint: pointFor(exit),
		entryPoints: routes
			.filter((route) => route.entry)
			.map((route) => ({ id: route.id, point: pointFor(document.querySelector<HTMLElement>(route.entry!)) }))
			.filter((route) => route.point !== null),
		lastRouteExit,
		searchHasExitContract: !searchDialog || (
			searchDialog.getAttribute("aria-keyshortcuts")?.includes("Escape") === true &&
			Boolean(searchDialog.querySelector('button[aria-label="Close search"]'))
		),
		visualPaneHasExit: !document.querySelector('[data-testid="visual-pane"]') ||
			Boolean(document.querySelector('[data-testid="visual-pane"] button[aria-label="Close visual"]')),
		containerPaneHasExit: !document.querySelector('[data-testid="container-pane"]') ||
			Boolean(document.querySelector('[data-testid="container-pane"] button[aria-label="Close container inspector"]')),
		terminalHasExit: !document.querySelector('[data-testid="terminal-panel"]') ||
			Boolean(document.querySelector('button[aria-label="Hide terminal"]')),
		chatHasExit: !document.querySelector('[data-testid="chat-transcript"]') ||
			Boolean(document.querySelector('button[aria-label="Close tab"]'))
	};
});

/** Exercise the supported lower bound and representative desktop sizes. */
export const exploreViewportSizes = actions(() => [
	"Wait",
	{ SetViewport: { width: 960, height: 640 } },
	{ SetViewport: { width: 1280, height: 840 } },
	{ SetViewport: { width: 1440, height: 900 } }
]);

/** Exercise the real layered controls before the normal viewport fuzzer runs. */
export const exercise_model_picker_above_terminal = actions(() => {
	if (!composerLayers.current.terminalOpen && composerLayers.current.showTerminalPoint) {
		return [{ Click: { name: "Open terminal for layered-composer fuzz", point: composerLayers.current.showTerminalPoint } }];
	}
	if (!composerLayers.current.menuOpen && composerLayers.current.modelPoint) {
		return [{ Click: { name: "Open model picker for layered-composer fuzz", point: composerLayers.current.modelPoint } }];
	}
	return ["Wait"];
});

/** Open and close Outputs whenever a resource-bearing transcript is available. */
export const exercise_outputs_alignment = actions(() =>
	visualAlignment.current.triggerPoint
		? [{ Click: {
			name: visualAlignment.current.shelfOpen ? "Close aligned Outputs" : "Open aligned Outputs",
			point: visualAlignment.current.triggerPoint
		} }]
		: visualAlignment.current.fixtureChatPoint
			? [{ Click: { name: "Open visual alignment fixture", point: visualAlignment.current.fixtureChatPoint } }]
		: ["Wait"]
);

/**
 * A modal search cannot depend on a human remembering a hidden escape route.
 * Direct this route on every exploration and assert the Escape postcondition.
 */
export const open_then_escape_conversation_search = actions(() =>
	searchDismissal.current.searchVisible
		? [{ PressKey: { code: 27 } }]
		: searchDismissal.current.searchPoint
			? [{ Click: { name: "Open conversation search", point: searchDismissal.current.searchPoint } }]
			: ["Wait"]
);

/**
 * Every shell route that replaces the working surface must expose an explicit
 * way back. When Bombadil enters one, its next directed action is that exit.
 */
export const return_from_every_navigable_surface = actions(() => {
	if (backNavigation.current.searchVisible) return [{ PressKey: { code: 27 } }];
	if (backNavigation.current.activeRoute && backNavigation.current.activeRouteExitPoint) {
		return [{ Click: {
			name: `Return from ${backNavigation.current.activeRoute}`,
			point: backNavigation.current.activeRouteExitPoint
		} }];
	}
	return backNavigation.current.entryPoints.map((route) => ({ Click: {
		name: `Open ${route.id}`,
		point: route.point
	} }));
});

export const composer_exists_when_expected = always(() =>
	!layout.current.expected || layout.current.present
);

export const composer_is_fully_visible = always(() =>
	!layout.current.expected || layout.current.insideViewport
);

export const composer_remains_usable = always(() =>
	!layout.current.expected || layout.current.usable
);

export const composer_does_not_overlap_visuals = always(() =>
	!layout.current.expected || layout.current.avoidsVisual
);

export const transcript_content_clears_composer = always(() =>
	!layout.current.expected || layout.current.clearsTranscript
);

export const transcript_and_composer_share_a_centerline = always(() =>
	visualAlignment.current.composerAlignedToTranscript
);

export const outputs_trigger_stays_inside_the_chat = always(() =>
	visualAlignment.current.triggerInsideChat && visualAlignment.current.triggerAlignedToChatEdge
);

export const outputs_panel_matches_its_expanded_state = always(() =>
	visualAlignment.current.triggerStateMatchesPanel
);

export const outputs_panel_stays_aligned_inside_the_chat = always(() =>
	visualAlignment.current.shelfInsideChat && visualAlignment.current.shelfAlignedToTrigger
);

export const outputs_panel_never_covers_the_composer = always(() =>
	visualAlignment.current.shelfClearsComposer
);

export const shell_never_overflows_horizontally = always(() =>
	layout.current.noHorizontalOverflow
);

export const app_shell_initializes_promptly = eventually(() =>
	layout.current.shellVisible
).within(3, "seconds");

export const terminal_and_model_picker_are_exercised = eventually(() =>
	composerLayers.current.terminalOpen && composerLayers.current.menuOpen
).within(5, "seconds");

export const model_picker_stays_visible_above_the_terminal = always(() =>
	!composerLayers.current.menuOpen || (
		composerLayers.current.menuInsideViewport &&
		composerLayers.current.menuClearsTerminal &&
		composerLayers.current.paintedOnTop
	)
);

export const core_shell_stays_visible = always(() =>
	!layout.current.initialized || layout.current.shellVisible
);

export const titlebar_runtime_status_stays_compact = always(() =>
	!layout.current.initialized || layout.current.runtimeStatusCompact
);

export const account_control_never_falls_back_to_stub_copy = always(() =>
	!accountControl.current.stubVisible
);

export const account_control_remains_present = always(() =>
	!layout.current.initialized || accountControl.current.present
);

export const connector_catalog_stays_inside_the_viewport = always(() =>
	primaryNavigation.current.connectorsFit
);

export const conversation_search_stays_inside_the_viewport = always(() =>
	primaryNavigation.current.searchFits
);

export const conversation_search_escape_always_returns_to_the_shell = always(() =>
	!searchDismissal.current.lastActionWasEscape
		? true
		: eventually(() => !searchDismissal.current.searchVisible).within(1, "seconds")
);

export const conversation_search_returns_focus_to_its_trigger = always(() =>
	!searchDismissal.current.lastActionWasEscape
		? true
		: eventually(() => searchDismissal.current.focusRestoredToTrigger).within(1, "seconds")
);

export const active_modal_never_traps_or_loses_focus = always(() =>
	interactionSafety.current.atMostOneModal && interactionSafety.current.modalIsUsable
);

export const expanded_controls_never_orphan_their_content = always(() =>
	interactionSafety.current.expandedControlsHaveContracts && interactionSafety.current.expandedControlsAreBound
);

export const every_active_surface_has_an_explicit_exit = always(() =>
	backNavigation.current.activeRouteHasExit &&
	backNavigation.current.searchHasExitContract &&
	backNavigation.current.visualPaneHasExit &&
	backNavigation.current.containerPaneHasExit &&
	backNavigation.current.terminalHasExit &&
	backNavigation.current.chatHasExit
);

export const route_back_buttons_always_return_to_the_shell = always(() =>
	!backNavigation.current.lastRouteExit
		? true
		: eventually(() => backNavigation.current.activeRoute !== backNavigation.current.lastRouteExit).within(1, "seconds")
);

export const primary_connector_and_search_controls_remain_usable = always(() =>
	!primaryNavigation.current.initialized || primaryNavigation.current.controlsUsable
);

export const subagent_visual_preserves_lifecycle_groups = always(() =>
	layout.current.subagentsValid
);

export const chat_rows_never_show_working_and_unread_at_once = always(() =>
	layout.current.chatIndicatorsValid
);

const polishState = extract((state: any) => {
	const document = state.document;
	const mode = document.querySelector<HTMLElement>('[data-testid="chat-transcript"]')?.dataset.activityMode ?? null;
	const queue = document.querySelectorAll('[data-testid^="queued-prompt-"]');
	const queueTexts = [...queue].map((node) => (node.querySelector("input") as HTMLInputElement | null)?.value ?? "");
	const active = document.activeElement as HTMLElement | null;
	const hiddenFocus = (() => {
		if (!active || active === document.body) return false;
		const style = state.window.getComputedStyle(active);
		const rect = active.getBoundingClientRect();
		return style.display === "none" || style.visibility === "hidden" || (rect.width === 0 && rect.height === 0);
	})();
	const composer = document.querySelector<HTMLElement>('[data-testid="composer"]');
	const composerRect = composer?.getBoundingClientRect();
	const sidebar = document.querySelector<HTMLElement>('[data-testid="sidebar"]');
	const sidebarWidth = sidebar?.getBoundingClientRect().width ?? null;
	const theme = document.documentElement.getAttribute("data-theme");
	const inferenceRail = document.querySelector<HTMLElement>('[data-testid="inference-rail"]');
	const inferencePanel = document.querySelector<HTMLElement>('[data-testid="inference-panel"]');
	const railRect = inferenceRail?.getBoundingClientRect();
	const panelRect = inferencePanel?.getBoundingClientRect();
	return {
		initialized: Boolean(document.querySelector(".app-shell")),
		modeValid: !mode || ["detailed", "grouped", "compact"].includes(mode),
		themeValid: !theme || ["light", "dark"].includes(theme),
		queueUnique: new Set(queueTexts).size === queueTexts.length,
		noHiddenFocus: !hiddenFocus,
		composerReachable: !composerRect || (composerRect.bottom <= state.window.innerHeight && composerRect.top >= 0),
		sidebarFinite: sidebarWidth === null || (Number.isFinite(sidebarWidth) && sidebarWidth >= 180 && sidebarWidth <= 420),
		steerErrorHonest: !document.querySelector('[data-testid="steer-error"]') || /not supported|unavailable|rejected/i.test(document.querySelector('[data-testid="steer-error"]')?.textContent ?? ""),
		settingsNavigable: !document.querySelector('[data-testid="settings-page"]') || Boolean(document.querySelector('[data-testid="settings-general"], [data-testid="settings-models"], [data-testid="settings-about"]')),
		projectsAbsent: !document.querySelector('[data-testid="project-list"], [data-testid="add-project"], [data-testid="quick-add-project"]'),
		inferenceContained: !railRect || !panelRect || (panelRect.left >= railRect.left && panelRect.right <= railRect.right + 1 && panelRect.top >= railRect.top && panelRect.bottom <= railRect.bottom + 1),
		inferenceInset: !railRect || !panelRect || (panelRect.left - railRect.left >= 8 && railRect.right - panelRect.right >= 8),
		composerClearsInference: !railRect || !composerRect || composerRect.right <= railRect.left + 1,
		inferenceNoOverflow: !inferenceRail || document.documentElement.scrollWidth <= state.window.innerWidth + 1
	};
});

const polishControls = extract((state: any) => {
	const point = (selector: string) => {
		const element = state.document.querySelector<HTMLElement>(selector);
		const rect = element?.getBoundingClientRect();
		return rect && rect.width > 0 && rect.height > 0
			? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }
			: null;
	};
	const controls = [
		["Dark theme", '[data-testid="theme-dark"]'],
		["Light theme", '[data-testid="theme-light"]'],
		["Detailed activity", '[data-testid="tool-activity-detailed"]'],
		["Grouped activity", '[data-testid="tool-activity-grouped"]'],
		["Compact activity", '[data-testid="tool-activity-compact"]'],
		["Enqueue with Enter", '[data-testid="active-enter-enqueue"]'],
		["Steer with Enter", '[data-testid="active-enter-steer"]'],
		["Save layout default", '[data-testid="save-layout-default"]'],
		["Apply layout default", '[data-testid="apply-layout-default"]'],
		["Reset layout", '[data-testid="reset-layout"]']
	].map(([name, selector]) => ({ name, point: point(selector) })).filter((entry) => entry.point !== null);
	return { settingsPoint: point('[data-testid="settings"]'), controls };
});

/** Exercise every currently visible preference control instead of only visiting Settings. */
export const exercise_poolside_polish_preferences = actions(() =>
	polishControls.current.controls.length > 0
		? polishControls.current.controls.map((control) => ({ Click: { name: control.name, point: control.point! } }))
		: polishControls.current.settingsPoint
			? [{ Click: { name: "Open settings for polish controls", point: polishControls.current.settingsPoint } }]
			: ["Wait"]
);

export const preferences_theme_stays_supported = always(() =>
	!polishState.current.initialized || polishState.current.themeValid
);

export const activity_mode_stays_supported = always(() =>
	!polishState.current.initialized || polishState.current.modeValid
);

export const queued_prompts_never_duplicate = always(() =>
	polishState.current.queueUnique
);

export const focus_never_lands_in_hidden_controls = always(() =>
	!polishState.current.initialized || polishState.current.noHiddenFocus
);

export const composer_remains_reachable_with_panes = always(() =>
	!polishState.current.initialized || polishState.current.composerReachable
);

export const sidebar_width_stays_within_bounds = always(() =>
	!polishState.current.initialized || polishState.current.sidebarFinite
);

export const inference_rail_keeps_a_contained_inset_panel = always(() =>
	!polishState.current.initialized || (polishState.current.inferenceContained && polishState.current.inferenceInset && polishState.current.composerClearsInference && polishState.current.inferenceNoOverflow)
);

export const steer_errors_stay_honest = always(() =>
	polishState.current.steerErrorHonest
);

export const settings_sections_remain_navigable = always(() =>
	!polishState.current.initialized || polishState.current.settingsNavigable
);

export const parked_projects_never_reappear = always(() =>
	!polishState.current.initialized || polishState.current.projectsAbsent
);

export const renderer_has_no_uncaught_errors = always(() =>
	runtimeErrors.current.uncaught === 0
);

export const renderer_has_no_console_errors = always(() =>
	runtimeErrors.current.consoleErrors === 0
);

/* Landing model-picker containment (12:54 screenshot regression): while the
 * dropdown is open it must stay inside the viewport with an 8px inset, never
 * cover the composer, scroll internally instead of growing past its slot, and
 * keep the selected option visible. */
const landingPickerLayout = extract((state: any) => {
	const document = state.document;
	const viewport = state.window;
	const trigger = document.querySelector<HTMLElement>('[data-testid="model-picker"]');
	const picker = document.querySelector<HTMLElement>('[data-testid="model-dropdown"]');
	const composer = document.querySelector<HTMLElement>('[data-testid="composer"]');
	const triggerRect = trigger?.getBoundingClientRect();
	const triggerPoint = triggerRect
		? { x: triggerRect.left + triggerRect.width / 2, y: triggerRect.top + triggerRect.height / 2 }
		: null;
	if (!picker) return { open: false, triggerPoint, insideViewport: true, avoidsComposer: true, scrollsInternally: true, selectedVisible: true, bodyOverflowX: false };
	const p = picker.getBoundingClientRect();
	const c = composer?.getBoundingClientRect() ?? null;
	const selected = picker.querySelector<HTMLElement>(".model-option.selected");
	const s = selected?.getBoundingClientRect() ?? null;
	return {
		open: true,
		triggerPoint,
		insideViewport:
			p.left >= 8 && p.top >= 8 &&
			p.right <= viewport.innerWidth - 8 && p.bottom <= viewport.innerHeight - 8,
		avoidsComposer: Boolean(
			!c || p.right <= c.left || p.left >= c.right || p.bottom <= c.top || p.top >= c.bottom
		),
		scrollsInternally: picker.scrollHeight <= picker.clientHeight ||
			getComputedStyle(picker).overflowY === "auto",
		selectedVisible: Boolean(!s || (s.top >= p.top - 1 && s.bottom <= p.bottom + 1)),
		bodyOverflowX: document.documentElement.scrollWidth > viewport.innerWidth + 1
	};
});

/** Open the landing picker, then let the viewport fuzzer squeeze it. */
export const exercise_landing_model_picker = actions(() => {
	if (!landingPickerLayout.current.open && landingPickerLayout.current.triggerPoint) {
		return [{ Click: { name: "Open landing model picker for containment fuzz", point: landingPickerLayout.current.triggerPoint } }];
	}
	return ["Wait"];
});

export const landing_model_picker_is_exercised = eventually(() =>
	landingPickerLayout.current.open
).within(5, "seconds");

export const landing_model_picker_stays_contained = always(() =>
	!landingPickerLayout.current.open || (
		landingPickerLayout.current.insideViewport &&
		landingPickerLayout.current.avoidsComposer &&
		landingPickerLayout.current.scrollsInternally &&
		landingPickerLayout.current.selectedVisible &&
		!landingPickerLayout.current.bodyOverflowX
	)
);
