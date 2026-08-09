import { useCallback, useEffect, useMemo, useState } from "react";
import { LANDING_SCENARIOS } from "./fixtures/landingScenarios";
import type { LandingScenarioId } from "./types/landing";
import { AVAILABLE_LORAS, EXECUTION_TARGETS, LORA_NONE } from "./types/landing";
import { ChatTranscript } from "./components/ChatTranscript";
import { CloudDesk } from "./components/CloudDesk";
import { Composer } from "./components/Composer";
import { LandingPage } from "./components/LandingPage";
import { ScenarioPicker } from "./components/ScenarioPicker";
import { SettingsPage } from "./components/SettingsPage";
import { Sidebar } from "./components/Sidebar";
import { SynthLogo } from "./components/SynthLogo";
import { VisualPane } from "./components/VisualPane";

type MainView =
	| { kind: "landing" }
	| { kind: "chat"; chatId: string }
	| { kind: "sync"; sessionId: string }
	| { kind: "async" }
	| { kind: "settings" };

function truncate(label: string, max = 22) {
	if (label.length <= max) return label;
	return `${label.slice(0, max - 1)}…`;
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
	const [scenarioId, setScenarioId] = useState<LandingScenarioId>("landing-downloading");
	const [selectedTargetId, setSelectedTargetId] = useState(
		LANDING_SCENARIOS["landing-downloading"].selectedTargetId
	);
	const [selectedLoraId, setSelectedLoraId] = useState(
		LANDING_SCENARIOS["landing-downloading"].selectedLoraId ?? LORA_NONE
	);
	const [downloadPaused, setDownloadPaused] = useState(false);
	const [toast, setToast] = useState<string | null>(null);
	const [view, setView] = useState<MainView>({ kind: "landing" });
	const [openArtifactId, setOpenArtifactId] = useState<string | null>(null);

	const toggleArtifact = useCallback((id: string | null) => {
		if (id == null) {
			setOpenArtifactId(null);
			return;
		}
		setOpenArtifactId((current) => (current === id ? null : id));
	}, []);

	const baseState = LANDING_SCENARIOS[scenarioId];

	const state = useMemo(() => {
		const withTarget = { ...baseState, selectedTargetId, selectedLoraId };
		if (withTarget.model.status !== "downloading") return withTarget;
		return {
			...withTarget,
			model: {
				...withTarget.model,
				downloadPaused
			}
		};
	}, [baseState, downloadPaused, selectedTargetId, selectedLoraId]);

	const activeChat =
		view.kind === "chat" ? (state.chats.find((c) => c.id === view.chatId) ?? null) : null;
	const activeSync =
		view.kind === "sync"
			? (state.syncSessions.find((s) => s.id === view.sessionId) ?? null)
			: null;

	const openArtifact =
		view.kind === "chat" && activeChat
			? (activeChat.artifacts?.find((a) => a.id === openArtifactId) ?? null)
			: view.kind === "sync" && activeSync
				? (activeSync.artifacts?.find((a) => a.id === openArtifactId) ?? null)
				: null;

	const viewKey =
		view.kind === "chat"
			? `chat:${view.chatId}`
			: view.kind === "sync"
				? `sync:${view.sessionId}`
				: view.kind;

	useEffect(() => {
		// Reset when switching chats/sessions — click-to-reveal from there.
		setOpenArtifactId(null);
	}, [viewKey]);

	useEffect(() => {
		if (!openArtifactId) return;
		const onKey = (e: KeyboardEvent) => {
			if (e.key === "Escape") setOpenArtifactId(null);
		};
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [openArtifactId]);

	const showToast = useCallback((message: string) => {
		setToast(message);
		window.setTimeout(() => setToast(null), 2200);
	}, []);

	const onScenarioChange = (id: LandingScenarioId) => {
		setScenarioId(id);
		setSelectedTargetId(LANDING_SCENARIOS[id].selectedTargetId);
		setSelectedLoraId(LANDING_SCENARIOS[id].selectedLoraId ?? LORA_NONE);
		setDownloadPaused(false);
		setView({ kind: "landing" });
		setOpenArtifactId(null);
	};

	const tabLabel =
		view.kind === "settings"
			? "Settings"
			: view.kind === "async"
				? "Intern · Background"
				: view.kind === "sync"
					? (activeSync?.title ?? "Intern · Live")
					: view.kind === "chat"
						? (activeChat?.title ?? "Chat")
						: (EXECUTION_TARGETS.find((t) => t.id === selectedTargetId)?.label ?? "Synth");

	const showComposer = view.kind === "landing" || view.kind === "chat";

	const onSelectLora = useCallback(
		(id: string) => {
			setSelectedLoraId(id);
			const lora = AVAILABLE_LORAS.find((l) => l.id === id);
			if (lora) setSelectedTargetId(lora.baseTargetId);
			else if (id === LORA_NONE) setSelectedTargetId("local-laguna");
		},
		[]
	);

	return (
		<div className="app-shell">
			{import.meta.env.DEV ? (
				<ScenarioPicker scenarioId={scenarioId} onChange={onScenarioChange} />
			) : null}

			<div className="body-row">
				<Sidebar
					state={state}
					activeChatId={view.kind === "chat" ? view.chatId : null}
					activeSyncId={view.kind === "sync" ? view.sessionId : null}
					asyncActive={view.kind === "async"}
					onNewConversation={() => {
						setView({ kind: "landing" });
						showToast("New local chat — coming in M1");
					}}
					onNewSyncSession={() => showToast("New Sync session — stub")}
					onOpenChat={(id) => setView({ kind: "chat", chatId: id })}
					onOpenSyncSession={(id) => setView({ kind: "sync", sessionId: id })}
					onOpenAsync={() => {
						if (!state.asyncIntern) {
							showToast("Async Intern not provisioned");
							return;
						}
						setView({ kind: "async" });
					}}
					onSettings={() => setView({ kind: "settings" })}
					onPauseToggle={() => setDownloadPaused((v) => !v)}
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
								onClick={() => showToast("New tab — coming in M1")}
							>
								+
							</button>
						</div>
						<div className="titlebar-actions">
							<span className="mock-badge" data-testid="mock-badge" title="Fixture UX pin-down — not the real app">
								MOCK
							</span>
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
								aria-label="Layout"
								onClick={() => showToast("Layout — stub")}
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
						/>
					) : null}

					{view.kind === "landing" ? (
						<LandingPage
							state={state}
							selectedTargetId={selectedTargetId}
							onSelectTarget={setSelectedTargetId}
							onAddProject={() => showToast("Add project — stub")}
							onSetupAgent={() => showToast("Set up agent — stub")}
						/>
					) : null}

					{view.kind === "chat" && activeChat ? (
						<div className={`workbench${openArtifact ? " with-visual" : ""}`}>
							<ChatTranscript
								chat={activeChat}
								openArtifactId={openArtifactId}
								onOpenArtifact={toggleArtifact}
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
							onAction={(label) => showToast(`${label} — stub`)}
						/>
					) : null}

					{view.kind === "async" && state.asyncIntern ? (
						<CloudDesk
							kind="async"
							intern={state.asyncIntern}
							onBack={() => setView({ kind: "landing" })}
							onAction={(label) => showToast(`${label} — stub`)}
						/>
					) : null}

					{showComposer ? (
						<Composer
							state={state}
							onSend={() => showToast("Send — active chat coming in M1")}
							onSelectTarget={setSelectedTargetId}
							onSelectLora={onSelectLora}
							onOpenFinetunes={() => setView({ kind: "settings" })}
						/>
					) : null}
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
