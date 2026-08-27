/**
 * Closed FailureView contract. Malformed envelopes become failure_contract_invalid.
 * Domain classification does not happen here.
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
};

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
	const message = typeof nested.message === "string" ? nested.message : "The operation failed.";
	const remediation = nested.remediation && typeof nested.remediation === "object"
		? String((nested.remediation as { label?: string }).label ?? "")
		: typeof nested.remediation === "string" ? nested.remediation : undefined;
	const disposition = typeof nested.disposition === "string" ? nested.disposition : undefined;
	return {
		code,
		message: redact(message),
		remediation: remediation ? redact(remediation) : undefined,
		retryable: disposition === "retryable" || disposition === "approval_required",
		failureId: typeof nested.failureId === "string" ? nested.failureId : typeof nested.failure_id === "string" ? nested.failure_id : undefined,
		disposition,
		lifecycleState: typeof nested.lifecycleState === "string" ? nested.lifecycleState : undefined
	};
}

export function toPublicError(reason: unknown, fallback = "The operation failed."): PublicError {
	if (reason instanceof Error) return { code: "failure_contract_invalid", message: redact(reason.message) || fallback };
	if (typeof reason === "string") return { code: "failure_contract_invalid", message: redact(reason) || fallback };
	if (reason && typeof reason === "object") {
		const value = reason as Record<string, unknown>;
		if (value.failure && typeof value.failure === "object") {
			return fromFailureView(value);
		}
		if (isFailureView(value) || typeof value.code === "string") {
			return fromFailureView(value);
		}
		return {
			code: "failure_contract_invalid",
			message: fallback
		};
	}
	return { code: "failure_contract_invalid", message: fallback };
}

export function publicError(reason: unknown, fallback = "The operation failed."): string {
	const { code, message, remediation } = toPublicError(reason, fallback);
	const parts = [message];
	if (remediation && remediation !== message) parts.push(remediation);
	const line = parts.join(" ");
	if (!code || code === "internal") return line;
	return line.includes(code) ? line : `${line} (${code})`;
}
