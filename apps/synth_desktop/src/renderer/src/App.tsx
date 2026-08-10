import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { appEventToRuntimeEvent } from "@synth/runtime-protocol";
import type {
	CodexActivityEvent,
	ContainerDeployment,
	EventPage,
	ExecutionTarget,
	RuntimeControlKind,
	RuntimeEvent,
	RuntimeHealth,
	SemanticUiSnapshot,
	Session,
	UsageLedgerEntry,
	VisualInstanceRecord,
	VisualRecord
} from "@synth/runtime-protocol";
import {
	EXECUTION_TARGETS,
	OPENROUTER_LAGUNA_S_MODEL,
	OPENROUTER_LUNA_MODEL,
	SYNTH_CLOUD_LAGUNA_S_MODEL,
	isInternTargetId
} from "./types/landing";
import type { ArtifactRef } from "./types/landing";
import { ChatTranscript } from "./components/ChatTranscript";
import { ContainerPane } from "./components/ContainerPane";
// CloudDesk stays dormant for v0.2; see the removal contract at its route site.
import { Composer } from "./components/Composer";
import { ConnectorsPage } from "./components/ConnectorsPage";
import { ConversationSearch } from "./components/ConversationSearch";
import { formatTps, InferencePanel, useInferenceMonitor } from "./components/InferencePanel";
import { InventoryPage } from "./components/InventoryPage";
import { LandingPage } from "./components/LandingPage";
import { OptimizersPage } from "./components/OptimizersPage";
import { PaneResizeHandle } from "./components/PaneResizeHandle";
import { SettingsPage } from "./components/SettingsPage";
import { Sidebar } from "./components/Sidebar";
import { SynthLogo } from "./components/SynthLogo";
import { TerminalPanel } from "./components/TerminalPanel";
import { artifactFromVisualRecord, VisualPane } from "./components/VisualHost";
import { VisualsPage } from "./components/VisualsPage";
import {
	buildLandingState,
	executionTargetToUiId,
	sessionIsAsync,
	sessionIsLocalChat,
	sessionIsSync,
	targetIdToExecutionTarget,
	visualRecordToArtifact
} from "./runtime/sessionView";
import { approvalModeConfig, approvalModeFromConfig, codexEventToRuntime, codexStartRequest, coreEventToRuntime, createCodexSession, restoreCodexSession, type ApprovalMode, type ApprovalPolicy, type SandboxMode } from "./runtime/nativeCodex";
import {
	loadModelKnobValues,
	modelKnobForTarget,
	modelKnobKey,
	turnStartEffortForExecutionTarget,
	type ModelKnobValue
} from "./runtime/modelCapabilities";
import {
	planComposerSend,
	planModelChipChange,
	threadHasHistoryFromEvents
} from "./runtime/modelSwitchPlan";
import type { CodexBridge, CodexSessionInfo, CodexSessionStart, CodexTurnFailure, ComposerImageAttachment, ConversationWorkspaceScope, LagunaStatus, ModelPerformanceSummary, SynthAccountSummary } from "./env";
import {
	applyPreferencesToDocument,
	archiveConversation,
	enqueuePrompt,
	getPreferences,
	loadPreferences,
	nextQueuedPrompt,
	normalizeLayoutSnapshot,
	pinConversation,
	preferencesAdapter,
	promptsForConversation,
	removeQueuedPrompt,
	renameConversation,
	saveLayout,
	setPermissionPreferences,
	setToolActivityMode,
	setUnreadCompletedChats,
	subscribePreferences,
	updateQueuedPrompt,
	type DesktopPreferences
} from "./preferences";

/**
 * User-facing copy for a lost app-server. The typed code and the session id
 * stay in debug logs; a raw UUID in a toast tells an operator nothing.
 */
const AGENT_DISCONNECTED_MESSAGE =
	"The local agent process disconnected before the turn started. Retry to reconnect.";

/** Legacy untyped rejections from the pre-`codex_turn_send` bridge path. */
const DETACHED_ERROR_TEXT = /codex session not started|is not attached|app-server (stopped|stdout closed)/i;

/** Normalizes both the typed Tauri rejection and any thrown Error. */
function codexTurnFailure(sessionId: string, reason: unknown): CodexTurnFailure {
	if (reason && typeof reason === "object" && !(reason instanceof Error) && "code" in reason) {
		const value = reason as Partial<CodexTurnFailure>;
		return {
			code: typeof value.code === "string" ? value.code : "codex_turn_start_failed",
			message: typeof value.message === "string" ? value.message : "The turn could not be started.",
			sessionId: typeof value.sessionId === "string" ? value.sessionId : sessionId,
			detail: typeof value.detail === "string" ? value.detail : String(reason)
		};
	}
	const message = reason instanceof Error ? reason.message : String(reason);
	return {
		code: DETACHED_ERROR_TEXT.test(message) ? "codex_session_detached" : "codex_turn_start_failed",
		message,
		sessionId,
		detail: message
	};
}

function turnFailureMessage(failure: CodexTurnFailure): string {
	if (failure.code === "codex_session_detached" || DETACHED_ERROR_TEXT.test(failure.message)) {
		return AGENT_DISCONNECTED_MESSAGE;
	}
	return failure.message;
}

/** A user message that reached no app-server, kept so it can be retried. */
type FailedSend = { sessionId: string; text: string; messageId: string; message: string };

function performanceTargetId(summary: ModelPerformanceSummary): string | null {
	if (summary.provider === "local-laguna") return "local-laguna";
	if (summary.provider === "synth-cloud" && summary.modelId === SYNTH_CLOUD_LAGUNA_S_MODEL) return "synth-cloud-laguna-s";
	if (summary.provider !== "openrouter") return null;
	if (summary.modelId === OPENROUTER_LUNA_MODEL) return "openrouter-luna";
	if (summary.modelId === OPENROUTER_LAGUNA_S_MODEL) return "openrouter-laguna-s";
	return null;
}

function performanceKindLabel(kind: ModelPerformanceSummary["measurementKind"]): string {
	if (kind === "decode") return "decode";
	if (kind === "provider_reported") return "provider";
	if (kind === "end_to_end") return "end-to-end";
	return "observed";
}

function performancePreference(summary: ModelPerformanceSummary, targetId: string): number {
	if (targetId === "local-laguna" && summary.measurementKind === "decode") return 4;
	if (summary.measurementKind === "observed_stream") return 3;
	if (summary.measurementKind === "provider_reported") return 2;
	return 1;
}

function summarizeAccountUsage(entries: UsageLedgerEntry[]) {
	const weekAgo = Date.now() - 7 * 24 * 60 * 60 * 1000;
	return entries.reduce((summary, entry) => {
		summary.totalTokens += Math.max(0, entry.totalTokens);
		summary.totalCostUsd += Math.max(0, entry.costUsd ?? 0);
		if (Date.parse(entry.createdAt) >= weekAgo) {
			summary.weeklyTokens += Math.max(0, entry.totalTokens);
			summary.weeklyCostUsd += Math.max(0, entry.costUsd ?? 0);
		}
		return summary;
	}, { weeklyTokens: 0, weeklyCostUsd: 0, totalTokens: 0, totalCostUsd: 0, entries: entries.length });
}

function isCodexCompactionEvent(event: { method: string; params: Record<string, unknown> }): boolean {
	if (event.method === "thread/compacted") return true;
	const item = event.params.item;
	return Boolean(item && typeof item === "object" && (item as Record<string, unknown>).type === "contextCompaction");
}

