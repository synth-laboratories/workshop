/**
 * Bind Trace V5 / local CAS / live SSE / fixture sources into template slot payloads.
 */

import type {
  EvalMatrixPoint,
  LiveEvalEvent,
  RolloutStep,
  TraceAnnotationMarker,
  VisualBinding,
  VisualBindingKind,
  VisualTemplateMeta
} from "./types.ts";

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
    case "fixture": {
      if (!ctx.loadFixture) {
        throw new Error(`No fixture loader for slot "${binding.slot}"`);
      }
      return dig(await ctx.loadFixture(binding.source), binding.path);
    }
    case "trace_v5": {
      if (!ctx.loadTraceV5) {
        throw new Error(`No Trace V5 loader for slot "${binding.slot}"`);
      }
      return dig(await ctx.loadTraceV5(binding.source), binding.path);
    }
    case "local_cas": {
      if (!ctx.loadLocalCas) {
        throw new Error(`No local CAS loader for slot "${binding.slot}"`);
      }
      return dig(await ctx.loadLocalCas(binding.source), binding.path);
    }
    case "live_sse": {
      // Live slots bind the URL; consumers subscribe via subscribeLiveSlot.
      return { sse_url: binding.source, schema: binding.schema ?? "synth.live_eval.v1" };
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
  bindings: VisualBinding[],
  ctx: BindContext = {}
): Promise<BindResult> {
  const bySlot = new Map(bindings.map((b) => [b.slot, b]));
  const slots: Record<string, BoundSlotPayload> = {};
  const errors: string[] = [];

  for (const slot of template.slots) {
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
        source: binding.source,
        data
      };
    } catch (err) {
      errors.push(err instanceof Error ? err.message : String(err));
    }
  }

  return { templateId: template.id, slots, errors };
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
