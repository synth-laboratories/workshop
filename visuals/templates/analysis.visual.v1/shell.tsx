import { VisualChrome } from "../../chrome/VisualChrome.tsx";
import type { VisualBinding } from "../../runtime/types.ts";

type Block =
  | { kind: "note"; title?: string; body: string; tone?: "neutral" | "caution" }
  | { kind: "metrics"; title?: string; items: Array<{ label: string; value: string; detail?: string }> }
  | { kind: "ranked-bars"; title: string; subtitle?: string; unit?: string; items: Array<{ label: string; value: number; display?: string; color?: string }> }
  | { kind: "frequency-diff"; title: string; subtitle?: string; baseline: string; comparison: string; rows: Array<{ label: string; baseline: number; comparison: number }> }
  | { kind: "table"; title: string; columns: string[]; rows: Array<Array<string | number>> }
  | { kind: "scatter"; title: string; xLabel: string; yLabel: string; points: Array<{ label: string; x: number; y: number; color?: string }> };

type AnalysisSpec = { kicker?: string; title?: string; lede?: string; footer?: string; blocks: Block[] };
export type ShellProps = { title?: string; lede?: string; spec?: AnalysisSpec; data?: AnalysisSpec; bindings?: VisualBinding[] };
const ORANGE = "#f05f22";
const INK = "#293241";

function SectionTitle({ title, subtitle }: { title: string; subtitle?: string }) {
  return <div className="sv-section-head"><h3>{title}</h3>{subtitle ? <span>{subtitle}</span> : null}</div>;
}

function Metrics({ block }: { block: Extract<Block, { kind: "metrics" }> }) {
  return <section className="sv-section">{block.title ? <SectionTitle title={block.title} /> : null}<div className="sv-metrics" role="group" aria-label={block.title ?? "Metrics"}>{block.items.map((item) => <div className="sv-metric" key={item.label}><span>{item.label}</span><strong>{item.value}</strong>{item.detail ? <small style={{ color: "var(--sv-text-faint)" }}>{item.detail}</small> : null}</div>)}</div></section>;
}

function RankedBars({ block }: { block: Extract<Block, { kind: "ranked-bars" }> }) {
  const max = Math.max(...block.items.map((item) => Math.abs(item.value)), 1);
  return <section className="sv-section"><SectionTitle title={block.title} subtitle={block.subtitle} /><div style={{ display: "grid", gap: 9 }}>{block.items.map((item) => <div key={item.label} style={{ display: "grid", gridTemplateColumns: "minmax(100px,1fr) minmax(120px,2fr) 58px", gap: 10, alignItems: "center" }}><strong style={{ fontSize: 11, overflow: "hidden", textOverflow: "ellipsis" }}>{item.label}</strong><span style={{ height: 8, background: "#edf0f3", borderRadius: 99, overflow: "hidden" }}><span style={{ display: "block", width: `${Math.abs(item.value) / max * 100}%`, height: "100%", borderRadius: 99, background: item.color ?? ORANGE }} /></span><span style={{ textAlign: "right", fontFamily: "var(--sv-mono)", fontSize: 11 }}>{item.display ?? `${item.value}${block.unit ?? ""}`}</span></div>)}</div></section>;
}

