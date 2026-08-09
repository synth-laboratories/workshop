export const RUNTIME_PROTOCOL_VERSION = "synth.desktop-runtime.v1" as const;
export const APP_EVENT_SCHEMA_VERSION = "synth.desktop-app-event.v1" as const;
export const VISUAL_SCHEMA_VERSION = "synth.desktop-visual.v1" as const;
export const RUNTIME_EVENT_SCHEMA_VERSION = "synth.desktop-runtime-event.v1" as const;

export type LocalExecutionTarget = {
  kind: "local";
  model: "laguna-xs-2.1";
  adapter: string | null;
};

export type RemoteExecutionTarget = {
  kind: "remote";
  provider: "openrouter";
  model: string;
  adapter: string | null;
};

export type InternExecutionTarget = {
  kind: "intern";
  mode: "sync" | "async";
  binding?: {
    factoryId?: string | null;
    projectId?: string | null;
    effortId?: string | null;
    runId?: string | null;
  };
};

export type ExecutionTarget =
  | LocalExecutionTarget
  | RemoteExecutionTarget
  | InternExecutionTarget;

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
  target: ExecutionTarget;
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

export type EventSource =
  | "local"
  | "remote"
  | "intern"
  | "codex"
  | "system"
  | "mlx"
  | "visual";

/**
 * Unified durable journal event owned by the Rust CoreRuntime.
 * React projections and MCP must reconcile against committed AppEvents.
 */
export type AppEvent = {
  schemaVersion: typeof APP_EVENT_SCHEMA_VERSION;
  sequence: number;
  eventId: string;
  sessionId?: string | null;
  /** Per-session cursor for UI replay compatibility with RuntimeEvent.sequence. */
  sessionSequence?: number | null;
  runId?: string | null;
  source: EventSource;
  kind: string;
  payload: Record<string, unknown>;
  remoteSequence?: number | null;
  commandId?: string | null;
  createdAt: string;
};

export type EventPage = {
  events: RuntimeEvent[];
  nextSequence: number;
};

export type AppEventPage = {
  events: AppEvent[];
  nextSequence: number;
};

export type VisualStatus = "draft" | "live" | "saved" | "failed" | "archived";
export type RendererKind = "template" | "tsx" | "html";
export type VisualBindingKind = "inline" | "trace_v5" | "local_cas" | "run_ref" | "live_sse" | "fixture";
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

export type VisualRecord = {
  schemaVersion: typeof VISUAL_SCHEMA_VERSION;
  id: string;
  currentRevision: number;
  title: string;
  templateId: string;
  status: VisualStatus;
  rendererKind: RendererKind;
  /** Canonical envelope for new records; plain objects remain readable during migration. */
  bindings: VisualBindings | Record<string, unknown>;
  sessionId?: string | null;
  messageId?: string | null;
  runId?: string | null;
  traceId?: string | null;
  parentVisualId?: string | null;
  sourceAgentId?: string | null;
  sourceModel?: string | null;
  contentDigest?: string | null;
  previewDigest?: string | null;
  metadata: Record<string, unknown>;
  createdAt: string;
  updatedAt: string;
};

export type VisualRevision = {
  visualId: string;
  revision: number;
  templateId: string;
  rendererKind: RendererKind;
  contentDigest?: string | null;
  bindingsDigest?: string | null;
  bindings?: Record<string, unknown> | null;
  previewDigest?: string | null;
  authorAgentId?: string | null;
  parentRevision?: number | null;
  createdAt: string;
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

export type ContainerDeployment = {
  id: string;
  name: string;
  location: "local" | "cloud";
  status: "pending" | "starting" | "ready" | "unhealthy" | "stopped" | "failed";
  baseUrl?: string | null;
  poolId?: string | null;
  taskFamily?: string | null;
  lastRolloutId?: string | null;
  health?: Record<string, unknown>;
  createdAt: string;
  updatedAt: string;
  metadata: Record<string, unknown>;
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

export type TraceBundleIngestRequest = {
  sourcePath: string;
  sourceKind?: string | null;
  sourceUri?: string | null;
  title?: string | null;
};

export type TraceBundleIngestResult = {
  compatibilityLevel: "native" | "legacy_native" | "migrated" | "opaque" | "partial" | "invalid" | string;
  trusted: boolean;
  duplicate: boolean;
  inputDigest: string;
  bundleDigest?: string | null;
  archiveDigest?: string | null;
  traces: TraceV5Record[];
  validation: {
    valid?: boolean;
    self_contained?: boolean | null;
    issues?: Array<Record<string, unknown>>;
    [key: string]: unknown;
  };
};

export type ResolvedTraceProjection = {
  traceDigest: string;
  projectionKind: string;
  projectionSchema: string;
  payloadDigest: string;
  relativePath: string;
  payload: unknown;
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
    mode: "stub" | "mlx";
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

export type CoreDiagnostics = {
  databasePath: string;
  schemaVersion: number;
  integrityOk: boolean;
  contentStorePath: string;
  journalHead: number;
  sessionCount: number;
  runCount: number;
  visualCount: number;
  migrationComplete: boolean;
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
export type InternSessionCreateRequest = {
  target: InternExecutionTarget;
  /** Nonempty task statement required by the cloud Intern contract. */
  objective: string;
  title?: string;
  projectId?: string | null;
};

export type InternSessionSendRequest = {
  sessionId: string;
  body: string;
};

export type InternSessionSendResult = {
  runId: string;
};

export type InternSessionControlRequest = {
  sessionId: string;
  kind: RuntimeControlKind;
  payload?: Record<string, unknown>;
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

export function targetLabel(target: ExecutionTarget): string {
  if (target.kind === "local") {
    return target.adapter ? `Laguna XS 2.1 · ${target.adapter}` : "Laguna XS 2.1";
  }
  if (target.kind === "remote") {
    const short = target.model.split("/").pop() ?? target.model;
    return `OpenRouter · ${short}`;
  }
  return target.mode === "sync" ? "Intern · Live" : "Intern · Background";
}

export function targetMode(target: ExecutionTarget): Run["mode"] {
  if (target.kind === "local") return "local";
  if (target.kind === "remote") return "remote";
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
    payload: event.payload,
    commandId: event.commandId ?? null,
    createdAt: event.createdAt,
    source
  };
}
