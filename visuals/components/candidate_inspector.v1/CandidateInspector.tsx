import type { LiveEvalEvent } from "../../runtime/types.ts";
import { envelopeIdentity } from "../../runtime/liveStream.ts";

export type CandidateRow = {
  identity: string;
  candidateId: string;
  event: LiveEvalEvent;
};

function asId(value: unknown): string | undefined {
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : undefined;
}

/**
 * Candidate identity from optimizer_event.v1 envelopes only.
 * Does not invent CISPO/SFT candidates from metrics or clip events.
 */
export function listCandidateRows(events: LiveEvalEvent[]): CandidateRow[] {
  const rows: CandidateRow[] = [];
  const seen = new Set<string>();
  for (const [index, event] of events.entries()) {
    const label = String(event.kind ?? event.type ?? "");
    if (label !== "candidate.accepted") continue;
    const payload = event.payload ?? {};
    const candidateId = asId(payload.candidate_id) ?? asId(payload.best_candidate_id);
    if (!candidateId || seen.has(candidateId)) continue;
    seen.add(candidateId);
    rows.push({
      identity: envelopeIdentity(event, index),
      candidateId,
      event
    });
  }
  return rows;
}

export function CandidateInspector({
  events,
  cursorId,
  onSelect
}: {
  events: LiveEvalEvent[];
  cursorId: string | null;
  onSelect?: (event: LiveEvalEvent, identity: string) => void;
}) {
  const rows = listCandidateRows(events);
  return (
    <section
      className="sv-section"
      aria-label="Candidate inspector"
      data-testid="compose-candidate-inspector"
    >
      <div className="sv-section-head">
        <h3>Candidates</h3>
        <span className="sv-mono">{rows.length}</span>
      </div>
      {rows.length === 0 ? (
        <p
          data-testid="compose-candidate-inspector-empty"
          style={{ color: "var(--sv-text-faint)", margin: 0 }}
        >
          No candidate events
        </p>
      ) : (
        <ol style={{ listStyle: "none", margin: 0, padding: 0 }}>
          {rows.map((row) => {
            const selected = row.identity === cursorId;
            return (
              <li key={row.candidateId}>
                <button
                  type="button"
                  className="sv-btn"
                  data-testid={`compose-candidate-${row.candidateId}`}
                  aria-current={selected ? "true" : undefined}
                  onClick={() => onSelect?.(row.event, row.identity)}
                  style={{
                    width: "100%",
                    textAlign: "left",
                    background: selected ? "var(--sv-accent-soft)" : "transparent"
                  }}
                >
                  <span className="sv-mono">{row.candidateId}</span>
                </button>
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}
