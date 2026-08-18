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
	images?: Array<{ path: string; name: string; previewUrl: string }>;
	/** Truncated provider final; render as "Incomplete answer". */
	incomplete?: boolean;
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
	group: "local" | "remote" | "subscription" | "cloud";
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
	/** Rust renderer kind; first-class diagrams bypass the TSX template shell. */
	rendererKind?: string;
	/** Runtime visual instance id from `/v1/visuals`. */
	visualId?: string;
	/** Durable revision used to invalidate asynchronous binding resolutions. */
	revision?: number;
	bindings?: import("@synth/runtime-protocol").VisualBindings | Record<string, unknown>;
	/** Durable visual metadata, including presentation and authoring review receipts. */
	metadata?: Record<string, unknown>;
	/** Session that authored this visual. Read-only display never copies this. */
	ownerSessionId?: string;
	/** Cross-task discovery is labeled, never adopted as this chat's output. */
	foreign?: boolean;
	/** Durable authoring state projected for transcript and pane chrome. */
	status?: "draft" | "review" | "ready" | "failed";
};

/** Inline activity line in a local Laguna transcript (Poolside-style). */
export type LocalActivityLine = {
	id: string;
	label: string;
	/** Correlates a pending approval with its durable grant/rejection event. */
	approvalId?: string;
	// Mirrors `ApprovalKind::as_str` in src-tauri/src/session/approval.rs.
	approvalKind?: "shell_command" | "paid_compute" | "sidecar_lifecycle" | "credential_access" | "plugin_lifecycle" | "permission";
	approvalPayload?: {
		operation?: string;
		parameters?: Record<string, unknown>;
		estimatedCostUsdMicros?: number;
		requestedCap?: { maxCostUsdMicros?: number; maxRollouts?: number };
		requestingAgent?: string;
	};
	alwaysAllowSupported?: boolean;
	/** Expanded raw detail (tool output / thought) when the line is opened. */
	detail?: string;
	/** Whether the disclosure contains local thought text or a provider summary. */
	reasoningDisplay?: "full" | "summary";
	/** Opens the first-class runtime artifact associated with this activity. */
	artifactId?: string;
	/** Opens the first-class container inspector associated with this MCP activity. */
	containerId?: string;
	/** Recipe-owned local runtime (local_process), distinct from a Synth Container. */
	runtimeId?: string;
	/**
	 * Durable optimizer run this activity started or acted on, read from the tool
	 * result rather than inferred from nearby prose. Its presence is what
	 * attaches a live run-progress card at this point in the transcript.
	 */
	optimizerRunId?: string;
	/** The run's workflow, when the tool result declared one chat has a card for. */
	runKind?: "eval" | "gepa" | "sft" | "environment";
	/**
	 * Optional file path for read/write lines — drives Poolside-style file-type icons
	 * (.md, .rs, .ts, .toml, …).
	 */
	path?: string;
	/** Sanitized lifecycle status for an allowlisted tool call. */
	toolStatus?: "running" | "completed" | "failed";
	/** Provider-reported wall duration for a finished tool call. */
	durationMs?: number;
	/** User-facing visual authoring milestone; never a raw tool operation. */
	visualStage?: "draft" | "review" | "ready" | "failed";
	/** Transcript placement relative to the assistant response owning this activity. */
	placement?: "before" | "after";
	/** Source event sequence — used by the placement chronology invariant. */
	sequence?: number;
	/** Token totals surrounding a context compaction (for the disclosure). */
	tokensBefore?: number;
	tokensAfter?: number;
	/** Latest observed total for the active turn's compact activity tail. */
	tokenTotal?: number;
	kind?: "thought" | "search" | "command" | "file_read" | "file_write" | "visual" | "visual_lifecycle" | "subagent" | "run_summary" | "context_compaction" | "approval" | "working";
};

