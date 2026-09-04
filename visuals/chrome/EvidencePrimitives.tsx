import type { ReactNode } from "react";

export type EvidenceState = "live" | "terminal" | "partial" | "stale" | "unavailable";

const STATE_COPY: Record<EvidenceState, string> = {
  live: "Live evidence",
  terminal: "Complete evidence",
  partial: "Partial evidence",
  stale: "Stale evidence",
  unavailable: "Evidence unavailable"
};

export function EvidenceStateBanner({
  state,
  children
}: {
  state: EvidenceState;
  children?: ReactNode;
}) {
  return (
    <aside
      data-evidence-state={state}
      role={state === "stale" || state === "unavailable" ? "alert" : "status"}
      style={{
        border: "1px solid var(--sv-border)",
        borderLeft: `4px solid ${state === "stale" || state === "unavailable" ? "#c2553f" : "var(--sv-accent)"}`,
        borderRadius: 8,
        padding: "9px 12px",
        marginBottom: 14,
        background: "var(--sv-surface-muted, #f7f7f5)",
        fontSize: 12
      }}
    >
      <strong>{STATE_COPY[state]}</strong>{children ? <span> · {children}</span> : null}
    </aside>
  );
}

export function MetricValue({
  label,
  value,
  qualifier
}: {
  label: string;
  value: ReactNode | null | undefined;
  qualifier?: string;
}) {
  const missing = value === null || value === undefined || value === "";
  return (
    <div data-evidence-missing={missing ? "true" : "false"} style={{ minWidth: 112 }}>
      <div className="sv-label">{label}</div>
      <div className="sv-mono" style={{ marginTop: 4, fontSize: 15 }}>{missing ? "—" : value}</div>
      {qualifier ? <div style={{ marginTop: 3, color: "var(--sv-text-faint)", fontSize: 10 }}>{qualifier}</div> : null}
    </div>
  );
}

export function ComparisonScopeNotice({
  comparable,
  contract
}: {
  comparable: boolean;
  contract?: string;
}) {
  return (
    <EvidenceStateBanner state={comparable ? "terminal" : "partial"}>
      {comparable
        ? `Rows share evaluation contract ${contract ?? "(unnamed)"}; ranking is valid.`
        : "Rows use different or unverified evaluation contracts; values are descriptive and are not ranked."}
    </EvidenceStateBanner>
  );
}

export function ProvenanceHeader({ items }: { items: Array<{ label: string; value?: string | null }> }) {
  const visible = items.filter((item) => item.value);
  if (!visible.length) return null;
  return (
    <dl aria-label="Evidence provenance" style={{ display: "flex", flexWrap: "wrap", gap: "6px 18px", margin: "0 0 14px", fontSize: 10 }}>
      {visible.map((item) => (
        <div key={item.label}>
          <dt style={{ color: "var(--sv-text-faint)" }}>{item.label}</dt>
          <dd className="sv-mono" style={{ margin: "2px 0 0" }}>{item.value}</dd>
        </div>
      ))}
    </dl>
  );
}

export function ReadinessBadge({ state }: { state: "ready" | "draft" | "stale" }) {
  return <span data-readiness={state} className="sv-label">{state}</span>;
}
