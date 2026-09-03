/**
 * Trace workstation internals, shared by the family-agnostic
 * `trace.workbench.v1` template and its Craftax specialization
 * `craftax.trace_workbench.v1`. Branding (labels, test ids, and whether the
 * family is frame-centric) is the only thing the two shells differ on.
 *
 * The drill-down this replaces showed one textual observation per finished
 * seed. Everything the container had already recorded — the native PNGs, the
 * policy's own messages, its reasoning, what it asked the environment to do and
 * what the environment actually did — arrived nowhere, and the pane only moved
 * when a whole seed finished.
 *
 * Layout follows the published Craftax viewer's standard: the world is
 * dominant on the left, the complete trajectory is a bounded rail on the right,
 * and the per-call detail answers four questions in order — what did the policy
 * see, what did it decide, what did the environment apply, what changed.
 *
 * Three behaviours are specific to this being *live*:
 *
 * - It follows the newest call and frame by default, and stops the moment the
 *   reviewer scrubs backwards. Chasing playback under someone reading a call is
 *   the fastest way to make a live viewer unusable.
 * - New data appends. Selection, scroll position and open disclosures survive
 *   every update, because the selection is held by index and identity here
 *   rather than being recomputed from the projection.
 * - A policy call that is still open is *shown* as still open. Hiding it would
 *   make the rail jump when it lands.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { VisualChrome } from "../../../chrome/VisualChrome.tsx";
import "./traceWorkbench.css";
import {
  craftaxTrialsFromRun,
	craftaxTraceFromSealedTrace,
	reconcileCraftaxTrace,
  type EvalTraceView,
  type TraceFrame,
  type TraceStep,
  type TrialView
} from "../../../runtime/craftaxTraceView.ts";
import { NO_MEDIA, type LoadedMedia, type MediaClient } from "../../../runtime/mediaClient.ts";
import {
  evalAggregateV1,
  evalAggregateWorkFacts,
  evalTerminalFacts,
  type EvalAggregateV1
} from "../../../runtime/evalAggregate.ts";
import {
  readReportedFacts,
  summarizeAchievementReportedFacts,
  summarizeNumericReportedFact,
  type ReportedFactSummary
} from "../../../runtime/reportedFacts.ts";

type Any = Record<string, any>;

export type TraceWorkbenchBranding = {
  /** Family label used in the kicker and frame wording, e.g. "Craftax". */
  label: string;
  defaultTitle: string;
  testId: string;
  aggregatesTestId: string;
  frameTestId: string;
  /**
   * Frame-centric families (Craftax) keep the native-frame-missing copy: a
   * missing frame there is a defect worth naming. Families whose producers
   * never emit frames replace an entirely frame-free replay with the
   * stream-events-only note instead of demanding frames that cannot exist.
   */
  frameCentric: boolean;
};

export type TraceWorkbenchProps = {
  title?: string;
  lede?: string;
  run?: Any;
  runViewV2?: Any;
  events?: Any[];
  enrichmentEvents?: Any[];
  data?: Any;
  media?: MediaClient;
  loadError?: string;
  visualId?: string | null;
  revision?: number | null;
	runLifecycle?: {
		usage: {
			calls?: number;
			costUsd?: number;
			costCapUsd?: number;
			costSource?: string;
			promptTokens?: number;
			completionTokens?: number;
		};
		rollouts: Array<{
			lane: string;
			seed?: number;
			status: string;
			reward?: number;
			tokens?: number;
			achievements?: string[];
		}>;
	};
	sealedTraceProjections?: Array<{
		trialId: string;
		rolloutId: string | null;
		digest: string;
		projection: Any;
	}>;
};

const MISSING = "—";

const mono = { fontFamily: "var(--sv-mono)" } as const;

function reward(value: number | null): string {
  if (value === null) return MISSING;
  return `${value >= 0 ? "+" : ""}${value.toFixed(2)}`;
}

function tokens(value: number | null): string {
  if (value === null) return MISSING;
  return value >= 1000 ? `${(value / 1000).toFixed(1)}k` : String(value);
}

type AggregateFilter =
  | { kind: "achievement"; name: string }
  | { kind: "reward"; low: number; high: number; inclusiveHigh: boolean }
  | null;

const finite = (value: unknown): number | null => {
  const parsed = typeof value === "string" ? Number(value) : value;
  return typeof parsed === "number" && Number.isFinite(parsed) ? parsed : null;
};

const totalTokens = (trial: TrialView): number | null => {
  const input = trial.view.run.usage.input_tokens;
  const output = trial.view.run.usage.output_tokens;
  return input === null && output === null ? null : (input ?? 0) + (output ?? 0);
};

const stepCount = (trial: TrialView): number | null => {
  // Legacy fallback reads an explicitly recorded count only. Frame indices,
  // applied-action turns, and trace length are not step facts.
  return finite(trial.record?.steps ?? trial.record?.env_steps ?? trial.record?.raw?.steps);
};

const reportedFactRecord = (trial: TrialView): unknown => trial.reportedFacts !== undefined
  ? { reportedFacts: trial.reportedFacts }
  : trial.record;

function FactMetadata({ summary }: { summary: ReportedFactSummary<unknown> }) {
  if (!summary.authoritative) return null;
  return (
    <div
      data-reported-fact-metadata=""
      style={{ display: "grid", gap: 1, color: "var(--sv-text-faint)", fontSize: "var(--sv-fs-micro)" }}
    >
      <span>source: {summary.sources.length ? summary.sources.join(", ") : "not_reported"}</span>
      {/* Only when there is one. "unavailable reason: none" is the same
          non-statement as "errors: none", and it sat under every healthy
          rollout. contractErrors beside it was already conditional. */}
      {summary.unavailableReasons.length ? (
        <span>unavailable reason: {summary.unavailableReasons.join(", ")}</span>
      ) : null}
      {summary.contractErrors.length ? <span>contract: {summary.contractErrors.join("; ")}</span> : null}
    </div>
  );
}

