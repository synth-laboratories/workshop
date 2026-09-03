export type PaneFit = {
	requested: number;
	viewportWidth: number;
	sidebarVisible: boolean;
	sidebarWidth: number;
	minPrimary: number;
	minPane: number;
	maxPane: number;
	maxShare?: number;
};

/** Fit a persisted pane width to the space that actually exists right now. */
export function fitPaneWidth({
	requested,
	viewportWidth,
	sidebarVisible,
	sidebarWidth,
	minPrimary,
	minPane,
	maxPane,
	maxShare
}: PaneFit): number {
	const usable = Math.max(0, viewportWidth - (sidebarVisible ? sidebarWidth : 0) - 8);
	let upper = Math.min(maxPane, Math.max(minPane, usable - minPrimary));
	if (maxShare !== undefined) upper = Math.min(upper, Math.max(minPane, Math.floor(usable * maxShare)));
	return Math.round(Math.min(upper, Math.max(minPane, requested)));
}
