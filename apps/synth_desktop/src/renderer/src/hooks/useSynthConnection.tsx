import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { SynthSignInBegin } from "../bridge";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";

const DEFAULT_POLL_INTERVAL_S = 4;

export type SynthConnectionState =
	| { kind: "idle" }
	| { kind: "opening_browser" }
	| { kind: "awaiting_approval"; begin: SynthSignInBegin }
	| { kind: "connected" }
	| { kind: "expired"; message: string }
	| { kind: "failed"; message: string };

type SynthConnection = {
	state: SynthConnectionState;
	start: () => Promise<void>;
	reopenBrowser: () => Promise<void>;
	cancel: () => Promise<void>;
	dismiss: () => void;
};

const Context = createContext<SynthConnection | null>(null);

function announceAccountChange(apiKeyConfigured: boolean) {
	window.dispatchEvent(new CustomEvent("synth:account-changed", {
		detail: { apiKeyConfigured }
	}));
}

export function SynthConnectionProvider({ children }: { children: ReactNode }) {
	const [state, setState] = useState<SynthConnectionState>({ kind: "idle" });
	const timerRef = useRef<number | null>(null);
	const generationRef = useRef(0);

	const stopPolling = useCallback(() => {
		if (timerRef.current !== null) {
			window.clearTimeout(timerRef.current);
			timerRef.current = null;
		}
	}, []);

	const schedulePoll = useCallback((delayS: number, generation: number) => {
		stopPolling();
		timerRef.current = window.setTimeout(async () => {
			if (generation !== generationRef.current) return;
			try {
				const result = await bridges.account?.pollSignIn();
				if (!result || generation !== generationRef.current) return;
				if (result.status === "active") {
					stopPolling();
					setState({ kind: "connected" });
					// `active` is returned only after the native host stored the key
					// and reloaded its runtime, so it is the authoritative transition.
					announceAccountChange(true);
				} else if (result.status === "expired") {
					stopPolling();
					setState({ kind: "expired", message: result.reason });
				} else {
					schedulePoll(result.retryInS ?? DEFAULT_POLL_INTERVAL_S, generation);
				}
			} catch (reason) {
				stopPolling();
				setState({ kind: "failed", message: publicError(reason) });
			}
		}, Math.max(1, delayS) * 1000);
	}, [stopPolling]);

	const begin = useCallback(async () => {
		if (!bridges.account) {
			setState({ kind: "failed", message: "Connecting Synth requires the Workshop desktop app." });
			return;
		}
		const generation = ++generationRef.current;
		stopPolling();
		setState({ kind: "opening_browser" });
		try {
			const next = await bridges.account.beginSignIn();
			if (generation !== generationRef.current) return;
			setState({ kind: "awaiting_approval", begin: next });
			schedulePoll(next.intervalS ?? DEFAULT_POLL_INTERVAL_S, generation);
		} catch (reason) {
			if (generation !== generationRef.current) return;
			setState({ kind: "failed", message: publicError(reason) });
		}
	}, [schedulePoll, stopPolling]);

	const cancel = useCallback(async () => {
		generationRef.current += 1;
		stopPolling();
		setState({ kind: "idle" });
		await bridges.account?.cancelSignIn();
	}, [stopPolling]);

	useEffect(() => () => {
		generationRef.current += 1;
		stopPolling();
	}, [stopPolling]);

	useEffect(() => {
		const onAccountChanged = (event: Event) => {
			const configured = (event as CustomEvent<{ apiKeyConfigured?: boolean }>).detail?.apiKeyConfigured;
			if (configured === false) setState({ kind: "idle" });
		};
		window.addEventListener("synth:account-changed", onAccountChanged);
		return () => window.removeEventListener("synth:account-changed", onAccountChanged);
	}, []);

	const value = useMemo<SynthConnection>(() => ({
		state,
		start: begin,
		reopenBrowser: begin,
		cancel,
		dismiss: () => {
			if (state.kind === "connected" || state.kind === "expired" || state.kind === "failed") {
				setState({ kind: "idle" });
			}
		}
	}), [begin, cancel, state]);

	return <Context.Provider value={value}>{children}</Context.Provider>;
}

export function useSynthConnection(): SynthConnection {
	const value = useContext(Context);
	if (!value) throw new Error("useSynthConnection must be used inside SynthConnectionProvider");
	return value;
}
