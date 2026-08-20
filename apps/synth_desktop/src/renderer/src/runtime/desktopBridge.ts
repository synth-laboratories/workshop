import { listen } from "@tauri-apps/api/event";
import { COMMANDS, EVENT_CHANNELS, invokeCommand, type EventOrigin } from "../bridge";
import { commands as spectaCommands } from "../generated/protocol";
import { open } from "@tauri-apps/plugin-dialog";
import desktopPackage from "../../../../package.json";
import type { AppEvent, InternSessionControlRequest, InternSessionCreateRequest, InternSessionSendRequest, RuntimeEvent, Session } from "@synth/runtime-protocol";
import type { CodexEvent, CodexOauthBegin, CodexOauthStatus, CodexSessionInfo, ComposerImageAttachment, ContextSnapshot, DesktopInstanceDiagnostics, DesktopPermissionSettings, InventoryCounts, LagunaDownloadProgress, LagunaModelHit, LagunaStatus, ModelMultiAgentSetting, ModelPerformanceSummary, ModelPerformanceTurnSample, OptimizerInferDelta, PersistedCodexSession, ProductTelemetryPolicy, RequestOptions, RuntimeBridge, SecretAuditEvent, SecretCapabilitySummary, SecretImportPreview, SecretSummary, SecretsBridge, SecretsInbox, SkillHit, SynthAccountSummary, SynthBackendSettings, SynthSignInBegin, SynthSignInPoll, TariffCard, TerminalEvent, TerminalInfo, TrainingArtifact, TrainingModelDownloadProgress, TrainingModelHit, UpdateStatus, VisualAnnotation, VisualSeal, VisualSealBundle, VisualTemplateMeta, VisualUpload, WhisperDownloadProgress, WhisperModelHit, WhisperRuntimeStatus, WorkspaceAccessSettings } from "../bridge";
import type { CoreDiagnostics, VisualRecord, VisualRevision } from "@synth/runtime-protocol";
import type { ContainerDeployment, ResolvedTraceProjection, TraceBundleIngestResult, TraceV5Record, UsageLedgerEntry, UsageSummary, UsageWindow } from "@synth/runtime-protocol";
import { publicError } from "../runtime/publicError";

// The packaged WebKit view is always served from the `tauri:` protocol.  The
// injected internals global can appear too late for eager ES-module evaluation,
// so treating it as the only signal can accidentally install the browser/
// legacy-runtime bridge inside the desktop app.
const isTauri = window.location.protocol === "tauri:" || "__TAURI_INTERNALS__" in window;

/** Wire envelope for `runtime:event` after the dual-channel collapse. */
type OriginTaggedAppEvent = { origin: EventOrigin; payload: AppEvent };

function unwrapRuntimeEvent(payload: AppEvent | OriginTaggedAppEvent): AppEvent {
	if (
		payload &&
		typeof payload === "object" &&
		"origin" in payload &&
		"payload" in payload &&
		payload.payload &&
		typeof payload.payload === "object" &&
		"schemaVersion" in payload.payload
	) {
		return payload.payload;
	}
	return payload as AppEvent;
}

function appEventToCodexEvent(event: AppEvent): CodexEvent | null {
	// Native approval requests for plugin lifecycle and paid compute are
	// intentionally journaled as system events, but they still belong to the
	// active Codex session and must render as blocking approval cards.
	const isApprovalBoundary = event.kind.startsWith("approval.");
	if (!event.sessionId || (event.source !== "codex" && !isApprovalBoundary)) return null;
	const params =
		event.payload && typeof event.payload === "object" && !Array.isArray(event.payload)
			? (event.payload as Record<string, unknown>)
			: {};
	return { sessionId: event.sessionId, method: event.kind, params };
}

