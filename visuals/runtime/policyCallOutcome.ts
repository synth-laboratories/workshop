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
