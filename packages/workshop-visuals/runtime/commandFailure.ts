/**
 * Structured command-failure summaries for trace surfaces.
 *
 * The 2026-09-03 DeepSWE sample failed every task with one cause — `bwrap: No
 * permissions to create a new namespace` on the first command of every turn —
 * and the trace workstation showed only "this call recorded no answer". The
 * cause was present in the relayed events all along, inside raw producer JSON
 * that a reader had to expand one call at a time.
 *
 * This projects those events into a named, counted failure with a remedy, so
 * the dominant cause is legible without opening JSON.
 */

type Json = Record<string, unknown>;

export type CommandFailureReason =
  | "sandbox_namespace_unavailable"
  | "command_not_found"
  | "permission_denied"
  | "command_failed";

export type CommandFailure = {
  rolloutId: string | null;
  command: string | null;
  exitCode: number | null;
  output: string;
  reason: CommandFailureReason;
};

export type CommandFailureGroup = {
  reason: CommandFailureReason;
  /** One line a reader can act on. */
  title: string;
  /** What to change. Empty when the class has no single known remedy. */
  remedy: string | null;
  /** Whether the class is an environment fault rather than a model mistake. */
  infrastructure: boolean;
  occurrences: number;
  rolloutIds: string[];
  sample: CommandFailure;
};

export type CommandFailureSummary = {
  total: number;
  groups: CommandFailureGroup[];
  /** The class that explains the most failures, when there is one. */
  dominant: CommandFailureGroup | null;
};

const NAMESPACE_DENIAL = [
  "No permissions to create a new namespace",
  "unprivileged user namespaces",
  "unprivileged_userns_clone"
];

const CLASSES: Record<CommandFailureReason, { title: string; remedy: string | null; infrastructure: boolean }> = {
  sandbox_namespace_unavailable: {
    title: "The inner agent sandbox could not be created",
    remedy:
      "The task container is already the isolation boundary and cannot nest a Linux user namespace. The effective policy sandbox must be danger-full-access; a workspace-write policy config fails every command before the model sees the workspace.",
    infrastructure: true
  },
  command_not_found: {
    title: "A command the agent ran is not installed in the task container",
    remedy: "Bake the missing tool into the task image, or have the policy use one that is present.",
    infrastructure: true
  },
  permission_denied: {
    title: "The task container refused a filesystem operation",
    remedy: "Check the workspace mount permissions and the user the agent runs as.",
    infrastructure: true
  },
  command_failed: {
    title: "A command the agent ran exited non-zero",
    remedy: null,
    infrastructure: false
  }
};

function object(value: unknown): Json | null {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Json : null;
}

function text(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

/** Accept an optimizer envelope, its delta, or a bare container event. */
function containerEvent(value: unknown): Json | null {
  const row = object(value);
  if (!row) return null;
  const nested =
    object(object(row.delta)?.container_event)
    ?? object(object(row.delta)?.containerEvent)
    ?? object(row.container_event)
    ?? object(row.containerEvent);
  return nested ?? row;
}

function classify(output: string): CommandFailureReason {
  if (NAMESPACE_DENIAL.some((signature) => output.includes(signature))) {
    return "sandbox_namespace_unavailable";
  }
  if (/command not found|: not found\b|No such file or directory/i.test(output)) {
    return "command_not_found";
  }
  if (/Permission denied|Operation not permitted/i.test(output)) return "permission_denied";
  return "command_failed";
}

function commandOutput(item: Json): string {
  return ["aggregated_output", "output", "stdout", "stderr", "text"]
    .map((key) => text(item[key]))
    .filter((value): value is string => value !== null)
    .join("\n");
}

/** Read one failed `command_execution` item out of a policy data event. */
function failureFrom(event: Json): CommandFailure | null {
  if (text(event.kind) !== "span.policy.data") return null;
  const payload = object(event.payload);
  const item = object(object(payload?.event)?.item);
  if (!item) return null;
  if (text(item.type) !== "command_execution") return null;
  if (text(item.status) !== "failed") return null;
  const output = commandOutput(item);
  const exitCode = typeof item.exit_code === "number" ? item.exit_code : null;
  return {
    rolloutId: text(event.rollout_id) ?? text(event.rolloutId),
    command: text(item.command),
    exitCode,
    output,
    reason: classify(output)
  };
}

export function projectCommandFailures(events: unknown[]): CommandFailureSummary {
  const failures: CommandFailure[] = [];
  for (const value of events ?? []) {
    const event = containerEvent(value);
    if (!event) continue;
    const failure = failureFrom(event);
    if (failure) failures.push(failure);
  }
  const byReason = new Map<CommandFailureReason, CommandFailureGroup>();
  for (const failure of failures) {
    const existing = byReason.get(failure.reason);
    if (existing) {
      existing.occurrences += 1;
      if (failure.rolloutId && !existing.rolloutIds.includes(failure.rolloutId)) {
        existing.rolloutIds.push(failure.rolloutId);
      }
      continue;
    }
    const shape = CLASSES[failure.reason];
    byReason.set(failure.reason, {
      reason: failure.reason,
      title: shape.title,
      remedy: shape.remedy,
      infrastructure: shape.infrastructure,
      occurrences: 1,
      rolloutIds: failure.rolloutId ? [failure.rolloutId] : [],
      sample: failure
    });
  }
  const groups = [...byReason.values()].sort((left, right) => {
    // An environment fault outranks a larger pile of ordinary non-zero exits:
    // it explains them, and it is the one a reader can act on.
    if (left.infrastructure !== right.infrastructure) return left.infrastructure ? -1 : 1;
    return right.occurrences - left.occurrences;
  });
  return { total: failures.length, groups, dominant: groups[0] ?? null };
}

/** The single line a summary card leads with. */
export function commandFailureHeadline(summary: CommandFailureSummary): string | null {
  const group = summary.dominant;
  if (!group) return null;
  const rollouts = group.rolloutIds.length;
  const scope = rollouts > 1 ? ` across ${rollouts} rollouts` : rollouts === 1 ? " in 1 rollout" : "";
  return `${group.title} · ${group.occurrences} command${group.occurrences === 1 ? "" : "s"}${scope}`;
}
