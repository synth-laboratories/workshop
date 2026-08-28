// @ts-nocheck — P0-1 generated protocol is stricter than prior handwritten DTOs; UI follow-up is out of specta-cutover file ownership.
/**
 * Wave 3c — desktop app controller.
 * Owns remaining App orchestration (boot, permissions, sessions, overlays,
 * eval host) so App.tsx stays shell + wiring only.
 */
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import { appEventToRuntimeEvent } from "@synth/runtime-protocol";
import desktopPackage from "../../../../package.json";
import type {
	CodexActivityEvent,
	ContainerDeployment,
	RuntimeControlKind,
	RuntimeEvent,
	ChatPresence,
	RecoveryNotice,
	RuntimeHealth,
	Session,
	VisualInstanceRecord,
	VisualRecord
} from "@synth/runtime-protocol";
import { publicError } from "../runtime/publicError";
import {
	dispatchLocalSessionStatus,
	dispatchRuntimeEvent,
	dispatchTurnAccepted,
	evictSessionEvents,
	mergeInternSessions,
	mergeSessionReplay,
	patchSessionMetadata,
	replaceSessions,
	selectChatPresence,
	selectSessionRunning,
	selectWorkingChatIds,
	sessionRecoveryNotice,
	upsertSession,
	useEventsBySession,
	useLiveTurns,
	useSessions
} from "../stores/sessionStore";
import {
	EXECUTION_TARGETS,
	isInternTargetId,
	resolveDefaultTargetId
} from "../types/landing";
import { useInferenceMonitor } from "../components/InferencePanel";
import { artifactFromVisualRecord } from "../components/VisualHost";
import { useAccountShell } from "./useAccountShell";
import { usePluginStatuses } from "./usePluginStatuses";
import { useComputerUse } from "./useComputerUse";
import { useShellLayout } from "./useShellLayout";
import { useCodexEventBridge, type CodexUsageSnapshot } from "./useCodexEventBridge";
import { useForeignSessionEventBridge } from "./useForeignSessionEventBridge";
import { useModelPerformanceLabels } from "./useModelPerformanceLabels";
import {
	buildLandingState,
	executionTargetToUiId,
	sessionIsAsync,
	sessionIsLocalChat,
	sessionIsSync,
	targetIdToExecutionTarget,
	visualRecordToArtifact,
	openArtifactIdForChat
} from "../runtime/sessionView";
import { chatInferencePhase } from "../runtime/chatWarmingState";
import { approvalModeFromConfig, codexStartRequest, coreEventToRuntime, createCodexSession, restoreCodexSession, type ApprovalMode, type ApprovalPolicy, type SandboxMode } from "../runtime/nativeCodex";
import { LOCAL_BASE_POLICY } from "../runtime/lagunaPolicies";
import type { LagunaPolicy } from "../bridge/types";
import {
	loadModelKnobValues,
	modelKnobForTarget,
	modelKnobKey,
	modelSupportsImageInput,
	serviceTierForExecutionTarget,
	turnStartEffortForExecutionTarget,
	type ModelKnobTransportValue
} from "../runtime/modelCapabilities";
import {
	canStartNewTurn,
	installModelCatalog,
	isOpenRouterCatalogTarget,
	modelCatalog,
	targetOptionForId
} from "../runtime/modelCatalog";
import {
	planComposerSend,
	planModelChipChange,
	threadHasHistoryFromEvents
} from "../runtime/modelSwitchPlan";
import type {
	CodexSessionInfo,
	CodexEvent,
	CodexOauthStatus,
	ComposerImageAttachment,
	ConversationWorkspaceScope,
	LagunaStatus
} from "../bridge";
import {
	applyPreferencesToDocument,
	loadPreferences,
	normalizeLayoutSnapshot,
	preferencesAdapter,
	renameConversation,
	saveLayout,
	setPermissionPreferences,
	setUnreadCompletedChats,
	subscribePreferences,
	type DesktopPreferences
} from "../preferences";
import { browserRuntimeClient } from "../runtime/browserRuntimeClient";
import {
	codexResumeRequest,
	codexTurnFailure,
	CODEX_SESSION_UNHEALTHY,
	desktopBootError,
	turnFailureMessage,
	type FailedSend
} from "../runtime/codexTurn";
import { loadDeviceUsage } from "../runtime/deviceUsage";
import { createSemanticEvalApi } from "../runtime/evalApi";
import { drainPromptQueues, removeQueuedPrompt } from "../runtime/promptQueue";
import { bridges } from "../runtime/desktopBridge";
import { DIAGNOSTIC_CODES, reportDiagnostic } from "../runtime/diagnostics";
import { responseTraceStore } from "../runtime/responseTraceStore";
import {
	EMPTY_VISUAL_REVISION_STATE,
	newestVisualArtifact,
	visualRevisionReducer
} from "../runtime/visualRevisionState";
import type { MainView } from "../routes";

// `turn/start` only proves the app-server accepted the request. It does not
// prove the provider stream is alive, so never leave the operator at Working…
// forever if no event follows.
const TURN_FIRST_ACTIVITY_TIMEOUT_MS = 45_000;
// XHigh reasoning can legitimately emit no transport events for several
// minutes. This is a last-resort dead-connection fence, not a latency budget;
// the separate first-activity watchdog still catches turns that never start.
const TURN_ACTIVITY_IDLE_TIMEOUT_MS = 10 * 60_000;
const TURN_START_TIMEOUT_MESSAGE =
	"This task did not begin producing activity. Check your ChatGPT connection and retry.";
const TURN_ACTIVITY_STALLED_MESSAGE =
	"This task stopped receiving provider activity. Check the Advanced trace and retry.";
const TRANSCRIPT_INITIAL_PAGE_SIZE = 250;
const TRANSCRIPT_PAGE_SIZE = 1000;
const TRANSCRIPT_CACHE_LIMIT = 5;

type TranscriptHydrationEntry = {
	initialized: boolean;
	state: "idle" | "loading" | "loaded" | "error";
	hasMore: boolean;
	earliestSequence?: number;
	error?: string;
	generation: number;
};

type TranscriptHistoryState = Pick<TranscriptHydrationEntry, "state" | "hasMore" | "error">;

