import { formatChildEvalCost, formatChildEvalReward } from "../../_shared/optimizer.run.v1/components/projectEvents.ts";
import { OptimizerFamilyShell } from "../../_shared/optimizer.run.v1/components/FamilyShell.tsx";
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
      templateId="optimizer.sft.rollouts.v1"
      kicker="SFT · rollouts"
      testId="visual-optimizer-sft-rollouts"
    >
      {({ projected }) => {
        const campaigns = projected.sft?.campaigns ?? [];
        return (
          <section className="sv-section" aria-label="Campaign child rollouts" data-testid="sft-campaign-rollouts">
            <div className="sv-section-head">
              <h3>Campaign refs</h3>
              <span className="sv-mono">{campaigns.length}</span>
            </div>
            <p className="sv-lede">Each child is a Containers rollout ref. Lanes are not sparse parallel metric arrays.</p>
            {campaigns.map((campaign) => (
              <article key={campaign.id} style={{ marginTop: 10, border: "1px solid var(--sv-border)", borderRadius: 8, padding: 10 }}>
                <div className="sv-section-head">
                  <h3 className="sv-mono">{campaign.id}</h3>
                  <span className="sv-mono">
                    {campaign.checkpointId ?? "—"} · {campaign.splitRole ?? "—"} · {campaign.status ?? "—"}
                  </span>
                </div>
                <table className="sv-table">
                  <thead>
                    <tr>
                      <th scope="col">Rollout</th>
                      <th scope="col">Stream</th>
                      <th scope="col">Reward URL</th>
                      <th scope="col">Reward</th>
                      <th scope="col">Cost</th>
                    </tr>
                  </thead>
                  <tbody>
                    {campaign.children.map((ref) => (
                      <tr key={ref.id} data-testid={`sft-rollout-${ref.id}`}>
                        <td className="sv-mono">{ref.id}</td>
                        <td className="sv-mono">{ref.attributes?.stream_id ?? "—"}</td>
                        <td className="sv-mono">{ref.attributes?.reward_url ?? "—"}</td>
                        <td className="sv-mono" data-testid={`sft-rollout-reward-${ref.id}`}>
                          {formatChildEvalReward(ref)}
                        </td>
                        <td className="sv-mono" data-testid={`sft-rollout-cost-${ref.id}`}>
                          {formatChildEvalCost(ref)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </article>
            ))}
            {campaigns.length === 0 ? (
              <p style={{ color: "var(--sv-text-faint)", fontSize: 12 }}>No campaign child refs at this cursor.</p>
            ) : null}
          </section>
        );
      }}
    </OptimizerFamilyShell>
  );
}

export default Shell;
