import { useMemo, useState } from "react";

export type TraceV5Family = "input" | "thinking" | "tool" | "output" | "artifact" | "system";
export type TraceV5Item = {
  id: string;
  sequence: number;
  family: TraceV5Family;
  kind: string;
  title: string;
  occurredAt?: string;
  body?: string;
  detail?: string;
  status?: string;
  candidateId?: string;
};

const META: Record<TraceV5Family, { label: string; glyph: string; tint: string }> = {
  input: { label: "Input", glyph: "→", tint: "#eaf3ff" },
  thinking: { label: "Thinking", glyph: "✦", tint: "#f6f0ff" },
  tool: { label: "Tool call", glyph: ">_", tint: "#eef8f1" },
  output: { label: "Output", glyph: "◆", tint: "#f2f4f7" },
  artifact: { label: "Artifact", glyph: "✓", tint: "#fff4e9" },
  system: { label: "Event", glyph: "·", tint: "#f7f7f8" }
};

function clock(value?: string): string {
  if (!value) return "—";
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? value : date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

export function TraceV5EventList({
  items,
  onSelectCandidate,
  defaultView = "focus",
  emptyToolText = "No structured tool-call events were captured by this transport."
}: {
  items: TraceV5Item[];
  onSelectCandidate?: (id: string) => void;
  defaultView?: "focus" | "full";
  emptyToolText?: string;
}) {
  const [view, setView] = useState<"focus" | "full">(defaultView);
  const [query, setQuery] = useState("");
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return items.filter((item) =>
      (view === "full" || ["input", "thinking", "tool", "output", "artifact"].includes(item.family)) &&
      (!needle || `${item.family} ${item.kind} ${item.title} ${item.body ?? ""} ${item.detail ?? ""}`.toLowerCase().includes(needle))
    );
  }, [items, query, view]);
  const toolCount = items.filter((item) => item.family === "tool").length;

  return (
    <div data-testid="trace-v5-event-list">
      <div style={{ display: "flex", flexWrap: "wrap", gap: 7, alignItems: "center", marginBottom: 9 }}>
        <div role="group" aria-label="Trace density" style={{ display: "flex" }}>
          {(["focus", "full"] as const).map((value) => (
            <button key={value} type="button" className="sv-btn" aria-pressed={view === value} onClick={() => setView(value)} style={{ fontSize: 10, background: view === value ? "var(--sv-text)" : "transparent", color: view === value ? "white" : "inherit" }}>{value}</button>
          ))}
        </div>
        <input aria-label="Search trace" placeholder="Search input, thinking, tools, output…" value={query} onChange={(event) => setQuery(event.target.value)} style={{ flex: "1 1 220px", minWidth: 0 }} />
        <span className="sv-mono" style={{ color: "var(--sv-text-faint)", fontSize: 9.5 }}>{visible.length}/{items.length} items · {toolCount} tool calls</span>
      </div>
      {toolCount === 0 ? <p style={{ margin: "0 0 9px", color: "var(--sv-text-faint)", fontSize: 10.5 }}>{emptyToolText}</p> : null}
      <div style={{ display: "grid", gap: 8 }}>
        {visible.map((item) => {
          const meta = META[item.family];
          const isLong = (item.body?.length ?? 0) > 520 || (item.detail?.length ?? 0) > 260;
          const open = expanded.has(item.id);
          return (
            <article key={item.id} data-testid={`trace-v5-item-${item.id}`} style={{ display: "grid", gridTemplateColumns: "42px minmax(0, 1fr)", gap: 8 }}>
              <aside style={{ paddingTop: 9, textAlign: "right", color: "var(--sv-text-faint)" }}><div className="sv-mono" style={{ fontSize: 9 }}>#{item.sequence}</div><time style={{ fontSize: 8 }}>{clock(item.occurredAt)}</time></aside>
              <div style={{ border: "1px solid var(--sv-border)", borderRadius: 9, overflow: "hidden", background: "var(--sv-surface)" }}>
                <header style={{ display: "flex", alignItems: "center", gap: 7, padding: "7px 9px", background: meta.tint, borderBottom: "1px solid var(--sv-border)" }}>
                  <span className="sv-mono" aria-hidden style={{ fontWeight: 800 }}>{meta.glyph}</span><strong style={{ fontSize: 10.5 }}>{meta.label}</strong><span className="sv-mono" style={{ color: "var(--sv-text-faint)", fontSize: 8.5 }}>{item.kind}</span>{item.status ? <span style={{ marginLeft: "auto", fontSize: 9, fontWeight: 700 }}>{item.status}</span> : null}
                </header>
                <div style={{ padding: 10 }}>
                  <strong style={{ display: "block", fontSize: 11.5 }}>{item.title}</strong>
                  {item.body ? <div style={{ marginTop: 5, whiteSpace: "pre-wrap", overflowWrap: "anywhere", fontSize: 11.5, lineHeight: 1.5, maxHeight: open ? "none" : item.family === "input" || item.family === "tool" ? 112 : 150, overflow: "hidden" }}>{item.body}</div> : null}
                  {item.detail ? <pre className="sv-mono" style={{ margin: "7px 0 0", padding: 8, borderRadius: 6, background: "var(--sv-surface-muted)", whiteSpace: "pre-wrap", overflowWrap: "anywhere", fontSize: 10.5, maxHeight: open ? 420 : 96, overflow: "auto" }}>{item.detail}</pre> : null}
                  <div style={{ display: "flex", gap: 6, marginTop: isLong || item.candidateId ? 7 : 0 }}>
                    {isLong ? <button type="button" className="sv-btn" onClick={() => setExpanded((current) => { const next = new Set(current); next.has(item.id) ? next.delete(item.id) : next.add(item.id); return next; })} style={{ fontSize: 10 }}>{open ? "Show less" : "Show all"}</button> : null}
                    {item.candidateId ? <button type="button" className="sv-btn" onClick={() => onSelectCandidate?.(item.candidateId!)} data-testid={`trace-open-candidate-${item.candidateId}`} style={{ fontSize: 10 }}>Open candidate →</button> : null}
                  </div>
                </div>
              </div>
            </article>
          );
        })}
        {!visible.length ? <p style={{ color: "var(--sv-text-faint)", fontSize: 11.5 }}>No trace items match this view.</p> : null}
      </div>
    </div>
  );
}
