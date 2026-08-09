/**
 * Visual template registry — list + resolve by id.
 */

import type { VisualTemplate, VisualTemplateMeta } from "../runtime/types.ts";

const manifestModules = import.meta.glob("../templates/*/template.json", { eager: true, import: "default" }) as Record<string, VisualTemplateMeta>;
const METAS = Object.values(manifestModules).sort((a, b) => a.id.localeCompare(b.id));

type ShellModule = {
  Shell: (props: Record<string, unknown>) => unknown;
  default: (props: Record<string, unknown>) => unknown;
};

/** Dynamic shell importers for Desktop bundlers that support import(). */
const shellModules = import.meta.glob("../templates/*/shell.tsx") as Record<string, () => Promise<ShellModule>>;
export const shellImporters = Object.fromEntries(Object.entries(shellModules).map(([path, importer]) => {
  const id = path.match(/templates\/([^/]+)\/shell\.tsx$/)?.[1];
  return [id, importer];
}).filter(([id]) => Boolean(id))) as Record<string, () => Promise<ShellModule>>;

export function listTemplates(): VisualTemplate[] {
  return METAS.map((meta) => ({
    ...meta,
    root: `templates/${meta.id}`
  }));
}

export function resolveTemplate(id: string): VisualTemplate | undefined {
  const meta = METAS.find((t) => t.id === id);
  if (!meta) return undefined;
  return { ...meta, root: `templates/${meta.id}` };
}

export function getShellImporter(id: string) {
  return shellImporters[id];
}

export const TEMPLATE_IDS = METAS.map((t) => t.id);

export type { VisualTemplate, VisualTemplateMeta, VisualInstance, VisualBinding } from "../runtime/types.ts";
export { bindTemplateSlots, subscribeLiveSlot, isVisualBindings, propsFromBindings } from "../runtime/bind.ts";
export { saveVisualInstanceTsx, renderInstanceTsx, markInstanceSaved } from "../runtime/save_tsx.ts";