function listenRuntimeAppEvents(listener: (event: AppEvent) => void, onAttached?: () => void): () => void {
	let disposed = false;
	let unlisten: (() => void) | undefined;
	void listen<AppEvent | OriginTaggedAppEvent>(EVENT_CHANNELS.RUNTIME, ({ payload }) => {
		listener(unwrapRuntimeEvent(payload));
	}).then((next) => {
		if (disposed) next();
		else {
			unlisten = next;
			onAttached?.();
		}
	});
	return () => {
		disposed = true;
		unlisten?.();
	};
}

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
					onStatus?.({ state: "reconnecting", detail: publicError(reason) });
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
		async sessionEventsTail(): Promise<AppEvent[]> { return []; },
		async sessionEventsBefore(): Promise<AppEvent[]> { return []; },
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
	if (!isTauri && import.meta.env.DEV) window.synthRuntime ??= browserRuntimeBridge();
	window.synthDesktop ??= {
		platform: navigator.platform,
		chooseImageFiles: async () => {
			if (!isTauri) return [];
			const selection = await open({ multiple: true, filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }] });
			const paths = Array.isArray(selection) ? selection : selection ? [selection] : [];
			return Promise.all(paths.map(async (path): Promise<ComposerImageAttachment> => ({
				path,
				name: path.split("/").at(-1) ?? "Screenshot",
				previewUrl: await invokeCommand<string>(COMMANDS.DESKTOP_IMAGE_PREVIEW, { path })
			})));
		},
		getInstanceDiagnostics: () => isTauri
			? spectaCommands.desktopInstanceDiagnostics().then((value) => value as DesktopInstanceDiagnostics)
			: Promise.resolve({
				mode: "development", name: "browser", displayName: "Synth Desktop · browser",
				appVersion: desktopPackage.version, sourceRevision: "vite", buildRevision: "vite",
				buildTimestamp: "0", executableDigest: null, processId: 0, executable: "browser",
				dataRoot: "browser-memory://", viteUrl: window.location.origin, manifest: null
			}),
		chooseWorkspaceDirectory: async () => {
			if (!isTauri) return null;
			const selection = await invokeCommand<string | null>(COMMANDS.WORKSPACE_CHOOSE_DIRECTORY).catch(() =>
				open({ directory: true, multiple: false })
			);
			return typeof selection === "string" ? selection : null;
		}
	};
	window.synthLaguna ??= isTauri
		? {
			getStatus: () => invokeCommand<LagunaStatus>(COMMANDS.LAGUNA_GET_STATUS),
			reload: () => invokeCommand<LagunaStatus>(COMMANDS.LAGUNA_RELOAD),
			freeMemory: () => invokeCommand<{ released: boolean; conflict: boolean; detail: string | null }>(COMMANDS.LAGUNA_MODEL_UNLOAD),
			listModels: () => invokeCommand<LagunaModelHit[]>(COMMANDS.LAGUNA_MODELS_LIST),
			chooseModelDirectory: async () => {
				const selection = await open({ directory: true, multiple: false, title: "Choose a Laguna model folder" });
				return typeof selection === "string" ? selection : null;
			},
			setModelDirectory: (path) => invokeCommand<LagunaModelHit>(COMMANDS.LAGUNA_MODELS_SET_DIRECTORY, { path }),
			clearModelDirectory: () => invokeCommand<void>(COMMANDS.LAGUNA_MODELS_CLEAR_DIRECTORY),
			setAdapter: (checkpointId) => invokeCommand<LagunaStatus>(COMMANDS.LAGUNA_SET_ADAPTER, { checkpointId }),
			downloadModel: (modelId) => invokeCommand<LagunaModelHit>(COMMANDS.LAGUNA_MODEL_DOWNLOAD, { modelId }),
			deleteModel: (modelId) => invokeCommand<void>(COMMANDS.LAGUNA_MODEL_DELETE, { modelId }),
			onDownloadProgress(listener) {
				let disposed = false;
				let unlisten: (() => void) | undefined;
				void listen<LagunaDownloadProgress>(EVENT_CHANNELS.LAGUNA_DOWNLOAD, ({ payload }) => listener(payload)).then((next) => {
					if (disposed) next();
					else unlisten = next;
				});
				return () => { disposed = true; unlisten?.(); };
			},
			onStatus(listener) {
				let disposed = false;
				let unlisten: (() => void) | undefined;
				const refresh = () => {
					void invokeCommand<LagunaStatus>(COMMANDS.LAGUNA_GET_STATUS).then((status) => {
						if (!disposed) listener(status);
					}).catch(() => undefined);
				};
				const poll = window.setInterval(refresh, 5_000);
				void listen<LagunaStatus>(EVENT_CHANNELS.LAGUNA_STATUS, ({ payload }) => listener(payload)).then((next) => {
					if (disposed) next();
					else unlisten = next;
				});
				return () => { disposed = true; window.clearInterval(poll); unlisten?.(); };
			}
		}
		: {
			getStatus: async () => unavailableLaguna,
			reload: async () => unavailableLaguna,
			freeMemory: async () => ({ released: false, conflict: false, detail: "Local model controls require Synth Desktop" }),
			listModels: async () => [],
			downloadModel: async () => { throw new Error("Model downloads require Synth Desktop"); },
			deleteModel: async () => { throw new Error("Model deletion requires Synth Desktop"); },
			chooseModelDirectory: async () => null,
			setModelDirectory: async () => { throw new Error("Model folders require the desktop app"); },
			clearModelDirectory: async () => undefined,
			setAdapter: async () => unavailableLaguna,
			onStatus: () => () => undefined
		};
	window.synthTrainingModels ??= isTauri
		? {
			listModels: () => invokeCommand<TrainingModelHit[]>(COMMANDS.TRAINING_MODELS_LIST),
			downloadModel: (modelId) =>
				invokeCommand<TrainingModelHit>(COMMANDS.TRAINING_MODELS_DOWNLOAD, { modelId }),
			deleteModel: (modelId) =>
				invokeCommand<void>(COMMANDS.TRAINING_MODELS_DELETE, { modelId }),
			onDownloadProgress(listener) {
				let disposed = false;
				let unlisten: (() => void) | undefined;
				void listen<TrainingModelDownloadProgress>(
					EVENT_CHANNELS.TRAINING_MODELS_DOWNLOAD,
					({ payload }) => listener(payload)
				).then((next) => {
					if (disposed) next();
					else unlisten = next;
				});
				return () => { disposed = true; unlisten?.(); };
			}
		}
		: {
			listModels: async () => [],
			downloadModel: async () => { throw new Error("Training model downloads require Synth Desktop"); },
			deleteModel: async () => { throw new Error("Training model deletion requires Synth Desktop"); },
			onDownloadProgress: () => () => undefined
		};
	window.synthTrainingArtifacts ??= isTauri
		? {
			list: () => invokeCommand<TrainingArtifact[]>(COMMANDS.TRAINING_ARTIFACTS_LIST),
			get: (id) => invokeCommand<TrainingArtifact>(COMMANDS.TRAINING_ARTIFACTS_GET, { id }),
			launchInference: (request) => invokeCommand(COMMANDS.TRAINING_ARTIFACTS_LAUNCH_INFERENCE, request),
			export: (request) => invokeCommand(COMMANDS.TRAINING_ARTIFACTS_EXPORT, request),
			delete: (request) => invokeCommand(COMMANDS.TRAINING_ARTIFACTS_DELETE, request)
		}
		: {
			list: async () => [],
			get: async () => { throw new Error("Training artifacts require Synth Desktop"); },
			launchInference: async () => { throw new Error("Training artifact inference requires Synth Desktop"); },
			export: async () => { throw new Error("Training artifact export requires Synth Desktop"); },
			delete: async () => { throw new Error("Training artifact deletion requires Synth Desktop"); }
		};
	window.synthWhisper ??= isTauri
		? {
			getRuntimeStatus: () => invokeCommand<WhisperRuntimeStatus>(COMMANDS.WHISPER_RUNTIME_STATUS),
			warmSelected: () => invokeCommand<WhisperRuntimeStatus>(COMMANDS.WHISPER_RUNTIME_WARM),
			onRuntimeStatus: (listener) => {
				let unlisten: (() => void) | undefined;
				void listen<WhisperRuntimeStatus>(EVENT_CHANNELS.WHISPER_RUNTIME, (event) => listener(event.payload)).then((dispose) => { unlisten = dispose; });
				return () => unlisten?.();
			},
			listModels: () => invokeCommand<WhisperModelHit[]>(COMMANDS.WHISPER_MODELS_LIST),
			downloadModel: (id) => invokeCommand<WhisperModelHit>(COMMANDS.WHISPER_MODEL_DOWNLOAD, { id }),
			onDownloadProgress(listener) {
				let disposed = false;
				let unlisten: (() => void) | undefined;
				void listen<WhisperDownloadProgress>(EVENT_CHANNELS.WHISPER_DOWNLOAD, ({ payload }) => listener(payload)).then((next) => {
					if (disposed) next();
					else unlisten = next;
				});
				return () => { disposed = true; unlisten?.(); };
			},
			setSelected: (id) => invokeCommand<void>(COMMANDS.WHISPER_MODELS_SET_SELECTED, { id }),
			clearModel: (id) => invokeCommand<void>(COMMANDS.WHISPER_MODELS_CLEAR, { id }),
			transcribe: (audioPath) =>
				invokeCommand<{ text: string }>(COMMANDS.WHISPER_TRANSCRIBE, { audioPath }).then((result) => result.text),
			transcribeAudio: (base64, mimeType) =>
				invokeCommand<{ text: string }>(COMMANDS.WHISPER_TRANSCRIBE_BASE64, { audioBase64: base64, mimeType }).then(
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
			diagnostics: () => invokeCommand<CoreDiagnostics>(COMMANDS.CORE_DIAGNOSTICS),
			eventsAfter: (afterSequence = 0, limit) =>
				invokeCommand<AppEvent[]>(COMMANDS.CORE_EVENTS_AFTER, { afterSequence, limit }),
			sessionEventsAfter: (sessionId, afterSequence = 0, limit) =>
				invokeCommand<AppEvent[]>(COMMANDS.CORE_SESSION_EVENTS_AFTER, { sessionId, afterSequence, limit }),
			sessionEventsTail: (sessionId, limit) =>
				invokeCommand<AppEvent[]>(COMMANDS.CORE_SESSION_EVENTS_TAIL, { sessionId, limit }),
			sessionEventsBefore: (sessionId, beforeSequence, limit) =>
				invokeCommand<AppEvent[]>(COMMANDS.CORE_SESSION_EVENTS_BEFORE, { sessionId, beforeSequence, limit }),
			onEvent(listener) {
				return listenRuntimeAppEvents(listener);
			}
		}
		: browserCoreBridge();
	window.synthIntern ??= isTauri
		? {
			listSessions: () => invokeCommand<Session[]>(COMMANDS.INTERN_SESSIONS_LIST),
			createSession: (request) => invokeCommand<Session>(COMMANDS.INTERN_SESSION_CREATE, { request }),
			send: (request) => invokeCommand<{ runId: string }>(COMMANDS.INTERN_SESSION_SEND, { request }),
			control: (request) => invokeCommand<{ accepted: boolean; receipt?: unknown }>(COMMANDS.INTERN_SESSION_CONTROL, { request }),
			eventsAfter: (sessionId, afterSequence = 0, limit) =>
				invokeCommand<AppEvent[]>(COMMANDS.INTERN_SESSION_EVENTS_AFTER, { sessionId, afterSequence, limit }),
			onEvent(listener) {
				return listenRuntimeAppEvents((payload) => {
					if (payload.source === "intern") listener(payload);
				});
			}
		}
		: browserInternBridge();
window.synthAccount ??= isTauri
		? {
			beginSignIn: () => invokeCommand<SynthSignInBegin>(COMMANDS.ACCOUNT_BEGIN_SIGN_IN),
			pollSignIn: () => invokeCommand<SynthSignInPoll>(COMMANDS.ACCOUNT_POLL_SIGN_IN),
			cancelSignIn: () => invokeCommand<void>(COMMANDS.ACCOUNT_CANCEL_SIGN_IN),
			signOut: () => invokeCommand<SynthBackendSettings>(COMMANDS.ACCOUNT_SIGN_OUT),
			getSummary: () => invokeCommand<SynthAccountSummary>(COMMANDS.ACCOUNT_GET_SUMMARY),
			refresh: () => invokeCommand<SynthAccountSummary>(COMMANDS.ACCOUNT_REFRESH),
			openBilling: (action, tier) => invokeCommand<string>(COMMANDS.ACCOUNT_OPEN_BILLING, { action, tier })
		}
		: {
			beginSignIn: async () => { throw new Error("Browser sign-in requires Synth Desktop"); },
			pollSignIn: async () => ({ status: "expired", reason: "Browser sign-in requires Synth Desktop" }),
			cancelSignIn: async () => undefined,
			signOut: async () => { throw new Error("Sign out requires Synth Desktop"); },
			getSummary: async () => ({ signedIn: false, state: "local_only", environment: "local", source: "none" }),
			refresh: async () => ({ signedIn: false, state: "local_only", environment: "local", source: "none" }),
			openBilling: async () => { throw new Error("Billing requires Synth Desktop"); }
		};
window.synthTelemetry ??= isTauri
	? {
		getPolicy: () => invokeCommand<ProductTelemetryPolicy>(COMMANDS.PRODUCT_TELEMETRY_GET_POLICY),
		setOptOut: (optOut) => invokeCommand<ProductTelemetryPolicy>(COMMANDS.PRODUCT_TELEMETRY_SET_OPT_OUT, { optOut })
	}
	: (() => {
		let optionalEnabled = true;
		return {
			getPolicy: async () => ({
				dictionaryVersion: "workshop.product-telemetry.v1",
				collectionPolicyVersion: "workshop.product-telemetry.policy.v1",
				optionalEnabled,
				consentVersion: "workshop.product-telemetry.policy.v1"
			}),
			setOptOut: async (optOut: boolean) => {
				optionalEnabled = !optOut;
				return {
					dictionaryVersion: "workshop.product-telemetry.v1",
					collectionPolicyVersion: "workshop.product-telemetry.policy.v1",
					optionalEnabled,
					consentVersion: "workshop.product-telemetry.policy.v1"
				};
			}
		};
	})();
window.synthCodexOauth ??= isTauri
	? {
		begin: () => invokeCommand<CodexOauthBegin>(COMMANDS.CODEX_OAUTH_BEGIN),
		completeManual: (redirectUrl) => invokeCommand<CodexOauthStatus>(COMMANDS.CODEX_OAUTH_COMPLETE_MANUAL, { redirectUrl }),
		status: () => invokeCommand<CodexOauthStatus>(COMMANDS.CODEX_OAUTH_STATUS),
		ensureReady: () => invokeCommand<CodexOauthStatus>(COMMANDS.CODEX_OAUTH_ENSURE_READY),
		disconnect: () => invokeCommand<CodexOauthStatus>(COMMANDS.CODEX_OAUTH_DISCONNECT),
		cancel: () => invokeCommand<void>(COMMANDS.CODEX_OAUTH_CANCEL)
	}
	: {
		begin: async () => { throw new Error("ChatGPT subscription sign-in requires Synth Desktop"); },
		completeManual: async () => { throw new Error("ChatGPT subscription sign-in requires Synth Desktop"); },
		status: async () => ({ state: "disconnected", action: "connect", canUseModels: false, guidance: "ChatGPT sign-in requires Synth Desktop.", configured: false }),
		ensureReady: async () => ({ state: "disconnected", action: "connect", canUseModels: false, guidance: "ChatGPT sign-in requires Synth Desktop.", configured: false }),
		disconnect: async () => ({ state: "disconnected", action: "connect", canUseModels: false, guidance: "ChatGPT sign-in requires Synth Desktop.", configured: false }),
		cancel: async () => undefined
	};
window.synthSecrets ??= isTauri
	? {
		list: (provider, scope) => invokeCommand<SecretSummary[]>(COMMANDS.SECRETS_LIST, { provider, scope }),
		create: (request) => invokeCommand<SecretSummary>(COMMANDS.SECRETS_CREATE, { request }),
		replace: (secretId, value) => invokeCommand<SecretSummary>(COMMANDS.SECRETS_REPLACE, { secretId, value }),
		delete: (secretId) => invokeCommand<void>(COMMANDS.SECRETS_DELETE, { secretId }),
		test: (secretId) => invokeCommand<SecretSummary>(COMMANDS.SECRETS_TEST, { secretId }),
		requestEnvImport: (sourcePath, variableNames) => invokeCommand<SecretImportPreview>(COMMANDS.SECRETS_REQUEST_ENV_IMPORT, { request: { sourcePath, variableNames } }),
		commitEnvImport: (requestId, selected, after, confirm) => invokeCommand<SecretSummary[]>(COMMANDS.SECRETS_COMMIT_ENV_IMPORT, { requestId, selected, after, confirm: confirm ?? false }),
		denyEnvImport: (requestId) => invokeCommand<void>(COMMANDS.SECRETS_DENY_ENV_IMPORT, { requestId }),
		pending: () => invokeCommand<SecretsInbox>(COMMANDS.SECRETS_PENDING),
		capabilities: () => invokeCommand<SecretCapabilitySummary[]>(COMMANDS.SECRETS_CAPABILITIES_LIST),
		revokeCapability: (capabilityId) => invokeCommand<void>(COMMANDS.SECRETS_REVOKE_CAPABILITY, { capabilityId }),
		audit: (limit) => invokeCommand<SecretAuditEvent[]>(COMMANDS.SECRETS_AUDIT_LIST, { limit }),
		grantUse: (secretId, runId, recipeId, rememberRecipe, requestId) => invokeCommand(COMMANDS.SECRETS_GRANT_USE, { secretId, runId, recipeId, rememberRecipe, requestId: requestId ?? null }),
		denyUse: (secretId) => invokeCommand(COMMANDS.SECRETS_DENY_USE, { secretId })
	} satisfies SecretsBridge
	: {
		list: async () => [],
		create: async () => { throw new Error("Secrets require Synth Desktop"); },
		replace: async () => { throw new Error("Secrets require Synth Desktop"); },
		delete: async () => undefined,
		test: async () => { throw new Error("Secrets require Synth Desktop"); },
		requestEnvImport: async () => { throw new Error("Secrets require Synth Desktop"); },
		commitEnvImport: async () => [],
		denyEnvImport: async () => undefined,
		pending: async () => ({ imports: [], grants: [], proxy: { running: false } }),
		capabilities: async () => [],
		revokeCapability: async () => undefined,
		audit: async () => [],
		grantUse: async () => ({ status: "denied" }),
		denyUse: async () => ({ status: "denied" })
	};
window.synthConfig ??= isTauri
		? {
			get: () => invokeCommand<SynthBackendSettings>(COMMANDS.SYNTH_CONFIG_GET),
			update: (request) => invokeCommand<SynthBackendSettings>(COMMANDS.SYNTH_CONFIG_UPDATE, { request }),
			listModelMultiAgent: () => invokeCommand<ModelMultiAgentSetting[]>(COMMANDS.MODEL_MULTI_AGENT_LIST),
			updateModelMultiAgent: (request) => invokeCommand<ModelMultiAgentSetting[]>(COMMANDS.MODEL_MULTI_AGENT_UPDATE, { request }),
			getWorkspaceAccess: () => invokeCommand<WorkspaceAccessSettings>(COMMANDS.WORKSPACE_ACCESS_GET),
			updateWorkspaceAccess: (request) => invokeCommand<WorkspaceAccessSettings>(COMMANDS.WORKSPACE_ACCESS_UPDATE, { request }),
			getDesktopPermissions: () => invokeCommand<DesktopPermissionSettings>(COMMANDS.DESKTOP_PERMISSIONS_GET),
			updateDesktopPermissions: (request) => invokeCommand<DesktopPermissionSettings>(COMMANDS.DESKTOP_PERMISSIONS_UPDATE, { request })
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
				{ modelId: "laguna-s-2.1", displayName: "Laguna S 2.1", preset: "none", effective: "none", overridden: false },
				{ modelId: "muse-spark-1.2", displayName: "Muse Spark 1.2", preset: "none", effective: "none", overridden: false }
			],
			updateModelMultiAgent: async () => { throw new Error("Model settings require Synth Desktop"); },
			getWorkspaceAccess: async () => ({ allowedRoots: [] }),
			updateWorkspaceAccess: async () => { throw new Error("Workspace access settings require Synth Desktop"); },
			getDesktopPermissions: async () => ({ configPath: "~/.synth-desktop/config.toml", approvalPolicy: "untrusted", sandboxMode: "workspace-write" }),
			updateDesktopPermissions: async () => { throw new Error("Desktop permission settings require Synth Desktop"); }
		};
