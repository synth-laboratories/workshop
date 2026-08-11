import { useState } from "react";
import { CandidateRail } from "../components/RunChrome.tsx";
import type { ProjectedState } from "../components/projectEvents.ts";

function candidateValues(candidate: Record<string, unknown>): Record<string, unknown> {
  for (const key of ["values", "payload"]) {
    const value = candidate[key];
    if (value && typeof value === "object" && !Array.isArray(value)) return value as Record<string, unknown>;
  }
  const common = ["prompt", "instruction", "systemPrompt", "system_prompt", "content", "text"];
  return Object.fromEntries(common.filter((key) => typeof candidate[key] === "string").map((key) => [key, candidate[key]]));
}

function safeFilePart(value: string): string {
  return value.replace(/[^a-zA-Z0-9._-]+/g, "_").slice(0, 100) || "candidate";
}

async function copyText(text: string): Promise<void> {
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

export function GepaOverlay({
  state,
  selectedId,
  onSelect
}: {
  state: ProjectedState;
  selectedId?: string | null;
  onSelect?: (id: string) => void;
}) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");
  const gepa = state.gepa;
  if (!gepa) return null;
  const cells = gepa.frontier;
  const selected = gepa.candidates.find((candidate) => String(candidate.id) === selectedId);
  const values = selected ? candidateValues(selected) : {};
  const valueEntries = Object.entries(values);
  const candidateExport = selected ? {
    schemaVersion: "optimizer_candidate.v1",
    algorithmId: "gepa",
    id: String(selected.id),
    status: selected.status ?? null,
    score: selected.score ?? selected.train_reward ?? null,
    heldoutScore: selected.heldout_reward ?? null,
    parentId: selected.parentId ?? null,
    values
  } : null;
  const serializedCandidate = candidateExport ? JSON.stringify(candidateExport, null, 2) : "";
  const copyCandidate = async () => {
    if (!selected || valueEntries.length === 0) return;
    const text = valueEntries.length === 1 && typeof valueEntries[0][1] === "string"
      ? valueEntries[0][1]
      : serializedCandidate;
    try {
      await copyText(text);
      setCopyState("copied");
      window.setTimeout(() => setCopyState("idle"), 1600);
    } catch {
      setCopyState("failed");
    }
  };
  const downloadCandidate = () => {
    if (!selected || !serializedCandidate) return;
    const url = URL.createObjectURL(new Blob([serializedCandidate], { type: "application/json" }));
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `${safeFilePart(String(selected.id))}.json`;
    anchor.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 0);
  };
  const maxQ = Math.max(1, ...cells.map((c) => Number(c.quality ?? 0)));
  const maxC = Math.max(0.01, ...cells.map((c) => Number(c.costUsd ?? 0)));

  return (
    <>
      <section className="sv-section" aria-label="Pareto frontier" data-testid="gepa-pareto-frontier">
        <div className="sv-section-head">
          <h3>Pareto frontier</h3>
        </div>
        <div
          role="img"
          aria-label="Quality versus cost frontier"
          style={{ border: "1px solid var(--sv-border)", borderRadius: 8, padding: 8 }}
        >
          <svg viewBox="0 0 320 180" width="100%" style={{ maxHeight: 200 }}>
            {cells.map((cell) => {
              const x = 40 + (Number(cell.costUsd ?? 0) / maxC) * 240;
              const y = 150 - (Number(cell.quality ?? 0) / maxQ) * 120;
              return (
                <g key={String(cell.candidateId)}>
                  <circle
                    cx={x}
                    cy={y}
                    r={cell.accent ? 8 : 6}
                    fill={cell.accent ? "#f05f22" : "#5c6573"}
                  />
                  <text x={x} y={y - 10} textAnchor="middle" fontSize="9" fill="#5c6573">
                    {String(cell.candidateId)}
                  </text>
                </g>
              );
            })}
            <text x="160" y="172" textAnchor="middle" fontSize="10" fill="#8b93a1">
              cost (USD)
            </text>
          </svg>
          {cells.length === 0 ? <p style={{ color: "var(--sv-text-faint)", fontSize: 12 }}>Frontier will appear after the first candidate evaluation.</p> : null}
          <div role="list" aria-label="Pareto frontier candidates" style={{ display: "grid", gap: 4 }}>
            {cells.map((cell) => {
              const id = String(cell.candidateId);
              return (
                <button key={id} type="button" role="listitem" className="sv-btn sv-mono" onClick={() => onSelect?.(id)} data-testid={`gepa-frontier-${id}`} style={{ textAlign: "left", borderColor: id === selectedId ? "var(--sv-accent)" : undefined }}>
                  {id} · quality {Number(cell.quality ?? 0).toFixed(2)} · ${Number(cell.costUsd ?? 0).toFixed(2)}
                </button>
              );
            })}
          </div>
        </div>
      </section>

      <CandidateRail
        candidates={gepa.candidates}
        selectedId={selectedId}
        onSelect={onSelect}
      />

      {selected ? (
        <section className="sv-section" aria-label="Selected candidate" data-testid="gepa-selected-candidate">
          <div className="sv-section-head">
            <h3>Candidate · {String(selected.id)}</h3>
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <span className="sv-mono">{String(selected.status ?? "unknown")}</span>
              <button className="sv-btn" type="button" disabled={valueEntries.length === 0} onClick={() => void copyCandidate()} data-testid="copy-gepa-candidate">
                {copyState === "copied" ? "Copied" : copyState === "failed" ? "Copy failed" : valueEntries.length === 1 ? "Copy prompt" : "Copy JSON"}
              </button>
              <button className="sv-btn" type="button" disabled={valueEntries.length === 0} onClick={downloadCandidate} data-testid="download-gepa-candidate">Download JSON</button>
            </div>
          </div>
          <dl style={{ display: "grid", gridTemplateColumns: "auto 1fr", gap: "6px 12px", margin: 0, fontSize: 12 }}>
            <dt style={{ color: "var(--sv-text-faint)" }}>Score</dt><dd style={{ margin: 0 }}>{typeof (selected.score ?? selected.train_reward) === "number" ? Number(selected.score ?? selected.train_reward).toFixed(3) : "—"}</dd>
            <dt style={{ color: "var(--sv-text-faint)" }}>Parent</dt><dd className="sv-mono" style={{ margin: 0 }}>{String(selected.parentId ?? "seed")}</dd>
            <dt style={{ color: "var(--sv-text-faint)" }}>Sequence</dt><dd className="sv-mono" style={{ margin: 0 }}>{String(selected.sequence ?? "—")}</dd>
          </dl>
          <div style={{ marginTop: 14 }} data-testid="gepa-candidate-content">
            <div className="sv-section-head"><h3>Candidate content</h3><span>{valueEntries.length} {valueEntries.length === 1 ? "lever" : "levers"}</span></div>
            {valueEntries.length === 0 ? (
              <p style={{ margin: 0, color: "var(--sv-text-faint)", fontSize: 12 }}>Candidate content was not persisted for this run. Refresh the run to reconcile local artifacts.</p>
            ) : valueEntries.map(([key, value]) => (
              <article key={key} style={{ marginTop: 8, overflow: "hidden", border: "1px solid var(--sv-border)", borderRadius: 8, background: "var(--sv-surface-muted)" }}>
                <div className="sv-mono" style={{ padding: "7px 10px", borderBottom: "1px solid var(--sv-border)", color: "var(--sv-text-muted)" }}>{key}</div>
                <pre style={{ margin: 0, padding: 10, overflow: "auto", whiteSpace: "pre-wrap", overflowWrap: "anywhere", font: "12px/1.55 var(--sv-mono)", color: "var(--sv-text)" }}>{typeof value === "string" ? value : JSON.stringify(value, null, 2)}</pre>
              </article>
            ))}
          </div>
        </section>
      ) : null}

      <section className="sv-section" aria-label="Reflections">
        <div className="sv-section-head">
          <h3>Reflections</h3>
        </div>
        {gepa.reflections.length === 0 ? (
          <p style={{ color: "var(--sv-text-faint)", fontSize: 12 }}>No reflections yet.</p>
        ) : (
          <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12 }}>
            {gepa.reflections.map((entry) => (
              <li key={String(entry.sequence)}>{String(entry.message ?? JSON.stringify(entry))}</li>
            ))}
          </ul>
        )}
      </section>
    </>
  );
}
