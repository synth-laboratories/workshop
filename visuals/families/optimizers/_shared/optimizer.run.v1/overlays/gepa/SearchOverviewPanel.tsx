/**
 * GEPA experiment setup.
 *
 * Setup is context, not result. Before a producer has reported its task,
 * dataset, and container, four full cards of the word "pending" occupied more
 * area than the seed/incumbent/lift outcome they framed. So the outcome and the
 * search contract now lead, and a card whose every field is still unreported
 * collapses to a single honest row instead of six repetitions of "pending".
 */

import type { ReactNode } from "react";
import type { GepaState } from "../../components/projectEvents.ts";
import { formatDurationMs } from "./model.ts";

const PENDING = "pending";

function label(value?: string): string {
  return value ? value.replaceAll("_", " ") : PENDING;
}

function count(value?: number): string {
  return value == null ? PENDING : value.toLocaleString();
}

function limitLabel(kind: string): string {
  return ({ total_rollouts: "Rollouts", proposer_calls: "Proposer calls", cost_usd: "Cost (USD)", wall_time_seconds: "Wall time" } as Record<string, string>)[kind] ?? label(kind);
}

type SetupRow = { name: string; value: string; title?: string; reported?: boolean };
type SetupCard = { eyebrow: string; title: string; testId: string; rows: SetupRow[] };

/** Reads the same rows the sighted card shows, so the two cannot drift. */
function cardSummary(card: SetupCard): string {
  return `${card.eyebrow}: ${card.title}. ${card.rows.map((row) => `${row.name} ${row.value}`).join(". ")}.`;
}

function DetailCard({ card }: { card: SetupCard }) {
  const pendingRows = card.rows.filter((row) => row.reported === false || row.value === PENDING).length;
  const unreported = pendingRows === card.rows.length;
  return (
    <div
      role="group"
      aria-label={cardSummary(card)}
      data-testid={card.testId}
      data-unreported={unreported ? "true" : undefined}
      style={{ minWidth: 0, border: "1px solid var(--sv-border)", borderRadius: 9, padding: 11, background: "var(--sv-surface)" }}
    >
      <div aria-hidden="true" style={{ color: "var(--sv-text-faint)", fontSize: 9, letterSpacing: ".08em", textTransform: "uppercase", marginBottom: 3 }}>{card.eyebrow}</div>
      <strong aria-hidden="true" style={{ display: "block", fontSize: 12 }}>{card.title}</strong>
      {unreported ? (
        <p aria-hidden="true" style={{ margin: "6px 0 0", color: "var(--sv-text-faint)", fontSize: 10.5 }}>
          Not reported yet · {card.rows.length} fields
        </p>
      ) : (
        <dl aria-hidden="true" style={{ display: "grid", gridTemplateColumns: "minmax(78px, .7fr) minmax(0, 1.5fr)", gap: "5px 9px", margin: "8px 0 0", fontSize: 11 }}>
          {card.rows.map((row) => (
            <Detail key={row.name} name={row.name} title={row.title}>{row.value}</Detail>
          ))}
        </dl>
      )}
    </div>
  );
}

function Detail({ name, children, title }: { name: string; children: ReactNode; title?: string }) {
  return <>
    <dt style={{ color: "var(--sv-text-faint)" }}>{name}</dt>
    <dd title={title} style={{ minWidth: 0, margin: 0, overflowWrap: "anywhere" }}>{children}</dd>
  </>;
}

