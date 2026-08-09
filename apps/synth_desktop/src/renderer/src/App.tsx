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
	Project,
	SemanticUiSnapshot,
	Session,
	VisualInstanceRecord,
	VisualRecord
} from "@synth/runtime-protocol";
import { EXECUTION_TARGETS } from "./types/landing";
import type { ArtifactRef } from "./types/landing";
import { ChatTranscript } from "./components/ChatTranscript";
import { ContainerPane } from "./components/ContainerPane";
import { CloudDesk } from "./components/CloudDesk";
import { Composer } from "./components/Composer";
import { ConnectorsPage } from "./components/ConnectorsPage";
import { ConversationSearch } from "./components/ConversationSearch";
import { DemoFixturesBar } from "./components/DemoFixturesBar";
import { InventoryPage } from "./components/InventoryPage";
import { LandingPage } from "./components/LandingPage";
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
import { codexEventToRuntime, codexStartRequest, coreEventToRuntime, createCodexSession, restoreCodexSession, type ApprovalMode } from "./runtime/nativeCodex";
import {
	loadModelKnobValues,
	modelKnobForTarget,
	modelKnobKey,
	turnStartEffortForExecutionTarget,
	type ModelKnobValue
} from "./runtime/modelCapabilities";
import type { LagunaStatus } from "./env";

