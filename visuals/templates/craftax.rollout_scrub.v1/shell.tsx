import { useEffect, useMemo, useState } from "react";
import { VisualChrome } from "../../chrome/VisualChrome.tsx";
import { TimelineScrubber } from "../../chrome/TimelineScrubber.tsx";
import type { RolloutStep, VisualBinding } from "../../runtime/types.ts";
import rolloutFixture from "../../fixtures/rollout_steps.json";

type Hud = {
  pos?: number[];
  vitals?: Record<string, number>;
  inventory?: Record<string, number>;
};

type RolloutPayload = {
  id?: string;
  model?: string;
  steps: RolloutStep[];
};

export type ShellProps = {
  title?: string;
  lede?: string;
  rollout?: RolloutPayload;
  data?: RolloutPayload;
  bindings?: VisualBinding[];
};

const VITALS: [string, string][] = [
  ["health", "#c2553f"],
  ["food", "#c99b3f"],
  ["drink", "#3d78bb"],
  ["energy", "#6f9a4d"],
  ["mana", "#8a5fd0"]
];

function asRollout(raw: unknown): RolloutPayload {
  if (raw && typeof raw === "object" && Array.isArray((raw as RolloutPayload).steps)) {
    return raw as RolloutPayload;
  }
  return rolloutFixture as RolloutPayload;
}

function FrameCanvas({ step }: { step: RolloutStep }) {
  const seed = step.index * 7;
  return (
    <div
      role="img"
      aria-label={`Environment frame at turn ${step.turn ?? step.index}`}
      style={{
        border: "1px solid var(--sv-border)",
        borderRadius: 10,
        overflow: "hidden",
        background: "#eef1f5"
      }}
    >
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(12, 1fr)",
          gap: 2,
          padding: 8,
          aspectRatio: "16 / 10"
        }}
      >
        {Array.from({ length: 96 }, (_, i) => {
          const t = (i * 7 + seed) % 5;
          const colors = ["#c8d0da", "#a8b4c2", "#8f9aab", "#6f9a4d", "#c99b3f"];
          return (
            <span
              key={i}
              style={{
                background: colors[t],
                borderRadius: 2,
                minHeight: 8
              }}
            />
          );
        })}
      </div>
    </div>
  );
}

