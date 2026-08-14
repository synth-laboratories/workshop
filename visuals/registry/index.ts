/**
 * Visual template registry — list + resolve by id.
 *
 * Two roots, one catalog:
 *
 *   templates/           public, versioned, ships in the release
 *   templates-internal/  private, gitignored, staged from ~/.synth at build time
 *
 * Both globs are resolved by the bundler at build time, so the renderer never
 * loads template code at runtime and `arbitraryTsxExecuted` stays false. A
 * public release simply builds with an empty internal root and therefore
 * contains no internal template — there is no runtime flag to get wrong.
 */

import type { VisualTemplate, VisualTemplateMeta } from "../runtime/types.ts";

type ShellModule = {
  Shell: (props: Record<string, unknown>) => unknown;
  default: (props: Record<string, unknown>) => unknown;
};

const publicManifests = import.meta.glob("../templates/*/template.json", { eager: true, import: "default" }) as Record<string, VisualTemplateMeta>;
const internalManifests = import.meta.glob("../templates-internal/*/template.json", { eager: true, import: "default" }) as Record<string, VisualTemplateMeta>;

const INTERNAL_IDS = new Set(Object.values(internalManifests).map((meta) => meta.id));

function rootFor(id: string): string {
  return INTERNAL_IDS.has(id) ? `templates-internal/${id}` : `templates/${id}`;
}

/** A private template is never silently public: the flag is derived from its root. */
function withDistribution(meta: VisualTemplateMeta): VisualTemplate {
  const internal = INTERNAL_IDS.has(meta.id);
  return {
    ...meta,
    distribution: internal ? "internal" : "public",
    root: rootFor(meta.id)
  } as VisualTemplate;
}

// A public id and an internal id must not collide; the public one wins so a
// private drop-in can never shadow a reviewed, shipped template.
const METAS = [
  ...Object.values(publicManifests),
  ...Object.values(internalManifests).filter(
    (meta) => !Object.values(publicManifests).some((pub) => pub.id === meta.id)
  )
].sort((a, b) => a.id.localeCompare(b.id));

/** Dynamic shell importers for Desktop bundlers that support import(). */
const publicShells = import.meta.glob("../templates/*/shell.tsx") as Record<string, () => Promise<ShellModule>>;
const internalShells = import.meta.glob("../templates-internal/*/shell.tsx") as Record<string, () => Promise<ShellModule>>;

function importersFrom(
  modules: Record<string, () => Promise<ShellModule>>,
  dir: string
): Record<string, () => Promise<ShellModule>> {
  const pattern = new RegExp(`${dir}/([^/]+)/shell\\.tsx$`);
  return Object.fromEntries(
    Object.entries(modules)
      .map(([path, importer]) => [path.match(pattern)?.[1], importer] as const)
      .filter(([id]) => Boolean(id))
  ) as Record<string, () => Promise<ShellModule>>;
}

export const shellImporters = {
  ...importersFrom(internalShells, "templates-internal"),
  ...importersFrom(publicShells, "templates")
} as Record<string, () => Promise<ShellModule>>;

export function listTemplates(): VisualTemplate[] {
  return METAS.map(withDistribution);
}

export function resolveTemplate(id: string): VisualTemplate | undefined {
  const meta = METAS.find((t) => t.id === id);
  if (!meta) return undefined;
  return withDistribution(meta);
}

export function getShellImporter(id: string) {
  return shellImporters[id];
}

/** True when the id resolves to a private template staged from ~/.synth. */
export function isInternalTemplate(id: string): boolean {
  return INTERNAL_IDS.has(id);
}

export const TEMPLATE_IDS = METAS.map((t) => t.id);
export const INTERNAL_TEMPLATE_IDS = [...INTERNAL_IDS].sort();

export type { VisualTemplate, VisualTemplateMeta, VisualInstance, VisualBinding } from "../runtime/types.ts";
export { bindTemplateSlots, subscribeLiveSlot, isVisualBindings, bindingSlots, propsFromBindings } from "../runtime/bind.ts";
export { ingestLiveEnvelopes, assertLiveEvalSlot, LIVE_EVAL_SLOT } from "../runtime/liveStream.ts";
export { saveVisualInstanceTsx, renderInstanceTsx, markInstanceSaved } from "../runtime/save_tsx.ts";
