export const RUNTIME_PROTOCOL_VERSION = "synth.desktop-runtime.v1" as const;
export const APP_EVENT_SCHEMA_VERSION = "synth.desktop-app-event.v1" as const;
export const VISUAL_SCHEMA_VERSION = "synth.desktop-visual.v1" as const;
export const RUNTIME_EVENT_SCHEMA_VERSION = "synth.desktop-runtime-event.v1" as const;

import type {
  RecoveryActivity,
  RecoveryPrompt,
  RecoveryNotice,
  EventSource,
  AppEvent,
  VisualStatus,
  RendererKind,
  VisualRecord,
  VisualRevision,
  ContainerDeployment,
  TraceBundleIngestRequest,
  TraceBundleIngestResult,
  ResolvedTraceProjection,
  UsageBreakdown,
  UsageDayPoint,
  UsageSummary,
  OptimizerCapabilities,
  OptimizerUsageSummary,
  OptimizerResourceRef,
  OptimizerExecutionBinding,
  OptimizerRunRecord,
  CoreDiagnostics,
  InternSessionCreateRequest,
  InternSessionSendRequest,
  InternSessionControlRequest
} from "../../../apps/synth_desktop/src/renderer/src/generated/protocol";

export type {
  RecoveryActivity,
  RecoveryPrompt,
  RecoveryNotice,
  EventSource,
  AppEvent,
  VisualStatus,
  RendererKind,
  VisualRecord,
  VisualRevision,
  ContainerDeployment,
  TraceBundleIngestRequest,
  TraceBundleIngestResult,
  ResolvedTraceProjection,
  UsageBreakdown,
  UsageDayPoint,
  UsageSummary,
  OptimizerCapabilities,
  OptimizerUsageSummary,
  OptimizerResourceRef,
  OptimizerExecutionBinding,
  OptimizerRunRecord,
  CoreDiagnostics,
  InternSessionCreateRequest,
  InternSessionSendRequest,
  InternSessionControlRequest
};


/** On-device Laguna (MLX). Maps to Rust `RuntimeTarget::LocalRuntime`. */
export type LocalRuntimeTarget = {
  kind: "local";
  /** The daemon policy this session pins: the base model id, or the model id a
   *  registered LoRA is served under. It rides on every request, which is what
   *  keeps one conversation's policy out of another's turn. */
  model: string;
  /** Catalog identity (`sha256:…`) of the adapter behind a non-base policy,
   *  kept for lineage. Selection is by `model`, never by this. */
  adapter: string | null;
};

/** OpenRouter-hosted models. Maps to Rust `RuntimeTarget::RemoteRuntime`. */
export type RemoteRuntimeTarget = {
  kind: "remote";
  /** Present on wire for renderer compatibility; always openrouter when set. */
  provider?: "openrouter" | "openai-codex-oauth";
  model: string;
  adapter: string | null;
  /** Stable native catalog identity. Model slug remains the request authority. */
  targetId?: string | null;
};

/** Synth gateway. Maps to Rust `RuntimeTarget::CloudRuntime`. */
export type CloudRuntimeTarget = {
  kind: "cloud";
  model: string;
  adapter: string | null;
};

/** Synth Intern sync|async. Maps to Rust `RuntimeTarget::InternRuntime`. */
export type InternRuntimeTarget = {
  kind: "intern";
  mode: "sync" | "async";
  binding?: {
    factoryId?: string | null;
    projectId?: string | null;
    effortId?: string | null;
    runId?: string | null;
  };
};

/**
 * Where a session runs inference / agent substrate.
 * Distinct from SessionKind (Codex | Intern — Wave 1).
 */
export type RuntimeTarget =
  | LocalRuntimeTarget
  | RemoteRuntimeTarget
  | CloudRuntimeTarget
  | InternRuntimeTarget;

/** @deprecated Historical synonym — prefer {@link RuntimeTarget}. */
export type ExecutionTarget = RuntimeTarget;
/** @deprecated Prefer {@link LocalRuntimeTarget}. */
export type LocalExecutionTarget = LocalRuntimeTarget;
/**
 * @deprecated Prefer {@link RemoteRuntimeTarget} or {@link CloudRuntimeTarget}.
 * Legacy payloads used `provider: "synth-cloud"` under `kind: "remote"`.
 */
export type RemoteExecutionTarget = {
  kind: "remote";
  provider: "openrouter" | "synth-cloud" | "openai-codex-oauth";
  model: string;
  adapter: string | null;
  /** Stable UI target identity retained when reading older runtime payloads. */
  targetId?: string | null;
};
/** @deprecated Prefer {@link InternRuntimeTarget}. */
export type InternExecutionTarget = InternRuntimeTarget;

