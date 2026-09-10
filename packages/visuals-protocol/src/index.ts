export const VISUALS_PROTOCOL_VERSION = "synth.visuals-core.v1" as const;

export type JsonPrimitive = null | boolean | number | string;
export type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue };

export type Exactness = "exact" | "estimated" | "heuristic" | "model_produced";
export type Completeness = "complete" | "partial" | "unknown";
export type TruthState = "observed" | "missing" | "pending" | "failed" | "redacted" | "not_applicable";

export type Observed<T> =
  | { state: "observed"; value: T; evidence?: string[] }
  | { state: Exclude<TruthState, "observed">; reason?: string; evidence?: string[] };

export type EvidenceSnapshotRef = {
  id: string;
  digest: string;
  completeness: Completeness;
  capturedAt: string;
};

export type SourceAuthority = "authoritative" | "derived" | "advisory" | "untrusted";
export type Freshness = "current" | "stale" | "superseded" | "unknown";

export type EvidenceNode = {
  id: string;
  kind: string;
  schema: string;
  schemaVersion?: string;
  digest?: string;
  cursorRange?: { from?: number | string; through?: number | string };
  authority: SourceAuthority;
  freshness: Freshness;
  completeness: Completeness;
  exactness: Exactness;
  diagnostics?: Diagnostic[];
};

export type EvidenceEdge = {
  from: string;
  to: string;
  relation: "derived_from" | "references" | "overlays" | "supersedes";
  projectorVersion?: string;
};

export type EvidenceGraph = { nodes: EvidenceNode[]; edges: EvidenceEdge[] };

export type Diagnostic = {
  code: string;
  severity: "info" | "warning" | "error";
  message: string;
  remediation?: string;
  evidence?: string[];
};

export type CorpusRef = {
  id: string;
  revision: string;
  schema: string;
  count: number;
  evidenceCut?: EvidenceSnapshotRef;
};

export type Scalar = null | boolean | number | string;
export type QueryComparison = "eq" | "neq" | "gt" | "gte" | "lt" | "lte" | "contains" | "in";
export type QueryExpression =
  | { op: "all" }
  | { op: "and" | "or"; expressions: QueryExpression[] }
  | { op: "not"; expression: QueryExpression }
  | { op: "exists"; field: string; exists?: boolean }
  | { op: "any"; field: string; where: QueryExpression }
  | { op: "sequence"; field: string; sequenceField: string; before: QueryExpression; after: QueryExpression; maxGap?: number }
  | { op: QueryComparison; field: string; value: Scalar | Scalar[] };

export type QueryOrder = { field: string; direction: "asc" | "desc" };
export type QuerySpec = {
  schemaVersion: typeof VISUALS_PROTOCOL_VERSION;
  where: QueryExpression;
  orderBy?: QueryOrder[];
};

export type QueryWindow = { offset: number; limit: number };
export type QueryResult<T> = {
  corpus: CorpusRef;
  query: QuerySpec;
  total: number;
  window: QueryWindow;
  rows: T[];
  exactness: Exactness;
  completeness: Completeness;
  excluded: number;
};

export type SamplingStrategy = "random" | "representative" | "diverse" | "boundary" | "failure" | "outlier";
export type SamplingReceipt = {
  id: string;
  strategy: SamplingStrategy;
  sourceCohortId: string;
  seed?: number;
  requested: number;
  returned: number;
  memberIds: string[];
  exactness: Exactness;
  parameters: Record<string, JsonValue>;
};

export type CohortRef = {
  id: string;
  name: string;
  corpus: CorpusRef;
  query: QuerySpec;
  count: number;
  denominator: number;
  parentCohortId?: string;
  completeness: Completeness;
  exactness: Exactness;
  excluded: number;
  sampling?: SamplingReceipt;
};

export type AggregateBucket = {
  key: string;
  value?: Scalar;
  count: number;
  denominator: number;
  ratio: number;
  query: QuerySpec;
};

export type AggregateResult = {
  corpus: CorpusRef;
  sourceCohortId: string;
  field: string;
  buckets: AggregateBucket[];
  exactness: Exactness;
  completeness: Completeness;
  missing?: number;
};

export type SemanticRef = { kind: string; id: string; parent?: SemanticRef };
export type SelectionSet = { primary?: SemanticRef; members: SemanticRef[] };
export type ClockCursor = { domain: string; value: number | string };
export type CursorSet = Record<string, ClockCursor>;

export type ClockDomain = {
  id: string;
  kind: "sequence" | "discrete" | "continuous" | "instant" | "stage";
  scope: "visual" | "source" | "lane" | "entity";
  unit?: string;
  ordering: "total" | "partial";
};

export type CursorMode = "fixed" | "follow";
export type TemporalCursor = ClockCursor & {
  scopeId?: string;
  mode: CursorMode;
  rangeEnd?: number | string;
};

export type TemporalCorrespondence = {
  from: ClockCursor & { scopeId?: string };
  to: ClockCursor & { scopeId?: string };
  evidence?: string[];
};

