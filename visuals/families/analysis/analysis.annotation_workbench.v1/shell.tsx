import { useMemo, useState } from "react";
import { MetricStrip, VisualChrome } from "../../../chrome/VisualChrome.tsx";
import type { VisualBinding } from "../../../runtime/types.ts";
import craftaxFixture from "../../../fixtures/annotation_workbench_craftax.json";

type FindingStatus = "applied" | "abstained" | "rejected";
type MilestoneState = "verified" | "partial" | "blocked" | "unsupported" | "attempted";
type ViewId = "overview" | "findings" | "milestones" | "rubric" | "trace" | "audit";

type SelectorRef = { kind?: string; id?: string; selector?: string; spanId?: string };
type Finding = {
  id: string;
  annotatorId: string;
  type?: string;
  label: string;
  severity?: string;
  status: FindingStatus;
  target?: SelectorRef;
  summary?: string;
};
type Milestone = { id: string; label: string; state: MilestoneState; engineVerified?: boolean };
type SpanRow = { id: string; sequence?: number; title: string; kind?: string };
type RubricCriterion = {
  id: string;
  label: string;
  judgment?: string;
  score?: number | null;
  citations?: string[];
};
type WorkbenchProjection = {
  schemaVersion?: string;
  campaign?: {
    id?: string;
    status?: string;
    label?: string;
    domain?: string;
    title?: string;
  };
  coverage?: {
    jobs?: number;
    sealed?: number;
    abstained?: number;
    failed?: number;
    rejected?: number;
    cacheHits?: number;
  };
  validation?: {
    selectorsResolved?: number;
    unresolvedSelectors?: number;
    validationFailures?: number;
  };
  cost?: { usd?: number | null; calls?: number | null; tokens?: number | null };
  evidenceHead?: { bundleId?: string; digest?: string; annotationCount?: number };
  rubric?: {
    available?: boolean;
    reason?: string;
    digest?: string | null;
    criteria?: RubricCriterion[];
  };
  findings?: Finding[];
  taxonomy?: { label: string; count: number }[];
  milestones?: Milestone[];
  spans?: SpanRow[];
  jobs?: { id: string; annotatorId: string; state: string; reason?: string }[];
  audit?: {
    abstentions?: { jobId?: string; reason: string }[];
    rejected?: { findingId?: string; reason: string }[];
    unresolvedSelectors?: { selector: string }[];
    consensus?: { target?: string; agreement?: number }[];
  };
};

export type ShellProps = {
  title?: string;
  lede?: string;
  evidence?: WorkbenchProjection;
  rubric?: { available?: boolean; reason?: string; criteria?: RubricCriterion[]; digest?: string | null };
  data?: WorkbenchProjection;
  bindings?: VisualBinding[];
  onReviewFinding?: (input: {
    findingId: string;
    decision: string;
    rationale: string;
    evidenceHeadDigest?: string;
  }) => Promise<void> | void;
};

const VIEWS: { id: ViewId; label: string }[] = [
  { id: "overview", label: "Overview" },
  { id: "findings", label: "Findings" },
  { id: "milestones", label: "Milestones" },
  { id: "rubric", label: "Rubric" },
  { id: "trace", label: "Trace" },
  { id: "audit", label: "Audit" }
];

