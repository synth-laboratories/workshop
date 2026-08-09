export const RUNTIME_PROTOCOL_VERSION = "synth.desktop-runtime.v1" as const;

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

export type RuntimeEvent = {
  schemaVersion: "synth.desktop-runtime-event.v1";
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

export type EventPage = {
  events: RuntimeEvent[];
  nextSequence: number;
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

export type RuntimeControlKind =
  | "cancel"
  | "pause"
  | "resume"
  | "close"
  | "request_checkpoint"
  | "approve"
  | "reject"
  | "set_approval_mode";

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
