/**
 * Canonical desktop UI preferences (Poolside polish pass).
 *
 * One durable source: `synth.preferences.v1` in localStorage, normalized on every
 * read. Legacy keys migrate in once and are left in place for older readers.
 */

export const PREFERENCES_STORAGE_KEY = "synth.preferences.v1";
export const PREFERENCES_SCHEMA_VERSION = 1 as const;

export type ThemePreference = "system" | "light" | "dark";
export type ToolActivityMode = "detailed" | "grouped" | "compact";
export type ActiveEnterAction = "steer" | "enqueue";
export type CompactContextModel = "lagunaXs" | "lagunaS" | "luna";
export type AutoCompactTokenLimits = Record<CompactContextModel, number>;

export const DEFAULT_AUTO_COMPACT_TOKEN_LIMITS: AutoCompactTokenLimits = {
	lagunaXs: 209_715,
	lagunaS: 840_000,
	luna: 840_000
};

export type LayoutSnapshot = {
	sidebarVisible: boolean;
	sidebarWidth: number;
	outputPaneVisible: boolean;
	outputPaneWidth: number;
	bottomPanelVisible: boolean;
	bottomPanelHeight: number;
	selectedConversationId: string | null;
	selectedOutputTab: string | null;
};

export type ConversationMeta = {
	titleOverride: string | null;
	pinned: boolean;
	pinOrder: number | null;
	archived: boolean;
	archivedAt: string | null;
};

export type QueuedPrompt = {
	id: string;
	conversationId: string;
	text: string;
	createdAt: string;
};

export type DesktopPreferences = {
	schemaVersion: typeof PREFERENCES_SCHEMA_VERSION;
	appearance: {
		theme: ThemePreference;
		chatFontSize: number;
		codeFontFamily: string;
		codeFontSize: number;
		terminalFontFamily: string;
		terminalFontSize: number;
	};
	submission: {
		/** Enter while an agent is working. Cmd+Enter performs the alternate. */
		activeEnterAction: ActiveEnterAction;
	};
	toolActivity: {
		mode: ToolActivityMode;
	};
	agentContext: {
		/** Per-model local summarization thresholds; defaults are 80% of each context window. */
		autoCompactTokenLimits: AutoCompactTokenLimits;
	};
	layout: {
		last: LayoutSnapshot;
		default: LayoutSnapshot;
	};
	conversations: Record<string, ConversationMeta>;
	/** Durable FIFO prompt queue keyed implicitly by conversationId. */
	promptQueue: QueuedPrompt[];
	/** Finished-but-unviewed chat ids (migrated from synth.unreadCompletedChats). */
	unreadCompletedChats: string[];
	approvalMode: "ask" | "accept-edits" | "allow-all";
};

export const DEFAULT_LAYOUT: LayoutSnapshot = {
	sidebarVisible: true,
	sidebarWidth: 260,
	outputPaneVisible: false,
	outputPaneWidth: 420,
	bottomPanelVisible: false,
	bottomPanelHeight: 220,
	selectedConversationId: null,
	selectedOutputTab: null
};

export const DEFAULT_PREFERENCES: DesktopPreferences = {
	schemaVersion: PREFERENCES_SCHEMA_VERSION,
	appearance: {
		theme: "system",
		chatFontSize: 14,
		codeFontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
		codeFontSize: 12,
		terminalFontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
		terminalFontSize: 12
	},
	submission: {
		activeEnterAction: "enqueue"
	},
	toolActivity: {
		mode: "grouped"
	},
	agentContext: {
		autoCompactTokenLimits: { ...DEFAULT_AUTO_COMPACT_TOKEN_LIMITS }
	},
	layout: {
		last: { ...DEFAULT_LAYOUT },
		default: { ...DEFAULT_LAYOUT }
	},
	conversations: {},
	promptQueue: [],
	unreadCompletedChats: [],
	approvalMode: "ask"
};

const THEMES = new Set<ThemePreference>(["system", "light", "dark"]);
const ACTIVITY_MODES = new Set<ToolActivityMode>(["detailed", "grouped", "compact"]);
const ENTER_ACTIONS = new Set<ActiveEnterAction>(["steer", "enqueue"]);
const APPROVAL_MODES = new Set(["ask", "accept-edits", "allow-all"]);

export function clampNumber(value: unknown, min: number, max: number, fallback: number): number {
	const n = typeof value === "number" ? value : Number(value);
	if (!Number.isFinite(n)) return fallback;
	return Math.min(max, Math.max(min, Math.round(n)));
}

