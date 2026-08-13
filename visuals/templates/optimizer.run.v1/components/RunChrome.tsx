import { useState, type ReactNode } from "react";

export function RunHeader({
  algorithmId,
  status,
  objective,
  metrics
}: {
  algorithmId: string;
  status: string;
  objective?: string;
  metrics: Array<{ label: string; value: string }>;
}) {
  return (
    <section className="sv-section" aria-label="Optimizer run header" data-testid="optimizer-run-header">
      <div className="sv-section-head">
        <h3>{algorithmLabel(algorithmId)}</h3>
        <span className="sv-mono" data-testid="optimizer-status">
          {status}
        </span>
      </div>
      {objective ? <p className="sv-lede">{objective}</p> : null}
      <div className="sv-metrics" role="group" aria-label="Run metrics">
        {metrics.map((m) => (
          <div key={m.label} className="sv-metric">
            <span>{m.label}</span>
            <strong>{m.value}</strong>
          </div>
        ))}
      </div>
    </section>
  );
}

export function GlobalTimeline({
  events,
  cursorIndex,
  onScrub,
  followLive,
  terminal,
  onFollowLive
}: {
  events: Array<{ sequence: number; type: string; occurredAt: string }>;
  cursorIndex: number;
  onScrub: (index: number) => void;
  followLive: boolean;
  terminal?: boolean;
  onFollowLive: () => void;
}) {
  const max = Math.max(0, events.length - 1);
  const current = events[Math.min(cursorIndex, max)];
  return (
    <section className="sv-section" aria-label="Optimizer timeline">
      <div className="sv-section-head">
        <h3>Timeline</h3>
        <button type="button" className="sv-btn" aria-pressed={followLive} onClick={onFollowLive} data-testid="optimizer-follow-live">
          {followLive ? (terminal ? "At end of run" : "Following live") : "Return to latest"}
        </button>
      </div>
      <input
        className="sv-scrubber"
        type="range"
        min={0}
        max={max}
        value={Math.min(cursorIndex, max)}
        aria-label="Historical scrub"
        aria-valuetext={
          current
            ? `seq ${current.sequence}: ${current.type}`
            : "empty"
        }
        data-testid="optimizer-scrubber"
        onChange={(e) => onScrub(Number(e.target.value))}
      />
      <p className="sv-mono" aria-live="polite">
        {current
          ? `seq ${current.sequence} · ${current.type} · ${current.occurredAt.slice(11, 19)}`
          : "No events"}
      </p>
      <ol style={{ listStyle: "none", margin: "8px 0 0", padding: 0, maxHeight: 140, overflow: "auto" }}>
        {events.map((event, index) => (
          <li key={`${event.sequence}-${event.type}`}>
            <button
              type="button"
              className="sv-btn"
              style={{
                width: "100%",
                textAlign: "left",
                marginBottom: 4,
                opacity: index === cursorIndex ? 1 : 0.7
              }}
              onClick={() => onScrub(index)}
              data-testid={`optimizer-timeline-event-${event.sequence}`}
            >
              <span className="sv-mono">{event.sequence}</span> {event.type}
            </button>
          </li>
        ))}
      </ol>
    </section>
  );
}

export function UsageCards({ usage }: { usage: Record<string, number | null> }) {
  const tokenTotal = usage.promptTokens == null && usage.completionTokens == null
    ? null
    : (usage.promptTokens ?? 0) + (usage.completionTokens ?? 0);
  return (
    <section className="sv-section" aria-label="Usage">
      <div className="sv-section-head">
        <h3>Usage</h3>
      </div>
      <div className="sv-metrics" role="group">
        <div className="sv-metric"><span>Cost</span><strong>{usage.costUsd == null ? "—" : `$${usage.costUsd.toFixed(2)}`}</strong></div>
        <div className="sv-metric"><span>Rollouts</span><strong>{usage.rollouts ?? "—"}</strong></div>
        <div className="sv-metric"><span>Tokens</span><strong>{tokenTotal ?? "—"}</strong></div>
        <div className="sv-metric"><span>Wall</span><strong>{usage.wallTimeMs == null ? "—" : formatMs(usage.wallTimeMs)}</strong></div>
      </div>
    </section>
  );
}

