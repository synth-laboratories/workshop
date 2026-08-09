import { actions, always, extract } from "@antithesishq/bombadil";

const layout = extract((state: any) => {
	const document = state.document;
	const viewport = state.window;
	const composer = document.querySelector<HTMLElement>('[data-testid="composer"]');
	const input = document.querySelector<HTMLElement>('[data-testid="composer-input"]');
	const visual = document.querySelector<HTMLElement>('[data-testid="visual-pane"]');
	const expected = Boolean(
		document.querySelector('[data-testid="landing-page"], [data-testid="chat-transcript"]')
	);
	const composerRect = composer?.getBoundingClientRect();
	const inputRect = input?.getBoundingClientRect();
	const visualRect = visual?.getBoundingClientRect();
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
		noHorizontalOverflow:
			document.documentElement.scrollWidth <= viewport.innerWidth + 1,
		shellVisible: Boolean(
			document.querySelector('[data-testid="sidebar"]') &&
			document.querySelector('[data-testid="titlebar"]')
		)
	};
});

const runtimeErrors = extract((state: any) => ({
	uncaught: state.errors?.uncaughtExceptions?.length ?? 0,
	consoleErrors: (state.console ?? []).filter((entry: { level?: string }) => entry.level === "error").length
}));

/** Exercise the supported lower bound and representative desktop sizes. */
export const exploreViewportSizes = actions(() => [
	"Wait",
	{ SetViewport: { width: 960, height: 640 } },
	{ SetViewport: { width: 1280, height: 840 } },
	{ SetViewport: { width: 1440, height: 900 } }
]);

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

export const shell_never_overflows_horizontally = always(() =>
	layout.current.noHorizontalOverflow
);

export const core_shell_stays_visible = always(() => layout.current.shellVisible);

export const renderer_has_no_uncaught_errors = always(() =>
	runtimeErrors.current.uncaught === 0
);

export const renderer_has_no_console_errors = always(() =>
	runtimeErrors.current.consoleErrors === 0
);