function FrequencyDiff({ block }: { block: Extract<Block, { kind: "frequency-diff" }> }) {
  const rows = [...block.rows].sort((a, b) => Math.abs(b.comparison - b.baseline) - Math.abs(a.comparison - a.baseline));
  return <section className="sv-section"><SectionTitle title={block.title} subtitle={block.subtitle} /><div style={{ display: "flex", gap: 14, marginBottom: 10, fontSize: 10, color: "var(--sv-text-faint)" }}><span><b style={{ color: ORANGE }}>●</b> {block.baseline}</span><span><b style={{ color: INK }}>●</b> {block.comparison}</span></div><div role="table" aria-label={block.title}>{rows.map((row) => { const delta = row.comparison - row.baseline; return <div role="row" key={row.label} style={{ display: "grid", gridTemplateColumns: "minmax(100px,1fr) minmax(150px,2fr) 54px", gap: 10, alignItems: "center", padding: "8px 0", borderTop: "1px solid #eef0f3" }}><strong style={{ fontFamily: "var(--sv-mono)", fontSize: 11 }}>{row.label}</strong><div style={{ display: "grid", gap: 4 }}>{[[row.baseline, ORANGE], [row.comparison, INK]].map(([value, color], index) => <div key={index} style={{ display: "grid", gridTemplateColumns: "34px 1fr", gap: 6, alignItems: "center" }}><span style={{ fontFamily: "var(--sv-mono)", fontSize: 9, textAlign: "right", color: "var(--sv-text-faint)" }}>{Math.round((value as number) * 100)}%</span><span style={{ height: 6, background: "#edf0f3", borderRadius: 99, overflow: "hidden" }}><span style={{ display: "block", width: `${Math.max(0, value as number) * 100}%`, height: "100%", background: color as string, borderRadius: 99 }} /></span></div>)}</div><strong style={{ textAlign: "right", fontFamily: "var(--sv-mono)", fontSize: 10, color: delta > 0 ? INK : delta < 0 ? ORANGE : "var(--sv-text-faint)" }}>{delta > 0 ? "+" : ""}{Math.round(delta * 100)}pp</strong></div>; })}</div></section>;
}

function TableBlock({ block }: { block: Extract<Block, { kind: "table" }> }) {
  return <section className="sv-section"><SectionTitle title={block.title} /><div style={{ overflowX: "auto", border: "1px solid var(--sv-border)", borderRadius: 8 }}><table style={{ width: "100%", borderCollapse: "collapse", fontSize: 11 }}><thead><tr>{block.columns.map((column) => <th key={column} style={{ padding: 9, textAlign: "left", color: "var(--sv-text-faint)", borderBottom: "1px solid var(--sv-border)" }}>{column}</th>)}</tr></thead><tbody>{block.rows.map((row, index) => <tr key={index}>{row.map((cell, cellIndex) => <td key={cellIndex} style={{ padding: 9, borderTop: index ? "1px solid #eef0f3" : undefined }}>{cell}</td>)}</tr>)}</tbody></table></div></section>;
}

function Scatter({ block }: { block: Extract<Block, { kind: "scatter" }> }) {
  const maxX = Math.max(...block.points.map((point) => point.x), 1); const maxY = Math.max(...block.points.map((point) => point.y), 1);
  return <section className="sv-section"><SectionTitle title={block.title} /><div role="img" aria-label={`${block.yLabel} versus ${block.xLabel}`} style={{ border: "1px solid var(--sv-border)", borderRadius: 8 }}><svg viewBox="0 0 360 210" width="100%">{[45,85,125,165].map((y) => <line key={y} x1="46" y1={y} x2="340" y2={y} stroke="#e8eaee" />)}{block.points.map((point, index) => { const x = 58 + point.x / maxX * 265; const y = 165 - point.y / maxY * 130; return <g key={`${point.label}-${index}`}><circle cx={x} cy={y} r="7" fill={point.color ?? (index ? INK : ORANGE)} stroke="#fff" strokeWidth="2" /><text x={x} y={y - 12} textAnchor="middle" fill="#5c6573" fontSize="9">{point.label}</text></g>; })}<text x="196" y="202" textAnchor="middle" fill="#8b93a1" fontSize="10">{block.xLabel}</text><text x="13" y="104" textAnchor="middle" fill="#8b93a1" fontSize="10" transform="rotate(-90 13 104)">{block.yLabel}</text></svg></div></section>;
}

