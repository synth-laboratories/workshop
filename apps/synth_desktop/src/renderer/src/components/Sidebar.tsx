import { useState } from "react";
import {
	ASYNC_PHASE_LABEL,
	SYNC_STATUS_LABEL,
	type LandingState
} from "../types/landing";
import { ModelDownloadBar } from "./ModelDownloadBar";
import { LocalModelResidency } from "./LocalModelResidency";
import type { LagunaStatus } from "../env";

type Props = {
	state: LandingState;
	lagunaStatus?: LagunaStatus | null;
	activeChatId?: string | null;
	activeSyncId?: string | null;
	asyncActive?: boolean;
	inventoryActive?: boolean;
	visualsActive?: boolean;
	connectorsActive?: boolean;
	workingChatIds?: ReadonlySet<string>;
	unreadChatIds?: ReadonlySet<string>;
	onNewConversation: () => void;
	onNewSyncSession: () => void;
	onOpenChat: (id: string) => void;
	onOpenSyncSession: (id: string) => void;
	onOpenAsync: () => void;
	onOpenInventory: () => void;
	onOpenVisuals: () => void;
	onOpenConnectors: () => void;
	onSearch: () => void;
	onSettings: () => void;
	onPauseToggle: () => void;
	onAddProject: () => void;
	onSelectProject: (id: string) => void;
	selectedProjectId?: string | null;
};

function IconPlusSquare({ className = "nav-icon" }: { className?: string }) {
	return (
		<svg className={className} viewBox="0 0 16 16" fill="none" aria-hidden>
			<rect x="2.5" y="2.5" width="11" height="11" rx="2" stroke="currentColor" strokeWidth="1.3" />
			<path d="M8 5.5v5M5.5 8h5" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
		</svg>
	);
}

function IconConnectors() {
	return (
		<svg className="nav-icon" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path
				d="M8 1.5l1.6 1.6L8 4.7 6.4 3.1 8 1.5zM3.2 6.3l1.6 1.6-1.6 1.6L1.6 7.9 3.2 6.3zM12.8 6.3l1.6 1.6-1.6 1.6-1.6-1.6 1.6-1.6zM8 11.1l1.6 1.6L8 14.3l-1.6-1.6L8 11.1z"
				stroke="currentColor"
				strokeWidth="1.15"
				strokeLinejoin="round"
			/>
		</svg>
	);
}

function IconSearch() {
	return (
		<svg className="nav-icon" viewBox="0 0 16 16" fill="none" aria-hidden>
			<circle cx="7" cy="7" r="4" stroke="currentColor" strokeWidth="1.35" />
			<path d="M10.2 10.2L13.5 13.5" stroke="currentColor" strokeWidth="1.35" strokeLinecap="round" />
		</svg>
	);
}

function IconFolderSmall() {
	return (
		<svg className="item-icon" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path d="M2 4.5h4l1.2 1.4H14v6.5a1 1 0 01-1 1H3a1 1 0 01-1-1V4.5z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" />
		</svg>
	);
}

function IconGlobe() {
	return (
		<svg className="item-icon" viewBox="0 0 16 16" fill="none" aria-hidden>
			<circle cx="8" cy="8" r="5.5" stroke="currentColor" strokeWidth="1.25" />
			<path
				d="M2.5 8h11M8 2.5c1.8 1.8 2.7 3.6 2.7 5.5S9.8 11.7 8 13.5C6.2 11.7 5.3 9.9 5.3 8S6.2 4.3 8 2.5z"
				stroke="currentColor"
				strokeWidth="1.15"
			/>
		</svg>
	);
}

function IconCloud() {
	return (
		<svg className="item-icon" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path
				d="M4.8 12h6.4a2.4 2.4 0 00.15-4.8 3.2 3.2 0 00-6.1-1A2.2 2.2 0 004.8 12z"
				stroke="currentColor"
				strokeWidth="1.2"
				strokeLinejoin="round"
			/>
		</svg>
	);
}

function IconBolt() {
	return (
		<svg className="item-icon" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path
				d="M9 2.5L4.5 9h3.2L7 13.5 12.5 7H9.2L9 2.5z"
				stroke="currentColor"
				strokeWidth="1.2"
				strokeLinejoin="round"
			/>
		</svg>
	);
}

function IconSettings() {
	return (
		<svg className="nav-icon" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path
				d="M6.4 1.8h3.2l.35 1.4a4.8 4.8 0 011.15.66l1.4-.45 1.6 2.77-1.1 1.05c.08.34.12.69.12 1.05s-.04.71-.12 1.05l1.1 1.05-1.6 2.77-1.4-.45a4.8 4.8 0 01-1.15.66l-.35 1.4H6.4l-.35-1.4a4.8 4.8 0 01-1.15-.66l-1.4.45L2 10.33l1.1-1.05A4.9 4.9 0 012.98 8c0-.36.04-.71.12-1.05L2 5.9l1.6-2.77 1.4.45c.34-.27.73-.5 1.15-.66l.35-1.4z"
				stroke="currentColor"
				strokeWidth="1.15"
				strokeLinejoin="round"
			/>
			<circle cx="8" cy="8" r="2" stroke="currentColor" strokeWidth="1.2" />
		</svg>
	);
}

