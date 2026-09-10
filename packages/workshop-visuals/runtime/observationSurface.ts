/**
 * Pick the element whose rendered-observation attributes are authoritative.
 *
 * Workshop's host wraps every template in its own element carrying
 * `data-visual-transport-state`, so a plain query for that attribute found the
 * wrapper before the template's own published observation. Two 2026-09-03
 * readiness failures came from that one shadowing:
 *
 * - `analysis.annotation_workbench.v1` declares `terminal` for immutable
 *   retained evidence; readiness harvested the wrapper's `idle` and rejected.
 * - `craftax.trace_workbench.v1` had frames on screen; the wrapper publishes
 *   no count attribute at all, so readiness harvested zero frames.
 *
 * The template's observation wins. The wrapper is the last resort, which keeps
 * a template that publishes nothing behaving exactly as before.
 */

export type ObservationSurface = {
  getAttribute(name: string): string | null;
  hasAttribute(name: string): boolean;
};

const COUNT_ATTRIBUTES = [
  "data-visual-semantic-event-count",
  "data-visual-rendered-frame-count",
  "data-visual-rollout-count"
];

export function selectObservationSurface<T extends ObservationSurface>(
  surfaces: readonly T[]
): T | undefined {
  return (
    surfaces.find((surface) => surface.getAttribute("data-visual-observation") === "template")
    // Templates that publish the attributes by hand, without the shared chrome
    // helper, are still recognisable: only a template publishes counts.
    ?? surfaces.find((surface) => COUNT_ATTRIBUTES.some((name) => surface.hasAttribute(name)))
    ?? surfaces[0]
  );
}
