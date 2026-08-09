/**
 * Serialize a VisualInstance shell to a .tsx file under visuals/instances/.
 */

import type { VisualBinding, VisualInstance } from "./types.ts";

export type SaveTsxOptions = {
  /** Absolute path to visuals/ root. */
  visualsRoot: string;
  /** Write implementation — Desktop / MCP injects fs.writeFile. */
  writeFile: (absPath: string, contents: string) => Promise<void> | void;
  /** Optional mkdir — created if provided. */
  mkdir?: (absPath: string) => Promise<void> | void;
  /** Join path segments. Defaults to `/` join. */
  joinPath?: (...parts: string[]) => string;
};

function defaultJoin(...parts: string[]): string {
  return parts
    .map((p, i) => (i === 0 ? p.replace(/\/+$/, "") : p.replace(/^\/+|\/+$/g, "")))
    .filter(Boolean)
    .join("/");
}

function sanitizeId(id: string): string {
  return id.replace(/[^a-zA-Z0-9._-]+/g, "_").slice(0, 120);
}

function bindingsLiteral(bindings: VisualBinding[]): string {
  return JSON.stringify(bindings, null, 2);
}

function propsLiteral(props: Record<string, unknown> | undefined): string {
  return JSON.stringify(props ?? {}, null, 2);
}

/**
 * Generate a self-contained TSX module that re-exports the template shell
 * with frozen bindings + props for Desktop to open in the visual pane.
 */
export function renderInstanceTsx(instance: VisualInstance): string {
  const componentName =
    "Visual_" +
    sanitizeId(instance.id)
      .replace(/^[0-9]/, "N")
      .replace(/[^a-zA-Z0-9_]/g, "_");

  return `/**
 * Auto-generated Synth visual instance.
 * template: ${instance.templateId}
 * id: ${instance.id}
 * Do not edit by hand unless forking — prefer MCP visual_save_tsx / bind tools.
 */
import { useMemo } from "react";
import { Shell } from "../templates/${instance.templateId}/shell.tsx";
import type { VisualBinding } from "../runtime/types.ts";

export const instanceId = ${JSON.stringify(instance.id)};
export const templateId = ${JSON.stringify(instance.templateId)};
export const title = ${JSON.stringify(instance.title)};

export const bindings: VisualBinding[] = ${bindingsLiteral(instance.bindings)};

export const instanceProps = ${propsLiteral(instance.props)} as Record<string, unknown>;

export default function ${componentName}() {
  const props = useMemo(
    () => ({
      ...instanceProps,
      title: (instanceProps.title as string | undefined) ?? title,
      bindings
    }),
    []
  );
  return <Shell {...props} />;
}
`;
}

/**
 * Write instance TSX under visuals/instances/<id>.tsx and return the relative path.
 */
export async function saveVisualInstanceTsx(
  instance: VisualInstance,
  options: SaveTsxOptions
): Promise<{ absPath: string; relativePath: string; contents: string }> {
  const join = options.joinPath ?? defaultJoin;
  const fileName = `${sanitizeId(instance.id)}.tsx`;
  const relativePath = join("instances", fileName);
  const absPath = join(options.visualsRoot, relativePath);
  const instancesDir = join(options.visualsRoot, "instances");

  if (options.mkdir) {
    await options.mkdir(instancesDir);
  }

  const contents = renderInstanceTsx(instance);
  await options.writeFile(absPath, contents);

  return { absPath, relativePath, contents };
}

/** Stamp saved metadata onto an instance (pure). */
export function markInstanceSaved(
  instance: VisualInstance,
  relativePath: string,
  now: string = new Date().toISOString()
): VisualInstance {
  return {
    ...instance,
    status: "saved",
    tsxPath: relativePath,
    updatedAt: now
  };
}