type MainView =
	| { kind: "landing" }
	| { kind: "chat"; chatId: string }
	| { kind: "sync"; sessionId: string }
	| { kind: "async"; sessionId: string }
	| { kind: "settings"; section?: "models" | "runtime" | "account" }
	| { kind: "connectors" }
	| { kind: "inventory" }
	| { kind: "visuals" };

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
	if (events.some((candidate) => candidate.sequence === event.sequence)) return events;
	return [...events, event].sort((left, right) => left.sequence - right.sequence);
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
	async listProjects() {
		return (await this.bridge().request<{ projects: Project[] }>("/v1/projects")).projects;
	},
	createProject(body: { path: string }) {
		return this.bridge().request<Project>("/v1/projects", { method: "POST", body });
	},
	createSession(target: ExecutionTarget, title?: string, projectId?: string | null, objective?: string) {
		return this.bridge().request<Session>("/v1/sessions", { method: "POST", body: { target, title, projectId, objective } });
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

function IconExpand() {
	return (
		<svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path
				d="M3 6V3h3M13 6V3h-3M3 10v3h3M13 10v3h-3"
				stroke="currentColor"
				strokeWidth="1.3"
				strokeLinecap="round"
				strokeLinejoin="round"
			/>
		</svg>
	);
}

function IconChevron() {
	return (
		<svg width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden>
			<path
				d="M3 4.5L6 7.5L9 4.5"
				stroke="currentColor"
				strokeWidth="1.4"
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
	const nativeProjects = isDesktop ? window.synthProjects : undefined;
	const [health, setHealth] = useState<RuntimeHealth | null>(null);
	const [laguna, setLaguna] = useState<LagunaStatus | null>(null);
	const [sessions, setSessions] = useState<Session[]>([]);
	const sessionsRef = useRef<Session[]>([]);
	const [projects, setProjects] = useState<Project[]>([]);
	const [selectedProjectId, setSelectedProjectId] = useState<string | null>(null);
	const [eventsBySession, setEventsBySession] = useState<Record<string, RuntimeEvent[]>>({});
	const [codexActivityBySession, setCodexActivityBySession] = useState<Record<string, CodexActivityEvent[]>>({});
	const [selectedTargetId, setSelectedTargetId] = useState("local-laguna");
	const [approvalMode, setApprovalMode] = useState<ApprovalMode>(() => {
		const saved = window.localStorage.getItem("synth.approvalMode");
		return saved === "accept-edits" || saved === "plan" || saved === "allow-all" ? saved : "ask";
	});
	const selectApprovalMode = useCallback((mode: ApprovalMode) => {
		setApprovalMode(mode);
		window.localStorage.setItem("synth.approvalMode", mode);
	}, []);
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
	const [view, setView] = useState<MainView>({ kind: "landing" });
	const [searchOpen, setSearchOpen] = useState(false);
	const [unreadChatIds, setUnreadChatIds] = useState<Set<string>>(() => {
		try {
			const saved = JSON.parse(window.localStorage.getItem("synth.unreadCompletedChats") ?? "[]");
			return new Set(Array.isArray(saved) ? saved.filter((id): id is string => typeof id === "string") : []);
		} catch {
			return new Set();
		}
	});
	const previousSessionStatusesRef = useRef(new Map<string, Session["status"]>());
	const [openArtifactId, setOpenArtifactId] = useState<string | null>(null);
	const [standaloneVisual, setStandaloneVisual] = useState<ArtifactRef | null>(null);
	const [openContainer, setOpenContainer] = useState<ContainerDeployment | null>(null);
	const [containerPaneExpanded, setContainerPaneExpanded] = useState(false);
	const [inventoryContainerWidth, setInventoryContainerWidth] = useState(() => {
		const saved = Number(window.localStorage.getItem("synth.inventoryContainerPaneWidth"));
		return Number.isFinite(saved) && saved >= 340 ? saved : 420;
	});
	const [busy, setBusy] = useState(false);
	const [demoBusy, setDemoBusy] = useState(false);
	const [bootError, setBootError] = useState<string | null>(null);
	const [terminalOpen, setTerminalOpen] = useState(false);
	const [defaultWorkspace, setDefaultWorkspace] = useState<string | null>(null);
	const eventsBySessionRef = useRef(eventsBySession);
	const nativeSequencesRef = useRef(new Map<string, number>());
	const autoOpenedSubagentsRef = useRef(new Set<string>());
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
			const [core, config, counts, currentProjects, currentLaguna] = await Promise.all([
				window.synthCore.diagnostics(),
				window.synthConfig.get(),
				window.synthInventory.counts(),
				nativeProjects?.list() ?? Promise.resolve([]),
				window.synthLaguna?.getStatus() ?? Promise.resolve(null)
			]);
			const next: RuntimeHealth = {
				status: "ok",
				protocolVersion: "synth.desktop-runtime.v1",
				runtimeId: "core-runtime",
				startedAt: new Date().toISOString(),
				intern: { mode: config.apiKeyConfigured ? "remote" : "unconfigured", backendUrl: config.backendUrl },
				local: {
					model: "laguna-xs-2.1",
					mode: currentLaguna?.phase === "ready" ? "mlx" : "stub",
					modelPath: currentLaguna?.loadedModel ?? null
				},
				openrouter: { mode: config.openrouterApiKeyConfigured ? "ready" : "unconfigured", models: [] },
				inventory: { containers: counts.containers, traces: counts.traces, visuals: core.visualCount },
				dataStore: {
					path: core.databasePath,
					projects: currentProjects.length,
					sessions: core.sessionCount,
					runs: core.runCount,
					events: core.journalHead,
					usage: counts.usage
				}
			};
			setHealth(next);
			return next;
		}
		const next = await browserRuntimeClient.health();
		setHealth(next);
		return next;
	}, [isDesktop, nativeProjects]);

	const refreshProjects = useCallback(async () => {
		const next = nativeProjects
			? await nativeProjects.list()
			: await browserRuntimeClient.listProjects();
		setProjects(next);
		return next;
	}, [nativeProjects]);

	useEffect(() => {
		void window.synthCodex?.defaultWorkspace().then(setDefaultWorkspace).catch(() => undefined);
	}, []);

	useEffect(() => {
		const onKey = (event: KeyboardEvent) => {
			if (!(event.metaKey || event.ctrlKey)) return;
			if (event.key.toLowerCase() === "j" && !event.shiftKey) {
				event.preventDefault(); setTerminalOpen((current) => !current);
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
			? Promise.all([nativeCodex.list(), nativeIntern?.listSessions() ?? Promise.resolve([]), refreshProjects(), refreshHealth()]).then(async ([persisted, internSessions]) => {
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
				setEventsBySession(Object.fromEntries(replay));
				for (const [sessionId, events] of replay) {
					const head = events.at(-1)?.sequence ?? 0;
					nativeSequencesRef.current.set(sessionId, head);
				}
			})
			: Promise.all([refreshHealth(), refreshSessions(), refreshProjects()]);
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
	}, [nativeCodex, nativeIntern, refreshHealth, refreshProjects, refreshSessions]);

	useEffect(() => {
		if (!nativeCodex) return;
		return nativeCodex.onEvent((event) => {
			const sequence = allocateNativeSequence(event.sessionId);
			const runtimeEvent = codexEventToRuntime(event, sequence);
			const updatedThreadName = event.method === "thread/name/updated"
				&& typeof event.params.threadName === "string"
				? event.params.threadName.trim()
				: null;
			setEventsBySession((current) => ({
				...current,
				[event.sessionId]: appendEvent(current[event.sessionId] ?? [], runtimeEvent)
			}));
			setSessions((current) => current.map((session) => session.id === event.sessionId
					? { ...session, ...(updatedThreadName ? { title: updatedThreadName } : {}),
						updatedAt: runtimeEvent.createdAt, latestCursor: sequence,
						status: runtimeEvent.eventKind === "run.started" ? "running"
							: runtimeEvent.eventKind === "run.completed" ? "ready"
						: runtimeEvent.eventKind === "run.failed" ? "failed"
						: runtimeEvent.eventKind === "run.cancelled" ? "cancelled"
						: runtimeEvent.eventKind === "session/unhealthy" ? "interrupted"
						: session.status }
				: session));
		});
	}, [allocateNativeSequence, nativeCodex]);

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
			void refreshProjects().catch(() => undefined);
			void refreshHealth().catch(() => undefined);
		}, 2_500);
		return () => window.clearInterval(interval);
	}, [nativeCodex, refreshHealth, refreshProjects, refreshSessions]);

	const activeSessionId = useMemo(() => {
		if (view.kind === "chat") return view.chatId;
		if (view.kind === "sync") return view.sessionId;
		if (view.kind === "async") return view.sessionId;
		return null;
	}, [view]);
	const terminalProject = projects.find((project) => project.id === selectedProjectId) ?? null;
	const terminalWorkspaceRoot = terminalProject?.path ?? defaultWorkspace;
	const terminalWorkspaceId = activeSessionId ?? terminalProject?.id ?? "default";

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
			projects: projects.map((project) => ({ id: project.id, name: project.name }))
		});
		if (base.model.status !== "downloading") return base;
		return {
			...base,
			model: { ...base.model, downloadPaused }
		};
	}, [
		downloadPaused,
		eventsBySession,
		codexActivityBySession,
		health,
		laguna,
		selectedTargetId,
		sessions,
		projects
	]);

	const workingChatIds = useMemo(() => new Set(sessions
		.filter((session) => session.target.kind !== "intern" && session.status === "running")
		.map((session) => session.id)), [sessions]);

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
			window.localStorage.setItem("synth.unreadCompletedChats", JSON.stringify([...next]));
			return next;
		});
	}, [sessions, view]);

	useEffect(() => {
		if (view.kind !== "chat" || !unreadChatIds.has(view.chatId)) return;
		setUnreadChatIds((current) => {
			const next = new Set(current);
			next.delete(view.chatId);
			window.localStorage.setItem("synth.unreadCompletedChats", JSON.stringify([...next]));
			return next;
		});
	}, [unreadChatIds, view]);

	const activeChat =
		view.kind === "chat" ? (state.chats.find((c) => c.id === view.chatId) ?? null) : null;
	const activeChatRunning = activeChat ? (() => {
		const session = sessions.find((candidate) => candidate.id === activeChat.id);
		// A restored session record is authoritative. In particular, a stale
		// run.started event must not resurrect Working after the app-server that
		// owned that turn has exited or the desktop app has restarted.
		if (session) return session.status === "running";
		const latestRunEvent = [...(eventsBySession[activeChat.id] ?? [])]
			.reverse()
			.find((event) => event.eventKind.startsWith("run."));
		return latestRunEvent?.eventKind === "run.started";
	})() : false;
	const activeSync =
		view.kind === "sync"
			? (state.syncSessions.find((s) => s.id === view.sessionId) ?? null)
			: null;
	const asyncSession =
		view.kind === "async" ? (sessions.find((s) => s.id === view.sessionId) ?? null) : null;

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
					// Projects organize cloud work. Local/configured-provider Codex tasks
					// always have their own safe workspace and never require a project.
					const workspace = await nativeCodex.defaultWorkspace();
					await nativeCodex.start(codexStartRequest(id, workspace, target, approvalMode));
					const session = createCodexSession(id, target, null, workspace, title, approvalMode);
					sessionsRef.current = [session, ...sessionsRef.current.filter((item) => item.id !== session.id)];
					setSessions(sessionsRef.current);
					setView({ kind: "chat", chatId: session.id });
					return session;
				}
				const session = target.kind === "intern" && nativeIntern
					? await nativeIntern.createSession({ target, objective: internObjective!, title, projectId: selectedProjectId })
					: await browserRuntimeClient.createSession(target, title, selectedProjectId, internObjective);
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
		[approvalMode, nativeCodex, nativeIntern, refreshSessions, selectedProjectId, selectedTargetId, showToast]
	);

	const onAddProject = useCallback(async () => {
		try {
			const path = await window.synthDesktop.chooseProjectDirectory();
			if (!path) return;
			if (nativeProjects) {
				const project = await nativeProjects.create({ path });
				setSelectedProjectId(project.id);
				await refreshProjects();
				showToast(`Project ready · ${project.name}`);
				return;
			}
			const project = await browserRuntimeClient.createProject({ path });
			setSelectedProjectId(project.id);
			await refreshProjects();
			showToast(`Project ready · ${project.name}`);
		} catch (reason) {
			showToast(reason instanceof Error ? reason.message : String(reason));
		}
	}, [nativeProjects, refreshProjects, showToast]);

	const ensureActiveSession = useCallback(async (objective: string): Promise<{ sessionId: string; objectiveConsumed: boolean } | null> => {
		if (activeSessionId) return { sessionId: activeSessionId, objectiveConsumed: false };
		if (view.kind !== "landing") return null;
		const target = targetIdToExecutionTarget(selectedTargetId);
		const objectiveConsumed = target.kind === "intern";
		const session = await createConversation(selectedTargetId, undefined, objectiveConsumed ? objective : undefined);
		return { sessionId: session.id, objectiveConsumed };
	}, [activeSessionId, createConversation, selectedTargetId, view.kind]);

	const sendToSession = useCallback(
		async (sessionId: string, text: string) => {
			setBusy(true);
			try {
				const session = sessionsRef.current.find((candidate) => candidate.id === sessionId);
				if (nativeCodex && (!session || session.target.kind !== "intern")) {
					if (!session) throw new Error(`Native Codex session is not registered: ${sessionId}`);
					if (session.metadata.runtime !== "codex-app-server") {
						throw new Error(`Session ${sessionId} is not owned by Codex app-server`);
					}
					const workspace = typeof session.metadata.workspace === "string"
						? session.metadata.workspace
						: await nativeCodex.defaultWorkspace();
					await nativeCodex.start({
						...codexStartRequest(sessionId, workspace, session.target),
						approvalPolicy: typeof session.metadata.approvalPolicy === "string" ? session.metadata.approvalPolicy : undefined,
						sandbox: typeof session.metadata.sandbox === "string" ? session.metadata.sandbox : undefined,
						threadId: typeof session.metadata.threadId === "string" ? session.metadata.threadId : undefined
					});
					const sequence = allocateNativeSequence(sessionId);
					const now = new Date().toISOString();
					setEventsBySession((current) => ({ ...current, [sessionId]: appendEvent(current[sessionId] ?? [], {
						schemaVersion: "synth.desktop-runtime-event.v1", sessionId, sequence,
						eventKind: "message.created", payload: { messageId: `user-${sequence}`, role: "user", content: text },
						createdAt: now, source: "local"
					}) }));
					setSessions((current) => current.map((item) => item.id === sessionId ? { ...item, status: "running", updatedAt: now } : item));
					const effort = turnStartEffortForExecutionTarget(session.target, modelKnobValues);
					await nativeCodex.startTurn(sessionId, text, effort);
					return;
				}
				if (session?.target.kind === "intern" && nativeIntern) {
					await nativeIntern.send({ sessionId, body: text });
				} else {
					await browserRuntimeClient.sendMessage(sessionId, text);
				}
				await refreshSessions();
			} catch (reason) {
				showToast(reason instanceof Error ? reason.message : String(reason));
			} finally {
				setBusy(false);
			}
		},
		[allocateNativeSequence, modelKnobValues, nativeCodex, nativeIntern, refreshSessions, showToast]
	);

	const onComposerSend = useCallback(
		async (text: string) => {
			try {
				const ensured = await ensureActiveSession(text);
				if (!ensured) {
					showToast("No active session");
					return;
				}
				// Intern creation itself starts the objective. Sending the same text
				// again would issue a duplicate operator command.
				if (!ensured.objectiveConsumed) {
					await sendToSession(ensured.sessionId, text);
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

	const onCloudAction = useCallback(
		(label: string) => {
			const lower = label.toLowerCase();
			if (lower === "pause") void controlActive("pause");
			else if (lower === "resume") void controlActive("resume");
			else if (lower === "close") void controlActive("close");
			else if (lower === "cancel") void controlActive("cancel");
			else if (lower === "checkpoint") void controlActive("request_checkpoint");
			else showToast(`${label} — stub`);
		},
		[controlActive, showToast]
	);

	const onSimulateLive = useCallback(async () => {
		setDemoBusy(true);
		try {
			if (window.synthVisuals) {
				const templates = await window.synthVisuals.listTemplates("compare");
				const template = templates.find((candidate) => candidate.id === "model.compare.v1") ?? templates[0];
				if (!template) throw new Error("No Rust visual template is available for the demo");
				const visual = await window.synthVisuals.create({
					templateId: template.id,
					title: "Live eval comparison",
					bindings: template.exampleBinding ?? {},
					status: "live",
					metadata: { source: "desktop-demo", fixture: true }
				});
				showToast(`Created visual · ${visual.title}`);
				openVisualRecord(visual);
			} else {
				const result = await browserRuntimeClient.simulateLive("eval");
				showToast(`Created visual · ${result.visual.title}`);
				openVisualRecord(result.visual);
			}
			await refreshHealth();
		} catch (reason) {
			showToast(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setDemoBusy(false);
		}
	}, [openVisualRecord, refreshHealth, showToast]);

	const onSelectTarget = useCallback((id: string) => {
		setSelectedTargetId(id);
		if (view.kind !== "chat") return;
		const current = sessionsRef.current.find((session) => session.id === view.chatId);
		if (!current || executionTargetToUiId(current.target) === id) return;
		// A Codex app-server thread is bound to its provider/model at creation.
		// Switching the composer target therefore starts a new task instead of
		// relabeling the existing thread and silently sending to the old model.
		setView({ kind: "landing" });
		setOpenArtifactId(null);
		setStandaloneVisual(null);
	}, [view]);

	const onNewConversation = useCallback(() => {
		setView({ kind: "landing" });
		setOpenArtifactId(null);
		setStandaloneVisual(null);
	}, []);

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

	const onNewSyncSession = useCallback(() => {
		setSelectedTargetId("intern-sync");
		setView({ kind: "landing" });
		setOpenArtifactId(null);
		setStandaloneVisual(null);
		showToast("Enter an objective to start Live Intern");
	}, [showToast]);

	useEffect(() => {
		const onKeyDown = (event: KeyboardEvent) => {
			if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
				event.preventDefault();
				setSearchOpen(true);
			}
			if (event.key === "Escape") setSearchOpen(false);
		};
		window.addEventListener("keydown", onKeyDown);
		return () => window.removeEventListener("keydown", onKeyDown);
	}, []);

	const tabLabel =
		view.kind === "settings"
			? "Settings"
			: view.kind === "connectors"
				? "Connectors"
			: view.kind === "visuals"
				? "Visuals"
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
				"select_session"
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
					if (sessionIsLocalChat(session)) setView({ kind: "chat", chatId: sessionId });
					else if (sessionIsSync(session)) setView({ kind: "sync", sessionId });
					else setView({ kind: "async", sessionId });
					return { selectedSessionId: sessionId };
				}
				throw new Error(`Unknown semantic action: ${action}`);
			}
		};
		window.__synthEval = api;
		window.dispatchEvent(new CustomEvent("synth-eval-ready"));
		return () => {
			if (window.__synthEval === api) delete window.__synthEval;
		};
	}, [
		activeSessionId,
		busy,
		createConversation,
		openArtifactId,
		openVisualRecord,
		selectedTargetId,
		sendToSession,
		sessions,
		showComposer,
		view.kind
	]);

	return (
		<div className="app-shell">
			{import.meta.env.DEV ? (
				<DemoFixturesBar onSimulateLive={() => void onSimulateLive()} busy={demoBusy} />
			) : null}

			<div className="body-row">
					<Sidebar
						state={state}
						lagunaStatus={laguna}
					activeChatId={view.kind === "chat" ? view.chatId : null}
					activeSyncId={view.kind === "sync" ? view.sessionId : null}
					asyncActive={view.kind === "async"}
					inventoryActive={view.kind === "inventory"}
					visualsActive={view.kind === "visuals"}
					connectorsActive={view.kind === "connectors"}
					workingChatIds={workingChatIds}
					unreadChatIds={unreadChatIds}
					onNewConversation={onNewConversation}
					onNewSyncSession={onNewSyncSession}
					onOpenChat={(id) => setView({ kind: "chat", chatId: id })}
					onOpenSyncSession={(id) => setView({ kind: "sync", sessionId: id })}
					onOpenAsync={() => {
						const pinned = sessions.find((session) =>
							sessionIsAsync(session) &&
							(!nativeIntern || session.metadata.runtime === "rust-intern")
						);
						if (!pinned || !state.asyncIntern) {
							setSelectedTargetId("intern-async");
							setView({ kind: "landing" });
							setOpenArtifactId(null);
							setStandaloneVisual(null);
							showToast("Enter an objective to start Background Intern");
							return;
						}
						setView({ kind: "async", sessionId: pinned.id });
					}}
					onOpenInventory={() => setView({ kind: "inventory" })}
					onOpenVisuals={() => setView({ kind: "visuals" })}
					onOpenConnectors={() => setView({ kind: "connectors" })}
					onSearch={() => setSearchOpen(true)}
					onSettings={() => setView({ kind: "settings" })}
					 onPauseToggle={() => setDownloadPaused((v) => !v)}
					onAddProject={onAddProject}
					onSelectProject={setSelectedProjectId}
					selectedProjectId={selectedProjectId}
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
							<button
								type="button"
								className="tab-new"
								aria-label="New tab"
								onClick={onNewConversation}
							>
								+
							</button>
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
										health.local.modelPath || "weights not detected",
										`Intern ${health.intern.mode}`,
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
								className="titlebar-icon-btn titlebar-chevron"
								aria-label="Account menu"
								onClick={() => showToast("Account menu — stub")}
							>
								<IconChevron />
							</button>
							<button
								type="button"
								className="titlebar-icon-btn"
								aria-label="Downloads"
								onClick={() => showToast("Downloads — stub")}
							>
								<IconCloud />
							</button>
							<button
								type="button"
								className="titlebar-icon-btn"
								aria-label={terminalOpen ? "Hide terminal" : "Show terminal"}
								title="Toggle terminal (⌘J)"
								onClick={() => setTerminalOpen((current) => !current)}
							>
								<IconLayout />
							</button>
							<button
								type="button"
								className="titlebar-icon-btn"
								aria-label="Expand"
								onClick={() => showToast("Expand — stub")}
							>
								<IconExpand />
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
							key={view.section ?? "models"}
							onBack={() => setView({ kind: "landing" })}
							onReloadLaguna={onReloadLaguna}
							health={health}
							lagunaPhase={laguna?.phase}
							initialSection={view.section}
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
									if (sessionIsLocalChat(session)) setView({ kind: "chat", chatId: sessionId });
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
										window.localStorage.setItem("synth.inventoryContainerPaneWidth", String(width));
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
							onAddProject={() => void onAddProject()}
							onSetupAgent={() => showToast("Set up agent — stub")}
						/>
					) : null}

					{view.kind === "chat" && activeChat ? (
						<div className={`workbench${openArtifact ? " with-visual" : ""}${openContainer ? " with-container" : ""}${containerPaneExpanded ? " container-expanded" : ""}`}>
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
										onStop={() => void controlActive("cancel")}
									/>
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
						</div>
					) : null}

					{view.kind === "sync" && activeSync ? (
						<CloudDesk
							kind="sync"
							session={activeSync}
							openArtifactId={openArtifactId}
							onOpenArtifact={toggleArtifact}
							onBack={() => setView({ kind: "landing" })}
							onAction={onCloudAction}
							onSendMessage={(text) => void sendToSession(activeSync.id, text)}
						/>
					) : null}

					{view.kind === "async" && state.asyncIntern && asyncSession ? (
						<CloudDesk
							kind="async"
							intern={state.asyncIntern}
							onBack={() => setView({ kind: "landing" })}
							onAction={onCloudAction}
							onSendMessage={(text) => void sendToSession(asyncSession.id, text)}
						/>
					) : null}

					{showComposer ? (
						<Composer
							state={state}
							onSend={(text) => void onComposerSend(text)}
							onSelectTarget={onSelectTarget}
							approvalMode={approvalMode}
							onSelectApprovalMode={selectApprovalMode}
							modelKnobValues={modelKnobValues}
							onSelectModelKnob={selectModelKnob}
						/>
					) : null}
			<TerminalPanel
						open={terminalOpen}
						workspaceId={terminalWorkspaceId}
						workspaceRoot={terminalWorkspaceRoot}
						onOpenChange={setTerminalOpen}
			/>
		</main>
	</div>

	{searchOpen ? (
		<ConversationSearch
			state={state}
			onClose={() => setSearchOpen(false)}
			onOpenChat={(id) => setView({ kind: "chat", chatId: id })}
			onOpenSync={(id) => setView({ kind: "sync", sessionId: id })}
			onOpenAsync={() => {
				const pinned = sessions.find((session) => sessionIsAsync(session));
				if (pinned) setView({ kind: "async", sessionId: pinned.id });
			}}
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
