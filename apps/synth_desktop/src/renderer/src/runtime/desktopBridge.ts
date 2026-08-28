import { listen } from "@tauri-apps/api/event";
import { EVENT_CHANNELS, fromGenerated, n, wire, type EventOrigin } from "../bridge";
import { commands as spectaCommands } from "../generated/protocol";
import { open } from "@tauri-apps/plugin-dialog";
import desktopPackage from "../../../../package.json";
import type { AppEvent, InternSessionControlRequest, InternSessionCreateRequest, InternSessionSendRequest, RuntimeEvent, Session } from "@synth/runtime-protocol";
import type { CodexEvent, ComposerImageAttachment, DesktopInstanceDiagnostics, HostedTrainingModelCatalog, LagunaAdapterStatus, LagunaDownloadProgress, LagunaModelHit, LagunaPolicy, LagunaStatus, ModelPerformanceSummary, ModelPerformanceTurnSample, OptimizerInferDelta, OptimizerRunOutputs, PersistedCodexSession, RequestOptions, RuntimeBridge, SavedLoraCheckpoint, SavedLoraCheckpointPage, SavedLoraDownload, SavedLoraRunPage, SecretsBridge, TerminalEvent, TrainingModelDownloadProgress, WhisperDownloadProgress, WhisperRuntimeStatus } from "../bridge";
import type { CoreDiagnostics } from "@synth/runtime-protocol";
import type { ContainerDeployment, TraceV5Record, UsageLedgerEntry, UsageWindow } from "@synth/runtime-protocol";
import { publicError } from "../runtime/publicError";

// The packaged WebKit view is always served from the `tauri:` protocol.  The
// injected internals global can appear too late for eager ES-module evaluation,
// so treating it as the only signal can accidentally install the browser/
// legacy-runtime bridge inside the desktop app.
const isTauri = window.location.protocol === "tauri:" || "__TAURI_INTERNALS__" in window;

