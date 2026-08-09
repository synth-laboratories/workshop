import { useEffect, useMemo, useState } from "react";
import { VisualChrome, MetricStrip } from "../../chrome/VisualChrome.tsx";
import { TimelineScrubber } from "../../chrome/TimelineScrubber.tsx";
import type { RolloutStep, VisualBinding } from "../../runtime/types.ts";
import rolloutFixture from "../../fixtures/rollout_steps.json";

type Trajectory = {
  id?: string;
  model?: string;
  total_reward?: number;
  steps: RolloutStep[];
};

export type ShellProps = {
  title?: string;
  lede?: string;
  trajectory?: Trajectory;
  data?: Trajectory;
  bindings?: VisualBinding[];
};

function asTrajectory(raw: unknown): Trajectory {
  if (raw && typeof raw === "object" && Array.isArray((raw as Trajectory).steps)) {
    return raw as Trajectory;
  }
  return rolloutFixture as Trajectory;
}

function RewardSparkline({ steps }: { steps: RolloutStep[] }) {
  const cum: number[] = [];
  let acc = 0;
  for (const s of steps) {
    acc += s.reward ?? 0;
    cum.push(acc);
  }
  const max = Math.max(...cum, 0.01);
  const w = 280;
  const h = 48;
  const pts = cum
    .map((v, i) => {
      const x = (i / Math.max(cum.length - 1, 1)) * (w - 8) + 4;
      const y = h - 4 - (v / max) * (h - 10);
      return `${x},${y}`;
    })
    .join(" ");

  return (
    <svg
      viewBox={`0 0 ${w} ${h}`}
      width="100%"
      role="img"
      aria-label={`Cumulative reward sparkline ending at ${acc.toFixed(2)}`}
    >
      <polyline fill="none" stroke="#f05f22" strokeWidth="2" points={pts} />
    </svg>
  );
}

export function Shell(props: ShellProps) {
  const traj = useMemo(
    () => asTrajectory(props.data ?? props.trajectory ?? rolloutFixture),
    [props.data, props.trajectory]
  );
  const steps = traj.steps;
  const [index, setIndex] = useState(0);
  const [playing, setPlaying] = useState(false);
  const step = steps[Math.min(index, Math.max(steps.length - 1, 0))];

  const totalReward =
    traj.total_reward ??
    steps.reduce((sum, s) => sum + (s.reward ?? 0), 0);

  useEffect(() => {
    if (!playing || steps.length < 2) return;
    const id = window.setInterval(() => {
      setIndex((i) => (i + 1) % steps.length);
    }, 500);
    return () => window.clearInterval(id);
  }, [playing, steps.length]);

  return (
    <VisualChrome
      kicker="PostTrain · trajectory"
      title={props.title ?? `Rollout ${traj.id ?? ""}`.trim()}
      lede={props.lede}
      testId="visual-posttrain-rollout-viewer"
      footer="posttrain.rollout_viewer.v1"
    >
      <MetricStrip
        metrics={[
          { label: "Steps", value: String(steps.length) },
          { label: "Total reward", value: totalReward.toFixed(2) },
          { label: "Model", value: traj.model ?? "—" }
        ]}
      />

      <section className="sv-section">
        <div className="sv-section-head">
          <h3>Cumulative reward</h3>
          <span>step →</span>
        </div>
        <RewardSparkline steps={steps} />
      </section>

      <TimelineScrubber
        index={index}
        total={steps.length}
        playing={playing}
        onTogglePlay={() => setPlaying((p) => !p)}
        onChange={(i) => {
          setPlaying(false);
          setIndex(i);
        }}
        label="Trajectory scrubber"
      />

      <section className="sv-section">
        <div className="sv-section-head">
          <h3>Current step</h3>
          <span className="sv-mono">
            #{step?.index ?? index} · reward {(step?.reward ?? 0).toFixed(2)}
          </span>
        </div>
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "120px 1fr",
            gap: 8,
            fontSize: 12
          }}
        >
          <span style={{ color: "var(--sv-text-muted)" }}>Action</span>
          <strong className="sv-mono">{step?.action ?? "—"}</strong>
          <span style={{ color: "var(--sv-text-muted)" }}>Observation</span>
          <p style={{ margin: 0 }} aria-live="polite">
            {step?.observation_text ?? "—"}
          </p>
        </div>
      </section>

      <section className="sv-section" aria-label="Step list">
        <div className="sv-section-head">
          <h3>Steps</h3>
        </div>
        <div
          role="list"
          style={{ maxHeight: 180, overflow: "auto", border: "1px solid var(--sv-border)", borderRadius: 8 }}
        >
          {steps.map((s, i) => (
            <button
              key={s.index}
              type="button"
              role="listitem"
              className="sv-btn"
              aria-current={i === index ? "step" : undefined}
              onClick={() => {
                setPlaying(false);
                setIndex(i);
              }}
              style={{
                display: "flex",
                width: "100%",
                justifyContent: "space-between",
                borderRadius: 0,
                border: "none",
                borderBottom: "1px solid var(--sv-border)",
                background: i === index ? "var(--sv-accent-soft)" : "transparent",
                textAlign: "left"
              }}
            >
              <span className="sv-mono">
                {s.index}. {s.action ?? "—"}
              </span>
              <span className="sv-mono" style={{ color: "var(--sv-accent)" }}>
                {(s.reward ?? 0).toFixed(2)}
              </span>
            </button>
          ))}
        </div>
      </section>
    </VisualChrome>
  );
}

export default Shell;
