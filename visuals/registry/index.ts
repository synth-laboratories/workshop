/**
 * Visual template registry — list + resolve by id.
 *
 * Families are the v0.3 public catalog. `templates/` and `templates-internal/`
 * may add private overlays but never shadow a family id. `optimizer.dag.live.v1`
 * is a v0.4 surface and is not in this catalog.
 *
 * Those three tiers are all `import.meta.glob`, which resolves at build time and
 * therefore cannot see a template a user wrote after the bundle was built. The
 * catalog is consequently `static tiers union runtime user templates`: the host
 * loads the runtime set through `setRuntimeTemplateLoader` (Desktop hands it
 * `visuals_templates_list`, whose Rust registry scans
 * `<state root>/visuals/templates/`), and every read below merges the two.
 * The no-shadow rule holds across the seam in the same direction as within it:
 * a runtime template may never take an id a bundled one already owns. Rust
 * hard-errors on that collision; this side refuses it a second time rather than
 * trusting the wire, because the consequence -- a shipped id silently meaning
 * different code on one machine -- is the thing the rule exists to prevent.
 */

import type { VisualTemplate, VisualTemplateMeta, VisualTemplateSlot } from "../runtime/types.ts";

const familyManifests = import.meta.glob("../families/**/template.json", { eager: true, import: "default" }) as Record<string, VisualTemplateMeta>;
const publicManifests = import.meta.glob("../templates/*/template.json", { eager: true, import: "default" }) as Record<string, VisualTemplateMeta>;
const internalManifests = import.meta.glob("../templates-internal/*/template.json", { eager: true, import: "default" }) as Record<string, VisualTemplateMeta>;

type RegistryEntry = {
  meta: VisualTemplateMeta;
  root: string;
  manifestPath: string;
  /** Set only on an entry discovered at runtime under the instance state root. */
  sourceKind?: typeof USER_TEMPLATE_SOURCE_KIND;
};

/**
 * `source_kind` the Rust registry tags a `template.json` + `shell.tsx`
 * directory with. Everything downstream branches on this, never on a template
 * id: `sourced.visual.v1` is one template that compiles in the pane, and
 * "compiles in the pane" is a property many templates now have.
 */
export const USER_TEMPLATE_SOURCE_KIND = "user" as const;

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

// v0.8 development builds briefly persisted this pre-registration spelling.
// Keep those immutable run artifacts inspectable while all new producers use
// the canonical family id.
const LEGACY_TEMPLATE_IDS: Readonly<Record<string, string>> = {
  "trace.workbench.v1": "craftax.trace_workbench.v1"
};

export function canonicalTemplateId(id: string): string {
  return LEGACY_TEMPLATE_IDS[id] ?? id;
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
  if (entry.sourceKind === USER_TEMPLATE_SOURCE_KIND) {
    return {
      ...entry.meta,
      distribution: "user",
      root: entry.root,
      sourceKind: USER_TEMPLATE_SOURCE_KIND,
    } as VisualTemplate;
  }
  const internal = entry.root.startsWith("templates-internal/");
  return {
    ...entry.meta,
    distribution: internal ? "internal" : "public",
    root: entry.root,
  } as VisualTemplate;
}

// ---------------------------------------------------------------------------
// Runtime tier: templates the bundler never saw.
// ---------------------------------------------------------------------------

/**
 * One row as the host registry serves it. Deliberately loose: this is the wire
 * shape of Rust's `TemplateMeta`, whose optional fields are optional on the
 * wire too, and a manifest a user hand-edited is exactly the input that will
 * not match a strict type.
 */
export type RuntimeTemplateRecord = {
  id?: string | null;
  title?: string | null;
  genre?: string | null;
  version?: string | null;
  description?: string | null;
  path?: string | null;
  shellPath?: string | null;
  sourceKind?: string | null;
  inputs?: unknown;
  slots?: unknown;
  components?: unknown;
  observationContract?: unknown;
};

/** What one registration round accepted, and what it refused and why. */
export type RuntimeTemplateSnapshot = {
  /** Ids now resolvable that the bundle does not contain. */
  accepted: string[];
  /** Ids a runtime template asked for that a bundled template already owns. */
  shadowed: string[];
  /** Set when the host could not be asked at all. Previously accepted ids stand. */
  error?: string;
};

export type RuntimeTemplateLoader = () => Promise<RuntimeTemplateRecord[]>;

const RUNTIME_BY_ID = new Map<string, RegistryEntry>();
/**
 * Ids the runtime tier has served at least once in this session.
 *
 * Never pruned, on purpose. A user template can leave the catalog while a pane
 * is still showing it — the file was deleted, renamed, or edited into a state
 * the registry refuses — and `RUNTIME_BY_ID` alone cannot tell that apart from
 * an id that was never real. Without this the pane would fall through to the
 * bundled loader, find no shell, and blank; with it the pane can say which of
 * the two happened, in the pane, where the author is looking.
 */
