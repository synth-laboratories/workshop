/**
 * The honest empty state a template shows instead of bundled example data.
 *
 * It names the input, the binding that was supposed to fill it, and why it did
 * not — so a reviewer looking at a capture can tell "nothing was bound" from
 * "the bound source is the wrong shape" without opening the envelope.
 */

import type { UnresolvedInput } from "../runtime/resolvedInput.ts";

export function UnresolvedInputNotice({
  unresolved,
  testId
}: {
  unresolved: UnresolvedInput;
  testId?: string;
}) {
  return (
    <section
      className="sv-section"
      role="note"
      aria-label={`Input ${unresolved.input} is unavailable`}
      data-testid={testId ?? "visual-input-unresolved"}
      data-unresolved-input={unresolved.input}
    >
      <div className="sv-section-head">
        <h3>Unavailable</h3>
        <span className="sv-mono">{unresolved.input}</span>
      </div>
      <p style={{ margin: 0, fontSize: 12.5 }}>{unresolved.reason}.</p>
      <p
        className="sv-mono"
        style={{ margin: "6px 0 0", fontSize: 11, color: "var(--sv-text-muted)", overflowWrap: "anywhere" }}
      >
        {unresolved.kind ? `kind ${unresolved.kind}` : "no binding declared"}
        {unresolved.source ? ` · ${unresolved.source}` : ""}
      </p>
      <p style={{ margin: "6px 0 0", fontSize: 11, color: "var(--sv-text-faint)" }}>
        No example values are substituted here. Nothing on this pane is a measurement.
      </p>
    </section>
  );
}

export default UnresolvedInputNotice;
