import { formatTps } from "./components/InferencePanel";
import { AppTitlebar } from "./components/AppTitlebar";
import { AppOverlays } from "./components/AppOverlays";
import { ComposerDock } from "./components/ComposerDock";
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
import { MainRoutes } from "./routes";

/** Shell + wiring only — orchestration lives in useAppController / ComposerDock. */
export default function App() {
	const c = useAppController();

	return (
		<div className="app-shell">
			<div className="body-row">
				{c.view.kind !== "settings" ? (
					<Sidebar
						state={c.state}
						lagunaStatus={c.laguna}
						activeChatId={c.view.kind === "chat" ? c.view.chatId : null}
						inventoryActive={c.view.kind === "inventory"}
						visualsActive={c.view.kind === "visuals"}
						optimizersActive={c.view.kind === "optimizers"}
						workingChatIds={c.workingChatIds}
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
								c.showToast(reason instanceof Error ? reason.message : String(reason));
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
						onOpenInventory={() => c.setView({ kind: "inventory" })}
						onOpenVisuals={() => c.setView({ kind: "visuals" })}
						onOpenOptimizers={() => c.setView({ kind: "optimizers" })}
						onSearch={c.openSearch}
						onSettings={() => c.setView({ kind: "settings" })}
						account={c.accountView}
						onOpenUsage={() => c.setUsageSheetOpen(true)}
						onBilling={(action) => void c.openBilling(action)}
						onRetryAccount={() => c.refreshAccountSummary(true)}
						onOpenAccount={() => c.setView({ kind: "settings", section: "account" })}
						onSignOut={async () => {
							if (!window.synthAccount) {
								c.setView({ kind: "settings", section: "account" });
								return;
							}
							try {
								const next = await window.synthAccount.signOut();
								c.setApiKeyConfigured(next.apiKeyConfigured);
								c.refreshAccountSummary();
								c.showToast("Signed out of Synth");
							} catch (reason) {
								c.showToast(reason instanceof Error ? reason.message : String(reason));
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
						terminalOpen={c.terminalOpen}
						sidePanelOpen={c.sidePanelOpen}
						sidePanelTab={c.sidePanelTab}
						onCloseTab={() => {
							c.setView({ kind: "landing" });
							c.showToast("Back to landing");
						}}
						onNewConversation={c.onNewConversation}
						onToggleTerminal={() => {
							c.setTerminalOpen((current) => {
								const next = !current;
								c.persistLayoutSnapshot({ bottomPanelVisible: next });
								return next;
							});
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
						state={c.state}
						sessions={c.sessions}
						selectedTargetId={c.selectedTargetId}
						onSelectTarget={c.onSelectTarget}
						activeChat={c.activeChat}
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
						sidePanelTab={c.sidePanelTab}
						setSidePanelTab={c.setSidePanelTab}
						setSidePanelOpen={c.setSidePanelOpen}
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
						openChat={c.openChat}
						openVisualRecord={c.openVisualRecord}
						toggleArtifact={c.toggleArtifact}
						toggleContainer={c.toggleContainer}
						probeOpenContainer={c.probeOpenContainer}
						controlActive={c.controlActive}
						setQueueAfterStop={c.setQueueAfterStop}
						promptsForConversationLength={(chatId) => promptsForConversation(chatId).length}
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
						defaultWorkspace={c.defaultWorkspace}
						workspaceScope={c.workspaceScope}
						setWorkspaceScope={c.setWorkspaceScope}
						composerSkills={c.composerSkills}
						selectedTargetId={c.selectedTargetId}
						onSelectTarget={c.onSelectTarget}
						onComposerSend={c.onComposerSend}
						sendToSession={c.sendToSession}
						createConversation={c.createConversation}
						onNewConversation={c.onNewConversation}
						onSlashRename={c.onSlashRename}
						onSlashCompact={c.onSlashCompact}
						showToast={c.showToast}
						setView={c.setView}
						setUsageSheetOpen={c.setUsageSheetOpen}
					/>

					<TerminalPanel
						open={c.terminalOpen}
						workspaceId={c.terminalWorkspaceId}
						workspaceRoot={c.terminalWorkspaceRoot}
						height={c.preferences.layout.last.bottomPanelHeight}
						fontFamily={c.preferences.appearance.terminalFontFamily}
						fontSize={c.preferences.appearance.terminalFontSize}
						onOpenChange={(open) => {
							c.setTerminalOpen(open);
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
