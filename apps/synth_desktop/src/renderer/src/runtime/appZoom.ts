/** App-owned page zoom (RP-CUA-038). Command/Ctrl + Plus/Minus/0. */

export const ZOOM_STEPS = [75, 90, 100, 110, 125, 150, 175, 200] as const;
export const DEFAULT_ZOOM_PERCENT = 100;
export const ZOOM_STORAGE_KEY = "synth.workbenchZoomPercent";
export const ZOOM_HUD_MS = 1400;

export type ZoomDirection = 1 | -1;

export function clampZoomPercent(value: number): number {
	if (!Number.isFinite(value)) return DEFAULT_ZOOM_PERCENT;
	const rounded = Math.round(value);
	const min = ZOOM_STEPS[0];
	const max = ZOOM_STEPS[ZOOM_STEPS.length - 1];
	return Math.min(max, Math.max(min, rounded));
}

export function stepZoomPercent(current: number, direction: ZoomDirection): number {
	const clamped = clampZoomPercent(current);
	if (direction === 1) {
		return ZOOM_STEPS.find((step) => step > clamped) ?? ZOOM_STEPS[ZOOM_STEPS.length - 1];
	}
	for (let i = ZOOM_STEPS.length - 1; i >= 0; i -= 1) {
		if (ZOOM_STEPS[i] < clamped) return ZOOM_STEPS[i];
	}
	return ZOOM_STEPS[0];
}

export function zoomShortcutAction(event: Pick<KeyboardEvent, "key" | "code" | "metaKey" | "ctrlKey" | "altKey">): "in" | "out" | "reset" | null {
	if (!(event.metaKey || event.ctrlKey) || event.altKey) return null;
	if (event.key === "0" || event.code === "Digit0" || event.code === "Numpad0") return "reset";
	if (event.key === "=" || event.key === "+" || event.code === "Equal" || event.code === "NumpadAdd") return "in";
	if (event.key === "-" || event.key === "_" || event.code === "Minus" || event.code === "NumpadSubtract") return "out";
	return null;
}

export function applyDocumentZoom(percent: number): number {
	const next = clampZoomPercent(percent);
	if (typeof document === "undefined") return next;
	document.documentElement.style.zoom = next === DEFAULT_ZOOM_PERCENT ? "" : String(next / 100);
	try {
		window.sessionStorage.setItem(ZOOM_STORAGE_KEY, String(next));
	} catch {
		// Private mode / blocked storage must not disable zoom.
	}
	return next;
}

export function readStoredZoomPercent(): number {
	if (typeof window === "undefined") return DEFAULT_ZOOM_PERCENT;
	try {
		const raw = window.sessionStorage.getItem(ZOOM_STORAGE_KEY);
		if (!raw) return DEFAULT_ZOOM_PERCENT;
		return clampZoomPercent(Number(raw));
	} catch {
		return DEFAULT_ZOOM_PERCENT;
	}
}
