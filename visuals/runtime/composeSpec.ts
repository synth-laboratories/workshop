/**
 * Compose spec: ordered placements of shipped visual components.
 *
 * Kind is the render contract. protocol_id is the bind dialect. Unknown
 * component ids fail closed. Transport stays on the host ReplayClient.
 */

import { resolveInputName } from "./types.ts";

export const COMPOSE_SPEC_SCHEMA = "synth.visual.compose_spec.v1" as const;

export const COMPOSE_EVENT_STREAM_INPUTS = ["stream", "optimizer_run"] as const;
/** COMPAT: alias of `COMPOSE_EVENT_STREAM_INPUTS`. */
export const COMPOSE_EVENT_STREAM_SLOTS = COMPOSE_EVENT_STREAM_INPUTS;

export type ComposeEventStreamSlot = (typeof COMPOSE_EVENT_STREAM_INPUTS)[number];

export const COMPOSE_COMPONENTS = {
  "event_stream.v1": {
    kind: "event_stream",
    protocolId: "event_stream.v1",
    consumes: ["stream", "optimizer_run"],
    emits: ["cursor"]
  },
  "detail_modal.v1": {
    kind: "detail_modal",
    protocolId: "detail_modal.v1",
    consumes: ["cursor"],
    emits: []
  },
  "metrics.v1": {
    kind: "metrics",
    protocolId: "metrics.reduce.v1",
    consumes: ["stream", "optimizer_run"],
    emits: []
  },
  "scrubber.v1": {
    kind: "scrubber",
    protocolId: "scrubber.v1",
    consumes: ["stream", "optimizer_run"],
    emits: ["cursor"]
  },
  "candidate_inspector.v1": {
    kind: "candidate_inspector",
    protocolId: "candidate_inspector.v1",
    consumes: ["optimizer_run"],
    emits: ["cursor"]
  }
} as const;

export type ComposeComponentId = keyof typeof COMPOSE_COMPONENTS;

export type ComposePlacement = {
  id: string;
  component: ComposeComponentId;
  /** Canonical bind-point this placement drinks from. */
  input?: string;
  /** COMPAT: one-release alias of `input`. */
  slot?: string;
  from?: string;
  config?: { includeKinds?: string[] };
};

export type ComposeSpec = {
  schemaVersion: typeof COMPOSE_SPEC_SCHEMA;
  title?: string;
  lede?: string;
  placements: ComposePlacement[];
};

export type ComposeSpecResult =
  | { ok: true; spec: ComposeSpec }
  | { ok: false; error: string };

function asString(value: unknown): string | undefined {
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : undefined;
}

export function isComposeComponentId(value: string): value is ComposeComponentId {
  return Object.prototype.hasOwnProperty.call(COMPOSE_COMPONENTS, value);
}

export function isComposeEventStreamSlot(value: string): value is ComposeEventStreamSlot {
  return (COMPOSE_EVENT_STREAM_INPUTS as readonly string[]).includes(value);
}

function componentSignals(id: ComposeComponentId): {
  consumes: readonly string[];
  emits: readonly string[];
} {
  const def = COMPOSE_COMPONENTS[id];
  return { consumes: def.consumes, emits: def.emits };
}

export function composeComponentEmitsCursor(id: ComposeComponentId): boolean {
  return componentSignals(id).emits.includes("cursor");
}

/** Dual-dialect placements (event_stream, metrics, scrubber). */
export function composeConsumesStreamOrOptimizer(id: ComposeComponentId): boolean {
  const { consumes } = componentSignals(id);
  return consumes.includes("stream") && consumes.includes("optimizer_run");
}

export function composeConsumesOptimizerRun(id: ComposeComponentId): boolean {
  return componentSignals(id).consumes.includes("optimizer_run");
}

/** Placement `input` selects the bind dialect. Default remains eval `stream`. */
export function composeEventStreamSlot(placement: ComposePlacement): ComposeEventStreamSlot {
  const resolved = resolveInputName(placement.input, placement.slot);
  const name = resolved.ok ? resolved.name : undefined;
  return name === "optimizer_run" ? "optimizer_run" : "stream";
}

