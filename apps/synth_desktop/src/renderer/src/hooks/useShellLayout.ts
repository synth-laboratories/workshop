import { useCallback, useEffect, useState } from "react";
import {
	getPreferences,
	loadPreferences,
	normalizeLayoutSnapshot,
	saveLayout,
	type DesktopPreferences
} from "../preferences";
import { restoreFocusIfLost } from "../runtime/restoreFocus";
import { fitPaneWidth } from "../runtime/layoutGeometry";

export type SidePanelTab = "visual" | "outputs" | "inference" | "trace" | "diagnostics" | "errors";

export type ShellLayoutState = {
	sidebarVisible: boolean;
	sidebarWidth: number;
	terminalOpen: boolean;
	viewportWidth: number;
	inventoryContainerWidth: number;
	sidePanelWidth: number;
	sidePanelOpen: boolean;
	sidePanelTab: SidePanelTab;
	containerPaneExpanded: boolean;
	setSidebarVisible: (visible: boolean) => void;
	setSidebarWidth: (width: number) => void;
	setTerminalOpen: (open: boolean | ((current: boolean) => boolean)) => void;
	setInventoryContainerWidth: (width: number) => void;
	setSidePanelWidth: (width: number) => void;
	setSidePanelOpen: (open: boolean | ((current: boolean) => boolean)) => void;
	setSidePanelTab: (tab: SidePanelTab) => void;
	setContainerPaneExpanded: (expanded: boolean) => void;
	persistLayoutSnapshot: (patch: Partial<DesktopPreferences["layout"]["last"]>) => void;
};

/**
 * Persisted chrome layout (sidebar / terminal / panes). Owns the related
 * useState cluster so App.tsx stays a wiring shell.
 */
export function useShellLayout(
	setPreferences: (next: DesktopPreferences) => void
): ShellLayoutState {
	const [sidePanelOpen, setSidePanelOpen] = useState(() => {
		if (window.localStorage.getItem("synth.inferenceRailDefaultV2") !== "1") {
			window.localStorage.setItem("synth.inferenceRailDefaultV2", "1");
			window.localStorage.setItem("synth.inferenceRailOpen", "1");
			return true;
		}
		return window.localStorage.getItem("synth.inferenceRailOpen") !== "0";
	});
	const [sidePanelTab, setSidePanelTab] = useState<SidePanelTab>("inference");
	const [inventoryContainerWidth, setInventoryContainerWidth] = useState(
		() => loadPreferences().layout.last.outputPaneWidth
	);
	const [sidePanelWidth, setSidePanelWidthState] = useState(() => {
		const raw = window.localStorage.getItem("synth.workbenchSidePanelWidth");
		const stored = raw === null ? Number.NaN : Number(raw);
		return Number.isFinite(stored) ? stored : 420;
	});
	const [terminalOpen, setTerminalOpen] = useState(
		() => loadPreferences().layout.last.bottomPanelVisible
	);
	const [sidebarVisible, setSidebarVisible] = useState(
		() => loadPreferences().layout.last.sidebarVisible
	);
	const [viewportWidth, setViewportWidth] = useState(() => window.innerWidth);
	const [sidebarWidth, setSidebarWidth] = useState(
		() => loadPreferences().layout.last.sidebarWidth
	);
	const [containerPaneExpanded, setContainerPaneExpanded] = useState(false);
	const fitInventoryWidth = useCallback((width: number) => fitPaneWidth({
		requested: width,
		viewportWidth,
		sidebarVisible,
		sidebarWidth,
		minPrimary: 260,
		minPane: 280,
		maxPane: 2400
	}), [sidebarVisible, sidebarWidth, viewportWidth]);
	const fitSidePanelWidth = useCallback((width: number) => fitPaneWidth({
		requested: width,
		viewportWidth,
		sidebarVisible,
		sidebarWidth,
		minPrimary: 380,
		minPane: 320,
		maxPane: 720,
		maxShare: 0.46
	}), [sidebarVisible, sidebarWidth, viewportWidth]);
	const setSidePanelWidth = useCallback((width: number) => {
		const next = fitSidePanelWidth(width);
		setSidePanelWidthState(next);
		window.localStorage.setItem("synth.workbenchSidePanelWidth", String(next));
	}, [fitSidePanelWidth]);

	useEffect(() => {
		const onResize = () => setViewportWidth(window.innerWidth);
		window.addEventListener("resize", onResize);
		return () => window.removeEventListener("resize", onResize);
	}, []);

	useEffect(() => {
		setInventoryContainerWidth((current) => fitInventoryWidth(current));
		setSidePanelWidthState((current) => fitSidePanelWidth(current));
	}, [fitInventoryWidth, fitSidePanelWidth]);

	useEffect(() => {
		const root = document.documentElement;
		const media = window.matchMedia("(max-width: 860px)");
		const syncCompactWorkbench = () => {
			root.classList.toggle("compact-workbench", media.matches);
		};
		syncCompactWorkbench();
		media.addEventListener("change", syncCompactWorkbench);
		return () => {
			media.removeEventListener("change", syncCompactWorkbench);
			root.classList.remove("compact-workbench");
		};
	}, []);

	const persistLayoutSnapshot = useCallback(
		(patch: Partial<DesktopPreferences["layout"]["last"]>) => {
			const current = getPreferences().layout.last;
			const next = normalizeLayoutSnapshot({ ...current, ...patch });
			const unchanged =
				next.sidebarVisible === current.sidebarVisible &&
				next.sidebarWidth === current.sidebarWidth &&
				next.outputPaneVisible === current.outputPaneVisible &&
				next.outputPaneWidth === current.outputPaneWidth &&
				next.visualsListWidth === current.visualsListWidth &&
				next.bottomPanelVisible === current.bottomPanelVisible &&
				next.bottomPanelHeight === current.bottomPanelHeight &&
				next.selectedConversationId === current.selectedConversationId &&
				next.selectedOutputTab === current.selectedOutputTab &&
				next.optimizers.selectedRunId === current.optimizers.selectedRunId;
			if ("sidebarVisible" in patch) setSidebarVisible(next.sidebarVisible);
			if ("sidebarWidth" in patch) setSidebarWidth(next.sidebarWidth);
			if ("outputPaneWidth" in patch) setInventoryContainerWidth(next.outputPaneWidth);
			if ("bottomPanelVisible" in patch) {
				const hiding = terminalOpen && !next.bottomPanelVisible;
				setTerminalOpen(next.bottomPanelVisible);
				if (hiding) restoreFocusIfLost('[data-testid="toggle-terminal"]');
			}
			if (!unchanged) setPreferences(saveLayout(next));
		},
		[setPreferences, terminalOpen]
	);

	return {
		sidebarVisible,
		sidebarWidth,
		terminalOpen,
		viewportWidth,
		inventoryContainerWidth,
		sidePanelWidth,
		sidePanelOpen,
		sidePanelTab,
		containerPaneExpanded,
		setSidebarVisible,
		setSidebarWidth,
		setTerminalOpen,
		setInventoryContainerWidth,
		setSidePanelWidth,
		setSidePanelOpen,
		setSidePanelTab,
		setContainerPaneExpanded,
		persistLayoutSnapshot
	};
}
