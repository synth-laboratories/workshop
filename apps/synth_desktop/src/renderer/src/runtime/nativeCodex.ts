import type { AppEvent, ExecutionTarget, RuntimeEvent, Session } from "@synth/runtime-protocol";
import type { CodexEvent, CodexSessionStart, PersistedCodexSession } from "../env";
import { MUSE_GLIMMER_MODEL } from "../types/landing";

export type ApprovalMode = "ask" | "accept-edits" | "allow-all";
export type ApprovalPolicy = "untrusted" | "on-request" | "never";
export type SandboxMode = "read-only" | "workspace-write" | "danger-full-access";
export type PermissionConfig = { approvalPolicy: ApprovalPolicy; sandbox: SandboxMode };

export function approvalModeFromConfig(approvalPolicy?: string, sandbox?: string): ApprovalMode {
	if (approvalPolicy === "never" && sandbox === "danger-full-access") return "allow-all";
	if (approvalPolicy === "on-request" && sandbox === "workspace-write") return "accept-edits";
	return "ask";
}

export function approvalModeConfig(mode: ApprovalMode): Pick<CodexSessionStart, "approvalPolicy" | "sandbox"> {
	switch (mode) {
		case "accept-edits": return { approvalPolicy: "on-request", sandbox: "workspace-write" };
		case "allow-all": return { approvalPolicy: "never", sandbox: "danger-full-access" };
		case "ask":
		default: return { approvalPolicy: "untrusted", sandbox: "workspace-write" };
	}
}

export function permissionConfigFromApprovalMode(mode: ApprovalMode): PermissionConfig {
	return approvalModeConfig(mode) as PermissionConfig;
}

export function codexStartRequest(
	sessionId: string, workspace: string, target: ExecutionTarget, permissions: ApprovalMode | PermissionConfig = "ask",
	autoCompactTokenLimits: Record<string, number> = { lagunaXs: 150_000, museGlimmer: 100_000, lagunaS: 250_000, luna: 250_000 }
): CodexSessionStart {
	const approval = typeof permissions === "string" ? approvalModeConfig(permissions) : permissions;
	if (target.kind === "intern") throw new Error("Intern sessions are owned by Synth Cloud");
	if (target.kind === "local") {
		// Both local models are served by the same daemon on the same port over
		// the same wire API; only the model identity and its context budget
		// differ. Sending Laguna XS's id and label for a Muse session would
		// mislabel the provider and hand Codex the wrong compaction threshold.
		const isMuse = target.model === MUSE_GLIMMER_MODEL;
		const autoCompactTokenLimit = isMuse
			? autoCompactTokenLimits.museGlimmer ?? 100_000
			: autoCompactTokenLimits.lagunaXs ?? 150_000;
		return {
			sessionId, workspace, baseUrl: "http://127.0.0.1:7333", apiKey: "",
			model: isMuse ? MUSE_GLIMMER_MODEL : "poolside/Laguna-XS-2.1-NVFP4-mlx",
			providerName: isMuse ? "local-muse-glimmer" : "local-laguna",
			providerTitle: isMuse ? "Muse Glimmer Responses" : "Laguna XS Responses",
			providerEnvKey: "SYNTH_LAGUNA_API_KEY",
			autoCompactTokenLimit, ...approval
		};
	}
	if (target.kind !== "remote") throw new Error("Unsupported Codex execution target");
	const autoCompactTokenLimit = target.model.includes("gpt-5.6-luna")
		? autoCompactTokenLimits.luna ?? 250_000
		: autoCompactTokenLimits.lagunaS ?? 250_000;
	if (target.provider === "synth-cloud") {
		return {
			// baseUrl is overwritten by the Rust host from synth_config; placeholder satisfies types.
			sessionId, workspace, baseUrl: "https://api.usesynth.ai/api/v1", apiKey: "",
			model: target.model, providerName: "synth-cloud", providerTitle: "Synth Cloud Responses",
			providerEnvKey: "SYNTH_API_KEY", autoCompactTokenLimit, ...approval
		};
	}
	return {
		sessionId, workspace, baseUrl: "https://openrouter.ai/api/v1", apiKey: "",
		model: target.model, providerName: "openrouter", providerTitle: "OpenRouter Responses",
		providerEnvKey: "OPENROUTER_API_KEY", autoCompactTokenLimit, ...approval
	};
}

export function createCodexSession(
	id: string, target: ExecutionTarget, projectId: string | null, workspace: string, title?: string, permissions: ApprovalMode | PermissionConfig = "ask"
): Session {
	const approval = typeof permissions === "string" ? approvalModeConfig(permissions) : permissions;
	const approvalMode = approvalModeFromConfig(approval.approvalPolicy, approval.sandbox);
	const now = new Date().toISOString();
	return {
		id,
		title: title || (target.kind === "local"
			? (target.model === MUSE_GLIMMER_MODEL ? "Muse Glimmer" : "Laguna XS")
			: target.kind === "remote" ? target.model : "Intern"),
		target,
		projectId, createdAt: now, updatedAt: now, status: "ready", latestCursor: 0,
		metadata: { runtime: "codex-app-server", workspace, approvalMode, ...approval }
	};
}

