/**
 * Canonical desktop UI preferences (Poolside polish pass).
 *
 * One durable source: `synth.preferences.v1` in localStorage, normalized on every
 * read. Legacy keys migrate in once and are left in place for older readers.
 */

export const PREFERENCES_STORAGE_KEY = "synth.preferences.v1";
export const PREFERENCES_SCHEMA_VERSION = 6 as const;
export const DEFAULT_VISIBLE_PLUGIN_IDS = ["visuals", "experiments", "inventory", "inference"] as const;

export type ThemePreference = "system" | "light" | "dark";
export type ToolActivityMode = "detailed" | "grouped" | "compact";
export type ActiveEnterAction = "steer" | "enqueue";
export type ApprovalPolicyPreference = "untrusted" | "on-request" | "never";
export type SandboxModePreference = "read-only" | "workspace-write" | "danger-full-access";
export type CompactContextModel = "lagunaXs" | "lagunaS" | "luna";
export type AutoCompactTokenLimits = Record<CompactContextModel, number>;

export const DEFAULT_AUTO_COMPACT_TOKEN_LIMITS: AutoCompactTokenLimits = {
	lagunaXs: 150_000,
	lagunaS: 250_000,
	luna: 250_000
};

export type LayoutSnapshot = {
	sidebarVisible: boolean;
	sidebarWidth: number;
	outputPaneVisible: boolean;
	outputPaneWidth: number;
	visualsListWidth: number;
	bottomPanelVisible: boolean;
	bottomPanelHeight: number;
	selectedConversationId: string | null;
	selectedOutputTab: string | null;
	optimizers: {
		selectedRunId: string | null;
	};
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
		/** Optional larval-mander header in the chat column. Default off. */
		showMascot: boolean;
	};
	submission: {
		/** Enter while an agent is working. Cmd+Enter performs the alternate. */
		activeEnterAction: ActiveEnterAction;
	};
	toolActivity: {
		mode: ToolActivityMode;
	};
	agentContext: {
		/** Per-model local summarization thresholds. */
		autoCompactTokenLimits: AutoCompactTokenLimits;
	};
	navigation: {
		visiblePluginIds: string[];
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
	approvalPolicy: ApprovalPolicyPreference;
	sandboxMode: SandboxModePreference;
};

export const DEFAULT_LAYOUT: LayoutSnapshot = {
	sidebarVisible: true,
	sidebarWidth: 260,
	outputPaneVisible: false,
	outputPaneWidth: 720,
	visualsListWidth: 560,
	bottomPanelVisible: false,
	bottomPanelHeight: 220,
	selectedConversationId: null,
	selectedOutputTab: null,
	optimizers: { selectedRunId: null }
};

