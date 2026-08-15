/**
 * Bind Trace V5 / local CAS / live SSE / fixture sources into template slot payloads.
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
import { assertDeclaredStreamSource, assertLiveEvalSlot } from "./liveStream.ts";

export type BoundSlotPayload = {
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

/**
 * Optional live SSE subscriber. Desktop injects EventSource / fetch-stream.
 * Returns an unsubscribe function.
 */
export type LiveSseSubscribe = (
  url: string,
  onEvent: (event: LiveEvalEvent) => void,
  onError?: (err: Error) => void
) => () => void;

export type BindContext = {
  loadFixture?: FixtureLoader;
  loadTraceV5?: TraceV5Loader;
  loadLocalCas?: LocalCasLoader;
  loadRun?: (runId: string) => Promise<unknown> | unknown;
  loadOptimizerRun?: (optimizerRunId: string) => Promise<unknown> | unknown;
  /** Declared create-rollout stream descriptor; required to bind guessed-looking URLs. */
  declaredStream?: import("./liveStream.ts").DeclaredStreamDescriptor | null;
  /** When true, missing optional slots are ignored. */
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
        throw new Error(`No fixture loader for slot "${binding.slot}"`);
      }
      return dig(await ctx.loadFixture(binding.source!), binding.path);
    }
    case "trace_v5": {
      if (!binding.source) {
        throw new Error(`Trace V5 binding for slot "${binding.slot}" requires a sealed trace digest`);
      }
      if (!ctx.loadTraceV5) {
        throw new Error(`No Trace V5 loader for slot "${binding.slot}"`);
      }
      return dig(await ctx.loadTraceV5(binding.source), binding.path);
    }
    case "local_cas": {
      if (!ctx.loadLocalCas) {
        throw new Error(`No local CAS loader for slot "${binding.slot}"`);
      }
      return dig(await ctx.loadLocalCas(binding.source!), binding.path);
    }
    case "run_ref": {
      if (!ctx.loadRun) throw new Error(`No run loader for slot "${binding.slot}"`);
      return dig(await ctx.loadRun(binding.source!), binding.path);
    }
    case "optimizer_run": {
      if (binding.data !== undefined) return dig(binding.data, binding.path);
      if (!ctx.loadOptimizerRun) {
        throw new Error(`No optimizer run loader for slot "${binding.slot}"`);
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
    default: {
      const _exhaustive: never = binding.kind;
      throw new Error(`Unknown binding kind: ${_exhaustive}`);
    }
  }
}

/**
 * Resolve all bindings for a template into slot payloads.
 * Does not open SSE streams — use `subscribeLiveSlot` for live.* templates.
 */
export async function bindTemplateSlots(
  template: VisualTemplateMeta,
  bindings: VisualBinding[] | VisualBindings,
  ctx: BindContext = {}
): Promise<BindResult> {
  const bindingSlots = Array.isArray(bindings) ? bindings : bindings.slots;
  const bySlot = new Map(bindingSlots.map((b) => [b.slot, b]));
  const slots: Record<string, BoundSlotPayload> = {};
  const errors: string[] = [];

  for (const binding of bindingSlots) {
    const slotError = assertLiveEvalSlot(binding.slot, template.id);
    if (slotError) errors.push(slotError);
  }

  for (const slot of template.slots) {
    const slotError = assertLiveEvalSlot(slot.name, template.id);
    if (slotError) errors.push(slotError);
    const binding = bySlot.get(slot.name);
    const required = slot.required !== false;
    if (!binding) {
      if (required && !ctx.skipOptional) {
        errors.push(`Missing required binding for slot "${slot.name}"`);
      }
      continue;
    }
    if (!slot.accepts.includes(binding.kind)) {
      errors.push(
        `Slot "${slot.name}" does not accept kind "${binding.kind}" (accepts: ${slot.accepts.join(", ")})`
      );
      continue;
    }
    try {
      const data = await resolveBinding(binding, ctx);
      slots[slot.name] = {
        slot: slot.name,
        kind: binding.kind,
        source: binding.source ?? "inline",
        data
      };
    } catch (err) {
      errors.push(err instanceof Error ? err.message : String(err));
    }
  }

  return { templateId: template.id, slots, errors };
}

export function isVisualBindings(value: unknown): value is VisualBindings {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<VisualBindings>;
  return candidate.schemaVersion === "synth.visual-bindings.v1" && Array.isArray(candidate.slots);
}

/** Desktop passes the bindings envelope; some hosts still pass a raw slot array. */
export function bindingSlots(value: unknown): VisualBinding[] {
  if (Array.isArray(value)) return value as VisualBinding[];
  if (isVisualBindings(value)) return value.slots;
  return [];
}

export function propsFromBindings(value: unknown): { props: Record<string, unknown>; errors: string[] } {
  if (!isVisualBindings(value)) {
    if (value && typeof value === "object" && !Array.isArray(value)) {
      return { props: value as Record<string, unknown>, errors: [] };
    }
    return { props: {}, errors: ["Visual bindings are not an object"] };
  }
  const props: Record<string, unknown> = {};
  const errors: string[] = [];
  for (const binding of value.slots) {
    if (!binding.slot || typeof binding.slot !== "string") {
      errors.push("A visual binding is missing its slot name");
      continue;
    }
    const slotError = assertLiveEvalSlot(binding.slot);
    if (slotError) {
      errors.push(slotError);
      continue;
    }
    if (binding.kind === "inline") {
      if (!("data" in binding)) errors.push(`Inline slot "${binding.slot}" has no data`);
      else props[binding.slot] = binding.data;
    } else if (binding.kind === "live_sse" && binding.source) {
      const guessed = assertDeclaredStreamSource(binding.source);
      if (guessed) errors.push(guessed);
      else {
        props[binding.slot] = {
          sse_url: binding.source,
          ...(binding.poll_url ? { poll_url: binding.poll_url } : {}),
          schema: binding.schema ?? "evals.event-stream.v1"
        };
      }
    } else if (binding.kind === "optimizer_run" && binding.source) {
      props[binding.slot] = {
        optimizer_run_id: binding.source,
        schema: binding.schema ?? "optimizer_run.v1"
      };
    } else if ("data" in binding) props[binding.slot] = binding.data;
    else errors.push(`Slot "${binding.slot}" (${binding.kind}) has not been resolved by the Rust runtime`);
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
