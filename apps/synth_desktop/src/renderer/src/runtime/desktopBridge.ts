import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type { AppEvent, InternSessionControlRequest, InternSessionCreateRequest, InternSessionSendRequest, RuntimeEvent, Session } from "@synth/runtime-protocol";
import type { CodexEvent, CodexSessionInfo, DesktopInstanceDiagnostics, InventoryCounts, LagunaModelHit, LagunaStatus, ModelMultiAgentSetting, PersistedCodexSession, RequestOptions, RuntimeBridge, SkillHit, SynthBackendSettings, SynthSignInBegin, SynthSignInPoll, TerminalEvent, TerminalInfo, VisualTemplateMeta, WhisperDownloadProgress, WhisperModelHit, WorkspaceAccessSettings } from "../env";
import type { CoreDiagnostics, VisualRecord, VisualRevision } from "@synth/runtime-protocol";
import type { ContainerDeployment, ResolvedTraceProjection, TraceBundleIngestResult, TraceV5Record, UsageLedgerEntry } from "@synth/runtime-protocol";

// The packaged WebKit view is always served from the `tauri:` protocol.  The
// injected internals global can appear too late for eager ES-module evaluation,
// so treating it as the only signal can accidentally install the browser/
// legacy-runtime bridge inside the desktop app.
const isTauri = window.location.protocol === "tauri:" || "__TAURI_INTERNALS__" in window;

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
		async subscribe(sessionId, afterSequence, onEvent, onStatus, _onActivity) {
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

function browserCoreBridge() {
	return {
		async diagnostics(): Promise<CoreDiagnostics> {
			return {
				databasePath: "browser-memory://core-runtime",
				schemaVersion: 0,
				integrityOk: true,
				contentStorePath: "browser-memory://content",
				journalHead: 0,
				sessionCount: 0,
				runCount: 0,
				visualCount: 0,
				migrationComplete: true
			};
		},
		async eventsAfter(): Promise<AppEvent[]> { return []; },
		async sessionEventsAfter(): Promise<AppEvent[]> { return []; },
		onEvent(): () => void { return () => undefined; }
	};
}

function legacyEventToAppEvent(event: RuntimeEvent): AppEvent {
	return {
		schemaVersion: "synth.desktop-app-event.v1",
		sequence: event.sequence,
		eventId: `legacy:${event.sessionId}:${event.sequence}`,
		sessionId: event.sessionId,
		sessionSequence: event.sequence,
		runId: event.runId,
		source: event.source,
		kind: event.eventKind,
		payload: event.payload,
		remoteSequence: event.remoteSequence,
		commandId: event.commandId,
		createdAt: event.createdAt
	};
}

function browserInternBridge() {
	return {
		async listSessions(): Promise<Session[]> {
			const result = await window.synthRuntime!.request<{ sessions: Session[] }>("/v1/sessions");
			return result.sessions.filter((session) => session.target.kind === "intern");
		},
		createSession(request: InternSessionCreateRequest): Promise<Session> {
			return window.synthRuntime!.request("/v1/sessions", { method: "POST", body: request });
		},
		send(request: InternSessionSendRequest): Promise<{ runId: string }> {
			return window.synthRuntime!.request(`/v1/sessions/${encodeURIComponent(request.sessionId)}/messages`, {
				method: "POST", body: { body: request.body }
			});
		},
		control(request: InternSessionControlRequest): Promise<{ accepted: boolean; receipt?: unknown }> {
			return window.synthRuntime!.request(`/v1/sessions/${encodeURIComponent(request.sessionId)}/commands`, {
				method: "POST", body: { kind: request.kind, payload: request.payload ?? {} }
			});
		},
		async eventsAfter(sessionId: string, afterSequence = 0, limit = 500): Promise<AppEvent[]> {
			const query = new URLSearchParams({ after_sequence: String(afterSequence), limit: String(limit) });
			const result = await window.synthRuntime!.request<{ events: RuntimeEvent[] }>(
				`/v1/sessions/${encodeURIComponent(sessionId)}/events?${query.toString()}`
			);
			return result.events.map(legacyEventToAppEvent);
		},
		onEvent(): () => void { return () => undefined; }
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

/** Installs Rust-owned desktop bridges; HTTP runtime compatibility is browser-only. */
export function installDesktopBridge(): void {
	if (!isTauri) window.synthRuntime ??= browserRuntimeBridge();
	window.synthDesktop ??= {
		platform: navigator.platform,
		getInstanceDiagnostics: () => isTauri
			? invoke<DesktopInstanceDiagnostics>("desktop_instance_diagnostics")
			: Promise.resolve({
				mode: "development", name: "browser", displayName: "Synth Desktop · browser",
				appVersion: "0.1.0", sourceRevision: "vite", buildRevision: "vite",
				buildTimestamp: "0", processId: 0, executable: "browser",
				dataRoot: "browser-memory://", viteUrl: window.location.origin, manifest: null
			}),
		chooseWorkspaceDirectory: async () => {
			if (!isTauri) return null;
			const selection = await invoke<string | null>("workspace_choose_directory").catch(() =>
				open({ directory: true, multiple: false })
			);
			return typeof selection === "string" ? selection : null;
		}
	};
	window.synthLaguna ??= isTauri
		? {
			getStatus: () => invoke<LagunaStatus>("laguna_get_status"),
			reload: () => invoke<LagunaStatus>("laguna_reload"),
			listModels: () => invoke<LagunaModelHit[]>("laguna_models_list"),
			chooseModelDirectory: async () => {
				const selection = await open({ directory: true, multiple: false, title: "Choose a Laguna model folder" });
				return typeof selection === "string" ? selection : null;
			},
			setModelDirectory: (path) => invoke<LagunaModelHit>("laguna_models_set_directory", { path }),
			clearModelDirectory: () => invoke<void>("laguna_models_clear_directory"),
			downloadModel: () => invoke<LagunaModelHit>("laguna_model_download"),
			onStatus(listener) {
				let disposed = false;
				let unlisten: (() => void) | undefined;
				const refresh = () => {
					void invoke<LagunaStatus>("laguna_get_status").then((status) => {
						if (!disposed) listener(status);
					}).catch(() => undefined);
				};
				const poll = window.setInterval(refresh, 5_000);
				void listen<LagunaStatus>("laguna:status", ({ payload }) => listener(payload)).then((next) => {
					if (disposed) next();
					else unlisten = next;
				});
				return () => { disposed = true; window.clearInterval(poll); unlisten?.(); };
			}
		}
		: {
			getStatus: async () => unavailableLaguna,
			reload: async () => unavailableLaguna,
			listModels: async () => [],
			downloadModel: async () => { throw new Error("Model downloads require Synth Desktop"); },
			chooseModelDirectory: async () => null,
			setModelDirectory: async () => { throw new Error("Model folders require the desktop app"); },
			clearModelDirectory: async () => undefined,
			onStatus: () => () => undefined
		};
	window.synthWhisper ??= isTauri
		? {
			listModels: () => invoke<WhisperModelHit[]>("whisper_models_list"),
			downloadModel: (id) => invoke<WhisperModelHit>("whisper_model_download", { id }),
			onDownloadProgress(listener) {
				let disposed = false;
				let unlisten: (() => void) | undefined;
				void listen<WhisperDownloadProgress>("whisper:download", ({ payload }) => listener(payload)).then((next) => {
					if (disposed) next();
					else unlisten = next;
				});
				return () => { disposed = true; unlisten?.(); };
			},
			setSelected: (id) => invoke<void>("whisper_models_set_selected", { id }),
			clearModel: (id) => invoke<void>("whisper_models_clear", { id }),
			transcribe: (audioPath) =>
				invoke<{ text: string }>("whisper_transcribe", { audioPath }).then((result) => result.text),
			transcribeAudio: (base64, mimeType) =>
				invoke<{ text: string }>("whisper_transcribe_base64", { audioBase64: base64, mimeType }).then(
					(result) => result.text
				)
		}
		: {
			listModels: async () => [],
			downloadModel: async () => { throw new Error("Whisper model downloads require Synth Desktop"); },
			setSelected: async () => { throw new Error("Whisper model selection requires Synth Desktop"); },
			clearModel: async () => undefined,
			transcribe: async () => { throw new Error("Transcription requires Synth Desktop"); },
			transcribeAudio: async () => { throw new Error("Transcription requires Synth Desktop"); }
		};
	window.synthCore ??= isTauri
		? {
			diagnostics: () => invoke<CoreDiagnostics>("core_diagnostics"),
			eventsAfter: (afterSequence = 0, limit) =>
				invoke<AppEvent[]>("core_events_after", { afterSequence, limit }),
			sessionEventsAfter: (sessionId, afterSequence = 0, limit) =>
				invoke<AppEvent[]>("core_session_events_after", { sessionId, afterSequence, limit }),
			onEvent(listener) {
				let disposed = false;
				let unlisten: (() => void) | undefined;
				void listen<AppEvent>("runtime:event", ({ payload }) => listener(payload)).then((next) => {
					if (disposed) next();
					else unlisten = next;
				});
				return () => { disposed = true; unlisten?.(); };
			}
		}
		: browserCoreBridge();
	window.synthIntern ??= isTauri
		? {
			listSessions: () => invoke<Session[]>("intern_sessions_list"),
			createSession: (request) => invoke<Session>("intern_session_create", { request }),
			send: (request) => invoke<{ runId: string }>("intern_session_send", { request }),
			control: (request) => invoke<{ accepted: boolean; receipt?: unknown }>("intern_session_control", { request }),
			eventsAfter: (sessionId, afterSequence = 0, limit) =>
				invoke<AppEvent[]>("intern_session_events_after", { sessionId, afterSequence, limit }),
			onEvent(listener) {
				let disposed = false;
				let unlisten: (() => void) | undefined;
				void listen<AppEvent>("runtime:event", ({ payload }) => {
					if (payload.source === "intern") listener(payload);
				}).then((next) => disposed ? next() : (unlisten = next));
				return () => { disposed = true; unlisten?.(); };
			}
		}
		: browserInternBridge();
	window.synthAccount ??= isTauri
		? {
			beginSignIn: () => invoke<SynthSignInBegin>("account_begin_sign_in"),
			pollSignIn: () => invoke<SynthSignInPoll>("account_poll_sign_in"),
			cancelSignIn: () => invoke<void>("account_cancel_sign_in"),
			signOut: () => invoke<SynthBackendSettings>("account_sign_out")
		}
		: {
			beginSignIn: async () => { throw new Error("Browser sign-in requires Synth Desktop"); },
			pollSignIn: async () => ({ status: "expired", reason: "Browser sign-in requires Synth Desktop" }),
			cancelSignIn: async () => undefined,
			signOut: async () => { throw new Error("Sign out requires Synth Desktop"); }
		};
window.synthConfig ??= isTauri
		? {
			get: () => invoke<SynthBackendSettings>("synth_config_get"),
			update: (request) => invoke<SynthBackendSettings>("synth_config_update", { request }),
			listModelMultiAgent: () => invoke<ModelMultiAgentSetting[]>("model_multi_agent_list"),
			updateModelMultiAgent: (request) => invoke<ModelMultiAgentSetting[]>("model_multi_agent_update", { request }),
			getWorkspaceAccess: () => invoke<WorkspaceAccessSettings>("workspace_access_get"),
			updateWorkspaceAccess: (request) => invoke<WorkspaceAccessSettings>("workspace_access_update", { request })
		}
		: {
			get: async () => ({
				configPath: "~/.synth-desktop/config.toml",
				envFile: "~/.synth-desktop/.env",
				profile: "prod",
				backendUrl: "https://api.usesynth.ai",
				apiKeyEnv: "SYNTH_API_KEY",
				apiKeyConfigured: false,
				workerKeyConfigured: false,
				openrouterApiKeyConfigured: false
			}),
			update: async () => { throw new Error("Backend settings require Synth Desktop"); },
			listModelMultiAgent: async () => [
				{ modelId: "gpt-5.6-sol", displayName: "GPT-5.6 Sol", preset: "v2", effective: "v2", overridden: false },
				{ modelId: "gpt-5.6-terra", displayName: "GPT-5.6 Terra", preset: "v2", effective: "v2", overridden: false },
				{ modelId: "gpt-5.6-luna", displayName: "GPT 5.6 Luna", preset: "v1", effective: "v1", overridden: false },
				{ modelId: "laguna-xs-2.1", displayName: "Laguna XS 2.1", preset: "none", effective: "none", overridden: false },
				{ modelId: "laguna-s-2.1", displayName: "Laguna S 2.1", preset: "none", effective: "none", overridden: false }
			],
			updateModelMultiAgent: async () => { throw new Error("Model settings require Synth Desktop"); },
			getWorkspaceAccess: async () => ({ allowedRoots: [] }),
			updateWorkspaceAccess: async () => { throw new Error("Workspace access settings require Synth Desktop"); }
		};
window.synthWorkspaceScope ??= isTauri
	? {
		get: (sessionId) => invoke("workspace_scope_get", { sessionId }),
		chooseAndAttach: (sessionId, proposedAccess) => invoke("workspace_scope_choose_and_attach", { sessionId, proposedAccess }),
		listRecentFolders: () => invoke("workspace_scope_recent_folders"),
		attachRecent: (sessionId, path) => invoke("workspace_scope_attach_recent", { sessionId, path }),
		removeAttachment: (sessionId, path) => invoke("workspace_scope_remove_attachment", { sessionId, path }),
		listGrants: (sessionId) => invoke("workspace_scope_grants_list", { sessionId }),
		approveRequest: (requestId) => invoke("workspace_scope_approve_request", { requestId }),
		denyRequest: (requestId) => invoke("workspace_scope_deny_request", { requestId })
	}
	: {
		get: async () => null,
		chooseAndAttach: async () => { throw new Error("Folder attachment requires Synth Desktop"); },
		listRecentFolders: async () => [],
		attachRecent: async () => { throw new Error("Recent folder attachment requires Synth Desktop"); },
		removeAttachment: async () => { throw new Error("Folder attachment requires Synth Desktop"); },
		listGrants: async () => [],
		approveRequest: async () => { throw new Error("Folder approval requires Synth Desktop"); },
		denyRequest: async () => { throw new Error("Folder approval requires Synth Desktop"); }
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
	window.synthInventory ??= isTauri
		? {
			listContainers: () => invoke<ContainerDeployment[]>("inventory_containers_list"),
			getContainer: (containerId) => invoke<ContainerDeployment>("inventory_containers_get", { containerId }),
			registerContainer: (request) => invoke<ContainerDeployment>("inventory_containers_register", { request }),
			probeContainer: (containerId) => invoke<ContainerDeployment>("inventory_containers_probe", { containerId }),
			listTraces: () => invoke<TraceV5Record[]>("inventory_traces_list"),
			getTrace: (traceId) => invoke<TraceV5Record>("inventory_traces_get", { traceId }),
			chooseTraceInput: async () => {
				const selection = await open({
					directory: false,
					multiple: false,
					title: "Import Trace V5 bundle",
					filters: [{ name: "Trace bundles", extensions: ["zip", "json"] }]
				});
				return typeof selection === "string" ? selection : null;
			},
			ingestTraceBundle: (request) => invoke<TraceBundleIngestResult>("inventory_traces_ingest", { request }),
			resolveTraceProjection: (traceDigest, projectionKind = "rollout-inspector") =>
				invoke<ResolvedTraceProjection>("inventory_trace_projection_resolve", { traceDigest, projectionKind }),
			listUsage: (limit = 100) => invoke<UsageLedgerEntry[]>("inventory_usage_list", { limit }),
			counts: () => invoke<InventoryCounts>("inventory_counts")
		}
		: {
			async listContainers() {
				return (await window.synthRuntime!.request<{ containers: ContainerDeployment[] }>("/v1/containers")).containers;
			},
			getContainer: (containerId) => window.synthRuntime!.request(`/v1/containers/${encodeURIComponent(containerId)}`),
			registerContainer: (request) => window.synthRuntime!.request("/v1/containers", { method: "POST", body: request }),
			probeContainer: (containerId) => window.synthRuntime!.request(`/v1/containers/${encodeURIComponent(containerId)}/probe`, { method: "POST" }),
			async listTraces() {
				return (await window.synthRuntime!.request<{ traces: TraceV5Record[] }>("/v1/traces")).traces;
			},
			getTrace: (traceId) => window.synthRuntime!.request(`/v1/traces/${encodeURIComponent(traceId)}`),
			chooseTraceInput: async () => null,
			ingestTraceBundle: async () => { throw new Error("Trace bundle import requires the desktop app"); },
			resolveTraceProjection: async () => { throw new Error("Trace projection resolution requires the desktop app"); },
			async listUsage(limit = 100) {
				return (await window.synthRuntime!.request<{ entries: UsageLedgerEntry[] }>(`/v1/usage?limit=${limit}`)).entries;
			},
			async counts() {
				const [containers, traces, usage] = await Promise.all([this.listContainers(), this.listTraces(), this.listUsage(2000)]);
				return { containers: containers.length, traces: traces.length, usage: usage.length };
			}
		};
	window.synthSkills ??= isTauri
		? { list: () => invoke<SkillHit[]>("skills_list") }
		: {
			list: async () => [
				{ id: "use-synth-containers", name: "use-synth-containers", description: "Synth container discovery and Trace V5 evidence." },
				{ id: "use-synth-visuals", name: "use-synth-visuals", description: "Create and manage Synth visuals." },
				{ id: "use-synth-optimizers", name: "use-synth-optimizers", description: "Operate Synth optimizer runs and recipes." },
				{ id: "run-live-container-evals", name: "run-live-container-evals", description: "Run live container-backed eval rollouts." }
			]
		};
	if (isTauri) {
		window.synthCodex ??= {
			defaultWorkspace: () => invoke<string>("codex_default_workspace"),
			list: () => invoke<PersistedCodexSession[]>("codex_sessions_list"),
			start: (request) => invoke<CodexSessionInfo>("codex_session_start", { request }),
			startTurn: (sessionId, prompt, effort) =>
				invoke<CodexSessionInfo>("codex_turn_start", { request: { sessionId, prompt, effort } }),
			sendTurn: (start, prompt, effort) =>
				invoke<CodexSessionInfo>("codex_turn_send", { request: { start, prompt, effort } }),
			interrupt: (sessionId) => invoke<void>("codex_turn_interrupt", { request: { sessionId } }),
			compact: (sessionId) => invoke<void>("codex_thread_compact", { request: { sessionId } }),
			steerTurn: (sessionId, text) =>
				invoke<void>("codex_turn_steer", { request: { sessionId, text } }),
			resolveApproval: (sessionId, approvalId, decision) => invoke<void>("codex_approval_resolve", { request: { sessionId, approvalId, decision } }),
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
		window.synthVisuals ??= {
			listTemplates: (genre) => invoke<VisualTemplateMeta[]>("visuals_templates_list", { genre: genre ?? null }),
			getTemplate: (templateId) => invoke<VisualTemplateMeta>("visuals_templates_get", { templateId }),
			list: (query) => invoke<VisualRecord[]>("visuals_list", { query: query ?? null }),
			get: (visualId) => invoke<VisualRecord>("visuals_get", { visualId }),
			revisions: (visualId) => invoke<VisualRevision[]>("visuals_revisions", { visualId }),
			create: (request) => invoke<VisualRecord>("visuals_create", { request }),
			update: (visualId, request) => invoke<VisualRecord>("visuals_update", { visualId, request }),
			save: (visualId, tsx) => invoke<VisualRecord>("visuals_save", { visualId, tsx: tsx ?? null }),
			fork: (visualId, title, sessionId) =>
				invoke<VisualRecord>("visuals_fork", { visualId, title: title ?? null, sessionId: sessionId ?? null }),
			archive: (visualId) => invoke<VisualRecord>("visuals_archive", { visualId }),
			show: (visualId, sessionId) =>
				invoke<VisualRecord>("visuals_show", { visualId, sessionId: sessionId ?? null }),
			onEvent(listener) {
				let disposed = false;
				let unlisten: (() => void) | undefined;
				void listen<AppEvent>("runtime:event", ({ payload }) => {
					if (payload.kind.startsWith("visual.")) listener(payload);
				}).then((next) => {
					if (disposed) next();
					else unlisten = next;
				});
				return () => { disposed = true; unlisten?.(); };
			},
			onShow(listener) {
				let disposed = false;
				let unlisten: (() => void) | undefined;
				void listen<AppEvent>("visual:show", ({ payload }) => listener(payload)).then((next) => {
					if (disposed) next();
					else unlisten = next;
				});
				return () => { disposed = true; unlisten?.(); };
			}
		};
		window.synthOptimizers ??= {
			listAlgorithms: () => invoke("optimizers_algorithms_list"),
			listRecipes: () => invoke("optimizers_recipes_list"),
			startRecipe: (request) => invoke("optimizers_recipe_start", { request }),
			list: (query) => invoke("optimizers_list", { query: query ?? null }),
			get: (optimizerRunId) => invoke("optimizers_get", { optimizerRunId }),
			create: (request) => invoke("optimizers_create", { request }),
			refresh: (optimizerRunId) => invoke("optimizers_refresh", { optimizerRunId }),
			eventsAfter: (optimizerRunId, afterSeq = 0, limit) =>
				invoke("optimizers_events_after", { optimizerRunId, afterSeq, limit: limit ?? null }),
			getState: (optimizerRunId, sliceId, atSeq) =>
				invoke("optimizers_get_state", { optimizerRunId, sliceId, atSeq: atSeq ?? null }),
			getStateBatch: (optimizerRunId, slices, atSeq) =>
				invoke("optimizers_get_state_batch", { optimizerRunId, slices: slices ?? null, atSeq: atSeq ?? null }),
			cancel: (optimizerRunId) => invoke("optimizers_cancel", { optimizerRunId }),
			pause: (optimizerRunId) => invoke("optimizers_pause", { optimizerRunId }),
			resume: (optimizerRunId) => invoke("optimizers_resume", { optimizerRunId }),
			openVisual: (optimizerRunId) => invoke("optimizers_open_visual", { optimizerRunId }),
			importLocal: (request) => invoke("optimizers_import_local", { request }),
			reconcileCloud: (request) => invoke("optimizers_reconcile_cloud", { request }),
			listCloud: (query) =>
				invoke("optimizers_list_cloud", {
					algorithm: query?.algorithm ?? null,
					status: query?.status ?? null,
					limit: query?.limit ?? null
				}),
			onEvent(listener) {
				let disposed = false;
				let unlisten: (() => void) | undefined;
				void listen<AppEvent>("runtime:event", ({ payload }) => {
					if (payload.kind.startsWith("optimizer.")) listener(payload);
				}).then((next) => {
					if (disposed) next();
					else unlisten = next;
				});
				return () => { disposed = true; unlisten?.(); };
			}
		};
	}
}