export function EventLog({ entries }: { entries: Array<Record<string, unknown>> }) {
  return (
    <section className="sv-section" aria-label="Event log" aria-live="polite" data-testid="optimizer-event-log">
      <div className="sv-section-head">
        <h3>Events</h3>
        <span className="sv-mono">{entries.length}</span>
      </div>
      <ol style={{ listStyle: "none", margin: 0, padding: 0, maxHeight: 220, overflow: "auto", border: "1px solid var(--sv-border)", borderRadius: 8 }}>
        {entries.map((entry, index) => (
          <li key={`${entry.sequence}-${index}`} style={{ padding: "8px 10px", borderBottom: "1px solid var(--sv-border)", fontSize: 12 }}>
            <span className="sv-mono" style={{ color: "var(--sv-accent)", marginRight: 8 }}>{String(entry.type)}</span>
            <span className="sv-mono" style={{ color: "var(--sv-text-faint)" }}>{String(entry.occurredAt ?? "").slice(11, 19)}</span>
            {entry.message ? <div>{String(entry.message)}</div> : null}
          </li>
        ))}
      </ol>
    </section>
  );
}

function artifactRecord(value: unknown, index: number): {
  key: string;
  kind: string;
  title: string;
  filename: string;
  path?: string;
  raw: unknown;
} {
  const record = value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
  const path = typeof record.path === "string" ? record.path : typeof record.id === "string" ? record.id : undefined;
  const filename = path?.split(/[\\/]/).filter(Boolean).at(-1) ?? `artifact-${index + 1}`;
  const kind = String(record.kind ?? "artifact");
  const kindTitle: Record<string, string> = {
    chart: "Score chart",
    workspace: "Optimizer workspace",
    candidate: "Best candidate",
    manifest: "Result manifest",
    log: filename.includes("stderr") ? "Process errors" : "Process output",
    candidate_registry: "Candidate registry"
  };
  return {
    key: path ?? `${kind}-${index}`,
    kind,
    title: String(record.title ?? kindTitle[kind] ?? "Artifact"),
    filename,
    path,
    raw: value
  };
}

async function copyArtifactText(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  document.execCommand("copy");
  textarea.remove();
}

