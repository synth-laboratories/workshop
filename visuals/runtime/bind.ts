/**
 * Bind Trace V5 / local CAS / live SSE / fixture sources into template input payloads.
 */

import type {
  EvalMatrixPoint,
  LiveEvalEvent,
  RolloutStep,
  TraceAnnotationMarker,
  VisualBinding,
  VisualBindings,
  VisualBindingKind,
  VisualTemplateMeta
} from "./types.ts";
import {
  VISUAL_BINDINGS_SCHEMA_VERSION,
  bindingInputName,
  resolveInputName,
  stampBindingInput,
  templateInputs
} from "./types.ts";
import { assertDeclaredStreamSource, assertLiveEvalSlot } from "./liveStream.ts";
import { presentRuntimeErrorMessage } from "./presentError.ts";

export type BoundSlotPayload = {
  input: string;
  slot: string;
  kind: VisualBindingKind;
  source: string;
  data: unknown;
};

export type BindResult = {
  templateId: string;
  slots: Record<string, BoundSlotPayload>;
  errors: string[];
};

export type FixtureLoader = (relativePath: string) => Promise<unknown> | unknown;
export type TraceV5Loader = (digestOrId: string) => Promise<unknown> | unknown;
export type LocalCasLoader = (digestOrPath: string) => Promise<unknown> | unknown;
export type QuerySnapshotLoader = (snapshotId: string) => Promise<unknown> | unknown;

/**
 * Optional live SSE subscriber. Desktop injects EventSource / fetch-stream.
 * Returns an unsubscribe function.
 */
export type LiveSseSubscribe = (
  url: string,
  onEvent: (event: LiveEvalEvent) => void,
  onError?: (err: Error) => void
) => () => void;

export type AnnotationEvidenceHeadLoader = (digest: string) => Promise<unknown> | unknown;
export type VerifierResultLoader = (digest: string) => Promise<unknown> | unknown;

export type BindContext = {
  loadFixture?: FixtureLoader;
  loadTraceV5?: TraceV5Loader;
  loadLocalCas?: LocalCasLoader;
  loadQuerySnapshot?: QuerySnapshotLoader;
  loadRun?: (runId: string) => Promise<unknown> | unknown;
  loadOptimizerRun?: (optimizerRunId: string) => Promise<unknown> | unknown;
  loadAnnotationEvidenceHead?: AnnotationEvidenceHeadLoader;
  loadVerifierResult?: VerifierResultLoader;
  /** Declared create-rollout stream descriptor; required to bind guessed-looking URLs. */
  declaredStream?: import("./liveStream.ts").DeclaredStreamDescriptor | null;
  /** When true, missing optional inputs are ignored. */
  skipOptional?: boolean;
};

