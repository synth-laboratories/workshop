import type { ExecutionTarget, RuntimeEvent, Session } from "@synth/runtime-protocol";
import type { CodexEvent, CodexSessionStart, PersistedCodexSession } from "../env";

export function codexStartRequest(
	sessionId: string,
	workspace: string,
	target: ExecutionTarget
): CodexSessionStart {
	if (target.kind === "intern") throw new Error("Intern sessions are owned by Synth Cloud");
	if (target.kind === "local") {
		return {
			sessionId, workspace, baseUrl: "http://127.0.0.1:7333", apiKey: "",
			model: "poolside/Laguna-XS-2.1-NVFP4-mlx", providerName: "local-laguna",
			providerTitle: "Laguna XS Responses", providerEnvKey: "SYNTH_LAGUNA_API_KEY",
			approvalPolicy: "never", sandbox: "workspace-write"
		};
	}
	if (target.kind !== "remote") throw new Error("Unsupported Codex execution target");
	return {
		sessionId, workspace, baseUrl: "https://openrouter.ai/api/v1", apiKey: "",
		model: target.model, providerName: "openrouter", providerTitle: "OpenRouter Responses",
		providerEnvKey: "OPENROUTER_API_KEY", approvalPolicy: "never", sandbox: "workspace-write"
	};
}

export function createCodexSession(
	id: string, target: ExecutionTarget, projectId: string | null, title?: string
): Session {
	const now = new Date().toISOString();
	return {
		id, title: title || (target.kind === "local" ? "Laguna XS" : target.kind === "remote" ? target.model : "Intern"), target,
		projectId, createdAt: now, updatedAt: now, status: "ready", latestCursor: 0,
		metadata: { runtime: "codex-app-server" }
	};
}

export function restoreCodexSession(value: PersistedCodexSession): Session {
	const now = new Date().toISOString();
	const local = value.providerName === "local-laguna";
	const target: ExecutionTarget = local
		? { kind: "local", model: "laguna-xs-2.1", adapter: null }
		: { kind: "remote", provider: "openrouter", model: value.model, adapter: null };
	const allowedStatuses = new Set<Session["status"]>([
		"created", "ready", "running", "waiting_for_input", "paused", "completed", "failed", "cancelled", "configuration_required"
	]);
	const status = allowedStatuses.has(value.status as Session["status"])
		? value.status as Session["status"]
		: "ready";
	return {
		id: value.sessionId,
		title: local ? "Laguna XS" : value.model,
		target,
		projectId: null,
		createdAt: now,
		updatedAt: now,
		status,
		latestCursor: 0,
		metadata: {
			runtime: "codex-app-server",
			threadId: value.threadId,
			workspace: value.workspace,
			providerTitle: value.providerTitle,
			baseUrl: value.baseUrl
		}
	};
}

function textValue(params: Record<string, unknown>): string | undefined {
	const candidates: unknown[] = [params.delta, params.text, params.message, params.content];
	const item = params.item;
	if (item && typeof item === "object") {
		const value = item as Record<string, unknown>;
		candidates.push(value.delta, value.text, value.message, value.content);
	}
	return candidates.find((value): value is string => typeof value === "string" && value.length > 0);
}

export function codexEventToRuntime(event: CodexEvent, sequence: number): RuntimeEvent {
	const method = event.method;
	const text = textValue(event.params);
	const lower = method.toLowerCase();
	let eventKind = method;
	if (lower.includes("agentmessage") || lower.includes("agent_message")) {
		eventKind = lower.includes("delta") ? "message.delta" : lower.includes("completed") ? "message.completed" : "message.created";
	} else if (lower.includes("reasoning")) eventKind = "agent.reasoning";
	else if (lower.includes("commandexecution")) eventKind = "command.execution";
	else if (lower.includes("filechange")) eventKind = "file.change";
	else if (lower === "turn/completed") eventKind = "run.completed";
	else if (lower === "turn/failed") eventKind = "run.failed";
	else if (lower === "turn/interrupted") eventKind = "run.cancelled";
	else if (lower === "turn/started") eventKind = "run.started";
	return {
		schemaVersion: "synth.desktop-runtime-event.v1", sessionId: event.sessionId,
		sequence, eventKind, payload: text ? { ...event.params, [eventKind === "message.delta" ? "delta" : "content"]: text } : event.params,
		createdAt: new Date().toISOString(), source: "local"
	};
}
