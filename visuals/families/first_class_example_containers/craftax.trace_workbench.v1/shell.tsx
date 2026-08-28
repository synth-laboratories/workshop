/**
 * Craftax trace workstation — the Craftax specialization of the shared trace
 * workbench. Craftax is frame-centric: its container emits native PNGs, and a
 * missing frame is a defect worth naming, so the native-frame-unavailable copy
 * stays. Everything else lives in `_shared/traceWorkbench.tsx`.
 */

import {
  TraceWorkbench,
  type TraceWorkbenchProps
} from "../_shared/traceWorkbench.tsx";

export type ShellProps = TraceWorkbenchProps;

export function Shell(props: ShellProps) {
  return (
    <TraceWorkbench
      {...props}
      branding={{
        label: "Craftax",
        defaultTitle: "Craftax trace workstation",
        testId: "craftax-trace-workbench",
        aggregatesTestId: "craftax-run-aggregates",
        frameTestId: "craftax-native-frame",
        frameCentric: true
      }}
    />
  );
}

export default Shell;
