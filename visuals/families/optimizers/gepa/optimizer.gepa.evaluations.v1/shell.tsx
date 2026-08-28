/** Focused evaluation-rollout view — the workspace's scalable browser. */

import { OptimizerFamilyShell } from "../../_shared/optimizer.run.v1/components/FamilyShell.tsx";
import { RolloutBrowser, type RolloutGroup, type RolloutRow } from "../../_shared/optimizer.run.v1/components/workspace/RolloutBrowser.tsx";
import { candidateName, stageTitle } from "../../_shared/optimizer.run.v1/overlays/gepa/model.ts";
import type { VisualBinding } from "../../../../runtime/types.ts";
import type { OptimizerEvent, OptimizerRun } from "../../_shared/optimizer.run.v1/components/projectEvents.ts";

export type ShellProps = {
  title?: string;
  lede?: string;
  data?: unknown;
  optimizer_run?: unknown;
  bindings?: VisualBinding[] | { slots?: VisualBinding[] };
  events?: OptimizerEvent[];
  run?: OptimizerRun;
  loadError?: string;
};

export function Shell(props: ShellProps) {
  return (
    <OptimizerFamilyShell
      {...props}
      templateId="optimizer.gepa.evaluations.v1"
      kicker="GEPA · evaluations"
      testId="visual-optimizer-gepa-evaluations"
      showTimeline={false}
    >
      {({ projected }) => {
        const gepa = projected.gepa;
        if (!gepa) return null;
        const candidateById = new Map(gepa.candidates.map((candidate) => [String(candidate.id), candidate]));
        const rows: RolloutRow[] = [];
        const groups: RolloutGroup[] = [];
        const seen = new Set<string>();
        for (const evaluation of gepa.evaluations) {
          const stage = evaluation.stage ?? "unknown";
          const candidateId = evaluation.candidateId ?? "unknown";
          const groupKey = `${candidateId}::${stage}`;
          rows.push({
            id: evaluation.ref.id,
            groupKey,
            sequence: evaluation.sequence,
            exampleId: evaluation.exampleId,
            stage,
            reward: evaluation.reward,
            costUsd: evaluation.costUsd,
            usage: evaluation.usage,
            streamId: evaluation.ref.attributes?.stream_id,
            rewardUrl: evaluation.ref.attributes?.reward_url,
            occurredAt: evaluation.occurredAt
          });
          if (!seen.has(groupKey)) {
            seen.add(groupKey);
            const candidate = candidateById.get(candidateId);
            groups.push({
              key: groupKey,
              title: candidate ? candidateName(candidate) : candidateId,
              subtitle: stageTitle(stage)
            });
          }
        }
        return (
          <div data-testid="gepa-child-evals">
            <RolloutBrowser
              groups={groups}
              rows={rows}
              emptyText="No child eval refs at this cursor."
            />
          </div>
        );
      }}
    </OptimizerFamilyShell>
  );
}

export default Shell;
