/**
 * Visual template registry — list + resolve by id.
 *
 * Families are the v0.3 public catalog. `templates/` and `templates-internal/`
 * may add private overlays but never shadow a family id. `optimizer.dag.live.v1`
 * is a v0.4 surface and is not in this catalog.
 */

import type { VisualTemplate, VisualTemplateMeta } from "../runtime/types.ts";
import { VisualExtensionRegistry } from "@synth/visuals-sdk";
import { definitionFromWorkshopTemplate, swarmTrajectoryDefinition } from "@synth/workshop-visuals";

const familyManifests = import.meta.glob("../families/**/template.json", { eager: true, import: "default" }) as Record<string, VisualTemplateMeta>;
const publicManifests = import.meta.glob("../templates/*/template.json", { eager: true, import: "default" }) as Record<string, VisualTemplateMeta>;
const internalManifests = import.meta.glob("../templates-internal/*/template.json", { eager: true, import: "default" }) as Record<string, VisualTemplateMeta>;

type RegistryEntry = {
  meta: VisualTemplateMeta;
  root: string;
  manifestPath: string;
};

function templateRoot(manifestPath: string): string {
  const root = manifestPath.replace(/^\.\.\//, "").replace(/\/template\.json$/, "");
  if (
    !(root.startsWith("families/") || root.startsWith("templates/") || root.startsWith("templates-internal/"))
    || root.includes("..")
    || root.startsWith("/")
  ) {
    throw new Error(`Unsafe visual template path: ${manifestPath}`);
  }
  return root;
}

const familyEntries: RegistryEntry[] = Object.entries(familyManifests)
  .sort(([left], [right]) => left.localeCompare(right))
  .map(([manifestPath, meta]) => ({ meta, root: templateRoot(manifestPath), manifestPath }));

const BY_ID = new Map<string, RegistryEntry>();
for (const entry of familyEntries) {
  const directoryId = entry.root.slice(entry.root.lastIndexOf("/") + 1);
  if (!entry.meta.id || entry.meta.id !== directoryId) {
    throw new Error(`Visual template id ${JSON.stringify(entry.meta.id)} does not match directory ${entry.root}`);
  }
  const existing = BY_ID.get(entry.meta.id);
  if (existing) {
    throw new Error(
      `Duplicate visual template id ${JSON.stringify(entry.meta.id)} in ${existing.manifestPath} and ${entry.manifestPath}`,
    );
  }
  BY_ID.set(entry.meta.id, entry);
}

function overlay(manifests: Record<string, VisualTemplateMeta>, kind: "templates" | "templates-internal") {
  for (const [manifestPath, meta] of Object.entries(manifests).sort(([left], [right]) => left.localeCompare(right))) {
    const root = templateRoot(manifestPath);
    const directoryId = root.slice(root.lastIndexOf("/") + 1);
    if (!meta.id || meta.id !== directoryId) {
      throw new Error(`Visual template id ${JSON.stringify(meta.id)} does not match directory ${root}`);
    }
    if (BY_ID.has(meta.id)) continue;
    BY_ID.set(meta.id, { meta, root, manifestPath });
  }
  void kind;
}

overlay(publicManifests, "templates");
overlay(internalManifests, "templates-internal");

const ORDERED_ENTRIES = [...BY_ID.values()].sort((left, right) => left.meta.id.localeCompare(right.meta.id));
const INTERNAL_IDS = new Set(Object.values(internalManifests).map((meta) => meta.id));
const RUNTIME_TEMPLATES = new Map<string, VisualTemplate>();

/** Native registry metadata, never a shell import path supplied by a template. */
export function registerRuntimeTemplate(meta: Record<string, unknown>): void {
  if (meta.sourceKind !== "user" || typeof meta.id !== "string" || meta.schemaVersion !== "synth.visual-template.v1") {
    throw new Error("Invalid user-template metadata");
  }
  if (BY_ID.has(meta.id)) throw new Error(`User template cannot shadow bundled template ${meta.id}`);
  const inputs = meta.inputs ?? meta.slots;
  if (!Array.isArray(inputs) || inputs.some(input => !input || typeof input !== "object" || typeof input.name !== "string")) {
    throw new Error("User template has invalid input declarations");
  }
  RUNTIME_TEMPLATES.set(meta.id, {
    ...meta, title: String(meta.title ?? meta.id), genre: String(meta.genre ?? "custom"),
    version: String(meta.version ?? ""), description: String(meta.description ?? ""),
    shell: "shell.tsx", root: String(meta.path ?? ""), inputs, slots: inputs,
  } as VisualTemplate);
}

type ShellModule = {
  Shell: (props: Record<string, unknown>) => unknown;
  default: (props: Record<string, unknown>) => unknown;
};

const familyShells = import.meta.glob("../families/**/shell.tsx") as Record<string, () => Promise<ShellModule>>;
const publicShells = import.meta.glob("../templates/*/shell.tsx") as Record<string, () => Promise<ShellModule>>;
const internalShells = import.meta.glob("../templates-internal/*/shell.tsx") as Record<string, () => Promise<ShellModule>>;

export const shellImporters = Object.fromEntries(ORDERED_ENTRIES.flatMap((entry) => {
  const importer = familyShells[`../${entry.root}/shell.tsx`]
    ?? publicShells[`../${entry.root}/shell.tsx`]
    ?? internalShells[`../${entry.root}/shell.tsx`];
  return importer ? [[entry.meta.id, importer] as const] : [];
})) as Record<string, () => Promise<ShellModule>>;

function withDistribution(entry: RegistryEntry): VisualTemplate {
  const internal = entry.root.startsWith("templates-internal/");
  return {
    ...entry.meta,
    distribution: internal ? "internal" : "public",
    root: entry.root,
  } as VisualTemplate;
}

export function listTemplates(): VisualTemplate[] {
  return [...ORDERED_ENTRIES.map(withDistribution), ...RUNTIME_TEMPLATES.values()];
}

export function resolveTemplate(id: string): VisualTemplate | undefined {
  const entry = BY_ID.get(id);
  if (!entry) return RUNTIME_TEMPLATES.get(id);
  return withDistribution(entry);
}

export function getShellImporter(id: string) {
  return shellImporters[id];
}

export function isInternalTemplate(id: string): boolean {
  return INTERNAL_IDS.has(id);
}

export const TEMPLATE_IDS = ORDERED_ENTRIES.map((entry) => entry.meta.id);
export const INTERNAL_TEMPLATE_IDS = [...INTERNAL_IDS].sort();

/** Workshop's v0.10 composition root. The legacy template catalog remains the
 * source of bundled manifests while every entry is exposed through the core
 * definition/renderer registry. */
export const visualExtensions = new VisualExtensionRegistry();
for (const renderer of [
  { id: "workshop.component", formats: ["interactive", "html"] },
  { id: "template", formats: ["interactive", "html"] },
  { id: "tsx", formats: ["interactive", "html"] },
  { id: "html", formats: ["interactive", "html"] },
  { id: "mermaid", formats: ["interactive", "svg", "png"] },
  { id: "systems", formats: ["interactive", "svg", "png"] },
  { id: "systems-dynamic", formats: ["interactive", "svg", "png"] },
  { id: "chart", formats: ["interactive", "svg", "png"] },
]) {
  visualExtensions.registerRenderer({
    ...renderer,
    version: "1.0.0",
    isolation: renderer.id==="html"?"sandboxed_frame":"in_process",
    capabilities: ["interaction", "semantic_scene", "snapshot", "static_rendition"],
    protocolRange: "^1",
    authorities: ["none"],
    deterministic: ["mermaid","systems","chart"].includes(renderer.id),
  });
}
for (const entry of ORDERED_ENTRIES) {
  visualExtensions.registerDefinition(entry.meta.id === swarmTrajectoryDefinition.id
    ? swarmTrajectoryDefinition
    : definitionFromWorkshopTemplate(entry.meta));
}

export type { VisualTemplate, VisualTemplateMeta, VisualInstance, VisualBinding } from "../runtime/types.ts";
export { bindingInputName, bindingList, resolveInputName, stampBindingInput, templateInputs } from "../runtime/types.ts";
export { selectObservationSurface } from "../runtime/observationSurface.ts";
export { anonymousDataProp, bindTemplateSlots, subscribeLiveSlot, isVisualBindings, bindingSlots, propsFromBindings, resolveVisualBindings } from "../runtime/bind.ts";
export { comparisonContractVerdict } from "../runtime/evidenceGrammar.ts";
export type { ComparisonContract, ComparisonContractVerdict } from "../runtime/evidenceGrammar.ts";
export { selectRenderedProjection, rememberLastKnownGood } from "../runtime/lastKnownGood.ts";
export type { ProjectionSource, SelectedProjection } from "../runtime/lastKnownGood.ts";
export {
  consumeInjectedRendererCrash,
  resetInjectedRendererCrashes
} from "../runtime/crashInject.ts";
export { presentRuntimeError, presentRuntimeErrorMessage } from "../runtime/presentError.ts";
export { captureEvidenceKind, CAPTURE_REVIEW_PRODUCT_CLASSES } from "../runtime/captureEvidence.ts";
export type { ResolvedVisualBindings, VisualBindingsStatus } from "../runtime/bind.ts";
export {
  createReplayClient,
  parseReplayPage,
  replayStreamsFromBindings,
  REPLAY_FIRST_RESPONSE_TIMEOUT_MS,
  REPLAY_PAGE_LIMIT,
  REPLAY_PAGE_LIMIT_MAX
} from "../runtime/replayClient.ts";
export type { LiveTemplateProps, ReplayClient, ReplayCursor, ReplayPage, ReplayStream, TransportState } from "../runtime/replayClient.ts";
export {
  VISUAL_MEDIA_PROTOCOL,
  MEDIA_CACHE_LIMIT,
  MEDIA_PRELOAD_AHEAD,
  MEDIA_PRELOAD_BEHIND,
  NO_MEDIA,
  createMediaClient,
  isCasDigest,
  mediaRefFrom
} from "../runtime/mediaClient.ts";
export type { LoadedMedia, MediaClient, MediaRef, MediaTransport } from "../runtime/mediaClient.ts";
export {
  EVAL_TRACE_VIEW_SCHEMA,
  CRAFTAX_PROJECTION_KIND,
  containerEventsFromOptimizerEvents,
  containerEventsFromSealedTrace,
  craftaxTraceFromOptimizerEvents,
  craftaxTraceFromSealedTrace,
  craftaxTrialsFromRun,
  foldCraftaxTrace,
  localMapRows,
  reconcileCraftaxTrace
} from "../runtime/craftaxTraceView.ts";
export type {
  AppliedAction,
  ContainerEvent,
  EvalTraceView,
  RejectedAction,
  StateDelta,
  TraceCoverage,
  TraceFrame,
  TraceIdentity,
  TraceMessage,
  TraceStep,
  TraceToolCall,
  TrialView
} from "../runtime/craftaxTraceView.ts";
export { ingestLiveEnvelopes, assertLiveEvalSlot, LIVE_EVAL_INPUT, LIVE_EVAL_SLOT } from "../runtime/liveStream.ts";
export { saveVisualInstanceTsx, renderInstanceTsx, markInstanceSaved } from "../runtime/save_tsx.ts";
export {
  compileSourcedModule,
  isSourcedTemplate,
  sourcedInvalidShell,
  SOURCED_TEMPLATE_ID,
  validateSourcedSource
} from "../runtime/sourcedVisual.ts";
