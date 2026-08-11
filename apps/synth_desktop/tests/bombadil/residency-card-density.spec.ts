import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const residency = extract((state: any) => {
	const document = state.document;
	const card = document.querySelector<HTMLElement>('[data-testid="model-residency"]');
	const summary = card?.querySelector<HTMLElement>(".model-residency-summary") ?? null;
	const details = document.querySelector<HTMLElement>('[data-testid="model-residency-details"]');
	const rows = details ? [...details.querySelectorAll<HTMLElement>(":scope > div")] : [];
	const summaryRect = summary?.getBoundingClientRect() ?? null;
	const detailsRect = details?.getBoundingClientRect() ?? null;
	const rowsReadable = rows.every((row) => {
		const label = row.querySelector<HTMLElement>("span");
		const value = row.querySelector<HTMLElement>("strong");
		if (!label || !value) return false;
		const labelStyle = getComputedStyle(label);
		const valueStyle = getComputedStyle(value);
		const labelLine = Number.parseFloat(labelStyle.lineHeight) || Number.parseFloat(labelStyle.fontSize) * 1.2;
		const valueLine = Number.parseFloat(valueStyle.lineHeight) || Number.parseFloat(valueStyle.fontSize) * 1.2;
		const labelRect = label.getBoundingClientRect();
		const valueRect = value.getBoundingClientRect();
		return labelRect.height <= labelLine * 1.35
			&& valueRect.height <= valueLine * 1.35
			&& (!detailsRect || valueRect.right <= detailsRect.right + 1)
			&& labelRect.right <= valueRect.left - 6;
	});
	return {
		summaryPoint: summaryRect ? { x: summaryRect.left + summaryRect.width / 2, y: summaryRect.top + summaryRect.height / 2 } : null,
		expanded: Boolean(details),
		hasAllRows: rows.length === 3,
		rowsReadable,
		contained: !detailsRect || detailsRect.left >= 0 && detailsRect.right <= state.window.innerWidth
	};
});

export const expand_residency_and_fuzz_sidebar_widths = actions(() => {
	if (!residency.current.expanded && residency.current.summaryPoint) {
		return [{ Click: { name: "Expand loaded-model residency", point: residency.current.summaryPoint } }];
	}
	return [
		{ SetViewport: { width: 960, height: 640 } },
		{ SetViewport: { width: 1172, height: 768 } },
		{ SetViewport: { width: 1280, height: 840 } }
	];
});

export const expanded_residency_fixture_is_exercised = eventually(() =>
	residency.current.expanded && residency.current.hasAllRows
).within(8, "seconds");

export const residency_metrics_stay_single_line_aligned_and_contained = always(() =>
	!residency.current.expanded || (residency.current.rowsReadable && residency.current.contained)
);