export function composePlacementNeedsStream(placement: ComposePlacement): boolean {
  if (!composeConsumesStreamOrOptimizer(placement.component)) return false;
  return composeEventStreamSlot(placement) === "stream";
}

export function composePlacementNeedsOptimizerRun(placement: ComposePlacement): boolean {
  if (!composeConsumesOptimizerRun(placement.component)) return false;
  if (!composeConsumesStreamOrOptimizer(placement.component)) return true;
  return composeEventStreamSlot(placement) === "optimizer_run";
}

export function parseComposeSpec(raw: unknown): ComposeSpecResult {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
    return { ok: false, error: "Compose spec must be an object" };
  }
  const value = raw as Record<string, unknown>;
  const schemaVersion = asString(value.schemaVersion);
  if (schemaVersion && schemaVersion !== COMPOSE_SPEC_SCHEMA) {
    return { ok: false, error: `Unsupported compose spec ${schemaVersion}` };
  }
  if (!Array.isArray(value.placements) || value.placements.length === 0) {
    return { ok: false, error: "Compose spec requires placements[]" };
  }
  const seen = new Set<string>();
  const placements: ComposePlacement[] = [];
  for (const [index, entry] of value.placements.entries()) {
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) {
      return { ok: false, error: `Placement ${index} must be an object` };
    }
    const row = entry as Record<string, unknown>;
    const id = asString(row.id);
    const component = asString(row.component);
    if (!id) return { ok: false, error: `Placement ${index} is missing id` };
    if (seen.has(id)) return { ok: false, error: `Duplicate placement id "${id}"` };
    seen.add(id);
    if (!component) return { ok: false, error: `Placement "${id}" is missing component` };
    if (!isComposeComponentId(component)) {
      return { ok: false, error: `Unknown compose component "${component}"` };
    }
    const includeKinds = Array.isArray(row.config)
      ? undefined
      : row.config && typeof row.config === "object"
        ? (row.config as { includeKinds?: unknown }).includeKinds
        : undefined;
    const named = resolveInputName(row.input, row.slot);
    if (!named.ok) {
      return { ok: false, error: `Placement "${id}" ${named.error}` };
    }
    placements.push({
      id,
      component,
      input: named.name,
      slot: named.name,
      from: asString(row.from),
      config: Array.isArray(includeKinds)
        ? { includeKinds: includeKinds.filter((kind): kind is string => typeof kind === "string") }
        : undefined
    });
  }
  for (const placement of placements) {
    if (composeConsumesStreamOrOptimizer(placement.component)) {
      const name = placement.input ?? placement.slot ?? "stream";
      if (!isComposeEventStreamSlot(name)) {
        return {
          ok: false,
          error: `Placement "${placement.id}" must consume input "stream" or "optimizer_run"`
        };
      }
    }
    if (placement.component === "candidate_inspector.v1") {
      const name = placement.input ?? placement.slot;
      if (name !== "optimizer_run") {
        return {
          ok: false,
          error: `Placement "${placement.id}" must consume input "optimizer_run"`
        };
      }
    }
    if (placement.component === "detail_modal.v1") {
      if (!placement.from) {
        return { ok: false, error: `Placement "${placement.id}" requires from` };
      }
      const source = placements.find((row) => row.id === placement.from);
      if (!source) {
        return { ok: false, error: `Placement "${placement.id}" from "${placement.from}" does not exist` };
      }
      if (!composeComponentEmitsCursor(source.component)) {
        return { ok: false, error: `Placement "${placement.id}" from "${placement.from}" does not emit a cursor` };
      }
    }
  }
  return {
    ok: true,
    spec: {
      schemaVersion: COMPOSE_SPEC_SCHEMA,
      title: asString(value.title),
      lede: asString(value.lede),
      placements
    }
  };
}
