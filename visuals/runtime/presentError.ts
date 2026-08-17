/**
 * One structured projection from a visual-runtime rejection to text a person
 * or agent can act on. `String({code, message})` is `[object Object]`; that is
 * the failure this helper exists to make impossible.
 */

export type PresentedRuntimeError = {
  code?: string;
  message: string;
  remediation?: string;
};

const FALLBACK = "Visual runtime failed";

function stringField(value: Record<string, unknown>, ...names: string[]): string | undefined {
  for (const name of names) {
    const candidate = value[name];
    if (typeof candidate === "string" && candidate.trim() && candidate !== "[object Object]") {
      return candidate.trim();
    }
  }
  return undefined;
}

export function presentRuntimeError(reason: unknown, fallback = FALLBACK): PresentedRuntimeError {
  if (reason instanceof Error) {
    const message = reason.message.trim();
    if (message && message !== "[object Object]") return { message };
    return { message: fallback };
  }
  if (typeof reason === "string") {
    const message = reason.trim();
    if (message && message !== "[object Object]") return { message };
    return { message: fallback };
  }
  if (reason && typeof reason === "object") {
    const value = reason as Record<string, unknown>;
    const code = stringField(value, "code");
    const message = stringField(value, "safeMessage", "safe_message", "message", "error", "reason");
    const remediation = stringField(value, "remediation");
    if (message) return { code, message, remediation };
    if (code) return { code, message: `${fallback} (${code})`, remediation };
  }
  return { message: fallback };
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
