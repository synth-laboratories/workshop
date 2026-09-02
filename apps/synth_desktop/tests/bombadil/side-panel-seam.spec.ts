import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const seam = extract((state: any) => {
	const document = state.document;
	const panel = document.querySelector<HTMLElement>('[data-testid="workbench-side-panel"]');
	const handle = document.querySelector<HTMLElement>('.workbench.with-side-panel > [data-testid="pane-resize-handle"]');
	const fixture = [...document.querySelectorAll<HTMLElement>('[data-testid^="local-chat-"]')]
		.find((element) => element.textContent?.includes("Bombadil visual alignment"));
	const outputs = document.querySelector<HTMLElement>('[data-testid="resource-shelf-trigger"]');
	const inferenceTab = [...document.querySelectorAll<HTMLElement>('[role="tab"]')]
		.find((element) => element.textContent?.trim() === "Inference");
	const advanced = document.querySelector<HTMLElement>('[data-testid="inference-advanced-summary"]');
	const activity = document.querySelector<HTMLElement>('[data-testid="inference-activity"]');
	const fingerprint = (element: HTMLElement | null | undefined) => element ? {
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
		inferencePoint: point(inferenceTab),
		inferenceFingerprint: fingerprint(inferenceTab),
		advancedVisible: Boolean(advanced && activity),
		advancedAligned: !advanced || !activity ||
			Math.abs(advanced.getBoundingClientRect().left - activity.getBoundingClientRect().left) <= 1,
		singlePaintedSeam: !panel || !handle ||
			(panelBorderLeft === 0 && dividerWidth !== null && dividerWidth >= 1)
	};
});

export const open_the_side_panel = actions(() => {
	if (seam.current.panelOpen && seam.current.inferencePoint && seam.current.inferenceFingerprint && !seam.current.advancedVisible) {
		return [{ Click: { fingerprint: seam.current.inferenceFingerprint, point: seam.current.inferencePoint } }];
	}
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

export const inference_advanced_marker_aligns_with_content_border = eventually(() =>
	seam.current.advancedVisible && seam.current.advancedAligned
).within(4, "seconds");
