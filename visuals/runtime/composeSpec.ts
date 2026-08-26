/**
 * Compose spec: ordered placements of shipped visual components.
 *
 * Kind is the render contract. protocol_id is the bind dialect. Unknown
 * component ids fail closed. Transport stays on the host ReplayClient.
 */

export const COMPOSE_SPEC_SCHEMA = "synth.visual.compose_spec.v1" as const;

export const COMPOSE_COMPONENTS = {
  "event_stream.v1": {
    kind: "event_stream",
    protocolId: "event_stream.v1",
    consumes: ["stream"],
    emits: ["cursor"]
  },
  "detail_modal.v1": {
    kind: "detail_modal",
    protocolId: "detail_modal.v1",
    consumes: ["cursor"],
    emits: []
  }
} as const;

export type ComposeComponentId = keyof typeof COMPOSE_COMPONENTS;

export type ComposePlacement = {
  id: string;
  component: ComposeComponentId;
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
    placements.push({
      id,
      component,
      slot: asString(row.slot),
      from: asString(row.from),
      config: Array.isArray(includeKinds)
        ? { includeKinds: includeKinds.filter((kind): kind is string => typeof kind === "string") }
        : undefined
    });
  }
  for (const placement of placements) {
    if (placement.component === "event_stream.v1") {
      const slot = placement.slot ?? "stream";
      if (slot !== "stream") {
        return { ok: false, error: `Placement "${placement.id}" must consume slot "stream"` };
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
      if (source.component !== "event_stream.v1") {
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