function RunAggregateHeader({
  run,
  runLifecycle,
  aggregate,
  trials,
  filter,
  onFilter,
  testId
}: {
  run: Any | null;
  runLifecycle?: TraceWorkbenchProps["runLifecycle"];
  aggregate: EvalAggregateV1 | null;
  trials: TrialView[];
  filter: AggregateFilter;
  onFilter: (filter: AggregateFilter) => void;
  testId: string;
}) {
  const terminalTrials = trials.filter((row) => row.state === "done" || row.state === "failed");
  const aggregateWork = aggregate ? evalAggregateWorkFacts(aggregate) : null;
  const rolloutCount = aggregateWork?.rolloutCount ?? trials.length;
  const running = aggregateWork?.running ?? trials.filter((row) => row.state === "running").length;
  const queued = aggregateWork?.queued ?? trials.filter((row) => row.state === "queued").length;
  const failed = aggregateWork?.failed ?? trials.filter((row) => row.state === "failed").length;
  const terminalCount = aggregateWork?.terminalCount ?? terminalTrials.length;
  const startedTrials = aggregateWork?.started ?? trials.filter((row) => row.state !== "queued").length;
  const summary = (run?.summary ?? {}) as Any;
  const bounds = (summary.bounds ?? {}) as Any;
  const started = Date.parse(String(run?.startedAt ?? run?.started_at ?? summary.startedAt ?? ""));
  const ended = Date.parse(String(run?.finishedAt ?? run?.finished_at ?? ""));
  const elapsedSeconds = Number.isFinite(started)
    ? Math.max(0, ((Number.isFinite(ended) ? ended : Date.now()) - started) / 1000)
    : null;

  const factRecords = trials.map(reportedFactRecord);
  const callUsage = summarizeNumericReportedFact(factRecords, "calls", trials.map((row) => row.view.run.usage.calls));
  const stepUsage = summarizeNumericReportedFact(factRecords, "steps", trials.map(stepCount));
  const tokenUsage = summarizeNumericReportedFact(factRecords, "tokens", trials.map(totalTokens));
  const costUsage = summarizeNumericReportedFact(factRecords, "costUsd", trials.map((row) => row.view.run.cost_usd));
  const frameUsage = summarizeNumericReportedFact(factRecords, "frames", trials.map((row) => row.view.coverage.framesRetained));
  const terminalFacts = evalTerminalFacts(runLifecycle?.rollouts ?? []);
  const providerCalls = finite(runLifecycle?.usage.calls);
  const providerPromptTokens = finite(runLifecycle?.usage.promptTokens);
  const providerCompletionTokens = finite(runLifecycle?.usage.completionTokens);
  const providerTokens = providerPromptTokens === null || providerCompletionTokens === null
    ? null
    : providerPromptTokens + providerCompletionTokens;
  const providerCost = finite(runLifecycle?.usage.costUsd);
  const maxRollouts = finite(bounds.maximumRollouts) ?? (rolloutCount || null);
  const callsPerRollout = finite(bounds.maximumModelCallsPerRollout);
  const stepsPerRollout = finite(bounds.maximumStepsPerRollout);
  const callLimit = callsPerRollout === null || maxRollouts === null ? null : callsPerRollout * maxRollouts;
  const stepLimit = stepsPerRollout === null || maxRollouts === null ? null : stepsPerRollout * maxRollouts;
  const tokenLimit = finite(bounds.maximumTokens);
  const costLimit = finite(bounds.hardTotalCostUsd ?? summary.costCeilingUsd);
  // Once V2 supplies the revision-addressed aggregate, raw rows remain drill-
  // down evidence only. Recomputing counts/reward here would create a second
  // aggregate with different validity and terminal rules.
  const rewards = terminalFacts.scoredRollouts > 0
    ? (runLifecycle?.rollouts ?? []).flatMap((row) => row.reward == null ? [] : [row.reward]).sort((a, b) => a - b)
    : aggregate
      ? []
      : terminalTrials.map((row) => row.reward).filter((value): value is number => value !== null).sort((a, b) => a - b);
  const mean = terminalFacts.rewardMean ?? (aggregate ? finite(aggregate.meanReward) : rewards.length ? rewards.reduce((sum, value) => sum + value, 0) / rewards.length : null);
  const scoredTrials = terminalFacts.scoredRollouts || (aggregate ? aggregate.scoredTrials : rewards.length);
  const median = terminalFacts.rewardMedian ?? (rewards.length
    ? rewards.length % 2
      ? rewards[(rewards.length - 1) / 2]
      : (rewards[rewards.length / 2 - 1] + rewards[rewards.length / 2]) / 2
    : null);
  const rewardMin = terminalFacts.rewardMin ?? (rewards.length ? rewards[0] : null);
  const rewardMax = terminalFacts.rewardMax ?? (rewards.length ? rewards[rewards.length - 1] : null);
  const bucketCount = Math.min(5, Math.max(1, rewards.length));
  const span = rewardMin !== null && rewardMax !== null ? rewardMax - rewardMin : 0;
  const buckets = rewards.length ? Array.from({ length: bucketCount }, (_, index) => {
    const low = rewardMin === null ? 0 : rewardMin + (span * index) / bucketCount;
    const high = rewardMax === null ? 0 : index === bucketCount - 1 ? rewardMax : rewardMin + (span * (index + 1)) / bucketCount;
    const inclusiveHigh = index === bucketCount - 1;
    const count = rewards.filter((value) => value >= low && (inclusiveHigh ? value <= high : value < high)).length;
    return { low, high, inclusiveHigh, count };
  }) : [];

  const achievementFacts = summarizeAchievementReportedFacts(
    factRecords,
    trials.map((trial) => trial.view.achievements)
  );
  const achievementEvents = achievementFacts.authoritative ? [] : trials.flatMap((trial) =>
    trial.view.events
      .filter((event) => event.kind === "achievement_unlocked")
      .map((event) => ({
        trial,
        name: String(event.payload.achievement ?? event.payload.name ?? ""),
        sequence: event.sequence
      }))
      .filter((event) => event.name)
  );
  const terminalAchievementRows = (runLifecycle?.rollouts ?? []).filter((rollout) => Array.isArray(rollout.achievements));
  const terminalAchievementNames = Object.keys(terminalFacts.achievementOccurrences);
  const names = terminalAchievementRows.length > 0 ? terminalAchievementNames : achievementFacts.value ?? [];
  const achievements = names.map((name) => {
    const terminalRows = terminalAchievementRows.filter((rollout) => rollout.achievements?.includes(name));
    const seedRows = trials.filter((trial, index) => (
      achievementFacts.authoritative
        ? achievementFacts.byRecord[index]?.includes(name) === true
        : trial.view.achievements.includes(name)
    ));
    const events = achievementEvents.filter((event) => event.name === name);
    const best = seedRows.filter((row) => row.reward !== null).sort((a, b) => (b.reward ?? -Infinity) - (a.reward ?? -Infinity))[0];
    const first = events.sort((a, b) => a.sequence - b.sequence)[0];
    return {
      name,
      seeds: terminalAchievementRows.length > 0 ? terminalRows.length : seedRows.length,
      occurrences: terminalAchievementRows.length > 0
        ? terminalFacts.achievementOccurrences[name] ?? 0
        : achievementFacts.authoritative ? null : events.length || seedRows.length,
      firstSeed: terminalAchievementRows.length > 0
        ? terminalRows[0]?.seed ?? null
        : achievementFacts.authoritative ? null : first?.trial.seed ?? null,
      bestSeed: terminalAchievementRows.length > 0
        ? terminalRows.filter((row) => row.reward != null).sort((left, right) => (right.reward ?? -Infinity) - (left.reward ?? -Infinity))[0]?.seed ?? null
        : achievementFacts.authoritative ? null : best?.seed ?? null
    };
  }).sort((a, b) => b.seeds - a.seeds || a.name.localeCompare(b.name));

  const formatDuration = (seconds: number | null) => seconds === null
    ? "unavailable"
    : `${Math.floor(seconds / 60)}m ${Math.floor(seconds % 60)}s`;
  const exactCount = (value: number) => value.toLocaleString("en-US", { maximumFractionDigits: 0 });
  const exactUsd = (value: number) => `$${value.toFixed(6).replace(/0+$/, "").replace(/\.$/, "")}`;
  const providerSummary = (value: number | null): ReportedFactSummary<number> => ({
    authoritative: true,
    value,
    present: value === null ? 0 : 1,
    total: 1,
    sources: ["workshop.secrets_proxy"],
    unavailableReasons: value === null ? ["not_reported"] : [],
    contractErrors: []
  });
  const usageCard = (
    label: string,
    usage: ReportedFactSummary<number>,
    limit: number | null,
    formatter = tokens,
    options?: { valueSuffix?: string; coverage?: string; source?: string }
  ) => {
    const ratio = usage.value !== null && limit !== null && limit > 0 ? usage.value / limit : null;
    const tone = ratio !== null && ratio >= .95 ? "var(--sv-bad-fg)" : ratio !== null && ratio >= .8 ? "var(--sv-warn-fg)" : "var(--sv-text)";
    return (
      <div style={{ minWidth: 0 }}>
        <div style={{ color: "var(--sv-text-faint)", fontSize: "var(--sv-fs-micro)", textTransform: "uppercase" }}>{label}</div>
        <strong style={{ ...mono, color: tone, fontSize: "var(--sv-fs-meta)" }}>
          {usage.value === null ? "unavailable" : `${formatter(usage.value)}${options?.valueSuffix ?? ""}`} / {limit === null ? "no limit" : formatter(limit)}
        </strong>
        <div style={{ color: "var(--sv-text-faint)", fontSize: "var(--sv-fs-micro)" }}>
          {options?.coverage ?? `${usage.present}/${usage.total || rolloutCount} seeds reported`}
        </div>
        {options?.source ? <div style={{ color: "var(--sv-text-faint)", fontSize: "var(--sv-fs-micro)" }}>source: {options.source}</div> : <FactMetadata summary={usage} />}
        <div style={{ height: 3, marginTop: 4, borderRadius: 3, overflow: "hidden", background: "var(--sv-surface-muted)" }}>
          <span style={{ display: "block", height: "100%", width: `${Math.min(100, (ratio ?? 0) * 100)}%`, background: tone }} />
        </div>
      </div>
    );
  };

  return (
    <section
      className="trace-workbench-aggregate"
      data-testid={testId}
      data-aggregate-schema={aggregate?.schemaVersion}
      data-projection-revision={aggregate?.projectionRevision}
      style={{
        position: "sticky",
        top: 0,
        zIndex: 4,
        marginBottom: "var(--sv-sp-3)",
        padding: "var(--sv-sp-3)",
        border: "1px solid var(--sv-border)",
        borderRadius: "var(--sv-radius-lg)",
        background: "color-mix(in srgb, var(--sv-surface) 96%, transparent)",
        boxShadow: "0 6px 18px rgba(0,0,0,.08)"
      }}
    >
      <div style={{ display: "flex", justifyContent: "space-between", gap: "var(--sv-sp-3)", flexWrap: "wrap", marginBottom: "var(--sv-sp-3)" }}>
        <strong style={{ ...mono, fontSize: "var(--sv-fs-meta)" }}>
          {terminalCount}/{rolloutCount} terminal · {running} running · {queued} queued · {failed} failed
        </strong>
        <span style={{ color: "var(--sv-text-faint)", fontSize: "var(--sv-fs-micro)" }}>
          {formatDuration(elapsedSeconds)} · {finite(summary.concurrency) === null ? "concurrency unavailable" : `${summary.concurrency} parallel`}
        </span>
      </div>
      <div className="trace-workbench-priority" aria-label="Run outcome" style={{ display: "grid", gridTemplateColumns: "repeat(3,minmax(0,1fr))", gap: "var(--sv-sp-2)" }}>
        <div><span>Progress</span><strong>{terminalCount}/{rolloutCount}</strong><small>{failed ? `${failed} failed` : "complete"}</small></div>
        <div><span>Mean reward</span><strong>{reward(mean)}</strong><small>{scoredTrials}/{terminalCount} scored</small></div>
        <div><span>Evidence</span><strong>{aggregate ? `${aggregate.evaluatorEvidence} + ${aggregate.traceCount}` : `${terminalCount} rollouts`}</strong><small>{aggregate ? "grader + traces" : "retained records"}</small></div>
      </div>
      <details className="trace-workbench-run-details" style={{ marginTop: "var(--sv-sp-2)" }}>
        <summary>Run details <span>{providerCalls === null ? "usage not reconciled" : `${exactCount(providerCalls)} billed calls`}{providerCost === null ? "" : ` · ${exactUsd(providerCost)}`}</span></summary>
        <div className="trace-workbench-usage" style={{ display: "grid", gridTemplateColumns: "var(--tw-usage-columns, repeat(auto-fit,minmax(130px,1fr)))", gap: "var(--sv-sp-3)" }}>
          {usageCard("Rollouts", { authoritative: false, value: startedTrials, present: rolloutCount, total: rolloutCount, sources: [], unavailableReasons: [], contractErrors: [] }, maxRollouts, (value) => String(value))}
          {providerCalls === null
            ? usageCard("Model calls", callUsage, callLimit)
            : usageCard("Provider calls", providerSummary(providerCalls), callLimit, exactCount, { valueSuffix: " billed", coverage: "run-level receipt", source: "Workshop proxy" })}
          {usageCard("Environment steps", stepUsage, stepLimit)}
          {usageCard("Runtime tokens", terminalFacts.runtimeTokens === null ? tokenUsage : { ...tokenUsage, value: terminalFacts.runtimeTokens }, tokenLimit, exactCount, { coverage: `${terminalFacts.reportedTokenRollouts || tokenUsage.present}/${rolloutCount} terminal records`, source: "container runtime" })}
          {providerTokens === null ? null : usageCard("Provider tokens", providerSummary(providerTokens), tokenLimit, exactCount, { valueSuffix: " billed", coverage: `${exactCount(providerPromptTokens ?? 0)} prompt + ${exactCount(providerCompletionTokens ?? 0)} completion`, source: "Workshop proxy" })}
          {runLifecycle?.usage.costSource === "workshop_proxy"
            ? usageCard("Provider cost", providerSummary(providerCost), costLimit, exactUsd, { coverage: "run-level receipt", source: "Workshop proxy" })
            : usageCard("Cost", costUsage, costLimit, exactUsd)}
          {usageCard("Frames", frameUsage, null)}
        </div>
        <div className="trace-workbench-distributions" style={{ display: "grid", gridTemplateColumns: "var(--tw-distribution-columns, repeat(auto-fit,minmax(260px,1fr)))", gap: "var(--sv-sp-4)", marginTop: "var(--sv-sp-3)", paddingTop: "var(--sv-sp-3)", borderTop: "1px solid var(--sv-border)" }}>
          <div>
            <div style={{ color: "var(--sv-text-faint)", fontSize: "var(--sv-fs-micro)", textTransform: "uppercase" }}>Reward distribution</div>
            <div style={{ ...mono, marginTop: 3, fontSize: "var(--sv-fs-meta)" }}>median {reward(median)} · range {reward(rewardMin)}–{reward(rewardMax)}</div>
            <div style={{ display: "flex", alignItems: "end", gap: 4, height: 34, marginTop: 5 }}>
              {buckets.map((bucket, index) => (
                <button key={index} type="button" aria-label={`${bucket.low.toFixed(2)} to ${bucket.high.toFixed(2)} · ${bucket.count} seeds`} title={`${bucket.low.toFixed(2)} to ${bucket.high.toFixed(2)} · ${bucket.count} seeds`} onClick={() => onFilter(filter?.kind === "reward" && filter.low === bucket.low ? null : { kind: "reward", ...bucket })} style={{ flex: 1, height: `${Math.max(5, bucket.count / Math.max(...buckets.map((row) => row.count), 1) * 100)}%`, border: "1px solid var(--sv-accent)", borderRadius: "3px 3px 0 0", background: filter?.kind === "reward" && filter.low === bucket.low ? "var(--sv-accent)" : "var(--sv-accent-soft)", cursor: "pointer" }} />
              ))}
            </div>
          </div>
          <div>
            <div style={{ color: "var(--sv-text-faint)", fontSize: "var(--sv-fs-micro)", textTransform: "uppercase" }}>Achievements · unique seeds / eligible · occurrences · first · best</div>
            <div style={{ display: "grid", gap: 3, maxHeight: 76, overflowY: "auto", marginTop: 4 }}>
              {achievementFacts.authoritative && achievementFacts.value === null ? (
                <span style={{ color: "var(--sv-text-faint)", fontSize: "var(--sv-fs-meta)" }}>Achievements unavailable.</span>
              ) : achievements.length ? achievements.map((row) => (
                <button className="trace-workbench-achievement" key={row.name} type="button" onClick={() => onFilter(filter?.kind === "achievement" && filter.name === row.name ? null : { kind: "achievement", name: row.name })} style={{ display: "grid", gridTemplateColumns: "var(--tw-achievement-columns, minmax(120px,1fr) auto)", gap: 8, padding: "2px 4px", border: "1px solid transparent", borderRadius: 4, background: filter?.kind === "achievement" && filter.name === row.name ? "var(--sv-accent-soft)" : "transparent", color: "var(--sv-text)", cursor: "pointer", textAlign: "left" }}>
                  <span style={{ ...mono, overflow: "hidden", textOverflow: "ellipsis" }}>{row.name}</span>
                  <span style={{ color: "var(--sv-text-faint)", fontSize: "var(--sv-fs-micro)" }}>{row.seeds}/{trials.length} · {row.occurrences === null ? "occurrences unavailable" : `${row.occurrences}×`} · first {row.firstSeed ?? MISSING} · best {row.bestSeed ?? MISSING}</span>
                </button>
              )) : <span style={{ color: "var(--sv-text-faint)", fontSize: "var(--sv-fs-meta)" }}>{achievementFacts.authoritative ? "No achievements achieved." : "No achievements reported yet."}</span>}
              <FactMetadata summary={achievementFacts} />
            </div>
          </div>
        </div>
      </details>
    </section>
  );
}

