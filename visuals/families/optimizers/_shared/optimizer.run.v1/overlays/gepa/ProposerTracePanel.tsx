/** Standard Trace V5 presentation for GEPA proposer calls. */

import { useState } from "react";
import { TraceV5EventList, type TraceV5Item } from "../../../../../../components/TraceV5EventList.tsx";
import type { GepaProposerTrace, GepaState, GepaTruncatedText } from "../../components/projectEvents.ts";
import { generationPalette } from "./model.ts";

function text(value?: GepaTruncatedText): string | undefined {
  if (!value?.text) return undefined;
  return value.truncated ? `${value.text}\n… ${value.totalChars ?? "?"} chars total; complete value is retained in the sealed artifact.` : value.text;
}

function compact(lines: Array<string | undefined>): string | undefined {
  const present = lines.filter((line): line is string => Boolean(line?.trim()));
  return present.length ? present.join("\n\n") : undefined;
}

function traceItems(trace: GepaProposerTrace): TraceV5Item[] {
  if (trace.traceV5Items?.length) {
    const items: TraceV5Item[] = trace.traceV5Items.map((item) => ({ ...item }));
    for (const step of trace.steps ?? []) {
      if (step.kind === "candidate" && step.candidateId && !items.some((item) => item.candidateId === step.candidateId)) {
        items.push({ id: `g${trace.generation}-candidate-${step.candidateId}`, sequence: step.sequence, family: "artifact", kind: "artifact.candidate", title: step.label, occurredAt: step.at, body: step.detail, candidateId: step.candidateId });
      }
    }
    return items.sort((a, b) => a.sequence - b.sequence);
  }
  const items: TraceV5Item[] = [];
  const context = trace.steps?.find((step) => step.kind === "context");
  items.push({
    id: `g${trace.generation}-input`,
    sequence: context?.sequence ?? trace.sequence,
    family: "input",
    kind: "message.input",
    title: "Reflection evidence supplied to proposer",
    occurredAt: context?.at ?? trace.startedAt,
    body: context?.detail ?? `Parent ${trace.parentCandidateId ?? "candidate"}${trace.lossCount != null ? ` · ${trace.lossCount} failing rollouts` : ""}`
  });

  const reflection = trace.reflection;
  if (reflection) {
    items.push({
      id: `g${trace.generation}-thinking`,
      sequence: (context?.sequence ?? trace.sequence) + 0.1,
      family: "thinking",
      kind: "reasoning.summary",
      title: "Reflection and diagnosis",
      occurredAt: trace.endedAt,
      body: compact([
        text(reflection.critique) ? `Critique\n${text(reflection.critique)}` : undefined,
        reflection.failurePatterns.length ? `Failure patterns\n${reflection.failurePatterns.map((value) => `• ${text(value)}`).join("\n")}` : undefined,
        reflection.winningPatterns.length ? `Winning patterns\n${reflection.winningPatterns.map((value) => `• ${text(value)}`).join("\n")}` : undefined,
        text(reflection.candidateComparison) ? `Candidate comparison\n${text(reflection.candidateComparison)}` : undefined,
        text(reflection.rationale) ? `Rationale\n${text(reflection.rationale)}` : undefined
      ])
    });
    reflection.proposals.forEach((proposal, index) => {
      const candidateId = trace.candidateIds?.[index];
      items.push({
        id: `g${trace.generation}-proposal-${index}`,
        sequence: (context?.sequence ?? trace.sequence) + 0.2 + index / 100,
        family: "output",
        kind: "message.output",
        title: `Proposal ${index + 1}${proposal.proposalType ? ` · ${proposal.proposalType.replaceAll("_", " ")}` : ""}`,
        occurredAt: trace.endedAt,
        body: text(proposal.rationale),
        detail: text(proposal.proposedPayload),
        candidateId
      });
    });
  } else {
    items.push({
      id: `g${trace.generation}-thinking-status`,
      sequence: (context?.sequence ?? trace.sequence) + 0.1,
      family: "thinking",
      kind: "reasoning.status",
      title: trace.status === "running" ? "Proposer is reflecting" : "Structured reflection unavailable",
      occurredAt: trace.startedAt,
      body: trace.status === "running"
        ? "Reasoning is streaming into the recorded trace. The structured summary replaces raw token deltas when the call seals."
        : "The call completed without a structured reflection projection. Raw transport text is intentionally not dumped into this view."
    });
  }

  for (const step of trace.steps ?? []) {
    if (step.kind === "candidate" && step.candidateId && !items.some((item) => item.candidateId === step.candidateId)) {
      items.push({ id: `g${trace.generation}-candidate-${step.candidateId}`, sequence: step.sequence, family: "artifact", kind: "artifact.candidate", title: step.label, occurredAt: step.at, body: step.detail, candidateId: step.candidateId });
    }
  }
  items.push({
    id: `g${trace.generation}-status`,
    sequence: Math.max(trace.sequence, ...(trace.steps ?? []).map((step) => step.sequence)) + 1,
    family: "system",
    kind: trace.status === "running" ? "span.open" : "span.closed",
    title: trace.status === "running" ? "Proposer call in progress" : `Proposer call ${trace.status}`,
    occurredAt: trace.endedAt ?? trace.startedAt,
    detail: compact([
      trace.model ? `model: ${trace.model}` : undefined,
      trace.wallSeconds != null ? `duration: ${trace.wallSeconds.toFixed(1)}s` : undefined,
      typeof trace.usage?.total_tokens === "number" ? `tokens: ${(trace.usage.total_tokens as number).toLocaleString()}` : undefined,
      trace.workspace ? `workspace: ${trace.workspace}` : undefined
    ]),
    status: trace.status
  });
  return items.sort((a, b) => a.sequence - b.sequence);
}

