import type { VisualAction } from "@synth/visuals-protocol";
import type { VisualsApi } from "./api.ts";

export type McpTool = { name: string; description: string; inputSchema: Record<string, unknown> };

export function visualsMcpTools(): McpTool[] {
  const visualId = { type: "string", description: "Stable visual artifact/session identity" };
  return [
    { name: "visual_inspect", description: "Inspect the machine-readable semantic scene for the same visual state a human sees.", inputSchema: { type: "object", additionalProperties: false, properties: { visual_id: visualId }, required: ["visual_id"] } },
    { name: "visual_interact", description: "Apply a typed semantic action with optimistic state-version checking.", inputSchema: { type: "object", additionalProperties: false, properties: { visual_id: visualId, action: { type: "object" } }, required: ["visual_id", "action"] } },
    { name: "visual_capture", description: "Capture a coherent semantic snapshot and bind any host rendition references.", inputSchema: { type: "object", additionalProperties: false, properties: { visual_id: visualId, rendition_refs: { type: "array", items: { type: "string" } } }, required: ["visual_id"] } },
    { name: "visual_record", description: "Start, stop, or inspect a semantic visual-session recording.", inputSchema: { type: "object", additionalProperties: false, properties: { visual_id: visualId, operation: { enum: ["start", "stop", "get"] } }, required: ["visual_id", "operation"] } },
  ];
}

export function createVisualsMcpAdapter(api: VisualsApi) {
  return {
    tools: visualsMcpTools,
    async call(name: string, args: Record<string, unknown>): Promise<unknown> {
      const visualId = String(args.visual_id ?? "");
      if (!visualId) throw new Error("visual_id is required");
      if (name === "visual_inspect") return api.query({ kind: "inspect", visualId });
      if (name === "visual_interact") return api.execute({ kind: "interact", visualId, action: args.action as VisualAction });
      if (name === "visual_capture") return api.execute({ kind: "capture_snapshot", visualId, renditionRefs: Array.isArray(args.rendition_refs) ? args.rendition_refs.map(String) : [] });
      if (name === "visual_record") {
        if (args.operation === "get") return api.query({ kind: "get_recording", visualId });
        if (args.operation === "start") return api.execute({ kind: "start_recording", visualId });
        if (args.operation === "stop") return api.execute({ kind: "stop_recording", visualId });
        throw new Error("visual_record operation must be start, stop, or get");
      }
      throw new Error(`Unknown visuals MCP tool ${name}`);
    },
  };
}
