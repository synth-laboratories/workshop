/** Pure SFT workspace derivations, importable from node tests. */

import type { ProjectedState } from "../../components/projectEvents.ts";
import type { WorkspaceStage } from "../../components/workspace/WorkspaceChrome.tsx";

export type SftState = NonNullable<ProjectedState["sft"]>;

export const SFT_TERMINAL_STATUSES = ["completed", "failed", "canceled", "cancelled", "succeeded"];

/** Derive the semantic SFT stages from what the event stream actually shows. */
export function sftStages(sft: SftState, status: string, promotedCheckpointId?: string): WorkspaceStage[] {
  const terminal = SFT_TERMINAL_STATUSES.includes(status);
  const failed = status === "failed";
  const datasetReady = Object.keys((sft.dataset.splits as Record<string, unknown> | undefined) ?? {}).length > 0;
  const trainingStarted = sft.points.length > 0;
  const checkpointCount = sft.checkpoints.length;
  const readyCount = sft.checkpoints.filter((ckpt) => ckpt.ready === true || ckpt.promoted === true).length;
  const campaignCount = sft.campaigns.length;
  const campaignsSettled = campaignCount > 0 && sft.campaigns.every((campaign) =>
    ["completed", "failed"].includes(String(campaign.status ?? ""))
  );
  const promoted = promotedCheckpointId != null ||
    sft.checkpoints.some((ckpt) => ckpt.promoted === true);
  const settle = (started: boolean, done: boolean): WorkspaceStage["status"] => {
    if (done) return "completed";
    if (started) return terminal ? (failed ? "failed" : "completed") : "active";
    return terminal ? "skipped" : "pending";
  };
  return [
    { id: "dataset", label: "Dataset", status: datasetReady ? "completed" : terminal ? "skipped" : "pending" },
    {
      id: "training",
      label: "Training",
      status: settle(trainingStarted, trainingStarted && terminal && !failed),
      detail: trainingStarted ? `${sft.points.length} metric records` : undefined
    },
    {
      id: "checkpoints",
      label: "Checkpoints",
      status: settle(checkpointCount > 0, checkpointCount > 0 && readyCount === checkpointCount && terminal),
      detail: checkpointCount > 0 ? `${readyCount}/${checkpointCount} ready` : undefined
    },
    {
      id: "evaluation",
      label: "Eval campaigns",
      status: settle(campaignCount > 0, campaignsSettled),
      detail: campaignCount > 0 ? `${campaignCount} campaign${campaignCount === 1 ? "" : "s"}` : undefined
    },
    {
      id: "promotion",
      label: "Promotion",
      status: promoted ? "completed" : terminal ? "skipped" : "pending",
      detail: promoted ? undefined : "requires an explicit promote event — checkpoint 'ready' is not promotion"
    }
  ];
}
