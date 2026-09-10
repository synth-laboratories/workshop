import { VisualChrome } from "../../chrome/VisualChrome.tsx";
import {
  ComparisonScopeNotice,
  EvidenceStateBanner,
  MetricValue,
  ProvenanceHeader,
  ReadinessBadge,
  type EvidenceState
} from "../../chrome/EvidencePrimitives.tsx";

const states: Array<{ state: EvidenceState; detail: string }> = [
  { state: "live", detail: "The declared source is open and caught up." },
  { state: "terminal", detail: "The declared source closed with retained evidence." },
  { state: "partial", detail: "Some evidence is present; completeness is not asserted." },
  { state: "stale", detail: "A prior judgment no longer matches the rendered identity." },
  { state: "unavailable", detail: "The producer supplied no defensible evidence." }
];

export function Shell() {
  return (
    <VisualChrome
      kicker="Visual QA · reviewer specimen"
      title="Evidence grammar acceptance gallery"
      lede="One compact surface for checking vocabulary, hierarchy, missing values, comparison scope, provenance, and readiness at every review width."
      testId="visual-evidence-acceptance"
      footer="evidence.acceptance.v1 · internal"
    >
      <section className="sv-section" aria-labelledby="state-specimens">
        <div className="sv-section-head"><h3 id="state-specimens">Evidence states</h3><span>five meanings · no aliases</span></div>
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(210px, 1fr))", gap: 10 }}>
          {states.map(({ state, detail }) => <EvidenceStateBanner key={state} state={state}>{detail}</EvidenceStateBanner>)}
        </div>
      </section>

      <section className="sv-section" aria-labelledby="scope-specimens">
        <div className="sv-section-head"><h3 id="scope-specimens">Comparison scope</h3><span>orthogonal to completeness</span></div>
        <ComparisonScopeNotice comparable contract="sha256:shared-contract" />
        <ComparisonScopeNotice comparable={false} />
      </section>

      <section className="sv-section" aria-labelledby="measurement-specimens">
        <div className="sv-section-head"><h3 id="measurement-specimens">Measurements and provenance</h3><span>missing is not zero</span></div>
        <ProvenanceHeader items={[
          { label: "Benchmark", value: "Craftax" },
          { label: "Evaluator", value: "retained-eval-v2" },
          { label: "Rollout", value: "roll_retained_1" },
          { label: "Digest", value: "sha256:7f…91" }
        ]} />
        <div style={{ display: "flex", flexWrap: "wrap", gap: "14px 30px", padding: 14, border: "1px solid var(--sv-border)", borderRadius: 10 }}>
          <MetricValue label="Mean reward" value="0.25" qualifier="4 scored rollouts" />
          <MetricValue label="Single-rollout reward" value="1.00" qualifier="aggregation: none" />
          <MetricValue label="Reported cost" value={null} qualifier="not reported" />
          <MetricValue label="Task success" value={null} qualifier="not inferred from reward" />
        </div>
      </section>

      <section className="sv-section" aria-labelledby="readiness-specimens">
        <div className="sv-section-head"><h3 id="readiness-specimens">Readiness</h3><span>reviewer-visible states</span></div>
        <div style={{ display: "flex", flexWrap: "wrap", gap: 14 }}>
          <ReadinessBadge state="draft" />
          <ReadinessBadge state="ready" />
          <ReadinessBadge state="stale" />
        </div>
      </section>

      <section className="sv-section" aria-labelledby="refusal-specimens">
        <div className="sv-section-head"><h3 id="refusal-specimens">Expected mechanical refusals</h3><span>must never turn green</span></div>
        <ul style={{ margin: 0, paddingLeft: 20, color: "var(--sv-text-muted)" }}>
          <li>Claimed viewport differs from the native capture receipt.</li>
          <li>Source and build revisions differ, are dirty, or lack an executable digest.</li>
          <li>Ranking metric differs from the shared comparison contract.</li>
          <li>Reward evidence has no scored rollout or measured component.</li>
          <li>Rendered observation changes while the screenshot is captured.</li>
        </ul>
      </section>
    </VisualChrome>
  );
}

export default Shell;
