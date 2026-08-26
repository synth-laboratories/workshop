/**
 * Restore keyboard focus to a disclosure invoker after the disclosed surface
 * unmounts. Call after the close state commit; the microtask + animation frame
 * wait until the invoker is in the DOM. Skip if the user already moved focus.
 */
export function restoreFocusIfLost(selector: string): void {
	queueMicrotask(() => {
		requestAnimationFrame(() => {
			const active = document.activeElement;
			if (
				active instanceof HTMLElement &&
				active !== document.body &&
				active !== document.documentElement &&
				active.isConnected
			) {
				return;
			}
			document.querySelector<HTMLElement>(selector)?.focus();
		});
	});
}
