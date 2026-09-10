/** Honest Harbor attempt phases projected from the durable Containers stream. */

import type { LiveEvalEvent } from "./types.ts";

export type HarborAttemptPhase =
  | "preflight"
  | "extracting"
  | "agent_running"
  | "submission_captured"
  | "verifier_running"
  | "scored"
  | "unscored"
  | "failed"
  | "unavailable";

export type HarborAttempt = {
  key: string;
  instruction?: string;
  sandbox?: string;
  trialId?: string;
  environmentReleaseId?: string;
  environmentStatus?: string;
  prewarmState?: string;
  runnable?: boolean;
  phase: HarborAttemptPhase;
  reward?: number | null;
  verifierScript?: string;
  reason?: string;
};

function payloadOf(event: LiveEvalEvent): Record<string, unknown> {
  return event.payload && typeof event.payload === "object"
    ? event.payload as Record<string, unknown>
    : {};
}

function releaseDetails(payload: Record<string, unknown>): {
  id?: string;
  status?: string;
  prewarmState?: string;
  runnable?: boolean;
} {
  const release = payload.environment_release;
  if (!release || typeof release !== "object") return {};
  const values = release as Record<string, unknown>;
  const prewarm = values.prewarm;
  return {
    id: typeof values.environment_release_id === "string" ? values.environment_release_id : undefined,
    status: typeof values.status === "string" ? values.status : undefined,
    prewarmState:
      prewarm && typeof prewarm === "object" && typeof (prewarm as Record<string, unknown>).state === "string"
        ? (prewarm as Record<string, unknown>).state as string
        : undefined,
    runnable: typeof values.runnable === "boolean" ? values.runnable : undefined,
  };
}

function declaredTrialKey(payload: Record<string, unknown>): string | undefined {
  for (const key of ["trial_id", "trialId", "attempt_id", "task_instance_id", "trial_image_id"]) {
    const value = payload[key];
    if (typeof value === "string" && value) return value;
  }
  return undefined;
}

/**
 * Supports both the generic Harbor events and current sibling-container
 * events. The latter are not coerced into a `verifier` record they never
 * emitted; their score comes only from a native `reward_signal`.
 */
export function projectHarborAttempts(events: LiveEvalEvent[]): HarborAttempt[] {
  const attempts = new Map<string, HarborAttempt>();
  let anonymous = 0;
  let activeKey: string | undefined;
  const ensure = (payload: Record<string, unknown>, phase: HarborAttemptPhase): HarborAttempt => {
    // Older generic Harbor events do not carry an attempt id on every event.
    // Keep those events on the current attempt rather than rendering a card per
    // event; a declared id always wins and can begin a new attempt.
    const key = declaredTrialKey(payload) ?? activeKey ?? `attempt_${++anonymous}`;
    const current = attempts.get(key) ?? { key, phase };
    const release = releaseDetails(payload);
    const next: HarborAttempt = {
      ...current,
      phase,
      trialId: typeof payload.trial_id === "string" ? payload.trial_id : current.trialId,
      instruction: typeof payload.instruction === "string" ? payload.instruction : current.instruction,
      sandbox: typeof payload.sandbox === "string" ? payload.sandbox : current.sandbox,
      environmentReleaseId: release.id ?? current.environmentReleaseId,
      environmentStatus: release.status ?? current.environmentStatus,
      prewarmState: release.prewarmState ?? current.prewarmState,
      runnable: release.runnable ?? current.runnable,
    };
    attempts.set(key, next);
    activeKey = key;
    return next;
  };
  const current = (): HarborAttempt | undefined => activeKey ? attempts.get(activeKey) : undefined;
  const replaceActive = (patch: Partial<HarborAttempt>): void => {
    const active = current();
    if (active) attempts.set(active.key, { ...active, ...patch });
  };

  for (const event of events) {
    const payload = payloadOf(event);
    switch (event.kind) {
      case "trial.planned":
        ensure(payload, "preflight");
        break;
      case "trial.launched":
        ensure(payload, "agent_running");
        break;
      case "env.episode.opened":
        ensure(payload, "preflight");
        break;
      case "nested.workspace.extracted":
        replaceActive({ phase: "extracting" });
        break;
      case "span.agent.opened":
      case "span.policy.opened":
        replaceActive({ phase: "agent_running" });
        break;
      case "submission.captured":
      case "nested.collected":
      case "nested.candidate.staged":
        replaceActive({ phase: "submission_captured" });
        break;
      case "span.verifier.opened":
      case "nested.verified":
        replaceActive({ phase: "verifier_running" });
        break;
      case "verifier": {
        const attempt = ensure(payload, "scored");
        const reward = payload["reward.txt"];
        attempts.set(attempt.key, {
          ...attempt,
          phase: typeof reward === "number" && Number.isFinite(reward) ? "scored" : "unscored",
          reward: typeof reward === "number" && Number.isFinite(reward) ? reward : null,
          verifierScript: typeof payload.script === "string" ? payload.script : attempt.verifierScript,
        });
        break;
      }
      case "reward_signal": {
        const reward = payload.value;
        replaceActive({
          phase: typeof reward === "number" && Number.isFinite(reward) ? "scored" : "unscored",
          reward: typeof reward === "number" && Number.isFinite(reward) ? reward : null,
        });
        break;
      }
      case "status": {
        const status = String(payload.status ?? "").toLowerCase();
        if (status === "failed" || status === "cancelled" || status === "timeout") {
          replaceActive({ phase: "failed", reason: typeof payload.reason === "string" ? payload.reason : status });
        } else if (status === "unavailable" || status === "refused") {
          replaceActive({ phase: "unavailable", reason: typeof payload.reason === "string" ? payload.reason : status });
        } else if (status === "completed") {
          const active = current();
          if (active && active.phase !== "scored") replaceActive({ phase: "unscored" });
        }
        break;
      }
      default:
        break;
    }
  }
  return [...attempts.values()];
}
