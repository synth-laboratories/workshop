import type { ProjectedState } from "../components/projectEvents.ts";

export function GoExOverlay({ state }: { state: ProjectedState }) {
  const goex = state.goex;
  if (!goex) return null;
  return (
    <>
      <section className="sv-section" aria-label="GELO board">
        <div className="sv-section-head">
          <h3>Phase board</h3>
        </div>
        <div className="sv-metrics">
          <div className="sv-metric"><span>Phase</span><strong>{String(goex.board.phase ?? "—")}</strong></div>
          <div className="sv-metric"><span>Tick</span><strong>{String(goex.board.tick ?? "—")}</strong></div>
          <div className="sv-metric"><span>Status</span><strong>{String(goex.board.status ?? "—")}</strong></div>
        </div>
      </section>
      <section className="sv-section" aria-label="Themes">
        <div className="sv-section-head">
          <h3>Themes</h3>
        </div>
        <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12 }}>
          {goex.themes.map((theme, index) => (
            <li key={index} className="sv-mono">
              {String(theme.theme ?? "theme")} · sat={String(theme.saturation ?? "—")}
            </li>
          ))}
        </ul>
      </section>
      {state.execution.bindings.length > 0 ? (
        <section className="sv-section" aria-label="Slot binding">
          <div className="sv-section-head">
            <h3>Execution</h3>
          </div>
          <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12 }}>
            {state.execution.bindings.map((binding, index) => (
              <li key={index} className="sv-mono">
                {String(binding.kind)}:{String(binding.id)} · {String(binding.status ?? "")}
              </li>
            ))}
          </ul>
        </section>
      ) : null}
    </>
  );
}
