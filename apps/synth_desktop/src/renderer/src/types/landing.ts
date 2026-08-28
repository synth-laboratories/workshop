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
	/** Exact provider slug where this target is remote. */
	modelId?: string;
	/** Native catalog state; false means visible but not selectable for new turns. */
	selectable?: boolean;
	availability?: "ready" | "credential_required" | "unverified" | "unavailable" | "expired";
	source?: "builtin" | "user_config";
	diagnostic?: string | null;
	/** Where tokens run — local Metal, remote API, or Synth Cloud Intern. */
	group: "local" | "remote" | "subscription" | "cloud";
};

export type ModelAccessKind = "local" | "api" | "chatgpt";

export const MODEL_ACCESS_ORDER: ModelAccessKind[] = ["local", "api", "chatgpt"];
export const MODEL_ACCESS_LABEL: Record<ModelAccessKind, string> = {
	local: "Local",
	api: "API",
	chatgpt: "ChatGPT"
};

export function modelAccessForTarget(target: ExecutionTargetOption): ModelAccessKind {
	if (target.group === "local") return "local";
	if (target.group === "subscription") return "chatgpt";
	return "api";
}

export function apiProviderForTarget(target: ExecutionTargetOption): "Synth" | "OpenRouter" | null {
	if (target.group === "remote") return "OpenRouter";
	if (target.group === "cloud") return "Synth";
	return null;
}

export type DefaultModelPreference = {
	model: string;
	effort: string;
	providers: string[];
};

/** Resolve the TOML-authored provider fallback chain without crossing auth boundaries. */
export function resolveDefaultTargetId(
	preference: DefaultModelPreference,
	availability: { chatgpt: boolean; openrouter: boolean; synth?: boolean },
	usage: Array<{ targetId: string; updatedAt: string }> = [],
	targets: ExecutionTargetOption[] = LAUNCH_PICKER_TARGETS
): string {
	const model = preference.model.toLowerCase();
	for (const provider of preference.providers) {
		if (provider === "chatgpt" && availability.chatgpt && model === CHATGPT_LUNA_MODEL) return "chatgpt-luna";
		if (provider === "openrouter" && availability.openrouter && model === CHATGPT_LUNA_MODEL) {
			return targets.find((target) => target.modelId === "openai/gpt-5.6-luna" && target.selectable !== false)?.id ?? "local-laguna";
		}
		if (provider === "openrouter" && availability.openrouter) {
			const configured = targets.find((target) => target.modelId?.toLowerCase() === model && target.selectable !== false);
			if (configured) return configured.id;
		}
	}
	const usable = (targetId: string) => {
		if (targetId.startsWith("chatgpt-")) return availability.chatgpt;
		if (isOpenRouterTargetId(targetId)) return availability.openrouter;
		if (targetId.startsWith("synth-cloud-")) return availability.synth === true;
		return targetId === "local-laguna";
	};
	const ranked = new Map<string, { count: number; lastUsed: number }>();
	for (const record of usage) {
		if (!targets.some((target) => target.id === record.targetId && target.selectable !== false) || !usable(record.targetId)) continue;
		const current = ranked.get(record.targetId) ?? { count: 0, lastUsed: 0 };
		ranked.set(record.targetId, {
			count: current.count + 1,
			lastUsed: Math.max(current.lastUsed, Date.parse(record.updatedAt) || 0)
		});
	}
	const mostUsed = [...ranked].sort((a, b) => b[1].count - a[1].count || b[1].lastUsed - a[1].lastUsed)[0]?.[0];
	if (mostUsed) return mostUsed;
	return "local-laguna";
}

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
	/** Digest of the exact visual content revision, used to bind annotations. */
	contentDigest?: string;
	bindings?: import("@synth/runtime-protocol").VisualBindings | Record<string, unknown>;
	/** Durable visual metadata, including presentation and authoring review receipts. */
	metadata?: Record<string, unknown>;
	/** Session that authored this visual. Read-only display never copies this. */
	ownerSessionId?: string;
	/** Workshop session id from VisualRecord. Display/follow only; not Outputs ownership. */
	sessionId?: string;
	/** Optimizer run id from VisualRecord. Local follow only. */
	runId?: string;
	/** Local trace id from VisualRecord. Data traces follow only. */
	traceId?: string;
	/** Cross-task discovery is labeled, never adopted as this chat's output. */
	foreign?: boolean;
	/** Durable VisualStatus. Review receipts stay on metadata; they are not a fourth vocab. */
	status?: ArtifactRefStatus;
	/** Seal receipt digest for this revision, when a VisualSeal exists. Not contentDigest. */
	receiptDigest?: string;
};

