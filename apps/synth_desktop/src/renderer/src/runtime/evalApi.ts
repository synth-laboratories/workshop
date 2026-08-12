import type {
	SemanticUiSnapshot,
	Session,
	VisualInstanceRecord,
	VisualRecord
} from "@synth/runtime-protocol";
import type { RuntimeEvent } from "@synth/runtime-protocol";
import { sessionIsLocalChat, sessionIsSync } from "./sessionView";
import type { MainView } from "../routes";

export type SemanticEvalApi = {
	schemaVersion: "synth.desktop-eval-api.v1";
	getState(): SemanticUiSnapshot;
	listActions(): string[];
	invoke(action: string, argumentsValue?: Record<string, unknown>): Promise<unknown>;
};

export type SemanticEvalHost = {
	activeSessionId: string | null;
	sessions: Session[];
	visibleEvents: RuntimeEvent[];
	openArtifactId: string | null;
	view: MainView;
	busy: boolean;
	showComposer: boolean;
	selectedTargetId: string;
	createConversation: (
		targetId?: string,
		title?: string,
		objective?: string
	) => Promise<Session>;
	sendToSession: (sessionId: string, text: string) => Promise<boolean>;
	openVisualRecord: (visual: VisualInstanceRecord | VisualRecord) => void;
	openChat: (chatId: string) => void;
	setView: (view: MainView) => void;
};

const ACTIONS = [
	"create_session",
	"send_message",
	"open_visual",
	"list_inventory",
	"select_session",
	"wait_for_terminal",
	"export_session"
] as const;

/** Pure factory for `window.__synthEval` — keeps the poll/export loops out of App.tsx. */
export function createSemanticEvalApi(host: SemanticEvalHost): SemanticEvalApi {
	const inventoryTab = host.view.kind === "inventory" ? ("containers" as const) : null;
	return {
		schemaVersion: "synth.desktop-eval-api.v1",
		getState: () => ({
			schemaVersion: "synth.desktop-semantic-ui.v1",
			selectedSessionId: host.activeSessionId,
			sessions: host.sessions,
			visibleEvents: host.visibleEvents,
			openVisualId: host.openArtifactId,
			inventoryTab,
			controls: [
				{
					id: "new-conversation",
					role: "button",
					name: "New conversation",
					enabled: !host.busy
				},
				{
					id: "composer-input",
					role: "textbox",
					name: "Message composer",
					enabled: host.showComposer && !host.busy
				},
				{
					id: "composer-send",
					role: "button",
					name: "Send",
					enabled: host.showComposer && !host.busy
				},
				{
					id: "open-inventory",
					role: "button",
					name: "Inventory",
					enabled: true
				}
			]
		}),
		listActions: () => [...ACTIONS],
		invoke: async (action, args = {}) => {
			if (action === "create_session") {
				const target =
					typeof args.targetId === "string"
						? args.targetId
						: typeof args.target === "string"
							? args.target
							: host.selectedTargetId;
				const objective = typeof args.objective === "string" ? args.objective : undefined;
				return host.createConversation(target, undefined, objective);
			}
			if (action === "send_message") {
				if (typeof args.body !== "string") throw new Error("send_message requires body");
				const sessionId =
					typeof args.sessionId === "string" ? args.sessionId : host.activeSessionId;
				if (!sessionId) throw new Error("send_message requires an active session");
				await host.sendToSession(sessionId, args.body);
				return { ok: true };
			}
			if (action === "open_visual") {
				const visualId = args.visualId;
				if (typeof visualId !== "string") throw new Error("open_visual requires visualId");
				if (!window.synthVisuals) throw new Error("Rust visual registry is unavailable");
				const visual = await window.synthVisuals.get(visualId);
				host.openVisualRecord(visual);
				return visual;
			}
			if (action === "list_inventory") {
				if (!window.synthInventory || !window.synthVisuals) {
					throw new Error("Rust inventory is unavailable");
				}
				const [containers, traces, visuals] = await Promise.all([
					window.synthInventory.listContainers(),
					window.synthInventory.listTraces(),
					window.synthVisuals.list({ limit: 500 })
				]);
				return { containers, traces, visuals };
			}
			if (action === "select_session") {
				const sessionId = args.sessionId;
				if (typeof sessionId !== "string") {
					throw new Error("select_session requires sessionId");
				}
				const session = host.sessions.find((item) => item.id === sessionId);
				if (!session) throw new Error("session not found");
				if (sessionIsLocalChat(session)) host.openChat(sessionId);
				else if (sessionIsSync(session)) host.setView({ kind: "sync", sessionId });
				else host.setView({ kind: "async", sessionId });
				return { selectedSessionId: sessionId };
			}
			if (action === "wait_for_terminal") {
				const sessionId =
					typeof args.sessionId === "string" ? args.sessionId : host.activeSessionId;
				if (!sessionId) throw new Error("wait_for_terminal requires sessionId");
				if (!window.synthCore) throw new Error("Rust journal is unavailable");
				const timeoutMs = typeof args.timeoutMs === "number" ? args.timeoutMs : 600_000;
				const pollMs = typeof args.pollMs === "number" ? args.pollMs : 500;
				const deadline = Date.now() + timeoutMs;
				let after = 0;
				while (Date.now() < deadline) {
					const page = await window.synthCore.sessionEventsAfter(sessionId, after, 500);
					for (const event of page) {
						after = Math.max(after, event.sessionSequence ?? event.sequence);
						const kind = event.kind;
						if (
							kind === "run.completed" ||
							kind === "run.failed" ||
							kind === "run.cancelled" ||
							kind === "session.run.completed" ||
							kind === "session.run.failed"
						) {
							return { terminal: true, kind, event, sessionId };
						}
					}
					await new Promise((resolve) => setTimeout(resolve, pollMs));
				}
				return { terminal: false, timedOut: true, sessionId, afterSequence: after };
			}
			if (action === "export_session") {
				const sessionId =
					typeof args.sessionId === "string" ? args.sessionId : host.activeSessionId;
				if (!sessionId) throw new Error("export_session requires sessionId");
				if (!window.synthCore) throw new Error("Rust journal is unavailable");
				const events = [];
				let after = 0;
				for (;;) {
					const page = await window.synthCore.sessionEventsAfter(sessionId, after, 500);
					if (!page.length) break;
					for (const event of page) {
						after = Math.max(after, event.sessionSequence ?? event.sequence);
					}
					events.push(...page);
					if (events.length > 50_000) break;
				}
				const session = host.sessions.find((item) => item.id === sessionId) ?? null;
				return {
					schemaVersion: "synth.eval-session-export.v1",
					sessionId,
					session,
					events,
					eventCount: events.length
				};
			}
			throw new Error(`Unknown semantic action: ${action}`);
		}
	};
}
