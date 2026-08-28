import type { RecoveryNotice, Session } from "@synth/runtime-protocol";
import type { LandingState, LocalChat } from "../types/landing";
import type { ConversationWorkspaceScope } from "../bridge";
import type { DesktopPreferences } from "../preferences";
import {
	enqueuePrompt,
	promptsForConversation,
	removeQueuedPrompt,
	updateQueuedPrompt
} from "../preferences";

/**
 * One user-facing sentence for a composer failure.
 *
 * Every path goes through {@link normalizeSteerFailure}, so an internal session
 * UUID, a transport body, or a bare object can never be interpolated into the
 * composer — the structured original stays in `detail` for diagnostics.
 */
export function composerErrorMessage(reason: unknown): string {
	return normalizeSteerFailure(reason).message;
}
import { normalizeSteerFailure, STEER_UNSUPPORTED } from "../runtime/steering";
import { nextQueuedPrompt } from "../runtime/promptQueue";
import type { ApprovalPolicy, SandboxMode } from "../runtime/nativeCodex";
import type { ModelKnobTransportValue } from "../runtime/modelCapabilities";
import type { FailedSend } from "../runtime/codexTurn";
import type { MainView } from "../routes";
import { Composer } from "./Composer";
import type { LagunaPolicy } from "../bridge/types";

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
	/** Set when a previous Workshop process died holding this chat's turn. */
	recoveryNotice?: RecoveryNotice | null;
	onRestartRecovered?: (sessionId: string) => void;
	defaultWorkspace: string | null;
	workspaceScope: ConversationWorkspaceScope | null;
	setWorkspaceScope: (scope: ConversationWorkspaceScope | null) => void;
	composerSkills: Array<{ id: string; name: string; description: string }>;
	selectedTargetId: string;
	onSelectTarget: (id: string) => void;
	lagunaAdapters: LagunaPolicy[];
	selectedLagunaAdapterId: string | null;
	onSelectLagunaAdapter: (checkpointId: string | null) => void;
	onComposerSend: (text: string) => void | Promise<void>;
	sendToSession: (sessionId: string, text: string) => Promise<boolean>;
	createConversation: (targetId?: string) => Promise<Session>;
	onNewConversation: () => void;
	onSlashRename: () => void;
	onSlashCompact: () => void | Promise<void>;
	showToast: (message: string) => void;
	setView: (view: MainView) => void;
	setUsageSheetOpen: (open: boolean) => void;
	onStopActiveTurn: () => void;
};

/**
 * One sentence, matching the send-failure line beside it: what happened, and
 * what that means for retrying. The detail (attempt count, previous owner)
 * stays in the journal.
 */
function recoveryMessage(notice: RecoveryNotice): string {
	if (notice.needsAttention) {
		return "Workshop exited while this task had work in flight. Check whether it completed before retrying.";
	}
	if (notice.externalObjectId) {
		return `Workshop exited after this task started ${notice.externalObjectId}.`;
	}
	return "Workshop restarted while this task was running. Continue on the same thread.";
}

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
	recoveryNotice,
	onRestartRecovered,
	defaultWorkspace,
	workspaceScope,
	setWorkspaceScope,
	composerSkills,
	selectedTargetId,
	onSelectTarget,
	lagunaAdapters,
	selectedLagunaAdapterId,
	onSelectLagunaAdapter,
	onComposerSend,
	sendToSession,
	createConversation,
	onNewConversation,
	onSlashRename,
	onSlashCompact,
	showToast,
	setView,
	setUsageSheetOpen,
	onStopActiveTurn
}: ComposerDockProps) {
	if (!show) return null;

	return (
		<Composer
			state={state}
			sentMessages={activeChat?.messages
				.filter((message) => message.role === "user")
				.map((message) => message.body) ?? []}
			onSend={(text) => void onComposerSend(text)}
			onSelectTarget={onSelectTarget}
			lagunaAdapter={{
				adapters: lagunaAdapters,
				selectedId: selectedLagunaAdapterId,
				onSelect: onSelectLagunaAdapter
			}}
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
						return undefined;
					}
					setSteerError(null);
					const next = enqueuePrompt(conversationId, text);
					setPreferences(next);
					return next.promptQueue.at(-1)?.id;
				},
				onEdit: (id, text) => {
					try {
						setPreferences(updateQueuedPrompt(id, text));
					} catch (reason) {
						showToast(composerErrorMessage(reason));
					}
				},
				onRemove: (id) => setPreferences(removeQueuedPrompt(id)),
				// Rejections propagate: the composer's steering state machine owns
				// the failure, so a prompt is retired only once the backend has
				// acknowledged the steer it was promoted into.
				onPromote: async (id, text) => {
					if (!activeSessionId || !nativeCodex?.steerTurn) throw STEER_UNSUPPORTED;
					setSteerError(null);
					await nativeCodex.steerTurn(activeSessionId, text);
					setPreferences(removeQueuedPrompt(id));
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
				// A live send failure is about this attempt; a recovery notice is
				// about the process that never got to finish the last one. The
				// live one wins — it is the more recent thing the user did.
				sendFailure: failedSend && failedSend.sessionId === activeChat?.id
					? { message: failedSend.message, onRetry: retryFailedSend }
					: recoveryNotice && activeChat?.id
						? {
							message: recoveryMessage(recoveryNotice),
							actionLabel: "Continue",
							onRetry: recoveryNotice.restartable
								? () => onRestartRecovered?.(activeChat.id)
								: undefined
						}
						: null,
				onSteer: async (text) => {
					if (!activeSessionId || !nativeCodex?.steerTurn) {
						setSteerError(STEER_UNSUPPORTED.message);
						return;
					}
					try {
						await nativeCodex.steerTurn(activeSessionId, text);
						setSteerError(null);
					} catch (reason) {
						const failure = normalizeSteerFailure(reason);
						console.error("[steer] direct steer rejected", failure.code, failure.detail);
						setSteerError(failure.message);
					}
				},
				onStop: onStopActiveTurn
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
