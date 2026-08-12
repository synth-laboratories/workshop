import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { appEventToRuntimeEvent } from "@synth/runtime-protocol";
import desktopPackage from "../../../package.json";
import type {
	CodexActivityEvent,
	ContainerDeployment,
	RuntimeControlKind,
	RuntimeEvent,
	RuntimeHealth,
	Session,
	VisualInstanceRecord,
	VisualRecord
} from "@synth/runtime-protocol";
import {
	dispatchLocalSessionStatus,
	dispatchRuntimeEvent,
	dispatchTurnAccepted,
	mergeInternSessions,
	mergeSessionReplay,
	patchSessionMetadata,
	replaceSessions,
	selectSessionRunning,
	selectWorkingChatIds,
	upsertSession,
	useEventsBySession,
	useSessions
} from "./stores/sessionStore";
import {
	EXECUTION_TARGETS,
	isInternTargetId
} from "./types/landing";
import type { ArtifactRef } from "./types/landing";
import { AppTitlebar } from "./components/AppTitlebar";
import { Composer } from "./components/Composer";
import { AppOverlays } from "./components/AppOverlays";
import { formatTps, useInferenceMonitor } from "./components/InferencePanel";
import { Sidebar } from "./components/Sidebar";
import { TerminalPanel } from "./components/TerminalPanel";
import { artifactFromVisualRecord } from "./components/VisualHost";
import { useAccountShell } from "./hooks/useAccountShell";
import { useShellLayout } from "./hooks/useShellLayout";
import { useCodexEventBridge } from "./hooks/useCodexEventBridge";
import { useForeignSessionEventBridge } from "./hooks/useForeignSessionEventBridge";
import { useModelPerformanceLabels } from "./hooks/useModelPerformanceLabels";
import {
	buildLandingState,
	executionTargetToUiId,
	sessionIsAsync,
	sessionIsLocalChat,
	sessionIsSync,
	targetIdToExecutionTarget,
	visualRecordToArtifact
} from "./runtime/sessionView";
import { approvalModeConfig, approvalModeFromConfig, codexStartRequest, coreEventToRuntime, createCodexSession, restoreCodexSession, type ApprovalMode, type ApprovalPolicy, type SandboxMode } from "./runtime/nativeCodex";
import {
	loadModelKnobValues,
	modelKnobForTarget,
	modelKnobKey,
	turnStartEffortForExecutionTarget,
	type ModelKnobTransportValue
} from "./runtime/modelCapabilities";
import {
	planComposerSend,
	planModelChipChange,
	threadHasHistoryFromEvents
} from "./runtime/modelSwitchPlan";
import type {
	CodexSessionInfo,
	ComposerImageAttachment,
	ConversationWorkspaceScope,
	LagunaStatus
} from "./bridge";
import {
	applyPreferencesToDocument,
	archiveConversation,
	enqueuePrompt,
	loadPreferences,
	normalizeLayoutSnapshot,
	pinConversation,
	preferencesAdapter,
	promptsForConversation,
	renameConversation,
	saveLayout,
	setPermissionPreferences,
	setToolActivityMode,
	setUnreadCompletedChats,
	subscribePreferences,
	updateQueuedPrompt,
	type DesktopPreferences
} from "./preferences";
import { browserRuntimeClient } from "./runtime/browserRuntimeClient";
import {
	codexResumeRequest,
	codexTurnFailure,
	desktopBootError,
	turnFailureMessage,
	type FailedSend
} from "./runtime/codexTurn";
import { loadDeviceUsage } from "./runtime/deviceUsage";
import { createSemanticEvalApi } from "./runtime/evalApi";
import { drainPromptQueues, nextQueuedPrompt, removeQueuedPrompt } from "./runtime/promptQueue";
import { MainRoutes, type MainView } from "./routes";
export default function App() {
	const isDesktop = window.location.protocol === "tauri:" || "__TAURI_INTERNALS__" in window;
	const nativeCodex = window.synthCodex;
	// synthIntern is installed in browsers too as a demo adapter. Codex presence is
	// the stable packaged-Tauri signal used here to select the Rust-owned path.
	const nativeIntern = nativeCodex ? window.synthIntern : undefined;
	const [appVersion, setAppVersion] = useState(desktopPackage.version);
	useEffect(() => {
		void window.synthDesktop.getInstanceDiagnostics()
			.then((identity) => {
				const runtimeVersion = identity.appVersion.trim();
				if (runtimeVersion) setAppVersion(runtimeVersion);
			})
			.catch(() => undefined);
	}, []);
	const [health, setHealth] = useState<RuntimeHealth | null>(null);
	const [laguna, setLaguna] = useState<LagunaStatus | null>(null);
	const sessions = useSessions();
	const sessionsRef = useRef<Session[]>(sessions);
	const eventsBySession = useEventsBySession();
	const [codexActivityBySession, setCodexActivityBySession] = useState<Record<string, CodexActivityEvent[]>>({});
	const [selectedTargetId, setSelectedTargetId] = useState("local-laguna");
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
	const [standaloneVisual, setStandaloneVisual] = useState<ArtifactRef | null>(null);
	const [openContainer, setOpenContainer] = useState<ContainerDeployment | null>(null);
	const inferenceMonitor = useInferenceMonitor({ visible: selectedTargetId === "local-laguna" });
	const {
		persistedPerformanceByTarget,
		selectedModelMedianTpsLabel,
		aggregateModelTpsLabels
	} = useModelPerformanceLabels(selectedTargetId, inferenceMonitor);
	const [busy, setBusy] = useState(false);
	const [bootError, setBootError] = useState<string | null>(null);
	const [steerError, setSteerError] = useState<string | null>(null);
	const [composerSkills, setComposerSkills] = useState<Array<{ id: string; name: string; description: string }>>([]);
	const [queueAfterStop, setQueueAfterStop] = useState(false);
	const [defaultWorkspace, setDefaultWorkspace] = useState<string | null>(null);
	const [workspaceScope, setWorkspaceScope] = useState<ConversationWorkspaceScope | null>(null);
	const eventsBySessionRef = useRef(eventsBySession);
	const nativeSequencesRef = useRef(new Map<string, number>());
	const autoOpenedSubagentsRef = useRef(new Set<string>());
	const [failedSend, setFailedSend] = useState<FailedSend | null>(null);
	const staleRunFenceRef = useRef(new Set<string>());
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

	useEffect(() => subscribePreferences((next) => {
		setPreferences(next);
		setApprovalMode(next.approvalMode);
		setApprovalPolicy(next.approvalPolicy);
		setSandboxMode(next.sandboxMode);
		setUnreadChatIds(new Set(next.unreadCompletedChats));
		applyPreferencesToDocument(next);
	}), []);

	useEffect(() => {
		applyPreferencesToDocument(preferences);
	}, [preferences]);

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
		if (isDesktop && window.synthCore && window.synthConfig && window.synthInventory) {
			const [core, config, counts, currentLaguna, usage] = await Promise.all([
				window.synthCore.diagnostics(),
				window.synthConfig.get(),
				window.synthInventory.counts(),
				window.synthLaguna?.getStatus() ?? Promise.resolve(null),
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
			window.synthConfig?.get().catch(() => null) ?? Promise.resolve(null)
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
		void window.synthCodex?.defaultWorkspace().then(setDefaultWorkspace).catch(() => undefined);
	}, []);

	useEffect(() => {
		const onKey = (event: KeyboardEvent) => {
			if (!(event.metaKey || event.ctrlKey)) return;
			if (event.key.toLowerCase() === "j" && !event.shiftKey) {
				event.preventDefault();
				setTerminalOpen((current) => {
					const next = !current;
					persistLayoutSnapshot({ bottomPanelVisible: next });
					return next;
				});
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
	}, []);

	useEffect(() => {
		let disposed = false;
		const boot = nativeCodex
			? Promise.all([nativeCodex.list(), nativeIntern?.listSessions() ?? Promise.resolve([]), refreshHealth()]).then(async ([persisted, internSessions]) => {
				const restored = persisted.filter((session) => session.status !== "closed").map(restoreCodexSession);
				const combined = [...restored, ...internSessions];
				sessionsRef.current = combined;
				replaceSessions(combined);
				const core = window.synthCore;
				if (!core) return;
				const replay = await Promise.all(combined.map(async (session) => {
					const rows = await core.sessionEventsAfter(session.id, 0, 2000);
					return [session.id, rows.map(session.target.kind === "intern" ? appEventToRuntimeEvent : coreEventToRuntime).filter((event): event is RuntimeEvent => event !== null)] as const;
				}));
				if (disposed) return;
				mergeSessionReplay(replay);
				for (const [sessionId, events] of replay) {
					const head = events.at(-1)?.sequence ?? 0;
					nativeSequencesRef.current.set(sessionId, head);
				}
			})
			: Promise.all([refreshHealth(), refreshSessions()]);
		boot
			.then(() => {
				if (!disposed) setBootError(null);
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
		showToast
	});

	useEffect(() => {
		const bridge = window.synthLaguna;
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
			void window.synthLaguna?.getStatus().then(setLaguna).catch(() => undefined);
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
	const terminalWorkspaceRoot = defaultWorkspace;
	const terminalWorkspaceId = activeSessionId ?? "default";
	const selectActivePermissions = useCallback((nextApprovalPolicy: ApprovalPolicy, nextSandboxMode: SandboxMode) => {
		const mode = approvalModeFromConfig(nextApprovalPolicy, nextSandboxMode);
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
		void nativeCodex?.close(activeSessionId).catch((reason) => showToast(reason instanceof Error ? reason.message : String(reason)));
	}, [activeSessionId, nativeCodex, showToast]);

	useEffect(() => {
		if (!activeSessionId) return;
		const session = sessions.find((candidate) => candidate.id === activeSessionId);
		if (!session || session.metadata.runtime !== "codex-app-server") return;
		const mode = typeof session.metadata.approvalMode === "string"
			? session.metadata.approvalMode as ApprovalMode
			: approvalModeFromConfig(
				typeof session.metadata.approvalPolicy === "string" ? session.metadata.approvalPolicy : undefined,
				typeof session.metadata.sandbox === "string" ? session.metadata.sandbox : undefined
			);
		setApprovalMode(mode);
		setApprovalPolicy(typeof session.metadata.approvalPolicy === "string" ? session.metadata.approvalPolicy as ApprovalPolicy : approvalModeConfig(mode).approvalPolicy as ApprovalPolicy);
		setSandboxMode(typeof session.metadata.sandbox === "string" ? session.metadata.sandbox as SandboxMode : approvalModeConfig(mode).sandbox as SandboxMode);
	}, [activeSessionId, sessions]);

	useEffect(() => {
		let disposed = false;
		setWorkspaceScope(null);
		if (!activeSessionId || !window.synthWorkspaceScope) return () => { disposed = true; };
		void window.synthWorkspaceScope.get(activeSessionId).then((scope) => { if (!disposed) setWorkspaceScope(scope); }).catch(() => undefined);
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
		downloadPaused,
		eventsBySession,
		codexActivityBySession,
		health,
		laguna,
		preferences.conversations,
		selectedTargetId,
		sessions
	]);

	const workingChatIds = useMemo(() => selectWorkingChatIds(sessions), [sessions]);

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
	// Session status + event arbitration — single selector, not an App.tsx IIFE.
	const activeChatRunning = activeChat
		? selectSessionRunning(activeChatSession, eventsBySession[activeChat.id] ?? [])
		: false;
	const activeChatWarmingUp = Boolean(
		activeChatRunning &&
		activeChatSession?.target.kind === "local" &&
		(laguna?.phase === "loading" || !laguna?.loadedModel)
	);
	const activeLocalModel = activeChatSession?.target.kind === "local";
	const workbenchWidth = viewportWidth - (sidebarVisible ? sidebarWidth : 0);
	const sidePanelFits = workbenchWidth >= 368 + 300;
	const showSidePanel = sidePanelOpen && sidePanelFits && (sidePanelTab === "outputs" || activeLocalModel);
	const activeSync =
		view.kind === "sync"
			? (state.syncSessions.find((s) => s.id === view.sessionId) ?? null)
			: null;

	const openArtifact =
		standaloneVisual && openArtifactId === standaloneVisual.id
			? standaloneVisual
			: view.kind === "chat" && activeChat
				? (activeChat.artifacts?.find((a) => a.id === openArtifactId) ?? null)
				: view.kind === "sync" && activeSync
					? (activeSync.artifacts?.find((a) => a.id === openArtifactId) ?? null)
					: null;

	const viewKey =
		view.kind === "chat"
			? `chat:${view.chatId}`
			: view.kind === "sync"
				? `sync:${view.sessionId}`
				: view.kind === "async"
					? `async:${view.sessionId}`
					: view.kind;

	useEffect(() => {
		setOpenArtifactId(null);
		setStandaloneVisual(null);
		setOpenContainer(null);
		setContainerPaneExpanded(false);
	}, [viewKey]);

	useEffect(() => {
		if (openArtifactId || openContainer) return;
		const surface = activeChat ?? activeSync;
		const subagents = surface?.artifacts?.find((artifact) => artifact.templateId === "synth.subagents.v1");
		if (!surface || !subagents || autoOpenedSubagentsRef.current.has(surface.id)) return;
		autoOpenedSubagentsRef.current.add(surface.id);
		setStandaloneVisual(null);
		setOpenArtifactId(subagents.id);
	}, [activeChat, activeSync, openArtifactId, openContainer]);

	useEffect(() => {
		if (!openArtifactId && !openContainer) return;
		const onKey = (e: KeyboardEvent) => {
			if (e.key === "Escape") {
				setOpenArtifactId(null);
				setStandaloneVisual(null);
				setOpenContainer(null);
				setContainerPaneExpanded(false);
			}
		};
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [openArtifactId, openContainer]);

	const toggleArtifact = useCallback((id: string | null) => {
		if (id == null) {
			setOpenArtifactId(null);
			setStandaloneVisual(null);
			return;
		}
		setStandaloneVisual(null);
		setOpenContainer(null);
		setOpenArtifactId((current) => (current === id ? null : id));
	}, []);

	const toggleContainer = useCallback(async (id: string | null) => {
		if (!id || openContainer?.id === id) {
			setOpenContainer(null);
			setContainerPaneExpanded(false);
			return;
		}
		if (!window.synthInventory) {
			showToast("Container inventory requires Synth Desktop");
			return;
		}
		try {
			const container = await window.synthInventory.getContainer(id);
			setOpenArtifactId(null);
			setStandaloneVisual(null);
			setOpenContainer(container);
		} catch (reason) {
			showToast(reason instanceof Error ? reason.message : String(reason));
		}
	}, [openContainer?.id, showToast]);

	const probeOpenContainer = useCallback(async () => {
		if (!openContainer || !window.synthInventory) return;
		try {
			const container = await window.synthInventory.probeContainer(openContainer.id);
			setOpenContainer(container);
			showToast(`${container.name} · ${container.status}`);
		} catch (reason) {
			showToast(reason instanceof Error ? reason.message : String(reason));
		}
	}, [openContainer, showToast]);

	const openVisualRecord = useCallback((visual: VisualInstanceRecord | VisualRecord) => {
		const artifact =
			"schemaVersion" in visual && visual.schemaVersion === "synth.desktop-visual.v1"
				? artifactFromVisualRecord(visual as VisualRecord)
				: visualRecordToArtifact(visual as VisualInstanceRecord);
		setStandaloneVisual(artifact);
		setOpenArtifactId(artifact.id);
		// Opening an artifact is a side-pane action. Navigation remains explicit so
		// traces stay beside their catalog and chat-created visuals stay beside chat.
	}, []);

	useEffect(() => {
		const unlisten = window.synthVisuals?.onShow?.(async (event) => {
			const visualId =
				typeof event.payload?.visualId === "string" ? event.payload.visualId : null;
			if (!visualId || !window.synthVisuals) return;
			try {
				const visual = await window.synthVisuals.get(visualId);
				openVisualRecord(visual);
				showToast(`Opened visual · ${visual.title}`);
			} catch (reason) {
				showToast(`Visual show failed · ${String(reason)}`);
			}
		});
		return () => unlisten?.();
	}, [openVisualRecord, showToast]);

	const ensureOpenRouterReady = useCallback(async (targetId: string): Promise<boolean> => {
		if (!targetId.startsWith("openrouter-")) return true;
		const config = await window.synthConfig?.get().catch(() => null);
		const configured = config?.openrouterApiKeyConfigured ?? health?.openrouter.mode === "ready";
		if (configured) {
			if (config) setBackendSettings(config);
			return true;
		}
		showToast("OpenRouter API key required — message was not sent");
		setView({ kind: "settings", section: "account" });
		return false;
	}, [health?.openrouter.mode, showToast]);

	const createConversation = useCallback(
		async (targetId: string = selectedTargetId, title?: string, objective?: string) => {
			setBusy(true);
			try {
				const target = targetIdToExecutionTarget(targetId);
				const internObjective = objective?.trim();
				if (target.kind === "intern" && !internObjective) {
					throw new Error("Enter an objective to start an Intern session");
				}
				if (nativeCodex && target.kind !== "intern") {
					const id = crypto.randomUUID();
					// Every local/configured-provider task starts in the configured safe workspace.
					const workspace = await nativeCodex.defaultWorkspace();
					const permissions = { approvalPolicy, sandbox: sandboxMode };
					await nativeCodex.start(codexStartRequest(id, workspace, target, permissions, preferences.agentContext.autoCompactTokenLimits, laguna?.baseUrl ?? undefined));
					const session = createCodexSession(id, target, null, workspace, title, permissions);
					sessionsRef.current = [session, ...sessionsRef.current.filter((item) => item.id !== session.id)];
					upsertSession(session);
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
				showToast(reason instanceof Error ? reason.message : String(reason));
				throw reason;
			} finally {
				setBusy(false);
			}
		},
		[approvalPolicy, sandboxMode, laguna?.baseUrl, nativeCodex, nativeIntern, preferences.agentContext.autoCompactTokenLimits, refreshSessions, selectedTargetId, showToast]
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
		const session = await createConversation(selectedTargetId, undefined, objectiveConsumed ? objective : undefined);
		return { sessionId: session.id, objectiveConsumed };
	}, [activeSessionId, createConversation, selectedTargetId, view.kind]);

	const sendToSession = useCallback(
		async (sessionId: string, text: string, options?: { messageId?: string; images?: ComposerImageAttachment[] }) => {
			try {
				const session = sessionsRef.current.find((candidate) => candidate.id === sessionId);
				const sessionTargetId = session ? executionTargetToUiId(session.target) : selectedTargetId;
				const pendingTargetId = isInternTargetId(selectedTargetId) ? sessionTargetId : selectedTargetId;
				if (!await ensureOpenRouterReady(pendingTargetId)) return false;
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
						destinationSupportsImages: false
					});
					if (sendPlan.kind === "block") {
						showToast(sendPlan.message);
						return false;
					}
					const executionTarget = sendPlan.kind === "model_switch_then_turn"
						? targetIdToExecutionTarget(sendPlan.destinationTargetId)
						: session.target;
					const workspace = typeof session.metadata.workspace === "string"
						? session.metadata.workspace
						: await nativeCodex.defaultWorkspace();
					const storedApprovalMode = typeof session.metadata.approvalMode === "string"
						? session.metadata.approvalMode as ApprovalMode
						: approvalModeFromConfig(
							typeof session.metadata.approvalPolicy === "string" ? session.metadata.approvalPolicy : undefined,
							typeof session.metadata.sandbox === "string" ? session.metadata.sandbox : undefined
						);
					const storedApproval = approvalModeConfig(storedApprovalMode);
					const startRequest = {
						...codexStartRequest(sessionId, workspace, executionTarget, "ask", preferences.agentContext.autoCompactTokenLimits, laguna?.baseUrl ?? undefined),
						// Restored pre-policy sessions can carry only the human mode. Never
						// turn that into an undefined request which Rust then treats as Ask.
						approvalPolicy: typeof session.metadata.approvalPolicy === "string" ? session.metadata.approvalPolicy : storedApproval.approvalPolicy,
						sandbox: typeof session.metadata.sandbox === "string" ? session.metadata.sandbox : storedApproval.sandbox,
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
					try {
						// One round trip owns attach/resume and turn/start, so the
						// app-server cannot exit in a gap the renderer can see.
						// Model switches compact on the source model inside sendTurn
						// before rebind (see modelSwitchPlan.ts).
						started = nativeCodex.sendTurn
							? await nativeCodex.sendTurn(
								startRequest,
								text,
								effort,
								{ compactBeforeModelSwitch: sendPlan.kind === "model_switch_then_turn" ? sendPlan.compact : false }
							)
							: await (async () => {
								await nativeCodex.start(startRequest);
								return nativeCodex.startTurn(sessionId, text, effort);
							})();
					} catch (reason) {
						failTurnStart(sessionId, text, messageId, reason);
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
				showToast(reason instanceof Error ? reason.message : String(reason));
				return false;
			} finally {
				setBusy(false);
			}
		},
		[allocateNativeSequence, ensureOpenRouterReady, failTurnStart, laguna?.baseUrl, modelKnobValues, nativeCodex, nativeIntern, preferences.agentContext.autoCompactTokenLimits, refreshSessions, selectedTargetId, showToast]
	);
	sendToSessionRef.current = sendToSession;

	const retryFailedSend = useCallback(() => {
		const pending = failedSend;
		if (!pending) return;
		setFailedSend(null);
		void sendToSession(pending.sessionId, pending.text, { messageId: pending.messageId });
	}, [failedSend, sendToSession]);

	const onComposerSend = useCallback(
		async (text: string, images: ComposerImageAttachment[] = []) => {
			try {
				if (!await ensureOpenRouterReady(selectedTargetId)) return;
				const ensured = await ensureActiveSession(text);
				if (!ensured) {
					showToast("No active session");
					return;
				}
				// Intern creation itself starts the objective. Sending the same text
				// again would issue a duplicate operator command.
				if (!ensured.objectiveConsumed) {
					await sendToSession(ensured.sessionId, text, { images });
				}
			} catch {
				/* toast already shown */
			}
		},
		[ensureActiveSession, ensureOpenRouterReady, selectedTargetId, sendToSession, showToast]
	);

	const controlActive = useCallback(
		async (kind: RuntimeControlKind, payload: Record<string, unknown> = {}) => {
			if (!activeSessionId) return;
			setBusy(true);
			try {
				const session = sessions.find((candidate) => candidate.id === activeSessionId);
				if (nativeCodex && session?.metadata.runtime === "codex-app-server") {
					if (kind === "close") await nativeCodex.close(activeSessionId);
					else if (kind === "cancel" || kind === "pause") await nativeCodex.interrupt(activeSessionId);
					else if (kind === "approve" || kind === "reject") {
						const approvalId = typeof payload.approvalId === "string" ? payload.approvalId : null;
						if (!approvalId) throw new Error("Approval id is missing");
						const decision = kind === "reject" ? "reject" : payload.decision === "always" ? "always" : "once";
						await nativeCodex.resolveApproval(activeSessionId, approvalId, decision);
					}
					else throw new Error(`${kind} is not supported for a Codex session`);
					if (kind === "close" || kind === "cancel" || kind === "pause") {
						dispatchLocalSessionStatus(
							activeSessionId,
							kind === "close" ? "completed" : "interrupted"
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
				showToast(reason instanceof Error ? reason.message : String(reason));
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
		setView({ kind: "landing" });
		setOpenArtifactId(null);
		setStandaloneVisual(null);
	}, []);

	const openChat = useCallback((id: string) => {
		setView({ kind: "chat", chatId: id });
		const session = sessionsRef.current.find((candidate) => candidate.id === id);
		if (session && sessionIsLocalChat(session)) {
			// Opening a thread adopts its bound model as pendingTarget so a
			// leftover chip from another chat cannot silently switch on send.
			setSelectedTargetId(executionTargetToUiId(session.target));
		}
	}, []);

	useEffect(() => {
		void window.synthSkills?.list()
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
			showToast(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setBusy(false);
		}
	}, [activeChatRunning, activeSessionId, laguna?.baseUrl, nativeCodex, preferences.agentContext.autoCompactTokenLimits, showToast]);

	const onReloadLaguna = useCallback(async () => {
		const bridge = window.synthLaguna;
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
		const bridge = window.synthLaguna;
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

	return (
		<div className="app-shell">
			<div className="body-row">
					{view.kind !== "settings" ? <Sidebar
						state={state}
						lagunaStatus={laguna}
					activeChatId={view.kind === "chat" ? view.chatId : null}
					inventoryActive={view.kind === "inventory"}
					visualsActive={view.kind === "visuals"}
					optimizersActive={view.kind === "optimizers"}
					workingChatIds={workingChatIds}
					activeLocalDecodeTps={inferenceMonitor.snapshot?.active?.decodeTokensPerSecond == null
						? null
						: `${formatTps(inferenceMonitor.snapshot.active.decodeTokensPerSecond)} tok/s`}
					unreadChatIds={unreadChatIds}
					pinnedChatIds={pinnedChatIds}
					conversationTitles={conversationTitles}
					sidebarWidth={sidebarWidth}
					sidebarVisible={sidebarVisible}
					onSidebarWidthChange={(width) => {
						setSidebarWidth(width);
						persistLayoutSnapshot({ sidebarWidth: width });
					}}
					onNewConversation={onNewConversation}
					onOpenChat={(id) => {
						openChat(id);
						persistLayoutSnapshot({ selectedConversationId: id });
					}}
					onRenameChat={(id, title) => {
						try {
							setPreferences(renameConversation(id, title));
						} catch (reason) {
							showToast(reason instanceof Error ? reason.message : String(reason));
						}
					}}
					onPinChat={(id, pinned) => setPreferences(pinConversation(id, pinned))}
					onArchiveChat={(id, archived) => {
						if (archived && workingChatIds.has(id)) {
							showToast("Stop the run before archiving");
							return;
						}
						setPreferences(archiveConversation(id, archived));
						if (archived && view.kind === "chat" && view.chatId === id) {
							setView({ kind: "landing" });
						}
					}}
					onOpenInventory={() => setView({ kind: "inventory" })}
					onOpenVisuals={() => setView({ kind: "visuals" })}
					onOpenOptimizers={() => setView({ kind: "optimizers" })}
					onSearch={openSearch}
					onSettings={() => setView({ kind: "settings" })}
					account={accountView}
					onOpenUsage={() => setUsageSheetOpen(true)}
					onBilling={(action) => void openBilling(action)}
					onRetryAccount={() => refreshAccountSummary(true)}
					onOpenAccount={() => setView({ kind: "settings", section: "account" })}
					onSignOut={async () => {
						if (!window.synthAccount) {
							setView({ kind: "settings", section: "account" });
							return;
						}
						try {
							const next = await window.synthAccount.signOut();
							setApiKeyConfigured(next.apiKeyConfigured);
							refreshAccountSummary();
							showToast("Signed out of Synth");
						} catch (reason) {
							showToast(reason instanceof Error ? reason.message : String(reason));
						}
					}}
					onPauseToggle={() => setDownloadPaused((v) => !v)}
					onFreeLocalMemory={onFreeLocalMemory}
				/> : null}

				<main className="main-pane">
					<AppTitlebar
						tabLabel={tabLabel}
						appVersion={appVersion}
						activeLocalModel={Boolean(activeLocalModel)}
						terminalOpen={terminalOpen}
						sidePanelOpen={sidePanelOpen}
						sidePanelTab={sidePanelTab}
						onCloseTab={() => {
							setView({ kind: "landing" });
							showToast("Back to landing");
						}}
						onNewConversation={onNewConversation}
						onToggleTerminal={() => {
							setTerminalOpen((current) => {
								const next = !current;
								persistLayoutSnapshot({ bottomPanelVisible: next });
								return next;
							});
						}}
						onToggleInference={() => {
							const next = !(sidePanelOpen && sidePanelTab === "inference");
							setSidePanelTab("inference");
							setSidePanelOpen(next);
							window.localStorage.setItem("synth.inferenceRailOpen", next ? "1" : "0");
						}}
					/>

					{bootError ? (
						<div className="boot-error" role="alert">
							Runtime unavailable: {bootError}
						</div>
					) : null}

					<MainRoutes
						view={view}
						setView={setView}
						state={state}
						sessions={sessions}
						selectedTargetId={selectedTargetId}
						onSelectTarget={onSelectTarget}
						activeChat={activeChat}
						activeChatSession={activeChatSession}
						activeChatRunning={activeChatRunning}
						activeChatWarmingUp={activeChatWarmingUp}
						activeLocalModel={Boolean(activeLocalModel)}
						activeSessionId={activeSessionId}
						openArtifact={openArtifact}
						openArtifactId={openArtifactId}
						openContainer={openContainer}
						containerPaneExpanded={containerPaneExpanded}
						setContainerPaneExpanded={setContainerPaneExpanded}
						inventoryContainerWidth={inventoryContainerWidth}
						setInventoryContainerWidth={setInventoryContainerWidth}
						persistLayoutSnapshot={persistLayoutSnapshot}
						showSidePanel={showSidePanel}
						sidePanelTab={sidePanelTab}
						setSidePanelTab={setSidePanelTab}
						setSidePanelOpen={setSidePanelOpen}
						inferenceMonitor={inferenceMonitor}
						persistedPerformanceByTarget={persistedPerformanceByTarget}
						preferences={preferences}
						setPreferences={setPreferences}
						accountView={accountView}
						accountSummary={accountSummary}
						accountUsage={accountUsage}
						backendSettings={backendSettings}
						laguna={laguna}
						onReloadLaguna={onReloadLaguna}
						openBilling={openBilling}
						refreshAccountSummary={refreshAccountSummary}
						setUsageSheetOpen={setUsageSheetOpen}
						setSidebarVisible={setSidebarVisible}
						setSidebarWidth={setSidebarWidth}
						setTerminalOpen={setTerminalOpen}
						setApprovalMode={setApprovalMode}
						setApprovalPolicy={setApprovalPolicy}
						setSandboxMode={setSandboxMode}
						showToast={showToast}
						openChat={openChat}
						openVisualRecord={openVisualRecord}
						toggleArtifact={toggleArtifact}
						toggleContainer={toggleContainer}
						probeOpenContainer={probeOpenContainer}
						controlActive={controlActive}
						setQueueAfterStop={setQueueAfterStop}
						promptsForConversationLength={(chatId) => promptsForConversation(chatId).length}
						onActivityModeChange={(mode) => setPreferences(setToolActivityMode(mode))}
					/>

					{showComposer ? (
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
								onResolveBilling: () => setUsageSheetOpen(true),
								onOpenVoiceSettings: () => setView({ kind: "settings", section: "voice" })
							}}
						/>
					) : null}
			<TerminalPanel
						open={terminalOpen}
						workspaceId={terminalWorkspaceId}
						workspaceRoot={terminalWorkspaceRoot}
						height={preferences.layout.last.bottomPanelHeight}
						fontFamily={preferences.appearance.terminalFontFamily}
						fontSize={preferences.appearance.terminalFontSize}
						onOpenChange={(open) => {
							setTerminalOpen(open);
							persistLayoutSnapshot({ bottomPanelVisible: open });
						}}
						onHeightChange={(height) => persistLayoutSnapshot({ bottomPanelHeight: height })}
			/>
		</main>
	</div>

	<AppOverlays
		searchOpen={searchOpen}
		state={state}
		onCloseSearch={closeSearch}
		onOpenChat={(id) => openChat(id)}
		usageSheetOpen={usageSheetOpen}
		accountView={accountView}
		accountSummary={accountSummary}
		onCloseUsage={() => setUsageSheetOpen(false)}
		onSignIn={() => {
			setUsageSheetOpen(false);
			setView({ kind: "settings", section: "account" });
		}}
		onBilling={(action) => void openBilling(action)}
		onRetryAccount={() => refreshAccountSummary(true)}
		onOpenDeviceUsage={() => {
			setUsageSheetOpen(false);
			setView({ kind: "inventory" });
		}}
		toast={toast}
	/>
		</div>
	);
}