/** Vitals a Craftax readout carries. Colours match the published viewer. */
const VITALS: [string, string][] = [
  ["health", "#c2553f"],
  ["food", "#c99b3f"],
  ["drink", "#3d78bb"],
  ["energy", "#6f9a4d"],
  ["mana", "#8a5fd0"]
];

function Chip({
  label,
  tone = "muted",
  title
}: {
  label: string;
  tone?: "muted" | "ok" | "bad" | "warn" | "accent";
  title?: string;
}) {
  const palette = {
    muted: ["var(--sv-surface-muted)", "var(--sv-text-muted)", "var(--sv-border)"],
    ok: ["var(--sv-ok-bg)", "var(--sv-ok-fg)", "var(--sv-ok-edge)"],
    bad: ["var(--sv-bad-bg)", "var(--sv-bad-fg)", "var(--sv-bad-edge)"],
    warn: ["var(--sv-warn-bg)", "var(--sv-warn-fg)", "var(--sv-warn-edge)"],
    accent: ["var(--sv-accent-soft)", "var(--sv-accent-hot)", "var(--sv-accent-soft)"]
  }[tone];
  return (
    <span
      title={title}
      style={{
        display: "inline-block",
        padding: "1px var(--sv-sp-2)",
        border: `1px solid ${palette[2]}`,
        borderRadius: 99,
        background: palette[0],
        color: palette[1],
        fontSize: "var(--sv-fs-micro)",
        whiteSpace: "nowrap"
      }}
    >
      {label}
    </span>
  );
}

