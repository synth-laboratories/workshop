/**
 * Capture evidence is either a mechanical observation contract or a screenshot.
 * Requiring an observation from a template that never declared one made capture
 * a guaranteed failure. Templates without a contract certify from the PNG.
 */

export type CaptureEvidenceKind = "observation" | "screenshot";

export function captureEvidenceKind(template: {
  observationContract?: unknown;
}): CaptureEvidenceKind {
  return template.observationContract ? "observation" : "screenshot";
}

/** Product classes v0.5 must be able to capture/review. */
export const CAPTURE_REVIEW_PRODUCT_CLASSES = [
  "optimizer.gepa.live.v1",
  "trace.rollout_inspector.v1",
  "live.craftax.v1",
  "optimizer.eval.live.v1",
  "optimizer.sft.live.v1",
  "optimizer.cispo.live.v1"
] as const;
