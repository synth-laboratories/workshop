import type {
	CodexActivityEvent,
	ExecutionTarget,
	RuntimeEvent,
	RuntimeHealth,
	Session,
	VisualInstanceRecord
} from "@synth/runtime-protocol";
import {
	OPENROUTER_LAGUNA_S_MODEL,
	OPENROUTER_LUNA_MODEL,
	OPENROUTER_MUSE_SPARK_MODEL,
	OPENROUTER_GEMINI_FLASH_MODEL,
	CHATGPT_LUNA_MODEL,
	CHATGPT_SOL_MODEL,
	CHATGPT_TERRA_MODEL,
	SYNTH_CLOUD_LAGUNA_S_MODEL,
	SYNTH_CLOUD_MUSE_SPARK_MODEL,
	type ActivityEvent,
	type ArtifactRef,
	type AsyncInternPin,
	type AsyncPhase,
	type ChatMessage,
	type LandingState,
	type LocalActivityLine,
	type LocalChat,
	type ModelStatus,
	type SyncSession,
	type SyncSessionStatus
} from "../types/landing";
import { assertLocalActivityPlacementInvariant } from "./activityPlacementInvariant";
import { modelCapabilitiesForExecutionTarget } from "./modelCapabilities";

/** Transcript divider copy for `thread/compacted` events. */
export function contextCompactionLabel(source: string): string {
	switch (source.toLowerCase()) {
		case "manual":
			return "Context compacted";
		case "model_switch":
			return "Model switch - context compacted";
		default:
			return "Context automatically compacted";
	}
}

/** Format a token count as a fraction of a million, e.g. `0.27M`. */
export function formatTokensAsMillions(tokens: number): string {
	const millions = Math.max(0, tokens) / 1_000_000;
	const digits = millions >= 10 ? 1 : 2;
	return `${millions.toFixed(digits)}M`;
}

/** Compact before → after summary for the compaction disclosure. */
export function contextCompactionTokenSummary(before: number, after: number): string {
	return `${formatTokensAsMillions(before)} → ${formatTokensAsMillions(after)}`;
}

function tokenTotalFromPayload(payload: Record<string, unknown>): number | undefined {
	const usage = payload.tokenUsage && typeof payload.tokenUsage === "object"
		? payload.tokenUsage as Record<string, unknown>
		: payload.usage && typeof payload.usage === "object"
			? payload.usage as Record<string, unknown>
			: payload;
	const nest = (value: unknown): Record<string, unknown> | null =>
		value && typeof value === "object" ? value as Record<string, unknown> : null;
	const candidates = [nest(usage.last), nest(usage.total), usage];
	for (const candidate of candidates) {
		if (!candidate) continue;
		const total = candidate.totalTokens ?? candidate.total_tokens;
		if (typeof total === "number" && Number.isFinite(total) && total >= 0) return total;
	}
	return undefined;
}

export function targetIdToExecutionTarget(targetId: string): ExecutionTarget {
	const adapter = null;

	switch (targetId) {
		case "chatgpt-luna":
		case "chatgpt-sol":
		case "chatgpt-terra":
			return {
				kind: "remote",
				provider: "openai-codex-oauth",
				model: targetId === "chatgpt-sol" ? CHATGPT_SOL_MODEL : targetId === "chatgpt-terra" ? CHATGPT_TERRA_MODEL : CHATGPT_LUNA_MODEL,
				adapter
			};
		case "openrouter-luna":
			return {
				kind: "remote",
				provider: "openrouter",
				model: OPENROUTER_LUNA_MODEL,
				adapter
			};
		case "openrouter-laguna-s":
		case "openrouter-poolside":
			return {
				kind: "remote",
				provider: "openrouter",
				model: OPENROUTER_LAGUNA_S_MODEL,
				adapter
			};
		case "openrouter-muse-spark":
			return {
				kind: "remote",
				provider: "openrouter",
				model: OPENROUTER_MUSE_SPARK_MODEL,
				adapter
			};
		case "openrouter-gemini-flash":
			return {
				kind: "remote",
				provider: "openrouter",
				model: OPENROUTER_GEMINI_FLASH_MODEL,
				adapter
			};
		case "synth-cloud-laguna-s":
			return {
				kind: "cloud",
				model: SYNTH_CLOUD_LAGUNA_S_MODEL,
				adapter
			};
		case "synth-cloud-muse-spark":
			return {
				kind: "cloud",
				model: SYNTH_CLOUD_MUSE_SPARK_MODEL,
				adapter
			};
		case "intern-sync":
			return { kind: "intern", mode: "sync" };
		case "intern-async":
			return { kind: "intern", mode: "async" };
		case "local-laguna":
		default:
			return {
				kind: "local",
				model: "laguna-xs-2.1",
				adapter
			};
	}
}

export function executionTargetToUiId(target: ExecutionTarget): string {
	if (target.kind === "local") return "local-laguna";
	if (target.kind === "intern") {
		return target.mode === "async" ? "intern-async" : "intern-sync";
	}
	if (target.kind === "cloud") {
		return target.model === SYNTH_CLOUD_MUSE_SPARK_MODEL
			? "synth-cloud-muse-spark"
			: "synth-cloud-laguna-s";
	}
	if (target.provider === "openai-codex-oauth") {
		return target.model === CHATGPT_SOL_MODEL ? "chatgpt-sol" : target.model === CHATGPT_TERRA_MODEL ? "chatgpt-terra" : "chatgpt-luna";
	}
	if (target.model === OPENROUTER_LUNA_MODEL || target.model.includes("kimi")) {
		return "openrouter-luna";
	}
	if (target.model === OPENROUTER_MUSE_SPARK_MODEL) return "openrouter-muse-spark";
	if (target.model === OPENROUTER_GEMINI_FLASH_MODEL) return "openrouter-gemini-flash";
	return "openrouter-laguna-s";
}

export function sessionIsLocalChat(session: Session): boolean {
	return session.target.kind === "local" || session.target.kind === "remote" || session.target.kind === "cloud";
}

export function sessionIsSync(session: Session): boolean {
	return session.target.kind === "intern" && session.target.mode === "sync";
}

export function sessionIsAsync(session: Session): boolean {
	return session.target.kind === "intern" && session.target.mode === "async";
}

function mapSessionStatus(status: Session["status"]): SyncSessionStatus {
	switch (status) {
		case "waiting_for_input":
			return "waiting_for_operator";
		case "paused":
			return "paused";
		case "completed":
		case "cancelled":
		case "failed":
			return "closed";
		case "running":
			return "thinking";
		default:
			return "ready";
	}
}

function mapAsyncPhase(status: Session["status"], events: RuntimeEvent[] = []): AsyncPhase {
	const lastAsyncEvent = [...events].reverse().find((event) =>
		event.eventKind.startsWith("async.") || event.eventKind === "checkpoint.created"
	);
	if (lastAsyncEvent?.eventKind === "async.sleeping") return "sleeping";
	switch (status) {
		case "waiting_for_input":
			return "waiting_for_input";
		case "paused":
			return "sleeping";
		case "failed":
			return "blocked";
		default:
			return "running";
	}
}