function Disclosure({
  summary,
  count,
  children
}: {
  summary: string;
  count?: number | null;
  children: React.ReactNode;
}) {
  return (
    <details style={{ marginTop: "var(--sv-sp-2)" }}>
      <summary
        style={{
          cursor: "pointer",
          color: "var(--sv-text-muted)",
          fontSize: "var(--sv-fs-meta)"
        }}
      >
        {summary}
        {count != null ? ` · ${count}` : ""}
      </summary>
      <div style={{ marginTop: "var(--sv-sp-2)" }}>{children}</div>
    </details>
  );
}

/**
 * The environment picture.
 *
 * A canonical native PNG when the relay retained one. Missing native media is
 * reported as unavailable; symbolic observations are evidence of a different
 * type and are never substituted for the frame in the default workstation.
 */
function FrameCanvas({
  frame,
  media,
  loaded,
  branding
}: {
  frame: TraceFrame | null;
  media: MediaClient;
  loaded: LoadedMedia | undefined;
  branding: TraceWorkbenchBranding;
}) {
  const label = frame ? `${branding.label} frame at step ${frame.step}` : "No frame for this call";
  const surface: React.CSSProperties = {
    display: "grid",
    placeItems: "center",
    minHeight: 320,
    border: "1px solid var(--sv-border)",
    borderRadius: "var(--sv-radius-lg)",
    background: "#12160f",
    overflow: "hidden"
  };
  if (frame?.media && loaded) {
    return (
      <div style={surface} data-testid={branding.frameTestId}>
        <img
          src={loaded.dataUrl}
          alt={label}
          style={{ width: "100%", height: "auto", imageRendering: "pixelated", display: "block" }}
        />
      </div>
    );
  }
  if (frame?.media && !loaded) {
    const failure = media.failures().get(frame.media.casDigest);
    return (
      <div style={{ ...surface, color: "#8d968a", fontSize: "var(--sv-fs-meta)" }}>
        {failure ? `This frame could not be loaded: ${failure}` : "Loading frame…"}
      </div>
    );
  }
  return (
    <div
      data-testid={`${branding.frameTestId}-unavailable`}
      style={{
        ...surface,
        padding: "var(--sv-sp-4)",
        color: "#8d968a",
        fontSize: "var(--sv-fs-meta)",
        textAlign: "center"
      }}
    >
      Native PNG unavailable: {frame?.unavailable ?? "this call recorded no environment frame."}
    </div>
  );
}

/** Vitals and inventory, read from the structured readout only. */
function Hud({ step }: { step: TraceStep | null }) {
  const readout = step?.content.readout ?? null;
  const vitals = (readout?.vitals ?? readout?.stats ?? null) as Any | null;
  const inventory = (readout?.inventory ?? null) as Any | null;
  if (!vitals && !inventory) return null;
  return (
    <div
      style={{
        display: "grid",
        gap: "var(--sv-sp-2)",
        marginTop: "var(--sv-sp-3)",
        padding: "var(--sv-sp-3)",
        border: "1px solid var(--sv-border)",
        borderRadius: "var(--sv-radius)",
        background: "var(--sv-surface-muted)"
      }}
    >
      {vitals ? (
        <div style={{ display: "flex", flexWrap: "wrap", gap: "var(--sv-sp-3)" }}>
          {VITALS.filter(([name]) => vitals[name] != null).map(([name, colour]) => (
            <span key={name} style={{ display: "flex", alignItems: "center", gap: 4 }}>
              <span
                aria-hidden
                style={{ width: 7, height: 7, borderRadius: 99, background: colour }}
              />
              <span style={{ color: "var(--sv-text-faint)", fontSize: "var(--sv-fs-micro)" }}>
                {name}
              </span>
              <strong style={{ ...mono, fontSize: "var(--sv-fs-meta)" }}>{vitals[name]}</strong>
            </span>
          ))}
        </div>
      ) : null}
      {inventory ? (
        <div style={{ display: "flex", flexWrap: "wrap", gap: "var(--sv-sp-1)" }}>
          {Object.entries(inventory)
            .filter(([, count]) => Number(count) > 0)
            .map(([name, count]) => (
              <Chip key={name} label={`${name} ${count}`} />
            ))}
        </div>
      ) : null}
    </div>
  );
}