const MILESTONE_TONE: Record<MilestoneState, { bg: string; fg: string }> = {
  verified: { bg: "var(--sv-ok-bg)", fg: "var(--sv-ok-fg)" },
  partial: { bg: "#fff4e9", fg: "#b94712" },
  blocked: { bg: "#fdeeee", fg: "#9b2c2c" },
  unsupported: { bg: "var(--sv-surface-muted)", fg: "var(--sv-text-muted)" },
  attempted: { bg: "#eef4ff", fg: "#2c5282" }
};

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : {};
}

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function num(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function selectorKey(target?: SelectorRef): string {
  if (!target) return "";
  return target.selector || target.spanId || target.id || "";
}

function readProjection(raw: unknown): WorkbenchProjection {
  const row = asRecord(raw);
  if (text(row.schemaVersion) === "synth.annotation-workbench.v1" || Array.isArray(row.findings) || row.campaign) {
    return raw as WorkbenchProjection;
  }
  const nested = asRecord(row.evidence);
  if (text(nested.schemaVersion) === "synth.annotation-workbench.v1") return nested as WorkbenchProjection;
  return craftaxFixture as WorkbenchProjection;
}

function usd(value?: number | null): string {
  if (value == null) return "—";
  return `$${value.toFixed(value >= 1 ? 2 : 4)}`;
}

function Chip({ label, tone }: { label: string; tone?: { bg: string; fg: string } }) {
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        padding: "2px 8px",
        borderRadius: 999,
        background: tone?.bg ?? "var(--sv-surface-muted)",
        color: tone?.fg ?? "var(--sv-text)",
        fontSize: "var(--sv-fs-meta)",
        fontWeight: 600
      }}
    >
      {label}
    </span>
  );
}

