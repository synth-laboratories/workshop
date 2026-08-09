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

/**
 * First-class adapter identity (base + LoRA), not an opaque merged model name.
 * Local Metal first; remote OpenRouter / cloud inference next.
 */
export type LoraAdapter = {
	id: string;
	/** File-ish name, e.g. craftax-triage.lora */
	name: string;
	displayName: string;
	/** Base execution target this adapter attaches to. */
	baseTargetId: string;
	revision: string;
	digest: string;
	status: "ready" | "downloading" | "training";
	scope: "local" | "remote";
	summary: string;
};

/** Base = no adapter. */
export const LORA_NONE = "base";

export const AVAILABLE_LORAS: LoraAdapter[] = [
	{
		id: "lora-craftax-triage",
		name: "craftax-triage.lora",
		displayName: "Craftax triage",
		baseTargetId: "local-laguna",
		revision: "r12",
		digest: "sha256:a1c4…9f2",
		status: "ready",
		scope: "local",
		summary: "Failure clustering + harness diffs for Craftax rollouts"
	},
	{
		id: "lora-company-code",
		name: "company-code.lora",
		displayName: "Company code",
		baseTargetId: "local-laguna",
		revision: "r7",
		digest: "sha256:b82e…11d",
		status: "ready",
		scope: "local",
		summary: "Internal repo conventions, review tone, and tooling"
	},
	{
		id: "lora-data-agent",
		name: "data-agent.lora",
		displayName: "Data agent",
		baseTargetId: "local-laguna",
		revision: "r4",
		digest: "sha256:c0ff…ee1",
		status: "ready",
		scope: "local",
		summary: "SQL / dataframe / eval metrics workflows"
	},
	{
		id: "lora-user-custom",
		name: "user-custom-r17.lora",
		displayName: "User custom",
		baseTargetId: "local-laguna",
		revision: "r17",
		digest: "sha256:d441…77a",
		status: "training",
		scope: "local",
		summary: "Personal finetune in progress — not hot-swappable yet"
	},
	{
		id: "lora-poolside-craftax",
		name: "craftax-remote.lora",
		displayName: "Craftax (remote)",
		baseTargetId: "openrouter-laguna-s",
		revision: "r3",
		digest: "sha256:e991…02b",
		status: "ready",
		scope: "remote",
		summary: "Same adapter identity on OpenRouter Laguna S 2.1 — usage tracked"
	}
];

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
	bindings?: Record<string, unknown>;
};

/** Inline activity line in a local Laguna transcript (Poolside-style). */
export type LocalActivityLine = {
	id: string;
	label: string;
	/** Expanded raw detail (tool output / thought) when the line is opened. */
	detail?: string;
	/**
	 * Optional file path for read/write lines — drives Poolside-style file-type icons
	 * (.md, .rs, .ts, .toml, …).
	 */
	path?: string;
	kind?: "thought" | "search" | "command" | "file_read" | "file_write" | "visual" | "working";
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
	/** Active LoRA adapter id, or `base` for none. */
	selectedLoraId: string;
	composerEnabled: boolean;
	composerPlaceholder: string;
};

/** OpenRouter model ids used by the remote ACP adapter. */
export const OPENROUTER_LUNA_MODEL = "moonshotai/kimi-k2.5";
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
		label: "Luna",
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
