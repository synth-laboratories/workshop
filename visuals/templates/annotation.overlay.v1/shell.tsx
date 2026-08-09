import { useMemo, useState } from "react";
import { VisualChrome } from "../../chrome/VisualChrome.tsx";
import { TimelineScrubber } from "../../chrome/TimelineScrubber.tsx";
import type { RolloutStep, TraceAnnotationMarker, VisualBinding } from "../../runtime/types.ts";
import markersFixture from "../../fixtures/annotation_markers.json";
import rolloutFixture from "../../fixtures/rollout_steps.json";

type AnnPayload = {
  trace_id?: string;
  markers: TraceAnnotationMarker[];
};

type TracePayload = {
  id?: string;
  steps?: RolloutStep[];
};

export type ShellProps = {
  title?: string;
  lede?: string;
  trace?: TracePayload;
  annotations?: AnnPayload;
  data?: { trace?: TracePayload; annotations?: AnnPayload };
  bindings?: VisualBinding[];
};

const KIND_COLOR: Record<string, string> = {
  note: "#5c6573",
  bug: "#c2553f",
  highlight: "#f05f22",
  reward: "#6f9a4d",
  acceptance: "#3d78bb"
};

function asAnn(raw: unknown): AnnPayload {
  if (raw && typeof raw === "object" && Array.isArray((raw as AnnPayload).markers)) {
    return raw as AnnPayload;
  }
  return markersFixture as AnnPayload;
}

function asTrace(raw: unknown): TracePayload {
  if (raw && typeof raw === "object") return raw as TracePayload;
  return rolloutFixture as TracePayload;
}

export function Shell(props: ShellProps) {
  const trace = asTrace(props.data?.trace ?? props.trace ?? rolloutFixture);
  const ann = asAnn(props.data?.annotations ?? props.annotations ?? markersFixture);
  const steps = trace.steps ?? (rolloutFixture as { steps: RolloutStep[] }).steps;
  const [index, setIndex] = useState(0);

  const markersAt = useMemo(
    () =>
      ann.markers.filter(
        (m) =>
          m.step_index === index ||
          m.turn === steps[index]?.turn ||
          (m.step_index == null && m.turn == null && index === 0)
      ),
    [ann.markers, index, steps]
  );

  const allPositions = useMemo(() => {
    return ann.markers.map((m) => m.step_index ?? m.turn ?? 0);
  }, [ann.markers]);

  return (
    <VisualChrome
      kicker="Overlay only · sealed Trace V5"
      title={props.title ?? "Annotation overlay"}
      lede={
        props.lede ??
        `Trace ${ann.trace_id ?? trace.id ?? "—"} — markers do not mutate the sealed artifact.`
      }
      testId="visual-annotation-overlay"
      footer="annotation.overlay.v1 · synth.rollout_annotations.v1 spirit"
    >
      <div
        role="img"
        aria-label="Trace timeline with annotation markers"
        style={{
          position: "relative",
          height: 56,
          background: "var(--sv-surface-muted)",
          border: "1px solid var(--sv-border)",
          borderRadius: 10,
          marginBottom: 8
        }}
      >
        <div
          style={{
            position: "absolute",
            left: 12,
            right: 12,
            top: "50%",
            height: 2,
            background: "var(--sv-border-strong)"
          }}
        />
        {ann.markers.map((m) => {
          const pos = m.step_index ?? m.turn ?? 0;
          const max = Math.max(...allPositions, steps.length - 1, 1);
          const left = `${(pos / max) * 100}%`;
          return (
            <button
              key={m.id}
              type="button"
              title={m.label}
              aria-label={`${m.kind} marker: ${m.label}`}
              onClick={() => setIndex(Math.min(pos, steps.length - 1))}
              style={{
                position: "absolute",
                left,
                top: "50%",
                transform: "translate(-50%, -50%)",
                width: 14,
                height: 14,
                borderRadius: "50%",
                border: "2px solid #fff",
                background: KIND_COLOR[m.kind] ?? "#5c6573",
                cursor: "pointer",
                boxShadow: "0 0 0 1px rgba(0,0,0,0.08)"
              }}
            />
          );
        })}
      </div>

      <TimelineScrubber
        index={index}
        total={steps.length}
        onChange={setIndex}
        label="Trace step scrubber"
      />

      <section className="sv-section">
        <div className="sv-section-head">
          <h3>Markers at this step</h3>
          <span>{markersAt.length} active</span>
        </div>
        {markersAt.length === 0 ? (
          <p style={{ color: "var(--sv-text-faint)", margin: 0 }}>No markers on this step.</p>
        ) : (
          <ul style={{ margin: 0, paddingLeft: 18 }}>
            {markersAt.map((m) => (
              <li key={m.id} style={{ marginBottom: 6 }}>
                <span
                  className="sv-mono"
                  style={{ color: KIND_COLOR[m.kind] ?? "inherit", marginRight: 6 }}
                >
                  [{m.kind}]
                </span>
                {m.label}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="sv-section" aria-label="All annotations">
        <div className="sv-section-head">
          <h3>All markers</h3>
          <span>{ann.markers.length}</span>
        </div>
        <table className="sv-table">
          <thead>
            <tr>
              <th scope="col">Step</th>
              <th scope="col">Kind</th>
              <th scope="col">Label</th>
            </tr>
          </thead>
          <tbody>
            {ann.markers.map((m) => (
              <tr key={m.id}>
                <td className="sv-mono">{m.step_index ?? m.turn ?? "—"}</td>
                <td style={{ color: KIND_COLOR[m.kind] }}>{m.kind}</td>
                <td>{m.label}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
    </VisualChrome>
  );
}

export default Shell;