/** The complete ordered trajectory. Every call, always — playback never hides one. */
function TrajectoryRail({
  view,
  selected,
  onSelect,
  query,
  onQuery
}: {
  view: EvalTraceView;
  selected: number;
  onSelect: (index: number) => void;
  query: string;
  onQuery: (value: string) => void;
}) {
  const railRef = useRef<HTMLDivElement | null>(null);
  const activeRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    // Auto-follow scrolls the rail, never the document. The reviewer's page
    // position is theirs; only this bounded box moves.
    const rail = railRef.current;
    const active = activeRef.current;
    if (!rail || !active) return;
    const top = active.offsetTop - rail.offsetTop;
    if (top < rail.scrollTop || top + active.offsetHeight > rail.scrollTop + rail.clientHeight) {
      rail.scrollTop = top - rail.clientHeight / 2 + active.offsetHeight / 2;
    }
  }, [selected, view.steps.length]);

  const needle = query.trim().toLowerCase();
  const matches = (step: TraceStep) => {
    if (!needle) return true;
    const haystack = [
      step.title,
      step.content.reasoning,
      step.content.message,
      ...step.action.proposed,
      ...step.action.applied.map((row) => row.name),
      ...step.action.rejected.map((row) => row.name),
      ...step.achievements,
      ...step.tool_calls.map((call) => call.name)
    ]
      .filter(Boolean)
      .join(" ")
      .toLowerCase();
    return haystack.includes(needle);
  };

  return (
    <div style={{ display: "grid", gridTemplateRows: "auto 1fr", gap: "var(--sv-sp-2)", minHeight: 0 }}>
      <input
        value={query}
        onChange={(event) => onQuery(event.target.value)}
        placeholder="Search calls, actions, achievements"
        aria-label="Search the trajectory"
        style={{
          padding: "var(--sv-sp-2)",
          border: "1px solid var(--sv-border)",
          borderRadius: "var(--sv-radius-sm)",
          background: "var(--sv-surface)",
          color: "var(--sv-text)",
          fontSize: "var(--sv-fs-meta)"
        }}
      />
      <div
        ref={railRef}
        role="listbox"
        aria-label="Policy calls"
        style={{ overflowY: "auto", minHeight: 0, display: "grid", gap: 3, alignContent: "start" }}
      >
        {view.steps.map((step, index) => {
          const active = index === selected;
          const dim = !matches(step);
          return (
            <button
              key={step.id}
              ref={active ? activeRef : undefined}
              type="button"
              role="option"
              aria-selected={active}
              onClick={() => onSelect(index)}
              style={{
                display: "grid",
                gridTemplateColumns: "auto 1fr auto",
                gap: "var(--sv-sp-2)",
                alignItems: "center",
                padding: "var(--sv-sp-2)",
                border: `1px solid ${active ? "var(--sv-accent)" : "var(--sv-border)"}`,
                borderRadius: "var(--sv-radius-sm)",
                background: active ? "var(--sv-accent-soft)" : "var(--sv-surface)",
                color: "var(--sv-text)",
                cursor: "pointer",
                textAlign: "left",
                opacity: dim ? 0.35 : 1
              }}
            >
              <span style={{ ...mono, fontSize: "var(--sv-fs-micro)", color: "var(--sv-text-faint)" }}>
                {String(step.index).padStart(2, "0")}
              </span>
              <span style={{ display: "grid", gap: 2, minWidth: 0 }}>
                <span
                  style={{
                    fontSize: "var(--sv-fs-meta)",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap"
                  }}
                >
                  {step.action.applied.length
                    ? step.action.applied.map((row) => row.name).join(" · ")
                    : step.status === "running"
                      ? "deciding…"
                      : "no environment action"}
                </span>
                <span style={{ display: "flex", gap: 3, flexWrap: "wrap" }}>
                  {step.status === "running" ? <Chip label="running" tone="accent" /> : null}
                  {step.status !== "running" && step.status !== "completed" ? (
                    <Chip label={step.status.replaceAll("_", " ")} tone="bad" />
                  ) : null}
                  {step.achievements.map((name) => (
                    <Chip key={name} label={name} tone="warn" />
                  ))}
                  {step.action.rejected.length ? (
                    <Chip label={`${step.action.rejected.length} rejected`} tone="bad" />
                  ) : null}
                </span>
              </span>
              <span style={{ ...mono, fontSize: "var(--sv-fs-micro)", color: "var(--sv-text-faint)" }}>
                {step.turn_start === null
                  ? MISSING
                  : step.turn_end === null || step.turn_end === step.turn_start
                    ? `t${step.turn_start}`
                    : `t${step.turn_start}–${step.turn_end}`}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

/** Observation → decision → applied → outcome, with raw behind disclosure. */
function CallDetail({ view, step }: { view: EvalTraceView; step: TraceStep | null }) {
  if (!step) {
    return (
      <p style={{ margin: 0, color: "var(--sv-text-faint)", fontSize: "var(--sv-fs-meta)" }}>
        This rollout has recorded no policy call yet.
      </p>
    );
  }
  const section: React.CSSProperties = {
    paddingTop: "var(--sv-sp-3)",
    borderTop: "1px solid var(--sv-border)"
  };
  const heading: React.CSSProperties = {
    margin: 0,
    color: "var(--sv-text-faint)",
    fontSize: "var(--sv-fs-micro)",
    letterSpacing: ".06em",
    textTransform: "uppercase"
  };
  const body: React.CSSProperties = {
    margin: "var(--sv-sp-1) 0 0",
    fontSize: "var(--sv-fs-body)",
    lineHeight: 1.5,
    whiteSpace: "pre-wrap"
  };
  const pre: React.CSSProperties = {
    ...mono,
    margin: 0,
    padding: "var(--sv-sp-2)",
    maxHeight: 220,
    overflow: "auto",
    border: "1px solid var(--sv-border)",
    borderRadius: "var(--sv-radius-sm)",
    background: "var(--sv-surface-muted)",
    fontSize: "var(--sv-fs-micro)",
    whiteSpace: "pre-wrap"
  };
  return (
    <div style={{ display: "grid", gap: "var(--sv-sp-3)", overflowY: "auto", minHeight: 0 }}>
      <div>
        <p style={heading}>Observed</p>
        <p style={{ ...body, color: "var(--sv-text-muted)" }}>
          {step.content.observation
            ? step.content.observation.split("\n").slice(0, 3).join("\n")
            : "No observation was recorded before this call."}
        </p>
        {view.system_prompt ? (
          <Disclosure summary="System prompt">
            <pre style={pre}>{view.system_prompt}</pre>
          </Disclosure>
        ) : null}
        {step.content.input_messages.length ? (
          <Disclosure summary="Policy-visible messages" count={step.content.input_messages.length}>
            <div style={{ display: "grid", gap: "var(--sv-sp-2)" }}>
              {step.content.input_messages.map((message, index) => (
                <div key={index}>
                  <Chip label={message.role} />
                  <pre style={{ ...pre, marginTop: 3 }}>{message.content || MISSING}</pre>
                </div>
              ))}
            </div>
          </Disclosure>
        ) : null}
        {step.content.observation ? (
          <Disclosure summary="Full observation">
            <pre style={pre}>{step.content.observation}</pre>
          </Disclosure>
        ) : null}
      </div>

      <div style={section}>
        <p style={heading}>Decided</p>
        {step.content.reasoning ? (
          <p style={{ ...body, color: "var(--sv-text-muted)", fontStyle: "italic" }}>
            {step.content.reasoning}
          </p>
        ) : null}
        {step.content.message ? <p style={body}>{step.content.message}</p> : null}
        {!step.content.reasoning && !step.content.message ? (
          <p style={{ ...body, color: "var(--sv-text-faint)" }}>
            {step.status === "running"
              ? "This call is still open; the model has not answered yet."
              : step.status === "aborted"
                ? `This call was aborted: ${step.closure?.reason.replaceAll("_", " ") ?? "closure reason unavailable"} (${step.closure?.source ?? "source unavailable"}).`
              : "No reasoning or message was recorded for this call."}
          </p>
        ) : null}
        {step.tool_calls.map((call, index) => (
          <div key={call.id ?? index} style={{ marginTop: "var(--sv-sp-2)" }}>
            <Chip label={call.name} tone="accent" />
            <Disclosure summary="Tool arguments">
              <pre style={pre}>
                {typeof call.arguments === "string"
                  ? call.argumentsText
                  : JSON.stringify(call.arguments, null, 2)}
              </pre>
            </Disclosure>
          </div>
        ))}
        {step.action.proposed.length ? (
          <Disclosure summary="Proposed actions" count={step.action.proposed.length}>
            <div style={{ display: "flex", gap: 3, flexWrap: "wrap" }}>
              {step.action.proposed.map((name, index) => (
                <Chip key={`${name}-${index}`} label={name} />
              ))}
            </div>
            <p
              style={{
                margin: "var(--sv-sp-2) 0 0",
                color: "var(--sv-text-faint)",
                fontSize: "var(--sv-fs-micro)"
              }}
            >
              What the model asked for. The environment's own record is below.
            </p>
          </Disclosure>
        ) : null}
      </div>

      {step.rubric.length ? (
        <details className="trace-workbench-rubric" style={section}>
          <summary style={{ cursor: "pointer" }}>
            <span style={heading}>Verifier &amp; rubric</span>{" "}
            <strong style={{ ...mono, fontSize: "var(--sv-fs-meta)" }}>
              {step.rubric.filter((grade) => grade.met === true).length}/{step.rubric.length} met
            </strong>
          </summary>
          <div style={{ display: "grid", gap: "var(--sv-sp-2)", marginTop: "var(--sv-sp-2)" }}>
            {step.rubric.map((grade, index) => (
              <details key={`${grade.index ?? index}-${grade.criterion}`} style={{ padding: "var(--sv-sp-2)", border: "1px solid var(--sv-border)", borderRadius: "var(--sv-radius-sm)", background: "var(--sv-surface-muted)" }}>
                <summary style={{ cursor: "pointer", fontSize: "var(--sv-fs-meta)" }}>
                  <Chip label={grade.met === true ? "met" : grade.met === false ? "unmet" : "unavailable"} tone={grade.met === true ? "ok" : grade.met === false ? "bad" : "muted"} />{" "}
                  {grade.points === null ? null : <span style={{ ...mono, color: "var(--sv-text-faint)" }}>{grade.points > 0 ? "+" : ""}{grade.points} · </span>}
                  <span>{grade.criterion}</span>
                </summary>
                {grade.explanation ? <p style={{ ...body, marginTop: "var(--sv-sp-2)", color: "var(--sv-text-muted)" }}>{grade.explanation}</p> : null}
              </details>
            ))}
          </div>
        </details>
      ) : null}

      <div style={section}>
        <p style={heading}>Applied by the environment</p>
        {step.action.applied.length ? (
          <ol
            style={{
              margin: "var(--sv-sp-2) 0 0",
              paddingLeft: "var(--sv-sp-5)",
              display: "grid",
              gap: 3
            }}
          >
            {step.action.applied.map((row, index) => {
              const noop = step.action.noop.some(
                (other) => other.turn === row.turn && other.name === row.name
              );
              return (
                <li key={`${row.name}-${row.turn}-${index}`} style={{ fontSize: "var(--sv-fs-meta)" }}>
                  <span style={mono}>{row.name}</span>
                  <span style={{ color: "var(--sv-text-faint)" }}>
                    {row.turn === null ? "" : ` · t${row.turn}`}
                  </span>
                  {noop ? <> <Chip label="no effect" /></> : null}
                </li>
              );
            })}
          </ol>
        ) : (
          <p style={{ ...body, color: "var(--sv-text-faint)" }}>
            The environment applied no action for this call.
          </p>
        )}
        {step.action.rejected.length ? (
          <div style={{ marginTop: "var(--sv-sp-2)", display: "grid", gap: 3 }}>
            {step.action.rejected.map((row, index) => (
              <div key={`${row.name}-${index}`} style={{ fontSize: "var(--sv-fs-meta)" }}>
                <Chip label="rejected" tone="bad" />{" "}
                <span style={mono}>{row.name}</span>
                <span style={{ color: "var(--sv-text-faint)" }}>
                  {row.reason ? ` · ${row.reason}` : " · no reason recorded"}
                </span>
              </div>
            ))}
          </div>
        ) : null}
      </div>

      <div style={section}>
        <p style={heading}>Changed</p>
        <div
          style={{
            display: "flex",
            gap: "var(--sv-sp-2)",
            flexWrap: "wrap",
            marginTop: "var(--sv-sp-2)"
          }}
        >
          <Chip label={`reward ${reward(step.reward)}`} tone={step.reward ? "ok" : "muted"} />
          <Chip label={`in ${tokens(step.tokens.input)}`} />
          <Chip label={`out ${tokens(step.tokens.output)}`} />
          {step.achievements.map((name) => (
            <Chip key={name} label={name} tone="warn" />
          ))}
        </div>
        {step.state_delta.length ? (
          <Disclosure summary="State deltas" count={step.state_delta.length}>
            <div style={{ display: "grid", gap: 2 }}>
              {step.state_delta.map((delta, index) => (
                <div
                  key={`${delta.field}-${index}`}
                  style={{ ...mono, fontSize: "var(--sv-fs-micro)" }}
                >
                  {delta.field}: {String(delta.before ?? MISSING)} → {String(delta.after ?? MISSING)}
                  {delta.turn === null ? "" : `  ·  t${delta.turn}`}
                </div>
              ))}
            </div>
          </Disclosure>
        ) : null}
        <Disclosure summary="Raw producer events" count={step.raw.length}>
          <pre style={pre}>
            {JSON.stringify(
              view.events.filter((event) => step.raw.includes(event.sequence)),
              null,
              2
            )}
          </pre>
        </Disclosure>
      </div>
    </div>
  );
}

export function TraceWorkbench({ branding, ...props }: TraceWorkbenchProps & { branding: TraceWorkbenchBranding }) {
  const run = (props.run ?? props.data?.run ?? null) as Any | null;
  const aggregateCandidate = (
    props.runViewV2?.aggregate
    ?? props.data?.runViewV2?.aggregate
    ?? props.data?.aggregate
    ?? null
  ) as Any | null;
  const aggregate = evalAggregateV1(aggregateCandidate, typeof run?.id === "string" ? run.id : null);
  const optimizerEvents = useMemo(
    () => [
      ...(Array.isArray(props.events) ? props.events : []),
      ...(Array.isArray(props.enrichmentEvents) ? props.enrichmentEvents : [])
    ],
    [props.events, props.enrichmentEvents]
  );
  const media = props.media ?? NO_MEDIA;

	const liveTrials = useMemo(
    () => (run ? craftaxTrialsFromRun(run, optimizerEvents, props.runViewV2 ?? props.data?.runViewV2 ?? null) : []),
    [run, optimizerEvents, props.runViewV2, props.data?.runViewV2]
  );
	const trials = useMemo(() => liveTrials.map((row) => {
		const sealed = props.sealedTraceProjections?.find((candidate) =>
			candidate.trialId === row.trialId ||
			(Boolean(row.rolloutId) && candidate.rolloutId === row.rolloutId)
		);
		if (!sealed) return row;
		const sealedView = craftaxTraceFromSealedTrace(sealed.projection, {
			traceId: row.rolloutId ?? row.trialId,
			scenario: row.view.task.scenario,
			seed: row.seed,
			status: row.state,
			model: row.view.run.model,
			provider: row.view.run.provider,
			effort: row.view.run.effort,
			totalReward: row.reward,
			contentDigest: sealed.digest
		});
		return { ...row, view: reconcileCraftaxTrace(row.view, sealedView).view ?? row.view };
	}), [liveTrials, props.sealedTraceProjections]);
  const trialFactReads = useMemo(
    () => trials.map((row) => readReportedFacts(reportedFactRecord(row))),
    [trials]
  );
  const achievementFactsAuthoritative = trialFactReads.some((read) => read.status !== "absent");

  // Selection is held by identity, not by object. A trial folded again on the
  // next append is a new object with the same id, and a selection keyed on the
  // object would reset on every update — which is precisely the "resets while
  // you are reading it" failure this pane exists to avoid.
  const [selectedTrialId, setSelectedTrialId] = useState<string | null>(null);
  const [selectedCall, setSelectedCall] = useState(0);
  const [selectedFrame, setSelectedFrame] = useState<number | null>(null);
  const [following, setFollowing] = useState(true);
  const [playing, setPlaying] = useState(false);
  const [query, setQuery] = useState("");
  const [loaded, setLoaded] = useState<LoadedMedia | undefined>(undefined);
  const [aggregateFilter, setAggregateFilter] = useState<AggregateFilter>(null);

  const trial: TrialView | null =
    trials.find((row) => row.trialId === selectedTrialId) ??
    trials.find((row) => row.state === "running") ??
    trials[0] ??
    null;
  const view = trial?.view ?? null;
  const selectedFrameFact = summarizeNumericReportedFact(
    trial ? [reportedFactRecord(trial)] : [],
    "frames",
    trial ? [trial.view.coverage.framesRetained] : []
  );

  const frameDigests = useMemo(
    () => (view?.frames ?? []).map((frame) => frame.media?.casDigest ?? ""),
    [view]
  );

  // Follow the newest call and frame — until the reviewer takes over.
  useEffect(() => {
    if (!following || !view) return;
    const lastCall = Math.max(0, view.steps.length - 1);
    setSelectedCall(lastCall);
    setSelectedFrame(view.frames.length ? view.frames.length - 1 : null);
  }, [following, view?.steps.length, view?.frames.length]);

  const step = view?.steps[selectedCall] ?? null;
  const frameIndex =
    selectedFrame ?? (step?.frames.length ? step.frames[step.frames.length - 1] : null);
  const frame = frameIndex === null ? null : (view?.frames[frameIndex] ?? null);

  useEffect(() => {
    if (frameIndex === null || !frameDigests[frameIndex]) {
      setLoaded(undefined);
      return;
    }
    let cancelled = false;
    // Only the selection and a small window around it. A 500-step episode is
    // 500 PNGs, and warming all of them to show one is how a pane stops
    // responding to the scrubber it is meant to serve.
    void media.warm(frameDigests, frameIndex).then((result) => {
      if (!cancelled) setLoaded(result);
    });
    return () => {
      cancelled = true;
    };
  }, [media, frameDigests, frameIndex]);

  /** Any deliberate move backwards stops auto-follow; forwards at the tip keeps it. */
  const gotoFrame = useCallback(
    (next: number) => {
      if (!view) return;
      const clamped = Math.max(0, Math.min(view.frames.length - 1, next));
      setSelectedFrame(clamped);
      if (clamped < view.frames.length - 1) {
        setFollowing(false);
        setPlaying(false);
      }
      const owner = view.steps.findIndex((candidate) => candidate.frames.includes(clamped));
      if (owner >= 0) setSelectedCall(owner);
    },
    [view]
  );

  const selectCall = useCallback(
    (index: number) => {
      if (!view) return;
      setSelectedCall(index);
      const owned = view.steps[index]?.frames ?? [];
      setSelectedFrame(owned.length ? owned[owned.length - 1] : null);
      // Selecting a call is a manual act, and playback pauses on one.
      setPlaying(false);
      if (index < view.steps.length - 1) setFollowing(false);
    },
    [view]
  );

  useEffect(() => {
    if (!playing || !view?.frames.length) return;
    const timer = window.setInterval(() => {
      setSelectedFrame((current) => {
        const next = (current ?? 0) + 1;
        if (next >= view.frames.length) {
          setPlaying(false);
          return current;
        }
        const owner = view.steps.findIndex((candidate) => candidate.frames.includes(next));
        if (owner >= 0) setSelectedCall(owner);
        return next;
      });
    }, 450);
    return () => window.clearInterval(timer);
  }, [playing, view]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      // Shortcuts do nothing while the reviewer is typing in the rail's search.
      if (target && /^(INPUT|SELECT|TEXTAREA)$/.test(target.tagName)) return;
      if (event.key === "j" || event.key === "ArrowDown") {
        selectCall(Math.min((view?.steps.length ?? 1) - 1, selectedCall + 1));
      } else if (event.key === "k" || event.key === "ArrowUp") {
        selectCall(Math.max(0, selectedCall - 1));
      } else if (event.key === "ArrowLeft") {
        gotoFrame((frameIndex ?? 0) - 1);
      } else if (event.key === "ArrowRight") {
        gotoFrame((frameIndex ?? -1) + 1);
      } else {
        return;
      }
      event.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selectCall, gotoFrame, selectedCall, frameIndex, view?.steps.length]);

  const sealed = view?.integrity.status === "sealed";
  const terminal = aggregate
    ? aggregate.lifecycle === "terminal"
    : trial ? trial.state === "done" || trial.state === "failed" : false;
  const aggregateWork = aggregate ? evalAggregateWorkFacts(aggregate) : null;
  const aggregateRollouts = aggregateWork?.rolloutCount ?? null;
  const aggregateTerminal = aggregateWork?.terminalCount ?? null;
  // An imported seal is an opaque import: it carries no native step/frame
  // identity, so frames never existed for it. A frame-free replay in a
  // non-frame-centric family is the same situation by declaration. Either way
  // the honest surface is the stream events, not frame-shaped absence copy.
  const importedSeal = typeof run?.objective === "string" && String(run.objective).startsWith("imported from");
  const framesAbsent = view !== null && view.frames.length === 0 && view.coverage.framesRetained === 0;
  const streamOnly = importedSeal || (!branding.frameCentric && framesAbsent);
  const frameFactSource = selectedFrameFact.sources.length ? selectedFrameFact.sources.join(", ") : "not_reported";
  const frameFactReason = selectedFrameFact.unavailableReasons.length
    ? selectedFrameFact.unavailableReasons.join(", ")
    : "none";
  // Append the reason only when there is one, for the same reason as above:
  // "0 frames · source: trusted_trace_v5 · unavailable reason: none" invites a
  // reader to hunt for a failure that did not happen.
  const frameReasonSuffix = selectedFrameFact.unavailableReasons.length
    ? ` · unavailable reason: ${frameFactReason}`
    : "";
  const terminalFrameCopy = selectedFrameFact.authoritative
    ? selectedFrameFact.value === null
      ? `Frames unavailable · source: ${frameFactSource}${frameReasonSuffix}`
      : `${selectedFrameFact.value} frame${selectedFrameFact.value === 1 ? "" : "s"} · source: ${frameFactSource}${frameReasonSuffix}`
    : view
      ? `${view.coverage.framesRetained}/${view.coverage.framesDeclared} frame observations retained · ${view.coverage.uniqueCasBlobs} unique CAS blob${view.coverage.uniqueCasBlobs === 1 ? "" : "s"}`
      : "";
  const button: React.CSSProperties = {
    padding: "var(--sv-sp-1) var(--sv-sp-3)",
    border: "1px solid var(--sv-border-strong)",
    borderRadius: "var(--sv-radius-sm)",
    background: "var(--sv-surface)",
    color: "var(--sv-text)",
    fontSize: "var(--sv-fs-meta)",
    cursor: "pointer"
  };

  return (
    <VisualChrome
      kicker={`${branding.label} · ${aggregateTerminal ?? trials.filter((row) => row.state === "done" || row.state === "failed").length}/${aggregateRollouts ?? trials.length} seeds`}
      title={props.title ?? branding.defaultTitle}
      lede={props.lede}
      live={!terminal}
      testId={branding.testId}
      observation={{
        transportState: props.loadError ? "error" : terminal ? "terminal" : "live",
        rolloutCount: aggregateRollouts ?? trials.length,
        renderedFrameCount: view?.frames.filter((row) => row.media).length ?? 0,
        semanticEventCount: view?.events.length ?? 0,
        terminal,
        error: props.loadError ?? null
      }}
      footer={
        <span style={{ ...mono, fontSize: "var(--sv-fs-micro)" }}>
          {sealed ? "Sealed Trace V5" : "Live relay"}
          {view?.integrity.content_digest ? ` · ${view.integrity.content_digest.slice(0, 20)}` : ""}
          {terminalFrameCopy ? ` · ${terminalFrameCopy}` : ""}
        </span>
      }
    >
      {props.loadError ? (
        <p
          style={{
            margin: "0 0 var(--sv-sp-3)",
            color: "var(--sv-bad-fg)",
            fontSize: "var(--sv-fs-meta)"
          }}
        >
          {props.loadError}
        </p>
      ) : null}

      <RunAggregateHeader
        run={run}
        runLifecycle={props.runLifecycle}
        aggregate={aggregate}
        trials={trials}
        filter={aggregateFilter}
        onFilter={setAggregateFilter}
        testId={branding.aggregatesTestId}
      />

      <div
        style={{
          display: "flex",
          gap: "var(--sv-sp-1)",
          flexWrap: "wrap",
          marginBottom: "var(--sv-sp-3)"
        }}
      >
        {trials.map((row, index) => (
          <button
            key={row.trialId}
            type="button"
            onClick={() => {
              setSelectedTrialId(row.trialId);
              setFollowing(row.state === "running");
              setSelectedCall(0);
              setSelectedFrame(null);
            }}
            style={{
              ...button,
              borderColor: row.trialId === trial?.trialId ? "var(--sv-accent)" : "var(--sv-border)",
              background:
                row.trialId === trial?.trialId ? "var(--sv-accent-soft)" : "var(--sv-surface)",
              opacity: aggregateFilter === null ||
                (aggregateFilter.kind === "achievement" && (
                  achievementFactsAuthoritative
                    ? trialFactReads[index]?.status === "present" && trialFactReads[index].facts.achievements.value?.includes(aggregateFilter.name) === true
                    : row.view.achievements.includes(aggregateFilter.name)
                )) ||
                (aggregateFilter.kind === "reward" && row.reward !== null && row.reward >= aggregateFilter.low && (aggregateFilter.inclusiveHigh ? row.reward <= aggregateFilter.high : row.reward < aggregateFilter.high))
                ? 1 : .3
            }}
          >
            <span style={mono}>seed {row.seed ?? MISSING}</span>
            <span style={{ color: "var(--sv-text-faint)" }}>
              {row.state === "done" ? ` · ${reward(row.reward)}` : ` · ${row.state}`}
            </span>
          </button>
        ))}
      </div>

      {!view ? (
        <p style={{ margin: 0, color: "var(--sv-text-faint)", fontSize: "var(--sv-fs-meta)" }}>
          No trial has been dispatched yet.
        </p>
      ) : (
        <div
          className="trace-workbench-layout"
          style={{
            display: "grid",
            gridTemplateColumns: "var(--tw-main-columns, minmax(320px, 3fr) minmax(240px, 2fr))",
            gap: "var(--sv-sp-4)",
            height: "var(--tw-main-height, 720px)",
            minHeight: 0
          }}
        >
          <section className="trace-workbench-frame-column" style={{ display: "grid", gridTemplateRows: "1fr auto auto", minHeight: 0 }}>
            {streamOnly ? (
              <div
                data-testid={`${branding.testId}-stream-only`}
                role="note"
                style={{
                  display: "grid",
                  placeItems: "center",
                  minHeight: branding.frameCentric ? 320 : 72,
                  padding: "var(--sv-sp-4)",
                  border: "1px solid var(--sv-warn-edge)",
                  borderRadius: "var(--sv-radius-lg)",
                  background: "var(--sv-warn-bg)",
                  color: "var(--sv-warn-fg)",
                  fontSize: "var(--sv-fs-meta)",
                  textAlign: "center"
                }}
              >
                {selectedFrameFact.authoritative
                  ? `${terminalFrameCopy}; showing stream events only.`
                  : importedSeal
                    ? "Imported seal has no native step/frame identity; showing stream events only."
                    : "This run recorded no environment frames; showing stream events only."}
              </div>
            ) : (
              <FrameCanvas frame={frame} media={media} loaded={loaded} branding={branding} />
            )}
            {streamOnly ? <span /> : <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: "var(--sv-sp-2)",
                marginTop: "var(--sv-sp-3)",
                flexWrap: "wrap"
              }}
            >
              <button type="button" style={button} onClick={() => gotoFrame((frameIndex ?? 0) - 1)}>
                ◀ prev
              </button>
              <button
                type="button"
                style={button}
                onClick={() => {
                  setPlaying((value) => !value);
                  if (!playing) setFollowing(false);
                }}
              >
                {playing ? "❚❚ pause" : "▶ play"}
              </button>
              <button type="button" style={button} onClick={() => gotoFrame((frameIndex ?? -1) + 1)}>
                next ▶
              </button>
              <input
                type="range"
                min={0}
                max={Math.max(0, view.frames.length - 1)}
                value={frameIndex ?? 0}
                aria-label="Frame scrubber"
                onChange={(event) => gotoFrame(Number(event.target.value))}
                style={{ flex: 1, minWidth: 120 }}
              />
              <span style={{ ...mono, fontSize: "var(--sv-fs-micro)", color: "var(--sv-text-faint)" }}>
                frame {view.frames.length ? (frameIndex ?? 0) + 1 : 0}/{view.frames.length} · call{" "}
                {view.steps.length ? selectedCall + 1 : 0}/{view.steps.length}
                {frame ? ` · t${frame.step}` : ""}
              </span>
              {!following && !terminal ? (
                <button
                  type="button"
                  style={{ ...button, borderColor: "var(--sv-accent)", color: "var(--sv-accent-hot)" }}
                  onClick={() => {
                    setFollowing(true);
                    setPlaying(false);
                  }}
                >
                  Follow live
                </button>
              ) : null}
            </div>}
            <Hud step={step} />
            {view.coverage.degradations.length ? (
              <Disclosure summary="Retention receipts" count={view.coverage.degradations.length}>
                <div style={{ display: "grid", gap: 3 }}>
                  {view.coverage.degradations.map((row, index) => (
                    <div key={index} style={{ fontSize: "var(--sv-fs-micro)" }}>
                      <Chip label={row.reason} tone="warn" />{" "}
                      <span style={{ color: "var(--sv-text-muted)" }}>{row.detail}</span>
                    </div>
                  ))}
                </div>
              </Disclosure>
            ) : null}
          </section>

          <section
            className="trace-workbench-call-column"
            style={{
              display: "grid",
              gridTemplateRows: "var(--tw-call-rows, minmax(140px, 40%) 1fr)",
              gap: "var(--sv-sp-3)",
              minHeight: 0
            }}
          >
            <TrajectoryRail
              view={view}
              selected={selectedCall}
              onSelect={selectCall}
              query={query}
              onQuery={setQuery}
            />
            <CallDetail view={view} step={step} />
          </section>
        </div>
      )}
    </VisualChrome>
  );
}