export function eventsToMessages(events: RuntimeEvent[]): ChatMessage[] {
	const byId = new Map<string, ChatMessage>();
	const order: string[] = [];
	const subagentIds = new Set(eventsToSubagents(events).map((agent) => agent.id));
	let activeAssistantId: string | null = null;
	let producedAssistantForTurn = false;
	let compactedDuringTurn = false;
	let terminalSeenForTurn = false;

	for (const event of events) {
		const payload = event.payload ?? {};
		// Child-thread events drive the Subagents visual, never the parent
		// transcript. In particular, every V2 child has its own turn lifecycle;
		// letting those terminal events reach the parent would manufacture
		// "provider ended" messages and reset the parent's streaming state.
		const sourceThreadId = eventThreadId(payload, eventItem(event));
		if (sourceThreadId && subagentIds.has(sourceThreadId)) continue;
		if (event.eventKind === "run.started") {
			activeAssistantId = null;
			producedAssistantForTurn = false;
			compactedDuringTurn = false;
			terminalSeenForTurn = false;
		}
		if (event.eventKind === "thread/compacted") {
			compactedDuringTurn = true;
			continue;
		}
		// Agent commentary is segmented by concrete activity. Once a tool or
		// approval begins, a later assistant delta is a new chronological block;
		// keeping the prior draft active would hoist that new text above the tools
		// that already ran. Rotating token-envelope ids are still coalesced as long
		// as no activity boundary intervenes.
		if (safeToolActivity(event) || event.eventKind.startsWith("approval.")) {
			activeAssistantId = null;
		}
		const messageThreadId = typeof payload.threadId === "string"
			? payload.threadId
			: typeof payload.thread_id === "string" ? payload.thread_id : undefined;
		if (messageThreadId && subagentIds.has(messageThreadId) && event.eventKind.startsWith("message.")) {
			continue;
		}
		const isInternAgentMessage =
			event.eventKind === "agent_message" ||
			event.eventKind === "intern.agent_message";
		const explicitMessageId =
			typeof payload.messageId === "string"
				? payload.messageId
				: isInternAgentMessage && typeof payload.id === "string"
					? payload.id
					: isInternAgentMessage && typeof payload.intern === "object" && payload.intern !== null &&
						  typeof (payload.intern as Record<string, unknown>).eventId === "string"
						? String((payload.intern as Record<string, unknown>).eventId)
					: null;
		// App-server/provider adapters do not all agree on delta identity. In
		// particular, some local Responses transports assign a fresh item id to
		// every token envelope. A streaming assistant response is one draft per
		// turn, so once a draft is active it must win over a changing envelope id.
		const messageId: string = event.eventKind === "message.delta"
			? (activeAssistantId ?? explicitMessageId ?? `assistant-draft-${event.sequence}`)
			: explicitMessageId ?? `evt-${event.sequence}`;
		const roleRaw = typeof payload.role === "string" ? payload.role : "assistant";
		const role =
			roleRaw === "user" || roleRaw === "system" || roleRaw === "assistant"
				? roleRaw
				: "assistant";

		if (event.eventKind === "message.created" || isInternAgentMessage) {
			const content =
				typeof payload.content === "string"
					? payload.content
					: typeof payload.body === "string"
						? payload.body
						: typeof payload.message === "string"
							? payload.message
							: typeof payload.text === "string"
								? payload.text
						: "";
			const images = Array.isArray(payload.images)
				? payload.images.flatMap((entry) => {
					if (typeof entry !== "object" || entry === null) return [];
					const image = entry as Record<string, unknown>;
					if (typeof image.path !== "string" || typeof image.name !== "string" || typeof image.previewUrl !== "string") return [];
					return [{ path: image.path, name: image.name, previewUrl: image.previewUrl }];
				})
				: undefined;
			if (!content && !images?.length) continue;
			if (!byId.has(messageId)) {
				order.push(messageId);
				byId.set(messageId, {
					id: messageId,
					role: isInternAgentMessage ? "assistant" : role,
					body: content,
					at: event.createdAt,
					images
				});
				} else {
					const existing = byId.get(messageId)!;
					byId.set(messageId, { ...existing, role, body: content || existing.body, images: images ?? existing.images });
				}
				if (!isInternAgentMessage && role === "assistant") {
					activeAssistantId = messageId;
					producedAssistantForTurn = true;
				}
				// The locally inserted user event is the most reliable turn boundary.
				// Some app-server/provider combinations emit turn/started late or omit
				// it, so carrying the prior assistant id past a user message merges the
				// next response into the preceding turn above the new prompt.
				if (!isInternAgentMessage && role === "user") {
					activeAssistantId = null;
					producedAssistantForTurn = false;
					terminalSeenForTurn = false;
				}
				continue;
		}

		if (event.eventKind === "message.delta") {
			const delta = typeof payload.delta === "string" ? payload.delta : "";
			const existing = byId.get(messageId);
			if (existing) {
				const nextBody =
					delta && existing.body && delta.startsWith(existing.body)
						? delta
						: existing.body + delta;
				byId.set(messageId, {
					...existing,
					role: "assistant",
					body: nextBody
				});
			} else {
				order.push(messageId);
				byId.set(messageId, {
					id: messageId,
					role: "assistant",
					body: delta,
					at: event.createdAt
				});
				}
				activeAssistantId = messageId;
				producedAssistantForTurn = true;
				continue;
		}

		if (event.eventKind === "message.completed") {
			const content =
				typeof payload.content === "string"
					? payload.content
					: typeof payload.body === "string"
						? payload.body
						: undefined;
			// A stable item id is authoritative. The only exception is an alternate
			// completion envelope containing the exact text already rendered for the
			// active item; app-server versions can publish both completion shapes.
			const completionMessageId = activeAssistantId
				?? (explicitMessageId && byId.has(explicitMessageId) ? explicitMessageId : null)
				?? explicitMessageId
				?? messageId;
			const existing = byId.get(completionMessageId);
			if (existing) {
				byId.set(completionMessageId, {
					...existing,
					role: "assistant",
					body: content ?? existing.body
				});
			} else if (content) {
				order.push(completionMessageId);
				byId.set(completionMessageId, {
					id: completionMessageId,
					role: "assistant",
					body: content,
					at: event.createdAt
				});
				}
				activeAssistantId = completionMessageId;
				producedAssistantForTurn = true;
			}

		if (
			event.eventKind === "run.completed" ||
			event.eventKind === "run.failed" ||
			event.eventKind === "run.cancelled"
		) {
			// The live app-server event and the durable run.status_changed event
			// can both project the same turn terminal. Only the first terminal owns
			// transcript fallback synthesis; replaying it after an assistant answer
			// must not manufacture a contradictory "no response" message.
			if (terminalSeenForTurn) continue;
			terminalSeenForTurn = true;
			// A typed final agent item can be the terminal run payload when the
			// app-server closes immediately after its final answer. Preserve that
			// authoritative content as an assistant message before deciding that
			// the provider returned no answer.
			if (!producedAssistantForTurn && event.eventKind === "run.completed") {
				const terminalAnswer = eventResultPreview(payload, objectValue(payload.item) ?? {});
				if (terminalAnswer) {
					const id = `terminal-answer-${event.sequence}`;
					order.push(id);
					byId.set(id, { id, role: "assistant", body: terminalAnswer, at: event.createdAt });
					producedAssistantForTurn = true;
				}
			}
			if (!producedAssistantForTurn && !compactedDuringTurn) {
				const detail = terminalTurnDetail(event.payload ?? {});
				const providerLimitMessage = providerLimitMessageFor(event.payload ?? {});
				const failureDetail = detail ? `: ${detail.replace(/[.!?]+$/, "")}.` : ".";
				const message = event.eventKind === "run.failed"
					? providerLimitMessage
						? providerLimitMessage
						: `The provider could not produce a response${failureDetail} Try again.`
					: event.eventKind === "run.cancelled"
						? "The response was stopped before the provider returned an answer."
						: "The provider ended the turn without a response. Please try again.";
				const previous = order.length ? byId.get(order[order.length - 1]) : undefined;
				// Some provider bridges publish the same terminal envelope more than
				// once. A transport retry is one user-visible failure, not a wall of
				// identical system messages.
				if (previous?.role !== "system" || previous.body !== message) {
					const id = `terminal-${event.sequence}`;
					order.push(id);
					byId.set(id, { id, role: "system", body: message, at: event.createdAt });
				}
			}
			activeAssistantId = null;
			compactedDuringTurn = false;
		}
	}

	return order.map((id) => byId.get(id)!).filter(Boolean);
}

function terminalTurnDetail(payload: Record<string, unknown>): string | undefined {
	const turn = payload.turn && typeof payload.turn === "object"
		? payload.turn as Record<string, unknown>
		: payload;
	const error = turn.error && typeof turn.error === "object"
		? turn.error as Record<string, unknown>
		: payload.error && typeof payload.error === "object" ? payload.error as Record<string, unknown> : undefined;
	const message = typeof error?.message === "string" ? error.message.trim() : "";
	if (!message) return undefined;

	// Provider adapters can put a full nested HTTP response in `message`. Never
	// expose that implementation payload in the transcript; preserve the useful
	// reason when it is recognizable and otherwise keep the failure concise.
	if (message.includes("Requests ending with a model turn are not supported")) {
		return "The provider rejected a request ending with a model turn";
	}
	if (message.toLowerCase().includes("temporarily rate-limited")) {
		return "Gemini is temporarily rate-limited";
	}
	if (message.startsWith("{") || message.length > 240) {
		return "The provider rejected the request";
	}
	return message.replace(/\s+/g, " ");
}

function providerLimitMessageFor(payload: Record<string, unknown>): string | undefined {
	const turn = payload.turn && typeof payload.turn === "object"
		? payload.turn as Record<string, unknown>
		: payload;
	const error = turn.error && typeof turn.error === "object"
		? turn.error as Record<string, unknown>
		: payload.error && typeof payload.error === "object" ? payload.error as Record<string, unknown> : undefined;
	const code = typeof error?.codexErrorInfo === "string" ? error.codexErrorInfo.toLowerCase() : "";
	const rawMessage = typeof error?.message === "string" ? error.message.trim() : "";
	const message = rawMessage.toLowerCase();
	const provider = typeof payload.provider === "string" ? payload.provider.toLowerCase() : "";
	if (code === "usagelimitexceeded" || message.includes("hit your usage limit")) {
		const reset = /try again at\s+(.+?)(?:\.+)?$/i.exec(rawMessage)?.[1]?.trim();
		return reset
			? `Your ChatGPT usage limit has been reached. You are still signed in; use another model or try again at ${reset}.`
			: "Your ChatGPT usage limit has been reached. You are still signed in; use another model or try again after your limit resets.";
	}
	if (
		provider === "openrouter" ||
		message.includes("openrouter") ||
		/(insufficient[_ ](?:credits|funds)|credit balance|no credits|payment required)/.test(message)
	) {
		return "Your OpenRouter credits are unavailable or exhausted. Add credits or choose another model, then retry.";
	}
	if (
		provider === "synth" || provider === "synth-cloud" ||
		message.includes("synth cloud") || message.includes("synth allowance") ||
		/(allowance (?:is )?(?:used up|exhausted)|billing (?:needs attention|limit))/.test(message)
	) {
		return "Your Synth Cloud allowance is unavailable. Manage billing or choose a local/API-key model, then retry.";
	}
	return undefined;
}

export function eventsToActivity(
	events: RuntimeEvent[],
	codexActivity: CodexActivityEvent[] = []
): ActivityEvent[] {
	const mailbox = events
		.filter((event) => {
			if (event.eventKind.startsWith("message.")) return false;
			if (
				event.eventKind === "agent_message" ||
				event.eventKind === "intern.agent_message"
			) return false;
			return true;
		})
		.map((event) => {
			const payload = event.payload ?? {};
			const summary =
				typeof payload.summary === "string"
					? payload.summary
					: typeof payload.message === "string"
						? payload.message
						: event.eventKind;
			const detail =
				typeof payload.detail === "string"
					? payload.detail
					: event.eventKind === "usage.recorded"
						? JSON.stringify(payload)
						: undefined;
			const lane =
				event.source === "intern"
					? ("intern" as const)
					: ("codex" as const);
			return {
				sequence: event.sequence,
				eventKind: event.eventKind,
				lane,
				summary,
				at: event.createdAt,
				detail
			};
		});
	const codex = codexActivity.map((event, index) => {
		const payload = event.payload ?? {};
		const summary =
			typeof payload.summary === "string" ? payload.summary :
			typeof payload.message === "string" ? payload.message :
			typeof payload.content === "string" ? payload.content :
			event.eventKind.replace(/\./g, " ");
		const detail =
			typeof payload.detail === "string" ? payload.detail :
			typeof payload.output === "string" ? payload.output : undefined;
		return {
			sequence: events.length + index + 1,
			eventKind: event.eventKind,
			lane: "codex" as const,
			summary,
			at: event.createdAt,
			detail
		};
	});
	return [...mailbox, ...codex].sort((left, right) => left.at.localeCompare(right.at));
}

function activityKind(eventKind: string): LocalActivityLine["kind"] {
	if (eventKind === "thought.created" || eventKind.startsWith("thought.")) return "thought";
	if (eventKind.startsWith("approval.")) return "approval";
	if (eventKind === "file.read") return "file_read";
	if (eventKind === "file.written" || eventKind === "file.changed") return "file_write";
	if (eventKind.startsWith("command.")) return "command";
	if (eventKind === "visual.created") return "visual";
	return "working";
}

type SafeToolActivity = Pick<LocalActivityLine, "label" | "detail" | "path" | "kind" | "toolStatus" | "durationMs" | "visualStage" | "artifactId" | "containerId" | "optimizerRunId" | "runKind"> & { key: string };

