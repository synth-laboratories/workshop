import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type { RuntimeEvent } from "@synth/runtime-protocol";
import type { CodexEvent, CodexSessionInfo, LagunaStatus, PersistedCodexSession, RequestOptions, RuntimeBridge, TerminalEvent, TerminalInfo } from "../env";

type RuntimeEventEnvelope = {
	subscriptionId: string;
	event?: RuntimeEvent;
	type: "event" | "status";
	status?: { state: string; detail?: string };
};

const isTauri = "__TAURI_INTERNALS__" in window;

function browserRuntimeBridge(): RuntimeBridge {
	return {
		async request<T>(path: string, options: RequestOptions = {}): Promise<T> {
			const response = await fetch(`/__runtime${path}`, {
				method: options.method ?? "GET",
				headers: options.body === undefined ? undefined : { "Content-Type": "application/json" },
				body: options.body === undefined ? undefined : JSON.stringify(options.body)
			});
			if (!response.ok) throw new Error(`Runtime request failed (${response.status})`);
			return response.json() as Promise<T>;
		},
		async subscribe(sessionId, afterSequence, onEvent, onStatus) {
			let closed = false;
			let cursor = afterSequence;
			onStatus?.({ state: "connected" });
			const poll = async () => {
				if (closed) return;
				try {
					const page = await this.request<{ events: RuntimeEvent[] }>(
						`/v1/sessions/${encodeURIComponent(sessionId)}/events?after_sequence=${cursor}&limit=500`
					);
					for (const event of page.events) {
						cursor = Math.max(cursor, event.sequence);
						onEvent(event);
					}
				} catch (reason) {
					onStatus?.({ state: "reconnecting", detail: String(reason) });
				}
				if (!closed) window.setTimeout(poll, 100);
			};
			void poll();
			return { close: () => { closed = true; } };
		}
	};
}

function tauriRuntimeBridge(): RuntimeBridge {
	return {
		request<T>(path: string, options: RequestOptions = {}): Promise<T> {
			return invoke<T>("runtime_request", {
				request: { path, method: options.method ?? "GET", body: options.body }
			});
		},
		async subscribe(sessionId, afterSequence, onEvent, onStatus) {
			const subscriptionId = crypto.randomUUID();
			const unlistenEvent = await listen<RuntimeEventEnvelope>("runtime:subscription", ({ payload }) => {
				if (payload.subscriptionId !== subscriptionId) return;
				if (payload.type === "event" && payload.event) onEvent(payload.event);
				if (payload.type === "status" && payload.status) onStatus?.(payload.status);
			});
			await invoke("runtime_subscribe", {
				request: { subscriptionId, sessionId, afterSequence }
			});
			return {
				close() {
					unlistenEvent();
					void invoke("runtime_unsubscribe", { subscriptionId });
				}
			};
		}
	};
}

const unavailableLaguna: LagunaStatus = {
	phase: "unavailable",
	baseUrl: null,
	backend: null,
	loadedModel: null,
	detail: "Laguna status is unavailable in the browser fixture",
	memoryBytes: null,
	updatedAt: Date.now()
};

/** Installs the legacy window contract consumed by runtime-client at the Tauri boundary. */
export function installDesktopBridge(): void {
	window.synthRuntime ??= isTauri ? tauriRuntimeBridge() : browserRuntimeBridge();
	window.synthDesktop ??= {
		platform: navigator.platform,
		chooseProjectDirectory: async () => {
			if (!isTauri) return null;
			const selection = await invoke<string | null>("project_choose_directory").catch(() =>
				open({ directory: true, multiple: false })
			);
			return typeof selection === "string" ? selection : null;
		}
	};
	window.synthLaguna ??= isTauri
		? {
			getStatus: () => invoke<LagunaStatus>("laguna_get_status"),
			onStatus(listener) {
				let disposed = false;
				let unlisten: (() => void) | undefined;
				void listen<LagunaStatus>("laguna:status", ({ payload }) => listener(payload)).then((next) => {
					if (disposed) next();
					else unlisten = next;
				});
				return () => { disposed = true; unlisten?.(); };
			}
		}
		: {
			getStatus: async () => unavailableLaguna,
			onStatus: () => () => undefined
		};
	window.synthTerminal ??= isTauri
		? {
			available: true,
			create: (request) => invoke<TerminalInfo>("terminal_create", { request }),
			list: (workspaceId) => invoke<TerminalInfo[]>("terminal_list", { workspaceId }),
			snapshot: (terminalId, afterSequence = 0) => invoke<TerminalEvent[]>("terminal_snapshot", { terminalId, afterSequence }),
			write: (terminalId, data) => invoke<void>("terminal_write", { terminalId, data }),
			resize: (terminalId, cols, rows) => invoke<void>("terminal_resize", { terminalId, cols, rows }),
			close: (terminalId) => invoke<void>("terminal_close", { terminalId }),
			onEvent(listener) {
				let unlisten: (() => void) | undefined;
				let disposed = false;
				void listen<TerminalEvent>("terminal:event", ({ payload }) => listener(payload)).then((next) => disposed ? next() : (unlisten = next));
				return () => { disposed = true; unlisten?.(); };
			}
		}
		: {
			available: false,
			create: async () => { throw new Error("Terminal is available in the desktop app"); },
			list: async () => [],
			snapshot: async () => [],
			write: async () => undefined,
			resize: async () => undefined,
			close: async () => undefined,
			onEvent: () => () => undefined
		};
	if (isTauri) {
		window.synthCodex ??= {
			defaultWorkspace: () => invoke<string>("codex_default_workspace"),
			list: () => invoke<PersistedCodexSession[]>("codex_sessions_list"),
			start: (request) => invoke<CodexSessionInfo>("codex_session_start", { request }),
			startTurn: (sessionId, prompt) =>
				invoke<CodexSessionInfo>("codex_turn_start", { request: { sessionId, prompt } }),
			interrupt: (sessionId) => invoke<void>("codex_turn_interrupt", { request: { sessionId } }),
			close: (sessionId) => invoke<void>("codex_session_close", { request: { sessionId } }),
			onEvent(listener) {
				let disposed = false;
				let unlisten: (() => void) | undefined;
				void listen<CodexEvent>("codex:event", ({ payload }) => listener(payload)).then((next) => {
					if (disposed) next();
					else unlisten = next;
				});
				return () => { disposed = true; unlisten?.(); };
			}
		};
	}
}
