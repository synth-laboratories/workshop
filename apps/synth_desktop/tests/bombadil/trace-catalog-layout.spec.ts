import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const traceLayout = extract((state: any) => {
	const document = state.document;
	const inventory = document.querySelector<HTMLElement>('[data-testid="inventory-page"]');
	const traces = document.querySelector<HTMLElement>('[data-testid="inventory-traces"]');
	const openInventory = document.querySelector<HTMLElement>('[data-testid="open-inventory"]');
	const tracesTab = document.querySelector<HTMLElement>('[data-testid="inventory-tab-traces"]');
	const rect = (element: HTMLElement | null) => element?.getBoundingClientRect() ?? null;
	const point = (element: HTMLElement | null) => {
		const value = rect(element);
		return value ? { x: value.left + value.width / 2, y: value.top + value.height / 2 } : null;
	};
	const panelRect = rect(traces);
	const inspectActions = [...document.querySelectorAll<HTMLElement>('[data-testid^="open-trace-"]')];
	return {
		inventoryVisible: Boolean(inventory),
		tracesVisible: Boolean(traces),
		openInventoryPoint: point(openInventory),
		tracesTabPoint: point(tracesTab),
		hasTraceRows: inspectActions.length > 0,
		actionsContained: inspectActions.every((action) => {
			const actionRect = action.getBoundingClientRect();
			return Boolean(panelRect
				&& actionRect.left >= panelRect.left
				&& actionRect.right <= panelRect.right + 1
				&& actionRect.right <= state.window.innerWidth - 8);
		}),
		noHorizontalOverflow: document.documentElement.scrollWidth <= state.window.innerWidth + 1
	};
});

export const open_trace_catalog_and_fuzz_supported_widths = actions(() => {
	if (!traceLayout.current.inventoryVisible && traceLayout.current.openInventoryPoint) {
		return [{ Click: { name: "Open Inventory", point: traceLayout.current.openInventoryPoint } }];
	}
	if (!traceLayout.current.tracesVisible && traceLayout.current.tracesTabPoint) {
		return [{ Click: { name: "Open Traces catalog", point: traceLayout.current.tracesTabPoint } }];
	}
	return [
		{ SetViewport: { width: 960, height: 640 } },
		{ SetViewport: { width: 1172, height: 768 } },
		{ SetViewport: { width: 1280, height: 840 } }
	];
});

export const populated_trace_catalog_is_exercised = eventually(() =>
	traceLayout.current.tracesVisible && traceLayout.current.hasTraceRows
).within(8, "seconds");

/** CUA 2026-08-10: every Inspect trace button was clipped at the right edge. */
export const trace_catalog_keeps_inspect_actions_inside_the_viewport = always(() =>
	!traceLayout.current.tracesVisible || (
		traceLayout.current.actionsContained && traceLayout.current.noHorizontalOverflow
	)
);
