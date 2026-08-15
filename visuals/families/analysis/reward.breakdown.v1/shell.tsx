import { VisualChrome, MetricStrip } from "../../../chrome/VisualChrome.tsx";
import type { VisualBinding } from "../../../runtime/types.ts";
import rewardFixture from "../../../fixtures/reward_breakdown.json";

type RewardComponent = {
  name: string;
  value: number;
  type: "sparse" | "dense" | "penalty" | "bonus" | string;
};

type RewardPayload = {
  total: number;
  components: RewardComponent[];
};

export type ShellProps = {
  title?: string;
  lede?: string;
  reward?: RewardPayload;
  data?: RewardPayload;
  bindings?: VisualBinding[];
};

const TYPE_COLOR: Record<string, string> = {
  sparse: "#f05f22",
  dense: "#3d78bb",
  penalty: "#c2553f",
  bonus: "#6f9a4d"
};

function asReward(raw: unknown): RewardPayload {
  if (raw && typeof raw === "object" && Array.isArray((raw as RewardPayload).components)) {
    return raw as RewardPayload;
  }
  return rewardFixture as RewardPayload;
}

export function Shell(props: ShellProps) {
  const reward = asReward(props.data ?? props.reward ?? rewardFixture);
  const maxAbs = Math.max(...reward.components.map((c) => Math.abs(c.value)), 0.01);

  return (
    <VisualChrome
      kicker="Reward · typed components"
      title={props.title ?? "Reward breakdown"}
      lede={props.lede}
      testId="visual-reward-breakdown"
      footer="reward.breakdown.v1"
    >
      <MetricStrip
        metrics={[
          { label: "Total", value: reward.total.toFixed(2) },
          { label: "Components", value: String(reward.components.length) }
        ]}
      />

      <section className="sv-section" aria-label="Reward component chart">
        <div className="sv-section-head">
          <h3>Components</h3>
          <span>signed · typed</span>
        </div>
        <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
          {reward.components.map((c) => {
            const pct = (Math.abs(c.value) / maxAbs) * 100;
            const color = TYPE_COLOR[c.type] ?? "#5c6573";
            const positive = c.value >= 0;
            return (
              <li key={c.name} style={{ marginBottom: 10 }}>
                <div
                  style={{
                    display: "flex",
                    justifyContent: "space-between",
                    marginBottom: 4,
                    fontSize: 12
                  }}
                >
                  <span>
                    <strong>{c.name}</strong>{" "}
                    <span className="sv-mono" style={{ color: "var(--sv-text-faint)" }}>
                      {c.type}
                    </span>
                  </span>
                  <span className="sv-mono" style={{ color: positive ? color : "#c2553f" }}>
                    {positive ? "+" : ""}
                    {c.value.toFixed(2)}
                  </span>
                </div>
                <div
                  role="meter"
                  aria-label={`${c.name} ${c.value}`}
                  aria-valuenow={c.value}
                  aria-valuemin={-maxAbs}
                  aria-valuemax={maxAbs}
                  style={{
                    height: 8,
                    background: "#eef0f3",
                    borderRadius: 4,
                    overflow: "hidden",
                    display: "flex",
                    justifyContent: positive ? "flex-start" : "flex-end"
                  }}
                >
                  <div
                    style={{
                      width: `${pct}%`,
                      height: "100%",
                      background: color,
                      opacity: positive ? 1 : 0.85
                    }}
                  />
                </div>
              </li>
            );
          })}
        </ul>
      </section>
    </VisualChrome>
  );
}

export default Shell;