function normalizeBlock(raw: unknown): Block | null {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const block = raw as Record<string, unknown>;
  // Agent-authored CUA payloads persist `type` / `text`; the shell schema uses `kind` / `body`.
  const kind = typeof block.kind === "string" ? block.kind : typeof block.type === "string" ? block.type : null;
  if (!kind) return null;
  if (kind === "note") {
    const body = typeof block.body === "string" ? block.body : typeof block.text === "string" ? block.text : "";
    return {
      kind: "note",
      title: typeof block.title === "string" ? block.title : undefined,
      body,
      tone: block.tone === "caution" ? "caution" : "neutral"
    };
  }
  if (kind === "metrics") {
    if (!Array.isArray(block.items)) return null;
    return {
      kind: "metrics",
      title: typeof block.title === "string" ? block.title : undefined,
      items: block.items as Extract<Block, { kind: "metrics" }>["items"]
    };
  }
  if (kind === "ranked-bars") {
    if (typeof block.title !== "string" || !Array.isArray(block.items)) return null;
    return {
      kind: "ranked-bars",
      title: block.title,
      subtitle: typeof block.subtitle === "string" ? block.subtitle : undefined,
      unit: typeof block.unit === "string" ? block.unit : undefined,
      items: block.items as Extract<Block, { kind: "ranked-bars" }>["items"]
    };
  }
  if (kind === "frequency-diff") {
    if (
      typeof block.title !== "string" ||
      typeof block.baseline !== "string" ||
      typeof block.comparison !== "string" ||
      !Array.isArray(block.rows)
    ) {
      return null;
    }
    return {
      kind: "frequency-diff",
      title: block.title,
      subtitle: typeof block.subtitle === "string" ? block.subtitle : undefined,
      baseline: block.baseline,
      comparison: block.comparison,
      rows: block.rows as Extract<Block, { kind: "frequency-diff" }>["rows"]
    };
  }
  if (kind === "table") {
    if (typeof block.title !== "string" || !Array.isArray(block.columns) || !Array.isArray(block.rows)) {
      return null;
    }
    return {
      kind: "table",
      title: block.title,
      columns: block.columns as string[],
      rows: block.rows as Extract<Block, { kind: "table" }>["rows"]
    };
  }
  if (kind === "scatter") {
    if (
      typeof block.title !== "string" ||
      typeof block.xLabel !== "string" ||
      typeof block.yLabel !== "string" ||
      !Array.isArray(block.points)
    ) {
      return null;
    }
    return {
      kind: "scatter",
      title: block.title,
      xLabel: block.xLabel,
      yLabel: block.yLabel,
      points: block.points as Extract<Block, { kind: "scatter" }>["points"]
    };
  }
  return null;
}

function renderBlock(block: Block, index: number) {
  if (block.kind === "note") return <section className="sv-section" key={index} style={{ padding: 12, borderRadius: 8, background: block.tone === "caution" ? "#fff6ee" : "#f5f7f9", color: "#5c6573", fontSize: 11 }}>{block.title ? <strong style={{ display: "block", marginBottom: 4, color: INK }}>{block.title}</strong> : null}{block.body}</section>;
  if (block.kind === "metrics") return <Metrics block={block} key={index} />;
  if (block.kind === "ranked-bars") return <RankedBars block={block} key={index} />;
  if (block.kind === "frequency-diff") return <FrequencyDiff block={block} key={index} />;
  if (block.kind === "table") return <TableBlock block={block} key={index} />;
  return <Scatter block={block} key={index} />;
}

export function Shell(props: ShellProps) {
  const raw = props.data ?? props.spec;
  const blocks = (raw?.blocks ?? []).map(normalizeBlock).filter((block): block is Block => block != null);
  if (!blocks.length) return <VisualChrome title={props.title ?? "Analysis visual"} lede="No visual specification was provided." testId="visual-analysis-spec"><></></VisualChrome>;
  return <VisualChrome kicker={raw?.kicker ?? "Agent-authored analysis"} title={props.title ?? raw?.title ?? "Analysis visual"} lede={props.lede ?? raw?.lede} footer={raw?.footer} testId="visual-analysis-spec">{blocks.map(renderBlock)}</VisualChrome>;
}

export default Shell;
