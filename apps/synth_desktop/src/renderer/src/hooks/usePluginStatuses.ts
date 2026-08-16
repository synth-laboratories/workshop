/**
 * App-level ownership of plugin registry status.
 *
 * One subscriber for the whole renderer. Previously the Optimizers page polled
 * `plugins.status("optimizers")` on a 750 ms interval for as long as it was
 * mounted, and every call runs `manager().refresh()` — a live sidecar probe.
 * The sidebar needs the same data on every screen, so a second poller would
 * have put that probe everywhere.
 *
 * Refresh happens on mount, on window focus, on the native `optimizer:status`
 * event, and — only while a phase is transitional — on a bounded timer.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import type { PluginStatus } from "../bridge/types";
import { bridges } from "../runtime/desktopBridge";

/** Slow enough not to be a probe loop, quick enough to follow a download. */
const TRANSITIONAL_POLL_MS = 1_500;

const TRANSITIONAL_PHASES = new Set([
	"downloading",
	"verifying",
	"starting",
	"stopping",
	"updating",
	"removing"
]);

export type PluginStatusesState = {
	/** Null until the first load resolves, or when the bridge is unavailable. */
	pluginStatuses: PluginStatus[] | null;
	refreshPluginStatuses: () => Promise<void>;
};

export function usePluginStatuses(): PluginStatusesState {
	const [pluginStatuses, setPluginStatuses] = useState<PluginStatus[] | null>(null);
	const mounted = useRef(true);

	const refreshPluginStatuses = useCallback(async () => {
		if (!bridges.plugins) return;
		try {
			const next = await bridges.plugins.list();
			if (mounted.current) setPluginStatuses(next);
		} catch {
			// A registry read failure must not blank the nav: keep the last
			// known statuses and let the destination page report the error.
		}
	}, []);

	useEffect(() => {
		mounted.current = true;
		void refreshPluginStatuses();

		const onFocus = () => void refreshPluginStatuses();
		window.addEventListener("focus", onFocus);
		const unlisten = bridges.plugins?.onStatusChanged?.(() => void refreshPluginStatuses());

		return () => {
			mounted.current = false;
			window.removeEventListener("focus", onFocus);
			unlisten?.();
		};
	}, [refreshPluginStatuses]);

	// Bounded fallback: a transitional phase has no terminal event of its own,
	// so follow it until it settles, then stop.
	const transitional = (pluginStatuses ?? []).some((status) => TRANSITIONAL_PHASES.has(status.phase));
	useEffect(() => {
		if (!transitional) return;
		const timer = window.setInterval(() => void refreshPluginStatuses(), TRANSITIONAL_POLL_MS);
		return () => window.clearInterval(timer);
	}, [transitional, refreshPluginStatuses]);

	return { pluginStatuses, refreshPluginStatuses };
}
