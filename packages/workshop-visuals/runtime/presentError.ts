/**
 * Visual runtime projection of FailureView. Unknown envelopes are
 * failure_contract_invalid — never raw transport prose.
 */

export type PresentedRuntimeError = {
  code?: string;
  message: string;
  remediation?: string;
};

const FALLBACK = "Visual runtime failed";
const SCHEMA = "synth.failure-view.v1";

export function presentRuntimeError(reason: unknown, fallback = FALLBACK): PresentedRuntimeError {
  if (reason instanceof Error) {
    return { message: reason.message.trim() || fallback };
  }
  if (reason && typeof reason === "object") {
    const value = reason as Record<string, unknown>;
    const envelope = value.failure && typeof value.failure === "object"
      ? value.failure as Record<string, unknown>
      : value;
    const schema = envelope.schemaVersion ?? envelope.schema_version;
    const code = typeof envelope.code === "string" ? envelope.code : undefined;
    const message = typeof envelope.message === "string" ? envelope.message : undefined;
    const remediation = envelope.remediation && typeof envelope.remediation === "object"
      ? String((envelope.remediation as { label?: string }).label ?? "")
      : typeof envelope.remediation === "string" ? envelope.remediation : undefined;
    if (schema === SCHEMA || code) {
      return {
        code,
        message: message || `${fallback}${code ? ` (${code})` : ""}`,
        remediation: remediation || undefined
      };
    }
    return { code: "failure_contract_invalid", message: fallback };
  }
  if (typeof reason === "string" && reason.trim() && reason !== "[object Object]") {
    return { message: reason.trim() };
  }
  return { code: "failure_contract_invalid", message: fallback };
}

export function presentRuntimeErrorMessage(reason: unknown, fallback = FALLBACK): string {
  const presented = presentRuntimeError(reason, fallback);
  const parts = [presented.message];
  if (presented.remediation && presented.remediation !== presented.message) {
    parts.push(presented.remediation);
  }
  if (presented.code && !parts.join(" ").includes(presented.code)) {
    parts.push(`(${presented.code})`);
  }
  return parts.join(" ");
}