export function formatVisualAdmissionIdentity(input: {
	visualId?: string | null;
	revision?: number | string | null;
	receiptDigest?: string | null;
	contentDigest?: string | null;
}): string {
	const visualId = input.visualId?.trim() || "vis —";
	const revision =
		input.revision === 0 || input.revision
			? `rev ${input.revision}`
			: "rev —";
	const digest = input.receiptDigest?.trim()
		? `receipt ${input.receiptDigest.slice(0, 8)}`
		: input.contentDigest?.trim()
			? `content ${input.contentDigest.slice(0, 8)}`
			: "digest —";
	return `${visualId} · ${revision} · ${digest}`;
}

export const VISUAL_OPS_NOT_A_WORKSHOP_ROUTE = "not a Workshop route";

export type VisualOpsKind = "session" | "run" | "trace";

export type VisualOpsRoute =
	| "missing"
	| "workshop-session"
	| "optimizer-run"
	| "local-trace"
	| "not-a-workshop-route";

/** Local disk is the default Workshop space. Intern/Shoal/Modal are not routes. */
export function classifyVisualOpsRoute(
	kind: VisualOpsKind,
	id: string | null | undefined,
	locallyOpenable: boolean | null = null
): VisualOpsRoute {
	if (!id?.trim()) return "missing";
	if (locallyOpenable === false) return "not-a-workshop-route";
	if (kind === "session") return "workshop-session";
	if (kind === "run") return "optimizer-run";
	return "local-trace";
}

export function visualOpsSpaceLabel(route: VisualOpsRoute): string | null {
	if (route === "workshop-session") return "Workshop session";
	if (route === "optimizer-run") return "optimizer run";
	if (route === "local-trace") return "local trace";
	if (route === "not-a-workshop-route") return VISUAL_OPS_NOT_A_WORKSHOP_ROUTE;
	return null;
}

export function formatVisualOpsPart(
	kind: VisualOpsKind,
	id: string | null | undefined,
	locallyOpenable: boolean | null = null
): string {
	const route = classifyVisualOpsRoute(kind, id, locallyOpenable);
	const value = id?.trim() || "—";
	if (route === "missing") return `${kind} —`;
	const space = visualOpsSpaceLabel(route);
	return space ? `${kind} ${value} · ${space}` : `${kind} ${value}`;
}

export function formatVisualOpsIdentity(input: {
	sessionId?: string | null;
	runId?: string | null;
	traceId?: string | null;
	sessionOpenable?: boolean | null;
	runOpenable?: boolean | null;
	traceOpenable?: boolean | null;
}): string {
	return [
		formatVisualOpsPart("session", input.sessionId, input.sessionOpenable ?? null),
		formatVisualOpsPart("run", input.runId, input.runOpenable ?? null),
		formatVisualOpsPart("trace", input.traceId, input.traceOpenable ?? null)
	].join(" · ");
}

/** Same machine as `VisualStatus` on VisualRecord. Viewer pointer, not a second lifecycle. */
export const ARTIFACT_REF_STATUSES = ["draft", "live", "saved", "failed", "archived"] as const;
export type ArtifactRefStatus = (typeof ARTIFACT_REF_STATUSES)[number];

export function parseArtifactRefStatus(value: unknown): ArtifactRefStatus {
	return ARTIFACT_REF_STATUSES.includes(value as ArtifactRefStatus)
		? (value as ArtifactRefStatus)
		: "draft";
}

