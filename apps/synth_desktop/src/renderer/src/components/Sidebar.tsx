import { useEffect, useMemo, useRef, useState } from "react";
import { type LandingState } from "../types/landing";
import { ModelDownloadBar } from "./ModelDownloadBar";
import { LocalModelResidency } from "./LocalModelResidency";
import type { LagunaStatus } from "../bridge";
import { type AccountViewModel } from "../runtime/accountView";
import { ConversationContextMenu } from "./GeneralPreferencesSettings";
import { PaneResizeHandle } from "./PaneResizeHandle";
import { ProviderMark } from "./ProviderMark";

type CodexUsageSnapshot = {
	usedPercent: number;
	resetsAt: number;
	windowMinutes?: number;
	planType?: string;
};

type Props = {
	state: LandingState;
	lagunaStatus?: LagunaStatus | null;
	activeChatId?: string | null;
	inventoryActive?: boolean;
	visualsActive?: boolean;
	optimizersActive?: boolean;
	workingChatIds?: ReadonlySet<string>;
	activeLocalDecodeTps?: string | null;
	unreadChatIds?: ReadonlySet<string>;
	pinnedChatIds?: ReadonlySet<string>;
	conversationTitles?: Record<string, string>;
	onNewConversation: () => void;
	onOpenChat: (id: string) => void;
	onOpenInventory: () => void;
	onOpenVisuals: () => void;
	onOpenOptimizers: () => void;
	onSearch: () => void;
	onSettings: () => void;
	/** Composed by the renderer from the host's account summary. */
	account: AccountViewModel;
	codexOauthConfigured?: boolean;
	codexUsage?: CodexUsageSnapshot | null;
	onOpenAccount?: () => void;
	onOpenUsage?: () => void;
	onBilling?: (action: "upgrade" | "manage") => void;
	onRetryAccount?: () => void;
	onSignOut?: () => void | Promise<void>;
	onPauseToggle: () => void;
	onFreeLocalMemory?: () => Promise<void>;
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

function IconSearch() {
	return (
		<svg className="nav-icon" viewBox="0 0 16 16" fill="none" aria-hidden>
			<circle cx="7" cy="7" r="4" stroke="currentColor" strokeWidth="1.35" />
			<path d="M10.2 10.2L13.5 13.5" stroke="currentColor" strokeWidth="1.35" strokeLinecap="round" />
		</svg>
	);
}

function IconPin({ pinned = false }: { pinned?: boolean }) {
	return (
		<svg className="chat-row-action-icon" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path d="M5.1 2.2h5.8l-.9 3 1.8 2v1.1H8.7v4.9L8 14l-.7-.8V8.3H4.2V7.2l1.8-2-.9-3z" fill={pinned ? "currentColor" : "none"} stroke="currentColor" strokeWidth="1.15" strokeLinejoin="round" />
		</svg>
	);
}

function IconArchive() {
	return (
		<svg className="chat-row-action-icon" viewBox="0 0 16 16" fill="none" aria-hidden>
			<rect x="2.4" y="3" width="11.2" height="2.8" rx=".8" stroke="currentColor" strokeWidth="1.15" />
			<path d="M3.3 5.8v6.7c0 .6.5 1.1 1.1 1.1h7.2c.6 0 1.1-.5 1.1-1.1V5.8M6.2 8.5h3.6" stroke="currentColor" strokeWidth="1.15" strokeLinecap="round" />
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
	inventoryActive = false,
	visualsActive = false,
	optimizersActive = false,
	workingChatIds = new Set<string>(),
	activeLocalDecodeTps = null,
	unreadChatIds = new Set<string>(),
	pinnedChatIds = new Set<string>(),
	conversationTitles = {},
	onNewConversation,
	onOpenChat,
	onOpenInventory,
	onOpenVisuals,
	onOpenOptimizers,
	onSearch,
	onSettings,
	account,
	codexOauthConfigured = false,
	codexUsage = null,
	onOpenAccount,
	onOpenUsage,
	onBilling,
	onRetryAccount,
	onSignOut,
	onPauseToggle,
	onFreeLocalMemory,
	onRenameChat,
	onPinChat,
	onArchiveChat,
	sidebarWidth = 260,
	onSidebarWidthChange,
	sidebarVisible = true
}: Props) {
	const [chatsOpen, setChatsOpen] = useState(true);
	const [inventoryOpen, setInventoryOpen] = useState(true);
	const [researchOpen, setResearchOpen] = useState(true);
	const [menu, setMenu] = useState<{ id: string; x: number; y: number; invoker: HTMLButtonElement } | null>(null);
	const [renamingId, setRenamingId] = useState<string | null>(null);
	const [renameDraft, setRenameDraft] = useState("");
	const [showAllChats, setShowAllChats] = useState(false);
	const [accountMenuOpen, setAccountMenuOpen] = useState(false);
	const [allowanceOpen, setAllowanceOpen] = useState(false);
	const [codexUsageOpen, setCodexUsageOpen] = useState(false);
	const accountMenuRef = useRef<HTMLDivElement>(null);
	const accountTriggerRef = useRef<HTMLButtonElement>(null);
	const codexRemaining = codexUsage ? Math.max(0, Math.round(100 - codexUsage.usedPercent)) : null;
	const codexReset = codexUsage
		? new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" }).format(new Date(codexUsage.resetsAt * 1_000))
		: null;
	// The footer is the Workshop/Synth account surface. A connected Codex OAuth
	// provider enables models, but it is not the user's Synth identity and must
	// never replace signed-in or signed-out Synth account copy here.
	const accountTitle = account.title;
	const accountSubtitle = account.subtitle;

	useEffect(() => {
		if (!accountMenuOpen) return;
		// Read the rows on demand: `Usage remaining` expands in place, so the
		// menu's contents change while it is open.
		const rows = () => Array.from(
			accountMenuRef.current?.querySelectorAll<HTMLButtonElement>("[role=\"menuitem\"]") ?? []
		);
		// An open menu owns the focus, so a keyboard user is inside it rather
		// than still on the trigger behind it.
		requestAnimationFrame(() => rows()[0]?.focus());
		const closeOutside = (event: MouseEvent) => {
			if (!accountMenuRef.current?.contains(event.target as Node)) setAccountMenuOpen(false);
		};
		const onKeyDown = (event: KeyboardEvent) => {
			if (event.key === "Escape") {
				setAccountMenuOpen(false);
				requestAnimationFrame(() => accountTriggerRef.current?.focus());
				return;
			}
			if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
			const items = rows();
			if (!items.length) return;
			// The arrows belong to the menu; let them not scroll the page.
			event.preventDefault();
			const current = items.indexOf(document.activeElement as HTMLButtonElement);
			const step = event.key === "ArrowDown" ? 1 : -1;
			items[(current + step + items.length) % items.length]?.focus();
		};
		document.addEventListener("mousedown", closeOutside);
		document.addEventListener("keydown", onKeyDown);
		return () => {
			document.removeEventListener("mousedown", closeOutside);
			document.removeEventListener("keydown", onKeyDown);
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
	const firstPinnedIndex = visibleChats.findIndex((chat) => pinnedChatIds.has(chat.id));
	const firstRecentIndex = visibleChats.findIndex((chat) => !pinnedChatIds.has(chat.id));

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
								visibleChats.map((chat, chatIndex) => {
									const title = conversationTitles[chat.id] ?? chat.title;
									const pinned = pinnedChatIds.has(chat.id);
									const working = workingChatIds.has(chat.id);
									const sectionLabel = chatIndex === firstPinnedIndex
										? "Pinned"
										: chatIndex === firstRecentIndex ? "Recents" : null;
									if (renamingId === chat.id) {
										return (
											<div key={chat.id} className="chat-section-entry">
												{sectionLabel ? <h3 className="chat-section-label">{sectionLabel}</h3> : null}
											<form
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
											</div>
										);
									}
					return (
						<div key={chat.id} className="chat-section-entry">
							{sectionLabel ? <h3 className="chat-section-label">{sectionLabel}</h3> : null}
						<div className="chat-row">
						<button
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
											<span className="item-label">{title}</span>
											{pinned ? <span className="sr-only" data-testid={`chat-pinned-${chat.id}`}>Pinned</span> : null}
											{working ? (
												<>
													<span
														className="chat-working-indicator"
														aria-label={activeChatId === chat.id && activeLocalDecodeTps ? `Working · ${activeLocalDecodeTps}` : "Working"}
														title={activeChatId === chat.id && activeLocalDecodeTps ? `Working · ${activeLocalDecodeTps}` : "Working"}
														data-testid={`chat-working-${chat.id}`}
													/>
													{activeChatId === chat.id && activeLocalDecodeTps ? (
														<span className="chat-working-rate" data-testid={`chat-working-rate-${chat.id}`}>{activeLocalDecodeTps}</span>
													) : null}
												</>
											) : unreadChatIds.has(chat.id) ? (
												<span className="chat-unread-indicator" aria-label="Finished, unviewed" title="Finished, unviewed" data-testid={`chat-unread-${chat.id}`} />
											) : null}
						</button>
						<div className="chat-row-actions" aria-label={`Actions for ${title}`}>
							<button
								type="button"
								className={`chat-row-action${pinned ? " selected" : ""}`}
								aria-label={pinned ? `Unpin ${title}` : `Pin ${title}`}
								title={pinned ? "Unpin" : "Pin"}
								data-testid={`chat-pin-${chat.id}`}
								onClick={() => onPinChat?.(chat.id, !pinned)}
							>
								<IconPin pinned={pinned} />
							</button>
							<button
								type="button"
								className="chat-row-action"
								aria-label={`Archive ${title}`}
								title={working ? "Wait for this chat to finish before archiving" : "Archive"}
								data-testid={`chat-archive-${chat.id}`}
								disabled={working}
								onClick={() => onArchiveChat?.(chat.id, true)}
							>
								<IconArchive />
							</button>
						</div>
						</div>
						</div>
									);
								})
							)}
							{orderedChats.length > visibleChats.length ? (
								<button
									type="button"
									className="sidebar-show-more"
									data-testid="sidebar-show-all-chats"
									aria-expanded={showAllChats}
									aria-controls="sidebar-chats"
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

				{/*
				 * v0.1 removal contract (launch_v0p1.md §"v0.1 removal contract"):
				 * the Cloud section — sync-session list, "New sync session" action,
				 * and the pinned Async Intern card — is de-scoped and must not be
				 * reachable in the shipped build. The dormant catalog, protocol,
				 * bridge, and CloudDesk component remain for v0.2 re-entry.
				 */}

				{/* ── Research = Visuals + Data ── */}
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

				{/* ── Data = containers / traces / usage ── */}
				<div className="sidebar-section">
					<div className="section-header">
						<button
							type="button"
							className="section-header-label"
							onClick={() => setInventoryOpen((v) => !v)}
							aria-expanded={inventoryOpen}
							aria-controls="sidebar-inventory"
						>
							Data
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
				<LocalModelResidency status={lagunaStatus} onFreeMemory={onFreeLocalMemory} />
				<ModelDownloadBar state={state} onPauseToggle={onPauseToggle} />
				<div className="account-footer" ref={accountMenuRef}>
					{accountMenuOpen ? (
						<div id="account-menu-panel" className="account-menu" role="menu" data-testid="account-menu">
							<div className="account-menu-identity">
								<span className="account-avatar" aria-hidden>{account.initial}</span>
								<span>
									<strong>{accountTitle}</strong>
									<small data-testid="account-menu-subtitle">{accountSubtitle}</small>
								</span>
							</div>
							{account.cloudBlockedReason ? (
								<p className="account-menu-alert" data-testid="account-menu-blocked">{account.cloudBlockedReason}</p>
							) : null}
							{/*
							  Cloud allowance, expandable. Separate from the `Usage`
							  row below on purpose: this summarizes Synth Cloud only,
							  while `Usage` opens the sheet where cloud and device
							  totals are shown side by side but never blended.
							*/}
							<button
								type="button"
								className="account-menu-row"
								onClick={() => setAllowanceOpen((value) => !value)}
								aria-expanded={allowanceOpen}
								aria-controls="account-allowance-panel"
								data-testid="account-usage-remaining"
								role="menuitem"
							>
								<span className="account-menu-glyph" aria-hidden>◔</span>
								<span>Usage remaining</span>
								{account.allowance.headline ? (
									<span className="account-menu-value" data-testid="account-usage-remaining-value">
										{account.allowance.headline}
									</span>
								) : null}
								<SectionChevron open={allowanceOpen} />
							</button>
							{allowanceOpen ? (
								<div id="account-allowance-panel" className="account-menu-panel" data-testid="account-allowance-panel">
									{account.allowance.rows.map((row) => (
										<p key={row.label} className="account-menu-fact">
											<span>{row.label}</span>
											<strong>{row.value}</strong>
										</p>
									))}
									{account.allowance.note ? (
										<p className="account-menu-note" data-testid="account-allowance-note">{account.allowance.note}</p>
									) : null}
									{account.allowance.isDevSeed ? (
										<p className="account-menu-note" data-testid="account-allowance-dev-seed">Local/dev plan stand-in</p>
									) : null}
								</div>
							) : null}
							{codexOauthConfigured ? (
								<>
									<button
										type="button"
										className="account-menu-row account-codex-usage-row"
										onClick={() => setCodexUsageOpen((value) => !value)}
										aria-expanded={codexUsageOpen}
										aria-controls="account-codex-usage-panel"
										data-testid="account-codex-usage-remaining"
										role="menuitem"
									>
										<ProviderMark kind="openai" className="account-menu-openai-mark" />
										<span>Codex usage remaining</span>
										<span className="account-menu-value">{codexRemaining == null ? "Check after a turn" : `${codexRemaining}%`}</span>
										<SectionChevron open={codexUsageOpen} />
									</button>
									{codexUsageOpen ? (
										<div id="account-codex-usage-panel" className="account-menu-panel account-codex-usage-panel" data-testid="account-codex-usage-panel">
											{codexUsage ? <>
												<p className="account-menu-fact"><span>Remaining</span><strong>{codexRemaining}%</strong></p>
												<p className="account-menu-fact"><span>Resets</span><strong>{codexReset}</strong></p>
												{codexUsage.planType ? <p className="account-menu-note">{codexUsage.planType} plan allowance</p> : null}
											</> : <p className="account-menu-note">Run a Codex turn to retrieve your current allowance.</p>}
										</div>
									) : null}
								</>
							) : null}
							<button type="button" className="account-menu-row" onClick={() => { setAccountMenuOpen(false); onOpenUsage?.(); }} data-testid="account-open-usage" role="menuitem">
								<span className="account-menu-glyph" aria-hidden>▤</span><span>Usage</span>
							</button>
							{account.primaryAction && account.primaryAction.kind !== "sign_in" ? (
								<button
									type="button"
									className="account-menu-row"
									data-testid="account-primary-action"
									role="menuitem"
									onClick={() => {
										setAccountMenuOpen(false);
										if (account.primaryAction?.kind === "retry") onRetryAccount?.();
										else onBilling?.(account.primaryAction?.kind === "upgrade" ? "upgrade" : "manage");
									}}
								>
									<span className="account-menu-glyph" aria-hidden>↗</span><span>{account.primaryAction.label}</span>
								</button>
							) : null}
							<button type="button" className="account-menu-row" onClick={() => { setAccountMenuOpen(false); (onOpenAccount ?? onSettings)(); }} data-testid="open-account-settings" role="menuitem">
								<span className="account-menu-glyph" aria-hidden>◎</span><span>{account.signedIn ? "Manage account" : "Sign in to Synth"}</span>
							</button>
							<button type="button" className="account-menu-row" onClick={() => { setAccountMenuOpen(false); onSettings(); }} data-testid="account-menu-settings" role="menuitem">
								<IconSettings /><span>Settings</span><kbd>⌘,</kbd>
							</button>
							{account.signedIn ? <button type="button" className="account-menu-row" onClick={() => { setAccountMenuOpen(false); void onSignOut?.(); }} data-testid="account-log-out" role="menuitem"><span className="account-menu-glyph" aria-hidden>↪</span><span>Log out</span></button> : null}
						</div>
					) : null}
					<button ref={accountTriggerRef} type="button" className="account-trigger" onClick={() => setAccountMenuOpen((value) => !value)} aria-expanded={accountMenuOpen} aria-controls="account-menu-panel" aria-haspopup="menu" data-testid="account-menu-trigger">
						<span className="account-avatar" aria-hidden>{account.initial}</span>
						<span className="account-trigger-copy"><strong>{accountTitle}</strong><small>{accountSubtitle}</small></span>
						<span className="account-help" aria-hidden>?</span>
					</button>
				</div>
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
