import type {
	EventPage,
	ExecutionTarget,
	RuntimeControlKind,
	RuntimeHealth,
	Session,
	VisualInstanceRecord
} from "@synth/runtime-protocol";

/**
 * Vite browser fixtures retain the old HTTP contract. The packaged Tauri app
 * never installs this bridge and therefore cannot call the Python runtime.
 */
export const browserRuntimeClient = {
	bridge() {
		if (!window.synthRuntime) throw new Error("Browser runtime fixture is unavailable");
		return window.synthRuntime;
	},
	async listSessions() {
		return (await this.bridge().request<{ sessions: Session[] }>("/v1/sessions")).sessions;
	},
	health() {
		return this.bridge().request<RuntimeHealth>("/v1/health");
	},
	createSession(target: ExecutionTarget, title?: string, objective?: string) {
		return this.bridge().request<Session>("/v1/sessions", {
			method: "POST",
			body: { target, title, projectId: null, objective }
		});
	},
	sendMessage(sessionId: string, body: string) {
		return this.bridge().request<{ runId: string }>(
			`/v1/sessions/${encodeURIComponent(sessionId)}/messages`,
			{ method: "POST", body: { body } }
		);
	},
	control(sessionId: string, kind: RuntimeControlKind, payload: Record<string, unknown>) {
		return this.bridge().request<{ accepted: boolean }>(
			`/v1/sessions/${encodeURIComponent(sessionId)}/commands`,
			{ method: "POST", body: { kind, payload } }
		);
	},
	events(sessionId: string, afterSequence: number, limit: number) {
		return this.bridge().request<EventPage>(
			`/v1/sessions/${encodeURIComponent(sessionId)}/events?after_sequence=${afterSequence}&limit=${limit}`
		);
	},
	subscribe: (...args: Parameters<NonNullable<typeof window.synthRuntime>["subscribe"]>) =>
		browserRuntimeClient.bridge().subscribe(...args),
	simulateLive(kind: string) {
		return this.bridge().request<{ visual: VisualInstanceRecord; eventCount: number }>(
			"/v1/visuals/simulate-live",
			{ method: "POST", body: { kind } }
		);
	}
};
