export type LandingScenarioId =
	| "landing-first-run"
	| "landing-downloading"
	| "landing-ready"
	| "landing-with-history"
	| "landing-with-project";

export type ModelStatus =
	| "not_installed"
	| "starting"
	| "loading"
	| "downloading"
	| "ready"
	| "error";

export type SyncSessionStatus =
	| "ready"
	| "thinking"
	| "waiting_for_operator"
	| "paused"
	| "closed";

export type AsyncPhase = "running" | "sleeping" | "blocked" | "waiting_for_input";

/** Product mailbox lane (Intern SSE) vs evidence-only Codex activity. */
export type ActivityLane = "intern" | "codex";

export type ChatMessage = {
	id: string;
	role: "user" | "assistant" | "system";
	body: string;
	at: string;
};

/**
 * Shapes inspired by:
 * - Intern mailbox ledger (`event_kind` + sequence) — product authority
 * - Codex activity stream (agent_message, reasoning, command_execution, …) — evidence only
 */
export type ActivityEvent = {
	sequence: number;
	eventKind: string;
	lane: ActivityLane;
	summary: string;
	at: string;
	detail?: string;
};

export type ExecutionTargetOption = {
	id: string;
	label: string;
	description: string;
	/** Where tokens run — local Metal, remote API, or Synth Cloud Intern. */
	group: "local" | "remote" | "cloud";
};

/** First-class visual / artifact (Claude Artifacts analogue, Synth-shaped). */
export type ArtifactKind =
	| "html"
	| "react_app"
	| "chart"
	| "score_series"
	| "environment_frame"
	| "report";

export type ArtifactRef = {
	id: string;
	kind: ArtifactKind;
	title: string;
	summary?: string;
	/** Message that introduced / attached this visual. */
	messageId?: string;
	/** Agent chose to surface it in the workbench. */
	shownByAgent?: boolean;
	/** Mock preview payload for the Visual pane. */
	preview?: {
		variant: "craftax_pareto" | "craftax_frame" | "generic";
		metrics?: { label: string; value: string }[];
	};
	/** Runtime visual template id (e.g. craftax.eval_matrix.v1). */
	templateId?: string;
	/** Runtime visual instance id from `/v1/visuals`. */
	visualId?: string;
	bindings?: import("@synth/runtime-protocol").VisualBindings | Record<string, unknown>;
};

/** Inline activity line in a local Laguna transcript (Poolside-style). */
export type LocalActivityLine = {
	id: string;
	label: string;
	/** Correlates a pending approval with its durable grant/rejection event. */
	approvalId?: string;
	alwaysAllowSupported?: boolean;
	/** Expanded raw detail (tool output / thought) when the line is opened. */
	detail?: string;
	/** Opens the first-class runtime artifact associated with this activity. */
	artifactId?: string;
	/** Opens the first-class container inspector associated with this MCP activity. */
	containerId?: string;
	/**
	 * Optional file path for read/write lines — drives Poolside-style file-type icons
	 * (.md, .rs, .ts, .toml, …).
	 */
	path?: string;
	/** Sanitized lifecycle status for an allowlisted tool call. */
	toolStatus?: "running" | "completed" | "failed";
	kind?: "thought" | "search" | "command" | "file_read" | "file_write" | "visual" | "subagent" | "run_summary" | "approval" | "working";
};

export type LocalChat = {
	id: string;
	title: string;
	messages: ChatMessage[];
	/** Activity lines keyed by the assistant message they precede. */
	activityByMessageId?: Record<string, LocalActivityLine[]>;
	artifacts?: ArtifactRef[];
};

export type SyncSession = {
	id: string;
	title: string;
	status: SyncSessionStatus;
	remoteId: string;
	cursor: number;
	messages: ChatMessage[];
	activity: ActivityEvent[];
	artifacts?: ArtifactRef[];
};

export type AsyncInternPin = {
	phase: AsyncPhase;
	summary: string;
	needsInput?: boolean;
	cycle?: number;
	checkpointId?: string;
	remoteId?: string;
	cursor?: number;
	messages: ChatMessage[];
	activity: ActivityEvent[];
};

export type LandingState = {
	id: LandingScenarioId;
	label: string;
	chats: LocalChat[];
	syncSessions: SyncSession[];
	asyncIntern: AsyncInternPin | null;
	projects: { id: string; name: string }[];
	model: {
		status: ModelStatus;
		name: string;
		detail?: string;
		downloadProgress?: number;
		downloadPaused?: boolean;
	};
	selectedTargetId: string;
	internMode?: "remote" | "demo" | "unconfigured";
	composerEnabled: boolean;
	composerPlaceholder: string;
};

/** OpenRouter model ids used by the remote ACP adapter. */
export const OPENROUTER_LUNA_MODEL = "openai/gpt-5.6-luna";
export const OPENROUTER_LAGUNA_S_MODEL = "poolside/laguna-s-2.1";

export const EXECUTION_TARGETS: ExecutionTargetOption[] = [
	{
		id: "local-laguna",
		label: "Laguna XS 2.1",
		description: "Local · MLX · Metal · usage tracked",
		group: "local"
	},
	{
		id: "openrouter-luna",
		label: "GPT 5.6 Luna",
		description: `OpenRouter · ${OPENROUTER_LUNA_MODEL} · usage tracked`,
		group: "remote"
	},
	{
		id: "openrouter-laguna-s",
		label: "Laguna S 2.1",
		description: `OpenRouter · ${OPENROUTER_LAGUNA_S_MODEL} · usage tracked`,
		group: "remote"
	},
	{
		id: "intern-sync",
		label: "Intern · Live",
		description: "Synth Cloud · sync session",
		group: "cloud"
	},
	{
		id: "intern-async",
		label: "Intern · Background",
		description: "Synth Cloud · async (pinned)",
		group: "cloud"
	}
];

/** @deprecated Prefer openrouter-laguna-s — kept for fixture compatibility. */
export const OPENROUTER_POOLSIDE_TARGET_ID = "openrouter-laguna-s";

export const TARGET_GROUP_LABEL: Record<ExecutionTargetOption["group"], string> = {
	local: "Local",
	remote: "Remote · OpenRouter",
	cloud: "Synth Cloud"
};

export const SYNC_STATUS_LABEL: Record<SyncSessionStatus, string> = {
	ready: "Ready",
	thinking: "Thinking",
	waiting_for_operator: "Waiting",
	paused: "Paused",
	closed: "Closed"
};

export const ASYNC_PHASE_LABEL: Record<AsyncPhase, string> = {
	running: "Running",
	sleeping: "Sleeping",
	blocked: "Blocked",
	waiting_for_input: "Needs input"
};
