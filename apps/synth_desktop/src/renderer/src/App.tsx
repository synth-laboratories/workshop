import { useEffect, useMemo } from "react";
import { formatTps } from "./components/InferencePanel";
import { AppTitlebar, type TabCopyItem } from "./components/AppTitlebar";
import { AppOverlays } from "./components/AppOverlays";
import { ComposerDock } from "./components/ComposerDock";
import { ManderLabGate } from "./components/mander";
import { Sidebar } from "./components/Sidebar";
import { TerminalPanel } from "./components/TerminalPanel";
import { useAppController } from "./hooks/useAppController";
import {
	archiveConversation,
	pinConversation,
	renameConversation,
	promptsForConversation,
	setToolActivityMode
} from "./preferences";
import { publicError } from "./runtime/publicError";
import { conversationMarkdown } from "./runtime/chatCopy";
import { copyText } from "./runtime/clipboard";
import { eventsToMessages } from "./runtime/sessionView";
import { MainRoutes } from "./routes";
import { bridges } from "./runtime/desktopBridge";

/** Shell + wiring only — orchestration lives in useAppController / ComposerDock. */
export default function App() {
	const c = useAppController();
	const tabCopyItems = useMemo<TabCopyItem[]>(() => {
		if (c.view.kind !== "chat" || !c.activeSessionId) return [];
		const messages = eventsToMessages(c.eventsBySession[c.activeSessionId] ?? []);
		const items: TabCopyItem[] = [];
		if (c.terminalWorkspaceRoot?.trim()) {
			items.push({ id: "working-directory", label: "Copy working directory", successMessage: "Working directory copied", value: c.terminalWorkspaceRoot });
		}
		items.push(
			{ id: "session-id", label: "Copy session ID", successMessage: "Session ID copied", value: c.activeSessionId },
			{ id: "markdown", label: "Copy as Markdown", successMessage: "Markdown copied", value: conversationMarkdown(c.tabLabel, messages) }
		);
		return items;
	}, [c.activeSessionId, c.eventsBySession, c.tabLabel, c.terminalWorkspaceRoot, c.view.kind]);

	useEffect(() => {
		const openReviewSurface = () => c.setView({ kind: "visuals" });
		window.addEventListener("synth:visual-review-capture", openReviewSurface);
		if ((window as Window & { __synthVisualReviewCapture?: { active?: boolean } }).__synthVisualReviewCapture?.active) {
			openReviewSurface();
		}
		return () => window.removeEventListener("synth:visual-review-capture", openReviewSurface);
	}, [c.setView]);

	return (
		<div className="app-shell">
			<ManderLabGate />
			<div className="body-row">
				{c.view.kind !== "settings" ? (
					<Sidebar
						state={c.state}
						lagunaStatus={c.laguna}
						activeChatId={c.view.kind === "chat" ? c.view.chatId : null}
						inventoryActive={c.view.kind === "inventory"}
						visualsActive={c.view.kind === "visuals"}
						reportsActive={c.view.kind === "reports"}
						experimentsActive={c.view.kind === "experiments"}
						optimizersActive={c.view.kind === "optimizers"}
						computerUseActive={c.view.kind === "computer-use"}
						workingChatIds={c.workingChatIds}
						chatPresence={c.chatPresence}
						activeLocalDecodeTps={c.inferenceMonitor.snapshot?.active?.decodeTokensPerSecond == null
							? null
							: `${formatTps(c.inferenceMonitor.snapshot.active.decodeTokensPerSecond)} tok/s`}
						unreadChatIds={c.unreadChatIds}
						pinnedChatIds={c.pinnedChatIds}
						conversationTitles={c.conversationTitles}
						sidebarWidth={c.sidebarWidth}
						sidebarVisible={c.sidebarVisible}
						onSidebarWidthChange={(width) => {
							c.setSidebarWidth(width);
							c.persistLayoutSnapshot({ sidebarWidth: width });
						}}
						onNewConversation={c.onNewConversation}
						onOpenChat={(id) => {
							c.openChat(id);
							c.persistLayoutSnapshot({ selectedConversationId: id });
						}}
						onRenameChat={(id, title) => {
							try {
								c.setPreferences(renameConversation(id, title));
							} catch (reason) {
								c.showToast(publicError(reason));
							}
						}}
						onPinChat={(id, pinned) => c.setPreferences(pinConversation(id, pinned))}
						onArchiveChat={(id, archived) => {
							if (archived && c.workingChatIds.has(id)) {
								c.showToast("Stop the run before archiving");
								return;
							}
							c.setPreferences(archiveConversation(id, archived));
							if (archived && c.view.kind === "chat" && c.view.chatId === id) {
								c.setView({ kind: "landing" });
							}
						}}
						pluginStatuses={c.pluginStatuses}
						onOpenInventory={() => c.setView({ kind: "inventory" })}
						onOpenVisuals={() => c.setView({ kind: "visuals" })}
						onOpenReports={() => c.setView({ kind: "reports" })}
						onOpenExperiments={() => c.setView({ kind: "experiments" })}
						onOpenOptimizers={() => c.setView({ kind: "optimizers" })}
						onOpenComputerUse={() => c.setView({ kind: "computer-use" })}
						onSearch={c.openSearch}
						onSettings={() => c.setView({ kind: "settings" })}
						account={c.accountView}
						codexOauthConfigured={c.state.codexOauthConfigured}
						codexUsage={c.codexUsage}
						onOpenUsage={() => c.setUsageSheetOpen(true)}
						onBilling={(action) => void c.openBilling(action)}
						onRetryAccount={() => c.refreshAccountSummary(true)}
						onOpenAccount={() => c.setView({ kind: "settings", section: "account" })}
						onSignOut={async () => {
							if (!bridges.account) {
								c.setView({ kind: "settings", section: "account" });
								return;
							}
							try {
								const next = await bridges.account.signOut();
								c.setApiKeyConfigured(next.apiKeyConfigured);
								c.refreshAccountSummary();
								c.showToast("Signed out of Synth");
							} catch (reason) {
								c.showToast(publicError(reason));
							}
						}}
						onPauseToggle={() => c.setDownloadPaused((v) => !v)}
						onFreeLocalMemory={c.onFreeLocalMemory}
					/>
				) : null}

				<main className="main-pane">
					<AppTitlebar
						tabLabel={c.tabLabel}
						appVersion={c.appVersion}
						activeLocalModel={Boolean(c.activeLocalModel)}
						reserveNativeControls={c.view.kind === "settings" || !c.sidebarVisible}
						brand={c.view.kind === "settings" && c.view.section === "models" ? "openai" : "synth"}
						copyItems={tabCopyItems}
						onCopyItem={async (item) => {
							try {
								await copyText(item.value);
								c.showToast(item.successMessage);
							} catch (reason) {
								c.showToast(`Copy failed: ${publicError(reason)}`);
							}
						}}
						terminalOpen={c.terminalOpen}
						sidePanelOpen={c.sidePanelOpen}
						sidePanelTab={c.sidePanelTab}
						onCloseTab={() => {
							c.setView({ kind: "landing" });
							c.showToast("Back to landing");
						}}
						onNewConversation={c.onNewConversation}
						onToggleTerminal={() => {
							c.persistLayoutSnapshot({ bottomPanelVisible: !c.terminalOpen });
						}}
						onToggleInference={() => {
							const next = !(c.sidePanelOpen && c.sidePanelTab === "inference");
							c.setSidePanelTab("inference");
							c.setSidePanelOpen(next);
							window.localStorage.setItem("synth.inferenceRailOpen", next ? "1" : "0");
						}}
					/>

					{c.bootError ? (
						<div className="boot-error" role="alert">
							Runtime unavailable: {c.bootError}
						</div>
					) : null}

					<MainRoutes
						view={c.view}
						setView={c.setView}
						computerUse={c.computerUse}
						computerUseBusy={c.computerUseBusy}
						onInstallComputerUse={() => void c.installComputerUse()}
						onRemoveComputerUse={() => void c.removeComputerUse()}
						onRefreshComputerUse={() => void c.refreshComputerUse()}
						onOpenComputerUseSettings={(permission) => void c.openComputerUseSettings(permission)}
						onRevokeComputerUseApp={(bundleId) => void c.revokeComputerUseApp(bundleId)}
						pluginStatuses={c.pluginStatuses}
						refreshPluginStatuses={c.refreshPluginStatuses}
						state={c.state}
						sessions={c.sessions}
						selectedTargetId={c.selectedTargetId}
						onSelectTarget={c.onSelectTarget}
						lagunaAdapters={c.lagunaAdapters}
						selectedLagunaAdapterId={c.selectedLagunaAdapterId}
						onSelectLagunaAdapter={(checkpointId) => void c.selectLagunaAdapter(checkpointId)}
						activeChat={c.activeChat}
						eventsBySession={c.eventsBySession}
						activeChatSession={c.activeChatSession}
						activeChatRunning={c.activeChatRunning}
						activeChatWarmingUp={c.activeChatWarmingUp}
						activeLocalModel={Boolean(c.activeLocalModel)}
						activeSessionId={c.activeSessionId}
						openArtifact={c.openArtifact}
						openArtifactId={c.openArtifactId}
						openContainer={c.openContainer}
						containerPaneExpanded={c.containerPaneExpanded}
						setContainerPaneExpanded={c.setContainerPaneExpanded}
						inventoryContainerWidth={c.inventoryContainerWidth}
						setInventoryContainerWidth={c.setInventoryContainerWidth}
						persistLayoutSnapshot={c.persistLayoutSnapshot}
						showSidePanel={c.showSidePanel}
						sidePanelCanSharePane={c.sidePanelCanSharePane}
						sidePanelTab={c.sidePanelTab}
						setSidePanelTab={c.setSidePanelTab}
						setSidePanelOpen={c.setSidePanelOpen}
						transcriptHistoryBySession={c.transcriptHistoryBySession}
						loadOlderTranscript={c.loadOlderTranscript}
						inferenceMonitor={c.inferenceMonitor}
						persistedPerformanceByTarget={c.persistedPerformanceByTarget}
						preferences={c.preferences}
						setPreferences={c.setPreferences}
						accountView={c.accountView}
						accountSummary={c.accountSummary}
						accountUsage={c.accountUsage}
						backendSettings={c.backendSettings}
						laguna={c.laguna}
						onReloadLaguna={c.onReloadLaguna}
						openBilling={c.openBilling}
						refreshAccountSummary={c.refreshAccountSummary}
						setUsageSheetOpen={c.setUsageSheetOpen}
						setSidebarVisible={c.setSidebarVisible}
						setSidebarWidth={c.setSidebarWidth}
						setTerminalOpen={c.setTerminalOpen}
						setApprovalMode={c.setApprovalMode}
						setApprovalPolicy={c.setApprovalPolicy}
						setSandboxMode={c.setSandboxMode}
						showToast={c.showToast}
						startOptimizerAgent={async (title, prompt) => {
							// An optimizer setup is an ordinary product turn on the
							// operator-selected target. Do not silently route a local
							// request through ChatGPT merely because OAuth exists.
							// Defer native startup so sendTurn atomically takes custody
							// of the first prompt instead of opening a blank eager thread.
							const session = await c.createConversation(
								c.selectedTargetId,
								title,
								undefined,
								{ deferNativeStart: true }
							);
							const sent = await c.sendToSession(session.id, prompt);
							if (!sent) throw new Error("The optimizer setup agent could not start");
						}}
						openChat={c.openChat}
						openVisualRecord={c.openVisualRecord}
						toggleArtifact={c.toggleArtifact}
						toggleContainer={c.toggleContainer}
						probeOpenContainer={c.probeOpenContainer}
						repairOpenContainer={c.repairOpenContainer}
						restartOpenContainer={c.restartOpenContainer}
						controlActive={c.controlActive}
						onActivityModeChange={(mode) => c.setPreferences(setToolActivityMode(mode))}
					/>

					<ComposerDock
						show={c.showComposer}
						state={c.state}
						view={c.view}
						activeSessionId={c.activeSessionId}
						activeChat={c.activeChat}
						activeChatRunning={c.activeChatRunning}
						sessions={c.sessions}
						preferences={c.preferences}
						setPreferences={c.setPreferences}
						nativeCodex={c.nativeCodex}
						approvalPolicy={c.approvalPolicy}
						sandboxMode={c.sandboxMode}
						selectActivePermissions={c.selectActivePermissions}
						modelKnobValues={c.modelKnobValues}
						selectModelKnob={c.selectModelKnob}
						selectedModelMedianTpsLabel={c.selectedModelMedianTpsLabel}
						aggregateModelTpsLabels={c.aggregateModelTpsLabels}
						queueAfterStop={c.queueAfterStop}
						setQueueAfterStop={c.setQueueAfterStop}
						steerError={c.steerError}
						setSteerError={c.setSteerError}
						failedSend={c.failedSend}
						retryFailedSend={c.retryFailedSend}
						recoveryNotice={c.view.kind === "chat" ? c.recoveryNotices[c.view.chatId] ?? null : null}
						onResumeRecovered={c.resumeRecoveredChat}
						defaultWorkspace={c.defaultWorkspace}
						workspaceScope={c.workspaceScope}
						setWorkspaceScope={c.setWorkspaceScope}
						composerSkills={c.composerSkills}
						selectedTargetId={c.selectedTargetId}
						onSelectTarget={c.onSelectTarget}
						lagunaAdapters={c.lagunaAdapters}
						selectedLagunaAdapterId={c.selectedLagunaAdapterId}
						onSelectLagunaAdapter={(checkpointId) => void c.selectLagunaAdapter(checkpointId)}
						onComposerSend={c.onComposerSend}
						sendToSession={c.sendToSession}
						createConversation={c.createConversation}
						onNewConversation={c.onNewConversation}
						onSlashRename={c.onSlashRename}
						onSlashCompact={c.onSlashCompact}
						showToast={c.showToast}
						setView={c.setView}
						setUsageSheetOpen={c.setUsageSheetOpen}
						onStopActiveTurn={() => {
							c.setQueueAfterStop(c.activeChat ? promptsForConversation(c.activeChat.id).length > 0 : false);
							void c.controlActive("cancel");
						}}
					/>

					<TerminalPanel
						open={c.terminalOpen}
						workspaceId={c.terminalWorkspaceId}
						workspaceRoot={c.terminalWorkspaceRoot}
						height={c.preferences.layout.last.bottomPanelHeight}
						fontFamily={c.preferences.appearance.terminalFontFamily}
						fontSize={c.preferences.appearance.terminalFontSize}
						onOpenChange={(open) => {
							c.persistLayoutSnapshot({ bottomPanelVisible: open });
						}}
						onHeightChange={(height) => c.persistLayoutSnapshot({ bottomPanelHeight: height })}
					/>
				</main>
			</div>

			<AppOverlays
				searchOpen={c.searchOpen}
				state={c.state}
				onCloseSearch={c.closeSearch}
				onOpenChat={(id) => c.openChat(id)}
				usageSheetOpen={c.usageSheetOpen}
				accountView={c.accountView}
				accountSummary={c.accountSummary}
				onCloseUsage={() => c.setUsageSheetOpen(false)}
				onSignIn={() => {
					c.setUsageSheetOpen(false);
					c.setView({ kind: "settings", section: "account" });
				}}
				onBilling={(action) => void c.openBilling(action)}
				onRetryAccount={() => c.refreshAccountSummary(true)}
				onOpenDeviceUsage={() => {
					c.setUsageSheetOpen(false);
					c.setView({ kind: "inventory" });
				}}
				toast={c.toast}
			/>
		</div>
	);
}
