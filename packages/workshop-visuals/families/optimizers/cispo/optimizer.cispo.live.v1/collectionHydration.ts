/**
 * Prefer the backend-owned algorithm projection over collection telemetry.
 *
 * Collection rows can describe a single optimizer step (for example a
 * microbatch size of one), while the top-level projection describes the
 * rollout group as a whole. The collection value is therefore only a
 * first-paint fallback when the projection has not observed that fact yet.
 *
 * Returns `null`, not `undefined`, when neither source reported the fact:
 * `CispoState` spells an unreported scalar `null`, and spreading `undefined`
 * over the projection would drop the key rather than carry the absence, so the
 * workspace could no longer tell "not reported" from "never in this state".
 */
export function projectedScalar(
  projected: unknown,
  collectionFallback: unknown
): number | null {
  if (typeof projected === "number" && Number.isFinite(projected)) return projected;
  return typeof collectionFallback === "number" && Number.isFinite(collectionFallback)
    ? collectionFallback
    : null;
}
