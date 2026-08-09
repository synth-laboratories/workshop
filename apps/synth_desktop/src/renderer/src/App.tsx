import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { runtimeClient } from "@synth/runtime-client";
import type {
	RuntimeControlKind,
	RuntimeEvent,
	RuntimeHealth,
	Project,
	SemanticUiSnapshot,
	Session,
	VisualInstanceRecord
} from "@synth/runtime-protocol";
import { AVAILABLE_LORAS, EXECUTION_TARGETS, LORA_NONE } from "./types/landing";
import type { ArtifactRef } from "./types/landing";
import { ChatTranscript } from "./components/ChatTranscript";
import { CloudDesk } from "./components/CloudDesk";
import { Composer } from "./components/Composer";
import { DemoFixturesBar } from "./components/DemoFixturesBar";
import { InventoryPage } from "./components/InventoryPage";
import { LandingPage } from "./components/LandingPage";
import { SettingsPage } from "./components/SettingsPage";
import { Sidebar } from "./components/Sidebar";
import { SynthLogo } from "./components/SynthLogo";
import { TerminalPanel } from "./components/TerminalPanel";
import { VisualPane } from "./components/VisualPane";
import {
	buildLandingState,
	sessionIsAsync,
	sessionIsLocalChat,
	sessionIsSync,
	targetIdToExecutionTarget,
	visualRecordToArtifact
} from "./runtime/sessionView";
import { codexEventToRuntime, codexStartRequest, createCodexSession, restoreCodexSession } from "./runtime/nativeCodex";

type MainView =
	| { kind: "landing" }
	| { kind: "chat"; chatId: string }
	| { kind: "sync"; sessionId: string }
	| { kind: "async"; sessionId: string }
	| { kind: "settings" }
	| { kind: "inventory" };

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