export type LocalChat = {
	id: string;
	title: string;
	messages: ChatMessage[];
	/** Activity lines keyed by their owning assistant message. */
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
	/** When true, closing the window does not pause the job (projection-driven). */
	leaveSafe?: boolean;
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
	model: {
		status: ModelStatus;
		name: string;
		detail?: string;
		downloadProgress?: number;
		downloadPaused?: boolean;
	};
	selectedTargetId: string;
	internMode?: "remote" | "demo" | "unconfigured";
	/** Synth org API key present — gates Synth Cloud billed models. Boolean only; never the secret. */
	apiKeyConfigured?: boolean;
	/** OpenRouter API key present — gates direct OpenRouter models. Boolean only; never the secret. */
	openrouterApiKeyConfigured?: boolean;
	/** ChatGPT subscription OAuth present in the native keychain. */
	codexOauthConfigured?: boolean;
	/** Rust-owned ChatGPT auth state and recovery instructions. */
	codexOauthStatus?: import("../bridge").CodexOauthStatus;
	/**
	 * Backend-authored reason billable Synth Cloud actions are blocked for this
	 * account (exhausted allowance, past due, cancelled). Local models are never
	 * affected. Null when cloud actions are allowed.
	 */
	cloudBlockedReason?: string | null;
	composerEnabled: boolean;
	composerPlaceholder: string;
};

/** OpenRouter model ids used by the remote ACP adapter. */
export const OPENROUTER_LUNA_MODEL = "openai/gpt-5.6-luna";
export const OPENROUTER_LAGUNA_S_MODEL = "poolside/laguna-s-2.1";
export const OPENROUTER_MUSE_SPARK_MODEL = "meta/muse-spark-1.2";
export const OPENROUTER_GEMINI_FLASH_MODEL = "google/gemini-3.7-flash";
/** Fully qualified — bare `laguna-s-2.1` resolves to a different catalog entry server-side. */
export const SYNTH_CLOUD_LAGUNA_S_MODEL = "openrouter/poolside/laguna-s-2.1";
export const SYNTH_CLOUD_MUSE_SPARK_MODEL = "meta/muse-spark-1.2";
export const CHATGPT_LUNA_MODEL = "gpt-5.6-luna";
export const CHATGPT_SOL_MODEL = "gpt-5.6-sol";
export const CHATGPT_TERRA_MODEL = "gpt-5.6-terra";

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
		id: "openrouter-muse-spark",
		label: "Muse Spark 1.2",
		description: `OpenRouter · ${OPENROUTER_MUSE_SPARK_MODEL} · usage tracked`,
		group: "remote"
	},
	{
		id: "openrouter-gemini-flash",
		label: "Gemini 3.7 Flash",
		description: `OpenRouter · ${OPENROUTER_GEMINI_FLASH_MODEL} · usage tracked`,
		group: "remote"
	},
	{
		id: "chatgpt-luna",
		label: "GPT-5.6 Luna",
		description: "ChatGPT · Codex plan allowance",
		group: "subscription"
	},
	{
		id: "chatgpt-sol",
		label: "GPT-5.6 Sol",
		description: "ChatGPT · Codex plan allowance",
		group: "subscription"
	},
	{
		id: "chatgpt-terra",
		label: "GPT-5.6 Terra",
		description: "ChatGPT · Codex plan allowance",
		group: "subscription"
	},
	{
		id: "synth-cloud-laguna-s",
		label: "Laguna S 2.1",
		description: "Synth Cloud · usage tracked",
		group: "cloud"
	},
	{
		id: "synth-cloud-muse-spark",
		label: "Muse Spark 1.2",
		description: "Synth Cloud · usage tracked",
		group: "cloud"
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

/** Intern Live/Background stay in the catalog for fixtures, but are hidden from v0.1 pickers. */
export function isInternTargetId(id: string): boolean {
	return id === "intern-sync" || id === "intern-async";
}

/** Targets shown in Composer / Landing model menus for the v0.1 launch. */
export const LAUNCH_PICKER_TARGETS: ExecutionTargetOption[] = EXECUTION_TARGETS.filter(
	(target) => !isInternTargetId(target.id)
);

/** @deprecated Prefer openrouter-laguna-s — kept for fixture compatibility. */
export const OPENROUTER_POOLSIDE_TARGET_ID = "openrouter-laguna-s";

export const TARGET_GROUP_LABEL: Record<ExecutionTargetOption["group"], string> = {
	local: "Local",
	remote: "Remote · OpenRouter",
	subscription: "ChatGPT · subscription",
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
