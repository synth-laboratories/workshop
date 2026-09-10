/**
 * One safe projection for both the closed FailureView contract and local or
 * compatibility boundary failures. FailureView remains authoritative when it
 * is present; Error/string values and older AppError envelopes still need to
 * remain legible while every producer migrates to that contract.
 */

const SECRET = /\b(?:sk|sess|key|tok)-[A-Za-z0-9_-]{12,}\b/g;
const MAX_LENGTH = 320;
const SCHEMA = "synth.failure-view.v1";

export type FailureRemediationView = {
	kind: string;
	label: string;
	containerId?: string | null;
	sessionId?: string | null;
	resumeToken?: string | null;
	settingsRoute?: string | null;
	resourceRef?: string | null;
};

export type FailureView = {
	schemaVersion: string;
	failureId: string;
	code: string;
	category: string;
	disposition: string;
	lifecycleState: string;
	operation: string;
	phase: string;
	message: string;
	remediation?: FailureRemediationView | null;
	safeContext: {
		sessionId?: string | null;
		containerId?: string | null;
		evaluationId?: string | null;
		rolloutId?: string | null;
		visualId?: string | null;
		operationId?: string | null;
		facts?: unknown;
	};
	diagnosticReference: string;
};

export type PublicError = {
	code?: string;
	message: string;
	remediation?: string;
	retryable?: boolean;
	failureId?: string;
	disposition?: string;
	lifecycleState?: string;
	state?: string;
	source?: "cloud" | "local";
	warmOperationId?: string;
	retryAfterSeconds?: number;
	elapsedMs?: number;
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

function isFailureView(value: Record<string, unknown>): value is FailureView & Record<string, unknown> {
	return value.schemaVersion === SCHEMA || value.schema_version === SCHEMA;
}

function fromFailureView(view: Record<string, unknown>): PublicError {
	const nested = view.failure && typeof view.failure === "object"
		? view.failure as Record<string, unknown>
		: view;
	const code = typeof nested.code === "string" ? nested.code : undefined;
	const message = typeof nested.message === "string" && nested.message.trim()
		? nested.message.trim()
		: "The operation failed.";
	const remediation = nested.remediation && typeof nested.remediation === "object"
		? String((nested.remediation as { label?: string }).label ?? "")
		: typeof nested.remediation === "string" ? nested.remediation : undefined;
	const disposition = typeof nested.disposition === "string" ? nested.disposition : undefined;
	return {
		code,
		message: redact(message),
		remediation: remediation ? redact(remediation) : undefined,
		retryable: typeof nested.retryable === "boolean"
			? nested.retryable
			: disposition === "retryable" || disposition === "approval_required",
		failureId: typeof nested.failureId === "string" ? nested.failureId : typeof nested.failure_id === "string" ? nested.failure_id : undefined,
		disposition,
		lifecycleState: typeof nested.lifecycleState === "string" ? nested.lifecycleState : undefined
	};
}

function fromCompatibilityEnvelope(value: Record<string, unknown>, fallback: string): PublicError {
	const detail = value.detail && typeof value.detail === "object"
		? value.detail as Record<string, unknown>
		: null;
	const envelope = detail ?? value;
	const nested = envelope.error && typeof envelope.error === "object"
		? envelope.error as Record<string, unknown>
		: value.error && typeof value.error === "object"
			? value.error as Record<string, unknown>
			: null;
	// Developer-only `detail` strings are deliberately excluded. A safe field
	// or stable code must exist before any boundary payload reaches the UI.
	const message = field(envelope, "safeMessage", "safe_message", "message", "reason")
		?? (nested ? field(nested, "message") : undefined)
		?? (detail ? field(value, "safeMessage", "safe_message", "message", "reason") : undefined);
	const code = (nested ? field(nested, "code") : undefined)
		?? field(envelope, "code", "error_code")
		?? (detail ? field(value, "code", "error_code") : undefined);
	const remediation = field(envelope, "remediation")
		?? (nested ? field(nested, "remediation") : undefined)
		?? (detail ? field(value, "remediation") : undefined);
	const retryable = typeof envelope.retryable === "boolean"
		? envelope.retryable
		: nested && typeof nested.retryable === "boolean"
			? nested.retryable
			: undefined;
	const state = nested ? field(nested, "state") : undefined;
	const source = nested && (nested.source === "cloud" || nested.source === "local")
		? nested.source
		: undefined;
	const warmOperationId = nested ? field(nested, "warm_operation_id", "warmOperationId") : undefined;
	const elapsedMs = nested && typeof nested.elapsed_ms === "number" ? nested.elapsed_ms : undefined;
	const retryAfterSeconds = typeof envelope.retry_after_seconds === "number"
		? envelope.retry_after_seconds
		: nested && typeof nested.retry_after_seconds === "number"
			? nested.retry_after_seconds
			: undefined;

	if (code === "inference_provider_capacity_pending" && retryable) {
		return {
			code,
			message: "Waiting for Synth Cloud GPU capacity.",
			remediation: "Retry after the indicated interval; your prompt is preserved and the same warm operation continues.",
			retryable: true,
			state,
			source: source ?? "cloud",
			warmOperationId,
			retryAfterSeconds,
			elapsedMs
		};
	}
	if (code === "inference_target_not_ready" && retryable) {
		return {
			code,
			message: "The hosted model is warming up.",
			remediation: "Retry in a moment; your workspace and prompt are preserved.",
			retryable: true
		};
	}
	if (message) {
		return {
			code,
			message: redact(message),
			remediation: remediation ? redact(remediation) : undefined,
			retryable
		};
	}
	if (code) {
		return {
			code,
			message: redact(`${fallback} (${code})`),
			remediation: remediation ? redact(remediation) : undefined,
			retryable
		};
	}
	return { message: fallback };
}

export function toPublicError(reason: unknown, fallback = "The operation failed."): PublicError {
	if (reason instanceof Error) return { message: redact(reason.message) || fallback };
	if (typeof reason === "string") return { message: redact(reason) || fallback };
	if (reason && typeof reason === "object") {
		const value = reason as Record<string, unknown>;
		if (value.failure && typeof value.failure === "object") {
			return fromFailureView(value);
		}
		if (isFailureView(value)) {
			return fromFailureView(value);
		}
		return fromCompatibilityEnvelope(value, fallback);
	}
	return { message: fallback };
}

export function publicError(reason: unknown, fallback = "The operation failed."): string {
	const { code, message, remediation } = toPublicError(reason, fallback);
	const parts = [message];
	if (remediation && remediation !== message) parts.push(remediation);
	const line = parts.join(" ");
	if (!code || code === "internal") return line;
	return line.includes(code) ? line : `${line} (${code})`;
}