export function normalizeLayoutSnapshot(
	raw: unknown,
	viewportWidth = typeof window !== "undefined" ? window.innerWidth : 1280,
	viewportHeight = typeof window !== "undefined" ? window.innerHeight : 800
): LayoutSnapshot {
	const source = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
	const minSidebar = 180;
	const maxSidebar = Math.max(minSidebar, Math.min(420, viewportWidth - 480));
	const minOutput = 280;
	const maxOutput = Math.max(minOutput, Math.min(720, viewportWidth - 520));
	const minBottom = 120;
	const maxBottom = Math.max(minBottom, Math.min(480, viewportHeight - 280));
	return {
		sidebarVisible: source.sidebarVisible !== false,
		sidebarWidth: clampNumber(source.sidebarWidth, minSidebar, maxSidebar, DEFAULT_LAYOUT.sidebarWidth),
		outputPaneVisible: source.outputPaneVisible === true,
		outputPaneWidth: clampNumber(source.outputPaneWidth, minOutput, maxOutput, DEFAULT_LAYOUT.outputPaneWidth),
		bottomPanelVisible: source.bottomPanelVisible === true,
		bottomPanelHeight: clampNumber(source.bottomPanelHeight, minBottom, maxBottom, DEFAULT_LAYOUT.bottomPanelHeight),
		selectedConversationId: typeof source.selectedConversationId === "string" ? source.selectedConversationId : null,
		selectedOutputTab: typeof source.selectedOutputTab === "string" ? source.selectedOutputTab : null
	};
}

function normalizeConversationMeta(raw: unknown): ConversationMeta {
	const source = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
	const pinned = source.pinned === true;
	return {
		titleOverride: typeof source.titleOverride === "string" && source.titleOverride.trim()
			? source.titleOverride.trim()
			: null,
		pinned,
		pinOrder: pinned ? clampNumber(source.pinOrder, 0, 1_000_000, 0) : null,
		archived: source.archived === true,
		archivedAt: typeof source.archivedAt === "string" ? source.archivedAt : null
	};
}

function normalizeQueuedPrompt(raw: unknown): QueuedPrompt | null {
	if (!raw || typeof raw !== "object") return null;
	const source = raw as Record<string, unknown>;
	if (typeof source.id !== "string" || typeof source.conversationId !== "string") return null;
	if (typeof source.text !== "string" || !source.text.trim()) return null;
	return {
		id: source.id,
		conversationId: source.conversationId,
		text: source.text,
		createdAt: typeof source.createdAt === "string" ? source.createdAt : new Date().toISOString()
	};
}

