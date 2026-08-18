import { actions, always, eventually, extract } from "@antithesishq/bombadil";

/**
 * The chart family renders in the pane by displaying the host's own SVG
 * rendition — the same bytes `capture_review` photographs. Every other link in
 * that chain is covered by Rust tests and a live MCP round trip; this is the
 * one that only exists once a browser has mounted `ChartVisual`.
 *
 * Fixture is injected by tests/bombadil/run.mjs (`chartPaneVisual`).
 */
const evidence = extract((state: any) => {
	const document = state.document;
	const firstRun = document.querySelector<HTMLElement>('[data-testid="first-run-account-choice"] button');
	const firstRunRect = firstRun?.getBoundingClientRect();
	const chat = document.querySelector<HTMLElement>('[data-testid="local-chat-v02-grouped-visual-session"]');
	const chatRect = chat?.getBoundingClientRect();
	const chip = document.querySelector<HTMLElement>('[data-testid="artifact-chip-vis_w1_craftax"]');
	const chipRect = chip?.getBoundingClientRect();
	const pane = document.querySelector<HTMLElement>('[data-testid="visual-pane"]');
	const chart = document.querySelector<HTMLElement>('[data-testid="visual-chart"]');
	const image = chart?.querySelector<HTMLImageElement>(".chart-visual-stage img");
	const stage = chart?.querySelector<HTMLElement>(".chart-visual-stage");
	const status = chart?.querySelector<HTMLElement>(".chart-visual-status");
	const loading = chart?.querySelector<HTMLElement>(".visual-loading");
	const actionLabels = [...(chart?.querySelectorAll<HTMLElement>(".chart-visual-actions button") ?? [])]
		.map((node) => (node.textContent ?? "").trim());
	const chartRect = chart?.getBoundingClientRect();
	const imageRect = image?.getBoundingClientRect();
	const stageRect = stage?.getBoundingClientRect();
	const point = (rect: DOMRect | undefined) =>
		rect && rect.width > 0 && rect.height > 0
			? { x: Math.round(rect.left + rect.width / 2), y: Math.round(rect.top + rect.height / 2) }
			: null;
	return {
		firstRunPoint: point(firstRunRect),
		chatPoint: point(chatRect),
		chipPoint: point(chipRect),
		paneVisible: Boolean(pane && (pane.getBoundingClientRect().width ?? 0) > 0),
		chartVisible: Boolean(chart && (chartRect?.width ?? 0) > 0),
		// The pane must show the host's rendition, not a placeholder.
		renditionVisible: Boolean(
			image?.src?.startsWith("data:image/svg+xml;base64,")
			&& (imageRect?.width ?? 0) > 0
			&& (imageRect?.height ?? 0) > 0
		),
		stillLoading: Boolean(loading),
		statusLabel: (status?.textContent ?? "").trim(),
		hasAuthoringActions: ["Spec", "Copy spec", "Export SVG", "Retry"]
			.every((label) => actionLabels.includes(label)),
		// A chart is a document: it scrolls, it does not spill sideways.
		renditionWithinStage: Boolean(
			!chart
			|| !stage
			|| !stageRect
			|| (stage.scrollWidth <= Math.ceil(stageRect.width) + 1
				&& (imageRect?.width ?? 0) <= (chartRect?.width ?? 0) + 1)
		),
		noHorizontalOverflow: document.documentElement.scrollWidth <= state.window.innerWidth + 1
	};
});

export const open_the_chart_visual = actions(() => {
	if (evidence.current.firstRunPoint) {
		return [{ Click: { name: "Continue locally", point: evidence.current.firstRunPoint } }];
	}
	if (evidence.current.chatPoint && !evidence.current.chipPoint) {
		return [{ Click: { name: "Open the chart turn", point: evidence.current.chatPoint } }];
	}
	if (evidence.current.chipPoint && !evidence.current.paneVisible) {
		return [{ Click: { name: "Open the chart visual", point: evidence.current.chipPoint } }];
	}
	return ["Wait"];
});

export const the_chart_pane_opens = eventually(() =>
	evidence.current.chartVisible
).within(8, "seconds");

export const the_pane_displays_the_host_rendition = eventually(() =>
	evidence.current.renditionVisible && !evidence.current.stillLoading
).within(8, "seconds");

export const the_pane_names_itself_a_chart = eventually(() =>
	evidence.current.statusLabel === "CHART"
).within(8, "seconds");

export const the_pane_offers_its_authoring_actions = eventually(() =>
	evidence.current.hasAuthoringActions
).within(8, "seconds");

export const the_rendition_never_escapes_its_stage = always(() =>
	evidence.current.renditionWithinStage
);

export const the_chart_pane_never_overflows_the_page = always(() =>
	evidence.current.noHorizontalOverflow
);