const RUNTIME_EVER = new Set<string>();
const runtimeListeners = new Set<() => void>();
let runtimeSnapshot: RuntimeTemplateSnapshot = { accepted: [], shadowed: [] };
let runtimeGeneration = 0;
let runtimeLoader: RuntimeTemplateLoader | null = null;
let runtimePending: Promise<RuntimeTemplateSnapshot> | null = null;

function announceRuntimeTemplates(snapshot: RuntimeTemplateSnapshot): RuntimeTemplateSnapshot {
  runtimeSnapshot = snapshot;
  runtimeGeneration += 1;
  for (const listener of [...runtimeListeners]) listener();
  return snapshot;
}

function text(value: unknown, fallback: string): string {
  return typeof value === "string" && value.trim().length > 0 ? value : fallback;
}

function slotList(value: unknown): VisualTemplateSlot[] {
  return Array.isArray(value) ? (value as VisualTemplateSlot[]) : [];
}

/**
 * A runtime manifest fills the same shape a bundled one does, so every consumer
 * downstream — binding resolution, review, the observation contract, sealing —
 * reads a user template through exactly the code path it reads a family
 * through. Missing optional fields become the empty value rather than
 * `undefined`, because a half-typed manifest must render, not throw.
 */
function runtimeMeta(record: RuntimeTemplateRecord, id: string): VisualTemplateMeta {
  const inputs = slotList(record.inputs ?? record.slots);
  const slots = slotList(record.slots ?? record.inputs);
  return {
    schemaVersion: "synth.visual-template.v1",
    id,
    title: text(record.title, id),
    genre: text(record.genre, "user"),
    version: text(record.version, "0.0.0"),
    description: text(record.description, ""),
    inputs,
    slots,
    shell: "shell.tsx",
    ...(Array.isArray(record.components)
      ? { components: record.components as VisualTemplateMeta["components"] }
      : {}),
    ...(record.observationContract
      ? { observationContract: record.observationContract as VisualTemplateMeta["observationContract"] }
      : {}),
  };
}

/**
 * Replace the runtime tier with `records`, keeping only user-authored ones.
 *
 * The host list contains every tier, so the `sourceKind` filter is what keeps a
 * bundled row from being re-registered as a runtime entry with no shell
 * importer. A row whose id a bundled template already owns is refused and
 * reported: no silent override in either direction.
 */
export function registerRuntimeTemplates(
  records: RuntimeTemplateRecord[],
  options: { quiet?: boolean } = {},
): RuntimeTemplateSnapshot {
  const next = new Map<string, RegistryEntry>();
  const shadowed: string[] = [];
  for (const record of Array.isArray(records) ? records : []) {
    if (!record || record.sourceKind !== USER_TEMPLATE_SOURCE_KIND) continue;
    const id = typeof record.id === "string" ? record.id.trim() : "";
    if (!id) continue;
    if (BY_ID.has(id) || next.has(id)) {
      shadowed.push(id);
      continue;
    }
    const root = text(record.path, id);
    const directoryId = root.slice(root.lastIndexOf("/") + 1);
    // Same invariant the static tiers assert, for the same reason: the id is
    // the directory, so one id cannot name two directories on disk.
    if (directoryId !== id) continue;
    next.set(id, {
      meta: runtimeMeta(record, id),
      root,
      manifestPath: `${root}/template.json`,
      sourceKind: USER_TEMPLATE_SOURCE_KIND,
    });
  }
  RUNTIME_BY_ID.clear();
  for (const [id, entry] of next) {
    RUNTIME_BY_ID.set(id, entry);
    RUNTIME_EVER.add(id);
  }
  const snapshot: RuntimeTemplateSnapshot = {
    accepted: [...next.keys()].sort((left, right) => left.localeCompare(right)),
    shadowed: shadowed.sort((left, right) => left.localeCompare(right)),
  };
  // A quiet round updates the map without waking anyone. It exists because
  // announcing is not free: every listener re-reads, and `VisualHost` answers
  // by re-reading and recompiling the source it is showing, which throws away
  // whatever state the mounted shell was holding. A caller that cannot tell
  // whether anything changed — the focus rescan — must not pay that on every
  // focus, so it asks quietly and only wakes the pane when the catalog moved.
  // The watcher is the opposite case: it already knows the bytes changed, and
  // an unchanged id list is exactly what a content edit looks like.
  if (options.quiet && sameSnapshot(snapshot, runtimeSnapshot)) {
    runtimeSnapshot = snapshot;
    return snapshot;
  }
  return announceRuntimeTemplates(snapshot);
}

function sameSnapshot(left: RuntimeTemplateSnapshot, right: RuntimeTemplateSnapshot): boolean {
  const sameIds = (a: string[], b: string[]) => a.length === b.length && a.every((id, index) => id === b[index]);
  return left.error === right.error && sameIds(left.accepted, right.accepted) && sameIds(left.shadowed, right.shadowed);
}

