/**
 * One projection from a rejected boundary call to text a person can act on.
 *
 * Tauri commands reject with a structured `AppError` — a plain object carrying
 * `code`, `message` and developer-facing `detail`, never an `Error`. Coercing
 * one with `String(reason)` yields `[object Object]`, which is what the
 * container attach form rendered above its own inputs. Every boundary in the
 * renderer projects failures through here so a stable code and its remediation
 * survive, and raw transport or engine text never reaches a user surface.
 */

const SECRET = /\b(?:sk|sess|key|tok)-[A-Za-z0-9_-]{12,}\b/g;
const MAX_LENGTH = 320;

export type PublicError = {
	/** Stable, machine-readable code when the boundary supplied one. */
	code?: string;
	/** User-facing sentence. Always present. */
	message: string;
	/** What to do next, when the boundary said. */
	remediation?: string;
	/** Whether retrying the same call is meaningful. */
	retryable?: boolean;
};

function field(value: Record<string, unknown>, ...names: string[]): string | undefined {
	for (const name of names) {
		const candidate = value[name];
		if (typeof candidate === "string" && candidate.trim()) return candidate.trim();
	}
	return undefined;
}

function redact(value: string): string {
	const clean = value.replace(SECRET, "[redacted]");
	return clean.length > MAX_LENGTH ? `${clean.slice(0, MAX_LENGTH - 1)}…` : clean;
}

/** Structured projection, for surfaces that render code or remediation apart. */
export function toPublicError(reason: unknown, fallback = "The operation failed."): PublicError {
	if (reason instanceof Error) return { message: redact(reason.message) || fallback };
	if (typeof reason === "string") return { message: redact(reason) || fallback };
	if (reason && typeof reason === "object") {
		const value = reason as Record<string, unknown>;
		const nested = value.error && typeof value.error === "object"
			? value.error as Record<string, unknown>
			: null;
		// `detail` is deliberately last and never alone: it is developer-facing.
		const message = field(value, "safeMessage", "safe_message", "message", "error", "reason")
			?? (nested ? field(nested, "message") : undefined);
		const code = field(value, "code") ?? (nested ? field(nested, "code") : undefined);
		const remediation = field(value, "remediation");
		const retryable = typeof value.retryable === "boolean"
			? value.retryable
			: nested && typeof nested.retryable === "boolean" ? nested.retryable : undefined;
		if (code === "inference_target_not_ready" && retryable) {
			return {
				code,
				message: "The hosted model is warming up.",
				remediation: "Retry in a moment; your workspace and prompt are preserved.",
				retryable: true
			};
		}
		if (message) return { code, message: redact(message), remediation: remediation && redact(remediation), retryable };
		// No message field: name the failure by its code rather than dumping the
		// object, so the surface stays legible and free of arbitrary payload.
		if (code) return { code, message: redact(`${fallback} (${code})`), remediation: remediation && redact(remediation), retryable };
		return { message: fallback };
	}
	return { message: fallback };
}

/** One line for a toast, banner, or inline form error. */
export function publicError(reason: unknown, fallback = "The operation failed."): string {
	const { code, message, remediation } = toPublicError(reason, fallback);
	const parts = [message];
	if (remediation && remediation !== message) parts.push(remediation);
	const line = parts.join(" ");
	// A code the message already names adds nothing; otherwise it is how a
	// person finds the same failure in Diagnostics.
	return code && !line.includes(code) ? `${line} (${code})` : line;
}
