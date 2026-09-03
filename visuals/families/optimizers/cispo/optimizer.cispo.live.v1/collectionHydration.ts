/**
 * Prefer the backend-owned algorithm projection over collection telemetry.
 *
 * Collection rows can describe a single optimizer step (for example a
 * microbatch size of one), while the top-level projection describes the
 * rollout group as a whole. The collection value is therefore only a
 * first-paint fallback when the projection has not observed that fact yet.
 */
export function projectedScalar(
  projected: unknown,
  collectionFallback: unknown
): number | undefined {
  if (typeof projected === "number" && Number.isFinite(projected)) return projected;
  return typeof collectionFallback === "number" && Number.isFinite(collectionFallback)
    ? collectionFallback
    : undefined;
}
