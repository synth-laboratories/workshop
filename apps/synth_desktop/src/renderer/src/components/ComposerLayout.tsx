import {
	createContext,
	useCallback,
	useContext,
	useMemo,
	useState,
	type ReactNode
} from "react";

type ComposerLayoutContextValue = {
	host: HTMLElement | null;
	registerHost: (host: HTMLElement | null) => void;
};

const ComposerLayoutContext = createContext<ComposerLayoutContextValue | null>(null);

/** Routes own composer geometry while the shell retains composer behavior. */
export function ComposerLayoutProvider({ children }: { children: ReactNode }) {
	const [host, setHost] = useState<HTMLElement | null>(null);
	const registerHost = useCallback((next: HTMLElement | null) => setHost(next), []);
	const value = useMemo(() => ({ host, registerHost }), [host, registerHost]);
	return <ComposerLayoutContext.Provider value={value}>{children}</ComposerLayoutContext.Provider>;
}

/** Route-local containing block for the floating composer. */
export function ComposerLayoutHost() {
	const context = useContext(ComposerLayoutContext);
	if (!context) throw new Error("ComposerLayoutHost requires ComposerLayoutProvider");
	return <div ref={context.registerHost} className="composer-layout-host" data-testid="composer-layout-host" />;
}

export function useComposerLayoutHost(): HTMLElement | null {
	const context = useContext(ComposerLayoutContext);
	if (!context) throw new Error("useComposerLayoutHost requires ComposerLayoutProvider");
	return context.host;
}
