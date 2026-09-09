import type { VisualValueSchema } from "@synth/visuals-protocol";
import { envelopeIdentity } from "./liveStream.ts";
import type { LiveEvalEvent } from "./types.ts";

/** Persist navigation intent, never a second copy of evidence. */
export const eventCursorSchema: VisualValueSchema = {
  type: "object", nullable: true, required: ["identity"], additionalProperties: false,
  properties: { identity: { type: "string" } },
};
export const composeCursorSchema: VisualValueSchema = {
  ...eventCursorSchema, required: ["identity", "placementId"],
  properties: { ...eventCursorSchema.properties, placementId: { type: "string" } },
};
export const pageMapSchema: VisualValueSchema = {
  type: "object", additionalProperties: { type: "number", minimum: 0, maximum: Number.MAX_SAFE_INTEGER },
};
export const pageTrailSchema: VisualValueSchema = {
  type: "array", maxItems: 4096, items: { type: "string", nullable: true },
};
export const swarmViewSchema: VisualValueSchema = {
  type: "object", required: ["filters", "label"], additionalProperties: false,
  properties: {
    label: {type:"string"},
    filters: {type:"object",additionalProperties:false,properties:{
      outcome:{type:"string",options:["success","failure","timeout"]},model:{type:"string"},
      behavior:{type:"string",options:["direct","recovery","tool_loop","exploratory","abandonment"]},
      rewardBand:{type:"string",options:["negative","low","medium","high"]},
    }},
  },
};
export const swarmHistorySchema: VisualValueSchema = {
  type:"array",maxItems:16,items:{type:"object",additionalProperties:false,
    required:["view","strategy","eventIndex"],properties:{
      view:swarmViewSchema,
      strategy:{type:"string",options:["representative","diverse","failure","boundary","outlier","random"]},
      selectedId:{type:"string"},eventIndex:{type:"number",minimum:0,maximum:Number.MAX_SAFE_INTEGER},
    },
  },
};

/** A missing/ambiguous identity remains unresolved; do not display stale evidence. */
export function resolveEventCursor(events: LiveEvalEvent[], identity: string | undefined): LiveEvalEvent | null {
  if (identity === undefined) return null;
  const matches = events.filter((event, index) => envelopeIdentity(event, index) === identity);
  return matches.length === 1 ? matches[0]! : null;
}