export function ProposerTracePanel({ gepa, onSelectCandidate, selectedItemId, onSelectItem }: { gepa: GepaState; onSelectCandidate?: (id: string) => void; selectedItemId?: string | null; onSelectItem?: (item: TraceV5Item, generation: number) => void }) {
  const [selectedGeneration, setSelectedGeneration] = useState<number | null>(null);
  const orderedTraces = [...gepa.proposerTraces].sort((a, b) => a.generation - b.generation);
  const selectedTrace = orderedTraces.find((trace) => trace.generation === selectedGeneration) ?? orderedTraces.at(-1);
  const selectedIndex = Math.max(0, orderedTraces.findIndex((trace) => trace.generation === selectedTrace?.generation));
  return (
    <section className="sv-section" aria-label="Proposer Trace V5" data-testid="gepa-proposer-traces">
      <div className="sv-section-head">
        <h3>Proposer trace</h3>
        <span className="sv-mono">Trace V5 · {gepa.proposerTraces.length} call{gepa.proposerTraces.length === 1 ? "" : "s"}</span>
      </div>
      {orderedTraces.length === 0 ? <p style={{ color: "var(--sv-text-faint)", fontSize: 12, margin: 0 }}>The proposer trace opens after seed evaluation.</p> : (
        <div style={{ display: "grid", gap: 10 }}>
          <div role="tablist" aria-label="Proposer generations" style={{ display: "flex", gap: 6, overflowX: "auto", paddingBottom: 2 }}>
            {orderedTraces.map((trace) => {
              const selected = trace.generation === selectedTrace?.generation;
              const toolCount = trace.traceV5Items?.filter((item) => item.family === "tool").length;
              const palette = generationPalette(trace.generation);
              return (
                <button
                  key={trace.generation}
                  type="button"
                  role="tab"
                  aria-selected={selected}
                  aria-controls={`proposer-generation-panel-${trace.generation}`}
                  className="sv-btn"
                  data-testid={`proposer-generation-tab-${trace.generation}`}
                  onClick={() => setSelectedGeneration(trace.generation)}
                  style={{ flex: "0 0 auto", padding: "8px 11px", borderColor: selected ? palette.color : "var(--sv-border)", borderLeft: `4px solid ${palette.color}`, background: selected ? palette.tint : "var(--sv-surface)" }}
                >
                  <strong>Generation {trace.generation}</strong>
                  <span className="sv-mono" style={{ marginLeft: 7, color: "var(--sv-text-muted)", fontSize: 9 }}>{trace.status}{toolCount != null ? ` · ${toolCount} tools` : ""}</span>
                </button>
              );
            })}
          </div>
          {orderedTraces.length > 1 ? (
            <div style={{ display: "grid", gridTemplateColumns: "auto minmax(120px, 1fr) auto", gap: 9, alignItems: "center", padding: "0 5px" }}>
              <span className="sv-mono" style={{ color: "var(--sv-text-faint)", fontSize: 9 }}>Gen {orderedTraces[0].generation}</span>
              <input
                type="range"
                aria-label="Scrub proposer generations"
                min={0}
                max={orderedTraces.length - 1}
                step={1}
                value={selectedIndex}
                onChange={(event) => setSelectedGeneration(orderedTraces[Number(event.currentTarget.value)]?.generation ?? null)}
                style={{ width: "100%", accentColor: "var(--sv-accent)" }}
              />
              <span className="sv-mono" style={{ color: "var(--sv-text-faint)", fontSize: 9 }}>Gen {orderedTraces.at(-1)?.generation}</span>
            </div>
          ) : null}
          {selectedTrace ? (
            <article id={`proposer-generation-panel-${selectedTrace.generation}`} role="tabpanel" aria-label={`Generation ${selectedTrace.generation} trace`} data-testid={`inspect-proposer-trace-${selectedTrace.generation}`} style={{ border: "1px solid var(--sv-border)", borderRadius: 10, padding: 12 }}>
              <div style={{ display: "flex", flexWrap: "wrap", gap: 8, alignItems: "baseline", marginBottom: 10 }}>
                <strong>Generation {selectedTrace.generation}</strong>
                <span className="sv-mono" style={{ color: "var(--sv-text-muted)", fontSize: 10.5 }}>{selectedTrace.model ?? "proposer"} · {selectedTrace.status}{selectedTrace.wallSeconds != null ? ` · ${selectedTrace.wallSeconds.toFixed(1)}s` : ""}</span>
              </div>
              <TraceV5EventList key={selectedTrace.generation} items={traceItems(selectedTrace)} onSelectCandidate={onSelectCandidate} selectedItemId={selectedItemId} onSelectItem={onSelectItem ? (item) => onSelectItem(item, selectedTrace.generation) : undefined} defaultView="full" emptyToolText="0 structured tool calls captured. This proposer transport currently preserves their effects and artifacts, but not tool-call envelopes; nothing is inferred or fabricated." />
            </article>
          ) : null}
        </div>
      )}
    </section>
  );
}
