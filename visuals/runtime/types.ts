/**
 * Core contracts for Synth Desktop deep visuals.
 * Templates bind data through VisualBinding; Desktop renders VisualInstance shells.
 */

/** How a template input is fed at runtime. */
export const VISUAL_BINDINGS_SCHEMA_VERSION = "synth.visual-bindings.v1" as const;
export type VisualBindingKind =
  | "inline"
  | "trace_v5"
  | "local_cas"
  | "run_ref"
  | "live_sse"
  | "fixture"
  | "optimizer_run"
  | "query_snapshot"
  | "annotation_evidence_head"
  | "verifier_result_v2";

/**
 * Bind-point name: canonical `input`; `slot` still binds on stored envelopes.
 * Both present and unequal is a conflict; callers must fail closed.
 */
export function resolveInputName(
  canonical: unknown,
  alias: unknown
): { ok: true; name: string | undefined } | { ok: false; error: string } {
  const input = typeof canonical === "string" && canonical.trim() ? canonical.trim() : undefined;
  const slot = typeof alias === "string" && alias.trim() ? alias.trim() : undefined;
  if (input && slot && input !== slot) {
    return {
      ok: false,
      error: `input ${JSON.stringify(input)} and slot ${JSON.stringify(slot)} disagree; send one name`
    };
  }
  return { ok: true, name: input ?? slot };
}

export function bindingInputName(binding: { input?: string; slot?: string }): string | undefined {
  const resolved = resolveInputName(binding.input, binding.slot);
  return resolved.ok ? resolved.name : undefined;
}

export function stampBindingInput<T extends VisualBinding>(binding: T, name: string): T {
	const { slot: _compatSlot, ...rest } = binding;
	return { ...rest, input: name } as T;
}

export function templateInputs(template: {
  inputs?: VisualTemplateSlot[];
  slots?: VisualTemplateSlot[];
}): VisualTemplateSlot[] {
  return template.inputs ?? template.slots ?? [];
}

export function bindingList(
  bindings: VisualBinding[] | { inputs?: VisualBinding[]; slots?: VisualBinding[] } | undefined
): VisualBinding[] {
  if (!bindings) return [];
  if (Array.isArray(bindings)) return bindings;
  return bindings.inputs ?? bindings.slots ?? [];
}

export type VisualBinding = {
  /** Canonical bind-point name declared in template.json `inputs`. */
  input?: string;
  /** Read-only alias of `input` on stored envelopes. New writers omit this. */
  slot?: string;
  kind: VisualBindingKind;
  /**
   * Kind-specific locator:
   * - trace_v5 → digest or catalog id
   * - local_cas → content-addressed blob digest / path
   * - live_sse → absolute SSE URL
   * - fixture → relative path under visuals/fixtures/ or template examples/
   * - optimizer_run → cloud/local optimizer_run_id
   * - query_snapshot → immutable trace query snapshot id
   * - run_ref → run identity resolved by the host
   * - annotation_evidence_head → sealed annotation evidence-head digest
   * - verifier_result_v2 → VerifierResultV2 content digest
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
  /** Canonical descriptor array. */
  inputs?: VisualBinding[];
  /** Read-only alias of `inputs` on stored envelopes. New writers omit this. */
  slots?: VisualBinding[];
};

export type VisualComponentMeta = {
  id: string;
  kind: string;
  protocolId: string;
  consumes: string[];
  emits?: string[];
  description?: string;
};

export type VisualTemplateSlot = {
  name: string;
  description: string;
  /** Accepted binding kinds for this input. */
  accepts: VisualBindingKind[];
  required?: boolean;
  /** Allow several independently declared sources to feed one semantic input. */
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
  rendererKind?: string;
  kind?: string;
  protocolId?: string;
  /** Canonical bind-point list. */
  inputs?: VisualTemplateSlot[];
  /** Read-only echo of `inputs` for old `list_templates` readers. */
  slots: VisualTemplateSlot[];
  /** Relative path to the React shell from the template root. */
  shell: string;
  tags?: string[];
  /** Advertised compose parts. Kind is the render contract; protocolId the bind dialect. */
  components?: VisualComponentMeta[];
  observationContract?: {
    schemaVersion: "synth.visual-observation-contract.v1";
    readiness: {
      rejectTransportStates?: string[];
      minimumRolloutCount?: number;
      minimumRenderedFrameCount?: number;
      minimumSemanticEventCount?: number;
      requireTerminal?: boolean;
    };
  };
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
  /** Stable, one-based order in which this viewer accepted the event. */
  logical_time?: number;
  run_id: string;
  kind: string;
  /** Optimizer envelopes use `type`; includeKinds matches kind or type. */
  type?: string;
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