export function useAppController() {
	const isDesktop = window.location.protocol === "tauri:" || "__TAURI_INTERNALS__" in window;
	const nativeCodex = bridges.codex;
	// synthIntern is installed in browsers too as a demo adapter. Codex presence is
	// the stable packaged-Tauri signal used here to select the Rust-owned path.
	const nativeIntern = nativeCodex ? bridges.intern : undefined;
	const [appVersion, setAppVersion] = useState(desktopPackage.version);
	useEffect(() => {
		void bridges.desktop.getInstanceDiagnostics()
			.then((identity) => {
				const runtimeVersion = identity.appVersion.trim();
				if (runtimeVersion) setAppVersion(runtimeVersion);
			})
			.catch(() => undefined);
	}, []);
	const [health, setHealth] = useState<RuntimeHealth | null>(null);
	const [laguna, setLaguna] = useState<LagunaStatus | null>(null);
	const sessions = useSessions();
	const liveTurns = useLiveTurns();
	const sessionsRef = useRef<Session[]>(sessions);
	const eventsBySession = useEventsBySession();
	const [codexActivityBySession, setCodexActivityBySession] = useState<Record<string, CodexActivityEvent[]>>({});
	const transcriptHydrationRef = useRef(new Map<string, TranscriptHydrationEntry>());
	const transcriptHydrationGenerationRef = useRef(0);
	const transcriptLruRef = useRef(new Map<string, true>());
	const activeChatIdRef = useRef<string | null>(null);
	const [transcriptHistoryBySession, setTranscriptHistoryBySession] = useState<Record<string, TranscriptHistoryState>>({});
	const [selectedTargetId, setSelectedTargetId] = useState("chatgpt-luna");
	const [catalogLoaded, setCatalogLoaded] = useState(false);
	const [catalogRevision, setCatalogRevision] = useState(0);
	const [selectedLagunaAdapterId, setSelectedLagunaAdapterId] = useState<string | null>(null);
	const [lagunaAdapters, setLagunaAdapters] = useState<LagunaPolicy[]>([]);
	const defaultModelResolvedRef = useRef(false);
	useEffect(() => {
		// v0.1 pickers hide Intern; never leave a hidden target selected.
		if (isInternTargetId(selectedTargetId)) setSelectedTargetId("local-laguna");
	}, [selectedTargetId]);
	const [preferences, setPreferences] = useState<DesktopPreferences>(() => loadPreferences());
	const shellLayout = useShellLayout(setPreferences);
	const {
		sidebarVisible, setSidebarVisible,
		sidebarWidth, setSidebarWidth,
		terminalOpen, setTerminalOpen,
		viewportWidth,
		inventoryContainerWidth, setInventoryContainerWidth,
		sidePanelOpen, setSidePanelOpen,
		sidePanelTab, setSidePanelTab,
		containerPaneExpanded, setContainerPaneExpanded,
		persistLayoutSnapshot
	} = shellLayout;
	const [, setApprovalMode] = useState<ApprovalMode>(() => loadPreferences().approvalMode);
	const [approvalPolicy, setApprovalPolicy] = useState<ApprovalPolicy>(() => loadPreferences().approvalPolicy);
	const [sandboxMode, setSandboxMode] = useState<SandboxMode>(() => loadPreferences().sandboxMode);
	const [modelKnobValues, setModelKnobValues] = useState(() => loadModelKnobValues(window.localStorage));
	const selectModelKnob = useCallback((targetId: string, knobId: string, value: ModelKnobTransportValue) => {
		const knob = modelKnobForTarget(targetId, knobId);
		if (!knob || !knob.options.some((option) => option.transportValue === value)) return;
		setModelKnobValues((current) => ({
			...current,
			[modelKnobKey(targetId, knobId)]: value
		}));
		window.localStorage.setItem(knob.storageKey, value);
	}, []);

	useEffect(() => { sessionsRef.current = sessions; }, [sessions]);
	const [downloadPaused, setDownloadPaused] = useState(false);
	const [toast, setToast] = useState<string | null>(null);
	const [view, setView] = useState<MainView>(() => {
		const selected = loadPreferences().layout.last.selectedConversationId;
		return selected ? { kind: "chat", chatId: selected } : { kind: "landing" };
	});
	const [searchOpen, setSearchOpen] = useState(false);
	const searchRestoreFocusRef = useRef<HTMLElement | null>(null);
	const [unreadChatIds, setUnreadChatIds] = useState<Set<string>>(() => new Set(loadPreferences().unreadCompletedChats));
	const previousSessionStatusesRef = useRef(new Map<string, Session["status"]>());
	const [openArtifactId, setOpenArtifactId] = useState<string | null>(null);
	const [visualRevisionState, dispatchVisualRevision] = useReducer(
		visualRevisionReducer,
		EMPTY_VISUAL_REVISION_STATE
	);
	const openArtifactIdRef = useRef<string | null>(null);
	const openArtifactByViewRef = useRef<Record<string, string | null>>({});
	const activeSessionIdRef = useRef<string | null>(null);
	const visualRequestGenerationRef = useRef(0);
	const pendingVisualRefreshRef = useRef<{ id: string; minimumRevision: number; generation: number } | null>(null);
	const acceptedVisualRevisionRef = useRef<{ id: string | null; revision: number }>({ id: null, revision: -1 });
	const [openContainer, setOpenContainer] = useState<ContainerDeployment | null>(null);
	const inferenceMonitor = useInferenceMonitor({ visible: selectedTargetId === "local-laguna" });
	const {
		persistedPerformanceByTarget,
		selectedModelMedianTpsLabel,
		aggregateModelTpsLabels
	} = useModelPerformanceLabels(selectedTargetId, inferenceMonitor);
	const [busy, setBusy] = useState(false);
	const [bootError, setBootError] = useState<string | null>(null);
	const [runtimeBootReady, setRuntimeBootReady] = useState(false);
	const [steerError, setSteerError] = useState<string | null>(null);
	const [composerSkills, setComposerSkills] = useState<Array<{ id: string; name: string; description: string }>>([]);
	const [queueAfterStop, setQueueAfterStop] = useState(false);
	const [defaultWorkspace, setDefaultWorkspace] = useState<string | null>(null);
	const [workspaceScope, setWorkspaceScope] = useState<ConversationWorkspaceScope | null>(null);
	const eventsBySessionRef = useRef(eventsBySession);
	const nativeSequencesRef = useRef(new Map<string, number>());
	const autoOpenedSubagentsRef = useRef(new Set<string>());
	const [failedSend, setFailedSend] = useState<FailedSend | null>(null);
	const [codexOauthConfigured, setCodexOauthConfigured] = useState(false);
	const [codexOauthStatus, setCodexOauthStatus] = useState<CodexOauthStatus | undefined>();
	const [codexUsage, setCodexUsage] = useState<CodexUsageSnapshot | null>(null);
	const staleRunFenceRef = useRef(new Set<string>());
	const turnStartWatchdogsRef = useRef(new Map<string, number>());
	const turnActivityWatchdogsRef = useRef(new Map<string, number>());
	const turnWatchdogInputsRef = useRef(new Map<string, { text: string; messageId: string }>());
	const manualCompactionPendingRef = useRef(new Set<string>());
	const queuedCompactionRef = useRef(new Set<string>());
	const sendToSessionRef = useRef<(sessionId: string, text: string, options?: { messageId?: string }) => Promise<boolean>>(async () => false);
	const queueDrainStatusesRef = useRef(new Map<string, Session["status"]>());
	const queueDrainingRef = useRef(new Set<string>());
	eventsBySessionRef.current = eventsBySession;
	const allocateNativeSequence = useCallback((sessionId: string) => {
		const rendered = eventsBySessionRef.current[sessionId]?.at(-1)?.sequence ?? 0;
		const next = Math.max(nativeSequencesRef.current.get(sessionId) ?? 0, rendered) + 1;
		nativeSequencesRef.current.set(sessionId, next);
		return next;
	}, []);

	const showToast = useCallback((message: string) => {
		setToast(message);
		window.setTimeout(() => setToast(null), 2200);
	}, []);

	useEffect(() => {
		let disposed = false;
		const apply = (catalog: ReturnType<typeof modelCatalog>) => {
			if (disposed) return;
			installModelCatalog(catalog);
			setCatalogLoaded(true);
			setCatalogRevision((revision) => revision + 1);
		};
		const load = async () => {
			const catalog = await bridges.config?.modelCatalog();
			if (catalog) apply(catalog);
		};
		void load().then(async () => {
			const refreshed = await bridges.config?.refreshModelCatalog();
			if (refreshed) apply(refreshed);
		}).catch((reason) => {
			if (!disposed) {
				setCatalogLoaded(true);
				showToast(`Could not refresh model catalog: ${publicError(reason)}`);
			}
		});
		const onFocus = () => { void load().catch(() => undefined); };
		window.addEventListener("focus", onFocus);
		return () => {
			disposed = true;
			window.removeEventListener("focus", onFocus);
		};
	}, [showToast]);

	const clearTurnStartWatchdog = useCallback((sessionId: string) => {
		const timer = turnStartWatchdogsRef.current.get(sessionId);
		if (timer !== undefined) window.clearTimeout(timer);
		turnStartWatchdogsRef.current.delete(sessionId);
	}, []);
	const clearTurnActivityWatchdog = useCallback((sessionId: string) => {
		const timer = turnActivityWatchdogsRef.current.get(sessionId);
		if (timer !== undefined) window.clearTimeout(timer);
		turnActivityWatchdogsRef.current.delete(sessionId);
	}, []);

	const armTurnActivityWatchdog = useCallback((sessionId: string, text: string, messageId: string) => {
		clearTurnActivityWatchdog(sessionId);
		turnWatchdogInputsRef.current.set(sessionId, { text, messageId });
		const timer = window.setTimeout(() => {
			turnActivityWatchdogsRef.current.delete(sessionId);
			turnWatchdogInputsRef.current.delete(sessionId);
			const session = sessionsRef.current.find((candidate) => candidate.id === sessionId);
			if (session?.status !== "running") return;
			staleRunFenceRef.current.add(sessionId);
			void nativeCodex?.interrupt(sessionId).catch(() => undefined);
			dispatchLocalSessionStatus(sessionId, "failed", { onlyIf: "running" });
			const sequence = allocateNativeSequence(sessionId);
			dispatchRuntimeEvent({
				schemaVersion: "synth.desktop-runtime-event.v1", sessionId, sequence,
				eventKind: "session/unhealthy",
				payload: { reason: "turn_activity_stalled", message: TURN_ACTIVITY_STALLED_MESSAGE },
				createdAt: new Date().toISOString(), source: "local"
			}, { updateStatus: false });
			setFailedSend({ sessionId, text, messageId, message: TURN_ACTIVITY_STALLED_MESSAGE });
			showToast(TURN_ACTIVITY_STALLED_MESSAGE);
			reportDiagnostic({
				sessionId,
				severity: "error",
				component: "renderer",
				event: "provider.activity.stalled",
				code: DIAGNOSTIC_CODES.providerStalled,
				message: TURN_ACTIVITY_STALLED_MESSAGE,
				retryable: true,
				details: { reason: "turn_activity_stalled", idleTimeoutMs: TURN_ACTIVITY_IDLE_TIMEOUT_MS }
			});
		}, TURN_ACTIVITY_IDLE_TIMEOUT_MS);
		turnActivityWatchdogsRef.current.set(sessionId, timer);
	}, [allocateNativeSequence, clearTurnActivityWatchdog, nativeCodex, showToast]);

	const armTurnStartWatchdog = useCallback((sessionId: string, text: string, messageId: string) => {
		clearTurnStartWatchdog(sessionId);
		const timer = window.setTimeout(() => {
			turnStartWatchdogsRef.current.delete(sessionId);
			const session = sessionsRef.current.find((candidate) => candidate.id === sessionId);
			if (session?.status !== "running") return;
			// Fence a delayed run.started event, stop the provider best-effort, and
			// make the transcript retryable rather than leaving a permanent spinner.
			staleRunFenceRef.current.add(sessionId);
			void nativeCodex?.interrupt(sessionId).catch(() => undefined);
			dispatchLocalSessionStatus(sessionId, "failed", { onlyIf: "running" });
			const sequence = allocateNativeSequence(sessionId);
			dispatchRuntimeEvent({
				schemaVersion: "synth.desktop-runtime-event.v1", sessionId, sequence,
				eventKind: "session/unhealthy",
				payload: { reason: "turn_first_activity_timeout", message: TURN_START_TIMEOUT_MESSAGE },
				createdAt: new Date().toISOString(), source: "local"
			}, { updateStatus: false });
			setFailedSend({ sessionId, text, messageId, message: TURN_START_TIMEOUT_MESSAGE });
			showToast(TURN_START_TIMEOUT_MESSAGE);
			reportDiagnostic({
				sessionId,
				severity: "error",
				component: "renderer",
				event: "provider.first_activity.timeout",
				code: DIAGNOSTIC_CODES.providerDisconnected,
				message: TURN_START_TIMEOUT_MESSAGE,
				retryable: true,
				details: { reason: "turn_first_activity_timeout", timeoutMs: TURN_FIRST_ACTIVITY_TIMEOUT_MS }
			});
		}, TURN_FIRST_ACTIVITY_TIMEOUT_MS);
		turnStartWatchdogsRef.current.set(sessionId, timer);
		armTurnActivityWatchdog(sessionId, text, messageId);
	}, [allocateNativeSequence, armTurnActivityWatchdog, clearTurnStartWatchdog, nativeCodex, showToast]);

	useEffect(() => () => {
		for (const timer of turnStartWatchdogsRef.current.values()) window.clearTimeout(timer);
		turnStartWatchdogsRef.current.clear();
		for (const timer of turnActivityWatchdogsRef.current.values()) window.clearTimeout(timer);
		turnActivityWatchdogsRef.current.clear();
		turnWatchdogInputsRef.current.clear();
	}, []);

	const {
		apiKeyConfigured,
		setApiKeyConfigured,
		backendSettings,
		setBackendSettings,
		accountUsage,
		setAccountUsage,
		accountSummary,
		usageSheetOpen,
		setUsageSheetOpen,
		refreshAccountSummary,
		accountView,
		openBilling
	} = useAccountShell(showToast);

	useEffect(() => {
		if (defaultModelResolvedRef.current || !catalogLoaded || view.kind !== "landing" || !backendSettings || !codexOauthStatus) return;
		const targetId = resolveDefaultTargetId(backendSettings.defaultModel ?? { model: "gpt-5.6-luna", effort: "xhigh", providers: ["chatgpt", "openrouter"] }, {
			chatgpt: codexOauthStatus.canUseModels,
			openrouter: backendSettings.openrouterApiKeyConfigured,
			synth: backendSettings.apiKeyConfigured
		}, sessions.filter((session) => session.latestCursor > 0).map((session) => ({
			targetId: executionTargetToUiId(session.target),
			updatedAt: session.updatedAt
		})));
		setSelectedTargetId(targetId);
		const defaultPreference = backendSettings.defaultModel ?? { model: "gpt-5.6-luna", effort: "xhigh", providers: ["chatgpt", "openrouter"] };
		if ((targetId === "chatgpt-luna" || targetId === "openrouter-luna") && defaultPreference.model.toLowerCase().includes("gpt-5.6-luna")) {
			const knob = modelKnobForTarget(targetId, "reasoning");
			const effort = defaultPreference.effort as ModelKnobTransportValue;
			if (knob?.options.some((option) => option.transportValue === effort)) {
				setModelKnobValues((current) => ({ ...current, [modelKnobKey(targetId, "reasoning")]: effort }));
				window.localStorage.setItem(knob.storageKey, effort);
			}
		}
		defaultModelResolvedRef.current = true;
	}, [backendSettings, catalogLoaded, catalogRevision, codexOauthStatus, sessions, view.kind]);

	// One owner for plugin registry status; Sidebar and OptimizersPage read it.
	const { pluginStatuses, refreshPluginStatuses } = usePluginStatuses();

	useEffect(() => subscribePreferences((next) => {
		setPreferences(next);
		setApprovalMode(next.approvalMode);
		setApprovalPolicy(next.approvalPolicy);
		setSandboxMode(next.sandboxMode);
		setUnreadChatIds(new Set(next.unreadCompletedChats));
		applyPreferencesToDocument(next);
	}), []);

	const loadMachinePermissions = useCallback(async () => {
		if (!isDesktop || !bridges.config) {
			return { approvalPolicy, sandboxMode };
		}
		const stored = await bridges.config.getDesktopPermissions();
		setPreferences(setPermissionPreferences(stored.approvalPolicy, stored.sandboxMode));
		return { approvalPolicy: stored.approvalPolicy, sandboxMode: stored.sandboxMode };
	}, [approvalPolicy, isDesktop, sandboxMode]);

	useEffect(() => {
		let disposed = false;
		void loadMachinePermissions().catch((reason) => {
			if (!disposed) showToast(`Could not load machine permissions: ${publicError(reason)}`);
		});
		return () => { disposed = true; };
	}, [loadMachinePermissions, showToast]);

	useEffect(() => {
		applyPreferencesToDocument(preferences);
	}, [preferences]);

	useEffect(() => {
		const refreshOauthStatus = () => {
			// Startup performs a passive read of Workshop's private credential file.
			// Network refresh is deferred until the user selects a ChatGPT model or
			// explicitly opens account settings.
			void bridges.codexOauth?.status().then((status) => {
				setCodexOauthStatus(status);
				setCodexOauthConfigured(status.canUseModels);
			}).catch(() => undefined);
		};
		// The Tauri bridge can attach just after the first renderer commit in a
		// packaged app. Retry once so the sidebar identity does not stay stale.
		refreshOauthStatus();
		const retry = window.setTimeout(refreshOauthStatus, 750);
		const changed = (event: Event) => {
			const status = (event as CustomEvent<CodexOauthStatus>).detail;
			if (status) { setCodexOauthStatus(status); setCodexOauthConfigured(status.canUseModels); }
		};
		window.addEventListener("codex-oauth-changed", changed);
		return () => {
			window.clearTimeout(retry);
			window.removeEventListener("codex-oauth-changed", changed);
		};
	}, []);

	useEffect(() => {
		const onResize = () => {
			const clamped = normalizeLayoutSnapshot(preferences.layout.last);
			if (
				clamped.sidebarWidth !== preferences.layout.last.sidebarWidth ||
				clamped.outputPaneWidth !== preferences.layout.last.outputPaneWidth ||
				clamped.bottomPanelHeight !== preferences.layout.last.bottomPanelHeight
			) {
				setSidebarWidth(clamped.sidebarWidth);
				setInventoryContainerWidth(clamped.outputPaneWidth);
				setPreferences(saveLayout(clamped));
			}
		};
		window.addEventListener("resize", onResize);
		return () => window.removeEventListener("resize", onResize);
	}, [preferences.layout.last]);

	const failTurnStart = useCallback((sessionId: string, text: string, messageId: string, reason: unknown) => {
		const failure = codexTurnFailure(sessionId, reason);
		console.debug("[codex] turn start rejected", {
			code: failure.code,
			sessionId: failure.sessionId,
			detail: failure.detail
		});
		staleRunFenceRef.current.add(sessionId);
		dispatchLocalSessionStatus(sessionId, "interrupted", { onlyIf: "running" });
		const sequence = allocateNativeSequence(sessionId);
		dispatchRuntimeEvent({
			schemaVersion: "synth.desktop-runtime-event.v1", sessionId, sequence,
			eventKind: "session/unhealthy",
			payload: { reason: failure.code, message: turnFailureMessage(failure) },
			createdAt: new Date().toISOString(), source: "local"
		}, { updateStatus: false });
		setFailedSend({ sessionId, text, messageId, message: turnFailureMessage(failure) });
		showToast(turnFailureMessage(failure));
	}, [allocateNativeSequence, showToast]);

	const refreshSessions = useCallback(async () => {
		if (nativeIntern) {
			const next = await nativeIntern.listSessions();
			mergeInternSessions(next);
			return next;
		}
		const next = await browserRuntimeClient.listSessions();
		replaceSessions(next);
		return next;
	}, [nativeIntern]);

	const refreshHealth = useCallback(async () => {
		if (isDesktop && bridges.core && bridges.config && bridges.inventory) {
			const [core, config, counts, currentLaguna, usage] = await Promise.all([
				bridges.core.diagnostics(),
				bridges.config.get(),
				bridges.inventory.counts(),
				bridges.laguna?.getStatus() ?? Promise.resolve(null),
				loadDeviceUsage().catch(() => null)
			]);
			const next: RuntimeHealth = {
				status: "ok",
				protocolVersion: "synth.desktop-runtime.v1",
				runtimeId: "core-runtime",
				startedAt: new Date().toISOString(),
				// Intern Sync/Async cloud mailbox is [alpha] / v0.2. Do not infer
				// "remote" from an API key alone — that falsely labels internal hosts
				// like backend-api as a live cloud mailbox.
				intern: { mode: "unconfigured", backendUrl: config.backendUrl },
				local: {
					model: "laguna-xs-2.1",
					mode: currentLaguna?.phase === "ready" ? "mlx" : "absent",
					modelPath: currentLaguna?.loadedModel ?? null
				},
				openrouter: { mode: config.openrouterApiKeyConfigured ? "ready" : "unconfigured", models: [] },
				inventory: { containers: counts.containers, traces: counts.traces, visuals: core.visualCount },
				dataStore: {
					path: core.databasePath,
					projects: 0,
					sessions: core.sessionCount,
					runs: core.runCount,
					events: core.journalHead,
					usage: counts.usage
				}
			};
			setApiKeyConfigured(config.apiKeyConfigured);
			setBackendSettings(config);
			setAccountUsage(usage);
			setHealth(next);
			return next;
		}
		const [next, config] = await Promise.all([
			browserRuntimeClient.health(),
			bridges.config?.get().catch(() => null) ?? Promise.resolve(null)
		]);
		if (config) {
			setApiKeyConfigured(config.apiKeyConfigured);
			setBackendSettings(config);
		}
		setHealth(next);
		return next;
	}, [isDesktop]);

	useEffect(() => {
		const onAccountChanged = (event: Event) => {
			const configured = (event as CustomEvent<{ apiKeyConfigured?: boolean }>).detail?.apiKeyConfigured;
			if (typeof configured === "boolean") setApiKeyConfigured(configured);
			else void refreshHealth().catch(() => undefined);
			refreshAccountSummary();
		};
		window.addEventListener("synth:account-changed", onAccountChanged);
		return () => window.removeEventListener("synth:account-changed", onAccountChanged);
	}, [refreshAccountSummary, refreshHealth]);

	useEffect(() => {
		void bridges.codex?.defaultWorkspace().then(setDefaultWorkspace).catch(() => undefined);
	}, []);

	useEffect(() => {
		const onKey = (event: KeyboardEvent) => {
			if (!(event.metaKey || event.ctrlKey)) return;
			if (event.key.toLowerCase() === "j" && !event.shiftKey) {
				event.preventDefault();
				persistLayoutSnapshot({ bottomPanelVisible: !terminalOpen });
			}
			if (event.key.toLowerCase() === "t" && event.shiftKey) {
				event.preventDefault();
				setTerminalOpen((current) => {
					if (current) window.setTimeout(() => window.dispatchEvent(new CustomEvent("synth:new-terminal")), 0);
					return true;
				});
			}
		};
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [persistLayoutSnapshot, terminalOpen]);

	useEffect(() => {
		let disposed = false;
		const boot = nativeCodex
			? Promise.all([nativeCodex.list(), nativeIntern?.listSessions() ?? Promise.resolve([]), refreshHealth()]).then(([persisted, internSessions]) => {
				const restored = persisted.filter((session) => session.status !== "closed").map(restoreCodexSession);
				const combined = [...restored, ...internSessions];
				sessionsRef.current = combined;
				replaceSessions(combined);
			})
			: Promise.all([refreshHealth(), refreshSessions()]);
		boot
			.then(() => {
				if (!disposed) {
					setBootError(null);
					setRuntimeBootReady(true);
				}
			})
			.catch((reason: unknown) => {
				if (!disposed) {
					setBootError(desktopBootError(reason));
				}
			});
		return () => {
			disposed = true;
		};
	}, [nativeCodex, nativeIntern, refreshHealth, refreshSessions]);

	useCodexEventBridge({
		nativeCodex,
		allocateNativeSequence,
		sessionsRef,
		manualCompactionPendingRef,
		queuedCompactionRef,
		staleRunFenceRef,
		autoCompactTokenLimits: preferences.agentContext.autoCompactTokenLimits,
		localBaseUrl: laguna?.baseUrl ?? undefined,
		showToast,
		onTurnActivity: (sessionId) => {
			clearTurnStartWatchdog(sessionId);
			const input = turnWatchdogInputsRef.current.get(sessionId);
			if (input) {
				armTurnActivityWatchdog(sessionId, input.text, input.messageId);
			}
		},
		onRawEvent: (event: CodexEvent) => responseTraceStore.appendLive(event),
		onOauthReauthRequired: () => {
			setCodexOauthConfigured(false);
			setCodexOauthStatus({
				state: "expired", action: "reauthenticate", canUseModels: false, configured: true,
				guidance: "Please sign in with ChatGPT to continue using this model."
			});
			setCodexUsage(null);
		},
		onCodexUsage: setCodexUsage
	});

	useEffect(() => {
		const bridge = bridges.laguna;
		if (!bridge) return;
		let disposed = false;
		void bridge.getStatus().then((status) => {
			if (!disposed) setLaguna(status);
		});
		const unsubscribe = bridge.onStatus((status) => {
			setLaguna(status);
		});
		return () => {
			disposed = true;
			unsubscribe();
		};
	}, []);

	useEffect(() => {
		const interval = window.setInterval(() => {
			void bridges.laguna?.getStatus().then(setLaguna).catch(() => undefined);
			// Tauri owns local/configured-provider sessions in Codex app-server and
			// Inventory in CoreRuntime.  The legacy compatibility poll returns a
			// different session universe and must never replace native state.
			if (nativeCodex) return;
			void refreshSessions().catch(() => undefined);
			void refreshHealth().catch(() => undefined);
		}, 2_500);
		return () => window.clearInterval(interval);
	}, [nativeCodex, refreshHealth, refreshSessions]);

	const activeSessionId = useMemo(() => {
		if (view.kind === "chat") return view.chatId;
		if (view.kind === "sync") return view.sessionId;
		if (view.kind === "async") return view.sessionId;
		return null;
	}, [view]);
	activeSessionIdRef.current = activeSessionId;
	const terminalWorkspaceRoot = defaultWorkspace;

	useEffect(() => {
		let disposed = false;
		const load = async () => {
			try {
				const policies = (await bridges.laguna?.policies?.()) ?? [];
				if (!disposed) setLagunaAdapters(policies);
			} catch {
				if (!disposed) setLagunaAdapters([]);
			}
		};
		void load();
		// Speed is measured as turns run, so the list is refreshed on optimizer
		// activity and whenever a turn completes rather than once at mount.
		const unlisten = bridges.optimizers?.onEvent?.(() => {
			void load();
		});
		const timer = window.setInterval(() => void load(), 30_000);
		return () => {
			disposed = true;
			unlisten?.();
			window.clearInterval(timer);
		};
	}, []);

	const selectLagunaAdapter = useCallback(async (modelId: string | null) => {
		// No daemon call: which policy a turn runs under is decided by the
		// `model` on that request, so selecting one cannot disturb a
		// conversation that is already open under another.
		setSelectedLagunaAdapterId(modelId);
		const sessionId = activeSessionIdRef.current;
		const session = sessionId
			? sessionsRef.current.find((candidate) => candidate.id === sessionId)
			: undefined;
		if (session?.target.kind === "local") {
			const next = { ...session, target: { ...session.target, model: modelId ?? LOCAL_BASE_POLICY } };
			sessionsRef.current = sessionsRef.current.map((item) => item.id === next.id ? next : item);
			upsertSession(next);
		}
	}, []);
	// Declared after activeSessionId: the allowlist is scoped to the session,
	// so the hook needs a session to ask about.
	const computerUseState = useComputerUse(activeSessionId ?? null);
	const terminalWorkspaceId = activeSessionId ?? "default";
	const selectActivePermissions = useCallback((nextApprovalPolicy: ApprovalPolicy, nextSandboxMode: SandboxMode) => {
		const mode = approvalModeFromConfig(nextApprovalPolicy, nextSandboxMode);
		if (isDesktop) {
			void bridges.config?.updateDesktopPermissions({ approvalPolicy: nextApprovalPolicy, sandboxMode: nextSandboxMode })
				.catch((reason) => showToast(`Could not save machine permissions: ${publicError(reason)}`));
		}
		if (!activeSessionId) {
			setApprovalMode(mode); setApprovalPolicy(nextApprovalPolicy); setSandboxMode(nextSandboxMode);
			setPreferences(setPermissionPreferences(nextApprovalPolicy, nextSandboxMode));
			return;
		}
		const activeSession = sessionsRef.current.find((session) => session.id === activeSessionId);
		if (activeSession?.status === "running") {
			// An app-server attaches the approval policy when its turn starts. Do
			// not relabel an in-flight Ask turn as Allow all: preserve an honest
			// current label and save the requested mode as the default for the
			// next turn instead.
			setPreferences(setPermissionPreferences(nextApprovalPolicy, nextSandboxMode));
			showToast("Approval mode will apply after the current turn finishes.");
			return;
		}
		setApprovalMode(mode); setApprovalPolicy(nextApprovalPolicy); setSandboxMode(nextSandboxMode);
		setPreferences(setPermissionPreferences(nextApprovalPolicy, nextSandboxMode));
		const config = { approvalPolicy: nextApprovalPolicy, sandbox: nextSandboxMode };
		patchSessionMetadata(activeSessionId, { approvalMode: mode, ...config });
		void nativeCodex?.close(activeSessionId).catch((reason) => showToast(publicError(reason)));
	}, [activeSessionId, isDesktop, nativeCodex, showToast]);

	useEffect(() => {
		if (!activeSessionId) return;
		const session = sessions.find((candidate) => candidate.id === activeSessionId);
		if (!session || session.metadata.runtime !== "codex-app-server") return;
		if (session.status === "running") return;
		if (session.metadata.approvalPolicy === approvalPolicy && session.metadata.sandbox === sandboxMode) return;
		const mode = approvalModeFromConfig(approvalPolicy, sandboxMode);
		patchSessionMetadata(activeSessionId, { approvalMode: mode, approvalPolicy, sandbox: sandboxMode });
		void nativeCodex?.close(activeSessionId).catch((reason) => showToast(publicError(reason)));
	}, [activeSessionId, approvalPolicy, nativeCodex, sandboxMode, sessions, showToast]);

	useEffect(() => {
		let disposed = false;
		setWorkspaceScope(null);
		if (!activeSessionId || !bridges.workspaceScope) return () => { disposed = true; };
		void bridges.workspaceScope.get(activeSessionId).then((scope) => { if (!disposed) setWorkspaceScope(scope); }).catch(() => undefined);
		return () => { disposed = true; };
	}, [activeSessionId]);

	useForeignSessionEventBridge({
		activeSessionId,
		sessions,
		nativeIntern,
		refreshSessions,
		showToast,
		setCodexActivityBySession
	});

	const state = useMemo(() => {
		const base = buildLandingState({
			health,
			sessions,
			eventsBySession,
			codexActivityBySession,
			selectedTargetId,
			laguna,
			apiKeyConfigured,
			openrouterApiKeyConfigured: backendSettings?.openrouterApiKeyConfigured,
			codexOauthConfigured,
			codexOauthStatus,
			cloudBlockedReason: accountView.cloudBlockedReason
		});
		const archived = new Set(
			Object.entries(preferences.conversations)
				.filter(([, meta]) => meta.archived)
				.map(([id]) => id)
		);
		const chats = base.chats
			.filter((chat) => !archived.has(chat.id))
			.map((chat) => {
				const override = preferences.conversations[chat.id]?.titleOverride;
				return override ? { ...chat, title: override } : chat;
			});
		const withTitles = { ...base, chats };
		if (withTitles.model.status !== "downloading") return withTitles;
		return {
			...withTitles,
			model: { ...withTitles.model, downloadPaused }
		};
	}, [
		accountView.cloudBlockedReason,
		apiKeyConfigured,
		backendSettings?.openrouterApiKeyConfigured,
		codexOauthConfigured,
		codexOauthStatus,
		codexUsage,
		downloadPaused,
		eventsBySession,
		codexActivityBySession,
		health,
		laguna,
		preferences.conversations,
		selectedTargetId,
		sessions
	]);

	// Packaged Desktop requires a live provider-stream ownership receipt; a
	// stored `running` row alone may be crash debris. The browser runtime has no
	// child process or recovery lease, so its in-memory adapter's running rows
	// are the live ownership authority (and keep the browser demo/test harness
	// honest without weakening the packaged rule).
	const presentationLiveTurns = useMemo(() => {
		if (isDesktop) return liveTurns;
		const browserTurns = { ...liveTurns };
		for (const session of sessions) {
			if (session.status === "running" && !(session.id in browserTurns)) {
				browserTurns[session.id] = `browser:${session.id}`;
			}
		}
		return browserTurns;
	}, [isDesktop, liveTurns, sessions]);

	// Working requires a live turn owned by this instance, never a stored status.
	const workingChatIds = useMemo(
		() => selectWorkingChatIds(sessions, presentationLiveTurns),
		[presentationLiveTurns, sessions]
	);

	const chatPresence = useMemo(() => {
		const presence: Record<string, ChatPresence> = {};
		for (const session of sessions) {
			presence[session.id] = selectChatPresence(session, presentationLiveTurns);
		}
		return presence;
	}, [presentationLiveTurns, sessions]);

	/** Durable notices left by turns a previous Workshop process abandoned. */
	const recoveryNotices = useMemo(() => {
		const notices: Record<string, RecoveryNotice> = {};
		for (const session of sessions) {
			const notice = sessionRecoveryNotice(session);
			if (notice) notices[session.id] = notice;
		}
		return notices;
	}, [sessions]);

	const pinnedChatIds = useMemo(() => new Set(
		Object.entries(preferences.conversations)
			.filter(([, meta]) => meta.pinned)
			.sort((a, b) => (a[1].pinOrder ?? 0) - (b[1].pinOrder ?? 0))
			.map(([id]) => id)
	), [preferences.conversations]);

	const conversationTitles = useMemo(() => {
		const titles: Record<string, string> = {};
		for (const [id, meta] of Object.entries(preferences.conversations)) {
			if (meta.titleOverride) titles[id] = meta.titleOverride;
		}
		for (const session of sessions) {
			if (!titles[session.id]) titles[session.id] = session.title;
		}
		return titles;
	}, [preferences.conversations, sessions]);

	useEffect(() => {
		const previous = previousSessionStatusesRef.current;
		const completedOffscreen: string[] = [];
		for (const session of sessions) {
			const oldStatus = previous.get(session.id);
			const finished = session.status === "ready" || session.status === "interrupted" || session.status === "completed" || session.status === "failed";
			const visible = view.kind === "chat" && view.chatId === session.id;
			if (oldStatus === "running" && finished && session.target.kind !== "intern" && !visible) {
				completedOffscreen.push(session.id);
			}
			previous.set(session.id, session.status);
		}
		if (completedOffscreen.length === 0) return;
		setUnreadChatIds((current) => {
			const next = new Set(current);
			completedOffscreen.forEach((id) => next.add(id));
			setPreferences(setUnreadCompletedChats(next));
			return next;
		});
	}, [sessions, view]);

	useEffect(() => {
		drainPromptQueues(sessions, {
			refs: { statuses: queueDrainStatusesRef.current, draining: queueDrainingRef.current },
			send: (sessionId, text) => sendToSessionRef.current(sessionId, text),
			onAccepted: (promptId) => setPreferences(removeQueuedPrompt(promptId))
		});
	}, [sessions]);

	useEffect(() => {
		if (view.kind !== "chat" || !unreadChatIds.has(view.chatId)) return;
		setUnreadChatIds((current) => {
			const next = new Set(current);
			next.delete(view.chatId);
			setPreferences(setUnreadCompletedChats(next));
			return next;
		});
	}, [unreadChatIds, view]);

	const activeChat =
		view.kind === "chat" ? (state.chats.find((c) => c.id === view.chatId) ?? null) : null;
	const activeChatSession = activeChat
		? sessions.find((candidate) => candidate.id === activeChat.id)
		: undefined;
	const activeChatTargetId = activeChatSession
		? executionTargetToUiId(activeChatSession.target)
		: undefined;
	activeChatIdRef.current = activeChat?.id ?? null;
	const touchTranscriptCache = useCallback((sessionId: string) => {
		const lru = transcriptLruRef.current;
		lru.delete(sessionId);
		lru.set(sessionId, true);
		const evicted: string[] = [];
		while (lru.size > TRANSCRIPT_CACHE_LIMIT) {
			const candidate = [...lru.keys()].find((id) => {
				if (id === sessionId) return false;
				const session = sessionsRef.current.find((item) => item.id === id);
				return session?.status !== "running" && session?.status !== "waiting_for_input";
			});
			if (!candidate) break;
			lru.delete(candidate);
			const hydration = transcriptHydrationRef.current.get(candidate);
			if (hydration) hydration.generation = ++transcriptHydrationGenerationRef.current;
			transcriptHydrationRef.current.delete(candidate);
			evicted.push(candidate);
		}
		if (evicted.length === 0) return;
		evictSessionEvents(evicted);
		setTranscriptHistoryBySession((current) => Object.fromEntries(
			Object.entries(current).filter(([id]) => !evicted.includes(id))
		));
		for (const id of evicted) responseTraceStore.evict(id);
	}, []);
	const hydrateTranscript = useCallback((sessionId: string, targetKind: Session["target"]["kind"]) => {
		const core = bridges.core;
		if (!core) return;
		const cached = transcriptHydrationRef.current.get(sessionId);
		if (cached?.initialized || cached?.state === "loading") return;
		const generation = ++transcriptHydrationGenerationRef.current;
		transcriptHydrationRef.current.set(sessionId, {
			initialized: false,
			state: "loading",
			hasMore: false,
			generation
		});
		setTranscriptHistoryBySession((current) => ({
			...current,
			[sessionId]: { state: "loading", hasMore: false }
		}));
		responseTraceStore.setLoading(sessionId);
		void Promise.allSettled([
			core.sessionEventsTail(sessionId, TRANSCRIPT_INITIAL_PAGE_SIZE + 1),
			bridges.visuals?.list({ sessionId, limit: 500 }) ?? Promise.resolve([]),
			bridges.optimizers?.list({ sessionRef: sessionId, limit: 500 }) ?? Promise.resolve([])
		]).then(([journal, visualRegistry, optimizerRegistry]) => {
			// Hydration belongs to the session cache, not to the currently mounted
			// chat view. Navigation, target attachment and StrictMode effect replay
			// must not throw away a completed journal read. A generation changes only
			// when the cache entry itself is evicted or explicitly superseded.
			if (transcriptHydrationRef.current.get(sessionId)?.generation !== generation) return;
			// The journal is a conversation-history authority, while the visual
			// registry is the durable Outputs authority. Do not couple them: an
			// interrupted journal read during restart used to make an owned visual
			// disappear from Outputs even though the registry had successfully
			// reopened the same stable visual id.
			const fetched = journal.status === "fulfilled" ? journal.value : [];
			const ownedVisuals = visualRegistry.status === "fulfilled" ? visualRegistry.value : [];
			const ownedRuns = optimizerRegistry.status === "fulfilled" ? optimizerRegistry.value : [];
			const journalError = journal.status === "rejected" ? desktopBootError(journal.reason) : null;
			const visualRegistryError = visualRegistry.status === "rejected"
				? desktopBootError(visualRegistry.reason)
				: null;
			for (const run of ownedRuns) {
				if (run.source !== "local") continue;
				if (["completed", "succeeded", "failed", "cancelled", "degraded"].includes(run.status)) continue;
				void bridges.optimizers?.refresh(run.id);
			}
			const hasMore = fetched.length > TRANSCRIPT_INITIAL_PAGE_SIZE;
			const rows = hasMore ? fetched.slice(1) : fetched;
			const hydrated = rows
				.map(targetKind === "intern" ? appEventToRuntimeEvent : coreEventToRuntime)
				.filter((event): event is RuntimeEvent => event !== null);
			// Transcript hydration is intentionally bounded, but an optimizer can
			// create its visual early and then emit hundreds of token deltas. Restore
			// durable chat outputs from the visual registry as a separate authority so
			// an old visual cannot fall out of the Outputs shelf merely because it is
			// outside the transcript tail.
			const visualOutputs: RuntimeEvent[] = ownedVisuals
				.filter((visual) => visual.sessionId === sessionId)
				.map((visual, index) => ({
				schemaVersion: "synth.desktop-runtime-event.v1",
				sessionId,
				runId: visual.runId ?? null,
				sequence: -(index + 1),
				eventKind: "visual.created",
				payload: {
					visualId: visual.id,
					ownerSessionId: visual.sessionId ?? sessionId,
					title: visual.title,
					templateId: visual.templateId,
					bindings: visual.bindings,
					metadata: visual.metadata,
					status: visual.status,
					revision: visual.currentRevision,
					messageId: visual.messageId ?? undefined
				},
				createdAt: visual.createdAt,
				source: "local"
			}));
			mergeSessionReplay([[sessionId, [...visualOutputs, ...hydrated]]]);
			const hydratedHead = hydrated.at(-1)?.sequence ?? 0;
			nativeSequencesRef.current.set(sessionId, Math.max(
				nativeSequencesRef.current.get(sessionId) ?? 0,
				eventsBySessionRef.current[sessionId]?.at(-1)?.sequence ?? 0,
				hydratedHead
			));
			const error = journalError ?? visualRegistryError;
			transcriptHydrationRef.current.set(sessionId, {
				initialized: true,
				state: error ? "error" : "loaded",
				hasMore,
				earliestSequence: rows[0]?.sessionSequence ?? undefined,
				error: error ?? undefined,
				generation
			});
			setTranscriptHistoryBySession((current) => ({
				...current,
				[sessionId]: error
					? { state: "error", hasMore, error }
					: { state: "loaded", hasMore }
			}));
			if (!journalError) responseTraceStore.setJournal(sessionId, rows);
			else responseTraceStore.setError(sessionId, journalError);
			if (error) mergeSessionReplay([[sessionId, [{
				schemaVersion: "synth.desktop-runtime-event.v1",
				sessionId,
				sequence: 1,
				eventKind: "session/unhealthy",
				payload: {
					reason: journalError ? "session_replay_failed" : "visual_registry_replay_failed",
					message: journalError
						? `Conversation history could not be restored; saved Outputs remain available: ${journalError}`
						: `Saved Outputs could not be reconciled: ${visualRegistryError}`
				},
				createdAt: new Date().toISOString(),
				source: "local"
			}]]]);
		});
	}, []);
	useEffect(() => {
		const sessionId = activeChat?.id;
		const targetKind = activeChatSession?.target.kind;
		if (!runtimeBootReady || !sessionId || !targetKind) return;
		touchTranscriptCache(sessionId);
		const cached = transcriptHydrationRef.current.get(sessionId);
		if (cached) {
			setTranscriptHistoryBySession((current) => ({
				...current,
				[sessionId]: { state: cached.state, hasMore: cached.hasMore, error: cached.error }
			}));
		}
		hydrateTranscript(sessionId, targetKind);
	}, [activeChat?.id, activeChatSession?.target.kind, hydrateTranscript, runtimeBootReady, touchTranscriptCache]);
	const loadOlderTranscript = useCallback(() => {
		const sessionId = activeChatIdRef.current;
		const targetKind = activeChatSession?.target.kind;
		const core = bridges.core;
		if (!sessionId || !targetKind || !core) return;
		const prior = transcriptHydrationRef.current.get(sessionId);
		if (!prior?.initialized || prior.state === "loading" || !prior.hasMore || prior.earliestSequence == null) return;
		const generation = ++transcriptHydrationGenerationRef.current;
		const beforeSequence = prior.earliestSequence;
		transcriptHydrationRef.current.set(sessionId, { ...prior, state: "loading", error: undefined, generation });
		setTranscriptHistoryBySession((current) => ({
			...current,
			[sessionId]: { state: "loading", hasMore: true }
		}));
		void (async () => {
			const fetched = await core.sessionEventsBefore(sessionId, beforeSequence, TRANSCRIPT_PAGE_SIZE + 1);
			const hasMore = fetched.length > TRANSCRIPT_PAGE_SIZE;
			const rows = hasMore ? fetched.slice(1) : fetched;
			if (activeChatIdRef.current !== sessionId || transcriptHydrationRef.current.get(sessionId)?.generation !== generation) return;
			const hydrated = rows
				.map(targetKind === "intern" ? appEventToRuntimeEvent : coreEventToRuntime)
				.filter((event): event is RuntimeEvent => event !== null);
			mergeSessionReplay([[sessionId, hydrated]]);
			const entry: TranscriptHydrationEntry = {
				initialized: true,
				state: "loaded",
				hasMore,
				earliestSequence: rows[0]?.sessionSequence ?? beforeSequence,
				generation
			};
			transcriptHydrationRef.current.set(sessionId, entry);
			setTranscriptHistoryBySession((current) => ({
				...current,
				[sessionId]: { state: "loaded", hasMore }
			}));
			touchTranscriptCache(sessionId);
		})().catch((reason: unknown) => {
			if (activeChatIdRef.current !== sessionId || transcriptHydrationRef.current.get(sessionId)?.generation !== generation) return;
			const message = desktopBootError(reason);
			transcriptHydrationRef.current.set(sessionId, { ...prior, state: "error", error: message, generation });
			setTranscriptHistoryBySession((current) => ({
				...current,
				[sessionId]: { state: "error", hasMore: prior.hasMore, error: message }
			}));
		});
	}, [activeChatSession?.target.kind, touchTranscriptCache]);
	// Restored layout state opens a chat without going through openChat(). Keep
	// the composer bound to the conversation's persisted execution target so a
	// Gemini thread never silently presents (or submits through) Laguna.
	useEffect(() => {
		if (activeChatTargetId) setSelectedTargetId(activeChatTargetId);
	}, [activeChat?.id, activeChatTargetId]);
	// Session status + event arbitration — single selector, not an App.tsx IIFE.
	const activeChatRunning = activeChat
		? selectSessionRunning(activeChatSession, eventsBySession[activeChat.id] ?? [], presentationLiveTurns)
		: false;
	const activeChatInferencePhase = chatInferencePhase({
		running: activeChatRunning,
		targetKind: activeChatSession?.target.kind ?? null,
		targetModel: activeChatSession?.target.kind === "cloud" ? activeChatSession.target.model : null,
		lastMessageRole: activeChat?.messages.at(-1)?.role ?? null,
		localPhase: laguna?.phase ?? null,
		localLoadedModel: laguna?.loadedModel ?? null
	});
	const activeChatWarmingUp = activeChatInferencePhase === "warming";
	const activeLocalModel = activeChatSession?.target.kind === "local";
	const workbenchWidth = viewportWidth - (sidebarVisible ? sidebarWidth : 0);
	const sidePanelFits = workbenchWidth >= 368 + 300;
	const sidePanelCanSharePane = workbenchWidth >= 380 + 7 + 260 + 300;
	const showSidePanel = sidePanelOpen && sidePanelFits && (
		sidePanelTab === "outputs"
		|| sidePanelTab === "trace"
		|| sidePanelTab === "diagnostics"
		|| sidePanelTab === "errors"
		|| activeLocalModel
	);
	const activeSync =
		view.kind === "sync"
			? (state.syncSessions.find((s) => s.id === view.sessionId) ?? null)
			: null;

	const contextualArtifact = view.kind === "chat" && activeChat
		? (activeChat.artifacts?.find((a) => a.id === openArtifactId) ?? null)
		: view.kind === "sync" && activeSync
			? (activeSync.artifacts?.find((a) => a.id === openArtifactId) ?? null)
			: null;
	const selectedOpenArtifact = newestVisualArtifact(
		openArtifactId,
		visualRevisionState.artifact,
		contextualArtifact
	) ?? contextualArtifact;
	const openArtifact = selectedOpenArtifact && visualRevisionState.id === openArtifactId
		? {
			...selectedOpenArtifact,
			metadata: {
				...selectedOpenArtifact.metadata,
				revisionSync: {
					loading: visualRevisionState.loading,
					requestedRevision: visualRevisionState.requestedRevision,
					acceptedRevision: visualRevisionState.acceptedRevision,
					error: visualRevisionState.error
				}
			}
		}
		: selectedOpenArtifact;
	openArtifactIdRef.current = openArtifactId;
	acceptedVisualRevisionRef.current = {
		id: openArtifactId,
		revision: typeof openArtifact?.revision === "number" ? openArtifact.revision : -1
	};

	useEffect(() => {
		if (!openArtifactId || !contextualArtifact?.visualId) return;
		dispatchVisualRevision({ type: "select", id: openArtifactId, artifact: contextualArtifact });
	}, [contextualArtifact, openArtifactId]);

	const windowPane =
		view.kind === "chat" ||
		view.kind === "visuals" ||
		view.kind === "experiments" ||
		view.kind === "optimizers" ||
		view.kind === "inventory" ||
		view.kind === "reports" ||
		(view.kind === "settings" && Boolean(openArtifactId));
	const viewKey =
		windowPane
			? "window"
			: view.kind === "sync"
				? `sync:${view.sessionId}`
				: view.kind === "async"
					? `async:${view.sessionId}`
					: view.kind;

	useEffect(() => {
		if (windowPane && openArtifactIdRef.current) {
			openArtifactByViewRef.current[viewKey] = openArtifactIdRef.current;
			return;
		}
		const hasRemembered = Object.prototype.hasOwnProperty.call(openArtifactByViewRef.current, viewKey);
		let remembered = hasRemembered ? openArtifactByViewRef.current[viewKey] : null;
		if (!hasRemembered) {
			const persistedId =
				(typeof activeChatSession?.metadata?.openVisualId === "string"
					? activeChatSession.metadata.openVisualId
					: null)
				?? (view.kind === "chat"
					? activeChat?.artifacts?.find((artifact) => artifact.templateId !== "synth.subagents.v1")?.id ?? null
					: view.kind === "sync"
						? activeSync?.artifacts?.find((artifact) => artifact.templateId !== "synth.subagents.v1")?.id ?? null
						: null);
			if (persistedId) {
				remembered = persistedId;
				openArtifactByViewRef.current[viewKey] = persistedId;
			}
		}
		setOpenArtifactId(remembered);
		openArtifactIdRef.current = remembered;
		visualRequestGenerationRef.current += 1;
		pendingVisualRefreshRef.current = null;
		dispatchVisualRevision(remembered ? { type: "select", id: remembered } : { type: "close" });
		setOpenContainer(null);
		setContainerPaneExpanded(false);
	}, [viewKey, windowPane, activeChatSession?.metadata?.openVisualId, activeChat?.id, activeSync?.id]);

	useEffect(() => {
		if (!openArtifactId) return;
		// Chat shares the window VisualPane with Visuals / Experiments / Optimizers / Data.
		// Transcript Outputs still use ownedChatArtifacts; they do not evict the pane.
		if (view.kind !== "sync" && view.kind !== "async") return;
		const artifacts = activeSync?.artifacts ?? [];
		if (!openArtifactIdForChat(openArtifactId, artifacts)) {
			setOpenArtifactId(null);
			openArtifactIdRef.current = null;
			visualRequestGenerationRef.current += 1;
			pendingVisualRefreshRef.current = null;
			dispatchVisualRevision({ type: "close" });
		}
	}, [openArtifactId, view.kind, activeSync]);

	useEffect(() => {
		if (openArtifactId || openContainer) return;
		const surface = activeChat ?? activeSync;
		const subagents = surface?.artifacts?.find((artifact) => artifact.templateId === "synth.subagents.v1");
		if (!surface || !subagents || autoOpenedSubagentsRef.current.has(surface.id)) return;
		autoOpenedSubagentsRef.current.add(surface.id);
		setOpenArtifactId(subagents.id);
	}, [activeChat, activeSync, openArtifactId, openContainer]);

	useEffect(() => {
		if (!openArtifactId && !openContainer) return;
		const onKey = (e: KeyboardEvent) => {
			if (e.key === "Escape") {
				if (e.defaultPrevented) return;
				setOpenArtifactId(null);
				openArtifactIdRef.current = null;
				visualRequestGenerationRef.current += 1;
				pendingVisualRefreshRef.current = null;
				dispatchVisualRevision({ type: "close" });
				setOpenContainer(null);
				setContainerPaneExpanded(false);
			}
		};
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [openArtifactId, openContainer]);

	const toggleArtifact = useCallback((id: string | null) => {
		if (id == null) {
			openArtifactByViewRef.current[viewKey] = null;
			setOpenArtifactId(null);
			openArtifactIdRef.current = null;
			visualRequestGenerationRef.current += 1;
			pendingVisualRefreshRef.current = null;
			dispatchVisualRevision({ type: "close" });
			return;
		}
		setOpenContainer(null);
		setOpenArtifactId((current) => {
			const next = current === id ? null : id;
			openArtifactIdRef.current = next;
			openArtifactByViewRef.current[viewKey] = next;
			visualRequestGenerationRef.current += 1;
			pendingVisualRefreshRef.current = null;
			dispatchVisualRevision(next ? { type: "select", id: next } : { type: "close" });
			return next;
		});
	}, [viewKey]);

	const toggleContainer = useCallback(async (id: string | null) => {
		if (!id || openContainer?.id === id) {
			setOpenContainer(null);
			setContainerPaneExpanded(false);
			return;
		}
		if (!bridges.inventory) {
			showToast("Container inventory requires Synth Desktop");
			return;
		}
			try {
			const container = await bridges.inventory.getContainer(id);
			setOpenArtifactId(null);
			openArtifactIdRef.current = null;
			visualRequestGenerationRef.current += 1;
			pendingVisualRefreshRef.current = null;
			dispatchVisualRevision({ type: "close" });
			setOpenContainer(container);
		} catch (reason) {
			showToast(publicError(reason));
		}
	}, [openContainer?.id, showToast]);

	const probeOpenContainer = useCallback(async () => {
		if (!openContainer || !bridges.inventory) return;
		try {
			const container = await bridges.inventory.probeContainer(openContainer.id);
			setOpenContainer(container);
			showToast(container.status === "ready" ? `${container.name} is ready.` : `Couldn't reach ${container.name}.`);
		} catch (reason) {
			showToast(publicError(reason));
		}
	}, [openContainer, showToast]);

	const repairOpenContainer = useCallback(async () => {
		if (!openContainer || !bridges.inventory) return;
		const sessionId = activeChatIdRef.current;
		if (!sessionId) {
			showToast("Open a conversation with this repository attached, then try again.");
			return;
		}
		try {
			const container = await bridges.inventory.reconcileContainer(openContainer.id, sessionId);
			setOpenContainer(container);
			showToast(`${container.name} launch files re-read.`);
		} catch (reason) {
			showToast(publicError(reason));
		}
	}, [openContainer, showToast]);

	const restartOpenContainer = useCallback(async () => {
		if (!openContainer || !bridges.inventory) return;
		const sessionId = activeChatIdRef.current;
		if (!sessionId) {
			showToast("Open a conversation with this repository attached, then try again.");
			return;
		}
		try {
			const container = await bridges.inventory.restartContainer(openContainer.id, sessionId);
			setOpenContainer(container);
			showToast(container.status === "ready" ? `${container.name} is ready.` : `Couldn't reach ${container.name}.`);
		} catch (reason) {
			showToast(publicError(reason));
		}
	}, [openContainer, showToast]);

	const openVisualRecord = useCallback((visual: VisualInstanceRecord | VisualRecord) => {
		const artifact =
			"schemaVersion" in visual && visual.schemaVersion === "synth.desktop-visual.v1"
				? artifactFromVisualRecord(visual as VisualRecord)
				: visualRecordToArtifact(visual as VisualInstanceRecord);
		openArtifactIdRef.current = artifact.id;
		openArtifactByViewRef.current[viewKey] = artifact.id;
		acceptedVisualRevisionRef.current = { id: artifact.id, revision: artifact.revision ?? -1 };
		visualRequestGenerationRef.current += 1;
		pendingVisualRefreshRef.current = null;
		dispatchVisualRevision({ type: "select", id: artifact.id, artifact });
		setOpenArtifactId(artifact.id);
		// Opening an artifact is a side-pane action. Navigation remains explicit so
		// traces stay beside their catalog and chat-created visuals stay beside chat.
	}, [viewKey]);

	const reconcileOpenVisual = useCallback((visualId: string, minimumRevision = -1, open = false) => {
		if (!bridges.visuals) return;
		const wasOpen = openArtifactIdRef.current === visualId;
		if (open) {
			if (openArtifactIdRef.current !== visualId) {
				visualRequestGenerationRef.current += 1;
				pendingVisualRefreshRef.current = null;
			}
			openArtifactIdRef.current = visualId;
			setOpenArtifactId(visualId);
			dispatchVisualRevision({ type: "select", id: visualId });
		} else if (openArtifactIdRef.current !== visualId) {
			return;
		}
		if (
			minimumRevision >= 0 &&
			acceptedVisualRevisionRef.current.id === visualId &&
			acceptedVisualRevisionRef.current.revision >= minimumRevision &&
			(!open || wasOpen)
		) return;
		const pending = pendingVisualRefreshRef.current;
		if (pending?.id === visualId && pending.minimumRevision >= minimumRevision) return;
		const generation = ++visualRequestGenerationRef.current;
		pendingVisualRefreshRef.current = { id: visualId, minimumRevision, generation };
		dispatchVisualRevision({ type: "request", id: visualId, minimumRevision, generation });
		void bridges.visuals.get(visualId).then(
			(visual) => {
				if (openArtifactIdRef.current !== visualId || visualRequestGenerationRef.current !== generation) return;
				const artifact = artifactFromVisualRecord(visual);
				acceptedVisualRevisionRef.current = { id: visualId, revision: artifact.revision ?? -1 };
				dispatchVisualRevision({ type: "resolve", id: visualId, artifact, generation });
				if (pendingVisualRefreshRef.current?.generation === generation) pendingVisualRefreshRef.current = null;
			},
			(reason) => {
				if (openArtifactIdRef.current !== visualId || visualRequestGenerationRef.current !== generation) return;
				dispatchVisualRevision({ type: "fail", id: visualId, generation, error: publicError(reason) });
				if (pendingVisualRefreshRef.current?.generation === generation) pendingVisualRefreshRef.current = null;
			}
		);
	}, []);

	useEffect(() => {
		if (openArtifactId && contextualArtifact?.visualId === openArtifactId) {
			reconcileOpenVisual(openArtifactId, contextualArtifact.revision ?? -1);
		}
	}, [contextualArtifact?.visualId, openArtifactId, reconcileOpenVisual]);

	useEffect(() => {
		if (!bridges.visuals) return;
		const reconcileSelected = () => {
			const visualId = openArtifactIdRef.current;
			if (visualId) reconcileOpenVisual(visualId);
		};
		const unlisten = bridges.visuals.onEvent((event) => {
			// Visual events are durable CoreRuntime session events, but Codex's
			// provider bridge intentionally projects only provider traffic. Fold the
			// visual lane into the active session store as it arrives so a visual
			// created by an optimizer appears in Outputs immediately; otherwise the
			// pane could open while the same chat incorrectly said "No outputs yet"
			// until a full transcript rehydrate after restart.
			if (event.sessionId) {
				const runtimeEvent = appEventToRuntimeEvent(event);
				if (runtimeEvent) dispatchRuntimeEvent(runtimeEvent, { updateStatus: false });
			}
			const visualId =
				typeof event.payload?.visualId === "string" ? event.payload.visualId : null;
			if (!visualId) return;
			const eventRevision = typeof event.payload?.revision === "number" ? event.payload.revision : -1;
			if (event.kind === "visual.show") {
				const owner =
					typeof event.payload?.ownerSessionId === "string"
						? event.payload.ownerSessionId
						: typeof event.sessionId === "string"
							? event.sessionId
							: null;
				const ownerViewKey = owner ? `chat:${owner}` : viewKey;
				openArtifactByViewRef.current[ownerViewKey] = visualId;
				openArtifactByViewRef.current.window = visualId;
				if (owner && owner !== activeSessionIdRef.current) {
					return;
				}
				reconcileOpenVisual(visualId, eventRevision, true);
			}
			else if (event.kind === "visual.updated" && openArtifactIdRef.current === visualId) {
				reconcileOpenVisual(visualId, eventRevision);
			}
		}, reconcileSelected);
		window.addEventListener("focus", reconcileSelected);
		// Covers browser fixtures whose listener is synchronous and older bridges
		// without an attachment callback. The callback repeats this fetch after the
		// real Tauri listener promise resolves, closing the attach gap.
		reconcileSelected();
		return () => {
			unlisten();
			window.removeEventListener("focus", reconcileSelected);
		};
	}, [reconcileOpenVisual, viewKey]);

	const ensureOpenRouterReady = useCallback(async (targetId: string): Promise<boolean> => {
		if (!isOpenRouterCatalogTarget(targetId)) return true;
		const config = await bridges.config?.get().catch(() => null);
		const configured = config?.openrouterApiKeyConfigured ?? health?.openrouter.mode === "ready";
		if (configured) {
			if (config) setBackendSettings(config);
			return true;
		}
		showToast("OpenRouter API key required — message was not sent");
		setView({ kind: "settings", section: "account" });
		return false;
	}, [health?.openrouter.mode, showToast]);

	const ensureCodexOauthReady = useCallback(async (targetId: string): Promise<boolean> => {
		if (!targetId.startsWith("chatgpt-")) return true;
		const status = await bridges.codexOauth?.ensureReady().catch(() => null);
		if (status) setCodexOauthStatus(status);
		if (status?.canUseModels) {
			setCodexOauthConfigured(true);
			return true;
		}
		setCodexOauthConfigured(false);
		showToast(status?.guidance ?? "ChatGPT authorization status could not be verified. Message was not sent.");
		setView({ kind: "settings", section: "models" });
		return false;
	}, [showToast]);

	const createConversation = useCallback(
		async (
			targetId: string = selectedTargetId,
			title?: string,
			objective?: string,
			options?: { deferNativeStart?: boolean }
		) => {
			setBusy(true);
			try {
				if (isOpenRouterCatalogTarget(targetId) && !canStartNewTurn(targetId)) {
					const entry = modelCatalog().entries.find((candidate) => candidate.targetId === targetId);
					throw new Error(entry?.diagnostic ?? "This OpenRouter target is unavailable for new turns.");
				}
				const target = targetIdToExecutionTarget(
					targetId,
					targetId === "local-laguna" ? selectedLagunaAdapterId : null
				);
				const internObjective = objective?.trim();
				if (target.kind === "intern" && !internObjective) {
					throw new Error("Enter an objective to start an Intern session");
				}
				if (nativeCodex && target.kind !== "intern") {
					const id = crypto.randomUUID();
					// Every local/configured-provider task starts in the configured safe workspace.
					const workspace = await nativeCodex.defaultWorkspace();
					const machinePermissions = await loadMachinePermissions();
					const permissions = {
						approvalPolicy: machinePermissions.approvalPolicy,
						sandbox: machinePermissions.sandboxMode
					};
					// A first-message send is owned by sendTurn(), which atomically
					// attaches/starts the app-server, journals the prompt, and starts
					// the turn. Eagerly starting here creates a long provisioning gap
					// before the prompt has durable custody; if the landing composer
					// is replaced during that gap, the first message can disappear.
					// Explicit New conversation actions still start eagerly.
					if (!options?.deferNativeStart) {
						await nativeCodex.start(codexStartRequest(id, workspace, target, permissions, preferences.agentContext.autoCompactTokenLimits, laguna?.baseUrl ?? undefined, serviceTierForExecutionTarget(target, modelKnobValues) ?? "default"));
					}
					const session = createCodexSession(id, target, null, workspace, title, permissions);
					sessionsRef.current = [session, ...sessionsRef.current.filter((item) => item.id !== session.id)];
					upsertSession(session);
					// This process owns the new session from its first optimistic event onward.
					// There is no persisted history to replay, so do not put first-message UX
					// behind a journal read (or make Advanced start a competing one).
					const generation = ++transcriptHydrationGenerationRef.current;
					transcriptHydrationRef.current.set(id, {
						initialized: true,
						state: "loaded",
						hasMore: false,
						generation
					});
					setTranscriptHistoryBySession((current) => ({
						...current,
						[id]: { state: "loaded", hasMore: false }
					}));
					responseTraceStore.markLoaded(id);
					setView({ kind: "chat", chatId: session.id });
					return session;
				}
				const session = target.kind === "intern" && nativeIntern
					? await nativeIntern.createSession({ target, objective: internObjective!, title, projectId: null })
					: await browserRuntimeClient.createSession(target, title, internObjective);
				await refreshSessions();
				if (sessionIsLocalChat(session)) {
					setView({ kind: "chat", chatId: session.id });
				} else if (sessionIsSync(session)) {
					setView({ kind: "sync", sessionId: session.id });
				} else if (sessionIsAsync(session)) {
					setView({ kind: "async", sessionId: session.id });
				}
				return session;
			} catch (reason) {
				// Tauri serializes `AppError` as a plain object. Preserve its safe
				// message here; otherwise a provider/session-start rejection becomes
				// the unusable "[object Object]" in the composer toast.
				const message = reason instanceof Error
					? reason.message
					: turnFailureMessage(codexTurnFailure("new-session", reason));
				showToast(message);
				throw new Error(message);
			} finally {
				setBusy(false);
			}
		},
		[laguna?.baseUrl, loadMachinePermissions, modelKnobValues, nativeCodex, nativeIntern, preferences.agentContext.autoCompactTokenLimits, refreshSessions, selectedLagunaAdapterId, selectedTargetId, showToast]
	);

	const ensureActiveSession = useCallback(async (objective: string): Promise<{ sessionId: string; objectiveConsumed: boolean } | null> => {
		if (activeSessionId && sessionsRef.current.some((session) => session.id === activeSessionId)) {
			return { sessionId: activeSessionId, objectiveConsumed: false };
		}
		// A persisted selection can outlive the underlying session record. Treat
		// that empty Chat shell like the landing page instead of sending to a UUID
		// that the native bridge no longer owns.
		if (view.kind !== "landing" && view.kind !== "chat") return null;
		const target = targetIdToExecutionTarget(selectedTargetId);
		const objectiveConsumed = target.kind === "intern";
		const session = await createConversation(
			selectedTargetId,
			undefined,
			objectiveConsumed ? objective : undefined,
			{ deferNativeStart: !objectiveConsumed }
		);
		return { sessionId: session.id, objectiveConsumed };
	}, [activeSessionId, createConversation, selectedTargetId, view.kind]);

	const sendToSession = useCallback(
		async (
			sessionId: string,
			text: string,
			options?: {
				messageId?: string;
				images?: ComposerImageAttachment[];
				readinessVerified?: boolean;
				recoveryMode?: boolean;
			}
		) => {
			try {
				const session = sessionsRef.current.find((candidate) => candidate.id === sessionId);
				const sessionTargetId = session ? executionTargetToUiId(session.target) : selectedTargetId;
				const pendingTargetId = isInternTargetId(selectedTargetId) ? sessionTargetId : selectedTargetId;
				// The landing composer verifies provider readiness before creating the
				// session. Repeating an OAuth probe after createConversation has changed
				// routes leaves the new session visible but the first prompt unjournaled
				// when that second probe stalls. Direct/session-specific sends still own
				// their readiness check.
				if (!options?.readinessVerified) {
					if (!await ensureOpenRouterReady(pendingTargetId)) return false;
					if (!await ensureCodexOauthReady(pendingTargetId)) return false;
				}
				setBusy(true);
				if (nativeCodex && (!session || session.target.kind !== "intern")) {
					if (!session) throw new Error(`Native Codex session is not registered: ${sessionId}`);
					if (session.metadata.runtime !== "codex-app-server") {
						throw new Error(`Session ${sessionId} is not owned by Codex app-server`);
					}
					const sendPlan = planComposerSend({
						pendingTargetId,
						sessionTargetId,
						threadHasHistory: threadHasHistoryFromEvents(eventsBySessionRef.current[sessionId] ?? []),
						turnRunning: session.status === "running",
						hasPendingImages: Boolean(options?.images?.length),
						destinationSupportsImages: modelSupportsImageInput(pendingTargetId)
					});
					if (sendPlan.kind === "block") {
						showToast(sendPlan.message);
						return false;
					}
					const executionTarget = sendPlan.kind === "model_switch_then_turn"
						? targetIdToExecutionTarget(
							sendPlan.destinationTargetId,
							sendPlan.destinationTargetId === "local-laguna" ? selectedLagunaAdapterId : null
						)
						: session.target.kind === "local"
							? { ...session.target, adapter: selectedLagunaAdapterId }
							: session.target;
					const workspace = typeof session.metadata.workspace === "string"
						? session.metadata.workspace
						: await nativeCodex.defaultWorkspace();
					const startRequest = {
						...codexStartRequest(sessionId, workspace, executionTarget, "ask", preferences.agentContext.autoCompactTokenLimits, laguna?.baseUrl ?? undefined, serviceTierForExecutionTarget(executionTarget, modelKnobValues) ?? "default"),
						// The session's sealed policy metadata is the authority; the
						// preference-seeded values cover only restored pre-policy sessions
						// that carry no metadata. Never send undefined, which Rust would
						// resolve against machine config as if nothing had been chosen.
						approvalPolicy: typeof session.metadata.approvalPolicy === "string" ? session.metadata.approvalPolicy : approvalPolicy,
						sandbox: typeof session.metadata.sandbox === "string" ? session.metadata.sandbox : sandboxMode,
						threadId: typeof session.metadata.threadId === "string" ? session.metadata.threadId : undefined
					};
					const sequence = allocateNativeSequence(sessionId);
					const now = new Date().toISOString();
					// The typed text is shown immediately and is never removed.
					// A retry reuses the same message id so the bubble is
					// updated in place instead of duplicated.
					const messageId = options?.messageId ?? `user-${sequence}`;
					dispatchRuntimeEvent({
						schemaVersion: "synth.desktop-runtime-event.v1", sessionId, sequence,
						eventKind: "message.created", payload: { messageId, role: "user", content: text },
						createdAt: now, source: "local"
					});
						const effort = turnStartEffortForExecutionTarget(executionTarget, modelKnobValues);
						let started: CodexSessionInfo;
						// Arm before awaiting. The app-server may emit run.started before
						// its RPC reply reaches us; arming afterwards would miss that event.
						armTurnStartWatchdog(sessionId, text, messageId);
						// A fence from an earlier completed Stop must never apply to this
						// new operator message. A Stop during the async start handshake
						// re-adds it and is handled immediately after sendTurn resolves.
						staleRunFenceRef.current.delete(sessionId);
						try {
						// One round trip owns attach/resume and turn/start, so the
						// app-server cannot exit in a gap the renderer can see.
						// Model switches compact on the source model inside sendTurn
						// before rebind (see modelSwitchPlan.ts).
						// Pass the optimistic messageId so Rust journals the same id
						// and eventsToMessages collapses host+optimistic into one bubble.
						started = nativeCodex.sendTurn
							? await nativeCodex.sendTurn(
								startRequest,
								text,
								effort,
								{
									compactBeforeModelSwitch: sendPlan.kind === "model_switch_then_turn" ? sendPlan.compact : false,
									clientMessageId: messageId,
									recoveryMode: Boolean(options?.recoveryMode)
								}
							)
							: await (async () => {
								await nativeCodex.start(startRequest);
								return nativeCodex.startTurn(sessionId, text, effort, { clientMessageId: messageId });
							})();
						} catch (reason) {
							clearTurnStartWatchdog(sessionId);
							failTurnStart(sessionId, text, messageId, reason);
							return false;
						}
					// Stop may be pressed while sendTurn is still attaching the app-server,
					// before Rust has a turn id to interrupt. Preserve that intent across
					// the handshake and interrupt the newly-created turn immediately.
					if (staleRunFenceRef.current.has(sessionId)) {
						await nativeCodex.interrupt(sessionId);
						staleRunFenceRef.current.delete(sessionId);
						dispatchLocalSessionStatus(sessionId, "interrupted");
						return false;
					}
					staleRunFenceRef.current.delete(sessionId);
					setFailedSend((current) => (current?.sessionId === sessionId ? null : current));
					// Working only appears once a real turn exists. Without a
					// turn id the run.started event promotes the status instead.
					dispatchTurnAccepted(sessionId, {
						target: executionTarget,
						turnId: started?.turnId
					});
					return true;
				}
				if (session?.target.kind === "intern" && nativeIntern) {
					await nativeIntern.send({ sessionId, body: text });
				} else {
					await browserRuntimeClient.sendMessage(sessionId, text);
				}
				await refreshSessions();
				return true;
			} catch (reason) {
				showToast(reason instanceof Error
					? reason.message
					: turnFailureMessage(codexTurnFailure(sessionId, reason)));
				return false;
			} finally {
				setBusy(false);
			}
		},
		[allocateNativeSequence, approvalPolicy, armTurnStartWatchdog, clearTurnStartWatchdog, ensureCodexOauthReady, ensureOpenRouterReady, failTurnStart, laguna?.baseUrl, modelKnobValues, nativeCodex, nativeIntern, preferences.agentContext.autoCompactTokenLimits, refreshSessions, sandboxMode, selectedLagunaAdapterId, selectedTargetId, showToast]
	);
	sendToSessionRef.current = sendToSession;

	const retryFailedSend = useCallback(() => {
		const pending = failedSend;
		if (!pending) return;
		setFailedSend(null);
		void sendToSession(pending.sessionId, pending.text, { messageId: pending.messageId });
	}, [failedSend, sendToSession]);

	/**
	 * Continue an abandoned turn in its existing Codex thread.
	 *
	 * `sendToSession` first reattaches with app-server `thread/resume`; this new
	 * turn therefore sees the persisted conversation and workspace state. Never
	 * replay the original prompt here: doing so can duplicate commands, paid
	 * requests, or external writes which completed immediately before the host
	 * disappeared. The host links the continuation through `recoveredFromRunId`.
	 */
	const resumeRecoveredChat = useCallback((sessionId: string) => {
		const notice = recoveryNotices[sessionId];
		if (!notice) {
			showToast("This chat has no interrupted work to resume.");
			return;
		}
		if (!notice.restartable) {
			showToast(
				notice.needsAttention
					? "Check whether the previous work completed before retrying."
					: "This task already started work that would be duplicated by a retry."
			);
			return;
		}
		void sendToSession(
			sessionId,
			"Continue the interrupted task from this persisted conversation and the current workspace state. Reconcile completed commands, file changes, approvals, and external objects before acting. Do not repeat completed or externally committed operations. Continue toward the original user goal. If any consequential state cannot be verified, stop and report the specific uncertainty instead of guessing.",
			{ recoveryMode: true }
		);
	}, [recoveryNotices, sendToSession, showToast]);

	const onComposerSend = useCallback(
		async (text: string, images: ComposerImageAttachment[] = []) => {
			try {
				if (!await ensureOpenRouterReady(selectedTargetId)) return;
				if (!await ensureCodexOauthReady(selectedTargetId)) return;
				const ensured = await ensureActiveSession(text);
				if (!ensured) {
					showToast("No active session");
					return;
				}
				// Intern creation itself starts the objective. Sending the same text
				// again would issue a duplicate operator command.
				if (!ensured.objectiveConsumed) {
					await sendToSession(ensured.sessionId, text, { images, readinessVerified: true });
				}
			} catch {
				/* toast already shown */
			}
		},
		[ensureActiveSession, ensureCodexOauthReady, ensureOpenRouterReady, selectedTargetId, sendToSession, showToast]
	);

	const controlActive = useCallback(
		async (kind: RuntimeControlKind, payload: Record<string, unknown> = {}) => {
			if (!activeSessionId) return;
			setBusy(true);
			try {
				const session = sessions.find((candidate) => candidate.id === activeSessionId);
				if (nativeCodex && session?.metadata.runtime === "codex-app-server") {
					if (kind === "cancel" || kind === "pause") {
						// Also fences a run.started event that lands after a warming-up stop.
						staleRunFenceRef.current.add(activeSessionId);
					}
					if (kind === "close") await nativeCodex.close(activeSessionId);
					else if (kind === "cancel" || kind === "pause") await nativeCodex.interrupt(activeSessionId);
					else if (kind === "approve" || kind === "reject") {
						const approvalId = typeof payload.approvalId === "string" ? payload.approvalId : null;
						if (!approvalId) throw new Error("Approval id is missing");
						const requestedDecision = payload.decision;
						const decision = kind === "reject"
							? "reject"
							: requestedDecision === "always" || requestedDecision === "remember-locator" || requestedDecision === "register-source"
								? requestedDecision
								: "once";
						await nativeCodex.resolveApproval(activeSessionId, approvalId, decision);
					}
					else throw new Error(`${kind} is not supported for a Codex session`);
					if (kind === "close" || kind === "cancel" || kind === "pause") {
						dispatchLocalSessionStatus(
							activeSessionId,
							kind === "close" ? "completed" : kind === "cancel" ? "cancelled" : "interrupted"
						);
					}
					return;
				}
				if (session?.target.kind === "intern" && nativeIntern) {
					await nativeIntern.control({ sessionId: activeSessionId, kind, payload });
				} else {
					await browserRuntimeClient.control(activeSessionId, kind, payload);
				}
				await refreshSessions();
			} catch (reason) {
				const failure = codexTurnFailure(activeSessionId, reason);
				const message = turnFailureMessage(failure);
				// A persisted approval card can outlive the in-memory app-server
				// attachment after an app restart. Treat that as an unhealthy
				// session, rather than showing the opaque `Codex session <UUID>`
				// context string in a toast while the transcript still says Working.
				if ((kind === "approve" || kind === "reject") && failure.code === CODEX_SESSION_UNHEALTHY) {
					dispatchLocalSessionStatus(activeSessionId, "interrupted", { onlyIf: "running" });
					const sequence = allocateNativeSequence(activeSessionId);
					dispatchRuntimeEvent({
						schemaVersion: "synth.desktop-runtime-event.v1",
						sessionId: activeSessionId,
						sequence,
						eventKind: "session/unhealthy",
						payload: { reason: failure.code, message },
						createdAt: new Date().toISOString(),
						source: "local"
					}, { updateStatus: false });
				}
				showToast(message);
			} finally {
				setBusy(false);
			}
		},
		[activeSessionId, nativeCodex, nativeIntern, refreshSessions, sessions, showToast]
	);

	const onSelectTarget = useCallback((id: string) => {
		if (isInternTargetId(id)) return;
		// Model chip change only updates pendingTarget. Compact/rebind happen
		// on the next send when pending ≠ session.target (modelSwitchPlan.ts).
		const plan = planModelChipChange({ nextTargetId: id });
		setSelectedTargetId(plan.pendingTargetId);
	}, []);

	const onNewConversation = useCallback(() => {
		void loadMachinePermissions()
			.catch((reason) => showToast(`Could not load machine permissions: ${publicError(reason)}`))
				.finally(() => {
					setView({ kind: "landing" });
					setOpenArtifactId(null);
					openArtifactIdRef.current = null;
					visualRequestGenerationRef.current += 1;
					pendingVisualRefreshRef.current = null;
					dispatchVisualRevision({ type: "close" });
			});
	}, [loadMachinePermissions, showToast]);

	const openChat = useCallback((id: string) => {
		setView({ kind: "chat", chatId: id });
		const session = sessionsRef.current.find((candidate) => candidate.id === id);
		if (session && sessionIsLocalChat(session)) {
			// Opening a thread adopts its bound model as pendingTarget so a
			// leftover chip from another chat cannot silently switch on send.
			setSelectedTargetId(executionTargetToUiId(session.target));
			if (session.target.kind === "local") {
				setSelectedLagunaAdapterId(session.target.model ?? null);
			}
		}
	}, [showToast]);

	useEffect(() => {
		void bridges.skills?.list()
			.then((hits) => setComposerSkills(hits.map((hit) => ({
				id: hit.id,
				name: hit.name,
				description: hit.description
			}))))
			.catch(() => undefined);
	}, []);

	const onSlashRename = useCallback(() => {
		if (!activeSessionId) {
			showToast("No active conversation to rename");
			return;
		}
		const current = sessionsRef.current.find((session) => session.id === activeSessionId);
		const next = window.prompt("Rename conversation", current?.title ?? "");
		if (next == null) return;
		const trimmed = next.trim();
		if (!trimmed) {
			showToast("Title cannot be empty");
			return;
		}
		setPreferences(renameConversation(activeSessionId, trimmed));
	}, [activeSessionId, showToast]);

	const onSlashCompact = useCallback(async () => {
		if (!activeSessionId || !nativeCodex?.compact) {
			showToast("Context compaction requires an active Codex conversation");
			return;
		}
		if (activeChatRunning) {
			queuedCompactionRef.current.add(activeSessionId);
			showToast("Compaction queued for after the current response");
			return;
		}
		setBusy(true);
		manualCompactionPendingRef.current.add(activeSessionId);
		try {
			const session = sessionsRef.current.find((candidate) => candidate.id === activeSessionId);
			if (!session) throw new Error(`Native Codex session is not registered: ${activeSessionId}`);
			const request = await codexResumeRequest(
				nativeCodex,
				session,
				preferences.agentContext.autoCompactTokenLimits,
				laguna?.baseUrl ?? undefined
			);
			await nativeCodex.compact(request);
			showToast("Compacting context…");
		} catch (reason) {
			manualCompactionPendingRef.current.delete(activeSessionId);
			showToast(publicError(reason));
		} finally {
			setBusy(false);
		}
	}, [activeChatRunning, activeSessionId, laguna?.baseUrl, nativeCodex, preferences.agentContext.autoCompactTokenLimits, showToast]);

	const onReloadLaguna = useCallback(async () => {
		const bridge = bridges.laguna;
		if (!bridge) throw new Error("Laguna controls are unavailable in this build");
		await bridge.reload();
		const status = await bridge.getStatus();
		setLaguna(status);
		await refreshHealth();
		if (status.phase !== "ready") {
			throw new Error(status.detail ?? `Laguna reload ended in ${status.phase}`);
		}
		return status;
	}, [refreshHealth]);

	const onFreeLocalMemory = useCallback(async () => {
		const bridge = bridges.laguna;
		if (!bridge?.freeMemory) throw new Error("Local model controls are unavailable in this build");
		const outcome = await bridge.freeMemory();
		if (outcome.conflict || !outcome.released) throw new Error(outcome.detail ?? "Local model memory could not be freed");
		setLaguna(await bridge.getStatus());
		showToast(outcome.detail ?? "Local model memory freed");
	}, [showToast]);

	const openSearch = useCallback(() => {
		if (!searchOpen && document.activeElement instanceof HTMLElement) {
			searchRestoreFocusRef.current = document.activeElement;
		}
		setSearchOpen(true);
	}, [searchOpen]);

	const closeSearch = useCallback((options?: { restoreFocus?: boolean }) => {
		if (!searchOpen) return;
		setSearchOpen(false);
		if (options?.restoreFocus === false) return;
		requestAnimationFrame(() => {
			if (searchRestoreFocusRef.current?.isConnected) searchRestoreFocusRef.current.focus();
		});
	}, [searchOpen]);

	useEffect(() => {
		const onKeyDown = (event: KeyboardEvent) => {
			if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
				event.preventDefault();
				if (searchOpen) closeSearch();
				else openSearch();
				return;
			}
			if (event.key === "Escape" && searchOpen) {
				event.preventDefault();
				closeSearch();
			}
		};
		window.addEventListener("keydown", onKeyDown);
		return () => window.removeEventListener("keydown", onKeyDown);
	}, [closeSearch, openSearch, searchOpen]);

	const tabLabel =
		view.kind === "settings"
			? "Settings"
			: view.kind === "connectors"
				? "Connectors"
			: view.kind === "visuals"
				? "Visuals"
			: view.kind === "reports"
				? "Reports"
			: view.kind === "experiments"
				? "Experiments"
			: view.kind === "optimizers"
				? "Optimizers"
			: view.kind === "inventory"
				? "Data"
				: view.kind === "async"
					? "Intern · Background"
					: view.kind === "sync"
						? (activeSync?.title ?? "Intern · Live")
						: view.kind === "chat"
							? (activeChat?.title ?? "Chat")
							: (EXECUTION_TARGETS.find((t) => t.id === selectedTargetId)?.label ?? "Synth");

	const showComposer = view.kind === "landing" || view.kind === "chat";

	useEffect(() => {
		const visibleEvents = activeSessionId
			? (eventsBySessionRef.current[activeSessionId] ?? [])
			: [];
		const api = createSemanticEvalApi({
			activeSessionId,
			sessions,
			visibleEvents,
			openArtifactId,
			view,
			busy,
			showComposer,
			selectedTargetId,
			createConversation,
			sendToSession,
			openVisualRecord,
			openChat,
			setView
		});
		window.__synthPreferences = preferencesAdapter();
		// Eval driver is DEV/test-only; keep it out of packaged production builds.
		if (import.meta.env.DEV) {
			window.__synthEval = api;
			window.dispatchEvent(new CustomEvent("synth-eval-ready"));
		}
		return () => {
			if (import.meta.env.DEV && window.__synthEval === api) delete window.__synthEval;
			delete window.__synthPreferences;
		};
	}, [
		activeSessionId,
		busy,
		createConversation,
		openArtifactId,
		openChat,
		openVisualRecord,
		selectedTargetId,
		sendToSession,
		sessions,
		showComposer,
		view
	]);


	return {
		isDesktop,
		nativeCodex,
		nativeIntern,
		appVersion,
		sessions,
		eventsBySession,
		transcriptHistoryBySession,
		loadOlderTranscript,
		health,
		laguna,
		bootError,
		defaultWorkspace,
		refreshHealth,
		refreshSessions,
		onReloadLaguna,
		onFreeLocalMemory,
		preferences,
		setPreferences,
		approvalPolicy,
		sandboxMode,
		setApprovalMode,
		setApprovalPolicy,
		setSandboxMode,
		selectActivePermissions,
		modelKnobValues,
		selectModelKnob,
		sidebarVisible,
		setSidebarVisible,
		sidebarWidth,
		setSidebarWidth,
		terminalOpen,
		setTerminalOpen,
		viewportWidth,
		inventoryContainerWidth,
		setInventoryContainerWidth,
		sidePanelOpen,
		setSidePanelOpen,
		sidePanelTab,
		setSidePanelTab,
		containerPaneExpanded,
		setContainerPaneExpanded,
		persistLayoutSnapshot,
		apiKeyConfigured,
		setApiKeyConfigured,
		backendSettings,
		setBackendSettings,
		accountUsage,
		accountSummary,
		usageSheetOpen,
		setUsageSheetOpen,
		refreshAccountSummary,
		accountView,
		openBilling,
		pluginStatuses,
		refreshPluginStatuses,
		...computerUseState,
		view,
		setView,
		toast,
		showToast,
		downloadPaused,
		setDownloadPaused,
		searchOpen,
		openSearch,
		closeSearch,
		unreadChatIds,
		pinnedChatIds,
		conversationTitles,
		workingChatIds,
		chatPresence,
		recoveryNotices,
		resumeRecoveredChat,
		selectedTargetId,
		onSelectTarget,
		lagunaAdapters,
		selectedLagunaAdapterId,
		selectLagunaAdapter,
		onNewConversation,
		openChat,
		openArtifactId,
		openArtifact,
		openContainer,
		standaloneVisual: visualRevisionState.artifact,
		toggleArtifact,
		toggleContainer,
		probeOpenContainer,
		repairOpenContainer,
		restartOpenContainer,
		openVisualRecord,
		state,
		activeSessionId,
		activeChat,
		activeChatSession,
		activeChatRunning,
		activeChatWarmingUp,
		activeLocalModel,
		activeSync,
		showSidePanel,
		sidePanelCanSharePane,
		showComposer,
		tabLabel,
		terminalWorkspaceRoot,
		terminalWorkspaceId,
		inferenceMonitor,
		persistedPerformanceByTarget,
		selectedModelMedianTpsLabel,
		aggregateModelTpsLabels,
		codexUsage,
		busy,
		steerError,
		setSteerError,
		failedSend,
		retryFailedSend,
		queueAfterStop,
		setQueueAfterStop,
		composerSkills,
		workspaceScope,
		setWorkspaceScope,
		createConversation,
		sendToSession,
		onComposerSend,
		controlActive,
		onSlashRename,
		onSlashCompact,
		ensureActiveSession,
	};
}
