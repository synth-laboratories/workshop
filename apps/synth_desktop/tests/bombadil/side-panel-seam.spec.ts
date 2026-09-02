import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const seam = extract((state: any) => {
	const document = state.document;
	const panel = document.querySelector<HTMLElement>('[data-testid="workbench-side-panel"]');
	const handle = document.querySelector<HTMLElement>('.workbench.with-side-panel > [data-testid="pane-resize-handle"]');
	const fixture = [...document.querySelectorAll<HTMLElement>('[data-testid^="local-chat-"]')]
		.find((element) => element.textContent?.includes("Bombadil visual alignment"));
	const outputs = document.querySelector<HTMLElement>('[data-testid="resource-shelf-trigger"]');
	const point = (element: HTMLElement | undefined | null) => {
		if (!element) return null;
		const rect = element.getBoundingClientRect();
		return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
	};
	const panelBorderLeft = panel ? Number.parseFloat(getComputedStyle(panel).borderLeftWidth) : null;
	const dividerWidth = handle ? Number.parseFloat(getComputedStyle(handle, "::after").width) : null;
	return {
		panelOpen: Boolean(panel && handle),
		fixturePoint: point(fixture),
		outputsPoint: point(outputs),
		singlePaintedSeam: !panel || !handle ||
			(panelBorderLeft === 0 && dividerWidth !== null && dividerWidth >= 1)
	};
});

export const open_the_side_panel = actions(() => {
	if (seam.current.panelOpen) return ["Wait"];
	if (seam.current.outputsPoint) return [{ Click: { name: "Open Outputs", point: seam.current.outputsPoint } }];
	if (seam.current.fixturePoint) return [{ Click: { name: "Select side-panel fixture", point: seam.current.fixturePoint } }];
	return ["Wait"];
});

export const side_panel_opens = eventually(() => seam.current.panelOpen)
	.within(4, "seconds");

/** The resize handle is the sole owner of the divider. A panel border here
 * produces the ugly parallel-line gutter from the regression capture. */
export const side_panel_has_exactly_one_vertical_seam = always(() =>
	seam.current.singlePaintedSeam
);
