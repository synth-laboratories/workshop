/**
 * Visual template registry — list + resolve by id.
 */

import type { VisualTemplate, VisualTemplateMeta } from "../runtime/types.ts";

import craftaxEvalMatrix from "../templates/craftax.eval_matrix.v1/template.json";
import craftaxRolloutScrub from "../templates/craftax.rollout_scrub.v1/template.json";
import posttrainRolloutViewer from "../templates/posttrain.rollout_viewer.v1/template.json";
import rewardBreakdown from "../templates/reward.breakdown.v1/template.json";
import annotationOverlay from "../templates/annotation.overlay.v1/template.json";
import modelCompare from "../templates/model.compare.v1/template.json";
import liveEvalStream from "../templates/live.eval_stream.v1/template.json";
import liveDockHarbor from "../templates/live.dock_harbor.v1/template.json";
import liveInternAcceptance from "../templates/live.intern_acceptance.v1/template.json";

const METAS: VisualTemplateMeta[] = [
  craftaxEvalMatrix,
  craftaxRolloutScrub,
  posttrainRolloutViewer,
  rewardBreakdown,
  annotationOverlay,
  modelCompare,
  liveEvalStream,
  liveDockHarbor,
  liveInternAcceptance
] as VisualTemplateMeta[];

/** Package-relative template roots (from visuals/). */
const ROOTS: Record<string, string> = {
  "craftax.eval_matrix.v1": "templates/craftax.eval_matrix.v1",
  "craftax.rollout_scrub.v1": "templates/craftax.rollout_scrub.v1",
  "posttrain.rollout_viewer.v1": "templates/posttrain.rollout_viewer.v1",
  "reward.breakdown.v1": "templates/reward.breakdown.v1",
  "annotation.overlay.v1": "templates/annotation.overlay.v1",
  "model.compare.v1": "templates/model.compare.v1",
  "live.eval_stream.v1": "templates/live.eval_stream.v1",
  "live.dock_harbor.v1": "templates/live.dock_harbor.v1",
  "live.intern_acceptance.v1": "templates/live.intern_acceptance.v1"
};

type ShellModule = {
  Shell: (props: Record<string, unknown>) => unknown;
  default: (props: Record<string, unknown>) => unknown;
};

/** Dynamic shell importers for Desktop bundlers that support import(). */
export const shellImporters: Record<string, () => Promise<ShellModule>> = {
  "craftax.eval_matrix.v1": () => import("../templates/craftax.eval_matrix.v1/shell.tsx"),
  "craftax.rollout_scrub.v1": () => import("../templates/craftax.rollout_scrub.v1/shell.tsx"),
  "posttrain.rollout_viewer.v1": () => import("../templates/posttrain.rollout_viewer.v1/shell.tsx"),
  "reward.breakdown.v1": () => import("../templates/reward.breakdown.v1/shell.tsx"),
  "annotation.overlay.v1": () => import("../templates/annotation.overlay.v1/shell.tsx"),
  "model.compare.v1": () => import("../templates/model.compare.v1/shell.tsx"),
  "live.eval_stream.v1": () => import("../templates/live.eval_stream.v1/shell.tsx"),
  "live.dock_harbor.v1": () => import("../templates/live.dock_harbor.v1/shell.tsx"),
  "live.intern_acceptance.v1": () => import("../templates/live.intern_acceptance.v1/shell.tsx")
};

export function listTemplates(): VisualTemplate[] {
  return METAS.map((meta) => ({
    ...meta,
    root: ROOTS[meta.id] ?? `templates/${meta.id}`
  }));
}

export function resolveTemplate(id: string): VisualTemplate | undefined {
  const meta = METAS.find((t) => t.id === id);
  if (!meta) return undefined;
  return { ...meta, root: ROOTS[meta.id] ?? `templates/${meta.id}` };
}

export function getShellImporter(id: string) {
  return shellImporters[id];
}

export const TEMPLATE_IDS = METAS.map((t) => t.id);

export type { VisualTemplate, VisualTemplateMeta, VisualInstance, VisualBinding } from "../runtime/types.ts";
export { bindTemplateSlots, subscribeLiveSlot } from "../runtime/bind.ts";
export { saveVisualInstanceTsx, renderInstanceTsx, markInstanceSaved } from "../runtime/save_tsx.ts";
