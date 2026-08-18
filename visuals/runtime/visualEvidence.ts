/**
 * Four-state visual evidence verdict. Partial/failed never gates task completion.
 * Subscription, compute, review, readiness, pinning, sealing, and sharing are
 * separate facets — Draft/Ready is not a substitute for any of them.
 */

export const VISUAL_EVIDENCE_STATES = ["ready", "reviewed", "partial", "failed"] as const;
export type VisualEvidenceState = (typeof VISUAL_EVIDENCE_STATES)[number];

export type VisualEvidence = {
  state: VisualEvidenceState;
  decidedAt: string;
  detail: string;
};

export type VisualLifecycleFacets = {
  subscription: "idle" | "bootstrapping" | "subscribed" | "stale" | "reconnecting" | "terminal" | "failed";
  compute: "running" | "terminal" | "unknown";
  review: "none" | "in_progress" | "reviewed";
  readiness: "waiting" | "ready" | "not_required";
  pinning: "unpinned" | "pinned";
  sealing: "unsealed" | "sealed";
  sharing: "private" | "requested" | "shared";
};

export function decideVisualEvidence(input: {
  readyReceipt: boolean;
  reviewed: boolean;
  hasVisual: boolean;
  renderFailed: boolean;
  decidedAt: string;
}): VisualEvidence {
  if (input.readyReceipt) {
    return {
      state: "ready",
      decidedAt: input.decidedAt,
      detail: "visual readiness receipt posted"
    };
  }
  if (input.reviewed) {
    return {
      state: "reviewed",
      decidedAt: input.decidedAt,
      detail: "reviews recorded without a readiness receipt"
    };
  }
  if (input.renderFailed || !input.hasVisual) {
    return {
      state: "failed",
      decidedAt: input.decidedAt,
      detail: "no usable product visual; this does not block task completion"
    };
  }
  return {
    state: "partial",
    decidedAt: input.decidedAt,
    detail: "product visual exists but is not certified; this does not block task completion"
  };
}

export function visualEvidenceBlocksCompletion(_evidence: VisualEvidence): boolean {
  return false;
}