/** Install the host's runtime template source. Desktop does this at bridge install. */
export function setRuntimeTemplateLoader(loader: RuntimeTemplateLoader | null): void {
  runtimeLoader = loader;
  runtimePending = null;
}

async function loadRuntimeTemplates(quiet: boolean): Promise<RuntimeTemplateSnapshot> {
  const loader = runtimeLoader;
  if (!loader) return runtimeSnapshot;
  try {
    return registerRuntimeTemplates(await loader(), { quiet });
  } catch (reason) {
    // Never reject: a host that cannot be asked must not blank a pane. The
    // previously accepted ids stand and the reason travels in the snapshot, so
    // the pane can say why the template it wants is unavailable.
    const failed = {
      ...runtimeSnapshot,
      error: reason instanceof Error ? reason.message : String(reason),
    };
    if (quiet && sameSnapshot(failed, runtimeSnapshot)) {
      runtimeSnapshot = failed;
      return failed;
    }
    return announceRuntimeTemplates(failed);
  }
}

/** Load the runtime tier once. Repeat calls share the first load. */
export function ensureRuntimeTemplates(): Promise<RuntimeTemplateSnapshot> {
  if (!runtimeLoader) return Promise.resolve(runtimeSnapshot);
  runtimePending ??= loadRuntimeTemplates(false);
  return runtimePending;
}

/**
 * Re-ask the host and wake every listener — the caller knows something changed.
 *
 * The file watcher's callback: it fires only after the bytes under the user
 * template root actually moved, and a content edit that leaves the id list
 * identical still has to reach the pane, so this announces unconditionally.
 */
export function refreshRuntimeTemplates(): Promise<RuntimeTemplateSnapshot> {
  runtimePending = null;
  return ensureRuntimeTemplates();
}

/**
 * Re-ask the host and wake listeners only if the catalog moved.
 *
 * For callers that poll on a hunch — window focus — where announcing every
 * time would remount every open pane for nothing.
 */
export function rescanRuntimeTemplates(): Promise<RuntimeTemplateSnapshot> {
  if (!runtimeLoader) return Promise.resolve(runtimeSnapshot);
  runtimePending = loadRuntimeTemplates(true);
  return runtimePending;
}

/** Bumped whenever the runtime tier changes, so a view can re-read. */
export function runtimeTemplatesVersion(): number {
  return runtimeGeneration;
}

export function runtimeTemplates(): RuntimeTemplateSnapshot {
  return runtimeSnapshot;
}

export function onRuntimeTemplatesChanged(listener: () => void): () => void {
  runtimeListeners.add(listener);
  return () => { runtimeListeners.delete(listener); };
}

/**
 * True when this id is a user-authored template: its shell is `shell.tsx` under
 * the instance state root, compiled in the pane rather than imported from the
 * bundle. The pane branches on this, not on `id === "sourced.visual.v1"`.
 */
export function isUserTemplate(id: string): boolean {
  return !BY_ID.has(id) && RUNTIME_BY_ID.has(id);
}

/**
 * True when this id was a user template in this session but is not one now.
 *
 * The answer to "why did my pane stop working", and the difference between a
 * message the author can act on and a blank rectangle. `runtimeTemplates()`
 * carries the reason when there is one — a host that could not be asked — and
 * its absence means the directory simply left the root.
 */
export function wasUserTemplate(id: string): boolean {
  return !BY_ID.has(id) && !RUNTIME_BY_ID.has(id) && RUNTIME_EVER.has(id);
}

export function listTemplates(): VisualTemplate[] {
  return [...ORDERED_ENTRIES, ...RUNTIME_BY_ID.values()]
    .sort((left, right) => left.meta.id.localeCompare(right.meta.id))
    .map(withDistribution);
}

export function resolveTemplate(id: string): VisualTemplate | undefined {
  // Bundled first, always: the runtime tier can add ids, never redefine one.
  const entry = BY_ID.get(id) ?? RUNTIME_BY_ID.get(id) ?? BY_ID.get(canonicalTemplateId(id));
  if (!entry) return undefined;
  return withDistribution(entry);
}

/**
 * Static shell importer, bundled tiers only. A user template has none by
 * construction — it is not in the module graph — and must not silently fall
 * back to another template's shell; `VisualHost` compiles its source instead.
 */
export function getShellImporter(id: string) {
  return shellImporters[id];
}

export function isInternalTemplate(id: string): boolean {
  return INTERNAL_IDS.has(id);
}

export const TEMPLATE_IDS = ORDERED_ENTRIES.map((entry) => entry.meta.id);
export const INTERNAL_TEMPLATE_IDS = [...INTERNAL_IDS].sort();

export type { VisualTemplate, VisualTemplateMeta, VisualInstance, VisualBinding } from "../runtime/types.ts";
export { bindingInputName, bindingList, resolveInputName, stampBindingInput, templateInputs } from "../runtime/types.ts";
export { bindTemplateSlots, subscribeLiveSlot, isVisualBindings, bindingSlots, propsFromBindings, resolveVisualBindings } from "../runtime/bind.ts";
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
