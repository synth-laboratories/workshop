import type { CodexActivityEvent, Session } from "@synth/runtime-protocol";
import type { CodexBridge, CodexSessionStart, CodexTurnFailure } from "../bridge";
import {
	approvalModeConfig,
	approvalModeFromConfig,
	codexStartRequest,
	type ApprovalMode
} from "./nativeCodex";
import { publicError } from "../runtime/publicError";

/**
 * User-facing copy for a lost app-server. The typed code and the session id
 * stay in debug logs; a raw UUID in a toast tells an operator nothing.
 */
export const AGENT_DISCONNECTED_MESSAGE =
	"The local agent process disconnected before the turn started. Retry to reconnect.";

export const CODEX_SESSION_UNHEALTHY = "codex_session_unhealthy";

/** Legacy untyped rejections from the pre-`codex_turn_send` bridge path. */
export const DETACHED_ERROR_TEXT =
	/codex session not started|is not attached|app-server (stopped|stdout closed)/i;

/** A user message that reached no app-server, kept so it can be retried. */
export type FailedSend = {
	sessionId: string;
	text: string;
	messageId: string;
	message: string;
};

/** Normalizes both the typed Tauri rejection and any thrown Error. */
export function codexTurnFailure(sessionId: string, reason: unknown): CodexTurnFailure {
	if (reason && typeof reason === "object" && !(reason instanceof Error)) {
		const value = reason as Partial<CodexTurnFailure> & { error?: unknown };
		const nested = value.error && typeof value.error === "object" ? value.error as Partial<CodexTurnFailure> : undefined;
		const message = typeof value.message === "string"
			? value.message
			: typeof nested?.message === "string"
				? nested.message
				: "The turn could not be started.";
		const detail = typeof value.detail === "string"
			? value.detail
			: typeof nested?.detail === "string"
				? nested.detail
				: message;
		return {
			code: typeof value.code === "string" ? value.code : typeof nested?.code === "string" ? nested.code : "codex_turn_start_failed",
			message,
			sessionId: typeof value.sessionId === "string" ? value.sessionId : typeof nested?.sessionId === "string" ? nested.sessionId : sessionId,
			detail
		};
	}
	const message = publicError(reason);
	return {
		code: DETACHED_ERROR_TEXT.test(message) ? "codex_session_detached" : "codex_turn_start_failed",
		message,
		sessionId,
		detail: message
	};
}

export function turnFailureMessage(failure: CodexTurnFailure): string {
	if (failure.code === "codex_session_detached" || failure.code === CODEX_SESSION_UNHEALTHY || DETACHED_ERROR_TEXT.test(failure.message)) {
		return AGENT_DISCONNECTED_MESSAGE;
	}
	return failure.message;
}

export function isCodexCompactionEvent(event: {
	method: string;
	params: Record<string, unknown>;
}): boolean {
	if (event.method === "thread/compacted") return true;
	const item = event.params.item;
	return Boolean(
		item && typeof item === "object" && (item as Record<string, unknown>).type === "contextCompaction"
	);
}

/** Rebuild a start request for an existing session (compaction path — no model switch). */
export async function codexResumeRequest(
	nativeCodex: CodexBridge,
	session: Session,
	autoCompactTokenLimits: Record<string, number>,
	localBaseUrl?: string
): Promise<CodexSessionStart> {
	if (session.metadata.runtime !== "codex-app-server") {
		throw new Error(`Session ${session.id} is not owned by Codex app-server`);
	}
	const workspace =
		typeof session.metadata.workspace === "string"
			? session.metadata.workspace
			: await nativeCodex.defaultWorkspace();
	const storedApprovalMode =
		typeof session.metadata.approvalMode === "string"
			? (session.metadata.approvalMode as ApprovalMode)
			: approvalModeFromConfig(
					typeof session.metadata.approvalPolicy === "string"
						? session.metadata.approvalPolicy
						: undefined,
					typeof session.metadata.sandbox === "string" ? session.metadata.sandbox : undefined
				);
	const storedApproval = approvalModeConfig(storedApprovalMode);
	return {
		...codexStartRequest(
			session.id,
			workspace,
			session.target,
			"ask",
			autoCompactTokenLimits,
			localBaseUrl
		),
		approvalPolicy:
			typeof session.metadata.approvalPolicy === "string"
				? session.metadata.approvalPolicy
				: storedApproval.approvalPolicy,
		sandbox:
			typeof session.metadata.sandbox === "string"
				? session.metadata.sandbox
				: storedApproval.sandbox,
		threadId: typeof session.metadata.threadId === "string" ? session.metadata.threadId : undefined
	};
}

export function appendCodexActivity(
	events: CodexActivityEvent[],
	event: CodexActivityEvent
): CodexActivityEvent[] {
	if (
		events.some(
			(candidate) =>
				candidate.executionId === event.executionId && candidate.streamId === event.streamId
		)
	) {
		return events;
	}
	return [...events, event];
}

export function truncate(label: string, max = 22) {
	if (label.length <= max) return label;
	return `${label.slice(0, max - 1)}…`;
}

export function desktopBootError(reason: unknown): string {
	const message = reason instanceof Error
		? reason.message
		: typeof reason === "string"
			? reason
			: reason && typeof reason === "object"
				? (() => {
					const detail = reason as { message?: unknown; error?: unknown; detail?: unknown };
					if (typeof detail.message === "string") return detail.message;
					if (typeof detail.error === "string") return detail.error;
					if (typeof detail.detail === "string") return detail.detail;
					try { return JSON.stringify(reason); } catch { return "Unknown desktop runtime error"; }
				})()
				: publicError(reason);
	if (/command\s+.+not found|unknown command/i.test(message)) {
		return "Desktop backend was updated; fully quit and reopen Synth Desktop.";
	}
	return message;
}