export function restoreCodexSession(value: PersistedCodexSession): Session {
	const now = new Date().toISOString();
	const muse = value.providerName === "local-muse-glimmer";
	const local = muse || value.providerName === "local-laguna";
	const synthCloud = value.providerName === "synth-cloud";
	const target: ExecutionTarget = local
		// A restored session must come back on the model it ran on. Treating
		// every local session as Laguna XS silently moved a Muse thread onto
		// different weights on reload.
		? { kind: "local", model: muse ? MUSE_GLIMMER_MODEL : "laguna-xs-2.1", adapter: null }
		: {
			kind: "remote",
			provider: synthCloud ? "synth-cloud" : "openrouter",
			model: value.model,
			adapter: null
		};
	const allowedStatuses = new Set<Session["status"]>([
		"created", "ready", "running", "waiting_for_input", "paused", "interrupted", "completed", "failed", "cancelled", "configuration_required"
	]);
	const status = allowedStatuses.has(value.status as Session["status"])
		? value.status as Session["status"]
		: "ready";
	return {
		id: value.sessionId,
		title: value.title || (local ? (muse ? "Muse Glimmer" : "Laguna XS") : value.model),
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
			baseUrl: value.baseUrl,
			approvalPolicy: value.approvalPolicy,
			sandbox: value.sandbox,
			approvalMode: approvalModeFromConfig(value.approvalPolicy, value.sandbox)
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

function completedTurnActuallyFailed(params: Record<string, unknown>): boolean {
	const turn = params.turn && typeof params.turn === "object"
		? params.turn as Record<string, unknown>
		: params;
	const status = typeof turn.status === "string" ? turn.status.toLowerCase() : "";
	return status === "failed" || status === "error" || ("error" in turn && turn.error != null);
}

export function codexEventToRuntime(event: CodexEvent, sequence: number): RuntimeEvent {
	const method = event.method;
	const text = textValue(event.params);
	const lower = method.toLowerCase();
	const item = event.params.item && typeof event.params.item === "object" ? event.params.item as Record<string, unknown> : undefined;
	const itemType = typeof item?.type === "string" ? item.type.toLowerCase() : "";
	const agentMessage = lower.includes("agentmessage") || lower.includes("agent_message") || itemType === "agentmessage";
	let eventKind = method;
	if (lower === "thread/compacted" || itemType === "contextcompaction") eventKind = "thread/compacted";
	else if (agentMessage) {
		eventKind = lower.includes("delta") ? "message.delta" : lower.includes("completed") ? "message.completed" : "message.created";
	} else if (lower.includes("reasoning") || itemType === "reasoning") eventKind = "agent.reasoning";
	else if (lower.includes("commandexecution") || itemType === "commandexecution") eventKind = "command.execution";
	else if (lower.includes("filechange") || itemType === "filechange") eventKind = "file.change";
	else if (lower === "turn/completed") eventKind = completedTurnActuallyFailed(event.params) ? "run.failed" : "run.completed";
	else if (lower === "turn/failed") eventKind = "run.failed";
	else if (lower === "turn/interrupted") eventKind = "run.cancelled";
	else if (lower === "turn/started") eventKind = "run.started";
	const messageId = typeof event.params.messageId === "string"
		? event.params.messageId
		: typeof event.params.itemId === "string"
			? event.params.itemId
			: typeof item?.id === "string" ? item.id : undefined;
	const payload = text
		? { ...event.params, ...(messageId ? { messageId } : {}), [eventKind === "message.delta" ? "delta" : "content"]: text }
		: { ...event.params, ...(messageId ? { messageId } : {}) };
	return {
		schemaVersion: "synth.desktop-runtime-event.v1", sessionId: event.sessionId,
		sequence, eventKind, payload,
		createdAt: new Date().toISOString(), source: "local"
	};
}

/** Projects a durable CoreRuntime journal row into the renderer protocol. */
export function coreEventToRuntime(event: AppEvent): RuntimeEvent | null {
	if (!event.sessionId || event.sessionSequence == null) return null;
	if (event.kind.startsWith("message.") || event.kind.startsWith("run.")) {
		return {
			schemaVersion: "synth.desktop-runtime-event.v1",
			sessionId: event.sessionId,
			sequence: event.sessionSequence,
			eventKind: event.kind,
			payload: event.payload,
			createdAt: event.createdAt,
			source: event.source === "remote" ? "remote"
				: event.source === "intern" ? "intern"
					: event.source === "system" ? "system" : "local"
		};
	}
	return codexEventToRuntime(
		{ sessionId: event.sessionId, method: event.kind, params: event.payload },
		event.sessionSequence
	);
}
