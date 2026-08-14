import type { CSSProperties, ReactNode } from "react";
import type {
	ContainerDeployment,
	Session,
	VisualInstanceRecord,
	VisualRecord
} from "@synth/runtime-protocol";
import type { ArtifactRef, LandingState, LocalChat } from "./types/landing";
import type { AccountViewModel } from "./runtime/accountView";
import type { DeviceUsageSummary } from "./components/UsageSheet";
import type { DesktopPreferences, ToolActivityMode } from "./preferences";
import { applyPreferencesToDocument } from "./preferences";
import type { LagunaStatus, ModelPerformanceSummary, SynthAccountSummary, SynthBackendSettings } from "./bridge";
import type { InferenceMonitor } from "./components/InferencePanel";
import type { ApprovalMode, ApprovalPolicy, SandboxMode } from "./runtime/nativeCodex";
import { ChatTranscript, OutputsPanel, outputContainerIds } from "./components/ChatTranscript";
import { ContainerPane } from "./components/ContainerPane";
import { ConnectorsPage } from "./components/ConnectorsPage";
import { InferencePanel } from "./components/InferencePanel";
import { DataPage } from "./components/DataPage";
import { LandingPage } from "./components/LandingPage";
import { OptimizersPage } from "./components/OptimizersPage";
import { PaneResizeHandle } from "./components/PaneResizeHandle";
import { SettingsPage } from "./components/SettingsPage";
import { VisualPane } from "./components/VisualHost";
import { VisualsPage } from "./components/VisualsPage";
import { ReportsPage } from "./components/ReportsPage";
import { WorkbenchSidePanel } from "./components/WorkbenchSidePanel";
import { sessionIsLocalChat, sessionIsSync } from "./runtime/sessionView";
import { bridges } from "./runtime/desktopBridge";

export type MainView =
	| { kind: "landing" }
	| { kind: "chat"; chatId: string }
	| { kind: "sync"; sessionId: string }
	| { kind: "async"; sessionId: string }
	| { kind: "settings"; section?: "general" | "context" | "models" | "inference" | "voice" | "account" | "about" }
	| { kind: "connectors" }
	| { kind: "inventory" }
	| { kind: "visuals" }
	| { kind: "reports" }
	| { kind: "optimizers" };