export type TimeContract = {
  domains: ClockDomain[];
  correspondences?: TemporalCorrespondence[];
  primaryDomain?: string;
};

export type InteractionTier = "ephemeral" | "presentation" | "overlay" | "domain_effect";
export type ActionDescriptor = {
  kind: string;
  tier: InteractionTier;
  payloadSchema?: string;
  requiredCapability?: string;
  idempotent?: boolean;
};

export type InteractionContract = { actions: ActionDescriptor[] };

export type VisualAction = {
  id: string;
  kind: "query" | "select" | "drill_down" | "back" | "seek" | "pan" | "zoom" | "snapshot" | "annotate" | (string & {});
  target?: SemanticRef;
  payload?: Record<string, JsonValue>;
  expectedStateVersion?: number;
  idempotencyKey?: string;
};

export type ExplorationStep = {
  id: string;
  action: VisualAction;
  from: SemanticRef;
  to: SemanticRef;
  cohort?: CohortRef;
  occurredAt: string;
};

export type ExplorationPath = {
  root: CorpusRef;
  steps: ExplorationStep[];
  current: SemanticRef;
};

export type SemanticLandmark = {
  ref: SemanticRef;
  role: string;
  label: string;
  state?: string;
  relationships?: SemanticRef[];
  actions: VisualAction["kind"][];
};

export type SemanticScene = {
  visualId: string;
  revision: number;
  stateVersion: number;
  clocks: CursorSet;
  selection: SelectionSet;
  landmarks: SemanticLandmark[];
  truth: Record<string, Observed<JsonValue>>;
  diagnostics: string[];
  availableActions?: ActionDescriptor[];
  evidenceGraph?: EvidenceGraph;
};

export type PresentationState = {
  schemaVersion: string;
  revision: number;
  stateVersion: number;
  value: Record<string, JsonValue>;
  digest: string;
  updatedAt: string;
};

export type LifecycleFacetState = "idle" | "pending" | "active" | "complete" | "failed" | "unavailable";
export type LifecycleFacet = { state: LifecycleFacetState; reason?: string; updatedAt?: string };
export type VisualLifecycle = {
  transport: LifecycleFacet;
  projection: LifecycleFacet;
  freshness: LifecycleFacet;
  review: LifecycleFacet;
  readiness: LifecycleFacet;
  pinning: LifecycleFacet;
  sealing: LifecycleFacet;
  sharing: LifecycleFacet;
};

export type VisualSnapshot = {
  id: string;
  visualId: string;
  revision: number;
  stateVersion: number;
  capturedAt: string;
  corpus: CorpusRef;
  cohort: CohortRef;
  exploration: ExplorationPath;
  selection: SelectionSet;
  semanticSceneDigest: string;
  presentationDigest: string;
  renditionRefs: string[];
  evidenceSnapshot?: EvidenceSnapshotRef;
  evidenceGraph?: EvidenceGraph;
  projectionDigest?: string;
  renderer?: ExtensionPin;
  projector?: ExtensionPin;
  viewport?: { width: number; height: number; scaleFactor?: number };
};

export type VisualRecordingEvent = {
  sequence: number;
  occurredAt: string;
  kind: "action" | "cohort_created" | "selection_changed" | "snapshot_captured";
  action?: VisualAction;
  cohort?: CohortRef;
  selection?: SelectionSet;
  snapshotId?: string;
  stateVersion: number;
};

export type VisualRecording = {
  id: string;
  visualId: string;
  startedAt: string;
  endedAt?: string;
  initialSnapshot?: VisualSnapshot;
  events: VisualRecordingEvent[];
  checkpoints: VisualSnapshot[];
  extensionPins?: ExtensionPin[];
  redactions?: Array<{ path: string; reason: string }>;
};

export type Capability =
  | "query"
  | "aggregate"
  | "drill_down"
  | "logical_time"
  | "interaction"
  | "snapshot"
  | "recording"
  | "semantic_scene"
  | "mcp"
  | "static_rendition";

export type ExtensionPin = { id: string; version: string; digest?: string };
export type ExtensionAuthority = "none" | "network" | "filesystem_read" | "filesystem_write" | "process" | "domain_effect";

export type ExtensionDescriptor = {
  id: string;
  version: string;
  protocolRange: string;
  deterministic: boolean;
  authorities: ExtensionAuthority[];
  inputSchemas?: string[];
  outputSchemas?: string[];
  resourceLimits?: { maxInputBytes?: number; maxOutputBytes?: number; timeoutMs?: number };
};

export type VisualInputContract = {
  name: string;
  schemas: string[];
  bindingKinds: string[];
  required: boolean;
  multiple?: boolean;
};

export type ProjectionDescriptor = ExtensionPin & {
  outputSchema: string;
  incremental?: boolean;
};

export type ObservationContract = {
  schemaVersion: string;
  readiness: {
    rejectTransportStates?: string[];
    minimumRolloutCount?: number;
    minimumRenderedFrameCount?: number;
    minimumSemanticEventCount?: number;
    requireTerminal?: boolean;
  };
};

