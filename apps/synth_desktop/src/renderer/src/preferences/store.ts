import {
	DEFAULT_PREFERENCES,
	migrateLegacyPreferences,
	normalizePreferences,
	PREFERENCES_STORAGE_KEY,
	type ConversationMeta,
	type DesktopPreferences,
	type LayoutSnapshot,
	type QueuedPrompt,
	type ToolActivityMode,
	type ActiveEnterAction,
	type ThemePreference,
	type CompactContextModel
} from "./schema";

type Listener = (prefs: DesktopPreferences) => void;

let cached: DesktopPreferences | null = null;
const listeners = new Set<Listener>();

function storage(): Storage | null {
	try {
		return typeof window !== "undefined" ? window.localStorage : null;
	} catch {
		return null;
	}
}

export function loadPreferences(): DesktopPreferences {
	if (cached) return cached;
	const store = storage();
	if (!store) {
		cached = normalizePreferences(DEFAULT_PREFERENCES);
		return cached;
	}
	cached = migrateLegacyPreferences(store);
	persist(cached, store);
	return cached;
}

function persist(prefs: DesktopPreferences, store: Storage | null = storage()) {
	cached = prefs;
	if (!store) return;
	store.setItem(PREFERENCES_STORAGE_KEY, JSON.stringify(prefs));
	// Keep legacy readers in sync for concurrent lanes / older tests.
	store.setItem("synth.approvalMode", prefs.approvalMode);
	store.setItem("synth.unreadCompletedChats", JSON.stringify(prefs.unreadCompletedChats));
	store.setItem("synth.inventoryContainerPaneWidth", String(prefs.layout.last.outputPaneWidth));
}

function commit(next: DesktopPreferences): DesktopPreferences {
	const normalized = normalizePreferences(next);
	persist(normalized);
	for (const listener of listeners) listener(normalized);
	return normalized;
}

export function subscribePreferences(listener: Listener): () => void {
	listeners.add(listener);
	return () => { listeners.delete(listener); };
}

export function getPreferences(): DesktopPreferences {
	return loadPreferences();
}

export function updatePreferences(mutator: (current: DesktopPreferences) => DesktopPreferences): DesktopPreferences {
	return commit(mutator(loadPreferences()));
}

export function resetPreferences(): DesktopPreferences {
	return commit(normalizePreferences(DEFAULT_PREFERENCES));
}

export function setTheme(theme: ThemePreference): DesktopPreferences {
	return updatePreferences((current) => ({
		...current,
		appearance: { ...current.appearance, theme }
	}));
}

export function setToolActivityMode(mode: ToolActivityMode): DesktopPreferences {
	return updatePreferences((current) => ({
		...current,
		toolActivity: { mode }
	}));
}

export function setAutoCompactTokenLimit(model: CompactContextModel, autoCompactTokenLimit: number): DesktopPreferences {
	return updatePreferences((current) => ({
		...current,
		agentContext: {
			autoCompactTokenLimits: {
				...current.agentContext.autoCompactTokenLimits,
				[model]: autoCompactTokenLimit
			}
		}
	}));
}

export function setActiveEnterAction(action: ActiveEnterAction): DesktopPreferences {
	return updatePreferences((current) => ({
		...current,
		submission: { activeEnterAction: action }
	}));
}

export function setAppearanceFonts(patch: Partial<DesktopPreferences["appearance"]>): DesktopPreferences {
	return updatePreferences((current) => ({
		...current,
		appearance: { ...current.appearance, ...patch }
	}));
}

export function saveLayout(last: LayoutSnapshot): DesktopPreferences {
	return updatePreferences((current) => ({
		...current,
		layout: { ...current.layout, last }
	}));
}

export function saveLayoutAsDefault(snapshot?: LayoutSnapshot): DesktopPreferences {
	return updatePreferences((current) => {
		const next = snapshot ?? current.layout.last;
		return {
			...current,
			layout: { last: next, default: { ...next } }
		};
	});
}

export function resetLayoutToDefault(): DesktopPreferences {
	return updatePreferences((current) => ({
		...current,
		layout: {
			last: { ...DEFAULT_PREFERENCES.layout.last },
			default: { ...DEFAULT_PREFERENCES.layout.default }
		}
	}));
}

export function applyDefaultLayout(): DesktopPreferences {
	return updatePreferences((current) => ({
		...current,
		layout: { ...current.layout, last: { ...current.layout.default } }
	}));
}

export function setUnreadCompletedChats(ids: Iterable<string>): DesktopPreferences {
	return updatePreferences((current) => ({
		...current,
		unreadCompletedChats: [...new Set(ids)]
	}));
}

export function setApprovalModePreference(mode: DesktopPreferences["approvalMode"]): DesktopPreferences {
	return updatePreferences((current) => ({ ...current, approvalMode: mode }));
}

function conversationMeta(current: DesktopPreferences, id: string): ConversationMeta {
	return current.conversations[id] ?? {
		titleOverride: null,
		pinned: false,
		pinOrder: null,
		archived: false,
		archivedAt: null
	};
}

