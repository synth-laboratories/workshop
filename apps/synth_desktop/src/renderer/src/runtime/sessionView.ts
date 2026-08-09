import type {
	ExecutionTarget,
	RuntimeEvent,
	RuntimeHealth,
	Session,
	VisualInstanceRecord
} from "@synth/runtime-protocol";
import {
	AVAILABLE_LORAS,
	LORA_NONE,
	OPENROUTER_LAGUNA_S_MODEL,
	OPENROUTER_LUNA_MODEL,
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

export function targetIdToExecutionTarget(
	targetId: string,
	loraId: string = LORA_NONE
): ExecutionTarget {
	const adapter =
		loraId !== LORA_NONE
			? (AVAILABLE_LORAS.find((l) => l.id === loraId)?.name ?? null)
			: null;

	switch (targetId) {
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
	if (target.model === OPENROUTER_LUNA_MODEL || target.model.includes("kimi")) {
		return "openrouter-luna";
	}
	return "openrouter-laguna-s";
}

export function sessionIsLocalChat(session: Session): boolean {
	return session.target.kind === "local" || session.target.kind === "remote";
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

	for (const event of events) {
		const payload = event.payload ?? {};
		const isInternAgentMessage =
			event.eventKind === "agent_message" ||
			event.eventKind === "intern.agent_message";
		const messageId =
			typeof payload.messageId === "string"
				? payload.messageId
				: isInternAgentMessage && typeof payload.id === "string"
					? payload.id
					: isInternAgentMessage && typeof payload.intern === "object" && payload.intern !== null &&
						  typeof (payload.intern as Record<string, unknown>).eventId === "string"
						? String((payload.intern as Record<string, unknown>).eventId)
				: `evt-${event.sequence}`;
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
			if (!content) continue;
			if (!byId.has(messageId)) {
				order.push(messageId);
				byId.set(messageId, {
					id: messageId,
					role: isInternAgentMessage ? "assistant" : role,
					body: content,
					at: event.createdAt
				});
			} else {
				const existing = byId.get(messageId)!;
				byId.set(messageId, { ...existing, role, body: content || existing.body });
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
			continue;
		}

		if (event.eventKind === "message.completed") {
			const content =
				typeof payload.content === "string"
					? payload.content
					: typeof payload.body === "string"
						? payload.body
						: undefined;
			const existing = byId.get(messageId);
			if (existing) {
				byId.set(messageId, {
					...existing,
					role: "assistant",
					body: content ?? existing.body
				});
			} else if (content) {
				order.push(messageId);
				byId.set(messageId, {
					id: messageId,
					role: "assistant",
					body: content,
					at: event.createdAt
				});
			}
		}
	}

	return order.map((id) => byId.get(id)!).filter(Boolean);
}

export function eventsToActivity(events: RuntimeEvent[]): ActivityEvent[] {
	return events
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
}

function activityKind(eventKind: string): LocalActivityLine["kind"] {
	if (eventKind === "thought.created" || eventKind.startsWith("thought.")) return "thought";
	if (eventKind === "file.read") return "file_read";
	if (eventKind === "file.written" || eventKind === "file.changed") return "file_write";
	if (eventKind.startsWith("command.")) return "command";
	if (eventKind === "visual.created") return "visual";
	return "working";
}

export function eventsToLocalActivity(
	events: RuntimeEvent[],
	messages: ChatMessage[]
): Record<string, LocalActivityLine[]> {
	const assistantIds = messages.filter((message) => message.role === "assistant").map((message) => message.id);
	if (!assistantIds.length) return {};
	const byMessage: Record<string, LocalActivityLine[]> = {};
	let current = assistantIds[0];
	for (const event of events) {
		const payload = event.payload ?? {};
		const explicit = typeof payload.messageId === "string" ? payload.messageId : null;
		if (explicit && assistantIds.includes(explicit)) current = explicit;
		if (event.eventKind.startsWith("message.") || event.eventKind === "run.completed" || event.eventKind === "run.started") continue;
		const path = typeof payload.path === "string" ? payload.path : undefined;
		const rawSummary =
			typeof payload.summary === "string" ? payload.summary :
			typeof payload.message === "string" ? payload.message :
			typeof payload.name === "string" ? payload.name : event.eventKind.replace(/\./g, " ");
		const label =
			event.eventKind === "thought.created" ? "Thinking" :
			event.eventKind === "tool.requested" ? `Tool · ${rawSummary}` :
			event.eventKind === "tool.completed" ? `Completed · ${rawSummary}` :
			event.eventKind === "approval.requested" ? "Approval requested" : rawSummary;
		const detail = typeof payload.detail === "string" ? payload.detail :
			typeof payload.output === "string" ? payload.output :
			typeof payload.content === "string" ? payload.content : undefined;
		(byMessage[current] ??= []).push({
			id: `activity-${event.sequence}`,
			label,
			detail,
			path,
			kind: activityKind(event.eventKind)
		});
	}
	return byMessage;
}

export function eventsToArtifacts(events: RuntimeEvent[]): ArtifactRef[] {
	const artifacts: ArtifactRef[] = [];
	for (const event of events) {
		if (
			event.eventKind !== "visual.created" &&
			event.eventKind !== "resource_ref.created"
		) {
			continue;
		}
		const payload = event.payload ?? {};
		const id =
			typeof payload.visualId === "string"
				? payload.visualId
				: typeof payload.id === "string"
					? payload.id
					: `visual-${event.sequence}`;
		const title =
			typeof payload.title === "string" ? payload.title : "Visual";
		const templateId =
			typeof payload.templateId === "string" ? payload.templateId : undefined;
		artifacts.push({
			id,
			kind: "report",
			title,
			summary:
				typeof payload.summary === "string" ? payload.summary : undefined,
			messageId:
				typeof payload.messageId === "string" ? payload.messageId : undefined,
			shownByAgent: true,
			templateId,
			visualId: id,
			bindings:
				payload.bindings && typeof payload.bindings === "object"
					? (payload.bindings as Record<string, unknown>)
					: undefined,
			preview: {
				variant: templateId?.includes("scrub") ? "craftax_frame" : "generic"
			}
		});
	}
	return artifacts;
}

export function visualRecordToArtifact(visual: VisualInstanceRecord): ArtifactRef {
	return {
		id: visual.id,
		kind: "report",
		title: visual.title,
		templateId: visual.templateId,
		visualId: visual.id,
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
	if (!health) {
		return {
			status: "starting",
			name,
			composerEnabled: false,
			composerPlaceholder: "Connecting to local runtime…",
			detail: "Connecting to local runtime…"
		};
	}
	if (health.local.mode === "mlx" || laguna?.phase === "ready") {
		return {
			status: "ready",
			name,
			composerEnabled: true,
			composerPlaceholder: "Ask Laguna something…",
			detail: "Laguna XS ready"
		};
	}
	if (health.local.mode === "stub") {
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

export function buildLandingState(args: {
	health: RuntimeHealth | null;
	sessions: Session[];
	eventsBySession: Record<string, RuntimeEvent[]>;
	selectedTargetId: string;
	selectedLoraId: string;
	projects?: { id: string; name: string }[];
	laguna?: {
		phase: string;
		detail?: string | null;
		loadedModel?: string | null;
	} | null;
}): LandingState {
	const model = healthToModelStatus(args.health, args.laguna);
	const chats: LocalChat[] = [];
	const syncSessions: SyncSession[] = [];
	let asyncIntern: AsyncInternPin | null = null;

	for (const session of args.sessions) {
		const events = args.eventsBySession[session.id] ?? [];
		const messages = eventsToMessages(events);
		const artifacts = eventsToArtifacts(events);

		if (sessionIsLocalChat(session)) {
			chats.push({
				id: session.id,
				title: session.title,
				messages,
				artifacts,
				activityByMessageId: eventsToLocalActivity(events, messages)
			});
			continue;
		}

		if (sessionIsSync(session)) {
			syncSessions.push({
				id: session.id,
				title: session.title,
				status: mapSessionStatus(session.status),
				remoteId: session.remoteId ?? session.id,
				cursor: session.latestCursor,
				messages,
				activity: eventsToActivity(events),
				artifacts
			});
			continue;
		}

		if (sessionIsAsync(session)) {
			const activity = eventsToActivity(events);
			const phase = mapAsyncPhase(session.status, events);
			asyncIntern = {
				phase,
				summary:
					session.status === "waiting_for_input"
						? "Waiting for operator input"
						: phase === "sleeping"
							? "Checkpoint saved · waiting for the next cycle"
						: session.title,
				needsInput: session.status === "waiting_for_input",
				remoteId: session.remoteId ?? session.id,
				cursor: session.latestCursor,
				messages,
				activity
			};
		}
	}

	return {
		id: "landing-ready",
		label: "Runtime",
		chats,
		syncSessions,
		asyncIntern,
		projects: args.projects ?? [],
		model: {
			status: model.status,
			name: model.name,
			detail: model.detail
		},
		selectedTargetId: args.selectedTargetId,
		internMode: args.health?.intern.mode,
		selectedLoraId: args.selectedLoraId,
		composerEnabled: model.composerEnabled,
		composerPlaceholder: model.composerPlaceholder
	};
}
