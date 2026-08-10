import { useEffect, useMemo, useRef, useState } from "react";
import {
	ASYNC_PHASE_LABEL,
	SYNC_STATUS_LABEL,
	type LandingState
} from "../types/landing";
import { ModelDownloadBar } from "./ModelDownloadBar";
import { LocalModelResidency } from "./LocalModelResidency";
import type { LagunaStatus } from "../env";
import { ConversationContextMenu } from "./GeneralPreferencesSettings";
import { PaneResizeHandle } from "./PaneResizeHandle";

type Props = {
	state: LandingState;
	lagunaStatus?: LagunaStatus | null;
	activeChatId?: string | null;
	activeSyncId?: string | null;
	asyncActive?: boolean;
	inventoryActive?: boolean;
	visualsActive?: boolean;
	optimizersActive?: boolean;
	connectorsActive?: boolean;
	workingChatIds?: ReadonlySet<string>;
	unreadChatIds?: ReadonlySet<string>;
	pinnedChatIds?: ReadonlySet<string>;
	conversationTitles?: Record<string, string>;
	onNewConversation: () => void;
	onNewSyncSession: () => void;
	onOpenChat: (id: string) => void;
	onOpenSyncSession: (id: string) => void;
	onOpenAsync: () => void;
	onOpenInventory: () => void;
	onOpenVisuals: () => void;
	onOpenOptimizers: () => void;
	onOpenConnectors: () => void;
	onSearch: () => void;
	onSettings: () => void;
	accountSignedIn?: boolean;
	accountDisplayName?: string | null;
	accountPlan?: {
		name: string;
		monthlyAllowanceUsd: number;
		usedUsd: number;
		remainingUsd: number;
		resetsAt: string;
	} | null;
	accountUsage?: {
		weeklyTokens: number;
		weeklyCostUsd: number;
		totalTokens: number;
		totalCostUsd: number;
		entries: number;
	} | null;
	onOpenAccount?: () => void;
	onSignOut?: () => void | Promise<void>;
	onPauseToggle: () => void;
	onRenameChat?: (id: string, title: string) => void;
	onPinChat?: (id: string, pinned: boolean) => void;
	onArchiveChat?: (id: string, archived: boolean) => void;
	sidebarWidth?: number;
	onSidebarWidthChange?: (width: number) => void;
	sidebarVisible?: boolean;
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
	optimizersActive = false,
	connectorsActive = false,
	workingChatIds = new Set<string>(),
	unreadChatIds = new Set<string>(),
	pinnedChatIds = new Set<string>(),
	conversationTitles = {},
	onNewConversation,
	onNewSyncSession,
	onOpenChat,
	onOpenSyncSession,
	onOpenAsync,
	onOpenInventory,
	onOpenVisuals,
	onOpenOptimizers,
	onOpenConnectors,
	onSearch,
	onSettings,
	accountSignedIn = false,
	accountDisplayName = null,
	accountPlan = null,
	accountUsage = null,
	onOpenAccount,
	onSignOut,
	onPauseToggle,
	onRenameChat,
	onPinChat,
	onArchiveChat,
	sidebarWidth = 260,
	onSidebarWidthChange,
	sidebarVisible = true
}: Props) {
	const [chatsOpen, setChatsOpen] = useState(true);
	const [cloudOpen, setCloudOpen] = useState(true);
	const [inventoryOpen, setInventoryOpen] = useState(true);
	const [researchOpen, setResearchOpen] = useState(true);
	const [menu, setMenu] = useState<{ id: string; x: number; y: number; invoker: HTMLButtonElement } | null>(null);
	const [renamingId, setRenamingId] = useState<string | null>(null);
	const [renameDraft, setRenameDraft] = useState("");
	const [showAllChats, setShowAllChats] = useState(false);
	const [accountMenuOpen, setAccountMenuOpen] = useState(false);
	const [usageOpen, setUsageOpen] = useState(false);
	const accountMenuRef = useRef<HTMLDivElement>(null);
	const accountTriggerRef = useRef<HTMLButtonElement>(null);

	useEffect(() => {
		if (!accountMenuOpen) return;
		const closeOutside = (event: MouseEvent) => {
			if (!accountMenuRef.current?.contains(event.target as Node)) setAccountMenuOpen(false);
		};
		const closeOnEscape = (event: KeyboardEvent) => {
			if (event.key !== "Escape") return;
			setAccountMenuOpen(false);
			requestAnimationFrame(() => accountTriggerRef.current?.focus());
		};
		document.addEventListener("mousedown", closeOutside);
		document.addEventListener("keydown", closeOnEscape);
		return () => {
			document.removeEventListener("mousedown", closeOutside);
			document.removeEventListener("keydown", closeOnEscape);
		};
	}, [accountMenuOpen]);

	const orderedChats = useMemo(() => [...state.chats].sort((a, b) => {
		const aPinned = pinnedChatIds.has(a.id);
		const bPinned = pinnedChatIds.has(b.id);
		if (aPinned !== bPinned) return aPinned ? -1 : 1;
		return 0;
	}), [pinnedChatIds, state.chats]);
	const visibleChats = useMemo(() => {
		if (showAllChats) return orderedChats;
		const alwaysVisible = new Set([
			...orderedChats.filter((chat) => pinnedChatIds.has(chat.id)).map((chat) => chat.id),
			...orderedChats.filter((chat) => chat.id === activeChatId || workingChatIds.has(chat.id)).map((chat) => chat.id)
		]);
		const priority = orderedChats.filter((chat) => alwaysVisible.has(chat.id));
		const remainder = orderedChats.filter((chat) => !alwaysVisible.has(chat.id));
		return [...priority, ...remainder].slice(0, Math.max(10, priority.length));
	}, [activeChatId, orderedChats, pinnedChatIds, showAllChats, workingChatIds]);

	if (!sidebarVisible) return null;

	return (
		<aside className="sidebar" data-testid="sidebar" style={{ width: sidebarWidth }}>
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
							aria-controls="sidebar-chats"
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
						<div id="sidebar-chats" className="section-list" data-testid="chat-list">
							{orderedChats.length === 0 ? (
								<p className="empty-hint">No local chats yet</p>
							) : (
								visibleChats.map((chat) => {
									const title = conversationTitles[chat.id] ?? chat.title;
									const pinned = pinnedChatIds.has(chat.id);
									const working = workingChatIds.has(chat.id);
									if (renamingId === chat.id) {
										return (
											<form
												key={chat.id}
												className="chat-rename-form"
												data-testid={`rename-chat-${chat.id}`}
												onSubmit={(event) => {
													event.preventDefault();
													try {
														onRenameChat?.(chat.id, renameDraft);
														setRenamingId(null);
													} catch {
														/* keep editing on empty reject */
													}
												}}
											>
												<input
													aria-label="Rename conversation"
													value={renameDraft}
													autoFocus
													onChange={(event) => setRenameDraft(event.target.value)}
													onKeyDown={(event) => {
														if (event.key === "Escape") {
															event.preventDefault();
															setRenamingId(null);
														}
													}}
												/>
												<button type="submit">Save</button>
												<button type="button" onClick={() => setRenamingId(null)}>Cancel</button>
											</form>
										);
									}
									return (
										<button
											key={chat.id}
											type="button"
											className={`chat-item${activeChatId === chat.id ? " active" : ""}${pinned ? " pinned" : ""}`}
											onClick={() => onOpenChat(chat.id)}
											onContextMenu={(event) => {
												event.preventDefault();
											setMenu({ id: chat.id, x: event.clientX, y: event.clientY, invoker: event.currentTarget });
											}}
											onKeyDown={(event) => {
												if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
													event.preventDefault();
													const rect = event.currentTarget.getBoundingClientRect();
												setMenu({ id: chat.id, x: rect.left + 8, y: rect.bottom, invoker: event.currentTarget });
												}
											}}
											aria-haspopup="menu"
											data-testid={`local-chat-${chat.id}`}
										>
											<IconGlobe />
											<span className="item-label">{title}</span>
											{pinned ? <span className="chat-pin-marker" aria-label="Pinned" title="Pinned" data-testid={`chat-pinned-${chat.id}`}>Pinned</span> : null}
											{working ? (
												<span className="chat-working-indicator" aria-label="Working" title="Working" data-testid={`chat-working-${chat.id}`} />
											) : unreadChatIds.has(chat.id) ? (
												<span className="chat-unread-indicator" aria-label="Finished, unviewed" title="Finished, unviewed" data-testid={`chat-unread-${chat.id}`} />
											) : null}
										</button>
									);
								})
							)}
							{orderedChats.length > visibleChats.length ? (
								<button
									type="button"
									className="sidebar-show-more"
									data-testid="sidebar-show-all-chats"
									aria-expanded={showAllChats}
									onClick={() => setShowAllChats(true)}
								>
									Show {orderedChats.length - visibleChats.length} more
								</button>
							) : showAllChats && orderedChats.length > 10 ? (
								<button
									type="button"
									className="sidebar-show-more"
									data-testid="sidebar-show-fewer-chats"
									onClick={() => setShowAllChats(false)}
								>
									Show fewer
								</button>
							) : null}
						</div>
					) : null}
				</div>

				{/* ── Cloud = Intern sync sessions + pinned async ── */}
				<div className="sidebar-section">
					<div className="section-header">
						<button
							type="button"
							className="section-header-label"
							onClick={() => setCloudOpen((v) => !v)}
							aria-expanded={cloudOpen}
							aria-controls="sidebar-cloud"
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
						<div id="sidebar-cloud" className="section-list" data-testid="cloud-list">
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
							aria-controls="sidebar-research"
						>
							Research
							<SectionChevron open={researchOpen} />
						</button>
					</div>
					{researchOpen ? (
						<div id="sidebar-research" className="section-list" data-testid="research-nav">
							<button
								type="button"
								className={`chat-item${visualsActive ? " active" : ""}`}
								onClick={onOpenVisuals}
								data-testid="open-visuals"
							>
								<IconSearch />
								<span className="item-label">Visuals</span>
							</button>
							<button
								type="button"
								className={`chat-item${optimizersActive ? " active" : ""}`}
								onClick={onOpenOptimizers}
								data-testid="open-optimizers"
							>
								<IconInventory />
								<span className="item-label">Optimizers</span>
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
							aria-controls="sidebar-inventory"
						>
							Inventory
							<SectionChevron open={inventoryOpen} />
						</button>
					</div>
					{inventoryOpen ? (
						<div id="sidebar-inventory" className="section-list" data-testid="inventory-nav">
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
				<div className="account-footer" ref={accountMenuRef}>
					{accountMenuOpen ? (
						<div className="account-menu" role="menu" data-testid="account-menu">
							<div className="account-menu-identity">
								<span className="account-avatar" aria-hidden>{(accountDisplayName ?? "S").slice(0, 1).toUpperCase()}</span>
								<span><strong>{accountDisplayName ?? (accountSignedIn ? "Synth account" : "Local mode")}</strong><small>{accountSignedIn ? "Signed in" : "Not signed in"}</small></span>
							</div>
							<button type="button" className="account-menu-row" onClick={() => setUsageOpen((value) => !value)} aria-expanded={usageOpen} data-testid="account-usage-toggle">
								<span className="account-menu-glyph" aria-hidden>◔</span><span>Usage remaining</span><span className={`account-menu-chevron${usageOpen ? " open" : ""}`} aria-hidden>›</span>
							</button>
							{usageOpen ? (
								<div className="account-usage" data-testid="account-usage">
									{accountPlan ? (
										<>
											<div><span>{accountPlan.name} plan</span><strong data-testid="account-plan-allowance">${accountPlan.monthlyAllowanceUsd.toFixed(2)} monthly</strong></div>
											<div><span>Used this month</span><strong data-testid="account-plan-used">${accountPlan.usedUsd.toFixed(2)}</strong></div>
											<div><span>Remaining</span><strong data-testid="account-plan-remaining">${accountPlan.remainingUsd.toFixed(2)}</strong></div>
											<div><span>Resets</span><strong data-testid="account-plan-resets">{new Date(accountPlan.resetsAt).toLocaleDateString()}</strong></div>
										</>
									) : (
										<div><span>Weekly budget</span><strong>Not reported</strong></div>
									)}
									<div><span>Tracked this week</span><strong>{(accountUsage?.weeklyTokens ?? 0).toLocaleString()} tokens</strong></div>
									{(accountUsage?.weeklyCostUsd ?? 0) > 0 ? <div><span>Estimated cost</span><strong>${accountUsage!.weeklyCostUsd.toFixed(2)}</strong></div> : null}
									<div><span>All tracked usage</span><strong>{(accountUsage?.totalTokens ?? 0).toLocaleString()} tokens · {accountUsage?.entries ?? 0} runs</strong></div>
									{accountPlan ? null : <p>Synth does not currently report a cloud allowance or reset date.</p>}
								</div>
							) : null}
							<button type="button" className="account-menu-row" onClick={() => { setAccountMenuOpen(false); (onOpenAccount ?? onSettings)(); }} data-testid="open-account-settings" role="menuitem">
								<span className="account-menu-glyph" aria-hidden>◎</span><span>{accountSignedIn ? "Manage account" : "Sign in to Synth"}</span>
							</button>
							<button type="button" className="account-menu-row" onClick={() => { setAccountMenuOpen(false); onSettings(); }} data-testid="account-menu-settings" role="menuitem">
								<IconSettings /><span>Settings</span><kbd>⌘,</kbd>
							</button>
							{accountSignedIn ? <button type="button" className="account-menu-row" onClick={() => { setAccountMenuOpen(false); void onSignOut?.(); }} data-testid="account-log-out" role="menuitem"><span className="account-menu-glyph" aria-hidden>↪</span><span>Log out</span></button> : null}
						</div>
					) : null}
					<button ref={accountTriggerRef} type="button" className="account-trigger" onClick={() => setAccountMenuOpen((value) => !value)} aria-expanded={accountMenuOpen} aria-haspopup="menu" data-testid="account-menu-trigger">
						<span className="account-avatar" aria-hidden>{(accountDisplayName ?? "S").slice(0, 1).toUpperCase()}</span>
						<span className="account-trigger-copy"><strong>{accountDisplayName ?? (accountSignedIn ? "Synth account" : "Sign in to Synth")}</strong><small>{accountSignedIn ? "Signed in" : "Local mode"}</small></span>
						<span className="account-help" aria-hidden>?</span>
					</button>
				</div>
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
			{onSidebarWidthChange ? (
				<PaneResizeHandle
					value={sidebarWidth}
					onChange={onSidebarWidthChange}
					minPrimary={480}
					minSecondary={180}
					ariaLabel="Resize sidebar"
					direction="sidebar"
				/>
			) : null}
			<ConversationContextMenu
				open={Boolean(menu)}
				x={menu?.x ?? 0}
				y={menu?.y ?? 0}
				conversationId={menu?.id ?? ""}
				pinned={menu ? pinnedChatIds.has(menu.id) : false}
				archived={false}
				working={menu ? workingChatIds.has(menu.id) : false}
				onClose={() => {
					const invoker = menu?.invoker;
					setMenu(null);
					requestAnimationFrame(() => invoker?.isConnected && invoker.focus());
				}}
				onRename={(id) => {
					const chat = state.chats.find((entry) => entry.id === id);
					setRenameDraft(conversationTitles[id] ?? chat?.title ?? "");
					setRenamingId(id);
				}}
				onPin={(id, pinned) => onPinChat?.(id, pinned)}
				onArchive={(id, archived) => onArchiveChat?.(id, archived)}
			/>
		</aside>
	);
}
