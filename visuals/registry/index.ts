/**
 * Visual template registry — list + resolve by id.
 */

import type { VisualTemplate, VisualTemplateMeta } from "../runtime/types.ts";

const manifestModules = import.meta.glob("../families/**/template.json", { eager: true, import: "default" }) as Record<string, VisualTemplateMeta>;

type RegistryEntry = {
  meta: VisualTemplateMeta;
  root: string;
  manifestPath: string;
};

function templateRoot(manifestPath: string): string {
  const root = manifestPath.replace(/^\.\.\//, "").replace(/\/template\.json$/, "");
  if (!root.startsWith("families/") || root.includes("..") || root.startsWith("/")) {
    throw new Error(`Unsafe visual template path: ${manifestPath}`);
  }
  return root;
}

const ENTRIES: RegistryEntry[] = Object.entries(manifestModules)
  .sort(([left], [right]) => left.localeCompare(right))
  .map(([manifestPath, meta]) => ({ meta, root: templateRoot(manifestPath), manifestPath }));

const BY_ID = new Map<string, RegistryEntry>();
for (const entry of ENTRIES) {
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

const ORDERED_ENTRIES = [...BY_ID.values()].sort((left, right) => left.meta.id.localeCompare(right.meta.id));

type ShellModule = {
  Shell: (props: Record<string, unknown>) => unknown;
  default: (props: Record<string, unknown>) => unknown;
};

/** Dynamic shell importers for Desktop bundlers that support import(). */
const shellModules = import.meta.glob("../families/**/shell.tsx") as Record<string, () => Promise<ShellModule>>;
export const shellImporters = Object.fromEntries(ORDERED_ENTRIES.flatMap((entry) => {
  const importer = shellModules[`../${entry.root}/shell.tsx`];
  return importer ? [[entry.meta.id, importer] as const] : [];
})) as Record<string, () => Promise<ShellModule>>;

export function listTemplates(): VisualTemplate[] {
  return ORDERED_ENTRIES.map(({ meta, root }) => ({
    ...meta,
    root,
  }));
}

export function resolveTemplate(id: string): VisualTemplate | undefined {
  const entry = BY_ID.get(id);
  if (!entry) return undefined;
  return { ...entry.meta, root: entry.root };
}

export function getShellImporter(id: string) {
  return shellImporters[id];
}

export const TEMPLATE_IDS = ORDERED_ENTRIES.map((entry) => entry.meta.id);

export type { VisualTemplate, VisualTemplateMeta, VisualInstance, VisualBinding } from "../runtime/types.ts";
export { bindTemplateSlots, subscribeLiveSlot, isVisualBindings, bindingSlots, propsFromBindings } from "../runtime/bind.ts";
export { ingestLiveEnvelopes, assertLiveEvalSlot, LIVE_EVAL_SLOT } from "../runtime/liveStream.ts";
export { saveVisualInstanceTsx, renderInstanceTsx, markInstanceSaved } from "../runtime/save_tsx.ts";