export type VisualDefinition = {
  id: string;
  version: string;
  renderer: string;
  capabilities: Capability[];
  inputSchemas: string[];
  inputs?: VisualInputContract[];
  projection?: ProjectionDescriptor;
  time?: TimeContract;
  interaction?: InteractionContract;
  observation?: ObservationContract;
  presentationSchema?: string;
};

export type RendererDescriptor = {
  id: string;
  version: string;
  isolation: "in_process" | "worker" | "sandboxed_frame";
  formats: string[];
  capabilities: Capability[];
  protocolRange?: string;
  authorities?: ExtensionAuthority[];
  deterministic?: boolean;
};

export type BindingDescriptor = {
  input: string;
  kind: string;
  source?: string;
  data?: JsonValue;
  path?: string;
  schema?: string;
  options?: Record<string, JsonValue>;
};

export type BindingSet = { schemaVersion: string; inputs: BindingDescriptor[] };

export type ResolvedSource<T = JsonValue> = {
  binding: BindingDescriptor;
  evidence: EvidenceNode;
  value?: T;
  contentRef?: string;
};

export type RevisionProvenance = {
  actor?: string;
  parentRevision?: number;
  commandId?: string;
  createdAt: string;
};

export type VisualArtifact = {
  id: string;
  currentRevision: number;
  title: string;
  status: string;
  createdAt: string;
  updatedAt: string;
};

export type VisualRevision = {
  visualId: string;
  revision: number;
  definition: ExtensionPin;
  renderer: ExtensionPin;
  projection?: ExtensionPin;
  contentRef?: string;
  bindings: BindingSet;
  props?: JsonValue;
  presentation?: PresentationState;
  provenance: RevisionProvenance;
};

export type Rendition = {
  id: string;
  visualId: string;
  revision: number;
  format: string;
  mediaType: string;
  contentRef: string;
  renderer: ExtensionPin;
  width?: number;
  height?: number;
  createdAt: string;
};

export type VisualCommand =
  | { kind: "interact"; visualId: string; action: VisualAction }
  | { kind: "capture_snapshot"; visualId: string; renditionRefs?: string[] }
  | { kind: "start_recording" | "stop_recording"; visualId: string };

export type VisualQuery =
  | { kind: "inspect"; visualId: string }
  | { kind: "get_recording"; visualId: string };

export type CommandReceipt = {
  commandId: string;
  visualId: string;
  accepted: boolean;
  stateVersion: number;
  snapshot?: VisualSnapshot;
  recording?: VisualRecording;
  diagnostics?: Diagnostic[];
  changed?: SemanticRef[];
};

export type VisualQueryResult = {
  visualId: string;
  stateVersion: number;
  scene?: SemanticScene;
  recording?: VisualRecording;
};

/** Generic presentation protocol. Analytical exploration is an optional extension,
 * so source diagrams and static renditions never need fabricated corpora. */
export const VISUAL_SESSION_SCHEMA = "synth.visual-session.v1" as const;
export type VisualSessionIdentity = { visualId: string; revision: number; viewKey: string };
/** Deliberately bounded, portable structural schema; not arbitrary JSON Schema. */
export type VisualValueSchema = {
  type: "string" | "number" | "boolean" | "object" | "array" | "null";
  nullable?: boolean;
  oneOf?: VisualValueSchema[];
  options?: JsonValue[];
  minimum?: number;
  maximum?: number;
  properties?: Record<string, VisualValueSchema>;
  required?: string[];
  additionalProperties?: boolean | VisualValueSchema;
  items?: VisualValueSchema;
  maxItems?: number;
};
export type VisualControl = VisualValueSchema & {
  id: string;
  label: string;
  clock?: string;
};
export type VisualSessionState = VisualSessionIdentity & {
  /** Restored values are inert until an explicit presentation edit. */
  replay?:{checkpointId:string}|{recordingId:string;sequence:number;eventCount?:number;playing?:boolean;intervalMs?:number};
  schemaVersion: typeof VISUAL_SESSION_SCHEMA;
  stateVersion: number;
  definition: ExtensionPin;
  values: Record<string, JsonValue>;
  controls: VisualControl[];
  scene?: SemanticScene;
  evidenceCut?: EvidenceSnapshotRef;
};
export type SessionCheckpoint = {
  schemaVersion: "synth.visual-checkpoint.v1";
  id: string;
  capturedAt: string;
  state: VisualSessionState;
  digest: string;
  renditionRefs: string[];
};
export type SessionEvent = {
  sequence: number;
  action: VisualAction;
  /** Committed registration/scene context immediately before the command. */
  before?: VisualSessionState;
  state: VisualSessionState;
  occurredAt: string;
};
export type SessionRecording = {
  schemaVersion: "synth.visual-session-recording.v1";
  id: string;
  initial: SessionCheckpoint;
  events: SessionEvent[];
  endedAt?: string;
};
export type SessionReceipt = {
  commandId: string;
  state: VisualSessionState;
  duplicate?: boolean;
};
