/**
 * Candidate list plus inspector: lineage, gate decision in plain language,
 * stage-by-stage scores, and a semantic parent-vs-candidate prompt diff.
 * Selection is neutral; orange/red is reserved for status semantics.
 */

import { useState } from "react";
import { formatMissingNumber } from "../../../../../../runtime/liveStream.ts";
import type { GepaState } from "../../components/projectEvents.ts";
import {
  candidateName,
  candidatePalette,
  candidateValues,
  decisionText,
  metricsByCandidate,
  shortId,
  statusLabel,
  statusTone,
  type CandidateRecord
} from "./model.ts";
import { wordDiff } from "./wordDiff.ts";

function scoreOf(candidate: CandidateRecord | undefined, key: string): number | undefined {
  const value = candidate?.[key];
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
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

function safeFilePart(value: string): string {
  return value.replace(/[^a-zA-Z0-9._-]+/g, "_").slice(0, 100) || "candidate";
}

export function CandidateList({
  gepa,
  candidates,
  selectedId,
  onSelect
}: {
  gepa: GepaState;
  candidates?: CandidateRecord[];
  selectedId?: string | null;
  onSelect?: (id: string) => void;
}) {
  const ordered = candidates ?? [...gepa.candidates].sort((a, b) => Number(a.sequence ?? 0) - Number(b.sequence ?? 0));
  return (
    <section className="sv-section" aria-label="Candidates" style={{ marginTop: 14 }}>
      <div className="sv-section-head">
        <h3>Candidates</h3>
        <span className="sv-mono">{ordered.length}</span>
      </div>
      <div role="list" style={{ display: "grid", gap: 6 }}>
        {ordered.length === 0 ? (
          <p style={{ color: "var(--sv-text-faint)", fontSize: 12, margin: 0 }}>The seed candidate registers when the run starts.</p>
        ) : ordered.map((candidate) => {
          const id = String(candidate.id ?? "");
          const selected = id === selectedId;
          const train = scoreOf(candidate, "train_reward");
          const minibatch = scoreOf(candidate, "minibatchReward");
          const parentMinibatch = scoreOf(candidate, "parentMinibatchReward");
          const heldoutScore = scoreOf(candidate, "heldout_reward");
          const delta = minibatch != null && parentMinibatch != null ? minibatch - parentMinibatch : undefined;
          const decisionGate = (candidate.decision as { gate?: string } | undefined)?.gate;
          const decision = decisionText(candidate);
          const palette = candidatePalette(candidate);
          const scoreBits = [
            train != null ? `train ${train.toFixed(2)}` : null,
            minibatch != null ? `minibatch diagnostic ${minibatch.toFixed(2)}${parentMinibatch != null ? ` vs parent ${parentMinibatch.toFixed(2)}` : ""}` : null,
            heldoutScore != null ? `heldout ${heldoutScore.toFixed(2)}` : null
          ].filter(Boolean);
          return (
            <div key={id} role="listitem">
            <button
              type="button"
              className="sv-candidate-card"
              data-selected={selected}
              data-testid={`optimizer-candidate-${id}`}
              data-annotation-kind="candidate"
              data-annotation-id={id}
              aria-pressed={selected}
              aria-label={`Inspect candidate ${candidateName(candidate)}`}
              onClick={() => onSelect?.(id)}
              style={{ borderLeft: `4px solid ${palette.color}`, background: selected ? palette.tint : undefined }}
            >
              <span style={{ display: "flex", flexWrap: "wrap", alignItems: "baseline", gap: 8 }}>
                <strong style={{ fontSize: 12.5 }}>{candidateName(candidate)}</strong>
                <span className="sv-mono" style={{ color: "var(--sv-text-faint)" }}>{shortId(id)}</span>
                <span className="sv-chip" data-tone={statusTone(candidate.status)} style={{ marginLeft: "auto" }}>
                  {statusLabel(candidate.status)}
                </span>
              </span>
              <span style={{ display: "block", marginTop: 4, fontSize: 11.5, color: "var(--sv-text-muted)" }}>
                {scoreBits.length > 0 ? scoreBits.join(" · ") : "no scores yet"}
                {delta != null && decisionGate !== "full_train" ? (
                  <span style={{ marginLeft: 6, fontWeight: 700, color: delta > 0 ? "#1e7a43" : delta < 0 ? "#b23830" : "var(--sv-text-muted)" }}>
                    Δ {delta >= 0 ? "+" : ""}{delta.toFixed(2)}
                  </span>
                ) : null}
              </span>
              {candidate.status === "evaluating" && candidate.stage ? (
                <span style={{ display: "block", marginTop: 2, fontSize: 11, color: "var(--sv-accent)" }}>
                  evaluating · {String(candidate.stage).replaceAll("_", " ")}
                </span>
              ) : null}
              {candidate.status === "aborted" ? (
                <span style={{ display: "block", marginTop: 2, fontSize: 11, color: "var(--sv-text-faint)" }}>run ended before this candidate received a complete score</span>
              ) : null}
              {decision ? (
                <span style={{ display: "block", marginTop: 2, fontSize: 11, color: "var(--sv-text-muted)" }}>{decision}</span>
              ) : null}
            </button>
            </div>
          );
        })}
      </div>
    </section>
  );
}

function LeverDiff({ lever, before, after }: { lever: string; before?: string; after: string }) {
  const changed = before != null && before !== after;
  return (
    <article style={{ marginTop: 8, overflow: "hidden", border: "1px solid var(--sv-border)", borderRadius: 8 }}>
      <div className="sv-mono" style={{ display: "flex", justifyContent: "space-between", padding: "6px 10px", borderBottom: "1px solid var(--sv-border)", background: "var(--sv-surface-muted)", color: "var(--sv-text-muted)" }}>
        <span>{lever}</span>
        <span>{before == null ? "new lever" : changed ? "changed" : "unchanged"}</span>
      </div>
      <pre className="sv-diff" style={{ margin: 0, padding: 10, whiteSpace: "pre-wrap", overflowWrap: "anywhere", font: "12px/1.6 var(--sv-mono)", color: "var(--sv-text)", maxHeight: 320, overflow: "auto" }}>
        {changed
          ? wordDiff(before!, after).map((segment, index) =>
              segment.type === "same"
                ? <span key={index}>{segment.text}</span>
                : segment.type === "add"
                  ? <ins key={index}>{segment.text}</ins>
                  : <del key={index}>{segment.text}</del>
            )
          : after}
      </pre>
    </article>
  );
}

export function CandidateInspector({
  gepa,
  selectedId,
  onSelect,
  onShowTrace
}: {
  gepa: GepaState;
  selectedId?: string | null;
  onSelect?: (id: string) => void;
  onShowTrace?: (generation: number) => void;
}) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");
  const selected = gepa.candidates.find((candidate) => String(candidate.id) === selectedId);
  if (!selected) {
    return (
      <section className="sv-section" aria-label="Candidate inspector" data-testid="gepa-selected-candidate" style={{ marginTop: 0 }}>
        <div className="sv-section-head"><h3>Candidate</h3></div>
        <p style={{ color: "var(--sv-text-faint)", fontSize: 12, margin: 0 }}>
          Select a candidate from the chart or the list to inspect its prompt, lineage, scores, and gate decision.
        </p>
      </section>
    );
  }
  const id = String(selected.id);
  const parentId = selected.parentId == null ? undefined : String(selected.parentId);
  const parent = parentId ? gepa.candidates.find((candidate) => String(candidate.id) === parentId) : undefined;
  const values = candidateValues(selected);
  const parentValues = parent ? candidateValues(parent) : {};
  const valueEntries = Object.entries(values);
  const changedLevers = valueEntries
    .filter(([lever, text]) => parent && parentValues[lever] !== undefined && parentValues[lever] !== text)
    .map(([lever]) => lever);
  const metrics = metricsByCandidate(gepa.evaluations).get(id);
  const decision = decisionText(selected);
  const decisionOutcome = (selected.decision as { outcome?: string } | undefined)?.outcome;
  const generation = typeof selected.generation === "number" ? selected.generation : undefined;
  const relatedTrace = generation != null ? gepa.proposerTraces.find((trace) => trace.generation === generation) : undefined;
  const palette = candidatePalette(selected);

  const stageRows = [
    { label: "Minibatch", candidate: scoreOf(selected, "minibatchReward"), parent: scoreOf(selected, "parentMinibatchReward") },
    { label: "Full train", candidate: scoreOf(selected, "train_reward"), parent: scoreOf(parent, "train_reward") },
    { label: "Heldout", candidate: scoreOf(selected, "heldout_reward"), parent: parentId ? scoreOf(parent, "heldout_reward") : undefined }
  ].filter((row) => row.candidate != null || row.parent != null);

  const candidateExport = {
    schemaVersion: "optimizer_candidate.v1",
    algorithmId: "gepa",
    id,
    status: selected.status ?? null,
    score: selected.score ?? selected.train_reward ?? null,
    heldoutScore: selected.heldout_reward ?? null,
    parentId: parentId ?? null,
    values
  };
  const serialized = JSON.stringify(candidateExport, null, 2);
  const copyCandidate = async () => {
    if (valueEntries.length === 0) return;
    const text = valueEntries.length === 1 ? valueEntries[0][1] : serialized;
    try {
      await copyText(text);
      setCopyState("copied");
      window.setTimeout(() => setCopyState("idle"), 1600);
    } catch {
      setCopyState("failed");
    }
  };
  const downloadCandidate = () => {
    const url = URL.createObjectURL(new Blob([serialized], { type: "application/json" }));
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `${safeFilePart(id)}.json`;
    anchor.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 0);
  };

  return (
    <section className="sv-section" aria-label="Candidate inspector" data-testid="gepa-selected-candidate" style={{ marginTop: 0 }}>
      <div className="sv-section-head">
        <h3 style={{ display: "flex", alignItems: "center", gap: 7 }}><span aria-hidden style={{ width: 9, height: 9, borderRadius: "50%", background: palette.color }} />{candidateName(selected)}</h3>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <span className="sv-chip" data-tone={statusTone(selected.status)}>{statusLabel(selected.status)}</span>
          <button className="sv-btn" type="button" disabled={valueEntries.length === 0} onClick={() => void copyCandidate()} data-testid="copy-gepa-candidate">
            {copyState === "copied" ? "Copied" : copyState === "failed" ? "Copy failed" : valueEntries.length === 1 ? "Copy prompt" : "Copy JSON"}
          </button>
          <button className="sv-btn" type="button" disabled={valueEntries.length === 0} onClick={downloadCandidate} data-testid="download-gepa-candidate">
            Download JSON
          </button>
        </div>
      </div>
      {decision ? (
        <p
          data-testid="gepa-candidate-decision"
          style={{
            margin: "0 0 10px",
            padding: "7px 10px",
            borderRadius: 8,
            fontSize: 12,
            border: `1px solid ${decisionOutcome === "accepted" ? "#b7dcc4" : decisionOutcome === "rejected" ? "#ecc4c0" : "var(--sv-border)"}`,
            background: decisionOutcome === "accepted" ? "#ecf7f0" : decisionOutcome === "rejected" ? "#fbefee" : "var(--sv-surface-muted)",
            color: "var(--sv-text)"
          }}
        >
          {decision}
        </p>
      ) : null}
      <dl style={{ display: "grid", gridTemplateColumns: "auto 1fr", gap: "4px 12px", margin: 0, fontSize: 12 }}>
        <dt style={{ color: "var(--sv-text-faint)" }}>ID</dt>
        <dd className="sv-mono" style={{ margin: 0, overflowWrap: "anywhere" }}>{id}</dd>
        <dt style={{ color: "var(--sv-text-faint)" }}>Lineage</dt>
        <dd style={{ margin: 0 }}>
          {parent ? (
            <button type="button" className="sv-btn" style={{ padding: "1px 7px", fontSize: 11 }} onClick={() => onSelect?.(String(parent.id))}>
              {candidateName(parent)}
            </button>
          ) : "seed — no parent"}
          {parent ? " → this candidate" : ""}
        </dd>
        {generation != null ? (
          <>
            <dt style={{ color: "var(--sv-text-faint)" }}>Generation</dt>
            <dd style={{ margin: 0 }}>
              {generation}
              {relatedTrace ? (
                <button type="button" className="sv-btn" style={{ marginLeft: 8, padding: "1px 7px", fontSize: 11 }} onClick={() => onShowTrace?.(generation)}>
                  View proposer trace →
                </button>
              ) : null}
            </dd>
          </>
        ) : null}
        {changedLevers.length > 0 ? (
          <>
            <dt style={{ color: "var(--sv-text-faint)" }}>Changed levers</dt>
            <dd className="sv-mono" style={{ margin: 0 }}>{changedLevers.join(", ")}</dd>
          </>
        ) : null}
      </dl>
      {stageRows.length > 0 ? (
        <table className="sv-table" style={{ marginTop: 10 }} data-testid="gepa-candidate-scores">
          <thead>
            <tr>
              <th scope="col">Stage</th>
              <th scope="col">This candidate</th>
              <th scope="col">Parent</th>
              <th scope="col">Δ</th>
            </tr>
          </thead>
          <tbody>
            {stageRows.map((row) => {
              const delta = row.candidate != null && row.parent != null ? row.candidate - row.parent : undefined;
              return (
                <tr key={row.label}>
                  <td>{row.label}</td>
                  <td className="sv-mono">{formatMissingNumber(row.candidate)}</td>
                  <td className="sv-mono">{formatMissingNumber(row.parent)}</td>
                  <td className="sv-mono" style={{ color: delta == null ? undefined : delta > 0 ? "#1e7a43" : delta < 0 ? "#b23830" : undefined }}>
                    {delta == null ? "—" : `${delta >= 0 ? "+" : ""}${delta.toFixed(2)}`}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      ) : null}
      {metrics ? (
        <p style={{ margin: "8px 0 0", fontSize: 11.5, color: "var(--sv-text-muted)" }}>
          {[...metrics.values()].map((stage) =>
            `${stage.stage.replaceAll("_", " ")}: ${stage.completed}/${stage.total} rollouts${stage.mean != null ? `, mean ${stage.mean.toFixed(2)}` : ""}`
          ).join(" · ")}
        </p>
      ) : null}
      <div style={{ marginTop: 12 }} data-testid="gepa-candidate-content">
        <div className="sv-section-head" style={{ marginBottom: 6 }}>
          <h3>Prompt {parent ? "diff vs parent" : "content"}</h3>
          <span>{valueEntries.length} {valueEntries.length === 1 ? "lever" : "levers"}</span>
        </div>
        {valueEntries.length === 0 ? (
          <p style={{ margin: 0, color: "var(--sv-text-faint)", fontSize: 12 }}>
            Candidate content was not persisted for this run. Refresh the run to reconcile local artifacts.
          </p>
        ) : valueEntries.map(([lever, text]) => (
          <LeverDiff key={lever} lever={lever} before={parent ? parentValues[lever] : undefined} after={text} />
        ))}
      </div>
    </section>
  );
}
