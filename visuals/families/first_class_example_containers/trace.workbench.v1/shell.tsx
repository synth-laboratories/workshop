/**
 * Family-agnostic trace workstation.
 *
 * The generic instantiation of `_shared/traceWorkbench.tsx`: title and kicker
 * come from the bound run's summary task rather than a hardcoded family name,
 * and the family is declared non-frame-centric — a producer that never emits
 * frames (liveFrames "unsupported", post_hoc families, imported seals) gets an
 * honest stream-events-only surface instead of Craftax frame-absence copy.
 * Which runs this template attaches to is the backend's decision; this shell
 * only has to render whatever run it is bound.
 */

import {
  TraceWorkbench,
  type TraceWorkbenchProps
} from "../_shared/traceWorkbench.tsx";

export type ShellProps = TraceWorkbenchProps;

function taskLabel(props: ShellProps): string {
  const run = (props.run ?? props.data?.run ?? null) as Record<string, any> | null;
  const summary = run?.summary && typeof run.summary === "object"
    ? run.summary as Record<string, unknown>
    : {};
  const candidate = [summary.task, summary.taskId, summary.family, run?.projectRef]
    .find((value): value is string => typeof value === "string" && value.trim().length > 0);
  return candidate?.trim() ?? "Trace";
}

export function Shell(props: ShellProps) {
  const task = taskLabel(props);
  return (
    <TraceWorkbench
      {...props}
      branding={{
        label: task,
        defaultTitle: `${task} trace workstation`,
        testId: "trace-workbench",
        aggregatesTestId: "trace-run-aggregates",
        frameTestId: "trace-native-frame",
        frameCentric: false
      }}
    />
  );
}

export default Shell;
