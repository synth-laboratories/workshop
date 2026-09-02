import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import type {
	ContainerDeployment,
	OptimizerRunRecord,
	Session,
	RuntimeEvent,
	VisualInstanceRecord,
	VisualRecord
} from "@synth/runtime-protocol";
import { publicError } from "./runtime/publicError";
import type { ArtifactRef, LandingState, LocalChat } from "./types/landing";
import type { AccountViewModel } from "./runtime/accountView";
import type { DeviceUsageSummary } from "./components/UsageSheet";
import type { DesktopPreferences, ToolActivityMode } from "./preferences";
import { applyPreferencesToDocument } from "./preferences";
import type { LagunaStatus, ModelPerformanceSummary, PluginPermission, PluginStatus, SynthAccountSummary, SynthBackendSettings } from "./bridge";
import type { LagunaPolicy } from "./bridge/types";
import type { ComputerUseView } from "./runtime/computerUse";
import type { InferenceMonitor } from "./components/InferencePanel";
import type { ApprovalMode, ApprovalPolicy, SandboxMode } from "./runtime/nativeCodex";
import { ChatTranscript, OutputsPanel, type TranscriptHistoryState } from "./components/ChatTranscript";
import { primaryVisualId, useChatOutputs } from "./hooks/useChatOutputs";
import { ContainerPane } from "./components/ContainerPane";
import { ConnectorsPage } from "./components/ConnectorsPage";
import { InferencePanel } from "./components/InferencePanel";
import { DataPage } from "./components/DataPage";
import { LandingPage } from "./components/LandingPage";
import { ComputerUsePage } from "./components/ComputerUsePage";
import { OptimizersPage } from "./components/OptimizersPage";
import { PaneResizeHandle } from "./components/PaneResizeHandle";
import { SettingsPage } from "./components/SettingsPage";
import { VisualPane } from "./components/VisualHost";
import { VisualsPage } from "./components/VisualsPage";
import { ReportsPage } from "./components/ReportsPage";
import { ExperimentsPage } from "./experiments/ExperimentsPage";
import { WorkbenchSidePanel } from "./components/WorkbenchSidePanel";
import type { SidePanelTab } from "./hooks/useShellLayout";
import { ResponsesTracePanel } from "./components/ResponsesTracePanel";
import { DiagnosticsPanel } from "./components/DiagnosticsPanel";
import { ErrorsLogsPanel } from "./components/ErrorsLogsPanel";
import { sessionIsLocalChat } from "./runtime/sessionView";
import { bridges, isDesktopApp } from "./runtime/desktopBridge";
import {
	openTraceReference,
	VISUAL_OPS_FOLLOW_EVENT,
	VISUAL_OPS_UNREACHABLE_EVENT,
	VISUAL_REFERENCE_ERROR_EVENT,
	VISUAL_REFERENCE_OPENED_EVENT,
	type VisualOpsFollowDetail
} from "./runtime/visualReferences";

export type MainView =
	| { kind: "landing" }
	| { kind: "chat"; chatId: string }
	| { kind: "sync"; sessionId: string }
	| { kind: "async"; sessionId: string }
	| { kind: "settings"; section?: "general" | "models" | "inference" | "context" | "voice" | "account" | "secrets" | "about" }
	| { kind: "connectors" }
	| { kind: "inventory" }
	| { kind: "visuals" }
	| { kind: "reports"; reportId?: string }
	| { kind: "experiments"; experimentId?: string }
	| { kind: "optimizers" }
	| { kind: "computer-use" };

const INVENTORY_ORIGIN_KINDS = new Set<MainView["kind"]>([
	"visuals",
	"experiments",
	"optimizers",
	"inventory",
	"settings",
	"reports"
]);

const REPORT_HASH_FRAGMENTS = new Set([
	"research-log",
	"findings",
	"methods",
	"outline",
	"experiment-records",
	"traces",
	"limitations",
	"claims",
	"review-comments",
	"results",
	"result"
]);

function isReportFragment(hash: string): boolean {
	const id = hash.startsWith("#") ? hash.slice(1) : hash;
	if (!id) return false;
	if (REPORT_HASH_FRAGMENTS.has(id)) return true;
	return id.startsWith("visual-");
}