/** Rebuild a start request for an existing session (compaction path — no model switch). */
async function codexResumeRequest(
	nativeCodex: CodexBridge,
	session: Session,
	autoCompactTokenLimits: Record<string, number>
): Promise<CodexSessionStart> {
	if (session.metadata.runtime !== "codex-app-server") {
		throw new Error(`Session ${session.id} is not owned by Codex app-server`);
	}
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
	return {
		...codexStartRequest(session.id, workspace, session.target, "ask", autoCompactTokenLimits),
		approvalPolicy: typeof session.metadata.approvalPolicy === "string" ? session.metadata.approvalPolicy : storedApproval.approvalPolicy,
		sandbox: typeof session.metadata.sandbox === "string" ? session.metadata.sandbox : storedApproval.sandbox,
		threadId: typeof session.metadata.threadId === "string" ? session.metadata.threadId : undefined
	};
}

type MainView =
	| { kind: "landing" }
	| { kind: "chat"; chatId: string }
	| { kind: "sync"; sessionId: string }
	| { kind: "async"; sessionId: string }
	| { kind: "settings"; section?: "general" | "models" | "inference" | "voice" | "runtime" | "account" | "about" }
	| { kind: "connectors" }
	| { kind: "inventory" }
	| { kind: "visuals" }
	| { kind: "optimizers" };

type SemanticEvalApi = {
	schemaVersion: "synth.desktop-eval-api.v1";
	getState(): SemanticUiSnapshot;
	listActions(): string[];
	invoke(action: string, argumentsValue?: Record<string, unknown>): Promise<unknown>;
};

function truncate(label: string, max = 22) {
	if (label.length <= max) return label;
	return `${label.slice(0, max - 1)}…`;
}

function localRuntimePresentation(health: RuntimeHealth | null, laguna: LagunaStatus | null) {
	if (laguna?.phase === "ready" || health?.local.mode === "mlx") {
		return { label: "Local ready", visibleLabel: "Local", tone: "is-ready" } as const;
	}
	if (laguna?.phase === "loading" || laguna?.phase === "starting") {
		return { label: "Local starting", visibleLabel: "Local", tone: "is-starting" } as const;
	}
	if (!health && !laguna) return { label: "Connecting to local runtime", visibleLabel: "Local", tone: "is-connecting" } as const;
	return { label: "Local offline", visibleLabel: "Local", tone: "is-offline" } as const;
}

function appendEvent(events: RuntimeEvent[], event: RuntimeEvent): RuntimeEvent[] {
	const payloadId = (value: RuntimeEvent) => {
		const payload = value.payload ?? {};
		return typeof payload.messageId === "string" ? payload.messageId
			: typeof payload.eventId === "string" ? payload.eventId
				: typeof payload.id === "string" ? payload.id : "";
	};
	if (events.some((candidate) =>
		candidate.sequence === event.sequence &&
		candidate.eventKind === event.eventKind &&
		candidate.source === event.source &&
		payloadId(candidate) === payloadId(event)
	)) return events;
	return [...events, event].sort((left, right) => left.sequence - right.sequence);
}

function mergeReplayedEvents(current: RuntimeEvent[], replayed: RuntimeEvent[]): RuntimeEvent[] {
	return [...replayed, ...current].reduce<RuntimeEvent[]>((events, event) => appendEvent(events, event), []);
}

function appendCodexActivity(
	events: CodexActivityEvent[],
	event: CodexActivityEvent
): CodexActivityEvent[] {
	if (events.some((candidate) =>
		candidate.executionId === event.executionId && candidate.streamId === event.streamId
	)) return events;
	return [...events, event];
}

function desktopBootError(reason: unknown): string {
	const message = reason instanceof Error ? reason.message : String(reason);
	if (/command\s+.+not found|unknown command/i.test(message)) {
		return "Desktop backend was updated; fully quit and reopen Synth Desktop.";
	}
	return message;
}

// Vite browser fixtures retain the old HTTP contract. The packaged Tauri app
// never installs this bridge and therefore cannot call the Python runtime.
const browserRuntimeClient = {
	bridge() {
		if (!window.synthRuntime) throw new Error("Browser runtime fixture is unavailable");
		return window.synthRuntime;
	},
	async listSessions() {
		return (await this.bridge().request<{ sessions: Session[] }>("/v1/sessions")).sessions;
	},
	health() { return this.bridge().request<RuntimeHealth>("/v1/health"); },
	createSession(target: ExecutionTarget, title?: string, objective?: string) {
		return this.bridge().request<Session>("/v1/sessions", { method: "POST", body: { target, title, projectId: null, objective } });
	},
	sendMessage(sessionId: string, body: string) {
		return this.bridge().request<{ runId: string }>(`/v1/sessions/${encodeURIComponent(sessionId)}/messages`, { method: "POST", body: { body } });
	},
	control(sessionId: string, kind: RuntimeControlKind, payload: Record<string, unknown>) {
		return this.bridge().request<{ accepted: boolean }>(`/v1/sessions/${encodeURIComponent(sessionId)}/commands`, { method: "POST", body: { kind, payload } });
	},
	events(sessionId: string, afterSequence: number, limit: number) {
		return this.bridge().request<EventPage>(`/v1/sessions/${encodeURIComponent(sessionId)}/events?after_sequence=${afterSequence}&limit=${limit}`);
	},
	subscribe: (...args: Parameters<NonNullable<typeof window.synthRuntime>["subscribe"]>) =>
		browserRuntimeClient.bridge().subscribe(...args),
	simulateLive(kind: string) {
		return this.bridge().request<{ visual: VisualInstanceRecord; eventCount: number }>("/v1/visuals/simulate-live", { method: "POST", body: { kind } });
	}
};

function IconCloud() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path
				d="M5.2 12.2h6.1a2.7 2.7 0 00.2-5.4 3.5 3.5 0 00-6.7-1.1A2.5 2.5 0 005.2 12.2z"
				stroke="currentColor"
				strokeWidth="1.25"
				strokeLinejoin="round"
			/>
		</svg>
	);
}

function IconLayout() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<rect x="2.5" y="2.5" width="11" height="11" rx="2" stroke="currentColor" strokeWidth="1.3" />
			<path d="M6.2 2.5v11" stroke="currentColor" strokeWidth="1.3" />
		</svg>
	);
}

function IconPulse() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path
				d="M1.5 8h2.6l1.6-4.3 2.4 8.6 1.7-4.3h2.7"
				stroke="currentColor"
				strokeWidth="1.3"
				strokeLinecap="round"
				strokeLinejoin="round"
			/>
		</svg>
	);
}

