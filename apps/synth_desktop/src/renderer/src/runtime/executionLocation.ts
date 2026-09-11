import type { ScopeView } from "../generated/protocol";

export type ExecutionLocationPresentation = {
  location: "local" | "cloud" | "unknown";
  label: string;
  description: string;
  cloudCreationEnabled: boolean;
  cloudReason: string | null;
};

/** Actor ownership determines location; hosted models still serve Local
 * native sessions. Ready identity alone never enables creation transport. */
export function executionLocationPresentation(input: {
  sessionKind?: string;
  selectedTargetId: string;
  scope: ScopeView;
  creationTransportReady: boolean;
}): ExecutionLocationPresentation {
  const location = input.sessionKind === undefined
    ? (input.selectedTargetId === "intern-sync" || input.selectedTargetId === "intern-async" ? "cloud" : "local")
    : input.sessionKind === "codex" ? "local" : input.sessionKind === "intern" ? "cloud" : "unknown";
  const cloudCreationEnabled = input.scope.availability === "ready" && input.creationTransportReady;
  return {
    location,
    label: location === "local" ? "Local" : location === "cloud" ? "Cloud" : "Location unavailable",
    description: location === "local"
      ? "The agent runs on this computer. Its model may use a hosted provider."
      : location === "cloud" ? "The agent runs in a Cloud runtime." : "This session's execution location could not be verified.",
    cloudCreationEnabled,
    cloudReason: cloudCreationEnabled ? null : input.scope.availability === "signed_out"
      ? "Sign in to use Cloud execution." : "Cloud execution is unavailable for new conversations."
  };
}