/** Accept legacy remote+synth-cloud bags and emit canonical RuntimeTarget. */
export function normalizeRuntimeTarget(
  target: RuntimeTarget | RemoteExecutionTarget
): RuntimeTarget {
  if (target.kind === "remote" && target.provider === "synth-cloud") {
    return {
      kind: "cloud",
      model: target.model,
      adapter: target.adapter
    };
  }
  if (target.kind === "remote") {
    return {
      kind: "remote",
      provider: "openrouter",
      model: target.model,
      adapter: target.adapter,
      targetId: target.targetId
    };
  }
  return target;
}

export type SessionStatus =
  | "created"
  | "ready"
  | "running"
  | "waiting_for_input"
  | "paused"
  | "interrupted"
  | "completed"
  | "failed"
  | "cancelled"
  | "configuration_required";

export type RunStatus =
  | "queued"
  | "starting"
  | "running"
  | "waiting_for_input"
  | "completed"
  | "failed"
  | "cancelled";

export type Session = {
  id: string;
  title: string;
  target: RuntimeTarget;
  projectId?: string | null;
  remoteId?: string | null;
  createdAt: string;
  updatedAt: string;
  status: SessionStatus;
  stateGeneration?: number | null;
  latestCursor: number;
  activeRunId?: string | null;
  metadata: Record<string, unknown>;
};

/** The last thing an abandoned turn durably did before its owner disappeared. */

/** The operator prompt that produced an abandoned turn, so Restart can reuse it. */

/**
 * Written by the host when a previous process died holding a turn. Delivered
 * both on `session.metadata.recovery` and as a `session/recovery_required`
 * event, so a client that missed the event still learns what happened.
 */

/**
 * What a chat is *doing*, as opposed to what its last persisted status was.
 *
 * `working` is deliberately unreachable from stored state alone: it requires a
 * live turn owned by the current Workshop instance. Everything a crash can
 * leave behind lands on `interrupted` or `needsAttention` instead.
 */
export type ChatPresence =
  | "idle"
  | "starting"
  | "working"
  | "recovering"
  | "interrupted"
  | "needsAttention";

export type Run = {
  id: string;
  sessionId: string;
  mode: "local" | "remote" | "sync" | "async";
  status: RunStatus;
  latestCursor: number;
  createdAt: string;
  startedAt?: string | null;
  completedAt?: string | null;
  checkpoint?: unknown;
  outcome?: unknown;
  model?: string | null;
  adapter?: string | null;
  metadata: Record<string, unknown>;
};

export type ArtifactRef = {
  kind: string;
  id?: string;
  uri?: string;
  title?: string;
  mediaType?: string;
  templateId?: string;
  metadata?: Record<string, unknown>;
};

export type Metric = {
  name: string;
  value: number;
  unit?: string;
  step?: number;
};

/** Legacy Python-runtime event wire shape. Prefer AppEvent for new Rust journal traffic. */
export type RuntimeEvent = {
  schemaVersion: typeof RUNTIME_EVENT_SCHEMA_VERSION;
  sessionId: string;
  runId?: string | null;
  sequence: number;
  remoteSequence?: number | null;
  eventKind: string;
  payload: Record<string, unknown>;
  commandId?: string | null;
  createdAt: string;
  source: "local" | "remote" | "intern" | "system";
};

/**
 * Who produced a boundary event (Wave 2).
 * Matches Rust `contract::events::EventOrigin` — Provider (codex/app-server)
 * vs Desktop (synthetic session/approval/health). Live Codex notifications
 * share the single origin-tagged `runtime:event` channel.
 */
export type EventOrigin = "provider" | "desktop";

/**
 * Unified durable journal event owned by the Rust CoreRuntime.
 * React projections and MCP must reconcile against committed AppEvents.
 */

export type EventPage = {
  events: RuntimeEvent[];
  nextSequence: number;
};

export type AppEventPage = {
  events: AppEvent[];
  nextSequence: number;
};

export type VisualBindingKind =
  | "inline"
  | "trace_v5"
  | "local_cas"
  | "run_ref"
  | "live_sse"
  | "fixture"
  | "optimizer_run"
  | "query_snapshot";
export type VisualBinding = {
  slot: string;
  kind: VisualBindingKind;
  source?: string;
  data?: unknown;
  path?: string;
  schema?: string;
};
export type VisualBindings = {
  schemaVersion: "synth.visual-bindings.v1";
  slots: VisualBinding[];
};

export type VisualReference = {
  kind: "visual_ref";
  visualId: string;
};

export type CodexActivityEvent = {
  sessionId: string;
  executionId: string;
  streamId: string;
  eventKind: string;
  payload: Record<string, unknown>;
  createdAt: string;
};

export type TraceV5Record = {
  id: string;
  digest: string;
  title: string;
  source: "local" | "cloud" | "import";
  containerId?: string | null;
  sessionId?: string | null;
  runId?: string | null;
  reward?: number | null;
  metrics: Metric[];
  createdAt: string;
  path?: string | null;
  metadata: Record<string, unknown>;
};

/** @deprecated Prefer VisualRecord from the Rust Visual Registry. */
export type VisualInstanceRecord = {
  id: string;
  templateId: string;
  title: string;
  bindings: Record<string, unknown>;
  tsxPath?: string | null;
  createdAt: string;
  updatedAt: string;
  metadata: Record<string, unknown>;
};