export default function App() {
	const isDesktop = window.location.protocol === "tauri:" || "__TAURI_INTERNALS__" in window;
	const nativeCodex = window.synthCodex;
	// synthIntern is installed in browsers too as a demo adapter. Codex presence is
	// the stable packaged-Tauri signal used here to select the Rust-owned path.
	const nativeIntern = nativeCodex ? window.synthIntern : undefined;
	const [health, setHealth] = useState<RuntimeHealth | null>(null);
	const [laguna, setLaguna] = useState<LagunaStatus | null>(null);
	const [museReady, setMuseReady] = useState(false);
	const [sessions, setSessions] = useState<Session[]>([]);
	const sessionsRef = useRef<Session[]>([]);
	const [eventsBySession, setEventsBySession] = useState<Record<string, RuntimeEvent[]>>({});
	const [codexActivityBySession, setCodexActivityBySession] = useState<Record<string, CodexActivityEvent[]>>({});
	const [selectedTargetId, setSelectedTargetId] = useState("local-laguna");
	useEffect(() => {
		// v0.1 pickers hide Intern; never leave a hidden target selected.
		if (isInternTargetId(selectedTargetId)) setSelectedTargetId("local-laguna");
	}, [selectedTargetId]);
	const [apiKeyConfigured, setApiKeyConfigured] = useState(false);
	const [accountUsage, setAccountUsage] = useState<ReturnType<typeof summarizeAccountUsage> | null>(null);
	const [accountSummary, setAccountSummary] = useState<SynthAccountSummary | null>(null);
	const refreshAccountSummary = useCallback(() => {
		const bridge = window.synthAccount;
		if (typeof bridge?.getSummary !== "function") {
			setAccountSummary(null);
			return;
		}
		void bridge.getSummary()
			.then(setAccountSummary)
			.catch(() => setAccountSummary(null));
	}, []);
	useEffect(() => {
		refreshAccountSummary();
		if (typeof window.synthInventory?.listUsage !== "function") return;
		void window.synthInventory.listUsage(2000)
			.then((entries) => setAccountUsage(summarizeAccountUsage(entries)))
			.catch(() => setAccountUsage(null));
	}, [refreshAccountSummary]);
	const [preferences, setPreferences] = useState<DesktopPreferences>(() => loadPreferences());
	const [, setApprovalMode] = useState<ApprovalMode>(() => loadPreferences().approvalMode);
	const [approvalPolicy, setApprovalPolicy] = useState<ApprovalPolicy>(() => loadPreferences().approvalPolicy);
	const [sandboxMode, setSandboxMode] = useState<SandboxMode>(() => loadPreferences().sandboxMode);
	const [modelKnobValues, setModelKnobValues] = useState(() => loadModelKnobValues(window.localStorage));
	const selectModelKnob = useCallback((targetId: string, knobId: string, value: ModelKnobValue) => {
		const knob = modelKnobForTarget(targetId, knobId);
		if (!knob || !knob.options.some((option) => option.id === value)) return;
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
	const [containerPaneExpanded, setContainerPaneExpanded] = useState(false);
	// Local inference is a first-class part of the workbench. Default the MLX
	// sidecar rail open once for existing installs, while preserving explicit
	// show/hide choices after that migration.
	const [inferenceRailOpen, setInferenceRailOpen] = useState(
		() => {
			if (window.localStorage.getItem("synth.inferenceRailDefaultV2") !== "1") {
				window.localStorage.setItem("synth.inferenceRailDefaultV2", "1");
				window.localStorage.setItem("synth.inferenceRailOpen", "1");
				return true;
			}
			return window.localStorage.getItem("synth.inferenceRailOpen") !== "0";
		}
	);
	const inferenceMonitor = useInferenceMonitor({ visible: selectedTargetId === "local-laguna" });
	const [modelPerformance, setModelPerformance] = useState<ModelPerformanceSummary[]>([]);
	useEffect(() => {
		let disposed = false;
		const refresh = async () => {
			try {
				const summaries = await window.synthModelPerformance?.summaries();
				if (!disposed && summaries) setModelPerformance(summaries);
			} catch {
				// Optional telemetry must never block chat.
			}
		};
		void refresh();
		const timer = window.setInterval(() => void refresh(), 10_000);
		return () => {
			disposed = true;
			window.clearInterval(timer);
		};
	}, []);
	const persistedPerformanceByTarget = useMemo(() => {
		const chosen = new Map<string, ModelPerformanceSummary>();
		for (const summary of modelPerformance) {
			if (summary.tpsP50 == null || summary.sampleCount < 1) continue;
			const targetId = performanceTargetId(summary);
			if (!targetId) continue;
			const current = chosen.get(targetId);
			if (!current || performancePreference(summary, targetId) > performancePreference(current, targetId)) chosen.set(targetId, summary);
		}
		return chosen;
	}, [modelPerformance]);
	const selectedModelMedianTps = selectedTargetId === "local-laguna"
		? inferenceMonitor.snapshot?.rolling.decodeTpsP50 ?? null
		: null;
	const selectedPersistedPerformance = persistedPerformanceByTarget.get(selectedTargetId);
	const selectedModelMedianTpsLabel = selectedModelMedianTps == null
		? selectedPersistedPerformance?.tpsP50 == null
			? null
			: `${formatTps(selectedPersistedPerformance.tpsP50)} tok/s ${performanceKindLabel(selectedPersistedPerformance.measurementKind)} p50`
		: `${formatTps(selectedModelMedianTps)} tok/s p50`;
	const aggregateModelTpsLabels = useMemo(() => {
		const labels: Record<string, string> = {};
		for (const [targetId, summary] of persistedPerformanceByTarget) {
			if (summary.tpsP50 == null) continue;
			labels[targetId] = `${formatTps(summary.tpsP50)} tok/s ${performanceKindLabel(summary.measurementKind)} p50 · ${summary.sampleCount} ${summary.sampleCount === 1 ? "request" : "requests"} · all sessions`;
		}
		if (!labels["local-laguna"] && selectedModelMedianTpsLabel) labels["local-laguna"] = `${selectedModelMedianTpsLabel} · daemon lifetime`;
		return labels;
	}, [persistedPerformanceByTarget, selectedModelMedianTpsLabel]);
	const [inventoryContainerWidth, setInventoryContainerWidth] = useState(() => loadPreferences().layout.last.outputPaneWidth);
	const [busy, setBusy] = useState(false);
	const [bootError, setBootError] = useState<string | null>(null);
	const [terminalOpen, setTerminalOpen] = useState(() => loadPreferences().layout.last.bottomPanelVisible);
	const [sidebarVisible, setSidebarVisible] = useState(() => loadPreferences().layout.last.sidebarVisible);
	const [viewportWidth, setViewportWidth] = useState(() => window.innerWidth);
	useEffect(() => {
		const onResize = () => setViewportWidth(window.innerWidth);
		window.addEventListener("resize", onResize);
		return () => window.removeEventListener("resize", onResize);
	}, []);
	const [sidebarWidth, setSidebarWidth] = useState(() => loadPreferences().layout.last.sidebarWidth);
	const [steerError, setSteerError] = useState<string | null>(null);
	const [composerSkills, setComposerSkills] = useState<Array<{ id: string; name: string; description: string }>>([]);
	const [queueAfterStop, setQueueAfterStop] = useState(false);
	const [defaultWorkspace, setDefaultWorkspace] = useState<string | null>(null);
	const [workspaceScope, setWorkspaceScope] = useState<ConversationWorkspaceScope | null>(null);
	const eventsBySessionRef = useRef(eventsBySession);
	const nativeSequencesRef = useRef(new Map<string, number>());
	const autoOpenedSubagentsRef = useRef(new Set<string>());
	const [failedSend, setFailedSend] = useState<FailedSend | null>(null);
	// Sessions whose last turn start was rejected. A late `run.started` from the
	// process that just died must not resurrect Working for them.
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

	const persistLayoutSnapshot = useCallback((patch: Partial<DesktopPreferences["layout"]["last"]>) => {
		const current = getPreferences().layout.last;
		const next = normalizeLayoutSnapshot({ ...current, ...patch });
		const unchanged =
			next.sidebarVisible === current.sidebarVisible &&
			next.sidebarWidth === current.sidebarWidth &&
			next.outputPaneVisible === current.outputPaneVisible &&
			next.outputPaneWidth === current.outputPaneWidth &&
			next.bottomPanelVisible === current.bottomPanelVisible &&
			next.bottomPanelHeight === current.bottomPanelHeight &&
			next.selectedConversationId === current.selectedConversationId &&
			next.selectedOutputTab === current.selectedOutputTab;
		if ("sidebarVisible" in patch) setSidebarVisible(next.sidebarVisible);
		if ("sidebarWidth" in patch) setSidebarWidth(next.sidebarWidth);
		if ("outputPaneWidth" in patch) setInventoryContainerWidth(next.outputPaneWidth);
		if ("bottomPanelVisible" in patch) setTerminalOpen(next.bottomPanelVisible);
		if (!unchanged) setPreferences(saveLayout(next));
	}, []);

	/**
	 * A turn that never started must leave no trace of Working: no `running`
	 * status, no Stop, an enabled composer, and the typed text kept for Retry.
	 * The Rust command has already reconciled the durable record and the run.
	 */
	const failTurnStart = useCallback((sessionId: string, text: string, messageId: string, reason: unknown) => {
		const failure = codexTurnFailure(sessionId, reason);
		console.debug("[codex] turn start rejected", {
			code: failure.code,
			sessionId: failure.sessionId,
			detail: failure.detail
		});
		staleRunFenceRef.current.add(sessionId);
		setSessions((current) => current.map((item) => item.id === sessionId && item.status === "running"
			? { ...item, status: "interrupted", updatedAt: new Date().toISOString() }
			: item));
		const sequence = allocateNativeSequence(sessionId);
		setEventsBySession((current) => ({ ...current, [sessionId]: appendEvent(current[sessionId] ?? [], {
			schemaVersion: "synth.desktop-runtime-event.v1", sessionId, sequence,
			eventKind: "session/unhealthy",
			payload: { reason: failure.code, message: turnFailureMessage(failure) },
			createdAt: new Date().toISOString(), source: "local"
		}) }));
		setFailedSend({ sessionId, text, messageId, message: turnFailureMessage(failure) });
		showToast(turnFailureMessage(failure));
	}, [allocateNativeSequence, showToast]);

	const refreshSessions = useCallback(async () => {
		if (nativeIntern) {
			const next = await nativeIntern.listSessions();
			setSessions((current) => [
				...current.filter((session) => session.target.kind !== "intern"),
				...next
			]);
			return next;
		}
		const next = await browserRuntimeClient.listSessions();
		setSessions(next);
		return next;
	}, [nativeIntern]);

	const refreshHealth = useCallback(async () => {
		if (isDesktop && window.synthCore && window.synthConfig && window.synthInventory) {
			const [core, config, counts, currentLaguna, usage] = await Promise.all([
				window.synthCore.diagnostics(),
				window.synthConfig.get(),
				window.synthInventory.counts(),
				window.synthLaguna?.getStatus() ?? Promise.resolve(null),
				typeof window.synthInventory.listUsage === "function"
					? window.synthInventory.listUsage(2000).catch((): UsageLedgerEntry[] => [])
					: Promise.resolve<UsageLedgerEntry[]>([])
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
			setAccountUsage(summarizeAccountUsage(usage));
			setHealth(next);
			return next;
		}
		const [next, config] = await Promise.all([
			browserRuntimeClient.health(),
			window.synthConfig?.get().catch(() => null) ?? Promise.resolve(null)
		]);
		if (config) setApiKeyConfigured(config.apiKeyConfigured);
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
				setSessions(combined);
				const core = window.synthCore;
				if (!core) return;
				const replay = await Promise.all(combined.map(async (session) => {
					const rows = await core.sessionEventsAfter(session.id, 0, 2000);
					return [session.id, rows.map(session.target.kind === "intern" ? appEventToRuntimeEvent : coreEventToRuntime).filter((event): event is RuntimeEvent => event !== null)] as const;
				}));
				if (disposed) return;
				setEventsBySession((current) => Object.fromEntries(replay.map(([sessionId, events]) => [
					sessionId,
					mergeReplayedEvents(current[sessionId] ?? [], events)
				])));
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

	useEffect(() => {
		if (!nativeCodex) return;
		return nativeCodex.onEvent((event) => {
			const manualCompaction = isCodexCompactionEvent(event)
				&& manualCompactionPendingRef.current.delete(event.sessionId);
			const normalizedEvent = manualCompaction
				? { ...event, params: { ...event.params, source: "manual" } }
				: event;
			const sequence = allocateNativeSequence(event.sessionId);
			const runtimeEvent = codexEventToRuntime(normalizedEvent, sequence);
			const updatedThreadName = event.method === "thread/name/updated"
				&& typeof event.params.threadName === "string"
				? event.params.threadName.trim()
				: null;
			// A turn start that was already rejected fences its own replay: the
			// process that emitted this run.started is the one that just died.
			const fenced = runtimeEvent.eventKind === "run.started"
				&& staleRunFenceRef.current.has(event.sessionId);
			if (runtimeEvent.eventKind === "run.failed" || runtimeEvent.eventKind === "run.cancelled") {
				manualCompactionPendingRef.current.delete(event.sessionId);
			}
			if (
				(runtimeEvent.eventKind === "run.completed" || runtimeEvent.eventKind === "run.failed" || runtimeEvent.eventKind === "run.cancelled")
				&& queuedCompactionRef.current.delete(event.sessionId)
				&& nativeCodex.compact
			) {
				manualCompactionPendingRef.current.add(event.sessionId);
				const session = sessionsRef.current.find((candidate) => candidate.id === event.sessionId);
				if (!session) return;
				void codexResumeRequest(nativeCodex, session, preferences.agentContext.autoCompactTokenLimits)
					.then((request) => nativeCodex.compact!(request))
					.then(() => showToast("Compacting context…"))
					.catch((reason) => {
						manualCompactionPendingRef.current.delete(event.sessionId);
						showToast(reason instanceof Error ? reason.message : String(reason));
					});
			}
			setEventsBySession((current) => ({
				...current,
				[event.sessionId]: appendEvent(current[event.sessionId] ?? [], runtimeEvent)
			}));
			setSessions((current) => current.map((session) => session.id === event.sessionId
					? { ...session, ...(updatedThreadName ? { title: updatedThreadName } : {}),
						updatedAt: runtimeEvent.createdAt, latestCursor: sequence,
						status: fenced ? session.status
							: runtimeEvent.eventKind === "run.started" ? "running"
							: runtimeEvent.eventKind === "run.completed" ? "ready"
						: runtimeEvent.eventKind === "run.failed" ? "failed"
						: runtimeEvent.eventKind === "run.cancelled" ? "cancelled"
						: runtimeEvent.eventKind === "session/unhealthy" ? "interrupted"
						: session.status }
				: session));
		});
	}, [allocateNativeSequence, nativeCodex, preferences.agentContext.autoCompactTokenLimits, showToast]);

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
		let disposed = false;
		const refresh = () => void window.synthLaguna?.listModels().then((models) => {
			if (!disposed) setMuseReady(models.some((model) =>
				model.modelId === "meta-models/Muse-Glimmer-30B-GGUF" && model.runtimeReady
			));
		}).catch(() => { if (!disposed) setMuseReady(false); });
		refresh();
		const interval = window.setInterval(refresh, 5_000);
		return () => { disposed = true; window.clearInterval(interval); };
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
		setSessions((current) => current.map((session) => session.id === activeSessionId ? {
			...session,
			metadata: { ...session.metadata, approvalMode: mode, ...config }
		} : session));
		void nativeCodex?.close(activeSessionId).catch((reason) => showToast(reason instanceof Error ? reason.message : String(reason)));
	}, [activeSessionId, nativeCodex, showToast]);

	// The composer describes the active conversation, not merely the global
	// default. Keep its label synchronized with the policy that will actually be
	// sent when a persisted Codex conversation is resumed.
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

	useEffect(() => {
		let disposed = false;
		let subscription: { close(): void } | null = null;
		if (!activeSessionId) return () => undefined;
		const sessionId = activeSessionId;
		const selected = sessions.find((session) => session.id === sessionId);
		if (selected?.metadata.runtime === "codex-app-server") return () => undefined;

		async function connect() {
			try {
				if (selected?.target.kind === "intern" && nativeIntern) {
					const rows = await nativeIntern.eventsAfter(sessionId, 0, 500);
					if (disposed) return;
					setEventsBySession((current) => ({
						...current,
						[sessionId]: rows.map(appEventToRuntimeEvent).filter((event): event is RuntimeEvent => event !== null)
					}));
					const unlisten = nativeIntern.onEvent((appEvent) => {
						if (disposed || appEvent.sessionId !== sessionId) return;
						const event = appEventToRuntimeEvent(appEvent);
						if (!event) return;
						setEventsBySession((current) => ({
							...current,
							[sessionId]: appendEvent(current[sessionId] ?? [], event)
						}));
						if (
							event.eventKind.startsWith("run.") ||
							event.eventKind === "command.receipt" ||
							event.eventKind === "command.resolved" ||
							event.eventKind === "session.updated" ||
							event.eventKind === "intern.projection_updated"
						) {
							void refreshSessions().catch(() => undefined);
						}
					});
					subscription = { close: unlisten };
					return;
				}
				const page = await browserRuntimeClient.events(sessionId, 0, 500);
				if (disposed) return;
				setEventsBySession((current) => ({
					...current,
					[sessionId]: page.events
				}));
				subscription = await browserRuntimeClient.subscribe(
					sessionId,
					page.nextSequence,
					(event) => {
						if (disposed) return;
						setEventsBySession((current) => ({
							...current,
							[sessionId]: appendEvent(current[sessionId] ?? [], event)
						}));
						if (
							event.eventKind.startsWith("run.") ||
							event.eventKind === "usage.recorded" ||
							event.eventKind === "command.receipt" ||
							event.eventKind === "command.resolved" ||
							event.eventKind === "session.updated" ||
							event.eventKind === "intern.projection_updated"
						) {
							void refreshSessions().catch(() => undefined);
						}
					},
					undefined,
					(event) => {
						if (disposed) return;
						setCodexActivityBySession((current) => ({
							...current,
							[sessionId]: appendCodexActivity(current[sessionId] ?? [], event)
						}));
					}
				);
			} catch (reason) {
				if (!disposed) {
					showToast(reason instanceof Error ? reason.message : String(reason));
				}
			}
		}

		void connect();
		return () => {
			disposed = true;
			subscription?.close();
		};
	}, [activeSessionId, nativeIntern, refreshSessions, sessions, showToast]);

	const state = useMemo(() => {
		const base = buildLandingState({
			health,
			sessions,
			eventsBySession,
			codexActivityBySession,
			selectedTargetId,
			laguna,
			apiKeyConfigured
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
		apiKeyConfigured,
		downloadPaused,
		eventsBySession,
		codexActivityBySession,
		health,
		laguna,
		preferences.conversations,
		selectedTargetId,
		sessions
	]);

	const workingChatIds = useMemo(() => new Set(sessions
		.filter((session) => session.target.kind !== "intern" && session.status === "running")
		.map((session) => session.id)), [sessions]);

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
		for (const session of sessions) {
			const previous = queueDrainStatusesRef.current.get(session.id);
			const finished = session.status === "ready" || session.status === "interrupted" || session.status === "completed" || session.status === "failed";
			if (previous === "running" && finished) {
				const next = nextQueuedPrompt(session.id);
				if (next && !queueDrainingRef.current.has(session.id)) {
					queueDrainingRef.current.add(session.id);
					void sendToSessionRef.current(session.id, next.text).then((accepted) => {
						if (accepted) setPreferences(removeQueuedPrompt(next.id));
					}).finally(() => queueDrainingRef.current.delete(session.id));
				}
			}
			queueDrainStatusesRef.current.set(session.id, session.status);
		}
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
	const activeChatRunning = activeChat ? (() => {
		// A restored session record is authoritative. In particular, a stale
		// run.started event must not resurrect Working after the app-server that
		// owned that turn has exited or the desktop app has restarted.
		if (activeChatSession) return activeChatSession.status === "running";
		const latestRunEvent = [...(eventsBySession[activeChat.id] ?? [])]
			.reverse()
			.find((event) => event.eventKind.startsWith("run."));
		return latestRunEvent?.eventKind === "run.started";
	})() : false;
	const activeChatWarmingUp = Boolean(
		activeChatRunning &&
		activeChatSession?.target.kind === "local" &&
		(laguna?.phase === "loading" || !laguna?.loadedModel)
	);
	const activeLocalModel = activeChatSession?.target.kind === "local";
	/*
	 * The rail takes a fixed-ish column out of the workbench. Below this width
	 * it squeezed the transcript until the composer fell under its 320px usable
	 * floor and slid beneath the rail. Unmount rather than `display: none` so
	 * the pane genuinely does not exist for layout, measurement, or a11y.
	 * 368px = the composer's 320px floor plus the transcript's 24px gutters;
	 * 300px = the rail's own minimum column.
	 */
	const workbenchWidth = viewportWidth - (sidebarVisible ? sidebarWidth : 0);
	const inferenceRailFits = workbenchWidth >= 368 + 300;
	const showInferenceRail = activeLocalModel && inferenceRailOpen && inferenceRailFits;
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
					await nativeCodex.start(codexStartRequest(id, workspace, target, permissions, preferences.agentContext.autoCompactTokenLimits));
					const session = createCodexSession(id, target, null, workspace, title, permissions);
					sessionsRef.current = [session, ...sessionsRef.current.filter((item) => item.id !== session.id)];
					setSessions(sessionsRef.current);
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
		[approvalPolicy, sandboxMode, nativeCodex, nativeIntern, preferences.agentContext.autoCompactTokenLimits, refreshSessions, selectedTargetId, showToast]
	);

	const ensureActiveSession = useCallback(async (objective: string): Promise<{ sessionId: string; objectiveConsumed: boolean } | null> => {
		if (activeSessionId) return { sessionId: activeSessionId, objectiveConsumed: false };
		if (view.kind !== "landing") return null;
		const target = targetIdToExecutionTarget(selectedTargetId);
		const objectiveConsumed = target.kind === "intern";
		const session = await createConversation(selectedTargetId, undefined, objectiveConsumed ? objective : undefined);
		return { sessionId: session.id, objectiveConsumed };
	}, [activeSessionId, createConversation, selectedTargetId, view.kind]);

	const sendToSession = useCallback(
		async (sessionId: string, text: string, options?: { messageId?: string; images?: ComposerImageAttachment[] }) => {
			setBusy(true);
			try {
				const session = sessionsRef.current.find((candidate) => candidate.id === sessionId);
				if (nativeCodex && (!session || session.target.kind !== "intern")) {
					if (!session) throw new Error(`Native Codex session is not registered: ${sessionId}`);
					if (session.metadata.runtime !== "codex-app-server") {
						throw new Error(`Session ${sessionId} is not owned by Codex app-server`);
					}
					const sessionTargetId = executionTargetToUiId(session.target);
					const pendingTargetId = isInternTargetId(selectedTargetId) ? sessionTargetId : selectedTargetId;
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
						...codexStartRequest(sessionId, workspace, executionTarget, "ask", preferences.agentContext.autoCompactTokenLimits),
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
					setEventsBySession((current) => ({ ...current, [sessionId]: appendEvent(current[sessionId] ?? [], {
						schemaVersion: "synth.desktop-runtime-event.v1", sessionId, sequence,
						eventKind: "message.created", payload: { messageId, role: "user", content: text },
						createdAt: now, source: "local"
					}) }));
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
					setSessions((current) => current.map((item) => {
						if (item.id !== sessionId) return item;
						return {
							...item,
							target: executionTarget,
							status: started?.turnId ? "running" : item.status,
							updatedAt: new Date().toISOString()
						};
					}));
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
		[allocateNativeSequence, failTurnStart, modelKnobValues, nativeCodex, nativeIntern, preferences.agentContext.autoCompactTokenLimits, refreshSessions, selectedTargetId, showToast]
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
		[ensureActiveSession, sendToSession, showToast]
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
					if (kind === "close" || kind === "cancel" || kind === "pause") setSessions((current) => current.map((item) => item.id === activeSessionId
						? { ...item, status: kind === "close" ? "completed" : "interrupted", updatedAt: new Date().toISOString() }
						: item));
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
		if (id === "local-muse-glimmer" && !museReady) return;
		// Model chip change only updates pendingTarget. Compact/rebind happen
		// on the next send when pending ≠ session.target (modelSwitchPlan.ts).
		const plan = planModelChipChange({ nextTargetId: id });
		setSelectedTargetId(plan.pendingTargetId);
		if (id.startsWith("local-")) {
			void window.synthLaguna?.listModels().then(async (models) => {
				const modelId = id === "local-muse-glimmer"
					? "meta-models/Muse-Glimmer-30B-GGUF"
					: "poolside/Laguna-XS-2.1-NVFP4-mlx";
				const hit = models.find((model) => model.modelId === modelId);
				if (!hit) throw new Error(`${modelId} is not installed`);
				await window.synthLaguna?.setModelDirectory(hit.path);
				setLaguna(await window.synthLaguna!.reload());
			}).catch((reason) => showToast(reason instanceof Error ? reason.message : String(reason)));
		}
	}, [museReady, showToast]);

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
				preferences.agentContext.autoCompactTokenLimits
			);
			await nativeCodex.compact(request);
			showToast("Compacting context…");
		} catch (reason) {
			manualCompactionPendingRef.current.delete(activeSessionId);
			showToast(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setBusy(false);
		}
	}, [activeChatRunning, activeSessionId, nativeCodex, preferences.agentContext.autoCompactTokenLimits, showToast]);

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
				? "Inventory"
				: view.kind === "async"
					? "Intern · Background"
					: view.kind === "sync"
						? (activeSync?.title ?? "Intern · Live")
						: view.kind === "chat"
							? (activeChat?.title ?? "Chat")
							: (EXECUTION_TARGETS.find((t) => t.id === selectedTargetId)?.label ?? "Synth");

	const showComposer = view.kind === "landing" || view.kind === "chat";

	useEffect(() => {
		const inventoryTab =
			view.kind === "inventory" ? ("containers" as const) : null;
		const visibleEvents = activeSessionId
			? (eventsBySessionRef.current[activeSessionId] ?? [])
			: [];
		const api: SemanticEvalApi = {
			schemaVersion: "synth.desktop-eval-api.v1",
			getState: () => ({
				schemaVersion: "synth.desktop-semantic-ui.v1",
				selectedSessionId: activeSessionId,
				sessions,
				visibleEvents,
				openVisualId: openArtifactId,
				inventoryTab,
				controls: [
					{
						id: "new-conversation",
						role: "button",
						name: "New conversation",
						enabled: !busy
					},
					{
						id: "composer-input",
						role: "textbox",
						name: "Message composer",
						enabled: showComposer && !busy
					},
					{
						id: "composer-send",
						role: "button",
						name: "Send",
						enabled: showComposer && !busy
					},
					{
						id: "open-inventory",
						role: "button",
						name: "Inventory",
						enabled: true
					}
				]
			}),
			listActions: () => [
				"create_session",
				"send_message",
				"open_visual",
				"list_inventory",
				"select_session",
				"wait_for_terminal",
				"export_session"
			],
			invoke: async (action, args = {}) => {
				if (action === "create_session") {
					const target =
						typeof args.targetId === "string"
							? args.targetId
							: typeof args.target === "string"
								? args.target
								: selectedTargetId;
					const objective = typeof args.objective === "string" ? args.objective : undefined;
					return createConversation(target, undefined, objective);
				}
				if (action === "send_message") {
					if (typeof args.body !== "string") throw new Error("send_message requires body");
					const sessionId =
						typeof args.sessionId === "string" ? args.sessionId : activeSessionId;
					if (!sessionId) throw new Error("send_message requires an active session");
					await sendToSession(sessionId, args.body);
					return { ok: true };
				}
				if (action === "open_visual") {
					const visualId = args.visualId;
					if (typeof visualId !== "string") throw new Error("open_visual requires visualId");
					if (!window.synthVisuals) throw new Error("Rust visual registry is unavailable");
					const visual = await window.synthVisuals.get(visualId);
					openVisualRecord(visual);
					return visual;
				}
				if (action === "list_inventory") {
					if (!window.synthInventory || !window.synthVisuals) {
						throw new Error("Rust inventory is unavailable");
					}
					const [containers, traces, visuals] = await Promise.all([
						window.synthInventory.listContainers(),
						window.synthInventory.listTraces(),
						window.synthVisuals.list({ limit: 500 })
					]);
					return { containers, traces, visuals };
				}
				if (action === "select_session") {
					const sessionId = args.sessionId;
					if (typeof sessionId !== "string") {
						throw new Error("select_session requires sessionId");
					}
					const session = sessions.find((s) => s.id === sessionId);
					if (!session) throw new Error("session not found");
					if (sessionIsLocalChat(session)) openChat(sessionId);
					else if (sessionIsSync(session)) setView({ kind: "sync", sessionId });
					else setView({ kind: "async", sessionId });
					return { selectedSessionId: sessionId };
				}
				if (action === "wait_for_terminal") {
					const sessionId =
						typeof args.sessionId === "string" ? args.sessionId : activeSessionId;
					if (!sessionId) throw new Error("wait_for_terminal requires sessionId");
					if (!window.synthCore) throw new Error("Rust journal is unavailable");
					const timeoutMs =
						typeof args.timeoutMs === "number" ? args.timeoutMs : 600_000;
					const pollMs = typeof args.pollMs === "number" ? args.pollMs : 500;
					const deadline = Date.now() + timeoutMs;
					let after = 0;
					while (Date.now() < deadline) {
						const page = await window.synthCore.sessionEventsAfter(sessionId, after, 500);
						for (const event of page) {
							after = Math.max(after, event.sessionSequence ?? event.sequence);
							const kind = event.kind;
							if (
								kind === "run.completed" ||
								kind === "run.failed" ||
								kind === "run.cancelled" ||
								kind === "session.run.completed" ||
								kind === "session.run.failed"
							) {
								return { terminal: true, kind, event, sessionId };
							}
						}
						await new Promise((resolve) => setTimeout(resolve, pollMs));
					}
					return { terminal: false, timedOut: true, sessionId, afterSequence: after };
				}
				if (action === "export_session") {
					const sessionId =
						typeof args.sessionId === "string" ? args.sessionId : activeSessionId;
					if (!sessionId) throw new Error("export_session requires sessionId");
					if (!window.synthCore) throw new Error("Rust journal is unavailable");
					const events = [];
					let after = 0;
					for (;;) {
						const page = await window.synthCore.sessionEventsAfter(sessionId, after, 500);
						if (!page.length) break;
						for (const event of page) {
							after = Math.max(after, event.sessionSequence ?? event.sequence);
						}
						events.push(...page);
						if (events.length > 50_000) break;
					}
					const session = sessions.find((s) => s.id === sessionId) ?? null;
					return {
						schemaVersion: "synth.eval-session-export.v1",
						sessionId,
						session,
						events,
						eventCount: events.length
					};
				}
				throw new Error(`Unknown semantic action: ${action}`);
			}
		};
		window.__synthEval = api;
		window.__synthPreferences = preferencesAdapter();
		window.dispatchEvent(new CustomEvent("synth-eval-ready"));
		return () => {
			if (window.__synthEval === api) delete window.__synthEval;
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
		view.kind
	]);

	return (
		<div className="app-shell">
			<div className="body-row">
					<Sidebar
						state={state}
						lagunaStatus={laguna}
					activeChatId={view.kind === "chat" ? view.chatId : null}
					inventoryActive={view.kind === "inventory"}
					visualsActive={view.kind === "visuals"}
					optimizersActive={view.kind === "optimizers"}
					connectorsActive={view.kind === "connectors"}
					workingChatIds={workingChatIds}
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
					onOpenConnectors={() => setView({ kind: "connectors" })}
					onSearch={openSearch}
					onSettings={() => setView({ kind: "settings" })}
					accountSignedIn={accountSummary?.signedIn ?? apiKeyConfigured}
					accountDisplayName={accountSummary?.displayName ?? null}
					accountPlan={accountSummary?.plan ?? null}
					accountUsage={accountUsage}
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
				/>

				<main className="main-pane">
					<header className="titlebar" data-testid="titlebar" data-tauri-drag-region="">
						<div className="titlebar-tabs" data-tauri-drag-region="">
							<div className="tab tab-active" role="tab" aria-selected data-tauri-drag-region="">
								<SynthLogo className="tab-logo" compact />
								<span>{truncate(tabLabel, 28)}</span>
								<button
									type="button"
									className="tab-close"
									aria-label="Close tab"
									onClick={() => {
										setView({ kind: "landing" });
										showToast("Back to landing");
									}}
								>
									×
								</button>
							</div>
							{activeLocalModel ? <button
								type="button"
								className="tab-new"
								aria-label="New tab"
								onClick={onNewConversation}
							>
								+
							</button> : null}
						</div>
						<div className="titlebar-actions">
							{(() => {
								const runtime = localRuntimePresentation(health, laguna);
								const diagnostic = health
									? [
										health.runtimeId,
										`Laguna ${health.local.mode}`,
										laguna?.phase ? `sidecar ${laguna.phase}` : null,
										laguna?.backend ? `backend ${laguna.backend}` : null,
										laguna?.loadedModel || health.local.modelPath ||
											(laguna?.phase === "ready" ? "weights currently unloaded" : "weights not detected"),
										`OpenRouter ${health.openrouter.mode}`,
										health.inventory
											? `Inventory ${health.inventory.containers} containers, ${health.inventory.traces} traces, ${health.inventory.visuals} visuals`
											: null
									].filter(Boolean).join(" · ")
									: laguna?.detail || runtime.label;
								return (
									<span
										className={`runtime-pill ${runtime.tone}`}
										data-testid="runtime-status"
										aria-label={runtime.label}
										title={diagnostic}
									>
										<span className="runtime-pill-dot" aria-hidden />
										<span className="runtime-pill-label">{runtime.visibleLabel}</span>
									</span>
								);
							})()}
							{activeLocalModel ? <button
								type="button"
								className={`titlebar-icon-btn${inferenceRailOpen ? " active" : ""}`}
								aria-label={inferenceRailOpen ? "Hide inference monitor" : "Show inference monitor"}
								aria-pressed={inferenceRailOpen}
								title="MLX sidecar inference stats"
								data-testid="toggle-inference-rail"
								onClick={() => {
									setInferenceRailOpen((current) => {
										const next = !current;
										window.localStorage.setItem("synth.inferenceRailOpen", next ? "1" : "0");
										return next;
									});
								}}
							>
								<IconPulse />
							</button> : null}
							<button
								type="button"
								className="avatar-btn"
								aria-label="Account"
								data-testid="open-account-settings"
								onClick={() => setView({ kind: "settings", section: "account" })}
							>
								S
							</button>
							<button
								type="button"
								className="titlebar-icon-btn"
								aria-label="Models"
								title="Models"
								data-testid="open-models-settings"
								onClick={() => setView({ kind: "settings", section: "models" })}
							>
								<IconCloud />
							</button>
							<button
								type="button"
								className="titlebar-icon-btn"
								aria-label={terminalOpen ? "Hide terminal" : "Show terminal"}
								title="Toggle terminal (⌘J)"
								onClick={() => {
									setTerminalOpen((current) => {
										const next = !current;
										persistLayoutSnapshot({ bottomPanelVisible: next });
										return next;
									});
								}}
							>
								<IconLayout />
							</button>
						</div>
					</header>

					{bootError ? (
						<div className="boot-error" role="alert">
							Runtime unavailable: {bootError}
						</div>
					) : null}

					{view.kind === "settings" ? (
						<SettingsPage
							key={view.section ?? "general"}
							onBack={() => setView({ kind: "landing" })}
							onReloadLaguna={onReloadLaguna}
							health={health}
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
							conversationTitles={conversationTitles}
							onUnarchiveConversation={(id) => setPreferences(archiveConversation(id, false))}
							onOpenConversation={(id) => {
								setPreferences(archiveConversation(id, false));
								openChat(id);
							}}
						/>
					) : null}

					{view.kind === "connectors" ? (
						<ConnectorsPage
							onBack={() => setView({ kind: "landing" })}
							onConfigure={(name) => showToast(name === "Synth Containers" || name === "Synth Visuals"
								? `${name} is provisioned automatically for every agent`
								: `${name} setup is not installed in this build`)}
						/>
					) : null}

					{view.kind === "visuals" ? (
						<div className={`inventory-workbench${openArtifact ? " with-visual" : ""}`}>
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
										if (!window.synthVisuals) {
											showToast("Visual registry requires Synth Desktop");
											return;
										}
										try {
											const templates = await window.synthVisuals.listTemplates();
											const templateId = templates[0]?.id ?? "reward.breakdown.v1";
											const visual = await window.synthVisuals.create({
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
								<VisualPane artifact={openArtifact} onClose={() => toggleArtifact(null)} />
							) : null}
						</div>
					) : null}

					{view.kind === "optimizers" ? (
						<div className={`inventory-workbench${openArtifact ? " with-visual" : ""}`}>
							<OptimizersPage
								onOpenVisual={(visualId) => {
									void (async () => {
										if (!window.synthVisuals) {
											showToast("Visual registry requires Synth Desktop");
											return;
										}
										try {
											const visual = await window.synthVisuals.get(visualId);
											openVisualRecord(visual);
										} catch (reason) {
											showToast(String(reason));
										}
									})();
								}}
								onBack={() => setView({ kind: "landing" })}
							/>
							{openArtifact ? (
								<VisualPane artifact={openArtifact} onClose={() => toggleArtifact(null)} />
							) : null}
						</div>
					) : null}

					{view.kind === "inventory" ? (
						<div
							className={`inventory-workbench${openArtifact ? " with-visual" : ""}${openContainer ? " with-container" : ""}${containerPaneExpanded ? " container-expanded" : ""}`}
							style={{ "--container-pane-width": `${inventoryContainerWidth}px` } as CSSProperties}
						>
							<InventoryPage
								onOpenVisual={openVisualRecord}
								onOpenContainer={(id) => void toggleContainer(id)}
								openContainerId={openContainer?.id ?? null}
								onBack={() => setView({ kind: "landing" })}
							/>
							{openArtifact ? (
								<VisualPane
									artifact={openArtifact}
									onClose={() => toggleArtifact(null)}
								/>
							) : null}
							{openContainer ? (
								<>
									<PaneResizeHandle value={inventoryContainerWidth} onChange={(width) => {
										setInventoryContainerWidth(width);
										persistLayoutSnapshot({ outputPaneWidth: width });
									}} />
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
								museReady={museReady}
						/>
					) : null}

					{view.kind === "chat" && activeChat ? (
						<div className={`workbench${openArtifact ? " with-visual" : ""}${openContainer ? " with-container" : ""}${containerPaneExpanded ? " container-expanded" : ""}${showInferenceRail ? " with-inference" : ""}`}>
								<ChatTranscript
									chat={activeChat}
									openArtifactId={openArtifactId}
									onOpenArtifact={toggleArtifact}
									openContainerId={openContainer?.id ?? null}
									onOpenContainer={(id) => void toggleContainer(id)}
									onApprove={(approvalId) => void controlActive("approve", { approvalId })}
									onAlwaysAllow={(approvalId) => void controlActive("approve", { approvalId, decision: "always" })}
										onReject={(approvalId) => void controlActive("reject", { approvalId })}
										running={activeChatRunning}
										warmingUp={activeChatWarmingUp}
										onStop={() => {
											setQueueAfterStop(promptsForConversation(activeChat.id).length > 0);
											void controlActive("cancel");
										}}
										activityMode={preferences.toolActivity.mode}
										onActivityModeChange={(mode) => setPreferences(setToolActivityMode(mode))}
								/>
							{failedSend && failedSend.sessionId === activeChat.id ? (
								<div
									role="status"
									data-testid="send-retry"
									style={{
										display: "flex", alignItems: "center", justifyContent: "space-between",
										gap: 12, margin: "0 16px 8px", padding: "8px 12px", borderRadius: 10,
										border: "1px solid currentColor", opacity: 0.9, fontSize: 13
									}}
								>
									<span>{failedSend.message}</span>
									<button type="button" data-testid="send-retry-button" onClick={retryFailedSend}>
										Retry
									</button>
								</div>
							) : null}
							{openArtifact ? (
								<VisualPane artifact={openArtifact} onClose={() => toggleArtifact(null)} />
							) : null}
							{openContainer ? (
								<ContainerPane
									container={openContainer}
									expanded={containerPaneExpanded}
									onExpandedChange={setContainerPaneExpanded}
									onProbe={() => void probeOpenContainer()}
									onClose={() => void toggleContainer(null)}
								/>
							) : null}
							{showInferenceRail ? (
								<aside className="inference-rail" data-testid="inference-rail" aria-label="Local inference monitor">
									<div className="inference-rail-label">
										<span>MLX sidecar</span>
										<small>Owns local model memory, prompt caches, and the single-GPU queue.</small>
									</div>
									{/* `visible` drives subscribe/teardown, so a closed rail
									    costs nothing. */}
									<InferencePanel
										visible
										monitor={inferenceMonitor}
										turnRunning={Boolean(
											activeChatRunning && activeChatSession?.target.kind === "local"
										)}
										warmingUp={activeChatWarmingUp}
										onOpenSettings={() => setView({ kind: "settings", section: "inference" })}
									/>
								</aside>
							) : null}
						</div>
					) : null}

					{/*
					 * v0.1 removal contract: the CloudDesk sync/async routes are the
					 * Intern surface and stay unmounted. components/CloudDesk.tsx is
					 * retained dormant so v0.2 re-entry is a routing change, not a
					 * rewrite.
					 */}

					{showComposer ? (
						<Composer
							state={state}
							workspaceSessionId={activeSessionId}
							onEnsureWorkspaceSession={async () => {
								if (activeSessionId) return activeSessionId;
								if (view.kind !== "landing") return null;
								const session = await createConversation(selectedTargetId);
								return session.id;
							}}
							workspaceFallback={activeSessionId ? (sessions.find((item) => item.id === activeSessionId)?.metadata.workspace as string | undefined) ?? defaultWorkspace : defaultWorkspace}
							workspaceScope={workspaceScope}
							onWorkspaceScopeChange={setWorkspaceScope}
							onWorkspaceError={showToast}
							onSend={(text) => void onComposerSend(text)}
							onSelectTarget={onSelectTarget}
							onConfigureAccount={() => setView({ kind: "settings", section: "account" })}
							museReady={museReady}
							onOpenVoiceSettings={() => setView({ kind: "settings", section: "voice" })}
							skills={composerSkills}
							onSlashNew={onNewConversation}
							onSlashMcp={() => setView({ kind: "connectors" })}
							onSlashRename={onSlashRename}
							onSlashCompact={onSlashCompact}
							approvalPolicy={approvalPolicy}
							sandboxMode={sandboxMode}
							onSelectPermissions={selectActivePermissions}
							modelKnobValues={modelKnobValues}
							onSelectModelKnob={selectModelKnob}
							modelMedianTpsLabel={selectedModelMedianTpsLabel}
							aggregateModelTpsLabels={aggregateModelTpsLabels}
							agentWorking={Boolean(activeChatRunning)}
							activeEnterAction={preferences.submission.activeEnterAction}
							steerSupported={Boolean(nativeCodex?.steerTurn)}
							steerError={steerError}
							onSteer={async (text) => {
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
							}}
							onEnqueue={(text) => {
								const conversationId = activeSessionId;
								if (!conversationId) {
									showToast("No active conversation to queue into");
									return;
								}
								setSteerError(null);
								setPreferences(enqueuePrompt(conversationId, text));
							}}
							queuedPrompts={activeSessionId ? promptsForConversation(activeSessionId, preferences) : []}
							onEditQueuedPrompt={(id, text) => {
								try {
									setPreferences(updateQueuedPrompt(id, text));
								} catch (reason) {
									showToast(reason instanceof Error ? reason.message : String(reason));
								}
							}}
								onRemoveQueuedPrompt={(id) => setPreferences(removeQueuedPrompt(id))}
								onPromoteQueuedPrompt={async (id, text) => {
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
								}}
								queueAfterStop={queueAfterStop}
							onKeepQueued={() => setQueueAfterStop(false)}
							onSendNextQueued={() => {
								if (!activeSessionId) return;
								const next = nextQueuedPrompt(activeSessionId);
								setQueueAfterStop(false);
								if (next) void sendToSession(activeSessionId, next.text).then((accepted) => {
									if (accepted) setPreferences(removeQueuedPrompt(next.id));
								});
							}}
						/>
					) : null}
			<TerminalPanel
						open={terminalOpen}
						workspaceId={terminalWorkspaceId}
						workspaceRoot={terminalWorkspaceRoot}
						onOpenChange={(open) => {
							setTerminalOpen(open);
							persistLayoutSnapshot({ bottomPanelVisible: open });
						}}
			/>
		</main>
	</div>

	{searchOpen ? (
		<ConversationSearch
			state={state}
			onClose={closeSearch}
			onOpenChat={(id) => openChat(id)}
		/>
	) : null}

			{toast ? (
				<div className="toast" role="status" key={toast}>
					{toast}
				</div>
			) : null}
		</div>
	);
}