function dig(value: unknown, path?: string): unknown {
  if (!path) return value;
  const parts = path.replace(/^\//, "").split(/[.\/]/).filter(Boolean);
  let cur: unknown = value;
  for (const part of parts) {
    if (cur == null || typeof cur !== "object") return undefined;
    cur = (cur as Record<string, unknown>)[part];
  }
  return cur;
}

async function resolveBinding(
  binding: VisualBinding,
  ctx: BindContext
): Promise<unknown> {
  switch (binding.kind) {
    case "inline":
      return dig(binding.data, binding.path);
    case "fixture": {
      if (!ctx.loadFixture) {
        throw new Error(`No fixture loader for input "${bindingInputName(binding) ?? "?"}"`);
      }
      return dig(await ctx.loadFixture(binding.source!), binding.path);
    }
    case "trace_v5": {
      if (!binding.source) {
        throw new Error(`Trace V5 binding for input "${bindingInputName(binding) ?? "?"}" requires a sealed trace digest`);
      }
      if (!ctx.loadTraceV5) {
        throw new Error(`No Trace V5 loader for input "${bindingInputName(binding) ?? "?"}"`);
      }
      return dig(await ctx.loadTraceV5(binding.source), binding.path);
    }
    case "local_cas": {
      if (binding.data !== undefined) return dig(binding.data, binding.path);
      if (!binding.source) {
        throw new Error(`local_cas binding for input "${bindingInputName(binding) ?? "?"}" requires a content digest`);
      }
      if (!ctx.loadLocalCas) {
        throw new Error(`No local CAS loader for input "${bindingInputName(binding) ?? "?"}"`);
      }
      return dig(await ctx.loadLocalCas(binding.source), binding.path);
    }
    case "query_snapshot": {
      if (binding.data !== undefined) return dig(binding.data, binding.path);
      if (!binding.source) {
        throw new Error(`query_snapshot binding for input "${bindingInputName(binding) ?? "?"}" requires a snapshot id`);
      }
      if (!ctx.loadQuerySnapshot) {
        throw new Error(`No query snapshot loader for input "${bindingInputName(binding) ?? "?"}"`);
      }
      return dig(await ctx.loadQuerySnapshot(binding.source), binding.path);
    }
    case "run_ref": {
      if (binding.data !== undefined) return dig(binding.data, binding.path);
      if (!ctx.loadRun) throw new Error(`No run loader for input "${bindingInputName(binding) ?? "?"}"`);
      return dig(await ctx.loadRun(binding.source!), binding.path);
    }
    case "optimizer_run": {
      if (binding.data !== undefined) return dig(binding.data, binding.path);
      if (!ctx.loadOptimizerRun) {
        throw new Error(`No optimizer run loader for input "${bindingInputName(binding) ?? "?"}"`);
      }
      return dig(await ctx.loadOptimizerRun(binding.source!), binding.path);
    }
    case "live_sse": {
      if (!binding.source) throw new Error("live_sse binding requires a source");
      const guessed = assertDeclaredStreamSource(binding.source, ctx.declaredStream);
      if (guessed) throw new Error(guessed);
      return {
        sse_url: binding.source,
        ...(binding.poll_url ? { poll_url: binding.poll_url } : {}),
        schema: binding.schema ?? "synth.live_eval.v1"
      };
    }
    case "annotation_evidence_head": {
      if (binding.data !== undefined) return dig(binding.data, binding.path);
      if (!binding.source) {
        throw new Error(`annotation_evidence_head binding for input "${bindingInputName(binding) ?? "?"}" requires an evidence-head digest`);
      }
      if (!ctx.loadAnnotationEvidenceHead) {
        throw new Error(`No annotation evidence-head loader for input "${bindingInputName(binding) ?? "?"}"`);
      }
      return dig(await ctx.loadAnnotationEvidenceHead(binding.source), binding.path);
    }
    case "verifier_result_v2": {
      if (binding.data !== undefined) return dig(binding.data, binding.path);
      if (!binding.source) {
        throw new Error(`verifier_result_v2 binding for input "${bindingInputName(binding) ?? "?"}" requires a verifier-result digest`);
      }
      if (!ctx.loadVerifierResult) {
        throw new Error(`No verifier-result loader for input "${bindingInputName(binding) ?? "?"}"`);
      }
      return dig(await ctx.loadVerifierResult(binding.source), binding.path);
    }
    default: {
      const _exhaustive: never = binding.kind;
      throw new Error(`Unknown binding kind: ${_exhaustive}`);
    }
  }
}

function describeError(err: unknown): string {
  return presentRuntimeErrorMessage(err, "Binding resolution failed");
}

/**
 * Resolve all bindings for a template into input payloads.
 * Does not open SSE streams — use `subscribeLiveSlot` for live.* templates.
 */
export async function bindTemplateSlots(
  template: VisualTemplateMeta,
  bindings: VisualBinding[] | VisualBindings,
  ctx: BindContext = {}
): Promise<BindResult> {
  const resolved = resolveVisualBindings(bindings);
  if (resolved.status === "rejected") {
    return { templateId: template.id, slots: {}, errors: [resolved.error ?? "Visual bindings are unreadable"] };
  }
  const bindingSlots = resolved.slots;
  const bySlot = new Map<string, VisualBinding[]>();
  for (const binding of bindingSlots) {
    const name = bindingInputName(binding);
    if (!name) {
      continue;
    }
    const existing = bySlot.get(name) ?? [];
    existing.push(binding);
    bySlot.set(name, existing);
  }
  const slots: Record<string, BoundSlotPayload> = {};
  const errors: string[] = [];

  for (const binding of bindingSlots) {
    const conflict = resolveInputName(binding.input, binding.slot);
    if (!conflict.ok) {
      errors.push(conflict.error);
      continue;
    }
    const name = conflict.name;
    if (!name) {
      errors.push("A visual binding is missing its input name");
      continue;
    }
    const slotError = assertLiveEvalSlot(name, template.id);
    if (slotError) errors.push(slotError);
  }

  const declared = templateInputs(template);
  if (template.inputs && template.slots && JSON.stringify(template.inputs) !== JSON.stringify(template.slots)) {
    errors.push("template inputs and slots disagree; send one list");
  }
  for (const slot of declared) {
    const slotError = assertLiveEvalSlot(slot.name, template.id);
    if (slotError) errors.push(slotError);
    const candidates = bySlot.get(slot.name) ?? [];
    const required = slot.required !== false;
    if (candidates.length === 0) {
      if (required && !ctx.skipOptional) {
        errors.push(`Missing required binding for input "${slot.name}"`);
      }
      continue;
    }
    if (candidates.length > 1 && !slot.multiple) {
      errors.push(`Input "${slot.name}" accepts one binding, received ${candidates.length}`);
      continue;
    }
    const resolvedPayloads: BoundSlotPayload[] = [];
    for (const binding of candidates) {
      if (!slot.accepts.includes(binding.kind)) {
        errors.push(`Input "${slot.name}" does not accept kind "${binding.kind}" (accepts: ${slot.accepts.join(", ")})`);
        continue;
      }
      try {
        resolvedPayloads.push({
          input: slot.name,
          slot: slot.name,
          kind: binding.kind,
          source: binding.source ?? "inline",
          data: await resolveBinding(binding, ctx)
        });
      } catch (err) {
        errors.push(describeError(err));
      }
    }
    if (resolvedPayloads.length > 0) {
      slots[slot.name] = slot.multiple
        ? { input: slot.name, slot: slot.name, kind: resolvedPayloads[0].kind, source: "multiple", data: resolvedPayloads.map((item) => item.data) }
        : resolvedPayloads[0];
    }
  }

  return { templateId: template.id, slots, errors };
}

export function isVisualBindings(value: unknown): value is VisualBindings {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<VisualBindings>;
  return (
    candidate.schemaVersion === "synth.visual-bindings.v1"
    && (Array.isArray(candidate.inputs) || Array.isArray(candidate.slots))
  );
}

const BINDING_KINDS: readonly string[] = [
  "inline",
  "trace_v5",
  "local_cas",
  "run_ref",
  "live_sse",
  "fixture",
  "optimizer_run",
  "query_snapshot",
  "annotation_evidence_head",
  "verifier_result_v2"
];

export type VisualBindingsStatus = "canonical" | "upgraded" | "rejected";

export type ResolvedVisualBindings = {
  status: VisualBindingsStatus;
  slots: VisualBinding[];
  /** Present when status is `rejected`. Render it; never swallow it. */
  error: string | null;
  /** Slot names an upgrade touched. Empty when already canonical. */
  upgradedSlots: string[];
};

function isBindingDescriptor(value: unknown): boolean {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const candidate = value as Record<string, unknown>;
  if (typeof candidate.kind !== "string" || !BINDING_KINDS.includes(candidate.kind)) return false;
  return ["input", "slot", "source", "data", "poll_url"].some((field) => field in candidate);
}

function isDescriptorEntry(value: unknown): boolean {
  if (Array.isArray(value)) return value.length > 0 && value.every(isBindingDescriptor);
  return isBindingDescriptor(value);
}

function stampResolvedBindings(bindings: VisualBinding[]): VisualBinding[] | { error: string } {
  const out: VisualBinding[] = [];
  for (const binding of bindings) {
    const resolved = resolveInputName(binding.input, binding.slot);
    if (!resolved.ok) return { error: resolved.error };
    if (!resolved.name) return { error: "A visual binding is missing its input name" };
    out.push(stampBindingInput(binding, resolved.name));
  }
  return out;
}

function stableBindingJson(bindings: VisualBinding[]): string {
  return JSON.stringify(
    bindings.map((binding) =>
      Object.fromEntries(Object.entries(binding).sort(([left], [right]) => left.localeCompare(right)))
    )
  );
}

function envelopeDescriptors(value: VisualBindings): VisualBinding[] | { error: string } {
  const inputs = value.inputs;
  const slots = value.slots;
  if (inputs && slots) {
    const left = stampResolvedBindings(inputs);
    const right = stampResolvedBindings(slots);
    if ("error" in left) return left;
    if ("error" in right) return right;
    if (stableBindingJson(left) !== stableBindingJson(right)) {
      return { error: "visual bindings inputs and slots disagree; send one array" };
    }
    return left;
  }
  return stampResolvedBindings(inputs ?? slots ?? []);
}

/**
 * Resolve any persisted bindings value into inputs the renderer can read.
 *
 * Mirrors `visuals::models::canonicalize_bindings` in Rust; the two must agree,
 * because Rust decides what is written and this decides what is rendered.
 *
 * Three outcomes and no fourth. In particular there is no "return an empty
 * array and let the caller render nothing": a shape this cannot read produced
 * a pane that sat at `connecting` with ten live streams bound to it and no
 * error anywhere. An unreadable binding is a rejection, and a rejection is
 * something a person can see.
 */
export function resolveVisualBindings(value: unknown): ResolvedVisualBindings {
  if (Array.isArray(value)) {
    const stamped = stampResolvedBindings(value as VisualBinding[]);
    if ("error" in stamped) {
      return { status: "rejected", slots: [], error: stamped.error, upgradedSlots: [] };
    }
    return { status: "canonical", slots: stamped, error: null, upgradedSlots: [] };
  }
  if (isVisualBindings(value)) {
    const descriptors = envelopeDescriptors(value);
    if ("error" in descriptors) {
      return { status: "rejected", slots: [], error: descriptors.error, upgradedSlots: [] };
    }
    return { status: "canonical", slots: descriptors, error: null, upgradedSlots: [] };
  }
  if (!value || typeof value !== "object") {
    return {
      status: "rejected",
      slots: [],
      error: "Visual bindings are not an object",
      upgradedSlots: []
    };
  }
  const entries = Object.entries(value as Record<string, unknown>);
  if (entries.length === 0) {
    return { status: "canonical", slots: [], error: null, upgradedSlots: [] };
  }
  if ("schemaVersion" in (value as Record<string, unknown>)) {
    const version = (value as Record<string, unknown>).schemaVersion;
    return {
      status: "rejected",
      slots: [],
      error: `Unsupported visual bindings schemaVersion ${String(version)}; this build reads ${VISUAL_BINDINGS_SCHEMA_VERSION}`,
      upgradedSlots: []
    };
  }
  // COMPAT: a slot-keyed descriptor map, written before the envelope was
  // enforced. Upgraded here so an existing visual still renders, reported so it
  // does not stay invisible. Removed with the Rust upgrade path.
  if (entries.some(([, entry]) => isDescriptorEntry(entry))) {
    const unreadable = entries.filter(([, entry]) => !isDescriptorEntry(entry)).map(([slot]) => slot);
    if (unreadable.length > 0) {
      return {
        status: "rejected",
        slots: [],
        error: `Visual bindings mix descriptors and inline data on ${unreadable.join(", ")}; re-bind with an explicit inputs array`,
        upgradedSlots: []
      };
    }
    const slots = entries.flatMap(([name, entry]) =>
      (Array.isArray(entry) ? entry : [entry]).map(
        (descriptor) => stampBindingInput({ ...(descriptor as VisualBinding) }, name)
      )
    );
    return { status: "upgraded", slots, error: null, upgradedSlots: entries.map(([slot]) => slot) };
  }
  // A legacy inline prop bag. Its values are data, not transports.
  return {
    status: "upgraded",
    slots: entries.map(([name, data]) => stampBindingInput({ kind: "inline" as const, data }, name)),
    error: null,
    upgradedSlots: entries.map(([slot]) => slot)
  };
}

/** Desktop passes the bindings envelope; some hosts still pass a raw input array. */
export function bindingSlots(value: unknown): VisualBinding[] {
  return resolveVisualBindings(value).slots;
}

export function propsFromBindings(value: unknown): { props: Record<string, unknown>; errors: string[] } {
  const resolved = resolveVisualBindings(value);
  if (resolved.status === "rejected") {
    return { props: {}, errors: [resolved.error ?? "Visual bindings are unreadable"] };
  }
  const props: Record<string, unknown> = {};
  const errors: string[] = [];
  for (const binding of resolved.slots) {
    const name = bindingInputName(binding);
    if (!name) {
      errors.push("A visual binding is missing its input name");
      continue;
    }
    const slotError = assertLiveEvalSlot(name);
    if (slotError) {
      errors.push(slotError);
      continue;
    }
    if (binding.kind === "inline") {
      if (!("data" in binding)) errors.push(`Inline input "${name}" has no data`);
      else props[name] = binding.data;
    } else if (binding.kind === "live_sse" && binding.source) {
      const guessed = assertDeclaredStreamSource(binding.source);
      if (guessed) errors.push(guessed);
      else {
        props[name] = {
          sse_url: binding.source,
          ...(binding.poll_url ? { poll_url: binding.poll_url } : {}),
          schema: binding.schema ?? "evals.event-stream.v1"
        };
      }
    } else if (binding.kind === "optimizer_run" && binding.source) {
      props[name] = {
        optimizer_run_id: binding.source,
        schema: binding.schema ?? "optimizer_run.v1"
      };
    } else if (binding.kind === "query_snapshot" && binding.source) {
      props[name] = {
        snapshot_id: binding.source,
        schema: binding.schema ?? "synth.trace-query-result.v1"
      };
    } else if ("data" in binding) props[name] = binding.data;
    else errors.push(`Input "${name}" (${binding.kind}) has not been resolved by the Rust runtime`);
  }
  return { props, errors };
}

/**
 * Subscribe a live_sse binding. Desktop passes a real EventSource adapter.
 */
export function subscribeLiveSlot(
  binding: VisualBinding,
  subscribe: LiveSseSubscribe,
  onEvent: (event: LiveEvalEvent) => void,
  onError?: (err: Error) => void
): () => void {
  if (binding.kind !== "live_sse") {
    throw new Error(`subscribeLiveSlot expects live_sse, got ${binding.kind}`);
  }
  if (!binding.source) throw new Error("live_sse binding requires a source");
  const guessed = assertDeclaredStreamSource(binding.source);
  if (guessed) throw new Error(guessed);
  return subscribe(binding.source, onEvent, onError);
}

/** Narrow helpers for common fixture shapes. */
export function asEvalMatrixPoints(data: unknown): EvalMatrixPoint[] {
  if (!Array.isArray(data)) {
    const nested = (data as { points?: unknown })?.points;
    if (Array.isArray(nested)) return nested as EvalMatrixPoint[];
    throw new Error("Expected EvalMatrixPoint[]");
  }
  return data as EvalMatrixPoint[];
}

export function asRolloutSteps(data: unknown): RolloutStep[] {
  if (!Array.isArray(data)) {
    const nested = (data as { steps?: unknown })?.steps;
    if (Array.isArray(nested)) return nested as RolloutStep[];
    throw new Error("Expected RolloutStep[]");
  }
  return data as RolloutStep[];
}

export function asLiveEvents(data: unknown): LiveEvalEvent[] {
  if (!Array.isArray(data)) {
    const nested = (data as { events?: unknown })?.events;
    if (Array.isArray(nested)) return nested as LiveEvalEvent[];
    throw new Error("Expected LiveEvalEvent[]");
  }
  return data as LiveEvalEvent[];
}

export function asAnnotationMarkers(data: unknown): TraceAnnotationMarker[] {
  if (!Array.isArray(data)) {
    const nested = (data as { markers?: unknown })?.markers;
    if (Array.isArray(nested)) return nested as TraceAnnotationMarker[];
    throw new Error("Expected TraceAnnotationMarker[]");
  }
  return data as TraceAnnotationMarker[];
}

/** Node-friendly fixture loader from a base directory (Desktop / MCP host). */
export function createJsonFixtureLoader(
  readFile: (absPath: string) => Promise<string> | string,
  resolvePath: (relative: string) => string
): FixtureLoader {
  return async (relativePath: string) => {
    const raw = await readFile(resolvePath(relativePath));
    return JSON.parse(raw) as unknown;
  };
}