/** Inline activity line in a local Laguna transcript (Poolside-style). */
export type LocalActivityLine = {
	id: string;
	label: string;
	/** Correlates a pending approval with its durable grant/rejection event. */
	approvalId?: string;
	// Mirrors `ApprovalKind::as_str` in src-tauri/src/session/approval.rs.
	approvalKind?: "shell_command" | "paid_compute" | "sidecar_lifecycle" | "container_lifecycle" | "credential_access" | "plugin_lifecycle" | "computer_use" | "visual_template_persist" | "permission";
	approvalPayload?: {
		operation?: string;
		parameters?: Record<string, unknown>;
		estimatedCostUsdMicros?: number;
		requestedCap?: { maxCostUsdMicros?: number; maxRollouts?: number };
		requestingAgent?: string;
		provider?: string;
		purpose?: string;
	};
	alwaysAllowSupported?: boolean;
	/** Expanded raw detail (tool output / thought) when the line is opened. */
	detail?: string;
	/**
	 * Bounded, inspectable payloads associated with an activity.  These are
	 * normalized before they reach React so nested runtime values can never
	 * accidentally stringify to `[object Object]` in a transcript.
	 */
	inspectable?: Array<{
		label: string;
		value: string;
		format: "json" | "text";
		truncated?: boolean;
		unavailable?: boolean;
	}>;
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
	toolStatus?: "running" | "completed" | "failed" | "cancelled";
	/** Provider-reported wall duration for a finished tool call. */
	durationMs?: number;
	/** User-facing visual authoring milestone; never a raw tool operation. */
	visualStage?: "draft" | "review" | "ready" | "failed";
	/** Transcript placement relative to the assistant response owning this activity. */
	placement?: "before" | "after";
	/** Source event sequence — used by the placement chronology invariant. */
	sequence?: number;
	/** Child thread opened by this delegation activity. */
	subagentId?: string;
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
	/** ChatGPT subscription OAuth present in Workshop's private credential file. */
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

/** Synth-hosted Shoal routes. Hardware/precision stay explicit in the id. */
export const SYNTH_CLOUD_LAGUNA_S_MODEL = "synth_internal/laguna-s-2.1-nvfp4";
export const SYNTH_CLOUD_LAGUNA_XS_B200_MODEL = "synth_internal/laguna-xs-2.1-nvfp4";
export const SYNTH_CLOUD_LAGUNA_XS_H100_MODEL = "synth_internal/laguna-xs-2.1-fp8-h100";
export const SYNTH_CLOUD_MUSE_SPARK_MODEL = "meta/muse-spark-1.2";
export const CHATGPT_LUNA_MODEL = "gpt-5.6-luna";
export const CHATGPT_SOL_MODEL = "gpt-5.6-sol";
export const CHATGPT_TERRA_MODEL = "gpt-5.6-terra";

export function isOpenRouterTargetId(id: string): boolean {
	return id.startsWith("openrouter-") || id.startsWith("openrouter:");
}

const BUILTIN_EXECUTION_TARGETS: ExecutionTargetOption[] = [
	{
		id: "local-laguna",
		label: "Laguna XS 2.1",
		description: "Local · MLX · Metal · usage tracked",
		group: "local"
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
		description: "Synth Cloud · B200 · usage tracked",
		group: "cloud"
	},
	{
		id: "synth-cloud-laguna-xs-b200",
		label: "Laguna XS 2.1 · B200",
		description: "Synth Cloud · B200 · usage tracked",
		group: "cloud"
	},
	{
		id: "synth-cloud-laguna-xs-h100",
		label: "Laguna XS 2.1 · H100",
		description: "Synth Cloud · H100 option · usage tracked",
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

/**
 * The OpenRouter portion is replaced by the native catalog once it arrives.
 * Keeping the non-OpenRouter targets source-owned avoids moving unrelated
 * local, ChatGPT, Synth Cloud, and Intern policies into config.toml.
 */
export let EXECUTION_TARGETS: ExecutionTargetOption[] = [...BUILTIN_EXECUTION_TARGETS];

/** Intern Live/Background stay in the catalog for fixtures, but are hidden from v0.1 pickers. */
export function isInternTargetId(id: string): boolean {
	return id === "intern-sync" || id === "intern-async";
}

/** Targets shown in Composer / Landing model menus for the v0.1 launch. */
export let LAUNCH_PICKER_TARGETS: ExecutionTargetOption[] = EXECUTION_TARGETS.filter(
	(target) => !isInternTargetId(target.id)
);

export function replaceOpenRouterExecutionTargets(entries: ExecutionTargetOption[]): void {
	const sourceOwned = BUILTIN_EXECUTION_TARGETS.filter((target) => !isOpenRouterTargetId(target.id));
	EXECUTION_TARGETS = [...sourceOwned.slice(0, 1), ...entries, ...sourceOwned.slice(1)];
	LAUNCH_PICKER_TARGETS = EXECUTION_TARGETS.filter((target) => !isInternTargetId(target.id));
}

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