export type MainRoutesProps = {
	view: MainView;
	setView: (view: MainView) => void;
	state: LandingState;
	sessions: Session[];
	selectedTargetId: string;
	onSelectTarget: (id: string) => void;
	activeChat: LocalChat | null;
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
	sidePanelTab: "outputs" | "inference";
	setSidePanelTab: (tab: "outputs" | "inference") => void;
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
	openChat: (chatId: string) => void;
	openVisualRecord: (visual: VisualInstanceRecord | VisualRecord) => void;
	toggleArtifact: (id: string | null) => void;
	toggleContainer: (id: string | null) => Promise<void>;
	probeOpenContainer: () => Promise<void>;
	controlActive: (kind: "approve" | "reject" | "cancel", payload?: Record<string, unknown>) => Promise<void>;
	setQueueAfterStop: (value: boolean) => void;
	promptsForConversationLength: (chatId: string) => number;
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
		state,
		sessions,
		selectedTargetId,
		onSelectTarget,
		activeChat,
		activeChatSession,
		activeChatRunning,
		activeChatWarmingUp,
		activeLocalModel,
		openArtifact,
		openArtifactId,
		openContainer,
		containerPaneExpanded,
		setContainerPaneExpanded,
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
		controlActive,
		setQueueAfterStop,
		promptsForConversationLength,
		onActivityModeChange,
		activeSessionId
	} = props;

	return (
		<>
			{view.kind === "settings" ? (
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
					onBack={() => setView({ kind: "landing" })}
					onSectionChange={(section) => setView({ kind: "settings", section })}
					onReloadLaguna={onReloadLaguna}
					lagunaPhase={laguna?.phase}
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
			) : null}

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

			{view.kind === "visuals" ? (
				<div className={`inventory-workbench${openArtifact ? " with-visual" : ""}`} style={{ "--visual-pane-width": `${inventoryContainerWidth}px` } as CSSProperties}>
					<VisualsPage
						onOpenVisual={openVisualRecord}
						onGoToChat={(sessionId) => {
							const session = sessions.find((item) => item.id === sessionId);
							if (!session) return;
							if (sessionIsLocalChat(session)) openChat(sessionId);
							else if (sessionIsSync(session)) setView({ kind: "sync", sessionId });
							else setView({ kind: "async", sessionId });
						}}
						onBack={() => setView({ kind: "landing" })}
						onCreate={() => {
							void (async () => {
								if (!bridges.visuals) {
									showToast("Visual registry requires Synth Desktop");
									return;
								}
								try {
									const templates = await bridges.visuals.listTemplates();
									const templateId = templates[0]?.id ?? "reward.breakdown.v1";
									const visual = await bridges.visuals.create({
										templateId,
										title: "New visual",
										bindings: {},
										sessionId: activeSessionId ?? undefined
									});
									openVisualRecord(visual);
									showToast(`Created visual · ${visual.title}`);
								} catch (reason) {
									showToast(String(reason));
								}
							})();
						}}
					/>
					{openArtifact ? (
						<><PaneResizeHandle value={inventoryContainerWidth} onChange={(width) => { setInventoryContainerWidth(width); persistLayoutSnapshot({ outputPaneWidth: width }); }} ariaLabel="Resize visual pane" /><VisualPane artifact={openArtifact} onClose={() => toggleArtifact(null)} /></>
					) : null}
				</div>
			) : null}

			{view.kind === "reports" ? (
				<div className="inventory-workbench">
					<ReportsPage onBack={() => setView({ kind: "landing" })} />
				</div>
			) : null}

			{view.kind === "optimizers" ? (
				<div className={`inventory-workbench${openArtifact ? " with-visual" : ""}`} style={{ "--visual-pane-width": `${inventoryContainerWidth}px` } as CSSProperties}>
					<OptimizersPage
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
									showToast(String(reason));
								}
							})();
						}}
						onBack={() => setView({ kind: "landing" })}
					/>
					{openArtifact ? (
						<><PaneResizeHandle value={inventoryContainerWidth} onChange={(width) => { setInventoryContainerWidth(width); persistLayoutSnapshot({ outputPaneWidth: width }); }} ariaLabel="Resize visual pane" /><VisualPane artifact={openArtifact} onClose={() => toggleArtifact(null)} /></>
					) : null}
				</div>
			) : null}

			{view.kind === "inventory" ? (
				<div
					className={`inventory-workbench${openArtifact ? " with-visual" : ""}${openContainer ? " with-container" : ""}${containerPaneExpanded ? " container-expanded" : ""}`}
					style={{ "--container-pane-width": `${inventoryContainerWidth}px` } as CSSProperties}
				>
					<DataPage
						onOpenVisual={openVisualRecord}
						onOpenContainer={(id) => void toggleContainer(id)}
						openContainerId={openContainer?.id ?? null}
						onBack={() => setView({ kind: "landing" })}
					/>
					{openArtifact ? (
						<><PaneResizeHandle value={inventoryContainerWidth} onChange={(width) => { setInventoryContainerWidth(width); persistLayoutSnapshot({ outputPaneWidth: width }); }} ariaLabel="Resize visual pane" /><VisualPane artifact={openArtifact} onClose={() => toggleArtifact(null)} /></>
					) : null}
					{openContainer ? (
						<>
							<PaneResizeHandle
								value={inventoryContainerWidth}
								onChange={(width) => {
									setInventoryContainerWidth(width);
									persistLayoutSnapshot({ outputPaneWidth: width });
								}}
							/>
							<ContainerPane
								container={openContainer}
								expanded={containerPaneExpanded}
								onExpandedChange={setContainerPaneExpanded}
								onProbe={() => void probeOpenContainer()}
								onClose={() => void toggleContainer(null)}
							/>
						</>
					) : null}
				</div>
			) : null}

			{view.kind === "landing" ? (
				<LandingPage
					state={state}
					selectedTargetId={selectedTargetId}
					onSelectTarget={onSelectTarget}
					onConfigureAccount={() => setView({ kind: "settings", section: "account" })}
					onConfigureModels={() => setView({ kind: "settings", section: "models" })}
					onResolveBilling={() => setUsageSheetOpen(true)}
				/>
			) : null}

	{view.kind === "chat" && activeChat ? (
				(() => {
				const visualPaneVisible = Boolean(openArtifact && (!showSidePanel || sidePanelCanSharePane));
				const containerPaneVisible = Boolean(openContainer && (!showSidePanel || sidePanelCanSharePane));
				return (
				<div
					className={`workbench${visualPaneVisible ? " with-visual" : ""}${containerPaneVisible ? " with-container" : ""}${containerPaneExpanded ? " container-expanded" : ""}${showSidePanel ? " with-side-panel" : ""}`}
					style={{ "--visual-pane-width": `${inventoryContainerWidth}px` } as CSSProperties}
				>
					<ChatTranscript
						chat={activeChat}
						openArtifactId={openArtifactId}
						onOpenArtifact={(id) => {
							if (showSidePanel && !sidePanelCanSharePane) {
								setSidePanelOpen(false);
								if (openArtifactId === id) return;
							}
							toggleArtifact(id);
						}}
						openContainerId={openContainer?.id ?? null}
						onOpenContainer={(id) => void toggleContainer(id)}
						onApprove={(approvalId) => void controlActive("approve", { approvalId })}
						onAlwaysAllow={(approvalId) =>
							void controlActive("approve", { approvalId, decision: "always" })
						}
						onReject={(approvalId) => void controlActive("reject", { approvalId })}
						running={activeChatRunning}
						warmingUp={activeChatWarmingUp}
						onStop={() => {
							setQueueAfterStop(promptsForConversationLength(activeChat.id) > 0);
							void controlActive("cancel");
						}}
						activityMode={preferences.toolActivity.mode}
						onActivityModeChange={onActivityModeChange}
						outputsOpen={showSidePanel && sidePanelTab === "outputs"}
						onToggleOutputs={() => {
							const next = !(showSidePanel && sidePanelTab === "outputs");
							setSidePanelTab("outputs");
							setSidePanelOpen(next);
						}}
					/>
					{visualPaneVisible && openArtifact ? (
						<>
							<PaneResizeHandle
								value={inventoryContainerWidth}
								minPrimary={showSidePanel ? 680 : 380}
								minSecondary={260}
								onChange={(width) => {
									setInventoryContainerWidth(width);
									persistLayoutSnapshot({ outputPaneWidth: width });
								}}
								ariaLabel="Resize visual pane"
							/>
							<VisualPane artifact={openArtifact} onClose={() => toggleArtifact(null)} />
						</>
					) : null}
					{containerPaneVisible && openContainer ? (
						<ContainerPane
							container={openContainer}
							expanded={containerPaneExpanded}
							onExpandedChange={setContainerPaneExpanded}
							onProbe={() => void probeOpenContainer()}
							onClose={() => void toggleContainer(null)}
						/>
					) : null}
					{showSidePanel ? (
						<WorkbenchSidePanel
							activeTabId={sidePanelTab}
							onTabChange={(tabId) => setSidePanelTab(tabId as "outputs" | "inference")}
							onClose={() => setSidePanelOpen(false)}
							tabs={[
								{
									id: "outputs",
									label: "Outputs",
									badge:
										outputContainerIds(activeChat).length + (activeChat.artifacts?.length ?? 0),
									content: (
										<OutputsPanel
											chat={activeChat}
											openArtifactId={openArtifactId}
											onOpenArtifact={(id) => {
												if (showSidePanel && !sidePanelCanSharePane) {
													setSidePanelOpen(false);
													if (openArtifactId === id) return;
												}
												toggleArtifact(id);
											}}
											openContainerId={openContainer?.id ?? null}
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
														observedPerformance={
															persistedPerformanceByTarget.get("local-laguna") ?? null
														}
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
					) : null}
				</div>
				);
				})()
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
