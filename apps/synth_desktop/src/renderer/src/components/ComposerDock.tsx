import type { Session } from "@synth/runtime-protocol";
import type { LandingState, LocalChat } from "../types/landing";
import type { ConversationWorkspaceScope } from "../bridge";
import type { DesktopPreferences } from "../preferences";
import {
	enqueuePrompt,
	promptsForConversation,
	removeQueuedPrompt,
	updateQueuedPrompt
} from "../preferences";
import { nextQueuedPrompt } from "../runtime/promptQueue";
import type { ApprovalPolicy, SandboxMode } from "../runtime/nativeCodex";
import type { ModelKnobTransportValue } from "../runtime/modelCapabilities";
import type { FailedSend } from "../runtime/codexTurn";
import type { MainView } from "../routes";
import { Composer } from "./Composer";

export type ComposerDockProps = {
	show: boolean;
	state: LandingState;
	view: MainView;
	activeSessionId: string | null;
	activeChat: LocalChat | null;
	activeChatRunning: boolean;
	sessions: Session[];
	preferences: DesktopPreferences;
	setPreferences: (next: DesktopPreferences) => void;
	nativeCodex: Window["synthCodex"];
	approvalPolicy: ApprovalPolicy;
	sandboxMode: SandboxMode;
	selectActivePermissions: (policy: ApprovalPolicy, sandbox: SandboxMode) => void;
	modelKnobValues: Record<string, ModelKnobTransportValue>;
	selectModelKnob: (targetId: string, knobId: string, value: ModelKnobTransportValue) => void;
	selectedModelMedianTpsLabel: string | null;
	aggregateModelTpsLabels: Record<string, string>;
	queueAfterStop: boolean;
	setQueueAfterStop: (value: boolean) => void;
	steerError: string | null;
	setSteerError: (value: string | null) => void;
	failedSend: FailedSend | null;
	retryFailedSend: () => void;
	defaultWorkspace: string | null;
	workspaceScope: ConversationWorkspaceScope | null;
	setWorkspaceScope: (scope: ConversationWorkspaceScope | null) => void;
	composerSkills: Array<{ id: string; name: string; description: string }>;
	selectedTargetId: string;
	onSelectTarget: (id: string) => void;
	onComposerSend: (text: string) => void | Promise<void>;
	sendToSession: (sessionId: string, text: string) => Promise<boolean>;
	createConversation: (targetId?: string) => Promise<Session>;
	onNewConversation: () => void;
	onSlashRename: () => void;
	onSlashCompact: () => void | Promise<void>;
	showToast: (message: string) => void;
	setView: (view: MainView) => void;
	setUsageSheetOpen: (open: boolean) => void;
};

/**
 * Composer wiring seam — keeps ≤10 prop groups out of App.tsx.
 */
export function ComposerDock({
	show,
	state,
	view,
	activeSessionId,
	activeChat,
	activeChatRunning,
	sessions,
	preferences,
	setPreferences,
	nativeCodex,
	approvalPolicy,
	sandboxMode,
	selectActivePermissions,
	modelKnobValues,
	selectModelKnob,
	selectedModelMedianTpsLabel,
	aggregateModelTpsLabels,
	queueAfterStop,
	setQueueAfterStop,
	steerError,
	setSteerError,
	failedSend,
	retryFailedSend,
	defaultWorkspace,
	workspaceScope,
	setWorkspaceScope,
	composerSkills,
	selectedTargetId,
	onSelectTarget,
	onComposerSend,
	sendToSession,
	createConversation,
	onNewConversation,
	onSlashRename,
	onSlashCompact,
	showToast,
	setView,
	setUsageSheetOpen
}: ComposerDockProps) {
	if (!show) return null;

	return (
		<Composer
			state={state}
			onSend={(text) => void onComposerSend(text)}
			onSelectTarget={onSelectTarget}
			permissions={{
				approvalPolicy,
				sandboxMode,
				onSelect: selectActivePermissions
			}}
			model={{
				knobValues: modelKnobValues,
				onSelectKnob: selectModelKnob,
				medianTpsLabel: selectedModelMedianTpsLabel,
				aggregateTpsLabels: aggregateModelTpsLabels
			}}
			queue={{
				prompts: activeSessionId ? promptsForConversation(activeSessionId, preferences) : [],
				onEnqueue: (text) => {
					const conversationId = activeSessionId;
					if (!conversationId) {
						showToast("No active conversation to queue into");
						return;
					}
					setSteerError(null);
					setPreferences(enqueuePrompt(conversationId, text));
				},
				onEdit: (id, text) => {
					try {
						setPreferences(updateQueuedPrompt(id, text));
					} catch (reason) {
						showToast(reason instanceof Error ? reason.message : String(reason));
					}
				},
				onRemove: (id) => setPreferences(removeQueuedPrompt(id)),
				onPromote: async (id, text) => {
					if (!activeSessionId || !nativeCodex?.steerTurn) {
						setSteerError("Steer is not supported by the current runtime. Keep the prompt queued or wait for the turn to finish.");
						return;
					}
					try {
						await nativeCodex.steerTurn(activeSessionId, text);
						setPreferences(removeQueuedPrompt(id));
						setSteerError(null);
					} catch (reason) {
						setSteerError(reason instanceof Error ? reason.message : String(reason));
					}
				},
				afterStop: queueAfterStop,
				onKeep: () => setQueueAfterStop(false),
				onSendNext: () => {
					if (!activeSessionId) return;
					const next = nextQueuedPrompt(activeSessionId);
					setQueueAfterStop(false);
					if (next) void sendToSession(activeSessionId, next.text).then((accepted) => {
						if (accepted) setPreferences(removeQueuedPrompt(next.id));
					});
				}
			}}
			turn={{
				agentWorking: Boolean(activeChatRunning),
				activeEnterAction: preferences.submission.activeEnterAction,
				steerSupported: Boolean(nativeCodex?.steerTurn),
				steerError,
				sendFailure: failedSend && failedSend.sessionId === activeChat?.id
					? { message: failedSend.message, onRetry: retryFailedSend }
					: null,
				onSteer: async (text) => {
					if (!activeSessionId || !nativeCodex?.steerTurn) {
						setSteerError("Steer is not supported by the current runtime. Queue the prompt or wait for the turn to finish.");
						return;
					}
					try {
						await nativeCodex.steerTurn(activeSessionId, text);
						setSteerError(null);
					} catch (reason) {
						setSteerError(reason instanceof Error ? reason.message : String(reason));
					}
				}
			}}
			workspace={{
				sessionId: activeSessionId,
				onEnsureSession: async () => {
					if (activeSessionId) return activeSessionId;
					if (view.kind !== "landing") return null;
					const session = await createConversation(selectedTargetId);
					return session.id;
				},
				fallback: activeSessionId ? (sessions.find((item) => item.id === activeSessionId)?.metadata.workspace as string | undefined) ?? defaultWorkspace : defaultWorkspace,
				scope: workspaceScope,
				onScopeChange: setWorkspaceScope,
				onError: showToast
			}}
			slash={{
				skills: composerSkills,
				onNew: onNewConversation,
				onMcp: () => setView({ kind: "connectors" }),
				onRename: onSlashRename,
				onCompact: onSlashCompact
			}}
			account={{
				onConfigureAccount: () => setView({ kind: "settings", section: "account" }),
				onConfigureModels: () => setView({ kind: "settings", section: "models" }),
				onResolveBilling: () => setUsageSheetOpen(true),
				onOpenVoiceSettings: () => setView({ kind: "settings", section: "voice" })
			}}
		/>
	);
}