export function ArtifactList({ artifacts }: { artifacts: unknown[] }) {
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const records = [...new Map(artifacts.map(artifactRecord).map((record) => [record.key, record])).values()];
  const copyPath = async (key: string, path: string) => {
    await copyArtifactText(path);
    setCopiedKey(key);
    window.setTimeout(() => setCopiedKey((current) => current === key ? null : current), 1600);
  };
  return (
    <section className="sv-section" aria-label="Artifacts" data-testid="optimizer-artifacts">
      <div className="sv-section-head">
        <h3>Artifacts</h3>
        <span className="sv-mono">{records.length}</span>
      </div>
      {records.length === 0 ? (
        <p style={{ color: "var(--sv-text-faint)", fontSize: 12 }}>No artifacts at this cursor.</p>
      ) : (
        <div role="list" style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(240px, 1fr))", gap: 8 }}>
          {records.map((artifact, index) => (
            <article key={artifact.key} role="listitem" data-testid={`optimizer-artifact-${index}`} style={{ minWidth: 0, padding: 10, border: "1px solid var(--sv-border)", borderRadius: 8, background: "var(--sv-surface-muted)" }}>
              <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 10 }}>
                <div style={{ minWidth: 0 }}>
                  <span style={{ display: "inline-block", marginBottom: 5, padding: "2px 6px", border: "1px solid var(--sv-border)", borderRadius: 999, background: "var(--sv-surface)", color: "var(--sv-text-muted)", fontSize: 9, fontWeight: 700, letterSpacing: ".06em", textTransform: "uppercase" }}>{artifact.kind.replaceAll("_", " ")}</span>
                  <strong style={{ display: "block", fontSize: 12.5 }}>{artifact.title}</strong>
                  <div className="sv-mono" title={artifact.filename} style={{ marginTop: 3, overflow: "hidden", color: "var(--sv-text-faint)", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{artifact.filename}</div>
                </div>
                {artifact.path ? (
                  <button className="sv-btn" type="button" onClick={() => void copyPath(artifact.key, artifact.path!)} data-testid={`copy-artifact-path-${index}`} style={{ flexShrink: 0 }}>
                    {copiedKey === artifact.key ? "Copied" : "Copy path"}
                  </button>
                ) : null}
              </div>
              <details style={{ marginTop: 8 }}>
                <summary style={{ width: "fit-content", cursor: "pointer", color: "var(--sv-text-muted)", fontSize: 10.5, fontWeight: 650 }}>Details</summary>
                {artifact.path ? <code className="sv-mono" style={{ display: "block", marginTop: 6, overflowWrap: "anywhere", color: "var(--sv-text-faint)" }}>{artifact.path}</code> : <pre className="sv-mono" style={{ margin: "6px 0 0", whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{JSON.stringify(artifact.raw, null, 2)}</pre>}
              </details>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}

export function ExecutionBindings({ bindings }: { bindings: Array<Record<string, unknown>> }) {
  return (
    <section className="sv-section" aria-label="Execution bindings" data-testid="optimizer-execution-bindings">
      <div className="sv-section-head"><h3>Execution</h3><span className="sv-mono">{bindings.length}</span></div>
      {bindings.length === 0 ? (
        <p style={{ color: "var(--sv-text-faint)", fontSize: 12 }}>No execution binding reported.</p>
      ) : (
        <div role="list" style={{ display: "grid", gap: 8 }}>
          {bindings.map((binding, index) => (
            <div role="listitem" key={String(binding.id ?? index)} style={{ border: "1px solid var(--sv-border)", borderRadius: 8, padding: "8px 10px", fontSize: 12 }}>
              <strong>{String(binding.label ?? binding.kind ?? "Execution target")}</strong>
              <div className="sv-mono" style={{ color: "var(--sv-text-faint)", marginTop: 3 }}>
                {String(binding.status ?? "unknown")} · {String(binding.id ?? "—")}
              </div>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

export function CandidateRail({
  candidates,
  selectedId,
  onSelect
}: {
  candidates: Array<Record<string, unknown>>;
  selectedId?: string | null;
  onSelect?: (id: string) => void;
}): ReactNode {
  return (
    <section className="sv-section" aria-label="Candidate rail">
      <div className="sv-section-head">
        <h3>Candidates</h3>
      </div>
      <div role="list" style={{ display: "grid", gap: 8 }}>
        {candidates.map((candidate) => {
          const id = String(candidate.id ?? "");
          const active = id === selectedId;
          return (
            <button
              key={id}
              type="button"
              role="listitem"
              className="sv-btn"
              data-testid={`optimizer-candidate-${id}`}
              aria-pressed={active}
              onClick={() => onSelect?.(id)}
              aria-label={`Inspect candidate ${id}`}
              style={{ textAlign: "left", borderColor: active ? "var(--sv-accent)" : undefined }}
            >
              <strong className="sv-mono">{id}</strong>
              <div style={{ fontSize: 12, color: "var(--sv-text-muted)" }}>
                score {formatScore(candidate.score ?? candidate.train_reward)} ·{" "}
                {String(candidate.status ?? "—")}
                {candidate.parentId ? ` · parent ${String(candidate.parentId)}` : ""}
              </div>
              <span style={{ display: "block", marginTop: 5, fontSize: 10.5, fontWeight: 700 }}>Inspect candidate →</span>
            </button>
          );
        })}
      </div>
    </section>
  );
}

function algorithmLabel(id: string): string {
  if (id === "gepa") return "GEPA";
  if (id === "go-ex") return "GELO";
  if (id === "sft") return "SFT";
  return id;
}

function formatScore(value: unknown): string {
  return typeof value === "number" ? value.toFixed(2) : "—";
}

function formatMs(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
  return `${Math.floor(ms / 60_000)}m ${Math.round((ms % 60_000) / 1000)}s`;
}
