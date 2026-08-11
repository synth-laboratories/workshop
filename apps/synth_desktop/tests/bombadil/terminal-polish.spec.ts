import { actions, always, eventually, extract } from "@antithesishq/bombadil";

/*
 * The terminal is a real bottom work surface, not a decorative overlay. This
 * focused fixture supplies two sessions so the test can verify the tab rail,
 * the resizer, and the composer clearance together at every supported size.
 */
const terminalSurface = extract((state: any) => {
	const document = state.document;
	const viewport = state.window;
	const panel = document.querySelector<HTMLElement>("[data-testid=terminal-panel]");
	const composer = document.querySelector<HTMLElement>("[data-testid=composer]");
	const resize = document.querySelector<HTMLElement>("[data-testid=terminal-resize-handle]");
	const tabs = [...document.querySelectorAll<HTMLElement>("[data-testid=terminal-panel] [role=tab]" )];
	const actions = [...document.querySelectorAll<HTMLElement>("[data-testid=terminal-panel] .terminal-action")];
	const point = (element: HTMLElement | null) => {
		const rect = element?.getBoundingClientRect();
		return rect ? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 } : null;
	};
	const panelRect = panel?.getBoundingClientRect();
	const composerRect = composer?.getBoundingClientRect();
	const resizeRect = resize?.getBoundingClientRect();
	const expectedHeight = Number.parseFloat(document.documentElement.style.getPropertyValue("--bottom-panel-height")) || 220;
	const controlsAreHitTestable = actions.every((control) => {
		const rect = control.getBoundingClientRect();
		const hit = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
		return Boolean(hit && control.contains(hit));
	});
	return {
		initialized: Boolean(document.querySelector(".app-shell")),
		showTerminalPoint: point(document.querySelector<HTMLElement>("[aria-label='Show terminal']")),
		resizePoint: point(resize),
		open: Boolean(panel),
		tabCount: tabs.length,
		panelHeight: panelRect?.height ?? 0,
		panelBounds: panelRect ? { left: panelRect.left, top: panelRect.top, right: panelRect.right, bottom: panelRect.bottom } : null,
		viewportBounds: { width: viewport.innerWidth, height: viewport.innerHeight },
		expectedHeight,
		heightMatchesPreference: !panelRect || Math.abs(panelRect.height - expectedHeight) <= 1,
		panelInsideViewport: !panelRect || (panelRect.left >= 0 && panelRect.right <= viewport.innerWidth && panelRect.top >= 0 && panelRect.bottom <= viewport.innerHeight),
		composerClearsPanel: !panelRect || !composerRect || composerRect.bottom <= panelRect.top - 16,
		resizeReachable: Boolean(resizeRect && resizeRect.width >= 1 && resizeRect.top <= panelRect!.top + 1),
		controlsAreHitTestable,
		noHorizontalOverflow: document.documentElement.scrollWidth <= viewport.innerWidth + 1
	};
});

export const exercise_terminal_surface = actions(() => {
	if (!terminalSurface.current.initialized) return ["Wait"];
	if (!terminalSurface.current.open && terminalSurface.current.showTerminalPoint) {
		return [{ Click: { name: "Open terminal chrome", point: terminalSurface.current.showTerminalPoint } }];
	}
	if (terminalSurface.current.panelHeight <= 221 && terminalSurface.current.resizePoint) {
		return [
			{ Click: { name: "Focus terminal resize handle", point: terminalSurface.current.resizePoint } },
			{ PressKey: { code: 38 } }
		];
	}
	return [
		{ SetViewport: { width: 960, height: 640 } },
		{ SetViewport: { width: 1024, height: 700 } },
		{ SetViewport: { width: 1280, height: 840 } },
		{ SetViewport: { width: 1440, height: 900 } }
	];
});

export const terminal_fixture_and_resize_are_exercised = eventually(() =>
	terminalSurface.current.open && terminalSurface.current.tabCount >= 2 && terminalSurface.current.panelHeight >= 244
).within(8, "seconds");

export const terminal_never_overflows_or_covers_the_composer = always(() =>
	!terminalSurface.current.open || (
		terminalSurface.current.heightMatchesPreference &&
		terminalSurface.current.panelInsideViewport &&
		terminalSurface.current.composerClearsPanel &&
		terminalSurface.current.noHorizontalOverflow
	)
);

export const terminal_toolbar_remains_operable = always(() =>
	!terminalSurface.current.open || (
		terminalSurface.current.resizeReachable &&
		terminalSurface.current.controlsAreHitTestable
	)
);
