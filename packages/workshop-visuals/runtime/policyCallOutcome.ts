/** Closed outcomes for one policy/model call. `null` means the call is live. */
export const POLICY_CALL_OUTCOMES = [
  "completed",
  "invalid_response",
  "timed_out",
  "cancelled",
  "aborted"
] as const;

export type PolicyCallOutcome = (typeof POLICY_CALL_OUTCOMES)[number];

export type PolicyCallClosureSource =
  | "span.policy.closed"
  | "eval.run.terminal"
  | "optimizer.run.terminal"
  | "run.terminal"
  | "run.view.v2"
  | "trace.sealed"
  | "relay.journal.closed";

export type PolicyCallClosureReason =
  | "producer_completed"
  | "producer_invalid_response"
  | "producer_timed_out"
  | "producer_cancelled"
  | "producer_aborted"
  | "parent_terminal_before_policy_close"
  | "trace_closed_before_policy_close";

/**
 * What the producer said about its own model-call capture, verbatim from the
 * Trace V5 `completeness`/`coverage` contract.
 *
 * A trace that recorded no policy spans is not the same fact as a rollout that
 * made no model calls, and neither is an aborted call. `unknown` is for a
 * source that declares nothing — a live relay — and is never upgraded by
 * counting frames, actions or anything else the viewer can see.
 */
export const POLICY_CALL_COVERAGES = ["complete", "partial", "unavailable", "unknown"] as const;

export type PolicyCallCoverage = (typeof POLICY_CALL_COVERAGES)[number];

export function normalizePolicyCallCoverage(value: unknown): PolicyCallCoverage {
  const candidate = String(value ?? "").toLowerCase();
  return (POLICY_CALL_COVERAGES as readonly string[]).includes(candidate)
    ? (candidate as PolicyCallCoverage)
    : "unknown";
}

/** Coverage that means the producer did not record call boundaries. */
export function policyCallsWereCaptured(coverage: PolicyCallCoverage): boolean {
  return coverage === "complete";
}

export type PolicyCallClosure = {
  outcome: PolicyCallOutcome;
  reason: PolicyCallClosureReason;
  source: PolicyCallClosureSource;
  sourceSequence: number | null;
};

type Json = Record<string, unknown>;

function normalizedOutcome(payload: Json): PolicyCallOutcome {
  const value = String(payload.outcome ?? payload.status ?? "").toLowerCase();
  if (value === "invalid_response" || value === "invalid-response") return "invalid_response";
  if (value === "timed_out" || value === "timed-out" || value === "timeout") return "timed_out";
  if (value === "cancelled" || value === "canceled") return "cancelled";
  if (value === "aborted") return "aborted";
  return "completed";
}

export function producerPolicyCallClosure(
  payload: Json,
  sourceSequence: number | null
): PolicyCallClosure {
  const outcome = normalizedOutcome(payload);
  const reason: PolicyCallClosureReason = outcome === "completed"
    ? "producer_completed"
    : outcome === "invalid_response"
      ? "producer_invalid_response"
      : outcome === "timed_out"
        ? "producer_timed_out"
        : outcome === "cancelled"
          ? "producer_cancelled"
          : "producer_aborted";
  return { outcome, reason, source: "span.policy.closed", sourceSequence };
}

export function parentTerminalPolicyCallClosure(
  source: Extract<PolicyCallClosureSource, "eval.run.terminal" | "optimizer.run.terminal" | "run.terminal" | "run.view.v2">,
  sourceSequence: number | null
): PolicyCallClosure {
  return {
    outcome: "aborted",
    reason: "parent_terminal_before_policy_close",
    source,
    sourceSequence
  };
}

export function closedTracePolicyCallClosure(
  source: Extract<PolicyCallClosureSource, "trace.sealed" | "relay.journal.closed">
): PolicyCallClosure {
  return {
    outcome: "aborted",
    reason: "trace_closed_before_policy_close",
    source,
    sourceSequence: null
  };
}

export function parentTerminalEventKind(kind: string):
  | "eval.run.terminal"
  | "optimizer.run.terminal"
  | "run.terminal"
  | null {
  if (kind === "eval.run.terminal" || kind === "optimizer.run.terminal" || kind === "run.terminal") {
    return kind;
  }
  return null;
}
