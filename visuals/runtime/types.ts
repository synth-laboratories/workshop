/**
 * Core contracts for Synth Desktop deep visuals.
 * Templates bind data through VisualBinding; Desktop renders VisualInstance shells.
 */

/** How a template slot is fed at runtime. */
export const VISUAL_BINDINGS_SCHEMA_VERSION = "synth.visual-bindings.v1" as const;
export type VisualBindingKind = "inline" | "trace_v5" | "local_cas" | "run_ref" | "live_sse" | "fixture" | "optimizer_run";

export type VisualBinding = {
  /** Slot name declared in template.json `slots`. */
  slot: string;
  kind: VisualBindingKind;
  /**
   * Kind-specific locator:
   * - trace_v5 → digest or catalog id
   * - local_cas → content-addressed blob digest / path
   * - live_sse → absolute SSE URL
   * - fixture → relative path under visuals/fixtures/ or template examples/
   * - optimizer_run → cloud/local optimizer_run_id
   */
  source?: string;
  /** Declared sibling poll endpoint for a normalized live stream. Never inferred. */
  poll_url?: string;
  /** Resolved payload. Required for inline bindings. */
  data?: unknown;
  /** Optional JSON-pointer / dotted path into the resolved payload. */
  path?: string;
  /** Optional MIME / schema hint for validators. */
  schema?: string;
};

export type VisualBindings = {
  schemaVersion: typeof VISUAL_BINDINGS_SCHEMA_VERSION;
  slots: VisualBinding[];
};

export type VisualTemplateSlot = {
  name: string;
  description: string;
  /** Accepted binding kinds for this slot. */
  accepts: VisualBindingKind[];
  required?: boolean;
  /** Allow several independently declared sources to feed one semantic slot. */
  multiple?: boolean;
  schema?: string;
};

export type VisualTemplateMeta = {
  schemaVersion?: "synth.visual-template.v1";
  id: string;
  title: string;
  genre: string;
  version: string;
  description: string;
  accent?: string;
  slots: VisualTemplateSlot[];
  /** Relative path to the React shell from the template root. */
  shell: string;
  tags?: string[];
};

/**
 * Where a template came from. `internal` templates are staged from ~/.synth
 * into templates-internal/ at build time and never ship in a public release.
 */
export type VisualTemplateDistribution = "public" | "internal";

export type VisualTemplate = VisualTemplateMeta & {
  /** Absolute or package-relative directory containing template.json. */
  root: string;
  /** Derived from the template root, not self-declared. */
  distribution?: VisualTemplateDistribution;
};

export type VisualInstanceStatus = "draft" | "bound" | "saved" | "open";

/**
 * A concrete visual the agent or Desktop has created from a template.
 * Saved shells land under visuals/instances/<id>.tsx.
 */
export type VisualInstance = {
  id: string;
  templateId: string;
  title: string;
  createdAt: string;
  updatedAt: string;
  status: VisualInstanceStatus;
  bindings: VisualBindings;
  /** Optional props passed into the shell (title overrides, selected model, etc.). */
  props?: Record<string, unknown>;
  /** Path relative to visuals/ when status is saved. */
  tsxPath?: string;
  /** Desktop pane open state. */
  paneOpen?: boolean;
};

/** Shared chrome / theme tokens for light Poolside-compatible panes. */
export type VisualChromeTheme = {
  accent: string;
  accentHot: string;
  surface: string;
  surfaceMuted: string;
  border: string;
  text: string;
  textMuted: string;
};

export const DEFAULT_CHROME: VisualChromeTheme = {
  accent: "#F05F22",
  accentHot: "#FF5C00",
  surface: "#ffffff",
  surfaceMuted: "#f6f7f9",
  border: "#e8eaee",
  text: "#1a1d23",
  textMuted: "#5c6573"
};

/** Live SSE event envelope used by live.* templates. */
export type LiveEvalEvent = {
  ts?: string;
  occurred_at?: string;
  run_id: string;
  kind: string;
  lane?: string | null;
  source?: string;
  sequence?: number | string | null;
  schema_version?: string;
  payload: Record<string, unknown>;
};

/** Minimal Trace V5 overlay annotation (never mutates sealed trace). */
export type TraceAnnotationMarker = {
  id: string;
  turn?: number;
  step_index?: number;
  label: string;
  kind: "note" | "bug" | "highlight" | "reward" | "acceptance";
  span?: { start: number; end: number };
  meta?: Record<string, unknown>;
};

/** Standard PostTrain / trajectory step used by rollout viewers. */
export type RolloutStep = {
  index: number;
  turn?: number;
  action?: string;
  reward?: number;
  observation_text?: string;
  metrics?: Record<string, number>;
  achievements?: string[];
  meta?: Record<string, unknown>;
};

/** Craftax-style cohort point for pareto plots. */
export type EvalMatrixPoint = {
  model: string;
  effort?: string;
  achievements: number;
  cost_usd: number;
  n?: number;
  accent?: boolean;
  achievement_rates?: Record<string, number>;
};