const VISUAL_MUTATION_TOOLS = new Set([
	"visual_manage",
	"visual_create",
	"visual_create_from_template",
	"visual_update",
	"visual_bind_data_source",
	"visual_save",
	"visual_save_tsx",
	"visual_fork",
	"visual_show",
	"visual_open_in_pane",
	"visual_archive"
]);

function nestedObject(value: Record<string, unknown>, ...names: string[]): Record<string, unknown> | undefined {
	for (const name of names) {
		const nested = objectValue(value[name]);
		if (nested) return nested;
	}
	return undefined;
}

function redactCommand(command: string): string {
	return command
		.replace(/\b([A-Z][A-Z0-9_]*(?:API_KEY|TOKEN|SECRET|PASSWORD))=(?:'[^']*'|"[^"]*"|\S+)/g, "$1=[redacted]")
		.replace(/\bBearer\s+[A-Za-z0-9._~+\/-]+/gi, "Bearer [redacted]")
		.replace(/\b(?:sk|sess|key)-[A-Za-z0-9_-]{12,}\b/g, "[redacted]")
		.slice(0, 800);
}

function safeToolStatus(item: Record<string, unknown>, eventKind = ""): LocalActivityLine["toolStatus"] {
	const status = (stringField(item, "status") ?? "").toLowerCase();
	const lifecycle = `${status} ${eventKind}`.toLowerCase();
	// An MCP result carrying isError is a failed call even when the provider
	// reported the turn item itself as completed.
	if (objectValue(item.result)?.isError === true) return "failed";
	if (/failed|error|cancelled|rejected/.test(lifecycle)) return "failed";
	if (/completed|success|done/.test(lifecycle)) return "completed";
	return "running";
}

function safeToolFailure(item: Record<string, unknown>): string | undefined {
	const result = objectValue(item.result);
	const structured = result && objectValue(result.structuredContent);
	// A structured application failure keeps its stable code and remediation so
	// the transcript says what to do next, not just that something failed.
	const coded = structured ? objectValue(structured.structuredError) : undefined;
	const codedMessage = coded
		? [stringField(coded, "code"), stringField(coded, "remediation")].filter(Boolean).join(" · ")
		: undefined;
	const structuredError = structured ? stringField(structured, "error", "message", "detail", "code") : undefined;
	// Provider error text is not a trusted transcript surface: it can contain
	// prompts, credentials, or arbitrary tool output. Only the structured,
	// application-owned failure envelope is safe to render.
	const message = codedMessage || structuredError;
	return message ? redactCommand(message).slice(0, 320) : undefined;
}

function compactToolArgs(args: Record<string, unknown>, fields: string[]): string | undefined {
	const values: string[] = [];
	for (const field of fields) {
		const value = args[field];
		if (typeof value === "string" && value.trim()) {
			values.push(`${field.replace(/_/g, " ")} ${redactCommand(value.trim()).slice(0, 120)}`);
		} else if (typeof value === "number" && Number.isFinite(value)) {
			values.push(`${field.replace(/_/g, " ")} ${value}`);
		} else if (typeof value === "boolean") {
			values.push(`${field.replace(/_/g, " ")} ${value ? "yes" : "no"}`);
		}
	}
	return values.length ? values.join(" · ") : undefined;
}

function parseJsonObject(value: unknown): Record<string, unknown> | undefined {
	if (typeof value !== "string") return objectValue(value);
	try {
		return objectValue(JSON.parse(value));
	} catch {
		return undefined;
	}
}

function visualFromToolResult(item: Record<string, unknown>): Record<string, unknown> | undefined {
	const result = objectValue(item.result);
	if (!result) return undefined;
	const structured = nestedObject(result, "structuredContent", "structured_content");
	const direct = nestedObject(structured ?? {}, "visual") ?? nestedObject(result, "visual");
	if (direct) return direct;
	const content = Array.isArray(result.content) ? result.content : [];
	for (const entry of content) {
		const parsed = parseJsonObject(objectValue(entry)?.text);
		const visual = parsed ? nestedObject(parsed, "visual") : undefined;
		if (visual) return visual;
	}
	return undefined;
}

function toolStructuredContent(item: Record<string, unknown>): Record<string, unknown> | undefined {
	const result = objectValue(item.result);
	if (!result) return undefined;
	const structured = nestedObject(result, "structuredContent", "structured_content");
	if (structured) return structured;
	const content = Array.isArray(result.content) ? result.content : [];
	for (const entry of content) {
		const parsed = parseJsonObject(objectValue(entry)?.text);
		if (parsed) return parsed;
	}
	return result;
}

function containerIdForTool(
	item: Record<string, unknown>,
	args: Record<string, unknown>,
	tool: string
): string | undefined {
	const fromArgs = stringField(args, "container_id", "containerId");
	if (fromArgs) return fromArgs;
	if (tool === "container_list") return undefined;
	const structured = toolStructuredContent(item);
	if (!structured) return undefined;
	const container = nestedObject(structured, "container") ?? structured;
	return stringField(container, "id", "containerId", "container_id");
}

function visualIdForTool(
	item: Record<string, unknown>,
	args: Record<string, unknown>,
	tool: string
): string | undefined {
	if (!VISUAL_MUTATION_TOOLS.has(tool)) return undefined;
	const resultVisual = visualFromToolResult(item);
	const resultId = resultVisual ? stringField(resultVisual, "id", "visualId", "visual_id") : undefined;
	if (resultId) return resultId;
	if (tool === "visual_manage") {
		const operation = stringField(args, "operation");
		if (operation === "create") return undefined;
		const argumentsValue = objectValue(args.arguments);
		return argumentsValue
			? stringField(argumentsValue, "visual_id", "visualId", "instance_id", "instanceId", "id")
			: undefined;
	}
	if (tool === "visual_create" || tool === "visual_create_from_template") return undefined;
	return stringField(args, "visual_id", "visualId", "instance_id", "instanceId", "id");
}

/** Workflows chat has a run-progress card for. Others stream but get no card. */
const CARDED_RUN_KINDS = new Set(["eval", "gepa", "sft", "environment"]);

/**
 * The durable run a `synth_optimizers` call started or acted on.
 *
 * Read from the tool *result* — the run record the host returned — and only
 * secondarily from the arguments, because an argument is a request and a result
 * is a fact. Nothing is inferred from surrounding prose: a run id in an
 * assistant sentence is text, not a subscription.
 */
function optimizerRunForTool(
	item: Record<string, unknown>,
	args: Record<string, unknown>
): { optimizerRunId?: string; runKind?: LocalActivityLine["runKind"] } {
	const structured = toolStructuredContent(item);
	const run = structured ? nestedObject(structured, "run", "optimizerRun") ?? structured : undefined;
	const id = (run && stringField(run, "id", "optimizerRunId", "optimizer_run_id"))
		?? stringField(args, "optimizer_run_id", "optimizerRunId")
		?? stringField(nestedObject(args, "arguments") ?? {}, "optimizer_run_id", "optimizerRunId");
	if (!id) return {};
	const algorithm = (run && stringField(run, "algorithmId", "algorithm_id")) ?? undefined;
	return {
		optimizerRunId: id,
		...(algorithm && CARDED_RUN_KINDS.has(algorithm)
			? { runKind: algorithm as LocalActivityLine["runKind"] }
			: {})
	};
}

function mcpToolActivity(
	item: Record<string, unknown>,
	args: Record<string, unknown>,
	id: string,
	itemType: string,
	tool: string,
	eventKind: string
): SafeToolActivity | undefined {
	if (!["mcptoolcall", "dynamictoolcall", "toolcall"].includes(itemType)) return undefined;
	const server = (stringField(item, "server", "pluginId", "plugin_id") ?? "").toLowerCase();
	const allowlisted: Record<string, string[]> = {
		"synth_containers.container_list": [],
		"synth_containers.container_register": ["base_url", "name"],
		"synth_containers.container_get": ["container_id"],
		"synth_containers.container_probe": ["container_id"],
		"synth_visuals.visual_manage": ["operation"],
		"synth_visuals.visual_list": [],
		"synth_visuals.visual_list_templates": ["genre"],
		"synth_visuals.visual_create": ["template_id", "title"],
		"synth_visuals.visual_create_from_template": ["template_id", "title"],
		"synth_visuals.visual_update": ["visual_id", "instance_id", "title", "status"],
		"synth_visuals.visual_bind_data_source": ["visual_id", "instance_id", "slot"],
		"synth_visuals.visual_save": ["visual_id", "instance_id"],
		"synth_visuals.visual_save_tsx": ["visual_id", "instance_id"],
		"synth_visuals.visual_fork": ["visual_id", "instance_id", "title"],
		"synth_visuals.visual_show": ["visual_id", "instance_id"],
		"synth_visuals.visual_open_in_pane": ["visual_id", "instance_id"],
		"synth_visuals.visual_archive": ["visual_id", "instance_id"],
	};
	const qualified = `${server}.${tool}`;
	const fields = allowlisted[qualified] ?? [
		"operation",
		"path",
		"query",
		"pattern",
		"glob",
		"container_id",
		"optimizer_run_id",
		"recipe_id",
		"visual_id",
		"title",
		"status",
		"limit"
	];
	const durationMs = typeof item.durationMs === "number" && Number.isFinite(item.durationMs)
		? Math.max(0, item.durationMs)
		: undefined;
	const nestedArgs = objectValue(args.arguments) ?? {};
	const argsLabel = [compactToolArgs(args, fields), compactToolArgs(nestedArgs, fields)]
		.filter(Boolean)
		.join(" · ") || undefined;
	const artifactId = server === "synth_visuals" ? visualIdForTool(item, args, tool) : undefined;
	const containerId = server === "synth_containers" ? containerIdForTool(item, args, tool) : undefined;
	const optimizerRun = server === "synth_optimizers" ? optimizerRunForTool(item, args) : {};
	const visualOperation = server === "synth_visuals"
		? (tool === "visual_manage" ? stringField(args, "operation") : tool.replace(/^visual_/, ""))
		: undefined;
	const toolStatus = safeToolStatus(item, eventKind);
	const failure = toolStatus === "failed" ? safeToolFailure(item) : undefined;
	const visualStage: LocalActivityLine["visualStage"] = toolStatus === "failed" && visualOperation
		? "failed"
		: visualOperation === "create"
			? "draft"
			: visualOperation === "capture_review" || visualOperation === "review"
				? "review"
				: visualOperation === "mark_ready"
					? "ready"
					: undefined;
	const lifecycleLabel = visualStage === "draft"
		? "Visual draft created"
		: visualStage === "review"
			? "Visual review"
			: visualStage === "ready"
				? "Visual ready"
				: visualStage === "failed"
					? "Visual update failed"
					: undefined;
	const visualDuration = visualStage && durationMs != null ? `${durationMs}ms` : undefined;
	return {
		key: `mcp:${id}`,
		label: lifecycleLabel ?? ([server, tool].filter(Boolean).join(".") || "Tool call"),
		detail: [argsLabel, failure, visualDuration].filter(Boolean).join(" · ") || undefined,
		kind: visualStage ? "visual_lifecycle" : artifactId ? "visual" : "working",
		artifactId,
		containerId,
		...optimizerRun,
		toolStatus,
		durationMs,
		visualStage
	};
}

/** Positive allowlist for transcript-safe tool affordances. */
function safeToolActivity(event: RuntimeEvent): SafeToolActivity | undefined {
	const payload = event.payload ?? {};
	const item = eventItem(event);
	const itemType = (stringField(item, "type") ?? "").toLowerCase();
	const tool = (stringField(item, "tool", "name", "toolName", "tool_name") ?? "").toLowerCase();
	const args = nestedObject(item, "arguments", "args", "input") ?? nestedObject(payload, "arguments", "args", "input") ?? {};
	const id = stringField(item, "id", "callId", "call_id") ?? `${event.eventKind}-${event.sequence}`;
	const toolStatus = safeToolStatus(item, event.eventKind);
	const durationMs = typeof item.durationMs === "number" && Number.isFinite(item.durationMs)
		? Math.max(0, item.durationMs)
		: typeof payload.durationMs === "number" && Number.isFinite(payload.durationMs)
			? Math.max(0, payload.durationMs)
			: undefined;
	const lifecycle = { toolStatus, durationMs };

	if (event.eventKind === "command.execution" || itemType === "commandexecution") {
		const raw = stringField(item, "command", "cmd") ?? stringField(payload, "command", "cmd");
		if (!raw) return undefined;
		return { key: `command:${id}`, label: "Run Shell Command", detail: redactCommand(raw), kind: "command", ...lifecycle };
	}

	if (event.eventKind === "file.change" || itemType === "filechange") {
		const path = stringField(item, "path", "filePath", "file_path") ?? stringField(payload, "path", "filePath", "file_path");
		return path ? { key: `write:${id}:${path}`, label: "Wrote", path, kind: "file_write", ...lifecycle } : undefined;
	}

	const path = stringField(args, "path", "file", "filePath", "file_path")
		?? stringField(item, "path", "file", "filePath", "file_path");
	if (["read", "read_file", "readfile", "read_text_file", "filesystem.read_text_file"].includes(tool)) {
		return path ? { key: `read:${id}:${path}`, label: "Read", path, kind: "file_read", ...lifecycle } : undefined;
	}
	if (["search", "search_files", "grep", "find", "find_files", "list_directory", "list_files"].includes(tool)) {
		const query = stringField(args, "query", "pattern", "glob");
		return { key: `search:${id}`, label: tool.startsWith("list") ? "Listed files" : "Searched files", detail: query?.slice(0, 300), kind: "search", ...lifecycle };
	}
	if (["web_search", "websearch", "search_web"].includes(tool) || itemType === "websearch") {
		const query = stringField(args, "query") ?? stringField(item, "query");
		return { key: `search:${id}`, label: "Searched the web", detail: query?.slice(0, 300), kind: "search", ...lifecycle };
	}
	if (["view_image", "viewimage"].includes(tool)) {
		return { key: `view:${id}`, label: "Viewed image", path, kind: "working", ...lifecycle };
	}
	return mcpToolActivity(item, args, id, itemType, tool, event.eventKind);
}

function compactDuration(start: string | undefined, end: string): string {
	if (!start) return "a moment";
	const milliseconds = Math.max(0, Date.parse(end) - Date.parse(start));
	if (!Number.isFinite(milliseconds) || milliseconds < 1_000) return "a moment";
	const seconds = Math.round(milliseconds / 1_000);
	if (seconds < 60) return `${seconds}s`;
	const minutes = Math.floor(seconds / 60);
	const remainder = seconds % 60;
	return remainder ? `${minutes}m ${remainder}s` : `${minutes}m`;
}

function runDuration(payload: Record<string, unknown>, start: string | undefined, end: string): string {
	const outcome = objectValue(payload.outcome);
	const turn = objectValue(payload.turn) ?? (outcome && objectValue(outcome.turn));
	const explicit = turn?.durationMs ?? payload.durationMs;
	if (typeof explicit !== "number" || !Number.isFinite(explicit)) return compactDuration(start, end);
	const syntheticEnd = new Date(Math.max(0, explicit)).toISOString();
	return compactDuration(new Date(0).toISOString(), syntheticEnd);
}

function runIdentity(payload: Record<string, unknown>): string | undefined {
	const outcome = objectValue(payload.outcome);
	const turn = objectValue(payload.turn) ?? (outcome && objectValue(outcome.turn));
	return stringField(payload, "runId", "run_id", "turnId", "turn_id")
		?? (turn && stringField(turn, "id", "runId", "run_id", "turnId", "turn_id"));
}

function actionCountLabel(counts: { commands: number; reads: number; writes: number; searches: number; tools: number }): string {
	const parts: string[] = [];
	if (counts.commands) parts.push(`ran ${counts.commands} command${counts.commands === 1 ? "" : "s"}`);
	if (counts.reads) parts.push(`read ${counts.reads} file${counts.reads === 1 ? "" : "s"}`);
	if (counts.writes) parts.push(`updated ${counts.writes} file${counts.writes === 1 ? "" : "s"}`);
	if (counts.searches) parts.push(`searched ${counts.searches === 1 ? "once" : `${counts.searches} times`}`);
	if (counts.tools) parts.push(`used ${counts.tools} tool${counts.tools === 1 ? "" : "s"}`);
	return parts.join(", ");
}

export type SubagentLifecycle = "starting" | "working" | "completed" | "interrupted" | "failed" | "stopped" | "unavailable";
export type SubagentProtocol = "v1" | "v2";

export type SubagentState = {
	id: string;
	title: string;
	summary?: string;
	status: SubagentLifecycle;
	protocol: SubagentProtocol;
	agentPath?: string;
	lastAction?: "started" | "contacted" | "interrupted";
	startedAt: string;
	updatedAt: string;
};

function objectValue(value: unknown): Record<string, unknown> | undefined {
	return value && typeof value === "object" && !Array.isArray(value)
		? value as Record<string, unknown>
		: undefined;
}

function eventItem(event: RuntimeEvent): Record<string, unknown> {
	return objectValue(event.payload?.item) ?? event.payload ?? {};
}

function stringField(value: Record<string, unknown>, ...names: string[]): string | undefined {
	for (const name of names) {
		if (typeof value[name] === "string" && value[name]) return value[name] as string;
	}
	return undefined;
}

function stringArrayField(value: Record<string, unknown>, ...names: string[]): string[] {
	for (const name of names) {
		if (Array.isArray(value[name])) {
			return (value[name] as unknown[]).filter((item): item is string => typeof item === "string");
		}
	}
	return [];
}

function collabTool(item: Record<string, unknown>): string | undefined {
	const itemType = stringField(item, "type")?.toLowerCase() ?? "";
	if (!itemType.includes("collabagent")) return undefined;
	return stringField(item, "tool", "name")?.toLowerCase();
}

function subagentActivityKind(item: Record<string, unknown>): "started" | "interacted" | "interrupted" | undefined {
	const itemType = stringField(item, "type")?.toLowerCase() ?? "";
	if (!itemType.includes("subagentactivity") && !itemType.includes("sub_agent_activity")) return undefined;
	const kind = stringField(item, "kind")?.toLowerCase();
	return kind === "started" || kind === "interacted" || kind === "interrupted" ? kind : undefined;
}

function subagentLifecycle(value: unknown): SubagentLifecycle | undefined {
	const record = objectValue(value);
	const raw = typeof value === "string" ? value : record ? stringField(record, "type", "status") : undefined;
	if (!raw) return undefined;
	if (/pending.?init|pending|starting/i.test(raw)) return "starting";
	if (/active|running|working/i.test(raw)) return "working";
	if (/completed|done/i.test(raw)) return "completed";
	if (/interrupted|cancelled/i.test(raw)) return "interrupted";
	if (/errored|error|failed/i.test(raw)) return "failed";
	if (/shutdown|closed/i.test(raw)) return "stopped";
	if (/not.?found/i.test(raw)) return "unavailable";
	// `ThreadStatus::Idle` means this thread has no active turn. It is not the
	// collaboration agent's terminal result and must never become “completed”.
	return undefined;
}

function subagentTitle(prompt: string): string {
	return prompt.split(/[\n.!?]/, 1)[0].trim().slice(0, 64) || "Subagent";
}

function subagentTitleFromPath(path: string | undefined): string {
	const leaf = path?.split("/").filter(Boolean).at(-1);
	if (!leaf) return "Subagent";
	return leaf
		.replace(/[_-]+/g, " ")
		.replace(/\b\p{L}/gu, (letter) => letter.toUpperCase())
		.slice(0, 64);
}

function eventThreadId(payload: Record<string, unknown>, item: Record<string, unknown>): string | undefined {
	return stringField(payload, "threadId", "thread_id") ?? stringField(item, "threadId", "thread_id");
}

function stateMessage(value: unknown): string | undefined {
	const record = objectValue(value);
	return record ? stringField(record, "message", "content", "text") : undefined;
}

function eventResultPreview(payload: Record<string, unknown>, item: Record<string, unknown>): string | undefined {
	const direct = stringField(payload, "content", "text", "message", "lastAgentMessage", "last_agent_message")
		?? stringField(item, "content", "text", "message");
	if (direct) return direct;
	const turn = objectValue(payload.turn);
	const turnText = turn && stringField(turn, "lastAgentMessage", "last_agent_message", "message", "text");
	if (turnText) return turnText;
	const error = objectValue(payload.error) ?? (turn && objectValue(turn.error));
	return error && stringField(error, "message", "detail");
}

function eventLifecycle(event: RuntimeEvent): SubagentLifecycle | undefined {
	const kind = event.eventKind.toLowerCase();
	if (kind === "run.started" || kind === "turn/started") return "working";
	if (kind === "run.completed" || kind === "turn/completed") return "completed";
	if (kind === "run.failed" || kind === "turn/failed") return "failed";
	if (kind === "run.cancelled" || kind === "turn/interrupted") return "interrupted";
	return undefined;
}

function isTerminalSubagentStatus(status: SubagentLifecycle): boolean {
	return status === "completed" || status === "interrupted" || status === "failed" || status === "stopped" || status === "unavailable";
}

function upsertSubagent(
	agents: Map<string, SubagentState>,
	id: string,
	next: Omit<SubagentState, "id" | "startedAt" | "updatedAt"> & Partial<Pick<SubagentState, "startedAt">>,
	updatedAt: string,
	options: { allowReactivation?: boolean } = {}
): void {
	const existing = agents.get(id);
	if (!existing) {
		agents.set(id, {
			id,
			title: next.title,
			summary: next.summary,
			status: next.status,
			protocol: next.protocol,
			agentPath: next.agentPath,
			lastAction: next.lastAction,
			startedAt: next.startedAt ?? updatedAt,
			updatedAt
		});
		return;
	}
	const requestedStatus = next.status;
	const status =
		!options.allowReactivation && isTerminalSubagentStatus(existing.status) && !isTerminalSubagentStatus(requestedStatus)
			? existing.status
			: existing.status === "working" && requestedStatus === "starting"
				? existing.status
				: requestedStatus;
	agents.set(id, {
		...existing,
		...next,
		status,
		title: next.title || existing.title,
		summary: next.summary ?? existing.summary,
		agentPath: next.agentPath ?? existing.agentPath,
		lastAction: next.lastAction ?? existing.lastAction,
		startedAt: existing.startedAt,
		updatedAt
	});
}

export function eventsToSubagents(events: RuntimeEvent[]): SubagentState[] {
	const agents = new Map<string, SubagentState>();
	for (const event of events) {
		const payload = event.payload ?? {};
		const item = eventItem(event);
		const activityKind = subagentActivityKind(item);
		if (activityKind) {
			const id = stringField(item, "agentThreadId", "agent_thread_id");
			if (!id) continue;
			const agentPath = stringField(item, "agentPath", "agent_path");
			const previous = agents.get(id);
			const status = activityKind === "interrupted" ? "interrupted" : previous?.status ?? "starting";
			upsertSubagent(agents, id, {
				title: previous?.title && previous.title !== "Subagent" ? previous.title : subagentTitleFromPath(agentPath),
				summary: activityKind === "interrupted" ? "Interrupted" : previous?.summary,
				status,
				protocol: "v2",
				agentPath,
				lastAction: activityKind === "interacted" ? "contacted" : activityKind
			}, event.createdAt);
			continue;
		}

		const tool = collabTool(item);
		if (tool && /spawn.?agent/.test(tool)) {
			const callId = stringField(item, "id") ?? `subagent-${event.sequence}`;
			const ids = stringArrayField(item, "receiverThreadIds", "receiver_thread_ids");
			const prompt = stringField(item, "prompt") ?? "Subagent task";
			const provisional = agents.get(callId);
			if (ids.length && provisional && !ids.includes(callId)) agents.delete(callId);
			for (const id of ids.length ? ids : [callId]) {
				const existing = agents.get(id);
				upsertSubagent(agents, id, {
					title: subagentTitle(prompt),
					summary: existing?.summary ?? prompt,
					status: existing?.status ?? "starting",
					protocol: "v1",
					startedAt: existing?.startedAt ?? provisional?.startedAt ?? event.createdAt
				}, event.createdAt);
			}
		}
		// V2's direct collaboration protocol can conclude a parent `wait` with
		// an empty receiver list and no `agentsStates` payload. The child message
		// is already available at that point, and the completed wait is the
		// authoritative parent-level acknowledgement that it was joined. This is
		// deliberately narrower than treating a child `idle` state as terminal.
		if (
			tool === "wait" &&
			event.eventKind === "item/completed" &&
			subagentLifecycle(stringField(item, "status")) === "completed"
		) {
			for (const [id, agent] of agents) {
				if (agent.protocol !== "v2" || !agent.summary || isTerminalSubagentStatus(agent.status)) continue;
				upsertSubagent(agents, id, {
					title: agent.title,
					summary: agent.summary,
					status: "completed",
					protocol: "v2",
					agentPath: agent.agentPath,
					lastAction: agent.lastAction
				}, event.createdAt);
			}
		}

		const states = objectValue(item.agentsStates) ?? objectValue(item.agents_states)
			?? objectValue(payload.agentsStates) ?? objectValue(payload.agents_states);
		if (states) {
			for (const [id, value] of Object.entries(states)) {
				const existing = agents.get(id);
				const status = subagentLifecycle(value);
				if (!status) continue;
				upsertSubagent(agents, id, {
					title: existing?.title ?? "Subagent",
					summary: stateMessage(value) ?? existing?.summary,
					status,
					protocol: existing?.protocol ?? "v1",
					agentPath: existing?.agentPath,
					lastAction: existing?.lastAction
				}, event.createdAt);
			}
		}

		const threadId = eventThreadId(payload, item);
		if (!threadId || !agents.has(threadId)) continue;
		const agent = agents.get(threadId)!;
		const lifecycle = eventLifecycle(event) ?? (agent.protocol === "v1" ? subagentLifecycle(payload.status) : undefined);
		const content = eventResultPreview(payload, item);
		const isChildMessage = event.eventKind === "message.completed" || (event.eventKind === "item/completed" && /agentmessage/i.test(stringField(item, "type") ?? ""));
		if (!lifecycle && !(content && isChildMessage)) continue;
		upsertSubagent(agents, threadId, {
			title: agent.title,
			summary: content && (isChildMessage || lifecycle === "completed" || lifecycle === "failed") ? content : agent.summary,
			status: lifecycle ?? agent.status,
			protocol: agent.protocol,
			agentPath: agent.agentPath,
			lastAction: agent.lastAction
		}, event.createdAt, { allowReactivation: agent.protocol === "v2" && lifecycle === "working" });
	}
	return [...agents.values()];
}

export function eventsToLocalActivity(
	events: RuntimeEvent[],
	messages: ChatMessage[],
	reasoningDisplay: "none" | "full" | "summary" = "none",
	options?: { enforcePlacementInvariant?: boolean }
): Record<string, LocalActivityLine[]> {
	const assistantIds = messages.filter((message) => message.role === "assistant").map((message) => message.id);
	const lastContentSequenceByMessageId = new Map<string, number>();
	const approvalKey = (event: RuntimeEvent): string | undefined => {
		const payload = event.payload ?? {};
		for (const field of ["approvalId", "requestId", "commandId", "toolCallId", "id"]) {
			if (typeof payload[field] === "string") return payload[field] as string;
		}
		return undefined;
	};
	const resolvedApprovalSequences = new Set<number>();
	const pendingApprovals: Array<{ sequence: number; key?: string }> = [];
	for (const event of events) {
		if (event.eventKind === "approval.requested") {
			pendingApprovals.push({ sequence: event.sequence, key: approvalKey(event) });
			continue;
		}
		if (event.eventKind !== "approval.granted"
			&& event.eventKind !== "approval.rejected"
			&& event.eventKind !== "approval.expired") continue;
		const key = approvalKey(event);
		let index = pendingApprovals.length - 1;
		if (key) {
			while (index >= 0 && pendingApprovals[index].key !== key) index -= 1;
		}
		if (index >= 0) {
			resolvedApprovalSequences.add(pendingApprovals[index].sequence);
			pendingApprovals.splice(index, 1);
		}
	}
	const byMessage: Record<string, LocalActivityLine[]> = {};
	// Activity belongs to the current turn, not to the first historical
	// assistant message. Until the new assistant item has a stable id it stays
	// in the active tail directly after the latest user bubble.
	let current = "__active__";
	const subagentTitles = new Map<string, string>();
	const shownSubagentStarts = new Set<string>();
	const shownSubagentEnds = new Set<string>();
	let runStartedAt: string | undefined;
	let activeRunKey: string | undefined;
	const completedRunKeys = new Set<string>();
	let runActions = { commands: 0, reads: 0, writes: 0, searches: 0, tools: 0 };
	let compactionMarkerAddedForRun = false;
	let lastTokenTotal: number | undefined;
	let openCompactionLine: LocalActivityLine | undefined;
	const shownToolLines = new Map<string, LocalActivityLine>();
	for (const event of events) {
		if (event.eventKind === "approval.requested" && resolvedApprovalSequences.has(event.sequence)) continue;
		const payload = event.payload ?? {};
		if (
			event.eventKind === "thread/tokenUsage/updated"
			|| event.eventKind === "thread/token_usage/updated"
			|| event.eventKind.toLowerCase() === "tokenusage/updated"
		) {
			const tokens = tokenTotalFromPayload(payload);
			if (tokens != null) {
				if (
					openCompactionLine
					&& openCompactionLine.tokensBefore != null
					&& tokens < openCompactionLine.tokensBefore
					&& (openCompactionLine.tokensAfter == null || tokens < openCompactionLine.tokensAfter)
				) {
					openCompactionLine.tokensAfter = tokens;
					openCompactionLine.detail = contextCompactionTokenSummary(openCompactionLine.tokensBefore, tokens);
				}
				lastTokenTotal = tokens;
			}
			continue;
		}
		const item = eventItem(event);
		const subagentAction = subagentActivityKind(item);
		if (subagentAction) {
			const id = stringField(item, "agentThreadId", "agent_thread_id");
			if (!id) continue;
			const path = stringField(item, "agentPath", "agent_path");
			const title = subagentTitleFromPath(path);
			subagentTitles.set(id, title);
			if (subagentAction === "started" && !shownSubagentStarts.has(id)) {
				shownSubagentStarts.add(id);
				(byMessage[current] ??= []).push({
					id: `activity-${event.sequence}`,
					label: `${title} started`,
					detail: path,
					artifactId: "codex-subagents",
					kind: "subagent"
				});
			}
			if (subagentAction === "interacted") {
				(byMessage[current] ??= []).push({
					id: `activity-${event.sequence}`,
					label: `${title} contacted`,
					artifactId: "codex-subagents",
					kind: "subagent"
				});
			}
			if (subagentAction === "interrupted") {
				const key = `${id}:interrupted`;
				if (!shownSubagentEnds.has(key)) {
					shownSubagentEnds.add(key);
					(byMessage[current] ??= []).push({
						id: `activity-${event.sequence}`,
						label: `${title} interrupted`,
						artifactId: "codex-subagents",
						kind: "subagent"
					});
				}
			}
			continue;
		}
		const tool = collabTool(item);
		if (tool && /spawn.?agent/.test(tool)) {
			const callId = stringField(item, "id") ?? `subagent-${event.sequence}`;
			const prompt = stringField(item, "prompt") ?? "Subagent task";
			const title = subagentTitle(prompt);
			const ids = stringArrayField(item, "receiverThreadIds", "receiver_thread_ids");
			for (const id of ids.length ? ids : [callId]) subagentTitles.set(id, title);
			if (!shownSubagentStarts.has(callId)) {
				shownSubagentStarts.add(callId);
				(byMessage[current] ??= []).push({
					id: `activity-${event.sequence}`,
					label: `${title} started`,
					detail: prompt,
					artifactId: "codex-subagents",
					kind: "subagent"
				});
			}
			continue;
		}
		const states = objectValue(item.agentsStates) ?? objectValue(item.agents_states)
			?? objectValue(payload.agentsStates) ?? objectValue(payload.agents_states);
		if (states) {
			for (const [id, value] of Object.entries(states)) {
				const status = subagentLifecycle(value);
				if (!status || !isTerminalSubagentStatus(status) || !subagentTitles.has(id)) continue;
				const key = `${id}:${status}`;
				if (shownSubagentEnds.has(key)) continue;
				shownSubagentEnds.add(key);
				(byMessage[current] ??= []).push({
					id: `activity-${event.sequence}-${id}`,
					label: `${subagentTitles.get(id)} ${status === "failed" ? "failed" : status === "interrupted" ? "interrupted" : "finished"}`,
					artifactId: "codex-subagents",
					kind: "subagent"
				});
			}
		}
		const threadId = eventThreadId(payload, item);
		const status = eventLifecycle(event) ?? subagentLifecycle(payload.status);
		if (threadId && status && isTerminalSubagentStatus(status) && subagentTitles.has(threadId)) {
			const key = `${threadId}:${status}`;
			if (!shownSubagentEnds.has(key)) {
				shownSubagentEnds.add(key);
				(byMessage[current] ??= []).push({
					id: `activity-${event.sequence}`,
					label: `${subagentTitles.get(threadId)} ${status === "failed" ? "failed" : status === "interrupted" ? "interrupted" : "finished"}`,
					artifactId: "codex-subagents",
					kind: "subagent"
				});
			}
			continue;
		}
		const explicit = typeof payload.messageId === "string" ? payload.messageId : null;
		const messageText = event.eventKind.startsWith("message.")
			? (typeof payload.delta === "string" ? payload.delta
				: typeof payload.content === "string" ? payload.content
					: typeof payload.text === "string" ? payload.text : "")
			: "";
		// Streaming adapters frequently rotate or omit message ids, and the
		// aggregate message timestamp can be later than its first preamble delta.
		// Resolve that delta by content before falling back to timestamps so tools
		// emitted after the preamble attach *after* its bubble.
		const contentMatchedAssistant = messageText
			? messages.filter((message) => message.role === "assistant" && message.body.includes(messageText)).at(-1)?.id
			: undefined;
		const chronologicalAssistant = event.eventKind.startsWith("message.")
			? messages.filter((message) => message.role === "assistant" && message.at <= event.createdAt).at(-1)?.id
			: undefined;
		const resolvedAssistant = explicit && assistantIds.includes(explicit)
			? explicit
			: contentMatchedAssistant ?? chronologicalAssistant;
		if (resolvedAssistant) {
			if (current === "__active__" && byMessage.__active__?.length) {
				(byMessage[resolvedAssistant] ??= []).push(...byMessage.__active__);
				delete byMessage.__active__;
			}
			current = resolvedAssistant;
			if (messageText) lastContentSequenceByMessageId.set(resolvedAssistant, event.sequence);
		}
		if (event.eventKind === "message.created" && payload.role === "user") {
			// `messages` is the projection of the complete event slice, so it may
			// already contain a future assistant message. Do not attach activity
			// following this user event to that future bubble until its own event is
			// encountered; otherwise pre-answer tools render below the answer.
			current = "__active__";
			shownToolLines.clear();
			openCompactionLine = undefined;
			continue;
		}
		if (event.eventKind === "run.started") {
			const key = runIdentity(payload);
			// Some provider bridges replay their terminal envelopes while restoring
			// a thread. A repeated start for an already-finished run must not reset
			// its clock and make the historical duration grow on every app launch.
			if (key && completedRunKeys.has(key)) continue;
			activeRunKey = key;
			runStartedAt = event.createdAt;
			runActions = { commands: 0, reads: 0, writes: 0, searches: 0, tools: 0 };
			compactionMarkerAddedForRun = false;
			shownToolLines.clear();
			// Keep openCompactionLine: model-switch compact finishes before the
			// destination turn starts, and post-compact tokenUsage arrives after.
			continue;
		}
		if (event.eventKind === "run.completed" || event.eventKind === "run.failed" || event.eventKind === "run.cancelled") {
			const key = runIdentity(payload) ?? activeRunKey;
			if (key && completedRunKeys.has(key)) continue;
			const actions = actionCountLabel(runActions);
			const outcome = event.eventKind === "run.completed" ? "Worked" : event.eventKind === "run.failed" ? "Stopped with an error after" : "Stopped after";
			(byMessage[current] ??= []).unshift({
				id: `run-summary-${event.sequence}`,
				label: `${outcome} ${runDuration(payload, runStartedAt, event.createdAt)}${actions ? ` · ${actions}` : ""}`,
				kind: "run_summary"
			});
			runStartedAt = undefined;
			activeRunKey = undefined;
			if (key) completedRunKeys.add(key);
			continue;
		}
		if (event.eventKind === "session/unhealthy") {
			(byMessage[current] ??= []).unshift({
				id: `session-health-${event.sequence}`,
				label: "Stopped because the local agent disconnected · send a message to reconnect",
				kind: "run_summary"
			});
			runStartedAt = undefined;
			continue;
		}
		if (event.eventKind === "thread/compacted") {
			if (compactionMarkerAddedForRun) continue;
			compactionMarkerAddedForRun = true;
			const source = typeof payload.source === "string" ? payload.source.toLowerCase() : "automatic";
			// Manual `/compact` is a discrete post-response action → render after
			// the owning assistant bubble. Model-switch and automatic compaction
			// happen before the continued turn's tools/text, so they stay in the
			// chronological before-stream (tools must render *below* the divider).
			const line: LocalActivityLine = {
				id: `context-compaction-${event.sequence}`,
				label: contextCompactionLabel(source),
				placement: source === "manual" ? "after" : "before",
				sequence: event.sequence,
				kind: "context_compaction",
				tokensBefore: lastTokenTotal
			};
			(byMessage[current] ??= []).push(line);
			openCompactionLine = line;
			continue;
		}
		if (event.eventKind.startsWith("message.")) continue;
		if (reasoningDisplay !== "none" && (event.eventKind === "agent.reasoning" || event.eventKind.startsWith("thought."))) {
			const supplied =
				typeof payload.delta === "string" ? payload.delta :
				typeof payload.content === "string" ? payload.content :
				typeof payload.text === "string" ? payload.text :
				typeof payload.summary === "string" ? payload.summary : "";
			if (!supplied) continue;
			const lines = (byMessage[current] ??= []);
			const previous = lines.at(-1);
			const label = reasoningDisplay === "full" ? "Thought" : "Reasoning summary";
			if (previous?.kind === "thought" && previous.reasoningDisplay === reasoningDisplay) {
				const existing = previous.detail ?? "";
				previous.detail = supplied.startsWith(existing) ? supplied : existing + supplied;
			} else {
				lines.push({
					id: `activity-${event.sequence}`,
					label,
					detail: supplied,
					placement: current === "__active__" ? undefined : "after",
					sequence: event.sequence,
					kind: "thought",
					reasoningDisplay
				});
			}
			continue;
		}
		const safeTool = safeToolActivity(event);
		if (safeTool) {
			const existing = shownToolLines.get(safeTool.key);
			if (existing) {
				existing.label = safeTool.label;
				existing.detail = safeTool.detail;
				existing.path = safeTool.path;
				existing.kind = safeTool.kind;
				existing.visualStage = safeTool.visualStage;
				existing.toolStatus = safeTool.toolStatus;
				existing.durationMs = safeTool.durationMs;
				existing.artifactId = safeTool.artifactId;
				existing.containerId = safeTool.containerId;
				// A run id only ever arrives (never disappears): the call that
				// returns the record often completes after the one that requested it.
				if (safeTool.optimizerRunId) existing.optimizerRunId = safeTool.optimizerRunId;
				if (safeTool.runKind) existing.runKind = safeTool.runKind;
			} else {
				if (safeTool.kind === "command") runActions.commands += 1;
				if (safeTool.kind === "file_read") runActions.reads += 1;
				if (safeTool.kind === "file_write") runActions.writes += 1;
				if (safeTool.kind === "search") runActions.searches += 1;
				if (
					safeTool.toolStatus
					&& !["command", "file_read", "file_write", "search"].includes(safeTool.kind ?? "")
				) runActions.tools += 1;
				const line: LocalActivityLine = {
					id: `activity-${event.sequence}`,
					label: safeTool.label,
					detail: safeTool.detail,
					path: safeTool.path,
					placement: current === "__active__" ? undefined : "after",
					sequence: event.sequence,
					kind: safeTool.kind,
					visualStage: safeTool.visualStage,
					artifactId: safeTool.artifactId,
					containerId: safeTool.containerId,
					optimizerRunId: safeTool.optimizerRunId,
					runKind: safeTool.runKind,
					toolStatus: safeTool.toolStatus,
					durationMs: safeTool.durationMs
				};
				shownToolLines.set(safeTool.key, line);
				(byMessage[current] ??= []).push(line);
			}
			continue;
		}
		if (!event.eventKind.startsWith("approval.")) continue;
		const path = typeof payload.path === "string" ? payload.path : undefined;
		const approvalKind = typeof payload.kind === "string" ? payload.kind : "permission";
		const label = event.eventKind === "approval.requested" && approvalKind === "paid_compute" ? "Paid compute approval"
			: event.eventKind === "approval.requested" && approvalKind === "credential_access" ? "Credential access"
				: event.eventKind === "approval.requested" && approvalKind === "sidecar_lifecycle" ? "Sidecar lifecycle"
			: event.eventKind === "approval.requested" && approvalKind === "plugin_lifecycle" ? "Plugin lifecycle"
			: event.eventKind === "approval.requested" ? "Approval requested"
			: event.eventKind === "approval.granted" ? "Approval granted"
				: event.eventKind === "approval.rejected" ? "Approval rejected"
					: event.eventKind === "approval.expired" ? "Approval expired" : "Approval updated";
		const command = typeof payload.command === "string" ? payload.command : undefined;
		const typedDetail = approvalKind === "credential_access"
			? [payload.provider, payload.purpose].filter((value): value is string => typeof value === "string").join(" · ")
			: approvalKind === "sidecar_lifecycle"
				? [payload.sidecar, payload.action].filter((value): value is string => typeof value === "string").join(" · ")
				: undefined;
		const pluginDetail = payload.kind === "plugin_lifecycle"
			? [
				payload.action,
				payload.pluginId,
				payload.version,
				payload.publisher,
				payload.digest,
				payload.networkHost,
				payload.serviceEffect,
				payload.retention,
				typeof payload.activeRuns === "number" ? `${payload.activeRuns} active runs` : null
			].filter((value): value is string => typeof value === "string" && value !== "").join(" · ")
			: payload.kind === "paid_compute"
				? [
					payload.recipeId ?? payload.operation,
					payload.dataset,
					payload.proposerModel ? `proposer ${payload.proposerModel}` : null,
					payload.evaluatorModel ? `evaluator ${payload.evaluatorModel}` : null,
					payload.requestedCap && typeof payload.requestedCap === "object"
						? `max ${String((payload.requestedCap as { maxRollouts?: number }).maxRollouts ?? "")} rollouts`
						: null,
					Array.isArray(payload.credentialNames) ? `credentials ${payload.credentialNames.join(", ")}` : null
				].filter((value): value is string => Boolean(value)).join(" · ")
				: undefined;
		const safeKind = payload.kind === "shell_command" || payload.kind === "file_change" || payload.kind === "permission"
			|| payload.kind === "plugin_lifecycle" || payload.kind === "paid_compute";
		const detail = typedDetail
			?? pluginDetail
			?? (safeKind && typeof payload.detail === "string"
				? payload.detail.slice(0, 500)
				: command ? redactCommand(command) : path);
		(byMessage[current] ??= []).push({
			id: `activity-${event.sequence}`,
			label,
			placement: current === "__active__" ? undefined : "after",
			sequence: event.sequence,
			approvalId: event.eventKind === "approval.requested"
				? approvalKey(event) ?? `approval-${event.sequence}`
				: undefined,
			approvalKind: approvalKind === "shell_command" || approvalKind === "paid_compute" || approvalKind === "sidecar_lifecycle" || approvalKind === "credential_access" || approvalKind === "plugin_lifecycle"
				? approvalKind : "permission",
			approvalPayload: event.eventKind === "approval.requested" && approvalKind === "paid_compute" ? {
				operation: typeof payload.operation === "string" ? payload.operation : undefined,
				parameters: payload.parameters && typeof payload.parameters === "object" && !Array.isArray(payload.parameters) ? payload.parameters as Record<string, unknown> : undefined,
				estimatedCostUsdMicros: typeof payload.estimatedCostUsdMicros === "number" ? payload.estimatedCostUsdMicros : undefined,
				requestedCap: payload.requestedCap && typeof payload.requestedCap === "object" && !Array.isArray(payload.requestedCap)
					? payload.requestedCap as { maxCostUsdMicros?: number; maxRollouts?: number }
					: undefined,
				requestingAgent: typeof payload.requestingAgent === "string" ? payload.requestingAgent : undefined
			} : undefined,
			alwaysAllowSupported: event.eventKind === "approval.requested" && payload.alwaysSupported === true,
			detail,
			path,
			kind: activityKind(event.eventKind)
		});
	}
	if (runStartedAt && lastTokenTotal != null) {
		const tail = byMessage[current]?.at(-1);
		if (tail) tail.tokenTotal = lastTokenTotal;
	}
	for (const [messageId, lines] of Object.entries(byMessage)) {
		byMessage[messageId] = lines.filter((line, index) => {
			const previous = lines[index - 1];
			return !previous || previous.kind !== line.kind || previous.label !== line.label || previous.detail !== line.detail;
		});
	}
	const enforce =
		options?.enforcePlacementInvariant
		?? (typeof import.meta !== "undefined" && Boolean(import.meta.env?.DEV));
	if (enforce) {
		assertLocalActivityPlacementInvariant(messages, byMessage, lastContentSequenceByMessageId);
	}
	return byMessage;
}

export { assertLocalActivityPlacementInvariant } from "./activityPlacementInvariant";

/** Operations that make a visual *this chat's* output.
 *
 * The durable visual registry is instance-global, so a visual returned by
 * `list`, `get`, `show`, `capture_review`, or `review` is very often someone
 * else's. Adopting on any call that happened to carry a visual record is how a
 * chat claimed outputs it had merely looked at. Reading is not owning. */
const VISUAL_OWNERSHIP_OPERATIONS = new Set([
	"create",
	"create_from_template",
	"update",
	"bind",
	"bind_data_source",
	"save",
	"save_tsx",
	"fork",
	"archive"
]);

const VISUAL_OWNERSHIP_TOOLS = new Set([
	"visual_create",
	"visual_create_from_template",
	"visual_update",
	"visual_bind_data_source",
	"visual_save",
	"visual_save_tsx",
	"visual_fork",
	"visual_archive"
]);

function toolClaimsVisualOwnership(tool: string, args: Record<string, unknown>): boolean {
	// `visual_manage` is one advertised tool covering every operation, so its
	// name alone says nothing about whether this call authored anything.
	if (tool === "visual_manage") {
		const operation = (stringField(args, "operation") ?? "").toLowerCase();
		return VISUAL_OWNERSHIP_OPERATIONS.has(operation);
	}
	return VISUAL_OWNERSHIP_TOOLS.has(tool);
}

/** A visual belongs to the session that created it, and to no other.
 *
 * A visual with no recorded owner stays adoptable, so chats that predate
 * ownership keep their own rails. One that names a *different* owner is never
 * adopted, however it arrived here. */
function visualBelongsToSession(visual: Record<string, unknown>, sessionId: string): boolean {
	const owner = stringField(visual, "sessionId", "session_id", "ownerSessionId", "owner_session_id");
	return !owner || owner === sessionId;
}

function toolResultToArtifact(event: RuntimeEvent): ArtifactRef | undefined {
	const item = eventItem(event);
	const server = (stringField(item, "server", "pluginId", "plugin_id") ?? "").toLowerCase();
	const tool = (stringField(item, "tool", "name", "toolName", "tool_name") ?? "").toLowerCase();
	if (server !== "synth_visuals" || !VISUAL_MUTATION_TOOLS.has(tool)) return undefined;
	const args = parseJsonObject(item.arguments) ?? {};
	if (!toolClaimsVisualOwnership(tool, args)) return undefined;
	const visual = visualFromToolResult(item);
	if (!visual) return undefined;
	if (!visualBelongsToSession(visual, event.sessionId)) return undefined;
	const id = stringField(visual, "id", "visualId", "visual_id");
	const templateId = stringField(visual, "templateId", "template_id");
	if (!id || !templateId) return undefined;
	const title = stringField(visual, "title") ?? "Visual";
	const metadata = objectValue(visual.metadata);
	const durableStatus = stringField(visual, "status");
	const reviewReceipts = metadata && Array.isArray(metadata.reviews) ? metadata.reviews.length : 0;
	const status: ArtifactRef["status"] = durableStatus === "failed"
		? "failed"
		: durableStatus === "live" || durableStatus === "saved"
			? "ready"
			: reviewReceipts > 0
				? "review"
				: "draft";
	return {
		id,
		kind: "report",
		title,
		summary: stringField(metadata ?? {}, "summary"),
		messageId: stringField(visual, "messageId", "message_id"),
		shownByAgent: true,
		templateId,
		visualId: id,
		bindings: objectValue(visual.bindings),
		metadata,
		status,
		preview: {
			variant: templateId.includes("scrub") || templateId.includes("rollout")
				? "craftax_frame"
				: templateId.includes("craftax") || templateId.includes("eval_matrix")
					? "craftax_pareto"
					: "generic"
		}
	};
}

export function eventsToArtifacts(events: RuntimeEvent[]): ArtifactRef[] {
	const artifacts = new Map<string, ArtifactRef>();
	for (const event of events) {
		const toolArtifact = toolResultToArtifact(event);
		if (toolArtifact) {
			artifacts.set(toolArtifact.id, toolArtifact);
			continue;
		}
		if (
			event.eventKind !== "visual.created" &&
			event.eventKind !== "visual.show" &&
			event.eventKind !== "resource_ref.created"
		) {
			continue;
		}
		const payload = event.payload ?? {};
		// Showing a visual puts it in the pane; it does not make it this chat's
		// output. The show event is journaled against whoever opened it, so the
		// record's own owner is the only authority on that.
		if (event.eventKind === "visual.show" && !visualBelongsToSession(payload, event.sessionId)) {
			continue;
		}
		const id =
			typeof payload.visualId === "string"
				? payload.visualId
				: typeof payload.id === "string"
					? payload.id
					: `visual-${event.sequence}`;
		const title =
			typeof payload.title === "string" ? payload.title : "Visual";
		const templateId =
			typeof payload.templateId === "string"
				? payload.templateId
				: artifacts.get(id)?.templateId;
		const prior = artifacts.get(id);
		artifacts.set(id, {
			id,
			kind: "report",
			title: typeof payload.title === "string" ? payload.title : prior?.title ?? title,
			summary:
				typeof payload.summary === "string" ? payload.summary : prior?.summary,
			messageId:
				typeof payload.messageId === "string" ? payload.messageId : prior?.messageId,
			shownByAgent: true,
			templateId,
			visualId: id,
			bindings:
				payload.bindings && typeof payload.bindings === "object"
					? (payload.bindings as Record<string, unknown>)
					: prior?.bindings,
			preview: {
				variant: templateId?.includes("scrub") ? "craftax_frame" : "generic"
			}
		});
	}
	const agents = eventsToSubagents(events);
	if (agents.length) {
		artifacts.set("codex-subagents", {
			id: "codex-subagents",
			kind: "report",
			title: "Subagents",
			summary: `${agents.filter((agent) => agent.status === "starting" || agent.status === "working").length} working · ${agents.filter((agent) => agent.status === "interrupted" || agent.status === "failed" || agent.status === "stopped" || agent.status === "unavailable").length} need attention · ${agents.filter((agent) => agent.status === "completed").length} completed`,
			shownByAgent: true,
			templateId: "synth.subagents.v1",
			bindings: { agents, sessionId: events[0]?.sessionId },
			preview: { variant: "generic" }
		});
	}
	const result = [...artifacts.values()];
	const subagents = result.findIndex((artifact) => artifact.id === "codex-subagents");
	if (subagents > 0) result.unshift(result.splice(subagents, 1)[0]);
	return result;
}

export function visualRecordToArtifact(visual: VisualInstanceRecord): ArtifactRef {
	return {
		id: visual.id,
		kind: "report",
		title: visual.title,
		templateId: visual.templateId,
		visualId: visual.id,
		rendererKind: typeof visual.metadata?.rendererKind === "string" ? visual.metadata.rendererKind : undefined,
		bindings: visual.bindings,
		preview: {
			variant:
				visual.templateId.includes("scrub") || visual.templateId.includes("rollout")
					? "craftax_frame"
					: visual.templateId.includes("craftax") ||
						  visual.templateId.includes("eval_matrix")
						? "craftax_pareto"
						: "generic"
		}
	};
}

export function healthToModelStatus(
	health: RuntimeHealth | null,
	laguna?: {
		phase: string;
		detail?: string | null;
		loadedModel?: string | null;
	} | null
): {
	status: ModelStatus;
	name: string;
	composerEnabled: boolean;
	composerPlaceholder: string;
	detail?: string;
} {
	const name = "Laguna-XS-2.1";
	if (laguna?.phase === "starting") {
		return {
			status: "starting",
			name,
			composerEnabled: health?.openrouter.mode === "ready",
			composerPlaceholder: "Starting Laguna XS…",
			detail: laguna.detail || "Starting Laguna sidecar…"
		};
	}
	if (laguna?.phase === "loading") {
		return {
			status: "loading",
			name,
			composerEnabled: health?.openrouter.mode === "ready",
			composerPlaceholder: "Loading Laguna XS…",
			detail: laguna.detail || "Loading model weights…"
		};
	}
	if (laguna?.phase === "error") {
		return {
			status: "error",
			name,
			composerEnabled: health?.openrouter.mode === "ready",
			composerPlaceholder: "Laguna unavailable — pick OpenRouter or retry",
			detail: laguna.detail || "Laguna sidecar error"
		};
	}
	if (laguna?.phase === "not_installed") {
		return {
			status: "not_installed",
			name,
			composerEnabled: health?.openrouter.mode === "ready",
			composerPlaceholder: "Download Laguna XS in Settings → Models",
			detail: laguna.detail || "Laguna XS is not installed"
		};
	}
	// Native Tauri sessions do not require the legacy Python runtime health
	// endpoint. A ready Laguna sidecar is authoritative on its own.
	if (laguna?.phase === "ready" || health?.local.mode === "mlx") {
		return {
			status: "ready",
			name,
			composerEnabled: true,
			composerPlaceholder: "Ask Laguna something…",
			detail: "Laguna XS ready"
		};
	}
	if (!health) {
		return {
			status: "starting",
			name,
			composerEnabled: false,
			composerPlaceholder: "Connecting to local runtime…",
			detail: "Connecting to local runtime…"
		};
	}
	if (health.local.mode === "absent") {
		return {
			status: "not_installed",
			name,
			composerEnabled: health.openrouter.mode === "ready",
			composerPlaceholder: "Local Laguna not ready — use OpenRouter or wait…",
			detail: "Local Laguna sidecar not connected"
		};
	}
	return {
		status: "not_installed",
		name,
		composerEnabled: health.openrouter.mode === "ready",
		composerPlaceholder: "Select a model…",
		detail: "Laguna not available"
	};
}

/**
 * Per-session view slice cache.
 *
 * Token events append to one session's events array (new reference for that
 * key only). Caching by `(session, events, codexActivity)` identity means
 * `buildLandingState` recomputes only the dirty session on each store commit
 * instead of O(sessions × events) for every token.
 *
 * Partial: the LandingState aggregate still reallocates the chats/sync arrays
 * on every call; only the expensive events→messages/activity/artifacts work is
 * memoized per session. Prefer `useSessionEvents` + `buildSessionViewSlice` in
 * active-chat surfaces when you need a single-session subscription.
 */
type SessionViewSliceCacheKey = {
	session: Session;
	events: RuntimeEvent[];
	codexActivity: CodexActivityEvent[];
	reasoningDisplay: "none" | "summary" | "full";
};

type SessionViewSlice =
	| { kind: "chat"; chat: LocalChat }
	| { kind: "sync"; session: SyncSession }
	| { kind: "async"; intern: AsyncInternPin; isRustIntern: boolean };

const sessionViewSliceCache = new WeakMap<object, { key: SessionViewSliceCacheKey; value: SessionViewSlice | null }>();

function artifactEventsForSession(
	events: RuntimeEvent[],
	codexActivity: CodexActivityEvent[]
): RuntimeEvent[] {
	return [
		...events,
		...codexActivity.map(
			(event, index): RuntimeEvent => ({
				schemaVersion: "synth.desktop-runtime-event.v1",
				sessionId: event.sessionId,
				sequence: events.length + index + 1,
				eventKind: event.eventKind,
				payload: event.payload,
				createdAt: event.createdAt,
				source: "intern"
			})
		)
	].sort((left, right) => left.createdAt.localeCompare(right.createdAt));
}

/** Build (or reuse) the expensive per-session projection. */
export function buildSessionViewSlice(
	session: Session,
	events: RuntimeEvent[],
	codexActivity: CodexActivityEvent[] = []
): SessionViewSlice | null {
	const reasoningDisplay =
		modelCapabilitiesForExecutionTarget(session.target)?.reasoningDisplay ?? "none";
	const cached = sessionViewSliceCache.get(events);
	if (
		cached &&
		cached.key.session === session &&
		cached.key.events === events &&
		cached.key.codexActivity === codexActivity &&
		cached.key.reasoningDisplay === reasoningDisplay
	) {
		return cached.value;
	}

	const messages = eventsToMessages(events);
	const artifacts = eventsToArtifacts(artifactEventsForSession(events, codexActivity));
	let value: SessionViewSlice | null = null;

	if (sessionIsLocalChat(session)) {
		value = {
			kind: "chat",
			chat: {
				id: session.id,
				title: session.title,
				messages,
				artifacts,
				activityByMessageId: eventsToLocalActivity(events, messages, reasoningDisplay)
			}
		};
	} else if (sessionIsSync(session)) {
		value = {
			kind: "sync",
			session: {
				id: session.id,
				title: session.title,
				status: mapSessionStatus(session.status),
				remoteId: session.remoteId ?? session.id,
				cursor: session.latestCursor,
				messages,
				activity: eventsToActivity(events, codexActivity),
				artifacts
			}
		};
	} else if (sessionIsAsync(session)) {
		const activity = eventsToActivity(events, codexActivity);
		const phase = mapAsyncPhase(session.status, events);
		value = {
			kind: "async",
			isRustIntern: session.metadata.runtime === "rust-intern",
			intern: {
				phase,
				summary:
					session.status === "waiting_for_input"
						? "Waiting for operator input"
						: phase === "sleeping"
							? "Checkpoint saved · waiting for the next cycle"
							: session.title,
				needsInput: session.status === "waiting_for_input",
				leaveSafe: true,
				remoteId: session.remoteId ?? session.id,
				cursor: session.latestCursor,
				messages,
				activity
			}
		};
	}

	sessionViewSliceCache.set(events, {
		key: { session, events, codexActivity, reasoningDisplay },
		value
	});
	return value;
}

export function buildLandingState(args: {
	health: RuntimeHealth | null;
	sessions: Session[];
	eventsBySession: Record<string, RuntimeEvent[]>;
	codexActivityBySession?: Record<string, CodexActivityEvent[]>;
	selectedTargetId: string;
	laguna?: {
		phase: string;
		detail?: string | null;
		loadedModel?: string | null;
	} | null;
	apiKeyConfigured?: boolean;
	openrouterApiKeyConfigured?: boolean;
	codexOauthConfigured?: boolean;
	codexOauthStatus?: import("../bridge").CodexOauthStatus;
	cloudBlockedReason?: string | null;
}): LandingState {
	const model = healthToModelStatus(args.health, args.laguna);
	const chats: LocalChat[] = [];
	const syncSessions: SyncSession[] = [];
	let asyncIntern: AsyncInternPin | null = null;

	for (const session of args.sessions) {
		const events = args.eventsBySession[session.id] ?? [];
		const codexActivity = args.codexActivityBySession?.[session.id] ?? [];
		const slice = buildSessionViewSlice(session, events, codexActivity);
		if (!slice) continue;

		if (slice.kind === "chat") {
			chats.push(slice.chat);
			continue;
		}
		if (slice.kind === "sync") {
			syncSessions.push(slice.session);
			continue;
		}
		// A migrated demo pin may coexist with the real organization singleton.
		// Once a Rust-backed binding exists it is the canonical Background
		// Intern projection and must not be overwritten by historical demo data.
		if (asyncIntern && !slice.isRustIntern) continue;
		asyncIntern = slice.intern;
	}

	return {
		id: "landing-ready",
		label: "Runtime",
		chats,
		syncSessions,
		asyncIntern,
		model: {
			status: model.status,
			name: model.name,
			detail: model.detail
		},
		selectedTargetId: args.selectedTargetId,
		internMode: args.health?.intern.mode,
		apiKeyConfigured: args.apiKeyConfigured,
		openrouterApiKeyConfigured: args.openrouterApiKeyConfigured ?? args.health?.openrouter.mode === "ready",
		codexOauthConfigured: args.codexOauthConfigured,
		codexOauthStatus: args.codexOauthStatus,
		cloudBlockedReason: args.cloudBlockedReason ?? null,
		composerEnabled: model.composerEnabled,
		composerPlaceholder: model.composerPlaceholder
	};
}
