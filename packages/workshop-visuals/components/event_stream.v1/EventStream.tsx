import type { LiveEvalEvent } from "../../runtime/types.ts";
import { envelopeIdentity, eventMatchesIncludeKinds } from "../../runtime/liveStream.ts";
import type { TransportState } from "../../runtime/replayClient.ts";

export function EventStream({
  events,
  state,
  error,
  includeKinds,
  cursorId,
  onSelect
}: {
  events: LiveEvalEvent[];
  state: TransportState;
  error: string | null;
  includeKinds?: string[];
  cursorId: string | null;
  onSelect: (event: LiveEvalEvent, identity: string) => void;
}) {
  const visible = events.filter((event) =>
    eventMatchesIncludeKinds(event, includeKinds)
  );
  const live = state === "live";
  return (
    <section className="sv-section" aria-label="Event stream" data-testid="compose-event-stream">
      <div className="sv-section-head">
        <h3>Event stream</h3>
        <span className="sv-mono">{live ? "LIVE" : state}</span>
      </div>
      {error ? (
        <p role="alert" style={{ color: "#c2553f" }}>
          {error}
        </p>
      ) : null}
      <ol
        style={{
          listStyle: "none",
          margin: 0,
          padding: 0,
          maxHeight: 320,
          overflow: "auto",
          border: "1px solid var(--sv-border)",
          borderRadius: 8
        }}
      >
        {visible.map((event, index) => {
          const identity = envelopeIdentity(event, index);
          const selected = identity === cursorId;
          return (
            <li key={identity} style={{ borderBottom: "1px solid var(--sv-border)" }}>
              <button
                type="button"
                className="sv-btn"
                data-testid={`compose-event-${identity}`}
                data-event-kind={event.kind}
                aria-current={selected ? "true" : undefined}
                onClick={() => onSelect(event, identity)}
                style={{
                  width: "100%",
                  textAlign: "left",
                  border: 0,
                  borderRadius: 0,
                  background: selected ? "var(--sv-accent-soft)" : "transparent"
                }}
              >
                <span className="sv-mono" style={{ color: "var(--sv-accent)", marginRight: 8 }}>
                  {event.kind}
                </span>
                <span className="sv-mono" style={{ color: "var(--sv-text-faint)" }}>
                  {String(event.ts ?? event.occurred_at ?? "").slice(11, 19)}
                </span>
              </button>
            </li>
          );
        })}
        {visible.length === 0 ? (
          <li style={{ padding: 12, color: "var(--sv-text-faint)" }}>Waiting for events…</li>
        ) : null}
      </ol>
    </section>
  );
}