function SectionChevron({ open }: { open: boolean }) {
	return (
		<svg
			className={`section-chevron${open ? "" : " collapsed"}`}
			viewBox="0 0 10 10"
			fill="none"
			aria-hidden
		>
			<path
				d="M2 3.5L5 6.5L8 3.5"
				stroke="currentColor"
				strokeWidth="1.3"
				strokeLinecap="round"
				strokeLinejoin="round"
			/>
		</svg>
	);
}

function IconInventory() {
	return (
		<svg className="item-icon" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path
				d="M2.5 4.5h11v8.2a1.3 1.3 0 01-1.3 1.3H3.8a1.3 1.3 0 01-1.3-1.3V4.5z"
				stroke="currentColor"
				strokeWidth="1.25"
			/>
			<path d="M5 4.5V3.4A1.4 1.4 0 016.4 2h3.2A1.4 1.4 0 0111 3.4v1.1" stroke="currentColor" strokeWidth="1.25" />
			<path d="M2.5 7h11" stroke="currentColor" strokeWidth="1.2" />
		</svg>
	);
}

export function Sidebar({
	state,
	lagunaStatus = null,
	activeChatId = null,
	activeSyncId = null,
	asyncActive = false,
	inventoryActive = false,
	visualsActive = false,
	connectorsActive = false,
	workingChatIds = new Set<string>(),
	unreadChatIds = new Set<string>(),
	onNewConversation,
	onNewSyncSession,
	onOpenChat,
	onOpenSyncSession,
	onOpenAsync,
	onOpenInventory,
	onOpenVisuals,
	onOpenConnectors,
	onSearch,
	onSettings,
	onPauseToggle,
	onAddProject,
	onSelectProject,
	selectedProjectId = null
}: Props) {
	const [chatsOpen, setChatsOpen] = useState(true);
	const [cloudOpen, setCloudOpen] = useState(true);
	const [inventoryOpen, setInventoryOpen] = useState(true);
	const [researchOpen, setResearchOpen] = useState(true);

	return (
		<aside className="sidebar" data-testid="sidebar">
			<div className="sidebar-drag-strip" data-tauri-drag-region="" aria-hidden />

			<nav className="sidebar-nav" aria-label="Primary">
				<button
					type="button"
					className="nav-item"
					onClick={onNewConversation}
					data-testid="new-conversation"
				>
					<IconPlusSquare />
					New conversation
				</button>
				<button type="button" className={`nav-item${connectorsActive ? " active" : ""}`} onClick={onOpenConnectors} data-testid="open-connectors">
					<IconConnectors />
					Connectors
				</button>
				<button type="button" className="nav-item" onClick={onSearch} data-testid="open-search" title="Search conversations (⌘K)">
					<IconSearch />
					Search
				</button>
			</nav>

			<div className="sidebar-scroll">
				{/* ── Chats = local Laguna ── */}
				<div className="sidebar-section">
					<div className="section-header">
						<button
							type="button"
							className="section-header-label"
							onClick={() => setChatsOpen((v) => !v)}
							aria-expanded={chatsOpen}
						>
							Chats
							<SectionChevron open={chatsOpen} />
						</button>
						<button
							type="button"
							className="section-action"
							aria-label="New local chat"
							onClick={onNewConversation}
						>
							<IconPlusSquare className="section-action-icon" />
						</button>
					</div>
					{chatsOpen ? (
						<div className="section-list" data-testid="chat-list">
							{state.chats.length === 0 ? (
								<p className="empty-hint">No local chats yet</p>
							) : (
								state.chats.map((chat) => (
									<button
										key={chat.id}
										type="button"
										className={`chat-item${activeChatId === chat.id ? " active" : ""}`}
										onClick={() => onOpenChat(chat.id)}
										data-testid={`local-chat-${chat.id}`}
									>
										<IconGlobe />
										<span className="item-label">{chat.title}</span>
										{workingChatIds.has(chat.id) ? (
											<span className="chat-working-indicator" aria-label="Working" title="Working" data-testid={`chat-working-${chat.id}`} />
										) : unreadChatIds.has(chat.id) ? (
											<span className="chat-unread-indicator" aria-label="Finished, unviewed" title="Finished, unviewed" data-testid={`chat-unread-${chat.id}`} />
										) : null}
									</button>
								))
							)}
						</div>
					) : null}
				</div>

				<div className="sidebar-section">
					<div className="section-header">
						<span className="section-header-label">Projects</span>
						<button
							type="button"
							className="section-action"
							aria-label="Add project"
							onClick={onAddProject}
							data-testid="add-project"
						>
							<IconPlusSquare className="section-action-icon" />
						</button>
					</div>
					<div className="section-list" data-testid="project-list">
						{state.projects.length === 0 ? (
							<button type="button" className="add-project-card" onClick={onAddProject}>
								<IconPlusSquare className="item-icon" />
								<span>Add a project</span>
							</button>
						) : (
							state.projects.map((project) => (
								<button
									key={project.id}
									type="button"
									className={`project-item${selectedProjectId === project.id ? " active" : ""}`}
									onClick={() => onSelectProject(project.id)}
									data-testid={`project-${project.id}`}
								>
									<IconFolderSmall />
									<span className="item-label">{project.name}</span>
								</button>
							))
						)}
					</div>
				</div>

				{/* ── Cloud = Intern sync sessions + pinned async ── */}
				<div className="sidebar-section">
					<div className="section-header">
						<button
							type="button"
							className="section-header-label"
							onClick={() => setCloudOpen((v) => !v)}
							aria-expanded={cloudOpen}
						>
							Cloud
							<SectionChevron open={cloudOpen} />
						</button>
						<button
							type="button"
							className="section-action"
							aria-label="New sync session"
							onClick={onNewSyncSession}
							data-testid="new-sync-session"
						>
							<IconPlusSquare className="section-action-icon" />
						</button>
					</div>
					{cloudOpen ? (
						<div className="section-list" data-testid="cloud-list">
							<p className="cloud-sublabel">Sync sessions</p>
							{state.syncSessions.length === 0 ? (
								<p className="empty-hint">No live sessions</p>
							) : (
								state.syncSessions.map((session) => (
									<button
										key={session.id}
										type="button"
										className={`chat-item cloud-item${activeSyncId === session.id ? " active" : ""}`}
										onClick={() => onOpenSyncSession(session.id)}
										data-testid={`sync-session-${session.id}`}
									>
										<IconCloud />
										<span className="item-label">{session.title}</span>
										<span className={`status-chip status-${session.status}`}>
											{SYNC_STATUS_LABEL[session.status]}
										</span>
									</button>
								))
							)}

							{/* Pinned Async Intern — not a session row */}
							{state.asyncIntern ? (
								<button
									type="button"
									className={`async-pin${state.asyncIntern.needsInput ? " needs-input" : ""}${asyncActive ? " active" : ""}`}
									onClick={onOpenAsync}
									data-testid="async-intern-pin"
								>
									<span className="async-pin-top">
										<IconBolt />
										<span className="async-pin-title">Async Intern</span>
										<span className={`status-chip status-async-${state.asyncIntern.phase}`}>
											{ASYNC_PHASE_LABEL[state.asyncIntern.phase]}
										</span>
									</span>
									<span className="async-pin-summary">{state.asyncIntern.summary}</span>
								</button>
							) : (
								<p className="empty-hint">Async Intern not provisioned</p>
							)}
						</div>
					) : null}
				</div>

				{/* ── Research = Visuals + Inventory ── */}
				<div className="sidebar-section">
					<div className="section-header">
						<button
							type="button"
							className="section-header-label"
							onClick={() => setResearchOpen((v) => !v)}
							aria-expanded={researchOpen}
						>
							Research
							<SectionChevron open={researchOpen} />
						</button>
					</div>
					{researchOpen ? (
						<div className="section-list" data-testid="research-nav">
							<button
								type="button"
								className={`chat-item${visualsActive ? " active" : ""}`}
								onClick={onOpenVisuals}
								data-testid="open-visuals"
							>
								<IconSearch />
								<span className="item-label">Visuals</span>
							</button>
						</div>
					) : null}
				</div>

				{/* ── Inventory = containers / traces / usage ── */}
				<div className="sidebar-section">
					<div className="section-header">
						<button
							type="button"
							className="section-header-label"
							onClick={() => setInventoryOpen((v) => !v)}
							aria-expanded={inventoryOpen}
						>
							Inventory
							<SectionChevron open={inventoryOpen} />
						</button>
					</div>
					{inventoryOpen ? (
						<div className="section-list" data-testid="inventory-nav">
							<button
								type="button"
								className={`chat-item${inventoryActive ? " active" : ""}`}
								onClick={onOpenInventory}
								data-testid="open-inventory"
							>
								<IconInventory />
								<span className="item-label">Containers · Traces · Usage</span>
							</button>
						</div>
					) : null}
				</div>
			</div>

			<div className="sidebar-footer">
				<LocalModelResidency status={lagunaStatus} />
				<ModelDownloadBar state={state} onPauseToggle={onPauseToggle} />
				<button
					type="button"
					className="settings-btn"
					onClick={onSettings}
					data-testid="settings"
					aria-label="Settings"
				>
					<IconSettings />
					Settings
				</button>
			</div>
		</aside>
	);
}
