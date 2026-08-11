import { actions, always, eventually, extract } from "@antithesishq/bombadil";

function overlaps(a: DOMRect, b: DOMRect, padding = 1) {
	return !(a.right <= b.left + padding || b.right <= a.left + padding || a.bottom <= b.top + padding || b.bottom <= a.top + padding);
}

const containment = extract((state: any) => {
	const document = state.document;
	const optimizerPage = document.querySelector<HTMLElement>('[data-testid="optimizers-page"]');
	const openOptimizers = document.querySelector<HTMLElement>('[data-testid="open-optimizers"]');
	const toolbar = document.querySelector<HTMLElement>('[data-testid="optimizer-toolbar"]');
	const residency = document.querySelector<HTMLElement>('[data-testid="model-residency"]');
	const inventoryNav = document.querySelector<HTMLElement>('[data-testid="open-inventory"]');
	const toolbarRect = toolbar?.getBoundingClientRect() ?? null;
	const controls = toolbar
		? [...toolbar.querySelectorAll<HTMLElement>("input, select, button")].filter((element) => {
			const rect = element.getBoundingClientRect();
			return rect.width > 0 && rect.height > 0;
		})
		: [];
	const controlRects = controls.map((control) => control.getBoundingClientRect());
	const residencyRect = residency?.getBoundingClientRect() ?? null;
	const inventoryRect = inventoryNav?.getBoundingClientRect() ?? null;
	const openRect = openOptimizers?.getBoundingClientRect() ?? null;
	return {
		optimizerVisible: Boolean(optimizerPage),
		openOptimizerPoint: openRect ? { x: openRect.left + openRect.width / 2, y: openRect.top + openRect.height / 2 } : null,
		toolbarVisible: Boolean(toolbar),
		toolbarControlsContained: !toolbarRect || controlRects.every((rect) =>
			rect.left >= toolbarRect.left && rect.right <= toolbarRect.right + 1
		),
		toolbarControlsDoNotOverlap: controlRects.every((rect, index) =>
			controlRects.slice(index + 1).every((other) => !overlaps(rect, other, 0))
		),
		residencyVisible: Boolean(residency),
		residencyClearsNavigation: !residencyRect || !inventoryRect || !overlaps(residencyRect, inventoryRect, 0),
		noHorizontalOverflow: document.documentElement.scrollWidth <= state.window.innerWidth + 1
	};
});

export const open_optimizers_and_fuzz_shell_widths = actions(() => {
	if (!containment.current.optimizerVisible && containment.current.openOptimizerPoint) {
		return [{ Click: { name: "Open Optimizers", point: containment.current.openOptimizerPoint } }];
	}
	return [
		{ SetViewport: { width: 960, height: 640 } },
		{ SetViewport: { width: 1172, height: 768 } },
		{ SetViewport: { width: 1280, height: 840 } }
	];
});

export const shell_containment_fixture_is_exercised = eventually(() =>
	containment.current.optimizerVisible && containment.current.toolbarVisible && containment.current.residencyVisible
).within(8, "seconds");

/** CUA: the loaded-model card obscured the Inventory navigation row. */
export const model_residency_never_overlays_primary_navigation = always(() =>
	containment.current.residencyClearsNavigation
);

/** CUA: optimizer filters and launch actions clipped and painted through peers. */
export const optimizer_toolbar_controls_stay_contained_and_non_overlapping = always(() =>
	!containment.current.toolbarVisible || (
		containment.current.toolbarControlsContained
		&& containment.current.toolbarControlsDoNotOverlap
		&& containment.current.noHorizontalOverflow
	)
);
