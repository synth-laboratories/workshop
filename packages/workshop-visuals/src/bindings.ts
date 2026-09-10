import type { JsonValue } from "@synth/visuals-protocol";
import { BindingResolverRegistry, loaderBindingResolver } from "@synth/visuals-sdk";

export type WorkshopBindingLoaders = Partial<Record<
  "trace_v5" | "run_ref" | "optimizer_run" | "query_snapshot" | "annotation_evidence_head" | "verifier_result_v2",
  (source: string, signal: AbortSignal) => Promise<JsonValue>
>>;

const WORKSHOP_KINDS = ["trace_v5", "run_ref", "optimizer_run", "query_snapshot", "annotation_evidence_head", "verifier_result_v2"] as const;

export function registerWorkshopBindingResolvers(registry: BindingResolverRegistry, loaders: WorkshopBindingLoaders): void {
  for (const kind of WORKSHOP_KINDS) {
    const load = loaders[kind];
    if (!load) continue;
    registry.register(loaderBindingResolver({
      kind,
      id: `workshop.binding.${kind}`,
      load,
      authority: kind === "query_snapshot" ? "derived" : "authoritative",
    }));
  }
}