function bridgeResult<T>(promise: Promise<unknown>): Promise<T> {
	return promise as Promise<T>;
}

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
	return { sessionId: event.sessionId, method: event.kind, params, createdAt: event.createdAt };
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
		runId: event.runId ?? null,
		source: event.source,
		kind: event.eventKind,
		payload: event.payload,
		remoteSequence: event.remoteSequence ?? undefined,
		commandId: event.commandId ?? null,
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
	memoryBytes: 0,
	idleSeconds: 0,
	idleUnloadAfterSeconds: 0,
	lastUsedAt: 0,
	freeAt: 0,
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
				previewUrl: await fromGenerated(spectaCommands.desktopImagePreview(path))
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
			const selection = await fromGenerated(spectaCommands.workspaceChooseDirectory()).catch(() =>
				open({ directory: true, multiple: false })
			);
			return typeof selection === "string" ? selection : null;
		}
	};
	window.synthLaguna ??= isTauri
		? {
			getStatus: () => fromGenerated(spectaCommands.lagunaGetStatus()),
			reload: () => fromGenerated(spectaCommands.lagunaReload()),
			freeMemory: () => fromGenerated(spectaCommands.lagunaModelUnload()),
			listModels: () => fromGenerated(spectaCommands.lagunaModelsList()),
			chooseModelDirectory: async () => {
				const selection = await open({ directory: true, multiple: false, title: "Choose a Laguna model folder" });
				return typeof selection === "string" ? selection : null;
			},
			setModelDirectory: (path) => bridgeResult<LagunaModelHit>(fromGenerated(spectaCommands.lagunaModelsSetDirectory(path))),
			clearModelDirectory: () => fromGenerated(spectaCommands.lagunaModelsClearDirectory()).then(() => undefined),
			policies: () => bridgeResult<LagunaPolicy[]>(fromGenerated(spectaCommands.lagunaPolicies())),
			registerPolicy: (checkpointId, modelId) =>
				bridgeResult<LagunaPolicy>(fromGenerated(spectaCommands.lagunaRegisterPolicy(checkpointId, modelId))),
			adapterStatus: () => bridgeResult<LagunaAdapterStatus[]>(fromGenerated(spectaCommands.lagunaAdapterStatus())),
			adapterDownload: (modelId) => bridgeResult<LagunaAdapterStatus>(fromGenerated(spectaCommands.lagunaAdapterDownload(modelId))),
			downloadModel: (modelId) => bridgeResult<LagunaModelHit>(fromGenerated(spectaCommands.lagunaModelDownload(modelId))),
			deleteModel: (modelId) => fromGenerated(spectaCommands.lagunaModelDelete(modelId)).then(() => undefined),
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
					void fromGenerated(spectaCommands.lagunaGetStatus()).then((status) => {
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
			policies: async () => [],
			adapterStatus: async () => [],
			adapterDownload: async () => { throw new Error("Adapters require Synth Desktop"); },
			registerPolicy: async () => { throw new Error("Policies require Synth Desktop"); },
			onStatus: () => () => undefined
		};
	window.synthTrainingModels ??= isTauri
		? {
			listModels: () => fromGenerated(spectaCommands.trainingModelsList()),
			runtimeStatus: () => fromGenerated(spectaCommands.trainingMlxRuntimeStatus()),
			installRuntime: (confirm) => fromGenerated(spectaCommands.trainingMlxRuntimeInstall(confirm)),
			downloadModel: (modelId) =>
				fromGenerated(spectaCommands.trainingModelsDownload(modelId)),
			deleteModel: (modelId) =>
				fromGenerated(spectaCommands.trainingModelsDelete(modelId)),
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
			runtimeStatus: async () => ({ installed: false, executable: null, version: "0.0.1", installHint: "Install the Synth MLX training runtime, then check again." }),
			installRuntime: async () => { throw new Error("MLX runtime installation requires Synth Desktop"); },
			downloadModel: async () => { throw new Error("Training model downloads require Synth Desktop"); },
			deleteModel: async () => { throw new Error("Training model deletion requires Synth Desktop"); },
			onDownloadProgress: () => () => undefined
		};
	// @ts-expect-error generated command DTOs vs Window TrainingArtifactsBridge
	window.synthTrainingArtifacts ??= isTauri
		? {
			list: () => fromGenerated(spectaCommands.trainingArtifactsList()),
			get: (id) => fromGenerated(spectaCommands.trainingArtifactsGet(id)),
			launchInference: (request) =>
				fromGenerated(spectaCommands.trainingArtifactsLaunchInference(request.id, request.message ?? null, request.confirm)),
			export: (request) =>
				fromGenerated(spectaCommands.trainingArtifactsExport(request.id, request.destination, request.expectedDigest ?? null, request.confirm)),
			delete: (request) =>
				fromGenerated(spectaCommands.trainingArtifactsDelete(request.id, request.confirm))
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
			getRuntimeStatus: () => fromGenerated(spectaCommands.whisperRuntimeStatus()),
			warmSelected: () => fromGenerated(spectaCommands.whisperRuntimeWarm()),
			onRuntimeStatus: (listener) => {
				let unlisten: (() => void) | undefined;
				void listen<WhisperRuntimeStatus>(EVENT_CHANNELS.WHISPER_RUNTIME, (event) => listener(event.payload)).then((dispose) => { unlisten = dispose; });
				return () => unlisten?.();
			},
			listModels: () => fromGenerated(spectaCommands.whisperModelsList()),
			downloadModel: (id) => fromGenerated(spectaCommands.whisperModelDownload(id)),
			onDownloadProgress(listener) {
				let disposed = false;
				let unlisten: (() => void) | undefined;
				void listen<WhisperDownloadProgress>(EVENT_CHANNELS.WHISPER_DOWNLOAD, ({ payload }) => listener(payload)).then((next) => {
					if (disposed) next();
					else unlisten = next;
				});
				return () => { disposed = true; unlisten?.(); };
			},
			setSelected: (id) => fromGenerated(spectaCommands.whisperModelsSetSelected(id)).then(() => undefined),
			clearModel: (id) => fromGenerated(spectaCommands.whisperModelsClear(id)).then(() => undefined),
			transcribe: (audioPath) =>
				fromGenerated(spectaCommands.whisperTranscribe(audioPath)).then((result) => result.text),
			transcribeAudio: (base64, mimeType) =>
				fromGenerated(spectaCommands.whisperTranscribeBase64(base64, mimeType)).then(
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
			diagnostics: () => fromGenerated(spectaCommands.coreDiagnostics()),
			eventsAfter: (afterSequence = 0, limit) =>
				fromGenerated(spectaCommands.coreEventsAfter(afterSequence, n(limit))),
			sessionEventsAfter: (sessionId, afterSequence = 0, limit) =>
				fromGenerated(spectaCommands.coreSessionEventsAfter(sessionId, afterSequence, n(limit))),
			sessionEventsTail: (sessionId, limit) =>
				fromGenerated(spectaCommands.coreSessionEventsTail(sessionId, n(limit))),
			sessionEventsBefore: (sessionId, beforeSequence, limit) =>
				fromGenerated(spectaCommands.coreSessionEventsBefore(sessionId, beforeSequence, n(limit))),
			onEvent(listener) {
				return listenRuntimeAppEvents(listener);
			}
		}
		: browserCoreBridge();
	window.synthIntern ??= isTauri
		? {
			listSessions: () => fromGenerated(spectaCommands.internSessionsList()) as Promise<Session[]>,
			createSession: (request) => fromGenerated(spectaCommands.internSessionCreate(wire(request))) as Promise<Session>,
			send: (request) => fromGenerated(spectaCommands.internSessionSend(wire(request))) as Promise<import("@synth/runtime-protocol").InternSessionSendResult>,
			control: (request) => fromGenerated(spectaCommands.internSessionControl(wire(request))) as Promise<import("@synth/runtime-protocol").InternSessionControlResult>,
			eventsAfter: (sessionId, afterSequence = 0, limit) =>
				fromGenerated(spectaCommands.internSessionEventsAfter(sessionId, afterSequence, n(limit))),
			onEvent(listener) {
				return listenRuntimeAppEvents((payload) => {
					if (payload.source === "intern") listener(payload);
				});
			}
		}
		: browserInternBridge();
window.synthAccount ??= isTauri
		? {
			beginSignIn: () => fromGenerated(spectaCommands.accountBeginSignIn()),
			pollSignIn: () => fromGenerated(spectaCommands.accountPollSignIn()),
			cancelSignIn: () => fromGenerated(spectaCommands.accountCancelSignIn()),
			signOut: () => fromGenerated(spectaCommands.accountSignOut()),
			getSummary: () => fromGenerated(spectaCommands.accountGetSummary()) as Promise<import("../bridge").SynthAccountSummary>,
			refresh: () => fromGenerated(spectaCommands.accountRefresh()) as Promise<import("../bridge").SynthAccountSummary>,
			openBilling: (action, tier) => fromGenerated(spectaCommands.accountOpenBilling(action, n(tier)))
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
		getPolicy: () => fromGenerated(spectaCommands.productTelemetryGetPolicy()),
		setOptOut: (optOut) => fromGenerated(spectaCommands.productTelemetrySetOptOut(optOut))
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
		begin: () => fromGenerated(spectaCommands.codexOauthBegin()),
		completeManual: (redirectUrl) => fromGenerated(spectaCommands.codexOauthCompleteManual(redirectUrl)),
		status: () => fromGenerated(spectaCommands.codexOauthStatus()),
		ensureReady: () => fromGenerated(spectaCommands.codexOauthEnsureReady()),
		disconnect: () => fromGenerated(spectaCommands.codexOauthDisconnect()),
		cancel: () => fromGenerated(spectaCommands.codexOauthCancel())
	}
	: {
		begin: async () => { throw new Error("ChatGPT subscription sign-in requires Synth Desktop"); },
		completeManual: async () => { throw new Error("ChatGPT subscription sign-in requires Synth Desktop"); },
		status: async () => ({ state: "disconnected", action: "connect", canUseModels: false, guidance: "ChatGPT sign-in requires Synth Desktop.", configured: false, accountHint: null, lastRefresh: null, expiresAt: null }),
		ensureReady: async () => ({ state: "disconnected", action: "connect", canUseModels: false, guidance: "ChatGPT sign-in requires Synth Desktop.", configured: false, accountHint: null, lastRefresh: null, expiresAt: null }),
		disconnect: async () => ({ state: "disconnected", action: "connect", canUseModels: false, guidance: "ChatGPT sign-in requires Synth Desktop.", configured: false, accountHint: null, lastRefresh: null, expiresAt: null }),
		cancel: async () => undefined
	};
window.synthSecrets ??= isTauri
	? {
		list: (provider, scope) => fromGenerated(spectaCommands.secretsList(n(provider), n(scope))),
		create: (request) => fromGenerated(spectaCommands.secretsCreate(wire(request))),
		replace: (secretId, value) => fromGenerated(spectaCommands.secretsReplace(secretId, value)),
		delete: (secretId) => fromGenerated(spectaCommands.secretsDelete(secretId)),
		test: (secretId) => fromGenerated(spectaCommands.secretsTest(secretId)),
		requestEnvImport: (sourcePath, variableNames) => fromGenerated(spectaCommands.secretsRequestEnvImport(wire({ sourcePath, variableNames: variableNames ?? null }))),
		commitEnvImport: (requestId, selected, after, confirm) => fromGenerated(spectaCommands.secretsCommitEnvImport(requestId, selected, after, confirm ?? false)),
		denyEnvImport: (requestId) => fromGenerated(spectaCommands.secretsDenyEnvImport(requestId)),
		pending: () => fromGenerated(spectaCommands.secretsPending()),
		capabilities: () => fromGenerated(spectaCommands.secretsCapabilitiesList()) as ReturnType<SecretsBridge["capabilities"]>,
		revokeCapability: (capabilityId) => fromGenerated(spectaCommands.secretsRevokeCapability(capabilityId)),
		audit: (limit) => fromGenerated(spectaCommands.secretsAuditList(n(limit))),
		grantUse: (secretId, runId, recipeId, rememberRecipe, requestId) => fromGenerated(spectaCommands.secretsGrantUse(secretId, runId, recipeId, rememberRecipe, null, requestId ?? null)),
		denyUse: (secretId) => fromGenerated(spectaCommands.secretsDenyUse(secretId))
	}
	: {
		list: async () => [],
		create: async () => { throw new Error("Secrets require Synth Desktop"); },
		replace: async () => { throw new Error("Secrets require Synth Desktop"); },
		delete: async () => undefined,
		test: async () => { throw new Error("Secrets require Synth Desktop"); },
		requestEnvImport: async () => { throw new Error("Secrets require Synth Desktop"); },
		commitEnvImport: async () => [],
		denyEnvImport: async () => undefined,
		pending: async () => ({ imports: [], grants: [], proxy: { running: false, origin: null } }),
		capabilities: async () => [],
		revokeCapability: async () => undefined,
		audit: async () => [],
		grantUse: async () => ({ status: "denied" }),
		denyUse: async () => ({ status: "denied" })
	};
window.synthConfig ??= isTauri
		? {
			get: () => fromGenerated(spectaCommands.synthConfigGet()),
			update: (request) => fromGenerated(spectaCommands.synthConfigUpdate(wire(request))),
			listModelMultiAgent: () => fromGenerated(spectaCommands.modelMultiAgentList()),
			updateModelMultiAgent: (request) => fromGenerated(spectaCommands.modelMultiAgentUpdate(wire(request))),
			getWorkspaceAccess: () => fromGenerated(spectaCommands.workspaceAccessGet()),
			updateWorkspaceAccess: (request) => fromGenerated(spectaCommands.workspaceAccessUpdate(request)),
			getDesktopPermissions: () => fromGenerated(spectaCommands.desktopPermissionsGet()),
			updateDesktopPermissions: (request) => fromGenerated(spectaCommands.desktopPermissionsUpdate(request))
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
		get: (sessionId) => fromGenerated(spectaCommands.workspaceScopeGet(sessionId)),
		chooseAndAttach: (sessionId, proposedAccess) => fromGenerated(spectaCommands.workspaceScopeChooseAndAttach(sessionId, proposedAccess)),
		listRecentFolders: () => fromGenerated(spectaCommands.workspaceScopeRecentFolders()),
		attachRecent: (sessionId, path) => fromGenerated(spectaCommands.workspaceScopeAttachRecent(sessionId, path)),
		removeAttachment: (sessionId, path) => fromGenerated(spectaCommands.workspaceScopeRemoveAttachment(sessionId, path)),
		listGrants: (sessionId) => fromGenerated(spectaCommands.workspaceScopeGrantsList(sessionId)),
		approveRequest: (requestId) => fromGenerated(spectaCommands.workspaceScopeApproveRequest(requestId)),
		denyRequest: (requestId) => fromGenerated(spectaCommands.workspaceScopeDenyRequest(requestId))
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
window.synthProjectSources ??= isTauri
	? {
		get: () => fromGenerated(spectaCommands.projectSourcesGet()),
		refresh: () => fromGenerated(spectaCommands.projectSourcesRefresh()),
		add: (containers, recipes) => fromGenerated(spectaCommands.projectSourceAdd(containers, recipes)),
		remove: (path) => fromGenerated(spectaCommands.projectSourceRemove(path)),
		listRequests: (sessionId) => fromGenerated(spectaCommands.projectSourceRequestsList(sessionId)),
		approveRequest: (requestId) => fromGenerated(spectaCommands.projectSourceApprove(requestId)),
		denyRequest: (requestId) => fromGenerated(spectaCommands.projectSourceDeny(requestId))
	}
	: {
		get: async () => ({ configPath: "", sources: [], implicitRoots: [] }),
		refresh: async () => ({ configPath: "", sources: [], implicitRoots: [] }),
		add: async () => { throw new Error("Project sources require Synth Desktop"); },
		remove: async () => { throw new Error("Project sources require Synth Desktop"); },
		listRequests: async () => [],
		approveRequest: async () => { throw new Error("Project source approval requires Synth Desktop"); },
		denyRequest: async () => { throw new Error("Project source approval requires Synth Desktop"); }
	};
	window.synthTerminal ??= isTauri
		? {
			available: true,
			create: (request) => fromGenerated(spectaCommands.terminalCreate(request)),
			list: (workspaceId) => fromGenerated(spectaCommands.terminalList(n(workspaceId))),
			snapshot: (terminalId, afterSequence = 0) => fromGenerated(spectaCommands.terminalSnapshot(terminalId, afterSequence)),
			write: (terminalId, data) => fromGenerated(spectaCommands.terminalWrite(terminalId, data)),
			resize: (terminalId, cols, rows) => fromGenerated(spectaCommands.terminalResize(terminalId, cols, rows)),
			close: (terminalId) => fromGenerated(spectaCommands.terminalClose(terminalId)),
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
	// @ts-expect-error generated command DTOs vs Window InventoryBridge
	window.synthInventory ??= isTauri
		? {
			listContainers: () => fromGenerated(spectaCommands.dataContainersList()),
			getContainer: (containerId) => fromGenerated(spectaCommands.dataContainersGet(containerId)),
			registerContainer: (request) => fromGenerated(spectaCommands.dataContainersRegister(wire(request))),
			probeContainer: (containerId) => fromGenerated(spectaCommands.dataContainersProbe(containerId)),
			listTraces: () => fromGenerated(spectaCommands.dataTracesList()),
			getTrace: (traceId) => fromGenerated(spectaCommands.dataTracesGet(traceId)),
			materializeContainerTrace: (containerId, rolloutId) => fromGenerated(spectaCommands.dataTraceMaterialize(containerId, rolloutId)),
			chooseTraceInput: async () => {
				const selection = await open({
					directory: false,
					multiple: false,
					title: "Import Trace V5 bundle",
					filters: [{ name: "Trace bundles", extensions: ["zip", "json"] }]
				});
				return typeof selection === "string" ? selection : null;
			},
			ingestTraceBundle: (request) => fromGenerated(spectaCommands.dataTracesIngest(request)),
			resolveTraceProjection: (traceDigest, projectionKind = "rollout-inspector") =>
				fromGenerated(spectaCommands.dataTraceProjectionResolve(traceDigest, projectionKind)),
			listUsage: (limit = 100) => fromGenerated(spectaCommands.dataUsageList(limit)),
			counts: () => fromGenerated(spectaCommands.dataCounts())
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
			materializeContainerTrace: (containerId, rolloutId) => window.synthRuntime!.request("/v1/traces/import", { method: "POST", body: { container_id: containerId, rollout_id: rolloutId } }),
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
			summaries: () => bridgeResult<ModelPerformanceSummary[]>(fromGenerated(spectaCommands.modelPerformanceSummary())),
			turnSamples: (sessionId) => bridgeResult<ModelPerformanceTurnSample[]>(fromGenerated(spectaCommands.modelPerformanceTurnSamples(sessionId)))
		}
		: { summaries: async () => [], turnSamples: async () => [] };
	window.synthUpdates ??= isTauri
		? {
			status: () => fromGenerated(spectaCommands.updateStatus()),
			openDownload: () => fromGenerated(spectaCommands.updateOpenDownload())
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
			summary: (window: UsageWindow) => fromGenerated(spectaCommands.usageSummary(window))
		};
		window.synthTariffs ??= {
			catalog: () => fromGenerated(spectaCommands.tariffCatalog())
		};
	}
	window.synthSkills ??= isTauri
		? { list: () => fromGenerated(spectaCommands.skillsList()) }
		: {
			list: async () => [
				{ id: "use-synth-containers", name: "use-synth-containers", description: "Synth container discovery and Trace V5 evidence." },
				{ id: "use-synth-visuals", name: "use-synth-visuals", description: "Create and manage Synth visuals." },
				{ id: "use-synth-optimizers", name: "use-synth-optimizers", description: "Operate Synth optimizer runs and recipes." },
				{ id: "run-live-container-evals", name: "run-live-container-evals", description: "Run live container-backed eval rollouts." },
				{ id: "author-synth-diagrams", name: "author-synth-diagrams", description: "Author a Mermaid diagram into the right Visual pane." }
			]
		};
	// @ts-expect-error generated command DTOs vs Window ContextBridge
	window.synthContext ??= isTauri
		? {
			snapshot: (workspace) => fromGenerated(spectaCommands.contextSnapshot(workspace)),
			updateWorkspaceAgents: (workspace, content) => fromGenerated(spectaCommands.contextWorkspaceAgentsUpdate(workspace, content)),
			updateSkill: (workspace, skillId, enabled, content) => fromGenerated(spectaCommands.contextSkillUpdate(workspace, skillId, enabled, content ?? null)),
			updateMcpGroup: (workspace, groupId, enabled) => fromGenerated(spectaCommands.contextMcpGroupUpdate(workspace, groupId, enabled)),
			installCookbooks: (workspace) => fromGenerated(spectaCommands.contextCookbooksInstall(workspace)),
			cancelCookbooks: (workspace) => fromGenerated(spectaCommands.contextCookbooksCancel(workspace)),
			setCookbooksEnabled: (workspace, enabled) => fromGenerated(spectaCommands.contextCookbooksSetEnabled(workspace, enabled)),
			uninstallCookbooks: (workspace) => fromGenerated(spectaCommands.contextCookbooksUninstall(workspace))
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
			defaultWorkspace: () => fromGenerated(spectaCommands.codexDefaultWorkspace()),
			list: () => fromGenerated(spectaCommands.codexSessionsList()) as Promise<PersistedCodexSession[]>,
			start: (request) => fromGenerated(spectaCommands.codexSessionStart(wire(request))),
			startTurn: (sessionId, prompt, effort, options) =>
				fromGenerated(spectaCommands.codexTurnStart(wire({
						sessionId,
						prompt,
						effort,
						clientMessageId: options?.clientMessageId ?? null
					}))),
			sendTurn: (start, prompt, effort, options) =>
				fromGenerated(spectaCommands.codexTurnSend(wire({
						start,
						prompt,
						effort,
						compactBeforeModelSwitch: Boolean(options?.compactBeforeModelSwitch),
						clientMessageId: options?.clientMessageId ?? null
					}))),
			interrupt: (sessionId) => fromGenerated(spectaCommands.codexTurnInterrupt({ sessionId })),
			compact: (request) => fromGenerated(spectaCommands.codexThreadCompact(wire(request))),
			readThread: (sessionId, threadId, includeTurns = true) =>
				fromGenerated(spectaCommands.codexThreadRead({ sessionId, threadId, includeTurns })),
			listThreadItems: (sessionId, threadId, cursor, limit) =>
				fromGenerated(spectaCommands.codexThreadItemsList(wire({ sessionId, threadId, cursor: cursor ?? null, limit: limit ?? null }))),
			steerTurn: (sessionId, text) =>
				fromGenerated(spectaCommands.codexTurnSteer({ sessionId, text })),
			resolveApproval: (sessionId, approvalId, decision) => fromGenerated(spectaCommands.codexApprovalResolve({ sessionId, approvalId, decision })),
			close: (sessionId) => fromGenerated(spectaCommands.codexSessionClose({ sessionId })),
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
			listTemplates: (genre) => fromGenerated(spectaCommands.visualsTemplatesList(genre ?? null)),
			getTemplate: (templateId) => fromGenerated(spectaCommands.visualsTemplatesGet(templateId)),
			list: (query) => fromGenerated(spectaCommands.visualsList(wire(query ?? null))),
			get: (visualId) => fromGenerated(spectaCommands.visualsGet(visualId)),
			reportObservation: (observation) => fromGenerated(spectaCommands.visualsObservationReport(wire(observation))),
			revisions: (visualId) => fromGenerated(spectaCommands.visualsRevisions(visualId)),
			annotations: (visualId) => fromGenerated(spectaCommands.visualsAnnotationsList(visualId)),
			createAnnotation: (visualId, request) => fromGenerated(spectaCommands.visualsAnnotationCreate(visualId, wire(request))),
			listSeals: (visualId) => fromGenerated(spectaCommands.visualsSealsList(visualId ?? null)),
			seal: (visualId, revision) => fromGenerated(spectaCommands.visualsSeal(visualId, revision)),
			getSeal: (receiptDigest) => fromGenerated(spectaCommands.visualsSealGet(receiptDigest)),
			uploadStatus: (receiptDigest) => fromGenerated(spectaCommands.visualsUploadStatus(receiptDigest)),
			shareSeal: (receiptDigest) => fromGenerated(spectaCommands.visualsShareSeal(receiptDigest)),
			openShared: (committedUrl) => fromGenerated(spectaCommands.visualsOpenShared(committedUrl)),
			create: (request) => fromGenerated(spectaCommands.visualsCreate(wire(request))),
			update: (visualId, request) => fromGenerated(spectaCommands.visualsUpdate(visualId, wire(request))),
			save: (visualId, tsx) => fromGenerated(spectaCommands.visualsSave(visualId, tsx ?? null)),
			fork: (visualId, title, sessionId) =>
				fromGenerated(spectaCommands.visualsFork(visualId, title ?? null, sessionId ?? null)),
			archive: (visualId) => fromGenerated(spectaCommands.visualsArchive(visualId)),
			show: (visualId, sessionId) =>
				fromGenerated(spectaCommands.visualsShow(visualId, sessionId ?? null)),
			content: (visualId) => fromGenerated(spectaCommands.visualsContent(visualId)),
			renditions: (visualId) => fromGenerated(spectaCommands.visualsRenditions(visualId)),
			rendition: (visualId, format, theme, sizeClass) =>
				fromGenerated(spectaCommands.visualsRendition(
					visualId,
					format ?? null,
					theme ?? null,
					sizeClass ?? null
				)),
			render: (visualId) => fromGenerated(spectaCommands.visualsRender(visualId)),
			pollStream: (request) => fromGenerated(spectaCommands.visualStreamPoll(request)),
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
			status: (pluginId) => fromGenerated(spectaCommands.pluginsStatus(pluginId ?? null)),
			list: () => fromGenerated(spectaCommands.pluginsList()),
			setReleaseChannel: (pluginId, channel) =>
				fromGenerated(spectaCommands.pluginsSetReleaseChannel(pluginId, channel)),
			manage: (operation, pluginId, version) =>
				fromGenerated(spectaCommands.pluginsManage(
					operation,
					pluginId,
					version ?? null,
					null
				)) as Promise<import("../bridge").PluginActionReceipt>,
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
				fromGenerated(spectaCommands.computerUseStatus(sessionId ?? null)),
			install: () => fromGenerated(spectaCommands.computerUseInstall()),
			remove: () => fromGenerated(spectaCommands.computerUseRemove()),
			revokeApp: (bundleId) => fromGenerated(spectaCommands.computerUseRevokeApp(bundleId)),
			openSettings: (permissionId) =>
				fromGenerated(spectaCommands.computerUseOpenSettings(permissionId))
		};
		window.synthBrowserAdmin ??= {
			status: () => fromGenerated(spectaCommands.browserRuntimeStatus()),
			allowOrigin: (origin) => fromGenerated(spectaCommands.browserPolicyAllowOrigin(origin)),
			revokeOrigin: (origin) => fromGenerated(spectaCommands.browserPolicyRevokeOrigin(origin))
		};
		window.synthReports ??= {
			list: (query) => fromGenerated(spectaCommands.reportsList(wire(query ?? null))),
			get: (reportId) => fromGenerated(spectaCommands.reportsGet(reportId)),
			getRevision: (reportId, revision) =>
				fromGenerated(spectaCommands.reportsRevisionGet(reportId, revision ?? null)),
			validate: (reportId, revision) =>
				fromGenerated(spectaCommands.reportsValidate(reportId, revision ?? null)),
			pinAll: (reportId) => fromGenerated(spectaCommands.reportsPinAll(reportId)),
			create: (request) => fromGenerated(spectaCommands.reportsCreate(wire(request))),
			update: (reportId, request) => fromGenerated(spectaCommands.reportsUpdate(reportId, wire(request))),
			archive: (reportId) => fromGenerated(spectaCommands.reportsArchive(reportId)),
			restore: (reportId) => fromGenerated(spectaCommands.reportsRestore(reportId)),
			listVisibilityRequests: (reportId) =>
				fromGenerated(spectaCommands.reportsVisibilityRequests(reportId ?? null)),
			requestVisibility: (reportId, request) =>
				fromGenerated(spectaCommands.reportsVisibilityRequest(reportId, wire(request))),
			decideVisibility: (requestId, approved) =>
				fromGenerated(spectaCommands.reportsVisibilityDecide(requestId, approved)),
			seal: (reportId, revision) => fromGenerated(spectaCommands.reportsSeal(reportId, revision)),
			listSeals: (reportId) => fromGenerated(spectaCommands.reportsSealsList(reportId ?? null)),
			getSeal: (receiptDigest) => fromGenerated(spectaCommands.reportsSealGet(receiptDigest)),
			compareSeals: (leftDigest, rightDigest) =>
				fromGenerated(spectaCommands.reportsSealsCompare(leftDigest, rightDigest)),
			uploadStatus: (receiptDigest) =>
				fromGenerated(spectaCommands.reportsUploadStatus(receiptDigest)),
			shareSeal: (receiptDigest) => fromGenerated(spectaCommands.reportsShare(receiptDigest)),
			setAudience: (publicationId, request) =>
				fromGenerated(spectaCommands.reportsAudienceSet(publicationId, request)),
			revokeAudience: (publicationId, receiptDigest) =>
				fromGenerated(spectaCommands.reportsAudienceRevoke(publicationId, receiptDigest)),
			promote: (publicationId, slug) => fromGenerated(spectaCommands.reportsPromote(publicationId, slug)),
			openShared: (committedUrl) => fromGenerated(spectaCommands.reportsOpenShared(committedUrl)),
			listComments: (reportId, revision) =>
				fromGenerated(spectaCommands.reportsCommentsList(reportId, revision ?? null)),
			createComment: (reportId, revision, request) =>
				fromGenerated(spectaCommands.reportsCommentCreate(reportId, revision, wire(request))),
			listExperiments: (reportId) => fromGenerated(spectaCommands.reportsExperimentsList(reportId)),
			upsertExperiment: (reportId, request) =>
				fromGenerated(spectaCommands.reportsExperimentUpsert(reportId, wire(request))),
			listLog: (reportId) => fromGenerated(spectaCommands.reportsLogList(reportId)),
			appendLog: (reportId, request) => fromGenerated(spectaCommands.reportsLogAppend(reportId, wire(request))),
			onEvent(listener) {
				return listenRuntimeAppEvents((payload) => {
					if (payload.kind.startsWith("report.")) listener(payload);
				});
			}
		};
		window.synthOptimizers ??= {
			listAlgorithms: () => fromGenerated(spectaCommands.optimizersAlgorithmsList()) as Promise<import("../bridge").OptimizerAlgorithmInfo[]>,
			listRecipes: () => fromGenerated(spectaCommands.optimizersRecipesList()) as Promise<import("../bridge").OptimizerRecipeInfo[]>,
			startRecipe: (request) => fromGenerated(spectaCommands.optimizersRecipeStart(wire(request))),
			stageEvalCandidates: (request) =>
				fromGenerated(spectaCommands.optimizersStageEvalCandidates(wire(request))) as Promise<{ id: string; candidates: { id: string; label: string }[] }>,
			list: (query) => fromGenerated(spectaCommands.optimizersList(wire(query ?? null))),
			get: (optimizerRunId) => fromGenerated(spectaCommands.optimizersGet(optimizerRunId)),
			create: (request) => fromGenerated(spectaCommands.optimizersCreate(request)),
			refresh: (optimizerRunId) => fromGenerated(spectaCommands.optimizersRefresh(optimizerRunId)),
			eventsAfter: (optimizerRunId, afterSeq = 0, limit) =>
				fromGenerated(spectaCommands.optimizersEventsAfter(optimizerRunId, afterSeq, limit ?? null)),
			getState: (optimizerRunId, sliceId, atSeq) =>
				fromGenerated(spectaCommands.optimizersGetState(optimizerRunId, sliceId, atSeq ?? null)),
			getStateBatch: (optimizerRunId, slices, atSeq) =>
				fromGenerated(spectaCommands.optimizersGetStateBatch(optimizerRunId, slices ?? null, atSeq ?? null)),
			cancel: (optimizerRunId) => fromGenerated(spectaCommands.optimizersCancel(optimizerRunId)),
			pause: (optimizerRunId) => fromGenerated(spectaCommands.optimizersPause(optimizerRunId)),
			resume: (optimizerRunId) => fromGenerated(spectaCommands.optimizersResume(optimizerRunId)),
			openVisual: (optimizerRunId) => fromGenerated(spectaCommands.optimizersOpenVisual(optimizerRunId)),
			importLocal: (request) => fromGenerated(spectaCommands.optimizersImportLocal(request)),
			reconcileCloud: (request) => fromGenerated(spectaCommands.optimizersReconcileCloud(request)),
			listCloud: (query) =>
				fromGenerated(spectaCommands.optimizersListCloud(
					query?.algorithm ?? null,
					query?.status ?? null,
					query?.limit ?? null
				)),
			searchSavedLoras: (query) =>
				bridgeResult<SavedLoraCheckpointPage>(fromGenerated(spectaCommands.optimizersSavedLorasSearch(wire(query ?? null)))),
			listRunCheckpoints: (optimizerRunId) =>
				bridgeResult<SavedLoraRunPage>(fromGenerated(spectaCommands.optimizersRunCheckpointsList(optimizerRunId))),
			runOutputs: (optimizerRunId) =>
				bridgeResult<OptimizerRunOutputs>(fromGenerated(spectaCommands.optimizersRunOutputs(optimizerRunId))),
			hostedTrainingModels: () => bridgeResult<HostedTrainingModelCatalog>(fromGenerated(spectaCommands.optimizersTrainingModels())),
			archiveSavedLora: (checkpointId) =>
				bridgeResult<SavedLoraCheckpoint>(fromGenerated(spectaCommands.optimizersSavedLoraArchive(checkpointId))),
			savedLoraDownload: (checkpointId) =>
				bridgeResult<SavedLoraDownload>(fromGenerated(spectaCommands.optimizersSavedLoraDownload(checkpointId))),
			importSavedLora: (path) =>
				bridgeResult<SavedLoraCheckpoint>(fromGenerated(spectaCommands.optimizersSavedLoraImport(path))),
			patchSavedLora: (checkpointId, patch) =>
				bridgeResult<SavedLoraCheckpoint>(fromGenerated(spectaCommands.optimizersSavedLoraPatch(checkpointId, {
					name: patch.name ?? null,
					description: patch.description ?? null,
					tags: patch.tags ?? null
				}))),
			publishSavedLora: (checkpointId) =>
				bridgeResult<SavedLoraCheckpoint>(fromGenerated(spectaCommands.optimizersSavedLoraPublish(checkpointId))),
			inferCheckpoint: (request) =>
				fromGenerated(spectaCommands.optimizersCheckpointInfer(request)),
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
				fromGenerated(spectaCommands.optimizersTrainingReconcile(optimizerRunId)) as Promise<{ schemaVersion: "workshop.training_snapshot.v1"; runId: string; projection: import("../bridge").TrainingProjection }>,
			recordVisualReady: (request) => fromGenerated(spectaCommands.visualSubscriptionReady(wire(request))),
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
	get projectSources() {
		return window.synthProjectSources;
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
