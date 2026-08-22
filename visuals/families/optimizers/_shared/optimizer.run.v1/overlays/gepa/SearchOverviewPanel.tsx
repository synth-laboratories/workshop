import type { GepaState } from "../../components/projectEvents.ts";
import { formatDurationMs } from "./model.ts";

function label(value?: string): string {
  return value ? value.replaceAll("_", " ") : "unavailable";
}

function limitLabel(kind: string): string {
  return ({ total_rollouts: "Rollouts", proposer_calls: "Proposer calls", cost_usd: "Cost (USD)", wall_time_seconds: "Wall time" } as Record<string, string>)[kind] ?? label(kind);
}

export function SearchOverviewPanel({ gepa }: { gepa: GepaState }) {
  const contract = gepa.contract;
  const objective = contract.objectiveSet;
  const nearest = gepa.nearestLimit;
  const seed = gepa.candidates.find((candidate) => String(candidate.source ?? "") === "seed" || candidate.parentId == null);
  const seedScore = typeof seed?.train_reward === "number" ? seed.train_reward : typeof seed?.score === "number" ? seed.score : undefined;
  const bestScore = gepa.best?.trainReward;
  const lift = seedScore != null && bestScore != null ? bestScore - seedScore : undefined;
  return (
    <section className="sv-section" aria-label="GEPA run contract and budget" data-testid="gepa-search-overview" style={{ marginTop: 0 }}>
      <div className="sv-section-head"><h3>Search contract</h3><span className="sv-mono">what can change · how success is judged · when search stops</span></div>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(min(230px, 100%), 1fr))", gap: 9 }}>
        <div style={{ border: "1px solid var(--sv-border)", borderRadius: 9, padding: 11 }}>
          <strong style={{ fontSize: 12 }}>Task & objective</strong>
          <dl style={{ display: "grid", gridTemplateColumns: "90px 1fr", gap: "5px 9px", margin: "8px 0 0", fontSize: 11 }}>
            <dt style={{ color: "var(--sv-text-faint)" }}>Task</dt><dd style={{ margin: 0 }}>{contract.task?.name ?? contract.task?.id ?? "unavailable"}</dd>
            <dt style={{ color: "var(--sv-text-faint)" }}>Select on</dt><dd style={{ margin: 0 }}>{objective?.selectionObjective ?? objective?.objectives[0]?.name ?? "unavailable"}</dd>
            <dt style={{ color: "var(--sv-text-faint)" }}>Frontier</dt><dd style={{ margin: 0 }}>{label(objective?.frontierType)}</dd>
            <dt style={{ color: "var(--sv-text-faint)" }}>Reward owner</dt><dd style={{ margin: 0 }}>{label(contract.container?.rewardAuthority)}</dd>
          </dl>
        </div>
        <div style={{ border: "1px solid var(--sv-border)", borderRadius: 9, padding: 11 }}>
          <strong style={{ fontSize: 12 }}>Search space & evidence</strong>
          <dl style={{ display: "grid", gridTemplateColumns: "90px 1fr", gap: "5px 9px", margin: "8px 0 0", fontSize: 11 }}>
            <dt style={{ color: "var(--sv-text-faint)" }}>Mutable</dt><dd style={{ margin: 0 }}>{contract.program?.mutableFields.join(", ") || "unavailable"}</dd>
            <dt style={{ color: "var(--sv-text-faint)" }}>Policy</dt><dd style={{ margin: 0 }}>{gepa.models.policy ?? contract.container?.policyConfig ?? "unavailable"}</dd>
            <dt style={{ color: "var(--sv-text-faint)" }}>Proposer</dt><dd style={{ margin: 0 }}>{gepa.models.proposer ?? "unavailable"}</dd>
            <dt style={{ color: "var(--sv-text-faint)" }}>Splits</dt><dd style={{ margin: 0 }}>mini {contract.splits?.minibatch ?? "—"} · reflect {contract.splits?.reflection ?? "—"} · Pareto {contract.splits?.pareto ?? "—"} · heldout {contract.splits?.heldout ?? "—"}</dd>
          </dl>
        </div>
        <div style={{ border: "1px solid var(--sv-border)", borderRadius: 9, padding: 11 }}>
          <strong style={{ fontSize: 12 }}>Outcome so far</strong>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 6, marginTop: 9 }}>
            {[["Seed", seedScore], ["Incumbent", bestScore], ["Lift", lift]].map(([name, value]) => <div key={String(name)}><div style={{ color: "var(--sv-text-faint)", fontSize: 9.5, textTransform: "uppercase" }}>{name}</div><div className="sv-mono" style={{ marginTop: 3, fontSize: 15 }}>{typeof value === "number" ? `${name === "Lift" && value >= 0 ? "+" : ""}${value.toFixed(3)}` : "—"}</div></div>)}
          </div>
          <p style={{ margin: "9px 0 0", color: "var(--sv-text-muted)", fontSize: 10.5 }}>Heldout remains separate and is never substituted with train, minibatch, or missing-as-zero evidence.</p>
        </div>
      </div>
      {gepa.limits.length ? <div style={{ display: "grid", gap: 7, marginTop: 10 }}>
        {gepa.limits.map((limit) => {
          const max = limit.max ?? 0;
          const spent = limit.spent ?? 0;
          const reserved = limit.reserved ?? 0;
          const usedPct = max > 0 ? Math.min(100, spent / max * 100) : 0;
          const reservedPct = max > 0 ? Math.min(100 - usedPct, reserved / max * 100) : 0;
          const isNearest = nearest?.kind === limit.kind;
          return <div key={limit.kind} className="sv-gepa-limit-row" style={{ display: "grid", gap: 10, alignItems: "center", fontSize: 10.5 }}>
            <span className="sv-gepa-limit-label"><strong>{limitLabel(limit.kind)}</strong>{isNearest ? <span className="sv-chip" data-tone="warn" style={{ marginLeft: 5 }}>nearest</span> : null}</span>
            <span className="sv-gepa-limit-meter" style={{ height: 11, position: "relative", borderRadius: 99, background: "var(--sv-surface-muted)", overflow: "hidden" }}><span style={{ position: "absolute", inset: 0, width: `${usedPct}%`, background: "var(--sv-accent)" }} /><span style={{ position: "absolute", top: 0, bottom: 0, left: `${usedPct}%`, width: `${reservedPct}%`, background: "var(--sv-border-strong)" }} /></span>
            <span className="sv-gepa-limit-receipt sv-mono" style={{ color: "var(--sv-text-muted)" }}>{limit.spent ?? "—"} spent + {limit.reserved ?? 0} reserved / {limit.max ?? "—"}{limit.forecast?.secondsToLimit != null ? ` · ~${formatDurationMs(limit.forecast.secondsToLimit * 1000)} left` : ""}</span>
          </div>;
        })}
      </div> : null}
    </section>
  );
}