function appendEvent(events: RuntimeEvent[], event: RuntimeEvent): RuntimeEvent[] {
	if (events.some((candidate) => candidate.sequence === event.sequence)) return events;
	return [...events, event].sort((left, right) => left.sequence - right.sequence);
}

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
	const nativeCodex = window.synthCodex;
	const [health, setHealth] = useState<RuntimeHealth | null>(null);
	const [laguna, setLaguna] = useState<{
		phase: string;
		detail?: string | null;
		loadedModel?: string | null;
		backend?: string | null;
	} | null>(null);
	const [sessions, setSessions] = useState<Session[]>([]);
	const [projects, setProjects] = useState<Project[]>([]);
	const [selectedProjectId, setSelectedProjectId] = useState<string | null>(null);
	const [eventsBySession, setEventsBySession] = useState<Record<string, RuntimeEvent[]>>({});
	const [selectedTargetId, setSelectedTargetId] = useState("local-laguna");
	const [selectedLoraId, setSelectedLoraId] = useState(LORA_NONE);
	const [downloadPaused, setDownloadPaused] = useState(false);
	const [toast, setToast] = useState<string | null>(null);
	const [view, setView] = useState<MainView>({ kind: "landing" });
	const [openArtifactId, setOpenArtifactId] = useState<string | null>(null);
	const [standaloneVisual, setStandaloneVisual] = useState<ArtifactRef | null>(null);
	const [busy, setBusy] = useState(false);
	const [demoBusy, setDemoBusy] = useState(false);
	const [bootError, setBootError] = useState<string | null>(null);
	const [terminalOpen, setTerminalOpen] = useState(false);
	const [defaultWorkspace, setDefaultWorkspace] = useState<string | null>(null);
	const eventsBySessionRef = useRef(eventsBySession);
	const nativeSequencesRef = useRef(new Map<string, number>());
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
		const next = await runtimeClient.listSessions();
		setSessions(next);
		return next;
	}, []);

	const refreshHealth = useCallback(async () => {
		const next = await runtimeClient.health();
		setHealth(next);
		return next;
	}, []);

	const refreshProjects = useCallback(async () => {
		const next = await runtimeClient.listProjects();
		setProjects(next);
		return next;
	}, []);

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
			? nativeCodex.list().then((persisted) => setSessions(
				persisted.filter((session) => session.status !== "closed").map(restoreCodexSession)
			))
			: Promise.all([refreshHealth(), refreshSessions(), refreshProjects()]);
		boot
			.then(() => {
				if (!disposed) setBootError(null);
			})
			.catch((reason: unknown) => {
				if (!disposed) {
					setBootError(reason instanceof Error ? reason.message : String(reason));
				}
			});
		return () => {
			disposed = true;
		};
	}, [nativeCodex, refreshHealth, refreshProjects, refreshSessions]);

	useEffect(() => {
		if (!nativeCodex) return;
		return nativeCodex.onEvent((event) => {
			const sequence = allocateNativeSequence(event.sessionId);
			const runtimeEvent = codexEventToRuntime(event, sequence);
			setEventsBySession((current) => ({
				...current,
				[event.sessionId]: appendEvent(current[event.sessionId] ?? [], runtimeEvent)
			}));
			setSessions((current) => current.map((session) => session.id === event.sessionId
				? { ...session, updatedAt: runtimeEvent.createdAt, latestCursor: sequence,
					status: runtimeEvent.eventKind === "run.completed" ? "ready"
						: runtimeEvent.eventKind === "run.failed" ? "failed"
						: runtimeEvent.eventKind === "run.cancelled" ? "cancelled"
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
			if (nativeCodex && view.kind !== "inventory" && !sessions.some((s) => s.target.kind === "intern")) return;
			void refreshSessions().catch(() => undefined);
			void refreshProjects().catch(() => undefined);
			void refreshHealth().catch(() => undefined);
			void window.synthLaguna?.getStatus().then(setLaguna).catch(() => undefined);
		}, 2_500);
		return () => window.clearInterval(interval);
	}, [nativeCodex, refreshHealth, refreshProjects, refreshSessions, sessions, view.kind]);

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
				const page = await runtimeClient.events(sessionId, 0, 500);
				if (disposed) return;
				setEventsBySession((current) => ({
					...current,
					[sessionId]: page.events
				}));
				subscription = await runtimeClient.subscribe(
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
							event.eventKind === "command.receipt"
						) {
							void refreshSessions().catch(() => undefined);
						}
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
	}, [activeSessionId, refreshSessions, sessions, showToast]);

	const state = useMemo(() => {
		const base = buildLandingState({
			health,
			sessions,
			eventsBySession,
			selectedTargetId,
			selectedLoraId,
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
		health,
		laguna,
		selectedLoraId,
		selectedTargetId,
		sessions,
		projects
	]);

	const activeChat =
		view.kind === "chat" ? (state.chats.find((c) => c.id === view.chatId) ?? null) : null;
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
	}, [viewKey]);

	useEffect(() => {
		if (!openArtifactId) return;
		const onKey = (e: KeyboardEvent) => {
			if (e.key === "Escape") {
				setOpenArtifactId(null);
				setStandaloneVisual(null);
			}
		};
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [openArtifactId]);

	const toggleArtifact = useCallback((id: string | null) => {
		if (id == null) {
			setOpenArtifactId(null);
			setStandaloneVisual(null);
			return;
		}
		setStandaloneVisual(null);
		setOpenArtifactId((current) => (current === id ? null : id));
	}, []);

	const openVisualRecord = useCallback((visual: VisualInstanceRecord) => {
		const artifact = visualRecordToArtifact(visual);
		setStandaloneVisual(artifact);
		setOpenArtifactId(artifact.id);
		setView({ kind: "inventory" });
	}, []);

	const createConversation = useCallback(
		async (targetId: string = selectedTargetId, title?: string) => {
			setBusy(true);
			try {
				const target = targetIdToExecutionTarget(targetId, selectedLoraId);
				if (nativeCodex && target.kind !== "intern") {
					const id = crypto.randomUUID();
					const project = projects.find((candidate) => candidate.id === selectedProjectId);
					const workspace = project?.path ?? await nativeCodex.defaultWorkspace();
					await nativeCodex.start(codexStartRequest(id, workspace, target));
					const session = createCodexSession(id, target, selectedProjectId, title);
					setSessions((current) => [session, ...current]);
					setView({ kind: "chat", chatId: session.id });
					return session;
				}
				const session = await runtimeClient.createSession(target, title, selectedProjectId);
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
		[nativeCodex, projects, refreshSessions, selectedLoraId, selectedProjectId, selectedTargetId, showToast]
	);

	const onAddProject = useCallback(async () => {
		try {
			const path = await window.synthDesktop.chooseProjectDirectory();
			if (!path) return;
			if (nativeCodex) {
				const now = new Date().toISOString();
				const project = { id: crypto.randomUUID(), name: path.split("/").filter(Boolean).at(-1) ?? path,
					path, vcs: null, metadata: {}, createdAt: now, updatedAt: now };
				setProjects((current) => [...current, project]);
				setSelectedProjectId(project.id);
				showToast(`Project ready · ${project.name}`);
				return;
			}
			const project = await runtimeClient.createProject({ path });
			setSelectedProjectId(project.id);
			await refreshProjects();
			showToast(`Project ready · ${project.name}`);
		} catch (reason) {
			showToast(reason instanceof Error ? reason.message : String(reason));
		}
	}, [nativeCodex, refreshProjects, showToast]);

	const ensureActiveSession = useCallback(async (): Promise<string | null> => {
		if (activeSessionId) return activeSessionId;
		if (view.kind !== "landing") return null;
		const session = await createConversation(selectedTargetId);
		return session.id;
	}, [activeSessionId, createConversation, selectedTargetId, view.kind]);

	const sendToSession = useCallback(
		async (sessionId: string, text: string) => {
			setBusy(true);
			try {
				const session = sessions.find((candidate) => candidate.id === sessionId);
				if (nativeCodex && session?.metadata.runtime === "codex-app-server") {
					const project = projects.find((candidate) => candidate.id === session.projectId);
					const workspace = typeof session.metadata.workspace === "string"
						? session.metadata.workspace
						: project?.path;
					if (!workspace) throw new Error("Choose a project before starting this Codex session");
					await nativeCodex.start({
						...codexStartRequest(sessionId, workspace, session.target),
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
					await nativeCodex.startTurn(sessionId, text);
					return;
				}
				await runtimeClient.sendMessage(sessionId, text);
				await refreshSessions();
			} catch (reason) {
				showToast(reason instanceof Error ? reason.message : String(reason));
			} finally {
				setBusy(false);
			}
		},
		[allocateNativeSequence, nativeCodex, projects, refreshSessions, sessions, showToast]
	);

	const onComposerSend = useCallback(
		async (text: string) => {
			try {
				const sessionId = await ensureActiveSession();
				if (!sessionId) {
					showToast("No active session");
					return;
				}
				await sendToSession(sessionId, text);
			} catch {
				/* toast already shown */
			}
		},
		[ensureActiveSession, sendToSession, showToast]
	);

	const controlActive = useCallback(
		async (kind: RuntimeControlKind) => {
			if (!activeSessionId) return;
			setBusy(true);
			try {
				const session = sessions.find((candidate) => candidate.id === activeSessionId);
				if (nativeCodex && session?.metadata.runtime === "codex-app-server") {
					if (kind === "close") await nativeCodex.close(activeSessionId);
					else if (kind === "cancel" || kind === "pause") await nativeCodex.interrupt(activeSessionId);
					else throw new Error(`${kind} is not supported for a Codex session`);
					setSessions((current) => current.map((item) => item.id === activeSessionId
						? { ...item, status: kind === "close" ? "completed" : "ready", updatedAt: new Date().toISOString() }
						: item));
					return;
				}
				await runtimeClient.control(activeSessionId, kind);
				await refreshSessions();
			} catch (reason) {
				showToast(reason instanceof Error ? reason.message : String(reason));
			} finally {
				setBusy(false);
			}
		},
		[activeSessionId, nativeCodex, refreshSessions, sessions, showToast]
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
			const result = await runtimeClient.simulateLive("eval");
			showToast(`Created visual · ${result.visual.title}`);
			openVisualRecord(result.visual);
			await refreshHealth();
		} catch (reason) {
			showToast(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setDemoBusy(false);
		}
	}, [openVisualRecord, refreshHealth, showToast]);

	const onSelectLora = useCallback((id: string) => {
		setSelectedLoraId(id);
		const lora = AVAILABLE_LORAS.find((l) => l.id === id);
		if (lora) setSelectedTargetId(lora.baseTargetId);
		else if (id === LORA_NONE) setSelectedTargetId("local-laguna");
	}, []);

	const onNewConversation = useCallback(() => {
		setView({ kind: "landing" });
		setOpenArtifactId(null);
		setStandaloneVisual(null);
	}, []);

	const onNewSyncSession = useCallback(() => {
		void createConversation("intern-sync", "Live Intern");
	}, [createConversation]);

	const tabLabel =
		view.kind === "settings"
			? "Settings"
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
					return createConversation(target);
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
					const visual = await runtimeClient.getVisual(visualId);
					openVisualRecord(visual);
					return visual;
				}
				if (action === "list_inventory") {
					const [containers, traces, visuals] = await Promise.all([
						runtimeClient.listContainers(),
						runtimeClient.listTraces(),
						runtimeClient.listVisuals()
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
					activeChatId={view.kind === "chat" ? view.chatId : null}
					activeSyncId={view.kind === "sync" ? view.sessionId : null}
					asyncActive={view.kind === "async"}
					inventoryActive={view.kind === "inventory"}
					onNewConversation={onNewConversation}
					onNewSyncSession={onNewSyncSession}
					onOpenChat={(id) => setView({ kind: "chat", chatId: id })}
					onOpenSyncSession={(id) => setView({ kind: "sync", sessionId: id })}
					onOpenAsync={() => {
						const pinned = sessions.find(sessionIsAsync);
						if (!pinned || !state.asyncIntern) {
							void createConversation("intern-async", "Background Intern");
							return;
						}
						setView({ kind: "async", sessionId: pinned.id });
					}}
					onOpenInventory={() => setView({ kind: "inventory" })}
					onSettings={() => setView({ kind: "settings" })}
					 onPauseToggle={() => setDownloadPaused((v) => !v)}
					onAddProject={onAddProject}
					onSelectProject={setSelectedProjectId}
					selectedProjectId={selectedProjectId}
				/>

				<main className="main-pane">
					<header className="titlebar" data-testid="titlebar">
						<div className="titlebar-tabs">
							<div className="tab tab-active" role="tab" aria-selected>
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
							{health ? (
								<span
									className="runtime-pill"
									data-testid="runtime-status"
									title={[
										health.runtimeId,
										`Laguna ${health.local.mode}`,
										laguna?.phase ? `sidecar ${laguna.phase}` : null,
										laguna?.backend ? `backend ${laguna.backend}` : null,
										health.local.modelPath || "weights not detected",
										`Intern ${health.intern.mode}`,
										`OpenRouter ${health.openrouter.mode}`
									]
										.filter(Boolean)
										.join(" · ")}
								>
									{laguna?.phase === "ready" || health.local.mode === "mlx"
										? "Laguna·MLX"
										: laguna?.phase === "loading" || laguna?.phase === "starting"
											? "Laguna·starting"
											: "Laguna·offline"}
									{health.openrouter.mode === "ready" ? " · OR" : ""}
									{health.intern.mode === "remote"
										? " · Intern"
										: health.intern.mode === "demo"
											? " · Intern·demo"
											: " · Intern·setup"}
									{health.inventory
										? ` · ${health.inventory.containers}/${health.inventory.traces}/${health.inventory.visuals}`
										: ""}
								</span>
							) : (
								<span className="runtime-pill is-connecting" data-testid="runtime-status">
									connecting…
								</span>
							)}
							<button
								type="button"
								className="avatar-btn"
								aria-label="Account"
								onClick={() => showToast("Account — stub")}
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
							selectedLoraId={selectedLoraId}
							onSelectLora={(id) => {
								onSelectLora(id);
								const name =
									id === LORA_NONE
										? "Base Laguna"
										: (AVAILABLE_LORAS.find((l) => l.id === id)?.displayName ?? id);
								showToast(`Active adapter · ${name}`);
							}}
							onBack={() => setView({ kind: "landing" })}
							onAction={(label) => showToast(`${label} — stub`)}
							health={health}
							lagunaPhase={laguna?.phase}
						/>
					) : null}

					{view.kind === "inventory" ? (
						<div className={`inventory-workbench${openArtifact ? " with-visual" : ""}`}>
							<InventoryPage
								onOpenVisual={openVisualRecord}
								onBack={() => setView({ kind: "landing" })}
							/>
							{openArtifact ? (
								<VisualPane
									artifact={openArtifact}
									onClose={() => toggleArtifact(null)}
								/>
							) : null}
						</div>
					) : null}

					{view.kind === "landing" ? (
						<LandingPage
							state={state}
							selectedTargetId={selectedTargetId}
							onSelectTarget={setSelectedTargetId}
							onAddProject={() => void onAddProject()}
							onSetupAgent={() => showToast("Set up agent — stub")}
						/>
					) : null}

					{view.kind === "chat" && activeChat ? (
						<div className={`workbench${openArtifact ? " with-visual" : ""}`}>
								<ChatTranscript
									chat={activeChat}
									openArtifactId={openArtifactId}
									onOpenArtifact={toggleArtifact}
									onApprove={() => void controlActive("approve")}
									onReject={() => void controlActive("reject")}
								/>
							{openArtifact ? (
								<VisualPane artifact={openArtifact} onClose={() => toggleArtifact(null)} />
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
							onSelectTarget={setSelectedTargetId}
							onSelectLora={onSelectLora}
							onOpenFinetunes={() => setView({ kind: "settings" })}
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

			{toast ? (
				<div className="toast" role="status" key={toast}>
					{toast}
				</div>
			) : null}
		</div>
	);
}
