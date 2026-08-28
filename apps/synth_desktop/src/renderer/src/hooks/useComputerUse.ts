/**
 * Computer Use state for the renderer.
 *
 * Mirrors usePluginStatuses: refresh on mount and on window focus, keep the
 * last known value when a read fails so the nav does not blank, and let the
 * destination page report the error.
 *
 * There is no polling. Permission state changes only when a person changes it
 * in System Settings, which means they left the app and came back — and focus
 * is exactly that signal.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import type { PluginPermission } from "../bridge/types";
import { bridges } from "../runtime/desktopBridge";
import { EMPTY_VIEW, type ComputerUseView } from "../runtime/computerUse";

export type ComputerUseState = {
	computerUse: ComputerUseView;
	computerUseBusy: boolean;
	refreshComputerUse: () => Promise<void>;
	installComputerUse: () => Promise<void>;
	removeComputerUse: () => Promise<void>;
	revokeComputerUseApp: (bundleId: string) => Promise<void>;
	openComputerUseSettings: (permission: PluginPermission) => Promise<void>;
	/** Last failure, for the page to render. Cleared by the next success. */
	computerUseError: string | null;
};

export function useComputerUse(sessionId: string | null): ComputerUseState {
	const [computerUse, setComputerUse] = useState<ComputerUseView>(EMPTY_VIEW);
	const [computerUseBusy, setBusy] = useState(false);
	const [computerUseError, setError] = useState<string | null>(null);
	const mounted = useRef(true);

	const refreshComputerUse = useCallback(async () => {
		if (!bridges.computerUse) return;
		try {
			const next = await bridges.computerUse.status(sessionId);
			if (!mounted.current) return;
			setComputerUse({ status: next.status, allowedApps: next.allowedApps });
			setError(null);
		} catch (reason) {
			if (mounted.current) setError(String(reason));
		}
	}, [sessionId]);

	// Every mutation follows the same shape: mark busy, do the thing, re-read
	// the truth from the host. The page never derives state from what it just
	// asked for — install can succeed and still leave the plugin unusable
	// because macOS has granted nothing yet.
	const run = useCallback(
		async (operation: () => Promise<unknown>) => {
			if (!bridges.computerUse) return;
			setBusy(true);
			try {
				await operation();
				setError(null);
			} catch (reason) {
				if (mounted.current) setError(String(reason));
			} finally {
				await refreshComputerUse();
				if (mounted.current) setBusy(false);
			}
		},
		[refreshComputerUse]
	);

	const installComputerUse = useCallback(
		() => run(() => bridges.computerUse!.install()),
		[run]
	);
	const removeComputerUse = useCallback(
		() => run(() => bridges.computerUse!.remove()),
		[run]
	);
	const revokeComputerUseApp = useCallback(
		(bundleId: string) => run(() => bridges.computerUse!.revokeApp(bundleId)),
		[run]
	);
	const openComputerUseSettings = useCallback(
		(permission: PluginPermission) => run(() => bridges.computerUse!.openSettings(permission.id)),
		[run]
	);

	useEffect(() => {
		mounted.current = true;
		void refreshComputerUse();
		// Granting a permission means leaving for System Settings and coming
		// back, so focus is the moment the answer can have changed.
		const onFocus = () => void refreshComputerUse();
		window.addEventListener("focus", onFocus);
		return () => {
			mounted.current = false;
			window.removeEventListener("focus", onFocus);
		};
	}, [refreshComputerUse]);

	return {
		computerUse,
		computerUseBusy,
		computerUseError,
		refreshComputerUse,
		installComputerUse,
		removeComputerUse,
		revokeComputerUseApp,
		openComputerUseSettings
	};
}