export function SearchOverviewPanel({ gepa }: { gepa: GepaState }) {
  const contract = gepa.contract;
  const objective = contract.objectiveSet;
  const nearest = gepa.nearestLimit;
  const dataset = contract.dataset;
  const container = contract.container;
  const seed = gepa.candidates.find((candidate) => String(candidate.source ?? "") === "seed" || candidate.parentId == null);
  const seedScore = typeof seed?.train_reward === "number" ? seed.train_reward : typeof seed?.score === "number" ? seed.score : undefined;
  const bestScore = gepa.best?.trainReward;
  const lift = seedScore != null && bestScore != null ? bestScore - seedScore : undefined;
  const proposerRunning = gepa.proposerTraces.filter((trace) => trace.status === "running").length;
  const proposerCompleted = gepa.proposerTraces.filter((trace) => trace.status === "completed").length;
  const proposerFailed = gepa.proposerTraces.filter((trace) => ["failed", "cancelled", "canceled"].includes(trace.status)).length;
  const scored = gepa.evaluations.filter((evaluation) => evaluation.reward != null).length;
  const attached = gepa.evaluations.filter((evaluation) => evaluation.reward == null).length;
  const endpoint = container?.url?.replace(/^https?:\/\//, "") ?? PENDING;
  const taskTitle = contract.task?.name ?? contract.task?.id ?? "Task pending";
  const datasetTitle = dataset?.source ?? "Dataset pending";
  const containerTitle = container?.specId ?? container?.targetId ?? "Container pending";

  const cards: SetupCard[] = [
    {
      eyebrow: "Configuration",
      title: taskTitle,
      testId: "gepa-config-card",
      rows: [
        { name: "Task ID", value: contract.task?.id ?? PENDING },
        { name: "Program", value: contract.program?.id ?? PENDING },
        { name: "Objective", value: objective?.selectionObjective ?? objective?.objectives[0]?.name ?? PENDING },
        { name: "Mutable", value: contract.program?.mutableFields.join(", ") || PENDING },
        { name: "Policy", value: gepa.models.policy ?? container?.policyConfig ?? PENDING },
        { name: "Proposer", value: gepa.models.proposer ?? PENDING }
      ]
    },
    {
      eyebrow: "Dataset",
      title: datasetTitle,
      testId: "gepa-dataset-card",
      rows: [
        { name: "Version", value: [dataset?.config, dataset?.revision].filter(Boolean).join(" · ") || PENDING },
        { name: "Catalog", value: `${count(dataset?.rowCount)} rows · ${count(dataset?.labelCount)} labels`, reported: dataset?.rowCount != null || dataset?.labelCount != null },
        { name: "Source splits", value: `train ${count(dataset?.splits?.train)} · selection ${count(dataset?.splits?.selection)} · heldout ${count(dataset?.splits?.heldout)}`, reported: dataset?.splits?.train != null || dataset?.splits?.selection != null || dataset?.splits?.heldout != null },
        { name: "Run taskset", value: `train ${count(contract.splits?.train)} · heldout ${count(contract.splits?.heldout)}`, reported: contract.splits?.train != null || contract.splits?.heldout != null },
        { name: "Search pools", value: `mini ${count(contract.splits?.minibatch)} · reflect ${count(contract.splits?.reflection)} · Pareto ${count(contract.splits?.pareto)}`, reported: contract.splits?.minibatch != null || contract.splits?.reflection != null || contract.splits?.pareto != null },
        { name: "Digest", value: dataset?.digest ? `${dataset.digest.slice(0, 18)}…` : PENDING, title: dataset?.digest }
      ]
    },
    {
      eyebrow: "Container",
      title: containerTitle,
      testId: "gepa-container-card",
      rows: [
        { name: "Binding", value: container?.verified ? "contract verified" : PENDING },
        { name: "Instance", value: container?.workshopInstance ?? PENDING },
        { name: "Endpoint", value: endpoint, title: container?.url },
        { name: "Runtime", value: label(container?.runtimeFamily) },
        { name: "Evaluator", value: container?.evaluatorId ?? PENDING },
        { name: "Reward owner", value: label(container?.rewardAuthority) },
        { name: "Credential", value: label(container?.credentialMode) }
      ]
    },
    {
      eyebrow: "Related work",
      title: `${gepa.candidates.length} candidate${gepa.candidates.length === 1 ? "" : "s"} · ${scored} scored`,
      testId: "gepa-related-work-card",
      rows: [
        { name: "Rollouts", value: `${gepa.rolloutsCompleted.toLocaleString()} completed · ${attached.toLocaleString()} attached` },
        { name: "Runtime", value: `${gepa.runtime.configuredRolloutWorkers ?? "?"} configured · ${gepa.runtime.estimatedEffectiveConcurrency?.toFixed(1) ?? "?"} effective · ${gepa.runtime.rolloutsPerMinute?.toFixed(1) ?? "?"} rollouts/min` },
        { name: "Proposer", value: `${proposerRunning} running · ${proposerCompleted} complete · ${proposerFailed} failed` },
        { name: "Failures", value: `${gepa.failedAttempts.length.toLocaleString()} exhausted attempts` },
        { name: "Current phase", value: gepa.activity.label },
        { name: "Generation", value: String(gepa.activity.generation ?? (gepa.proposerTraces.length ? Math.max(...gepa.proposerTraces.map((trace) => trace.generation)) : PENDING)) }
      ]
    }
  ];

  const setupRows = cards.flatMap((card) => card.rows);
  const setupPending = setupRows.filter((row) => row.reported === false || row.value === PENDING).length;
  const setupIncomplete = setupPending > 0;

  return (
    <section className="sv-section" aria-label="GEPA experiment setup" data-testid="gepa-search-overview" style={{ marginTop: 0 }}>
      <div className="sv-gepa-contract-outcome" style={{ display: "grid", gridTemplateColumns: "minmax(0, 1.35fr) minmax(230px, .65fr)", gap: 9 }}>
        <div style={{ minWidth: 0, border: "1px solid var(--sv-border)", borderRadius: 9, padding: 11 }}>
          <strong style={{ fontSize: 12 }}>Search contract</strong>
          <div style={{ display: "flex", flexWrap: "wrap", gap: "7px 13px", marginTop: 8, fontSize: 11 }}>
            <span><span style={{ color: "var(--sv-text-faint)" }}>Frontier</span> · {label(objective?.frontierType)}</span>
            <span><span style={{ color: "var(--sv-text-faint)" }}>Select on</span> · {objective?.selectionObjective ?? PENDING}</span>
            <span><span style={{ color: "var(--sv-text-faint)" }}>Retention</span> · {label(container?.retention)}</span>
            <span><span style={{ color: "var(--sv-text-faint)" }}>Scale leases</span> · {container?.scaleLeases ?? PENDING}</span>
          </div>
          {contract.task?.description ? <p style={{ margin: "8px 0 0", color: "var(--sv-text-muted)", fontSize: 10.5 }}>{contract.task.description}</p> : null}
        </div>
        <div style={{ border: "1px solid var(--sv-border)", borderRadius: 9, padding: 11 }} data-testid="gepa-outcome-card">
          <strong style={{ fontSize: 12 }}>Outcome so far</strong>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 6, marginTop: 9 }}>
            {[["Seed", seedScore], ["Incumbent", bestScore], ["Lift", lift]].map(([name, value]) => <div key={String(name)}><div style={{ color: "var(--sv-text-faint)", fontSize: 9.5, textTransform: "uppercase" }}>{name}</div><div className="sv-mono" style={{ marginTop: 3, fontSize: 15 }}>{typeof value === "number" ? `${name === "Lift" && value >= 0 ? "+" : ""}${value.toFixed(3)}` : "—"}</div></div>)}
          </div>
          <p style={{ margin: "9px 0 0", color: "var(--sv-text-muted)", fontSize: 10.5 }}>Heldout stays isolated from candidate selection.</p>
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

      <div className="sv-section-head" style={{ marginTop: 12 }}>
        <h3>Experiment setup</h3>
        <span className="sv-mono" data-testid="gepa-setup-completeness">
          {setupIncomplete
            ? `setup incomplete · ${setupPending}/${setupRows.length} fields pending`
            : "configuration · dataset · container · related work"}
        </span>
      </div>
      <div data-testid="gepa-experiment-context" style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(min(238px, 100%), 1fr))", gap: 9 }}>
        {cards.map((card) => <DetailCard key={card.testId} card={card} />)}
      </div>
    </section>
  );
}