function HudPanel({ step }: { step: RolloutStep }) {
  const hud = (step.meta?.hud ?? {}) as Hud;
  const vitals = hud.vitals ?? step.metrics ?? {};
  const inventory = Object.entries(hud.inventory ?? {}).filter(([, n]) => Number(n) > 0);
  const unlocked = step.achievements ?? [];

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <div
        style={{
          border: "1px solid var(--sv-border)",
          borderRadius: 10,
          padding: 10,
          background: "var(--sv-surface-muted)"
        }}
      >
        <div className="sv-mono" style={{ color: "var(--sv-text-muted)", marginBottom: 8 }}>
          vitals · turn {step.turn ?? step.index}
          {hud.pos?.length === 2 ? ` · ${hud.pos[0]},${hud.pos[1]}` : ""}
        </div>
        {VITALS.map(([name, color]) => {
          const value = Number(vitals[name] ?? 0);
          const pct = Math.max(0, Math.min(value / 9, 1)) * 100;
          return (
            <div
              key={name}
              style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4 }}
            >
              <span style={{ width: 48, fontSize: 11, color: "var(--sv-text-muted)" }}>{name}</span>
              <div
                role="progressbar"
                aria-label={`${name} ${value} of 9`}
                aria-valuenow={value}
                aria-valuemin={0}
                aria-valuemax={9}
                style={{
                  flex: 1,
                  height: 6,
                  background: "#e2e6ec",
                  borderRadius: 3,
                  overflow: "hidden"
                }}
              >
                <div style={{ width: `${pct}%`, height: "100%", background: color }} />
              </div>
              <span className="sv-mono" style={{ width: 16, textAlign: "right" }}>
                {value}
              </span>
            </div>
          );
        })}
        {step.action ? (
          <div className="sv-mono" style={{ marginTop: 8, color: "var(--sv-text-muted)" }}>
            action <strong style={{ color: "var(--sv-text)" }}>{step.action}</strong>
          </div>
        ) : null}
      </div>

      <div
        style={{
          border: "1px solid var(--sv-border)",
          borderRadius: 10,
          padding: 10
        }}
      >
        <div className="sv-mono" style={{ color: "var(--sv-text-muted)" }}>
          inventory
        </div>
        <div style={{ display: "flex", flexWrap: "wrap", gap: 6, marginTop: 6 }}>
          {inventory.length ? (
            inventory.map(([name, count]) => (
              <span
                key={name}
                className="sv-mono"
                style={{
                  border: "1px solid var(--sv-border)",
                  borderRadius: 6,
                  padding: "2px 6px"
                }}
              >
                {name} {count}
              </span>
            ))
          ) : (
            <span style={{ color: "var(--sv-text-faint)", fontSize: 11 }}>empty</span>
          )}
        </div>
      </div>

      <div
        style={{
          border: "1px solid var(--sv-border)",
          borderRadius: 10,
          padding: 10
        }}
      >
        <div className="sv-mono" style={{ color: "var(--sv-text-muted)" }}>
          achievements · <span style={{ color: "var(--sv-accent)" }}>{unlocked.length}</span>
        </div>
        <div style={{ display: "flex", flexWrap: "wrap", gap: 4, marginTop: 6 }}>
          {unlocked.length ? (
            unlocked.map((a) => (
              <span
                key={a}
                style={{
                  background: "var(--sv-accent-soft)",
                  color: "var(--sv-accent)",
                  borderRadius: 6,
                  padding: "2px 6px",
                  fontSize: 11
                }}
              >
                {a}
              </span>
            ))
          ) : (
            <span style={{ color: "var(--sv-text-faint)", fontSize: 11 }}>none yet</span>
          )}
        </div>
      </div>
    </div>
  );
}

export function Shell(props: ShellProps) {
  const rollout = useMemo(
    () => asRollout(props.data ?? props.rollout ?? rolloutFixture),
    [props.data, props.rollout]
  );
  const steps = rollout.steps;
  const [index, setIndex] = useState(0);
  const [playing, setPlaying] = useState(false);
  const step = steps[Math.min(index, Math.max(steps.length - 1, 0))] ?? steps[0];

  useEffect(() => {
    if (!playing || steps.length < 2) return;
    const id = window.setInterval(() => {
      setIndex((i) => (i + 1) % steps.length);
    }, 450);
    return () => window.clearInterval(id);
  }, [playing, steps.length]);

  return (
    <VisualChrome
      kicker="Environment frame · Craftax"
      title={props.title ?? `Rollout ${rollout.id ?? ""}`.trim()}
      lede={props.lede ?? (rollout.model ? `Model ${rollout.model}` : undefined)}
      testId="visual-craftax-rollout-scrub"
      footer="craftax.rollout_scrub.v1 · text projection required for a11y / CUA"
    >
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(0, 1.4fr) minmax(180px, 0.8fr)",
          gap: 14
        }}
      >
        <div>
          {step ? <FrameCanvas step={step} /> : null}
          <p
            className="sv-mono"
            style={{ marginTop: 8, color: "var(--sv-text-muted)" }}
            aria-live="polite"
          >
            {step?.observation_text ?? "No observation text"}
          </p>
          <TimelineScrubber
            index={index}
            total={steps.length}
            playing={playing}
            onTogglePlay={() => setPlaying((p) => !p)}
            onChange={(i) => {
              setPlaying(false);
              setIndex(i);
            }}
            label="Frame scrubber"
            valueText={`Turn ${step?.turn ?? index} · frame ${index + 1}/${steps.length}`}
          />
        </div>
        {step ? <HudPanel step={step} /> : null}
      </div>
    </VisualChrome>
  );
}

export default Shell;