export const DEFAULT_PREFERENCES: DesktopPreferences = {
	schemaVersion: PREFERENCES_SCHEMA_VERSION,
	appearance: {
		theme: "system",
		chatFontSize: 14,
		codeFontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
		codeFontSize: 12,
		terminalFontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
		terminalFontSize: 12,
		showMascot: false
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
	navigation: {
		visiblePluginIds: [...DEFAULT_VISIBLE_PLUGIN_IDS]
	},
	layout: {
		last: { ...DEFAULT_LAYOUT },
		default: { ...DEFAULT_LAYOUT }
	},
	conversations: {},
	promptQueue: [],
	unreadCompletedChats: [],
	approvalMode: "ask",
	approvalPolicy: "untrusted",
	sandboxMode: "workspace-write"
};

const THEMES = new Set<ThemePreference>(["system", "light", "dark"]);
const ACTIVITY_MODES = new Set<ToolActivityMode>(["detailed", "grouped", "compact"]);
const ENTER_ACTIONS = new Set<ActiveEnterAction>(["steer", "enqueue"]);
const APPROVAL_MODES = new Set(["ask", "accept-edits", "allow-all"]);
const APPROVAL_POLICIES = new Set<ApprovalPolicyPreference>(["untrusted", "on-request", "never"]);
const SANDBOX_MODES = new Set<SandboxModePreference>(["read-only", "workspace-write", "danger-full-access"]);

function legacyPermissionConfig(mode: DesktopPreferences["approvalMode"]): Pick<DesktopPreferences, "approvalPolicy" | "sandboxMode"> {
	if (mode === "allow-all") return { approvalPolicy: "never", sandboxMode: "danger-full-access" };
	if (mode === "accept-edits") return { approvalPolicy: "on-request", sandboxMode: "workspace-write" };
	return { approvalPolicy: "untrusted", sandboxMode: "workspace-write" };
}

export function clampNumber(value: unknown, min: number, max: number, fallback: number): number {
	// Unset must fall back, never clamp: Number(null) and Number("") are 0,
	// which silently pinned absent values to `min` (the 16k autocompact bug).
	if (value === null || value === undefined || value === "") return fallback;
	const n = typeof value === "number" ? value : Number(value);
	if (!Number.isFinite(n)) return fallback;
	return Math.min(max, Math.max(min, Math.round(n)));
}

function normalizeAutoCompactTokenLimit(value: unknown, max: number, fallback: number): number {
	if (value === null || value === undefined || value === "") return fallback;
	const parsed = typeof value === "number" ? value : Number(value);
	// 16k was an accidental normalization floor, never a valid persisted
	// compact threshold. Invalid and formerly floor-clamped values return to the
	// model's documented default instead of being raised to another low limit.
	if (!Number.isFinite(parsed) || parsed <= 16_000) return fallback;
	return Math.min(max, Math.round(parsed));
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
	// A visual can consume nearly the full workbench. The live splitter still
	// preserves a narrow primary rail, but persisted widths must not re-clamp a
	// wide visual to the historical 720px ceiling.
	const maxOutput = Math.max(minOutput, Math.min(2400, viewportWidth - 160));
	const minVisualsList = 280;
	// Preserve the desktop preference while compact layouts are stacked; the
	// live separator clamps against its actual parent content box.
	const maxVisualsList = 960;
	const minBottom = 120;
	const maxBottom = Math.max(minBottom, Math.min(480, viewportHeight - 280));
	const optimizers = source.optimizers && typeof source.optimizers === "object"
		? source.optimizers as Record<string, unknown>
		: {};
	return {
		sidebarVisible: source.sidebarVisible !== false,
		sidebarWidth: clampNumber(source.sidebarWidth, minSidebar, maxSidebar, DEFAULT_LAYOUT.sidebarWidth),
		outputPaneVisible: source.outputPaneVisible === true,
		outputPaneWidth: clampNumber(source.outputPaneWidth, minOutput, maxOutput, DEFAULT_LAYOUT.outputPaneWidth),
		visualsListWidth: clampNumber(source.visualsListWidth, minVisualsList, maxVisualsList, DEFAULT_LAYOUT.visualsListWidth),
		bottomPanelVisible: source.bottomPanelVisible === true,
		bottomPanelHeight: clampNumber(source.bottomPanelHeight, minBottom, maxBottom, DEFAULT_LAYOUT.bottomPanelHeight),
		selectedConversationId: typeof source.selectedConversationId === "string" ? source.selectedConversationId : null,
		selectedOutputTab: typeof source.selectedOutputTab === "string" ? source.selectedOutputTab : null,
		optimizers: {
			selectedRunId: typeof optimizers.selectedRunId === "string" ? optimizers.selectedRunId : null
		}
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
	const storedSchemaVersion = Number(source.schemaVersion);
	// Version 1 persisted the old computed 80%-of-1.05M defaults. Move only
	// those exact generated values to the new 250k defaults; preserve edits.
	const lagunaSLimit = storedSchemaVersion < 2 && compactLimits.lagunaS === 840_000
		? DEFAULT_AUTO_COMPACT_TOKEN_LIMITS.lagunaS
		: compactLimits.lagunaS;
	const lunaLimit = storedSchemaVersion < 2 && compactLimits.luna === 840_000
		? DEFAULT_AUTO_COMPACT_TOKEN_LIMITS.luna
		: compactLimits.luna;
	// Version < 4: the UI floor (16k) was accidentally persisted for all models
	// and caused mid-turn autocompact loops — first written directly (< 3), then
	// regenerated by normalize itself when the limits were absent (clampNumber
	// pinned Number(null) === 0 to the floor, < 4). Treat exact 16k as unset.
	const storedBeforeFloorFix = !(storedSchemaVersion >= 4);
	const migrateFloor = (value: unknown, fallback: number) =>
		storedBeforeFloorFix && value === 16_000 ? fallback : value;
	const legacyCompactLimitValue = agentContext.autoCompactTokenLimit;
	const legacyCompactLimit = legacyCompactLimitValue === null || legacyCompactLimitValue === undefined || legacyCompactLimitValue === ""
		? Number.NaN
		: Number(legacyCompactLimitValue);
	const legacyOverride = Number.isFinite(legacyCompactLimit) && legacyCompactLimit !== 196_000 && legacyCompactLimit > 16_000
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
	const legacyPermissions = legacyPermissionConfig(approvalMode);
	const approvalPolicy = APPROVAL_POLICIES.has(source.approvalPolicy as ApprovalPolicyPreference) ? source.approvalPolicy as ApprovalPolicyPreference : legacyPermissions.approvalPolicy;
	const sandboxMode = SANDBOX_MODES.has(source.sandboxMode as SandboxModePreference) ? source.sandboxMode as SandboxModePreference : legacyPermissions.sandboxMode;
	const navigation = source.navigation && typeof source.navigation === "object"
		? source.navigation as Record<string, unknown>
		: {};
	const visiblePluginIds = Array.isArray(navigation.visiblePluginIds)
		? [...new Set(navigation.visiblePluginIds.filter((id): id is string => typeof id === "string" && id.trim() !== ""))]
		: [...DEFAULT_VISIBLE_PLUGIN_IDS];

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
			terminalFontSize: clampNumber(appearance.terminalFontSize, 10, 20, DEFAULT_PREFERENCES.appearance.terminalFontSize),
			showMascot: appearance.showMascot === true
		},
		submission: { activeEnterAction: enter },
		toolActivity: { mode },
		agentContext: {
			autoCompactTokenLimits: {
				lagunaXs: normalizeAutoCompactTokenLimit(
					migrateFloor(compactLimits.lagunaXs, DEFAULT_AUTO_COMPACT_TOKEN_LIMITS.lagunaXs) ?? legacyOverride,
					235_929,
					DEFAULT_AUTO_COMPACT_TOKEN_LIMITS.lagunaXs
				),
				lagunaS: normalizeAutoCompactTokenLimit(
					migrateFloor(lagunaSLimit, DEFAULT_AUTO_COMPACT_TOKEN_LIMITS.lagunaS) ?? legacyOverride,
					945_000,
					DEFAULT_AUTO_COMPACT_TOKEN_LIMITS.lagunaS
				),
				luna: normalizeAutoCompactTokenLimit(
					migrateFloor(lunaLimit, DEFAULT_AUTO_COMPACT_TOKEN_LIMITS.luna) ?? legacyOverride,
					945_000,
					DEFAULT_AUTO_COMPACT_TOKEN_LIMITS.luna
				)
			}
		},
		navigation: { visiblePluginIds },
		layout: {
			last: normalizeLayoutSnapshot(layout.last),
			default: normalizeLayoutSnapshot(layout.default)
		},
		conversations,
		promptQueue,
		unreadCompletedChats: unread,
		approvalMode,
		approvalPolicy,
		sandboxMode
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
	const parsedVersion = parsed && typeof parsed === "object"
		? Number((parsed as Record<string, unknown>).schemaVersion)
		: 0;
	// Version 5 shipped a 420px output-pane default. Give untouched installs the
	// roomier visual default while preserving every explicitly resized width.
	if (parsedVersion > 0 && parsedVersion < 6
		&& base.layout.last.outputPaneWidth === 420
		&& base.layout.default.outputPaneWidth === 420) {
		base.layout.last.outputPaneWidth = 720;
		base.layout.default.outputPaneWidth = 720;
	}

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
		base.layout.last.outputPaneWidth = clampNumber(inventoryWidth, 280, 2400, base.layout.last.outputPaneWidth);
		base.layout.default.outputPaneWidth = base.layout.last.outputPaneWidth;
	}

	const legacyVisualsListWidth = Number(storage.getItem("synth.visuals.list-width"));
	const parsedLast = parsed && typeof parsed === "object"
		&& (parsed as Record<string, unknown>).layout
		&& typeof (parsed as Record<string, unknown>).layout === "object"
		? ((parsed as Record<string, unknown>).layout as Record<string, unknown>).last
		: null;
	const hasCanonicalVisualsWidth = parsedLast && typeof parsedLast === "object"
		&& "visualsListWidth" in (parsedLast as Record<string, unknown>);
	if (!hasCanonicalVisualsWidth && Number.isFinite(legacyVisualsListWidth) && legacyVisualsListWidth > 0) {
		base.layout.last.visualsListWidth = clampNumber(legacyVisualsListWidth, 280, 960, base.layout.last.visualsListWidth);
		base.layout.default.visualsListWidth = base.layout.last.visualsListWidth;
	}

	return normalizePreferences(base);
}