window.synthWorkspaceScope ??= isTauri
	? {
		get: (sessionId) => invokeCommand(COMMANDS.WORKSPACE_SCOPE_GET, { sessionId }),
		chooseAndAttach: (sessionId, proposedAccess) => invokeCommand(COMMANDS.WORKSPACE_SCOPE_CHOOSE_AND_ATTACH, { sessionId, proposedAccess }),
		listRecentFolders: () => invokeCommand(COMMANDS.WORKSPACE_SCOPE_RECENT_FOLDERS),
		attachRecent: (sessionId, path) => invokeCommand(COMMANDS.WORKSPACE_SCOPE_ATTACH_RECENT, { sessionId, path }),
		removeAttachment: (sessionId, path) => invokeCommand(COMMANDS.WORKSPACE_SCOPE_REMOVE_ATTACHMENT, { sessionId, path }),
		listGrants: (sessionId) => invokeCommand(COMMANDS.WORKSPACE_SCOPE_GRANTS_LIST, { sessionId }),
		approveRequest: (requestId) => invokeCommand(COMMANDS.WORKSPACE_SCOPE_APPROVE_REQUEST, { requestId }),
		denyRequest: (requestId) => invokeCommand(COMMANDS.WORKSPACE_SCOPE_DENY_REQUEST, { requestId })
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
			create: (request) => invokeCommand<TerminalInfo>(COMMANDS.TERMINAL_CREATE, { request }),
			list: (workspaceId) => invokeCommand<TerminalInfo[]>(COMMANDS.TERMINAL_LIST, { workspaceId }),
			snapshot: (terminalId, afterSequence = 0) => invokeCommand<TerminalEvent[]>(COMMANDS.TERMINAL_SNAPSHOT, { terminalId, afterSequence }),
			write: (terminalId, data) => invokeCommand<void>(COMMANDS.TERMINAL_WRITE, { terminalId, data }),
			resize: (terminalId, cols, rows) => invokeCommand<void>(COMMANDS.TERMINAL_RESIZE, { terminalId, cols, rows }),
			close: (terminalId) => invokeCommand<void>(COMMANDS.TERMINAL_CLOSE, { terminalId }),
			onEvent(listener) {
				let unlisten: (() => void) | undefined;
				let disposed = false;
				void listen<TerminalEvent>(EVENT_CHANNELS.TERMINAL, ({ payload }) => listener(payload)).then((next) => disposed ? next() : (unlisten = next));
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
			listContainers: () => invokeCommand<ContainerDeployment[]>(COMMANDS.DATA_CONTAINERS_LIST),
			getContainer: (containerId) => invokeCommand<ContainerDeployment>(COMMANDS.DATA_CONTAINERS_GET, { containerId }),
			registerContainer: (request) => invokeCommand<ContainerDeployment>(COMMANDS.DATA_CONTAINERS_REGISTER, { request }),
			probeContainer: (containerId) => invokeCommand<ContainerDeployment>(COMMANDS.DATA_CONTAINERS_PROBE, { containerId }),
			listTraces: () => invokeCommand<TraceV5Record[]>(COMMANDS.DATA_TRACES_LIST),
			getTrace: (traceId) => invokeCommand<TraceV5Record>(COMMANDS.DATA_TRACES_GET, { traceId }),
			chooseTraceInput: async () => {
				const selection = await open({
					directory: false,
					multiple: false,
					title: "Import Trace V5 bundle",
					filters: [{ name: "Trace bundles", extensions: ["zip", "json"] }]
				});
				return typeof selection === "string" ? selection : null;
			},
			ingestTraceBundle: (request) => invokeCommand<TraceBundleIngestResult>(COMMANDS.DATA_TRACES_INGEST, { request }),
			resolveTraceProjection: (traceDigest, projectionKind = "rollout-inspector") =>
				invokeCommand<ResolvedTraceProjection>(COMMANDS.DATA_TRACE_PROJECTION_RESOLVE, { traceDigest, projectionKind }),
			listUsage: (limit = 100) => invokeCommand<UsageLedgerEntry[]>(COMMANDS.DATA_USAGE_LIST, { limit }),
			counts: () => invokeCommand<InventoryCounts>(COMMANDS.DATA_COUNTS)
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
	window.synthModelPerformance ??= isTauri
		? {
			summaries: () => invokeCommand<ModelPerformanceSummary[]>(COMMANDS.MODEL_PERFORMANCE_SUMMARY),
			turnSamples: (sessionId) => invokeCommand<ModelPerformanceTurnSample[]>(COMMANDS.MODEL_PERFORMANCE_TURN_SAMPLES, { sessionId })
		}
		: { summaries: async () => [], turnSamples: async () => [] };
	window.synthUpdates ??= isTauri
		? {
			status: () => invokeCommand<UpdateStatus>(COMMANDS.UPDATE_STATUS),
			openDownload: () => invokeCommand<void>(COMMANDS.UPDATE_OPEN_DOWNLOAD)
		}
		: {
			status: async () => ({
				currentVersion: "0.4.0",
				channel: "stable",
				latestVersion: null,
				updateAvailable: false
			}),
			openDownload: async () => undefined
		};
	if (isTauri) {
		window.synthUsage ??= {
			summary: (window: UsageWindow) => invokeCommand<UsageSummary>(COMMANDS.USAGE_SUMMARY, { window })
		};
		window.synthTariffs ??= {
			catalog: () => invokeCommand<TariffCard[]>(COMMANDS.TARIFF_CATALOG)
		};
	}
	window.synthSkills ??= isTauri
		? { list: () => invokeCommand<SkillHit[]>(COMMANDS.SKILLS_LIST) }
		: {
			list: async () => [
				{ id: "use-synth-containers", name: "use-synth-containers", description: "Synth container discovery and Trace V5 evidence." },
				{ id: "use-synth-visuals", name: "use-synth-visuals", description: "Create and manage Synth visuals." },
				{ id: "use-synth-optimizers", name: "use-synth-optimizers", description: "Operate Synth optimizer runs and recipes." },
				{ id: "run-live-container-evals", name: "run-live-container-evals", description: "Run live container-backed eval rollouts." },
				{ id: "author-synth-diagrams", name: "author-synth-diagrams", description: "Author a Mermaid diagram into the right Visual pane." }
			]
		};
	window.synthContext ??= isTauri
		? {
			snapshot: (workspace) => invokeCommand<ContextSnapshot>(COMMANDS.CONTEXT_SNAPSHOT, { workspace }),
			updateWorkspaceAgents: (workspace, content) => invokeCommand<ContextSnapshot>(COMMANDS.CONTEXT_WORKSPACE_AGENTS_UPDATE, { workspace, content }),
			updateSkill: (workspace, skillId, enabled, content) => invokeCommand<ContextSnapshot>(COMMANDS.CONTEXT_SKILL_UPDATE, { workspace, skillId, enabled, content: content ?? null }),
			updateMcpGroup: (workspace, groupId, enabled) => invokeCommand<ContextSnapshot>(COMMANDS.CONTEXT_MCP_GROUP_UPDATE, { workspace, groupId, enabled }),
			installCookbooks: (workspace) => invokeCommand<ContextSnapshot>(COMMANDS.CONTEXT_COOKBOOKS_INSTALL, { workspace }),
			cancelCookbooks: (workspace) => invokeCommand<ContextSnapshot>(COMMANDS.CONTEXT_COOKBOOKS_CANCEL, { workspace }),
			setCookbooksEnabled: (workspace, enabled) => invokeCommand<ContextSnapshot>(COMMANDS.CONTEXT_COOKBOOKS_SET_ENABLED, { workspace, enabled }),
			uninstallCookbooks: (workspace) => invokeCommand<ContextSnapshot>(COMMANDS.CONTEXT_COOKBOOKS_UNINSTALL, { workspace })
		}
		: {
			snapshot: async (workspace) => ({ workshopAgents: { path: "bundled://WORKSHOP_AGENTS.md", content: "Workshop collaboration context", state: "bundled", editable: false, version: "dev" }, workspaceAgents: { path: `${workspace}/AGENTS.md`, content: "", state: "absent", editable: true }, cookbooks: { enabled: false, installed: false, phase: "off" }, skills: [], mcpGroups: [] }),
			updateWorkspaceAgents: async () => { throw new Error("Context editing requires Synth Desktop"); },
			updateSkill: async () => { throw new Error("Context editing requires Synth Desktop"); },
			updateMcpGroup: async () => { throw new Error("Context editing requires Synth Desktop"); },
			installCookbooks: async () => { throw new Error("Cookbook installation requires Synth Desktop"); },
			cancelCookbooks: async () => { throw new Error("Cookbook installation requires Synth Desktop"); },
			setCookbooksEnabled: async () => { throw new Error("Cookbook controls require Synth Desktop"); },
			uninstallCookbooks: async () => { throw new Error("Cookbook controls require Synth Desktop"); }
		};
	if (isTauri) {
		window.synthCodex ??= {
			defaultWorkspace: () => invokeCommand<string>(COMMANDS.CODEX_DEFAULT_WORKSPACE),
			list: () => invokeCommand<PersistedCodexSession[]>(COMMANDS.CODEX_SESSIONS_LIST),
			start: (request) => invokeCommand<CodexSessionInfo>(COMMANDS.CODEX_SESSION_START, { request }),
			startTurn: (sessionId, prompt, effort, options) =>
				invokeCommand<CodexSessionInfo>(COMMANDS.CODEX_TURN_START, {
					request: {
						sessionId,
						prompt,
						effort,
						clientMessageId: options?.clientMessageId
					}
				}),
			sendTurn: (start, prompt, effort, options) =>
				invokeCommand<CodexSessionInfo>(COMMANDS.CODEX_TURN_SEND, {
					request: {
						start,
						prompt,
						effort,
						compactBeforeModelSwitch: Boolean(options?.compactBeforeModelSwitch),
						clientMessageId: options?.clientMessageId
					}
				}),
			interrupt: (sessionId) => invokeCommand<void>(COMMANDS.CODEX_TURN_INTERRUPT, { request: { sessionId } }),
			compact: (request) => invokeCommand<void>(COMMANDS.CODEX_THREAD_COMPACT, { request }),
			readThread: (sessionId, threadId, includeTurns = true) =>
				invokeCommand<unknown>(COMMANDS.CODEX_THREAD_READ, {
					request: { sessionId, threadId, includeTurns }
				}),
			listThreadItems: (sessionId, threadId, cursor, limit) =>
				invokeCommand<unknown>(COMMANDS.CODEX_THREAD_ITEMS_LIST, {
					request: { sessionId, threadId, cursor, limit }
				}),
			steerTurn: (sessionId, text) =>
				invokeCommand<void>(COMMANDS.CODEX_TURN_STEER, { request: { sessionId, text } }),
			resolveApproval: (sessionId, approvalId, decision) => invokeCommand<void>(COMMANDS.CODEX_APPROVAL_RESOLVE, { request: { sessionId, approvalId, decision } }),
			close: (sessionId) => invokeCommand<void>(COMMANDS.CODEX_SESSION_CLOSE, { request: { sessionId } }),
			onEvent(listener) {
				let disposed = false;
				const unsubs: Array<() => void> = [];
				// Primary: single origin-tagged runtime:event stream.
				unsubs.push(
					listenRuntimeAppEvents((appEvent) => {
						const codex = appEventToCodexEvent(appEvent);
						if (codex) listener(codex);
					})
				);
				// Compat: legacy codex:event (flagged for removal — producers no longer emit).
				void listen<CodexEvent>(EVENT_CHANNELS.CODEX, ({ payload }) => listener(payload)).then((next) => {
					if (disposed) next();
					else unsubs.push(next);
				});
				return () => {
					disposed = true;
					for (const unsub of unsubs) unsub();
				};
			}
		};
		window.synthVisuals ??= {
			listTemplates: (genre) => invokeCommand<VisualTemplateMeta[]>(COMMANDS.VISUALS_TEMPLATES_LIST, { genre: genre ?? null }),
			getTemplate: (templateId) => invokeCommand<VisualTemplateMeta>(COMMANDS.VISUALS_TEMPLATES_GET, { templateId }),
			list: (query) => invokeCommand<VisualRecord[]>(COMMANDS.VISUALS_LIST, { query: query ?? null }),
			get: (visualId) => invokeCommand<VisualRecord>(COMMANDS.VISUALS_GET, { visualId }),
			reportObservation: (observation) => invokeCommand<void>(COMMANDS.VISUALS_OBSERVATION_REPORT, { observation }),
			revisions: (visualId) => invokeCommand<VisualRevision[]>(COMMANDS.VISUALS_REVISIONS, { visualId }),
			annotations: (visualId) => invokeCommand<VisualAnnotation[]>(COMMANDS.VISUALS_ANNOTATIONS_LIST, { visualId }),
			createAnnotation: (visualId, request) => invokeCommand<VisualAnnotation>(COMMANDS.VISUALS_ANNOTATION_CREATE, { visualId, request }),
			listSeals: (visualId) => invokeCommand<VisualSeal[]>(COMMANDS.VISUALS_SEALS_LIST, { visualId: visualId ?? null }),
			seal: (visualId, revision) => invokeCommand<VisualSeal>(COMMANDS.VISUALS_SEAL, { visualId, revision }),
			getSeal: (receiptDigest) => invokeCommand<VisualSealBundle>(COMMANDS.VISUALS_SEAL_GET, { receiptDigest }),
			uploadStatus: (receiptDigest) => invokeCommand<VisualUpload | null>(COMMANDS.VISUALS_UPLOAD_STATUS, { receiptDigest }),
			shareSeal: (receiptDigest) => invokeCommand<VisualUpload>(COMMANDS.VISUALS_SHARE_SEAL, { receiptDigest }),
			openShared: (committedUrl) => invokeCommand<VisualSealBundle>(COMMANDS.VISUALS_OPEN_SHARED, { committedUrl }),
			create: (request) => invokeCommand<VisualRecord>(COMMANDS.VISUALS_CREATE, { request }),
			update: (visualId, request) => invokeCommand<VisualRecord>(COMMANDS.VISUALS_UPDATE, { visualId, request }),
			save: (visualId, tsx) => invokeCommand<VisualRecord>(COMMANDS.VISUALS_SAVE, { visualId, tsx: tsx ?? null }),
			fork: (visualId, title, sessionId) =>
				invokeCommand<VisualRecord>(COMMANDS.VISUALS_FORK, { visualId, title: title ?? null, sessionId: sessionId ?? null }),
			archive: (visualId) => invokeCommand<VisualRecord>(COMMANDS.VISUALS_ARCHIVE, { visualId }),
			show: (visualId, sessionId) =>
				invokeCommand<VisualRecord>(COMMANDS.VISUALS_SHOW, { visualId, sessionId: sessionId ?? null }),
			content: (visualId) => invokeCommand(COMMANDS.VISUALS_CONTENT, { visualId }),
			renditions: (visualId) => invokeCommand(COMMANDS.VISUALS_RENDITIONS, { visualId }),
			rendition: (visualId, format, theme, sizeClass) =>
				invokeCommand(COMMANDS.VISUALS_RENDITION, {
					visualId,
					format: format ?? null,
					theme: theme ?? null,
					sizeClass: sizeClass ?? null
				}),
			render: (visualId) => invokeCommand<VisualRecord>(COMMANDS.VISUALS_RENDER, { visualId }),
			pollStream: (request) => invokeCommand(COMMANDS.VISUAL_STREAM_POLL, { request }),
			onEvent(listener, onAttached) {
				return listenRuntimeAppEvents((payload) => {
					if (payload.kind.startsWith("visual.")) listener(payload);
				}, onAttached);
			},
			onShow(listener) {
				let disposed = false;
				let unlisten: (() => void) | undefined;
				void listen<AppEvent>(EVENT_CHANNELS.VISUAL_SHOW, ({ payload }) => listener(payload)).then((next) => {
					if (disposed) next();
					else unlisten = next;
				});
				return () => { disposed = true; unlisten?.(); };
			}
		};
		window.synthPlugins ??= {
			status: (pluginId) => invokeCommand(COMMANDS.PLUGINS_STATUS, { pluginId: pluginId ?? null }),
			list: () => invokeCommand(COMMANDS.PLUGINS_LIST),
			setReleaseChannel: (pluginId, channel) =>
				invokeCommand(COMMANDS.PLUGINS_SET_RELEASE_CHANNEL, { pluginId, channel }),
			manage: (operation, pluginId, version) =>
				invokeCommand(COMMANDS.PLUGINS_MANAGE, {
					operation,
					pluginId,
					version: version ?? null,
					sessionId: null
				}),
			// `optimizer:status` has been emitted since the sidecar manager
			// landed and had no subscriber, which is why the Optimizers page
			// polled the registry every 750 ms — and every poll re-probed the
			// live sidecar.
			onStatusChanged(listener) {
				let disposed = false;
				let unlisten: (() => void) | undefined;
				void listen(EVENT_CHANNELS.OPTIMIZER_STATUS, () => listener()).then((next) => {
					if (disposed) next();
					else unlisten = next;
				});
				return () => { disposed = true; unlisten?.(); };
			}
		};
		window.synthComputerUse ??= {
			status: (sessionId) =>
				invokeCommand(COMMANDS.COMPUTER_USE_STATUS, { sessionId: sessionId ?? null }),
			install: () => invokeCommand(COMMANDS.COMPUTER_USE_INSTALL),
			remove: () => invokeCommand(COMMANDS.COMPUTER_USE_REMOVE),
			revokeApp: (bundleId) => invokeCommand(COMMANDS.COMPUTER_USE_REVOKE_APP, { bundleId }),
			openSettings: (permissionId) =>
				invokeCommand(COMMANDS.COMPUTER_USE_OPEN_SETTINGS, { permissionId })
		};
		window.synthBrowserAdmin ??= {
			status: () => invokeCommand(COMMANDS.BROWSER_RUNTIME_STATUS),
			allowOrigin: (origin) => invokeCommand(COMMANDS.BROWSER_POLICY_ALLOW_ORIGIN, { origin }),
			revokeOrigin: (origin) => invokeCommand(COMMANDS.BROWSER_POLICY_REVOKE_ORIGIN, { origin })
		};
		window.synthReports ??= {
			list: (query) => invokeCommand(COMMANDS.REPORTS_LIST, { query: query ?? null }),
			get: (reportId) => invokeCommand(COMMANDS.REPORTS_GET, { reportId }),
			getRevision: (reportId, revision) =>
				invokeCommand(COMMANDS.REPORTS_REVISION_GET, { reportId, revision: revision ?? null }),
			validate: (reportId, revision) =>
				invokeCommand(COMMANDS.REPORTS_VALIDATE, { reportId, revision: revision ?? null }),
			pinAll: (reportId) => invokeCommand(COMMANDS.REPORTS_PIN_ALL, { reportId }),
			create: (request) => invokeCommand(COMMANDS.REPORTS_CREATE, { request }),
			update: (reportId, request) => invokeCommand(COMMANDS.REPORTS_UPDATE, { reportId, request }),
			archive: (reportId) => invokeCommand(COMMANDS.REPORTS_ARCHIVE, { reportId }),
			restore: (reportId) => invokeCommand(COMMANDS.REPORTS_RESTORE, { reportId }),
			listVisibilityRequests: (reportId) =>
				invokeCommand(COMMANDS.REPORTS_VISIBILITY_REQUESTS, { reportId: reportId ?? null }),
			requestVisibility: (reportId, request) =>
				invokeCommand(COMMANDS.REPORTS_VISIBILITY_REQUEST, { reportId, request }),
			decideVisibility: (requestId, approved) =>
				invokeCommand(COMMANDS.REPORTS_VISIBILITY_DECIDE, { requestId, approved }),
			seal: (reportId, revision) => invokeCommand(COMMANDS.REPORTS_SEAL, { reportId, revision }),
			listSeals: (reportId) => invokeCommand(COMMANDS.REPORTS_SEALS_LIST, { reportId: reportId ?? null }),
			getSeal: (receiptDigest) => invokeCommand(COMMANDS.REPORTS_SEAL_GET, { receiptDigest }),
			compareSeals: (leftDigest, rightDigest) =>
				invokeCommand(COMMANDS.REPORTS_SEALS_COMPARE, { leftDigest, rightDigest }),
			uploadStatus: (receiptDigest) =>
				invokeCommand(COMMANDS.REPORTS_UPLOAD_STATUS, { receiptDigest }),
			shareSeal: (receiptDigest) => invokeCommand(COMMANDS.REPORTS_SHARE, { receiptDigest }),
			setAudience: (publicationId, request) =>
				invokeCommand(COMMANDS.REPORTS_AUDIENCE_SET, { publicationId, request }),
			revokeAudience: (publicationId, receiptDigest) =>
				invokeCommand(COMMANDS.REPORTS_AUDIENCE_REVOKE, { publicationId, receiptDigest }),
			promote: (publicationId, slug) => invokeCommand(COMMANDS.REPORTS_PROMOTE, { publicationId, slug }),
			openShared: (committedUrl) => invokeCommand(COMMANDS.REPORTS_OPEN_SHARED, { committedUrl }),
			listComments: (reportId, revision) =>
				invokeCommand(COMMANDS.REPORTS_COMMENTS_LIST, { reportId, revision: revision ?? null }),
			createComment: (reportId, revision, request) =>
				invokeCommand(COMMANDS.REPORTS_COMMENT_CREATE, { reportId, revision, request }),
			listExperiments: (reportId) => invokeCommand(COMMANDS.REPORTS_EXPERIMENTS_LIST, { reportId }),
			upsertExperiment: (reportId, request) =>
				invokeCommand(COMMANDS.REPORTS_EXPERIMENT_UPSERT, { reportId, request }),
			listLog: (reportId) => invokeCommand(COMMANDS.REPORTS_LOG_LIST, { reportId }),
			appendLog: (reportId, request) => invokeCommand(COMMANDS.REPORTS_LOG_APPEND, { reportId, request }),
			onEvent(listener) {
				return listenRuntimeAppEvents((payload) => {
					if (payload.kind.startsWith("report.")) listener(payload);
				});
			}
		};
		window.synthOptimizers ??= {
			listAlgorithms: () => invokeCommand(COMMANDS.OPTIMIZERS_ALGORITHMS_LIST),
			listRecipes: () => invokeCommand(COMMANDS.OPTIMIZERS_RECIPES_LIST),
			startRecipe: (request) => invokeCommand(COMMANDS.OPTIMIZERS_RECIPE_START, { request }),
			stageEvalCandidates: (request) =>
				invokeCommand(COMMANDS.OPTIMIZERS_STAGE_EVAL_CANDIDATES, { request }),
			list: (query) => invokeCommand(COMMANDS.OPTIMIZERS_LIST, { query: query ?? null }),
			get: (optimizerRunId) => invokeCommand(COMMANDS.OPTIMIZERS_GET, { optimizerRunId }),
			create: (request) => invokeCommand(COMMANDS.OPTIMIZERS_CREATE, { request }),
			refresh: (optimizerRunId) => invokeCommand(COMMANDS.OPTIMIZERS_REFRESH, { optimizerRunId }),
			eventsAfter: (optimizerRunId, afterSeq = 0, limit) =>
				invokeCommand(COMMANDS.OPTIMIZERS_EVENTS_AFTER, { optimizerRunId, afterSeq, limit: limit ?? null }),
			getState: (optimizerRunId, sliceId, atSeq) =>
				invokeCommand(COMMANDS.OPTIMIZERS_GET_STATE, { optimizerRunId, sliceId, atSeq: atSeq ?? null }),
			getStateBatch: (optimizerRunId, slices, atSeq) =>
				invokeCommand(COMMANDS.OPTIMIZERS_GET_STATE_BATCH, { optimizerRunId, slices: slices ?? null, atSeq: atSeq ?? null }),
			cancel: (optimizerRunId) => invokeCommand(COMMANDS.OPTIMIZERS_CANCEL, { optimizerRunId }),
			pause: (optimizerRunId) => invokeCommand(COMMANDS.OPTIMIZERS_PAUSE, { optimizerRunId }),
			resume: (optimizerRunId) => invokeCommand(COMMANDS.OPTIMIZERS_RESUME, { optimizerRunId }),
			openVisual: (optimizerRunId) => invokeCommand(COMMANDS.OPTIMIZERS_OPEN_VISUAL, { optimizerRunId }),
			importLocal: (request) => invokeCommand(COMMANDS.OPTIMIZERS_IMPORT_LOCAL, { request }),
			reconcileCloud: (request) => invokeCommand(COMMANDS.OPTIMIZERS_RECONCILE_CLOUD, { request }),
			listCloud: (query) =>
				invokeCommand(COMMANDS.OPTIMIZERS_LIST_CLOUD, {
					algorithm: query?.algorithm ?? null,
					status: query?.status ?? null,
					limit: query?.limit ?? null
				}),
			searchSavedLoras: (query) =>
				invokeCommand(COMMANDS.OPTIMIZERS_SAVED_LORAS_SEARCH, { query: query ?? null }),
			listRunCheckpoints: (optimizerRunId) =>
				invokeCommand(COMMANDS.OPTIMIZERS_RUN_CHECKPOINTS_LIST, { optimizerRunId }),
			runOutputs: (optimizerRunId) =>
				invokeCommand(COMMANDS.OPTIMIZERS_RUN_OUTPUTS, { optimizerRunId }),
			hostedTrainingModels: () => invokeCommand(COMMANDS.OPTIMIZERS_TRAINING_MODELS),
			archiveSavedLora: (checkpointId) =>
				invokeCommand(COMMANDS.OPTIMIZERS_SAVED_LORA_ARCHIVE, { checkpointId }),
			savedLoraDownload: (checkpointId) =>
				invokeCommand(COMMANDS.OPTIMIZERS_SAVED_LORA_DOWNLOAD, { checkpointId }),
			importSavedLora: (path) =>
				invokeCommand(COMMANDS.OPTIMIZERS_SAVED_LORA_IMPORT, { path }),
			patchSavedLora: (checkpointId, patch) =>
				invokeCommand(COMMANDS.OPTIMIZERS_SAVED_LORA_PATCH, { checkpointId, patch }),
			publishSavedLora: (checkpointId) =>
				invokeCommand(COMMANDS.OPTIMIZERS_SAVED_LORA_PUBLISH, { checkpointId }),
			inferCheckpoint: (request) =>
				invokeCommand(COMMANDS.OPTIMIZERS_CHECKPOINT_INFER, { request }),
			onInferDelta(listener) {
				let disposed = false;
				let unlisten: (() => void) | undefined;
				void listen<OptimizerInferDelta>(EVENT_CHANNELS.OPTIMIZER_INFER, ({ payload }) => listener(payload)).then((next) => {
					if (disposed) next();
					else unlisten = next;
				});
				return () => { disposed = true; unlisten?.(); };
			},
			reconcileTraining: (optimizerRunId) =>
				invokeCommand(COMMANDS.OPTIMIZERS_TRAINING_RECONCILE, { optimizerRunId }),
			recordVisualReady: (request) => invokeCommand(COMMANDS.VISUAL_SUBSCRIPTION_READY, { request }),
			onEvent(listener) {
				return listenRuntimeAppEvents((payload) => {
					if (payload.kind.startsWith("optimizer.")) listener(payload);
				});
			}
		};
	}
}


/** Quarantined window.synth* accessors — import these instead of reading window. */
export const bridges = {
	get desktop() {
		return window.synthDesktop;
	},
	get runtime() {
		return window.synthRuntime;
	},
	get laguna() {
		return window.synthLaguna;
	},
	get trainingModels() {
		return window.synthTrainingModels;
	},
	get trainingArtifacts() {
		return window.synthTrainingArtifacts;
	},
	get whisper() {
		return window.synthWhisper;
	},
	get skills() {
		return window.synthSkills;
	},
	get context() {
		return window.synthContext;
	},
	get config() {
		return window.synthConfig;
	},
	get workspaceScope() {
		return window.synthWorkspaceScope;
	},
	get account() {
		return window.synthAccount;
	},
	get codexOauth() {
		return window.synthCodexOauth;
	},
	get codex() {
		return window.synthCodex;
	},
	get core() {
		return window.synthCore;
	},
	get intern() {
		return window.synthIntern;
	},
	get inventory() {
		return window.synthInventory;
	},
	get modelPerformance() {
		return window.synthModelPerformance;
	},
	get usage() {
		return window.synthUsage;
	},
	get tariffs() {
		return window.synthTariffs;
	},
	get updates() {
		return window.synthUpdates;
	},
	get plugins() {
		return window.synthPlugins;
	},
	get computerUse() {
		return window.synthComputerUse;
	},
	get browserAdmin() {
		return window.synthBrowserAdmin;
	},
	get visuals() {
		return window.synthVisuals;
	},
	get reports() {
		return window.synthReports;
	},
	get optimizers() {
		return window.synthOptimizers;
	},
	get secrets() {
		return window.synthSecrets;
	},
	get telemetry() {
		return window.synthTelemetry;
	},
	get terminal() {
		return window.synthTerminal;
	}
} as const;

/** True when the renderer is running inside the packaged Tauri webview. */
export function isDesktopApp(): boolean {
	return window.location.protocol === "tauri:" || "__TAURI_INTERNALS__" in window;
}