export function renameConversation(id: string, title: string): DesktopPreferences {
	const normalized = title.trim();
	if (!normalized) throw new Error("Conversation name cannot be empty");
	return updatePreferences((current) => ({
		...current,
		conversations: {
			...current.conversations,
			[id]: { ...conversationMeta(current, id), titleOverride: normalized }
		}
	}));
}

export function pinConversation(id: string, pinned: boolean): DesktopPreferences {
	return updatePreferences((current) => {
		const meta = conversationMeta(current, id);
		const maxOrder = Object.values(current.conversations)
			.filter((entry) => entry.pinned)
			.reduce((max, entry) => Math.max(max, entry.pinOrder ?? 0), -1);
		return {
			...current,
			conversations: {
				...current.conversations,
				[id]: {
					...meta,
					pinned,
					pinOrder: pinned ? (meta.pinOrder ?? maxOrder + 1) : null
				}
			}
		};
	});
}

export function archiveConversation(id: string, archived: boolean): DesktopPreferences {
	return updatePreferences((current) => ({
		...current,
		conversations: {
			...current.conversations,
			[id]: {
				...conversationMeta(current, id),
				archived,
				archivedAt: archived ? new Date().toISOString() : null
			}
		},
		// Archived chats leave the active unread set.
		unreadCompletedChats: archived
			? current.unreadCompletedChats.filter((chatId) => chatId !== id)
			: current.unreadCompletedChats
	}));
}

export function listArchivedConversationIds(prefs: DesktopPreferences = loadPreferences()): string[] {
	return Object.entries(prefs.conversations)
		.filter(([, meta]) => meta.archived)
		.sort((a, b) => String(b[1].archivedAt ?? "").localeCompare(String(a[1].archivedAt ?? "")))
		.map(([id]) => id);
}

export function enqueuePrompt(conversationId: string, text: string): DesktopPreferences {
	const trimmed = text.trim();
	if (!trimmed) throw new Error("Queued prompt cannot be empty");
	const item: QueuedPrompt = {
		id: `queue-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
		conversationId,
		text: trimmed,
		createdAt: new Date().toISOString()
	};
	return updatePreferences((current) => ({
		...current,
		promptQueue: [...current.promptQueue, item]
	}));
}

export function updateQueuedPrompt(id: string, text: string): DesktopPreferences {
	const trimmed = text.trim();
	if (!trimmed) throw new Error("Queued prompt cannot be empty");
	return updatePreferences((current) => ({
		...current,
		promptQueue: current.promptQueue.map((item) => item.id === id ? { ...item, text: trimmed } : item)
	}));
}

export function removeQueuedPrompt(id: string): DesktopPreferences {
	return updatePreferences((current) => ({
		...current,
		promptQueue: current.promptQueue.filter((item) => item.id !== id)
	}));
}

export function promptsForConversation(conversationId: string, prefs: DesktopPreferences = loadPreferences()): QueuedPrompt[] {
	return prefs.promptQueue.filter((item) => item.conversationId === conversationId);
}

/** Peek without removing. A queued prompt is acknowledged only after a real send succeeds. */
export function nextQueuedPrompt(conversationId: string): QueuedPrompt | null {
	return loadPreferences().promptQueue.find((item) => item.conversationId === conversationId) ?? null;
}

/** Apply CSS custom properties and theme attribute from preferences. */
export function applyPreferencesToDocument(prefs: DesktopPreferences, root: HTMLElement = document.documentElement): void {
	const theme = prefs.appearance.theme;
	if (theme === "system") {
		root.removeAttribute("data-theme");
	} else {
		root.setAttribute("data-theme", theme);
	}
	root.style.setProperty("--chat-font-size", `${prefs.appearance.chatFontSize}px`);
	root.style.setProperty("--code-font-family", prefs.appearance.codeFontFamily);
	root.style.setProperty("--code-font-size", `${prefs.appearance.codeFontSize}px`);
	root.style.setProperty("--terminal-font-family", prefs.appearance.terminalFontFamily);
	root.style.setProperty("--terminal-font-size", `${prefs.appearance.terminalFontSize}px`);
	root.style.setProperty("--sidebar-width", `${prefs.layout.last.sidebarWidth}px`);
	root.style.setProperty("--output-pane-width", `${prefs.layout.last.outputPaneWidth}px`);
	root.style.setProperty("--bottom-panel-height", `${prefs.layout.last.bottomPanelHeight}px`);
	root.classList.toggle("sidebar-hidden", !prefs.layout.last.sidebarVisible);
	root.classList.toggle("bottom-panel-visible", prefs.layout.last.bottomPanelVisible);
}

/** Test / eval adapter — same persistence path production uses. */
export function preferencesAdapter() {
	return {
		get: getPreferences,
		set: (raw: unknown) => commit(normalizePreferences(raw)),
		reset: resetPreferences,
		storageKey: PREFERENCES_STORAGE_KEY
	};
}
