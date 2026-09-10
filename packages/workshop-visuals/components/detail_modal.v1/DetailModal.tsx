import type { LiveEvalEvent } from "../../runtime/types.ts";

export function DetailModal({
  event,
  onClose
}: {
  event: LiveEvalEvent | null;
  onClose: () => void;
}) {
  if (!event) return null;
  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Event detail"
      data-testid="compose-detail-modal"
      style={{
        marginTop: 12,
        padding: 12,
        border: "1px solid var(--sv-border)",
        borderRadius: 8,
        background: "var(--sv-surface-muted, #f6f7f9)"
      }}
    >
      <div className="sv-section-head">
        <h3>{event.kind}</h3>
        <button type="button" className="sv-btn" onClick={onClose} data-testid="compose-detail-close">
          Close
        </button>
      </div>
      <p className="sv-mono" style={{ color: "var(--sv-text-faint)", margin: "0 0 8px" }}>
        {String(event.ts ?? event.occurred_at ?? "")}
      </p>
      <pre
        data-testid="compose-detail-payload"
        style={{ margin: 0, fontSize: 11, whiteSpace: "pre-wrap", wordBreak: "break-word" }}
      >
        {JSON.stringify(event.payload ?? {}, null, 2)}
      </pre>
    </div>
  );
}