/** Normalize any stored/malformed value into a supported schema. */
export function normalizePreferences(raw: unknown): DesktopPreferences {
	const source = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
	const appearance = source.appearance && typeof source.appearance === "object"
		? (source.appearance as Record<string, unknown>)
		: {};
	const submission = source.submission && typeof source.submission === "object"
		? (source.submission as Record<string, unknown>)
		: {};
	const toolActivity = source.toolActivity && typeof source.toolActivity === "object"
		? (source.toolActivity as Record<string, unknown>)
		: {};
	const agentContext = source.agentContext && typeof source.agentContext === "object"
		? (source.agentContext as Record<string, unknown>)
		: {};
	const compactLimits = agentContext.autoCompactTokenLimits && typeof agentContext.autoCompactTokenLimits === "object"
		? (agentContext.autoCompactTokenLimits as Record<string, unknown>)
		: {};
	const legacyCompactLimit = Number(agentContext.autoCompactTokenLimit);
	const legacyOverride = Number.isFinite(legacyCompactLimit) && legacyCompactLimit !== 196_000
		? legacyCompactLimit
		: null;
	const layout = source.layout && typeof source.layout === "object"
		? (source.layout as Record<string, unknown>)
		: {};
	const conversationsRaw = source.conversations && typeof source.conversations === "object"
		? (source.conversations as Record<string, unknown>)
		: {};
	const conversations: Record<string, ConversationMeta> = {};
	for (const [id, meta] of Object.entries(conversationsRaw)) {
		if (typeof id === "string" && id) conversations[id] = normalizeConversationMeta(meta);
	}
	const promptQueue = Array.isArray(source.promptQueue)
		? source.promptQueue.map(normalizeQueuedPrompt).filter((item): item is QueuedPrompt => Boolean(item))
		: [];
	const unread = Array.isArray(source.unreadCompletedChats)
		? source.unreadCompletedChats.filter((id): id is string => typeof id === "string")
		: [];
	const theme = THEMES.has(appearance.theme as ThemePreference)
		? (appearance.theme as ThemePreference)
		: DEFAULT_PREFERENCES.appearance.theme;
	const mode = ACTIVITY_MODES.has(toolActivity.mode as ToolActivityMode)
		? (toolActivity.mode as ToolActivityMode)
		: DEFAULT_PREFERENCES.toolActivity.mode;
	const enter = ENTER_ACTIONS.has(submission.activeEnterAction as ActiveEnterAction)
		? (submission.activeEnterAction as ActiveEnterAction)
		: DEFAULT_PREFERENCES.submission.activeEnterAction;
	const approvalMode = APPROVAL_MODES.has(source.approvalMode as string)
		? (source.approvalMode as DesktopPreferences["approvalMode"])
		: DEFAULT_PREFERENCES.approvalMode;

	return {
		schemaVersion: PREFERENCES_SCHEMA_VERSION,
		appearance: {
			theme,
			chatFontSize: clampNumber(appearance.chatFontSize, 12, 22, DEFAULT_PREFERENCES.appearance.chatFontSize),
			codeFontFamily: typeof appearance.codeFontFamily === "string" && appearance.codeFontFamily.trim()
				? appearance.codeFontFamily.trim()
				: DEFAULT_PREFERENCES.appearance.codeFontFamily,
			codeFontSize: clampNumber(appearance.codeFontSize, 10, 20, DEFAULT_PREFERENCES.appearance.codeFontSize),
			terminalFontFamily: typeof appearance.terminalFontFamily === "string" && appearance.terminalFontFamily.trim()
				? appearance.terminalFontFamily.trim()
				: DEFAULT_PREFERENCES.appearance.terminalFontFamily,
			terminalFontSize: clampNumber(appearance.terminalFontSize, 10, 20, DEFAULT_PREFERENCES.appearance.terminalFontSize)
		},
		submission: { activeEnterAction: enter },
		toolActivity: { mode },
		agentContext: {
			autoCompactTokenLimits: {
				lagunaXs: clampNumber(compactLimits.lagunaXs ?? legacyOverride, 16_000, 235_929, DEFAULT_AUTO_COMPACT_TOKEN_LIMITS.lagunaXs),
				lagunaS: clampNumber(compactLimits.lagunaS ?? legacyOverride, 16_000, 945_000, DEFAULT_AUTO_COMPACT_TOKEN_LIMITS.lagunaS),
				luna: clampNumber(compactLimits.luna ?? legacyOverride, 16_000, 945_000, DEFAULT_AUTO_COMPACT_TOKEN_LIMITS.luna)
			}
		},
		layout: {
			last: normalizeLayoutSnapshot(layout.last),
			default: normalizeLayoutSnapshot(layout.default)
		},
		conversations,
		promptQueue,
		unreadCompletedChats: unread,
		approvalMode
	};
}

/** Migrate scattered legacy localStorage keys into the unified schema. */
export function migrateLegacyPreferences(storage: Storage): DesktopPreferences {
	let parsed: unknown = null;
	try {
		const raw = storage.getItem(PREFERENCES_STORAGE_KEY);
		parsed = raw ? JSON.parse(raw) : null;
	} catch {
		parsed = null;
	}
	const base = normalizePreferences(parsed);

	const approval = storage.getItem("synth.approvalMode");
	if (APPROVAL_MODES.has(approval as string) && (!parsed || typeof parsed !== "object")) {
		base.approvalMode = approval as DesktopPreferences["approvalMode"];
	}

	try {
		const unread = JSON.parse(storage.getItem("synth.unreadCompletedChats") ?? "[]");
		if (Array.isArray(unread) && base.unreadCompletedChats.length === 0) {
			base.unreadCompletedChats = unread.filter((id): id is string => typeof id === "string");
		}
	} catch { /* ignore */ }

	const inventoryWidth = Number(storage.getItem("synth.inventoryContainerPaneWidth"));
	if (Number.isFinite(inventoryWidth) && inventoryWidth >= 280 && (!parsed || typeof parsed !== "object")) {
		base.layout.last.outputPaneWidth = clampNumber(inventoryWidth, 280, 720, base.layout.last.outputPaneWidth);
		base.layout.default.outputPaneWidth = base.layout.last.outputPaneWidth;
	}

	return normalizePreferences(base);
}