export type UsageLedgerEntry = {
  id: string;
  provider: string;
  model: string;
  sessionId?: string | null;
  runId?: string | null;
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
  costUsd?: number | null;
  createdAt: string;
};

export type UsageWindow = "today" | "7d" | "30d" | "all";

export type UsageCostSource =
  | "provider_reported"
  | "synth_cloud"
  | "tariff_estimate"
  | "none";

/**
 * One aggregated usage slice — the device total or one (provider, model)
 * pair — reduced natively over the per-request `usage_records` ledger.
 * Nullable fields mean "never reported", which the UI renders as
 * Unavailable rather than zero. `billedCostUsd` is settled money only;
 * `estimatedCostUsd` covers exactly the requests that have no settled
 * charge, so the two never double-count a request.
 */

/**
 * One local calendar day for one provider, reduced from the same ledger rows
 * as the window totals, so a daily chart can never disagree with the headline
 * it sits under. The provider rides on `totals.provider`; `totals.modelId` is
 * `"all"` because a day point spans every model that provider ran.
 */

export type OptimizerAlgorithmInfo = {
  id: string;
  title: string;
  availability: "available" | "private_beta" | "unavailable" | string;
  description?: string;
};

export type Project = {
  id: string;
  name: string;
  path: string;
  vcs?: string | null;
  metadata: Record<string, unknown>;
  createdAt: string;
  updatedAt: string;
};

export type RuntimeHealth = {
  status: "ok";
  protocolVersion: typeof RUNTIME_PROTOCOL_VERSION;
  runtimeId: string;
  startedAt: string;
  intern: {
    mode: "remote" | "demo" | "unconfigured";
    backendUrl?: string | null;
  };
  local: {
    model: "laguna-xs-2.1";
    mode: "absent" | "mlx";
    modelPath?: string | null;
  };
  openrouter: {
    mode: "ready" | "unconfigured";
    models: string[];
  };
  inventory: {
    containers: number;
    traces: number;
    visuals: number;
  };
  dataStore?: {
    path: string;
    projects: number;
    sessions: number;
    runs: number;
    events: number;
    usage: number;
  };
};

export type RuntimeControlKind =
  | "cancel"
  | "pause"
  | "resume"
  | "close"
  | "request_checkpoint"
  | "approve"
  | "reject"
  | "set_approval_mode";

/** Renderer-to-CoreRuntime contract for Rust-owned Intern sessions. */

export type InternSessionSendResult = {
  runId: string;
};

export type InternSessionControlResult = {
  accepted: boolean;
  receipt?: unknown;
};

export type SemanticUiSnapshot = {
  schemaVersion: "synth.desktop-semantic-ui.v1";
  selectedSessionId: string | null;
  sessions: Session[];
  visibleEvents: RuntimeEvent[];
  openVisualId: string | null;
  inventoryTab: "containers" | "traces" | "visuals" | null;
  controls: Array<{
    id: string;
    role: string;
    name: string;
    enabled: boolean;
  }>;
};

export function targetLabel(target: RuntimeTarget): string {
  if (target.kind === "local") {
    return target.adapter ? `Laguna XS 2.1 · ${target.adapter}` : "Laguna XS 2.1";
  }
  if (target.kind === "cloud") {
    const short = target.model.split("/").pop() ?? target.model;
    return `Synth Cloud · ${short}`;
  }
  if (target.kind === "remote") {
    const short = target.model.split("/").pop() ?? target.model;
    return `OpenRouter · ${short}`;
  }
  return target.mode === "sync" ? "Intern · Live" : "Intern · Background";
}

export function targetMode(target: RuntimeTarget): Run["mode"] {
  if (target.kind === "local") return "local";
  if (target.kind === "remote" || target.kind === "cloud") return "remote";
  return target.mode;
}

/** Project a durable AppEvent into the legacy RuntimeEvent shape when session-scoped. */
export function appEventToRuntimeEvent(event: AppEvent): RuntimeEvent | null {
  if (!event.sessionId) return null;
  const sequence = event.sessionSequence ?? event.sequence;
  const source: RuntimeEvent["source"] =
    event.source === "codex" || event.source === "mlx" || event.source === "visual"
      ? "local"
      : event.source === "remote" || event.source === "intern" || event.source === "system"
        ? event.source
        : "local";
  return {
    schemaVersion: RUNTIME_EVENT_SCHEMA_VERSION,
    sessionId: event.sessionId,
    runId: event.runId ?? null,
    sequence,
    remoteSequence: event.remoteSequence ?? null,
    eventKind: event.kind,
    payload:
      event.payload && typeof event.payload === "object" && !Array.isArray(event.payload)
        ? (event.payload as Record<string, unknown>)
        : {},
    commandId: event.commandId ?? null,
    createdAt: event.createdAt,
    source
  };
}