function clearReportFragment(): void {
	if (typeof window === "undefined") return;
	if (!isReportFragment(window.location.hash)) return;
	window.history.replaceState(window.history.state, "", `${window.location.pathname}${window.location.search}`);
}

type OriginLayout = {
	sidePanelOpen: boolean;
	sidePanelTab: SidePanelTab;
};

type OriginFrame = {
	view: MainView;
	layout?: OriginLayout;
};

function isCloudDeskOrigin(view: MainView): boolean {
	return view.kind === "sync" || view.kind === "async";
}

export type MainRoutesProps = {
	view: MainView;
	computerUse: ComputerUseView;
	computerUseBusy: boolean;
	onInstallComputerUse: () => void;
	onRemoveComputerUse: () => void;
	onRefreshComputerUse: () => void;
	onOpenComputerUseSettings: (permission: PluginPermission) => void;
	onRevokeComputerUseApp: (bundleId: string) => void;
	setView: (view: MainView) => void;
	state: LandingState;
	sessions: Session[];
	selectedTargetId: string;
	onSelectTarget: (id: string) => void;
	lagunaAdapters: LagunaPolicy[];
	selectedLagunaAdapterId: string | null;
	onSelectLagunaAdapter: (checkpointId: string | null) => void;
	activeChat: LocalChat | null;
	eventsBySession: Record<string, RuntimeEvent[]>;
	activeChatSession: Session | undefined;
	activeChatRunning: boolean;
	activeChatWarmingUp: boolean;
	activeLocalModel: boolean;
	activeSessionId: string | null;
	openArtifact: ArtifactRef | null;
	openArtifactId: string | null;
	openContainer: ContainerDeployment | null;
	containerPaneExpanded: boolean;
	setContainerPaneExpanded: (expanded: boolean) => void;
	inventoryContainerWidth: number;
	setInventoryContainerWidth: (width: number) => void;
	persistLayoutSnapshot: (patch: Partial<DesktopPreferences["layout"]["last"]>) => void;
	showSidePanel: boolean;
	sidePanelCanSharePane: boolean;
	sidePanelTab: SidePanelTab;
	setSidePanelTab: (tab: SidePanelTab) => void;
	transcriptHistoryBySession: Record<string, TranscriptHistoryState>;
	loadOlderTranscript: () => void;
	setSidePanelOpen: (open: boolean) => void;
	inferenceMonitor: InferenceMonitor;
	persistedPerformanceByTarget: Map<string, ModelPerformanceSummary>;
	preferences: DesktopPreferences;
	setPreferences: (next: DesktopPreferences) => void;
	accountView: AccountViewModel;
	accountSummary: SynthAccountSummary | null;
	accountUsage: DeviceUsageSummary | null;
	backendSettings: SynthBackendSettings | null;
	laguna: LagunaStatus | null;
	onReloadLaguna: () => Promise<LagunaStatus>;
	openBilling: (action: "upgrade" | "manage") => Promise<void>;
	refreshAccountSummary: (force?: boolean) => void;
	setUsageSheetOpen: (open: boolean) => void;
	setSidebarVisible: (visible: boolean) => void;
	setSidebarWidth: (width: number) => void;
	setTerminalOpen: (open: boolean) => void;
	setApprovalMode: (mode: ApprovalMode) => void;
	setApprovalPolicy: (policy: ApprovalPolicy) => void;
	setSandboxMode: (mode: SandboxMode) => void;
	showToast: (message: string) => void;
	startOptimizerAgent: (title: string, prompt: string) => Promise<void>;
	pluginStatuses: readonly PluginStatus[] | null;
	refreshPluginStatuses: () => Promise<void>;
	openChat: (chatId: string) => void;
	openVisualRecord: (visual: VisualInstanceRecord | VisualRecord) => void;
	toggleArtifact: (id: string | null) => void;
	toggleContainer: (id: string | null) => Promise<void>;
	probeOpenContainer: () => Promise<void>;
	repairOpenContainer: () => Promise<void>;
	restartOpenContainer: () => Promise<void>;
	controlActive: (kind: "approve" | "reject" | "cancel", payload?: Record<string, unknown>) => Promise<void>;
	onActivityModeChange: (mode: ToolActivityMode) => void;
};

