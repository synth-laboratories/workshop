import type { LiveEvalEvent } from "../../runtime/types.ts";
import { envelopeIdentity } from "../../runtime/liveStream.ts";

function numericSequence(event: LiveEvalEvent): number | undefined {
  const raw = event.sequence;
  if (typeof raw === "number" && Number.isSafeInteger(raw)) return raw;
  if (typeof raw === "string" && raw.length > 0 && Number.isSafeInteger(Number(raw))) {
    return Number(raw);
  }
  return undefined;
}

export function Scrubber({
  events,
  cursorId,
  onSelect
}: {
  events: LiveEvalEvent[];
  cursorId: string | null;
  onSelect: (event: LiveEvalEvent, identity: string, sequence: number | string | null) => void;
}) {
  const sequenced = events
    .map((event, index) => {
      const sequence = numericSequence(event);
      if (sequence == null) return null;
      return { event, identity: envelopeIdentity(event, index), sequence };
    })
    .filter((row): row is { event: LiveEvalEvent; identity: string; sequence: number } => row != null)
    .sort((left, right) => left.sequence - right.sequence);
  const min = sequenced[0]?.sequence;
  const max = sequenced[sequenced.length - 1]?.sequence;
  const selected = sequenced.find((row) => row.identity === cursorId) ?? sequenced[sequenced.length - 1];
  const value = selected?.sequence ?? min ?? 0;

  return (
    <section className="sv-section" aria-label="Sequence scrubber" data-testid="compose-scrubber">
      <div className="sv-section-head">
        <h3>Scrubber</h3>
        <span className="sv-mono" data-testid="compose-scrubber-sequence">
          {selected ? String(selected.sequence) : "—"}
        </span>
      </div>
      {sequenced.length === 0 ? (
        <p style={{ color: "var(--sv-text-faint)", margin: 0 }}>No sequenced events</p>
      ) : (
        <input
          type="range"
          min={min}
          max={max}
          step={1}
          value={value}
          aria-label="Event sequence"
          data-testid="compose-scrubber-slider"
          onChange={(event) => {
            const sequence = Number(event.target.value);
            const hit = sequenced.find((row) => row.sequence === sequence)
              ?? sequenced.reduce((best, row) =>
                Math.abs(row.sequence - sequence) < Math.abs(best.sequence - sequence) ? row : best
              );
            onSelect(hit.event, hit.identity, hit.sequence);
          }}
        />
      )}
    </section>
  );
}
