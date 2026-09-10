import { useEffect, useRef, useState } from "react";
import {
	applyDocumentZoom,
	DEFAULT_ZOOM_PERCENT,
	readStoredZoomPercent,
	stepZoomPercent,
	ZOOM_HUD_MS,
	zoomShortcutAction
} from "../runtime/appZoom";

/** Visible zoom state for Command/Ctrl + Plus/Minus/0. */
export function ZoomHud() {
	const [percent, setPercent] = useState(DEFAULT_ZOOM_PERCENT);
	const [visible, setVisible] = useState(false);
	const hideTimer = useRef<number | null>(null);

	useEffect(() => {
		const initial = readStoredZoomPercent();
		setPercent(applyDocumentZoom(initial));
		const flash = (next: number) => {
			setPercent(next);
			setVisible(true);
			if (hideTimer.current != null) window.clearTimeout(hideTimer.current);
			hideTimer.current = window.setTimeout(() => setVisible(false), ZOOM_HUD_MS);
		};
		const onKeyDown = (event: KeyboardEvent) => {
			const action = zoomShortcutAction(event);
			if (!action) return;
			event.preventDefault();
			const current = readStoredZoomPercent();
			const next =
				action === "reset"
					? DEFAULT_ZOOM_PERCENT
					: stepZoomPercent(current, action === "in" ? 1 : -1);
			flash(applyDocumentZoom(next));
		};
		window.addEventListener("keydown", onKeyDown);
		return () => {
			window.removeEventListener("keydown", onKeyDown);
			if (hideTimer.current != null) window.clearTimeout(hideTimer.current);
		};
	}, []);

	if (!visible) return null;
	return (
		<div className="zoom-hud" role="status" aria-live="polite" data-testid="zoom-indicator">
			Zoom {percent}%
		</div>
	);
}
