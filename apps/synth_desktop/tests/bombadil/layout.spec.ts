import { actions, always, extract } from "@antithesishq/bombadil";

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

const runtimeErrors = extract((state: any) => ({
	uncaught: state.errors?.uncaughtExceptions?.length ?? 0,
	consoleErrors: (state.console ?? []).filter((entry: { level?: string }) => entry.level === "error").length
}));

const accountRoute = extract((state: any) => {
	const account = state.document.querySelector<HTMLElement>('[data-testid="open-account-settings"]');
	const rect = account?.getBoundingClientRect();
	return {
		point: rect ? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 } : null,
		stubVisible: state.document.body.textContent?.includes("Account — stub") ?? false,
		lastActionWasAccount:
			typeof state.lastAction === "object" &&
			state.lastAction !== null &&
			"Click" in state.lastAction &&
			state.lastAction.Click.name === "Open Account settings",
		accountSettingsVisible: Boolean(state.document.querySelector('[data-testid="backend-settings"]'))
	};
});

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
		connectorsPoint: connectorsRect ? { x: connectorsRect.left + connectorsRect.width / 2, y: connectorsRect.top + connectorsRect.height / 2 } : null,
		searchPoint: searchRect ? { x: searchRect.left + searchRect.width / 2, y: searchRect.top + searchRect.height / 2 } : null,
		connectorsVisible: Boolean(state.document.querySelector('[data-testid="connectors-page"]')),
		searchVisible: Boolean(state.document.querySelector('[data-testid="conversation-search"]')),
		controlsUsable: Boolean(connectorsRect && searchRect && connectorsRect.width >= 120 && connectorsRect.height >= 28 && searchRect.width >= 120 && searchRect.height >= 28),
		connectorsFit: !connectorsPageRect || (connectorsPageRect.left >= 0 && connectorsPageRect.right <= state.window.innerWidth && connectorsPageRect.bottom <= state.window.innerHeight),
		searchFits: !searchDialogRect || (searchDialogRect.left >= 0 && searchDialogRect.right <= state.window.innerWidth && searchDialogRect.bottom <= state.window.innerHeight)
	};
});

/** Exercise the supported lower bound and representative desktop sizes. */
export const exploreViewportSizes = actions(() => [
	"Wait",
	{ SetViewport: { width: 960, height: 640 } },
	{ SetViewport: { width: 1280, height: 840 } },
	{ SetViewport: { width: 1440, height: 900 } }
]);

/** Directed dogfood action: exercise the titlebar avatar in every Bombadil run. */
export const openAccountSettings = actions(() => accountRoute.current.point ? [
	{ Click: { name: "Open Account settings", point: accountRoute.current.point } },
	"Wait"
] : ["Wait"]);

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

export const shell_never_overflows_horizontally = always(() =>
	layout.current.noHorizontalOverflow
);

export const core_shell_stays_visible = always(() => layout.current.shellVisible);

export const titlebar_runtime_status_stays_compact = always(() =>
	layout.current.runtimeStatusCompact
);

export const account_control_never_falls_back_to_stub_copy = always(() =>
	!accountRoute.current.stubVisible
);

export const account_click_reaches_backend_settings = always(() =>
	!accountRoute.current.lastActionWasAccount || accountRoute.current.accountSettingsVisible
);

export const connector_catalog_stays_inside_the_viewport = always(() =>
	primaryNavigation.current.connectorsFit
);

export const conversation_search_stays_inside_the_viewport = always(() =>
	primaryNavigation.current.searchFits
);

export const primary_connector_and_search_controls_remain_usable = always(() =>
	primaryNavigation.current.controlsUsable
);

export const subagent_visual_preserves_lifecycle_groups = always(() =>
	layout.current.subagentsValid
);

export const chat_rows_never_show_working_and_unread_at_once = always(() =>
	layout.current.chatIndicatorsValid
);

export const renderer_has_no_uncaught_errors = always(() =>
	runtimeErrors.current.uncaught === 0
);

export const renderer_has_no_console_errors = always(() =>
	runtimeErrors.current.consoleErrors === 0
);