/**
 * Main pane route table. CloudDesk sync/async routes stay deliberately
 * unmounted (v0.1 removal contract); re-entry is a routing change only.
 */
export function MainRoutes(props: MainRoutesProps): ReactNode {
	const {
		view,
		setView,
		computerUse,
		computerUseBusy,
		onInstallComputerUse,
		onRemoveComputerUse,
		onRefreshComputerUse,
		onOpenComputerUseSettings,
		onRevokeComputerUseApp,
		state,
		sessions,
		selectedTargetId,
		onSelectTarget,
		lagunaAdapters,
		selectedLagunaAdapterId,
		onSelectLagunaAdapter,
		activeChat,
		eventsBySession,
		activeChatSession,
		activeChatRunning,
		activeChatWarmingUp,
		activeLocalModel,
		openArtifact,
		openArtifactId,
		openContainer,
		containerPaneExpanded,
		setContainerPaneExpanded,
		pluginStatuses,
		refreshPluginStatuses,
		inventoryContainerWidth,
		setInventoryContainerWidth,
		persistLayoutSnapshot,
		showSidePanel,
		sidePanelCanSharePane,
		sidePanelTab,
		setSidePanelTab,
		setSidePanelOpen,
		inferenceMonitor,
		persistedPerformanceByTarget,
		preferences,
		setPreferences,
		accountView,
		accountSummary,
		accountUsage,
		backendSettings,
		laguna,
		onReloadLaguna,
		openBilling,
		refreshAccountSummary,
		setUsageSheetOpen,
		setSidebarVisible,
		setSidebarWidth,
		setTerminalOpen,
		setApprovalMode,
		setApprovalPolicy,
		setSandboxMode,
		showToast,
		startOptimizerAgent,
		openChat,
		openVisualRecord,
		toggleArtifact,
		toggleContainer,
		probeOpenContainer,
		repairOpenContainer,
		restartOpenContainer,
		controlActive,
		onActivityModeChange,
		activeSessionId,
		transcriptHistoryBySession,
		loadOlderTranscript
	} = props;
	const [transcriptCollapsed, setTranscriptCollapsed] = useState(false);
	const [openVisualTabs, setOpenVisualTabs] = useState<ArtifactRef[]>([]);
	useEffect(() => {
		if (!showSidePanel) setTranscriptCollapsed(false);
	}, [showSidePanel]);
	useEffect(() => {
		if (!openArtifact) return;
		setOpenVisualTabs((current) => {
			const index = current.findIndex((artifact) => artifact.id === openArtifact.id);
			if (index < 0) return [...current, openArtifact];
			const next = [...current];
			next[index] = openArtifact;
			return next;
		});
	}, [openArtifact]);
	useEffect(() => {
		if (!isDesktopApp()) return;
		let disposed = false;
		let unlisten: (() => void) | undefined;
		void listen<{ instance?: string | null; view: string; runId?: string | null }>(
			"desktop:deep-link",
			(event) => {
				const route = event.payload;
				if (route.instance && route.instance !== document.documentElement.dataset.desktopInstance) {
					showToast(`This link targets Workshop instance ${route.instance}. Open it from the instance switcher.`);
					return;
				}
				if (route.runId) persistLayoutSnapshot({ optimizers: { selectedRunId: route.runId } });
				if (route.view === "optimizers" || route.runId) setView({ kind: "optimizers" });
				else if (route.view === "experiments") setView({ kind: "experiments" });
				else if (route.view === "visuals") setView({ kind: "visuals" });
				else setView({ kind: "landing" });
			}
		).then((stop) => {
			if (disposed) stop();
			else unlisten = stop;
		}).catch((reason) => showToast(publicError(reason)));
		return () => {
			disposed = true;
			unlisten?.();
		};
	}, [persistLayoutSnapshot, setView, showToast]);
	useEffect(() => {
		const opened = (event: Event) => openVisualRecord((event as CustomEvent<VisualRecord>).detail);
		const failed = (event: Event) => showToast((event as CustomEvent<string>).detail);
		window.addEventListener(VISUAL_REFERENCE_OPENED_EVENT, opened);
		window.addEventListener(VISUAL_REFERENCE_ERROR_EVENT, failed);
		return () => {
			window.removeEventListener(VISUAL_REFERENCE_OPENED_EVENT, opened);
			window.removeEventListener(VISUAL_REFERENCE_ERROR_EVENT, failed);
		};
	}, [openVisualRecord, showToast]);
	const chatOutputs = useChatOutputs(activeChat ?? { id: "", title: "", messages: [] });
	const openOwnedRun = (run: OptimizerRunRecord) => {
		const visualId = primaryVisualId(run);
		if (visualId) {
			if (showSidePanel && !sidePanelCanSharePane) {
				setSidePanelOpen(false);
				if (openArtifactId === visualId) return;
			}
			toggleArtifact(visualId);
			return;
		}
		void (async () => {
			if (!bridges.optimizers) return;
			try {
				const opened = await bridges.optimizers.openVisual(run.id);
				const openedId = primaryVisualId(opened);
				if (openedId) toggleArtifact(openedId);
			} catch (reason) {
				showToast(publicError(reason));
			}
		})();
	};
	useEffect(() => {
		const markUnreachable = (detail: VisualOpsFollowDetail) => {
			window.dispatchEvent(new CustomEvent(VISUAL_OPS_UNREACHABLE_EVENT, { detail }));
		};
		const follow = (event: Event) => {
			const detail = (event as CustomEvent<VisualOpsFollowDetail>).detail;
			if (!detail?.id || !detail.kind) return;
			if (detail.kind === "session") {
				const session = sessions.find((item) => item.id === detail.id);
				if (session && sessionIsLocalChat(session)) {
					openChat(detail.id);
					return;
				}
				markUnreachable(detail);
				return;
			}
			if (detail.kind === "run") {
				void (async () => {
					if (!bridges.optimizers) {
						markUnreachable(detail);
						return;
					}
					try {
						const run = await bridges.optimizers.get(detail.id);
						openOwnedRun(run);
						if (!primaryVisualId(run)) setView({ kind: "optimizers" });
					} catch {
						markUnreachable(detail);
					}
				})();
				return;
			}
			void openTraceReference(detail.id).then(
				(visual) => {
					window.dispatchEvent(new CustomEvent(VISUAL_REFERENCE_OPENED_EVENT, { detail: visual }));
				},
				() => markUnreachable(detail)
			);
		};
		window.addEventListener(VISUAL_OPS_FOLLOW_EVENT, follow);
		return () => window.removeEventListener(VISUAL_OPS_FOLLOW_EVENT, follow);
	}, [sessions, openChat, openOwnedRun, setView]);
	// Chat, Visuals, Experiments, Optimizers, Data, and Reports share one
	// workbench so VisualPane keeps expand, SSE, and seal state. Settings
	// joins that host only while a pane is open; otherwise it is full-page.
	const chatRoute = view.kind === "chat" && activeChat != null;
	const settingsWithPane = view.kind === "settings" && Boolean(openArtifact);
	const inventoryHost =
		view.kind === "visuals" ||
		view.kind === "experiments" ||
		view.kind === "optimizers" ||
		view.kind === "inventory" ||
		view.kind === "reports";
	const paneHost = inventoryHost || chatRoute || settingsWithPane;
	const inventoryOriginRef = useRef<MainView | null>(null);
	const originStackRef = useRef<OriginFrame[]>([]);
	const restoringOriginRef = useRef(false);
	const previousViewRef = useRef(view);
	if (previousViewRef.current.kind !== view.kind) {
		if (restoringOriginRef.current) {
			restoringOriginRef.current = false;
		} else if (INVENTORY_ORIGIN_KINDS.has(view.kind)) {
			const previous = previousViewRef.current;
			originStackRef.current.push({
				view: previous,
				layout: previous.kind === "chat"
					? { sidePanelOpen: showSidePanel, sidePanelTab }
					: undefined
			});
			inventoryOriginRef.current = previous;
		} else {
			originStackRef.current = [];
			inventoryOriginRef.current = null;
		}
	}
	previousViewRef.current = view;
	useEffect(() => {
		if (view.kind === "reports") return;
		clearReportFragment();
	}, [view.kind]);
	const leaveInventory = (fallbackOrigin: MainView | null) => {
		restoringOriginRef.current = true;
		const frame = originStackRef.current.pop() ?? (fallbackOrigin ? { view: fallbackOrigin } : null);
		inventoryOriginRef.current = originStackRef.current.at(-1)?.view ?? null;
		if (view.kind === "reports") clearReportFragment();
		const origin = frame?.view ?? fallbackOrigin;
		if (origin?.kind === "chat") {
			openChat(origin.chatId);
			if (frame?.layout) {
				setSidePanelOpen(frame.layout.sidePanelOpen);
				setSidePanelTab(frame.layout.sidePanelTab);
			}
			return;
		}
		if (origin && !isCloudDeskOrigin(origin)) {
			setView(origin);
			return;
		}
		setView({ kind: "landing" });
	};
	const leaveReports = () => {
		leaveInventory(inventoryOriginRef.current);
	};
	// Chat artifacts live in the same right-hand dock as Outputs and the
	// inspector tabs.  Keep the legacy standalone pane only as the compact
	// fallback (where the dock cannot fit) and for non-chat inventory views.
	const visualPaneVisible = Boolean(openArtifact && (!chatRoute || !showSidePanel));
	const openArtifactInDock = (id: string | null) => {
		if (id == null) {
			toggleArtifact(null);
			setSidePanelTab("outputs");
			return;
		}
		if (openArtifactId !== id) toggleArtifact(id);
		setSidePanelTab("visual");
		setSidePanelOpen(true);
	};
	const closeVisualTab = (id: string) => {
		const index = openVisualTabs.findIndex((artifact) => artifact.id === id);
		const remaining = openVisualTabs.filter((artifact) => artifact.id !== id);
		setOpenVisualTabs(remaining);
		if (openArtifactId !== id) return;
		const neighbor = remaining[Math.min(index, remaining.length - 1)] ?? null;
		if (neighbor) {
			toggleArtifact(neighbor.id);
			setSidePanelTab("visual");
			return;
		}
		openArtifactInDock(null);
	};
	const visualPaneContent = openArtifact ? (
		<VisualPane
			key="window-visual-host"
			artifact={openArtifact}
			onClose={() => {
				toggleArtifact(null);
				if (chatRoute && showSidePanel) setSidePanelTab("outputs");
			}}
		/>
	) : null;
	const chatContainerVisible = Boolean(chatRoute && openContainer && (!showSidePanel || sidePanelCanSharePane));
	const inventoryContainerVisible = view.kind === "inventory" && Boolean(openContainer);
	const resizeInventoryPane = (width: number) => {
		setInventoryContainerWidth(width);
		persistLayoutSnapshot({ outputPaneWidth: width });
	};
	const paneClassName = chatRoute
		? `workbench${visualPaneVisible ? " with-visual" : ""}${chatContainerVisible ? " with-container" : ""}${chatContainerVisible && containerPaneExpanded ? " container-expanded" : ""}${showSidePanel ? " with-side-panel" : ""}${transcriptCollapsed ? " transcript-collapsed" : ""}`
		: `inventory-workbench${visualPaneVisible ? " with-visual" : ""}${inventoryContainerVisible ? " with-container" : ""}${inventoryContainerVisible && containerPaneExpanded ? " container-expanded" : ""}`;

	const settingsPage = view.kind === "settings" ? (
		<SettingsPage
			account={{
				view: accountView,
				summary: accountSummary,
				deviceUsage: accountUsage,
				connection: backendSettings,
				onBilling: (action) => void openBilling(action),
				onRefresh: () => refreshAccountSummary(true),
				onOpenDeviceUsage: () => setView({ kind: "inventory" })
			}}
			key={view.section ?? "general"}
			onBack={() => leaveInventory(inventoryOriginRef.current)}
			onSectionChange={(section) => setView({ kind: "settings", section })}
			onReloadLaguna={onReloadLaguna}
			lagunaPhase={laguna?.phase}
			pluginStatuses={pluginStatuses}
			initialSection={view.section}
			preferences={preferences}
			onPreferencesChange={(next) => {
				setPreferences(next);
				applyPreferencesToDocument(next);
				setSidebarVisible(next.layout.last.sidebarVisible);
				setSidebarWidth(next.layout.last.sidebarWidth);
				setInventoryContainerWidth(next.layout.last.outputPaneWidth);
				setTerminalOpen(next.layout.last.bottomPanelVisible);
				setApprovalMode(next.approvalMode);
				setApprovalPolicy(next.approvalPolicy);
				setSandboxMode(next.sandboxMode);
			}}
		/>
	) : null;

	return (
		<>
			{view.kind === "settings" && !openArtifact ? settingsPage : null}

			{view.kind === "connectors" ? (
				<ConnectorsPage
					onBack={() => setView({ kind: "landing" })}
					onConfigure={(name) =>
						showToast(
							name === "Synth Containers" || name === "Synth Visuals"
								? `${name} is provisioned automatically for every agent`
								: `${name} setup is not installed in this build`
						)
					}
				/>
			) : null}

			{paneHost ? (
				<div
					key="window-pane-host"
					className={paneClassName}
					style={{ "--visual-pane-width": `${inventoryContainerWidth}px`, "--container-pane-width": `${inventoryContainerWidth}px`, "--side-panel-width": `${inventoryContainerWidth}px` } as CSSProperties}
				>
					{chatRoute && activeChat ? (
					<ChatTranscript
						chat={activeChat}
						events={eventsBySession[activeChat.id] ?? []}
						openArtifactId={openArtifactId}
						onOpenArtifact={openArtifactInDock}
						openContainerId={openContainer?.id ?? null}
						onOpenContainer={(id) => void toggleContainer(id)}
						onApprove={(approvalId, decision) => void controlActive("approve", { approvalId, decision })}
						onAlwaysAllow={(approvalId) =>
							void controlActive("approve", { approvalId, decision: "always" })
						}
						onReject={(approvalId) => void controlActive("reject", { approvalId })}
						running={activeChatRunning}
						warmingUp={activeChatWarmingUp}
						onAdvanced={() => {
							setSidePanelTab("trace");
							setSidePanelOpen(true);
						}}
						activityMode={preferences.toolActivity.mode}
						onActivityModeChange={onActivityModeChange}
						outputsOpen={showSidePanel && sidePanelTab === "outputs"}
						onToggleOutputs={() => {
							const next = !(showSidePanel && sidePanelTab === "outputs");
							setSidePanelTab("outputs");
							setSidePanelOpen(next);
						}}
						showMascot={preferences.appearance.showMascot}
						session={activeChatSession}
						historyState={transcriptHistoryBySession[activeChat.id]}
						onLoadOlder={loadOlderTranscript}
					/>
					) : null}
					{view.kind === "visuals" ? (
						<VisualsPage
							onOpenVisual={openVisualRecord}
							onGoToChat={(sessionId) => {
								const session = sessions.find((item) => item.id === sessionId);
								if (!session || !sessionIsLocalChat(session)) return;
								openChat(sessionId);
							}}
							onOpenReport={(reportId) => setView({ kind: "reports", reportId })}
							onBack={() => leaveInventory(inventoryOriginRef.current)}
							onCreate={() => {
								void (async () => {
									if (!bridges.visuals) {
										showToast("Visual registry requires Synth Desktop");
										return;
									}
									try {
										// The registry's first template is the chart template, whose
										// content is intentionally mandatory.  A generic “New visual”
										// action must create an immediately valid draft instead of
										// presenting that validation error before the user can choose a
										// template or add content.
										const templateId = "blank.canvas.v1";
										await bridges.visuals.getTemplate(templateId);
										const visual = await bridges.visuals.create({
											templateId,
											title: "New visual",
											bindings: {},
											sessionId: activeSessionId ?? undefined
										});
										openVisualRecord(visual);
										showToast(`Created visual · ${visual.title}`);
									} catch (reason) {
										showToast(publicError(reason));
									}
								})();
							}}
						/>
					) : null}
					{view.kind === "experiments" ? (
						<ExperimentsPage initialId={view.experimentId} onBack={() => leaveInventory(inventoryOriginRef.current)} />
					) : null}
					{view.kind === "optimizers" ? (
						<OptimizersPage
							sessionRef={
								inventoryOriginRef.current?.kind === "chat"
									? inventoryOriginRef.current.chatId
									: inventoryOriginRef.current?.kind === "sync" || inventoryOriginRef.current?.kind === "async"
										? inventoryOriginRef.current.sessionId
										: null
							}
							pluginStatuses={pluginStatuses}
							initialRunId={preferences.layout.last.optimizers.selectedRunId}
							onSelectedRunIdChange={(selectedRunId) => persistLayoutSnapshot({ optimizers: { selectedRunId } })}
							selectedContainerId={openContainer?.id ?? null}
							onRefreshPlugins={refreshPluginStatuses}
							accessibilityHidden={visualPaneVisible}
							onStartAgent={(guide) => startOptimizerAgent(`Plan a ${guide.name} optimization`, guide.prompt)}
							onOpenVisual={(visualId) => {
								void (async () => {
									if (!bridges.visuals) {
										showToast("Visual registry requires Synth Desktop");
										return;
									}
									try {
										const visual = await bridges.visuals.get(visualId);
										openVisualRecord(visual);
									} catch (reason) {
										showToast(publicError(reason));
									}
								})();
							}}
							onBack={() => leaveInventory(inventoryOriginRef.current)}
						/>
					) : null}
					{view.kind === "inventory" ? (
						<DataPage
							onOpenVisual={openVisualRecord}
							onOpenContainer={(id) => void toggleContainer(id)}
							openContainerId={openContainer?.id ?? null}
							onBack={() => leaveInventory(inventoryOriginRef.current)}
						/>
					) : null}
					{view.kind === "reports" ? (
						<ReportsPage initialReportId={view.reportId} onBack={leaveReports} />
					) : null}
					{view.kind === "settings" && openArtifact ? settingsPage : null}
					{visualPaneVisible && visualPaneContent ? (
						<>
							<PaneResizeHandle
								value={inventoryContainerWidth}
								minPrimary={chatRoute ? (showSidePanel ? 680 : 380) : 360}
								minSecondary={chatRoute ? 260 : 340}
								onChange={resizeInventoryPane}
								ariaLabel="Resize visual pane"
							/>
							{visualPaneContent}
						</>
					) : null}
					{chatContainerVisible && openContainer ? (
						<ContainerPane
							container={openContainer}
							expanded={containerPaneExpanded}
							onExpandedChange={setContainerPaneExpanded}
							onProbe={() => void probeOpenContainer()}
							onRestart={() => void restartOpenContainer()}
							onRepair={() => void repairOpenContainer()}
							onClose={() => void toggleContainer(null)}
						/>
					) : null}
					{inventoryContainerVisible && openContainer ? (
						<>
							<PaneResizeHandle value={inventoryContainerWidth} onChange={resizeInventoryPane} />
							<ContainerPane
								container={openContainer}
								expanded={containerPaneExpanded}
								onExpandedChange={setContainerPaneExpanded}
								onProbe={() => void probeOpenContainer()}
								onRestart={() => void restartOpenContainer()}
								onRepair={() => void repairOpenContainer()}
								onClose={() => void toggleContainer(null)}
							/>
						</>
					) : null}
					{chatRoute && showSidePanel && activeChat ? (
						<>
							<PaneResizeHandle
								value={inventoryContainerWidth}
								minPrimary={380}
								minSecondary={260}
								onChange={resizeInventoryPane}
								allowPrimaryCollapse
								primaryCollapsed={transcriptCollapsed}
								onPrimaryCollapsedChange={setTranscriptCollapsed}
								ariaLabel="Resize workbench side panel"
							/>
							<WorkbenchSidePanel
							activeTabId={sidePanelTab === "visual" && openArtifactId ? `visual:${openArtifactId}` : sidePanelTab}
							onTabChange={(tabId) => {
								if (tabId.startsWith("visual:")) {
									const visualId = tabId.slice("visual:".length);
									if (openArtifactId !== visualId) toggleArtifact(visualId);
									setSidePanelTab("visual");
									return;
								}
								if (
									tabId === "outputs"
									|| tabId === "inference"
									|| tabId === "trace"
									|| tabId === "diagnostics"
									|| tabId === "errors"
								) {
									setSidePanelTab(tabId);
								}
							}}
							onClose={() => setSidePanelOpen(false)}
							tabs={[
								...openVisualTabs.map((artifact) => ({
										id: `visual:${artifact.id}`,
										label: artifact.displayName?.trim() || artifact.title || "Visual",
										title: artifact.title || artifact.displayName || "Visual",
										content: (
											<VisualPane
												key={`dock-visual-${artifact.id}`}
												artifact={artifact}
												onClose={() => closeVisualTab(artifact.id)}
											/>
										),
										kind: "document" as const,
										onClose: () => closeVisualTab(artifact.id)
									})),
								{
									id: "outputs",
									label: "Outputs",
									badge: chatOutputs.count,
									content: (
										<OutputsPanel
											chat={activeChat}
											openArtifactId={openArtifactId}
											onOpenArtifact={openArtifactInDock}
											openContainerId={openContainer?.id ?? null}
											onOpenContainer={(id) => void toggleContainer(id)}
											onOpenReport={(reportId) => setView({ kind: "reports", reportId })}
											onOpenRun={openOwnedRun}
										/>
									)
								},
								{
									id: "trace",
									label: "Advanced",
									content: <ResponsesTracePanel sessionId={activeChat.id} running={activeChatRunning} />
								},
								{
									id: "diagnostics",
									label: "Diagnostics",
									content: (
										<DiagnosticsPanel
											sessionId={activeChat.id}
											visualId={openArtifact?.visualId ?? openArtifact?.id ?? null}
											pluginStatuses={pluginStatuses}
											lagunaPhase={laguna?.phase}
											onOpenVisual={(id) => toggleArtifact(id)}
											onOpenContainer={(id) => void toggleContainer(id)}
											onOpenOptimizer={() => setView({ kind: "optimizers" })}
											onOpenTrace={() => setView({ kind: "inventory" })}
										/>
									)
								},
								{
									id: "errors",
									label: "Failures",
									content: (
										<ErrorsLogsPanel
											sessionId={activeChat.id}
											onOpenContainer={(id) => void toggleContainer(id)}
										/>
									)
								},
								...(activeLocalModel
									? [
											{
												id: "inference",
												label: "Inference",
												content: (
													<InferencePanel
														visible
														monitor={inferenceMonitor}
														status={laguna}
														observedPerformance={
															persistedPerformanceByTarget.get("local-laguna") ?? null
														}
														selectedModel={activeChatSession?.target.kind === "local" ? activeChatSession.target.model : null}
														turnRunning={Boolean(
															activeChatRunning && activeChatSession?.target.kind === "local"
														)}
														warmingUp={activeChatWarmingUp}
														onOpenSettings={() =>
															setView({ kind: "settings", section: "inference" })
														}
													/>
												)
											}
										]
									: [])
							]}
							/>
						</>
					) : null}
				</div>
			) : null}

			{view.kind === "computer-use" ? (
				<div className="inventory-workbench">
					<ComputerUsePage
						status={computerUse.status}
						allowedApps={computerUse.allowedApps}
						busy={computerUseBusy}
						onBack={() => setView({ kind: "landing" })}
						onInstall={onInstallComputerUse}
						onRemove={onRemoveComputerUse}
						onRefresh={onRefreshComputerUse}
						onOpenSettings={onOpenComputerUseSettings}
						onRevokeApp={onRevokeComputerUseApp}
					/>
				</div>
			) : null}

			{view.kind === "landing" ? (
				<LandingPage
					state={state}
					selectedTargetId={selectedTargetId}
					onSelectTarget={onSelectTarget}
					lagunaAdapters={lagunaAdapters}
					selectedLagunaAdapterId={selectedLagunaAdapterId}
					onSelectLagunaAdapter={onSelectLagunaAdapter}
					onConfigureAccount={() => setView({ kind: "settings", section: "account" })}
					onConfigureModels={() => setView({ kind: "settings", section: "models" })}
					onResolveBilling={() => setUsageSheetOpen(true)}
				/>
			) : null}

			{/*
			 * v0.1 removal contract: the CloudDesk sync/async routes are the
			 * Intern surface and stay unmounted. components/CloudDesk.tsx is
			 * retained dormant so v0.2 re-entry is a routing change, not a
			 * rewrite.
			 */}
		</>
	);
}