export function Shell(props: ShellProps) {
  const projection = readProjection(props.evidence ?? props.data ?? craftaxFixture);
  const rubric = props.rubric ?? projection.rubric ?? { available: false, reason: "verifier_result_missing" };
  const findings = projection.findings ?? [];
  const milestones = projection.milestones ?? [];
  const spans = projection.spans ?? [];
  const taxonomy = projection.taxonomy ?? [];
  const campaign = projection.campaign ?? {};
  const coverage = projection.coverage ?? {};
  const validation = projection.validation ?? {};
  const [view, setView] = useState<ViewId>("overview");
  const [selectedFinding, setSelectedFinding] = useState<string | null>(null);
  const [selectedSpan, setSelectedSpan] = useState<string | null>(null);
  const [labelFilter, setLabelFilter] = useState<string | null>(null);
  const [reviewFindingId, setReviewFindingId] = useState<string>("");
  const [reviewDecision, setReviewDecision] = useState("flag");
  const [reviewRationale, setReviewRationale] = useState("");
  const [reviewStatus, setReviewStatus] = useState<string | null>(null);

  const focusedSpan = selectedSpan || selectorKey(findings.find((row) => row.id === selectedFinding)?.target);
  const citing = useMemo(
    () => (focusedSpan ? findings.filter((row) => selectorKey(row.target) === focusedSpan || row.target?.id === focusedSpan) : []),
    [findings, focusedSpan]
  );
  const visibleFindings = labelFilter ? findings.filter((row) => row.label === labelFilter) : findings;

  const openFinding = (finding: Finding) => {
    setSelectedFinding(finding.id);
    setSelectedSpan(selectorKey(finding.target) || finding.target?.id || null);
    setView("trace");
  };
  const openSpan = (span: SpanRow) => {
    setSelectedSpan(span.id);
    setSelectedFinding(null);
    setView("findings");
  };

  const title = props.title ?? campaign.title ?? "Annotation workbench";
  const semanticCount = findings.length + milestones.length + (projection.jobs?.length ?? 0);

  return (
    <VisualChrome
      kicker={`${campaign.domain ?? "trace"} · analysis projection · not eval reward`}
      title={title}
      lede={
        props.lede ??
        `${coverage.sealed ?? 0}/${coverage.jobs ?? 0} jobs sealed · ${validation.selectorsResolved ?? 0} selectors resolved · ${campaign.status ?? "unknown"}`
      }
      testId="visual-annotation-workbench"
      observation={{
        transportState: "terminal",
        semanticEventCount: Math.max(semanticCount, 1),
        terminal: true
      }}
      footer="analysis.annotation_workbench.v1 · projection only · engine/verifier artifacts remain authoritative"
    >
      <MetricStrip
        metrics={[
          { label: "Sealed", value: `${coverage.sealed ?? 0}/${coverage.jobs ?? 0}` },
          { label: "Abstained", value: String(coverage.abstained ?? 0) },
          { label: "Rejected", value: String(coverage.rejected ?? 0) },
          { label: "Selectors", value: String(validation.selectorsResolved ?? 0) },
          { label: "Cost", value: usd(projection.cost?.usd) }
        ]}
      />
      <div role="tablist" aria-label="Analysis views" style={{ display: "flex", gap: 6, margin: "12px 0 16px", flexWrap: "wrap" }}>
        {VIEWS.map((tab) => {
          const active = view === tab.id;
          return (
            <button
              key={tab.id}
              type="button"
              role="tab"
              aria-selected={active}
              data-testid={`analysis-view-${tab.id}`}
              onClick={() => setView(tab.id)}
              style={{
                border: "1px solid var(--sv-border)",
                background: active ? "var(--sv-accent-soft)" : "var(--sv-surface)",
                color: "var(--sv-text)",
                borderRadius: 8,
                padding: "6px 10px",
                fontSize: "var(--sv-fs-body)",
                fontWeight: active ? 700 : 500
              }}
            >
              {tab.label}
            </button>
          );
        })}
      </div>

      {view === "overview" ? (
        <section aria-label="Campaign overview">
          <p style={{ color: "var(--sv-text-muted)", fontSize: "var(--sv-fs-body)", marginTop: 0 }}>
            Evidence head {projection.evidenceHead?.digest ?? "unbound"} · campaign {campaign.id ?? "—"}.
            Validation failures: {validation.validationFailures ?? 0}. Unresolved selectors: {validation.unresolvedSelectors ?? 0}.
          </p>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(160px, 1fr))", gap: 8 }}>
            {taxonomy.map((row) => (
              <button
                key={row.label}
                type="button"
                onClick={() => {
                  setLabelFilter(row.label);
                  setView("findings");
                }}
                style={{
                  textAlign: "left",
                  border: "1px solid var(--sv-border)",
                  borderRadius: 10,
                  padding: 12,
                  background: "var(--sv-surface-muted)"
                }}
              >
                <strong style={{ display: "block", fontSize: "var(--sv-fs-hero)" }}>{row.count}</strong>
                <span style={{ fontSize: "var(--sv-fs-meta)", color: "var(--sv-text-muted)" }}>{row.label}</span>
              </button>
            ))}
          </div>
        </section>
      ) : null}

      {view === "findings" ? (
        <section aria-label="Findings">
          {labelFilter ? (
            <p style={{ marginTop: 0 }}>
              Filter <Chip label={labelFilter} />{" "}
              <button type="button" onClick={() => setLabelFilter(null)}>
                Clear
              </button>
            </p>
          ) : null}
          {citing.length > 0 && focusedSpan ? (
            <p style={{ color: "var(--sv-text-muted)", fontSize: "var(--sv-fs-body)" }}>
              {citing.length} finding{citing.length === 1 ? "" : "s"} cite `{focusedSpan}`.
            </p>
          ) : null}
          <ul style={{ listStyle: "none", padding: 0, margin: 0, display: "grid", gap: 8 }}>
            {visibleFindings.map((finding) => (
              <li key={finding.id}>
                <button
                  type="button"
                  data-testid={`analysis-finding-${finding.id}`}
                  onClick={() => openFinding(finding)}
                  style={{
                    width: "100%",
                    textAlign: "left",
                    border: selectedFinding === finding.id ? "1px solid var(--sv-accent)" : "1px solid var(--sv-border)",
                    borderRadius: 10,
                    padding: 12,
                    background: "var(--sv-surface)"
                  }}
                >
                  <div style={{ display: "flex", gap: 8, flexWrap: "wrap", marginBottom: 6 }}>
                    <Chip label={finding.label} />
                    <Chip label={finding.status} />
                    {finding.severity ? <Chip label={finding.severity} /> : null}
                  </div>
                  <strong>{finding.summary ?? finding.label}</strong>
                  <div style={{ color: "var(--sv-text-muted)", fontSize: "var(--sv-fs-meta)", marginTop: 4 }}>
                    {finding.annotatorId} · {selectorKey(finding.target) || "no selector"}
                  </div>
                </button>
              </li>
            ))}
          </ul>
        </section>
      ) : null}

      {view === "milestones" ? (
        <section aria-label="Milestones">
          <ul style={{ listStyle: "none", padding: 0, margin: 0, display: "grid", gap: 8 }}>
            {milestones.map((milestone) => (
              <li
                key={milestone.id}
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  gap: 12,
                  border: "1px solid var(--sv-border)",
                  borderRadius: 10,
                  padding: 12
                }}
              >
                <span>
                  <strong>{milestone.label}</strong>
                  <div style={{ fontSize: "var(--sv-fs-meta)", color: "var(--sv-text-muted)" }}>
                    {milestone.engineVerified ? "engine-verified" : "annotation only"}
                  </div>
                </span>
                <Chip label={milestone.state} tone={MILESTONE_TONE[milestone.state]} />
              </li>
            ))}
          </ul>
        </section>
      ) : null}

      {view === "rubric" ? (
        <section aria-label="Rubric" data-testid="analysis-rubric">
          {rubric.available && (rubric.criteria?.length ?? 0) > 0 ? (
            <ul style={{ listStyle: "none", padding: 0, margin: 0, display: "grid", gap: 8 }}>
              {(rubric.criteria ?? []).map((criterion) => (
                <li key={criterion.id} style={{ border: "1px solid var(--sv-border)", borderRadius: 10, padding: 12 }}>
                  <strong>{criterion.label}</strong>
                  <div style={{ fontSize: "var(--sv-fs-body)", color: "var(--sv-text-muted)" }}>
                    {criterion.judgment ?? "—"}
                    {criterion.score != null ? ` · ${criterion.score}` : ""}
                  </div>
                </li>
              ))}
            </ul>
          ) : (
            <p data-testid="analysis-rubric-unavailable" style={{ color: "var(--sv-text-muted)" }}>
              Rubric unavailable{rubric.reason ? ` (${rubric.reason})` : ""}. Missing verifier evidence is not a zero score.
            </p>
          )}
        </section>
      ) : null}

      {view === "trace" ? (
        <section aria-label="Trace spans">
          <ol style={{ listStyle: "none", padding: 0, margin: 0, display: "grid", gap: 6 }}>
            {spans.map((span) => {
              const active = focusedSpan === span.id;
              const marks = findings.filter((row) => selectorKey(row.target) === span.id || row.target?.id === span.id);
              return (
                <li key={span.id}>
                  <button
                    type="button"
                    data-testid={`analysis-span-${span.id}`}
                    onClick={() => openSpan(span)}
                    style={{
                      width: "100%",
                      textAlign: "left",
                      border: active ? "1px solid var(--sv-accent)" : "1px solid var(--sv-border)",
                      borderRadius: 10,
                      padding: 10,
                      background: active ? "var(--sv-accent-soft)" : "var(--sv-surface)"
                    }}
                  >
                    <strong>
                      {span.sequence != null ? `${span.sequence}. ` : ""}
                      {span.title}
                    </strong>
                    <div style={{ fontSize: "var(--sv-fs-meta)", color: "var(--sv-text-muted)" }}>
                      {span.kind ?? "span"} · {marks.length} citation{marks.length === 1 ? "" : "s"}
                    </div>
                  </button>
                </li>
              );
            })}
          </ol>
        </section>
      ) : null}

      {view === "audit" ? (
        <section aria-label="Audit">
          <form
            data-testid="analysis-review-form"
            onSubmit={(event) => {
              event.preventDefault();
              const findingId = reviewFindingId || selectedFinding || findings[0]?.id;
              if (!findingId || !props.onReviewFinding) {
                setReviewStatus(props.onReviewFinding ? "Pick a finding to review." : "Review is unavailable in this host.");
                return;
              }
              setReviewStatus("Saving…");
              void Promise.resolve(props.onReviewFinding({
                findingId,
                decision: reviewDecision,
                rationale: reviewRationale,
                evidenceHeadDigest: projection.evidenceHead?.digest
              })).then(() => {
                setReviewStatus(`Recorded ${reviewDecision} on ${findingId}`);
                setReviewRationale("");
              }).catch((reason) => {
                setReviewStatus(reason instanceof Error ? reason.message : "Review failed");
              });
            }}
            style={{ display: "grid", gap: 8, marginBottom: 16, padding: 12, border: "1px solid var(--sv-border)", borderRadius: 10 }}
          >
            <h3 style={{ fontSize: "var(--sv-fs-strong)", margin: 0 }}>Local review</h3>
            <label style={{ display: "grid", gap: 4, fontSize: "var(--sv-fs-meta)" }}>
              Finding
              <select
                data-testid="analysis-review-finding"
                value={reviewFindingId || selectedFinding || findings[0]?.id || ""}
                onChange={(event) => setReviewFindingId(event.target.value)}
              >
                {findings.map((finding) => (
                  <option key={finding.id} value={finding.id}>{finding.label} · {finding.id}</option>
                ))}
              </select>
            </label>
            <label style={{ display: "grid", gap: 4, fontSize: "var(--sv-fs-meta)" }}>
              Decision
              <select data-testid="analysis-review-decision" value={reviewDecision} onChange={(event) => setReviewDecision(event.target.value)}>
                <option value="flag">Flag</option>
                <option value="confirm">Confirm</option>
                <option value="reject">Reject</option>
                <option value="needs_human">Needs human</option>
              </select>
            </label>
            <label style={{ display: "grid", gap: 4, fontSize: "var(--sv-fs-meta)" }}>
              Rationale
              <textarea
                data-testid="analysis-review-rationale"
                value={reviewRationale}
                onChange={(event) => setReviewRationale(event.target.value)}
                rows={3}
                style={{ resize: "vertical" }}
              />
            </label>
            <button className="sv-btn" type="submit" data-testid="analysis-review-submit" disabled={!props.onReviewFinding || findings.length === 0}>
              Record review
            </button>
            {reviewStatus ? <p className="sv-mono" data-testid="analysis-review-status" style={{ margin: 0 }}>{reviewStatus}</p> : null}
          </form>
          <h3 style={{ fontSize: "var(--sv-fs-strong)", margin: "0 0 8px" }}>Abstentions</h3>
          <ul>
            {(projection.audit?.abstentions ?? []).map((row, index) => (
              <li key={`${row.jobId ?? "abs"}-${index}`}>
                {row.jobId ? `${row.jobId}: ` : ""}
                {row.reason}
              </li>
            ))}
          </ul>
          <h3 style={{ fontSize: "var(--sv-fs-strong)" }}>Rejected</h3>
          {(projection.audit?.rejected ?? []).length === 0 ? <p>None.</p> : (
            <ul>
              {(projection.audit?.rejected ?? []).map((row, index) => (
                <li key={`${row.findingId ?? "rej"}-${index}`}>{row.reason}</li>
              ))}
            </ul>
          )}
          <h3 style={{ fontSize: "var(--sv-fs-strong)" }}>Unresolved selectors</h3>
          {(projection.audit?.unresolvedSelectors ?? []).length === 0 ? <p>None.</p> : (
            <ul>
              {(projection.audit?.unresolvedSelectors ?? []).map((row) => (
                <li key={row.selector}>{row.selector}</li>
              ))}
            </ul>
          )}
        </section>
      ) : null}
    </VisualChrome>
  );
}
