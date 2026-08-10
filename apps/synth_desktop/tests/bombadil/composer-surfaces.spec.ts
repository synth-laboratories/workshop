import { actions, always, eventually, extract } from "@antithesishq/bombadil";

/**
 * The model picker and terminal are both layered surfaces. Exercise their
 * actual controls through viewport changes; a rectangle merely existing in
 * the DOM is not enough if another layer paints above it.
 */
const surfaces = extract((state: any) => {
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
	const menuPaintsOnTop = !menu || !menuRect || [0.18, 0.5, 0.82].every((ratio) => {
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
		menuPaintsOnTop,
		noHorizontalOverflow: document.documentElement.scrollWidth <= viewport.innerWidth + 1
	};
});

/** Open the two actual controls before Bombadil explores their layout states. */
export const exercise_model_picker_over_terminal = actions(() => {
	if (!surfaces.current.terminalOpen && surfaces.current.showTerminalPoint) {
		return [{ Click: { name: "Open terminal before picker fuzz", point: surfaces.current.showTerminalPoint } }];
	}
	if (!surfaces.current.menuOpen && surfaces.current.modelPoint) {
		return [{ Click: { name: "Open model picker above terminal", point: surfaces.current.modelPoint } }];
	}
	return [
		{ SetViewport: { width: 960, height: 640 } },
		{ SetViewport: { width: 1024, height: 700 } },
		{ SetViewport: { width: 1280, height: 840 } },
		{ SetViewport: { width: 1440, height: 900 } }
	];
});

export const picker_and_terminal_are_actually_exercised = eventually(() =>
	surfaces.current.terminalOpen && surfaces.current.menuOpen
).within(5, "seconds");

export const model_picker_never_leaves_the_viewport = always(() =>
	!surfaces.current.menuOpen || surfaces.current.menuInsideViewport
);

export const model_picker_never_disappears_behind_the_terminal = always(() =>
	!surfaces.current.menuOpen || surfaces.current.menuClearsTerminal
);

export const every_visible_picker_option_remains_hit_testable = always(() =>
	!surfaces.current.menuOpen || surfaces.current.menuPaintsOnTop
);

export const layered_surfaces_never_create_horizontal_overflow = always(() =>
	surfaces.current.noHorizontalOverflow
);
