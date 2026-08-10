/// <reference types="vite/client" />

import type { AppEvent, CodexActivityEvent, ContainerDeployment, CoreDiagnostics, InternSessionControlRequest, InternSessionControlResult, InternSessionCreateRequest, InternSessionSendRequest, InternSessionSendResult, ResolvedTraceProjection, RuntimeEvent, Session, TraceBundleIngestRequest, TraceBundleIngestResult, TraceV5Record, UsageLedgerEntry } from "@synth/runtime-protocol";

export {};

export type RequestOptions = {
	method?: "GET" | "POST" | "DELETE";
	body?: unknown;
};

export type EventSubscription = {
	close(): void;
};

export type DesktopInstanceDiagnostics = {
	mode: "development" | "canonical";
	name?: string | null;
	displayName: string;
	appVersion: string;
	sourceRevision: string;
	buildRevision: string;
	buildTimestamp: string;
	processId: number;
	executable: string;
	dataRoot: string;
	viteUrl?: string | null;
	manifest?: string | null;
};

export type RuntimeBridge = {
	request<T = unknown>(path: string, options?: RequestOptions): Promise<T>;
	subscribe(
		sessionId: string,
		afterSequence: number,
		onEvent: (event: RuntimeEvent) => void,
		onStatus?: (status: { state: string; detail?: string }) => void,
		onActivity?: (event: CodexActivityEvent) => void
	): Promise<EventSubscription>;
};

export type LagunaPhase =
	| "unknown"
	| "starting"
	| "loading"
	| "ready"
	| "unloaded"
	| "error"
	| "unavailable";

export type LagunaStatus = {
	phase: LagunaPhase;
	baseUrl: string | null;
	backend: string | null;
	loadedModel: string | null;
	detail: string | null;
	memoryBytes: number | null;
	idleSeconds?: number | null;
	idleUnloadAfterSeconds?: number | null;
	lastUsedAt?: number | null;
	freeAt?: number | null;
	updatedAt: number;
};

export type LagunaModelHit = {
	path: string;
	modelsRoot: string;
	modelId: string;
	shardCount: number;
	totalBytes: number;
	selected: boolean;
	runtimeReady: boolean;
	companionBytes: number;
};

export type LagunaDownloadProgress = {
	modelId: string;
	phase: "preparing" | "provisioning" | "downloading" | "ready" | "error";
	detail: string;
	downloadedBytes?: number;
	totalBytes?: number;
};

export type LagunaUnloadOutcome = {
	released: boolean;
	conflict: boolean;
	detail: string | null;
};

export type LagunaBridge = {
	getStatus(): Promise<LagunaStatus>;
	reload(): Promise<LagunaStatus>;
	freeMemory?(): Promise<LagunaUnloadOutcome>;
	onStatus(listener: (status: LagunaStatus) => void): () => void;
	listModels(): Promise<LagunaModelHit[]>;
	chooseModelDirectory(): Promise<string | null>;
	setModelDirectory(path: string): Promise<LagunaModelHit>;
	clearModelDirectory(): Promise<void>;
	downloadModel(modelId: string): Promise<LagunaModelHit>;
	deleteModel(modelId: string): Promise<void>;
	onDownloadProgress?(listener: (progress: LagunaDownloadProgress) => void): () => void;
};

export type WhisperModelHit = {
	id: string;
	title: string;
	description?: string | null;
	recommended: boolean;
	multilingual: boolean;
	downloadBytes: number;
	installedBytes?: number | null;
	path?: string | null;
	selected: boolean;
	modelsRoot: string;
};

export type WhisperDownloadProgress = {
	id: string;
	phase: "preparing" | "downloading" | "ready" | "error";
	detail: string;
	downloadedBytes?: number;
	totalBytes?: number;
};

export type WhisperRuntimeStatus = {
	phase: string;
	loadedModel: string | null;
	idleSeconds: number | null;
	idleUnloadAfterSeconds: number;
	lastUsedAt: number | null;
	freeAt: number | null;
	updatedAt: number;
};

export type WhisperBridge = {
	listModels(): Promise<WhisperModelHit[]>;
	downloadModel(id: string): Promise<WhisperModelHit>;
	onDownloadProgress?(listener: (progress: WhisperDownloadProgress) => void): () => void;
	getRuntimeStatus?(): Promise<WhisperRuntimeStatus>;
	onRuntimeStatus?(listener: (status: WhisperRuntimeStatus) => void): () => void;
	warmSelected?(): Promise<WhisperRuntimeStatus>;
	setSelected(id: string): Promise<void>;
	clearModel(id: string): Promise<void>;
	transcribe(audioPath: string): Promise<string>;
	/**
	 * Fallback for renderers that cannot write a temp file (no fs/path plugin
	 * wired yet). Renderer records with MediaRecorder, base64-encodes the blob,
	 * and hands it to the bridge instead of a file path.
	 */
	transcribeAudio?(base64: string, mimeType: string): Promise<string>;
};

export type SkillHit = {
	id: string;
	name: string;
	description: string;
};

export type SkillsBridge = {
	list(): Promise<SkillHit[]>;
};

export type SynthBackendSettings = {
	configPath: string;
	envFile: string;
	profile: string;
	backendUrl: string;
	apiKeyEnv: string;
	apiKeyConfigured: boolean;
	apiKeyFingerprint?: string | null;
	apiKeySource?: string | null;
	workerKeyConfigured: boolean;
	openrouterApiKeyConfigured: boolean;
	openrouterApiKeyFingerprint?: string | null;
	openrouterApiKeySource?: string | null;
};

export type MultiAgentVersion = "none" | "v1" | "v2";
export type ModelMultiAgentSetting = {
	modelId: string;
	displayName: string;
	preset: MultiAgentVersion;
	effective: MultiAgentVersion;
	overridden: boolean;
};

export type WorkspaceAccessSettings = {
	allowedRoots: string[];
};

export type SynthConfigBridge = {
	get(): Promise<SynthBackendSettings>;
	update(request: {
		profile: string;
		backendUrl: string;
		envFile: string;
		apiKeyEnv: string;
	}): Promise<SynthBackendSettings>;
	listModelMultiAgent(): Promise<ModelMultiAgentSetting[]>;
	updateModelMultiAgent(request: {
		modelId: string;
		version?: MultiAgentVersion | null;
	}): Promise<ModelMultiAgentSetting[]>;
	getWorkspaceAccess(): Promise<WorkspaceAccessSettings>;
	updateWorkspaceAccess(request: { allowedRoots: string[] }): Promise<WorkspaceAccessSettings>;
};

export type CodexSessionStart = {
	sessionId: string;
	workspace: string;
	baseUrl: string;
	model: string;
	providerName: string;
	providerTitle: string;
	providerEnvKey: string;
	approvalPolicy?: string;
	sandbox?: string;
	threadId?: string;
	forceNewThread?: boolean;
	multiAgentVersion?: MultiAgentVersion;
	autoCompactTokenLimit: number;
};

export type CodexSessionInfo = { sessionId: string; threadId: string; turnId?: string | null };
export type ComposerImageAttachment = { path: string; name: string; previewUrl: string };
export type PersistedCodexSession = {
	sessionId: string;
	threadId: string;
	workspace: string;
	model: string;
	providerName: string;
	providerTitle: string;
	baseUrl: string;
	status: string;
	title?: string | null;
	titleOrigin?: "default" | "automatic" | "manual" | null;
	approvalPolicy: string;
	sandbox: string;
};
export type CodexEvent = { sessionId: string; method: string; params: Record<string, unknown> };
/** Typed rejection payload of `codex_turn_send`. */
export type CodexTurnFailure = {
	code: "codex_session_detached" | "codex_turn_start_failed" | "codex_provider_unavailable" | string;
	message: string;
	sessionId: string;
	/** Developer detail. Debug logs only — never a user-facing surface. */
	detail: string;
};
export type CodexBridge = {
	defaultWorkspace(): Promise<string>;
	list(): Promise<PersistedCodexSession[]>;
	start(request: CodexSessionStart): Promise<CodexSessionInfo>;
	startTurn(sessionId: string, prompt: string, effort?: string): Promise<CodexSessionInfo>;
	/**
	 * Atomic attach-or-resume plus turn start. Optional because browser demo
	 * adapters and older test fixtures only implement start + startTurn.
	 * `options.compactBeforeModelSwitch` asks Rust to run `thread/compact/start`
	 * on the live source model before rebinding when the start request targets
	 * a different model (see `modelSwitchPlan.ts`).
	 */
	sendTurn?(
		request: CodexSessionStart,
		prompt: string,
		effort?: string,
		options?: { compactBeforeModelSwitch?: boolean }
	): Promise<CodexSessionInfo>;
	interrupt(sessionId: string): Promise<void>;
	/** Atomically attaches/resumes a Codex thread and starts ad-hoc compaction. */
	compact?(request: CodexSessionStart): Promise<void>;
	/** Mid-turn user input via Codex `turn/steer`. Optional on browser fixtures without a native runtime. */
	steerTurn?(sessionId: string, text: string): Promise<void>;
	resolveApproval(sessionId: string, approvalId: string, decision: "once" | "always" | "reject"): Promise<void>;
	close(sessionId: string): Promise<void>;
	onEvent(listener: (event: CodexEvent) => void): () => void;
};

export type CoreBridge = {
	diagnostics(): Promise<CoreDiagnostics>;
	eventsAfter(afterSequence?: number, limit?: number): Promise<AppEvent[]>;
	sessionEventsAfter(sessionId: string, afterSequence?: number, limit?: number): Promise<AppEvent[]>;
	onEvent(listener: (event: AppEvent) => void): () => void;
};

export type InternBridge = {
	listSessions(): Promise<Session[]>;
	createSession(request: InternSessionCreateRequest): Promise<Session>;
	send(request: InternSessionSendRequest): Promise<InternSessionSendResult>;
	control(request: InternSessionControlRequest): Promise<InternSessionControlResult>;
	eventsAfter(sessionId: string, afterSequence?: number, limit?: number): Promise<AppEvent[]>;
	onEvent(listener: (event: AppEvent) => void): () => void;
};

export type InventoryCounts = { containers: number; traces: number; usage: number };
export type InventoryBridge = {
	listContainers(): Promise<ContainerDeployment[]>;
	getContainer(containerId: string): Promise<ContainerDeployment>;
	registerContainer(request: { name?: string; baseUrl: string; location?: "local" | string; taskFamily?: string; metadata?: Record<string, unknown> }): Promise<ContainerDeployment>;
	probeContainer(containerId: string): Promise<ContainerDeployment>;
	listTraces(): Promise<TraceV5Record[]>;
	getTrace(traceId: string): Promise<TraceV5Record>;
	chooseTraceInput(): Promise<string | null>;
	ingestTraceBundle(request: TraceBundleIngestRequest): Promise<TraceBundleIngestResult>;
	resolveTraceProjection(traceDigest: string, projectionKind?: string): Promise<ResolvedTraceProjection>;
	listUsage(limit?: number): Promise<UsageLedgerEntry[]>;
	counts(): Promise<InventoryCounts>;
};

export type ModelPerformanceSummary = {
	provider: string;
	modelId: string;
	measurementKind: "decode" | "observed_stream" | "end_to_end" | "provider_reported";
	sampleCount: number;
	tpsP50: number | null;
	tpsP95: number | null;
	ttftP50Ms: number | null;
	lastObservedAt: string;
};

export type ModelPerformanceBridge = {
	summaries(): Promise<ModelPerformanceSummary[]>;
};

export type VisualTemplateMeta = {
	id: string;
	title: string;
	genre?: string | null;
	version?: string | null;
	description?: string | null;
	path?: string | null;
	shellPath?: string | null;
	exampleBinding?: Record<string, unknown> | null;
};

export type VisualsBridge = {
	listTemplates(genre?: string | null): Promise<VisualTemplateMeta[]>;
	getTemplate(templateId: string): Promise<VisualTemplateMeta>;
	list(query?: {
		status?: string;
		sessionId?: string;
		templateId?: string;
		search?: string;
		limit?: number;
		offset?: number;
	}): Promise<import("@synth/runtime-protocol").VisualRecord[]>;
	get(visualId: string): Promise<import("@synth/runtime-protocol").VisualRecord>;
	revisions(visualId: string): Promise<import("@synth/runtime-protocol").VisualRevision[]>;
	create(request: {
		templateId: string;
		title?: string;
		bindings?: Record<string, unknown>;
		id?: string;
		sessionId?: string;
		status?: string;
		traceId?: string;
		metadata?: Record<string, unknown>;
	}): Promise<import("@synth/runtime-protocol").VisualRecord>;
	update(visualId: string, request: Record<string, unknown>): Promise<import("@synth/runtime-protocol").VisualRecord>;
	save(visualId: string, tsx?: string | null): Promise<import("@synth/runtime-protocol").VisualRecord>;
	fork(visualId: string, title?: string | null, sessionId?: string | null): Promise<import("@synth/runtime-protocol").VisualRecord>;
	archive(visualId: string): Promise<import("@synth/runtime-protocol").VisualRecord>;
	show(visualId: string, sessionId?: string | null): Promise<import("@synth/runtime-protocol").VisualRecord>;
	onEvent(listener: (event: AppEvent) => void): () => void;
	onShow(listener: (event: AppEvent) => void): () => void;
};

export type OptimizersBridge = {
	listAlgorithms(): Promise<import("@synth/runtime-protocol").OptimizerAlgorithmInfo[]>;
	listRecipes(): Promise<Array<{
		id: string;
		title: string;
		algorithmId: string;
		task: string;
		availability: string;
		limits: Record<string, number>;
	}>>;
	startRecipe(request: { recipeId: string; sessionRef?: string; openVisual?: boolean }): Promise<import("@synth/runtime-protocol").OptimizerRunRecord>;
	list(query?: {
		status?: string;
		algorithmId?: string;
		source?: string;
		search?: string;
		sessionRef?: string;
		limit?: number;
		offset?: number;
	}): Promise<import("@synth/runtime-protocol").OptimizerRunRecord[]>;
	get(optimizerRunId: string): Promise<import("@synth/runtime-protocol").OptimizerRunRecord>;
	create(request: {
		algorithmId: string;
		algorithmVersion?: string;
		objective?: string;
		source?: string;
		projectRef?: string;
		sessionRef?: string;
		id?: string;
		openVisual?: boolean;
		seedFixture?: string;
		cloudConfig?: Record<string, unknown>;
		localPath?: string;
	}): Promise<import("@synth/runtime-protocol").OptimizerRunRecord>;
	refresh(optimizerRunId: string): Promise<import("@synth/runtime-protocol").OptimizerRunRecord>;
	eventsAfter(optimizerRunId: string, afterSeq?: number, limit?: number): Promise<unknown[]>;
	getState(optimizerRunId: string, sliceId: string, atSeq?: number): Promise<unknown>;
	getStateBatch(optimizerRunId: string, slices?: string[], atSeq?: number): Promise<unknown[]>;
	cancel(optimizerRunId: string): Promise<import("@synth/runtime-protocol").OptimizerRunRecord>;
	pause(optimizerRunId: string): Promise<import("@synth/runtime-protocol").OptimizerRunRecord>;
	resume(optimizerRunId: string): Promise<import("@synth/runtime-protocol").OptimizerRunRecord>;
	openVisual(optimizerRunId: string): Promise<import("@synth/runtime-protocol").OptimizerRunRecord>;
	importLocal(request: { path: string; sessionRef?: string; openVisual?: boolean }): Promise<import("@synth/runtime-protocol").OptimizerRunRecord>;
	reconcileCloud(request: { optimizerRunId: string; afterSeq?: number; openVisual?: boolean }): Promise<import("@synth/runtime-protocol").OptimizerRunRecord>;
	listCloud(query?: { algorithm?: string; status?: string; limit?: number }): Promise<unknown[]>;
	onEvent(listener: (event: AppEvent) => void): () => void;
};

export type TerminalInfo = { id: string; workspaceId: string; cwd: string; shell: string; title: string; status: "running" | "exited" | "failed"; createdAt: number; exitCode?: number | null };
export type TerminalEvent = { terminalId: string; sequence: number; kind: "output" | "exit" | "error"; dataBase64?: string | null; exitCode?: number | null; message?: string | null };
export type TerminalCreateRequest = { workspaceId: string; workspaceRoot: string; cwd?: string; cols?: number; rows?: number };
export type TerminalBridge = {
	available: boolean;
	create(request: TerminalCreateRequest): Promise<TerminalInfo>;
	list(workspaceId?: string): Promise<TerminalInfo[]>;
	snapshot(terminalId: string, afterSequence?: number): Promise<TerminalEvent[]>;
	write(terminalId: string, data: string): Promise<void>;
	resize(terminalId: string, cols: number, rows: number): Promise<void>;
	close(terminalId: string): Promise<void>;
	onEvent(listener: (event: TerminalEvent) => void): () => void;
};

export type WorkspaceAccessMode = "read_only" | "read_write";
export type WorkspaceAttachment = { path: string; access: WorkspaceAccessMode; source: "user_picker" | "recent_folder" | "agent_request" | "migrated_default"; createdAt: string };
export type ConversationWorkspaceScope = { sessionId: string; workspace: string; attachments: WorkspaceAttachment[]; revision: number; boundRevision: number; bindingStatus: "pending" | "active" | "failed"; bindingError?: string | null };
export type WorkspaceGrantRequest = { id: string; sessionId: string; path: string; access: WorkspaceAccessMode; reason: string; status: "pending" | "approved" | "denied"; createdAt: string; resolvedAt?: string | null };
export type WorkspaceScopeBridge = {
	get(sessionId: string): Promise<ConversationWorkspaceScope | null>;
	chooseAndAttach(sessionId: string, access: WorkspaceAccessMode): Promise<ConversationWorkspaceScope | null>;
	listRecentFolders(): Promise<string[]>;
	attachRecent(sessionId: string, path: string): Promise<ConversationWorkspaceScope>;
	removeAttachment(sessionId: string, path: string): Promise<ConversationWorkspaceScope>;
	listGrants(sessionId: string): Promise<WorkspaceGrantRequest[]>;
	approveRequest(requestId: string): Promise<ConversationWorkspaceScope | null>;
	denyRequest(requestId: string): Promise<WorkspaceGrantRequest>;
};

type SemanticEvalApi = {
	schemaVersion: "synth.desktop-eval-api.v1";
	getState(): unknown;
	listActions(): string[];
	invoke(action: string, argumentsValue?: Record<string, unknown>): Promise<unknown>;
};

export type SynthSignInBegin = {
	verificationUri: string;
	expiresAtEpochS: number;
};

export type SynthSignInPoll =
	| { status: "pending" }
	| { status: "active" }
	| { status: "expired"; reason: string };

/**
 * Shell account state. `pairing` is renderer-owned (a sign-in is in flight);
 * every other value is decided by the Rust host from the Account Snapshot.
 */
export type SynthAccountState =
	| "local_only"
	| "signed_out"
	| "pairing"
	| "active"
	| "limited"
	| "past_due"
	| "canceled"
	| "error"
	| "unknown";

/** `cloud` = Synth Cloud snapshot; `dev_seed` = the labelled local/dev stand-in. */
export type SynthAccountSource = "cloud" | "dev_seed" | "none";

export type SynthAccountPlan = {
	name: string;
	tier?: string;
	state?: string;
	/** False when the backend reports no dollar allowance: show no dollars. */
	metered?: boolean;
	monthlyAllowanceUsd?: number;
	usedUsd: number;
	remainingUsd?: number;
	resetsAt?: string;
	renewsAt?: string;
	source?: SynthAccountSource;
};

export type SynthAccountOrganization = {
	id: string;
	displayName?: string;
	role?: string;
};

export type SynthAccountUsageWindow = {
	events: number;
	costUsd: number;
};

export type SynthAccountCloudUsage = {
	today: SynthAccountUsageWindow;
	sevenDays: SynthAccountUsageWindow;
	thirtyDays: SynthAccountUsageWindow;
};

export type SynthAccountBilling = {
	checkoutUrl?: string;
	portalUrl?: string;
	upgradeTier?: string;
};

export type SynthAccountPlanOption = {
	tier: string;
	displayName: string;
	priceUsd: number;
	monthlyAllowanceUsd: number;
};

export type SynthAccountSummary = {
	signedIn: boolean;
	/** Absent on older hosts; the renderer derives a state when it is missing. */
	state?: SynthAccountState;
	accountId?: string;
	displayName?: string;
	email?: string;
	organization?: SynthAccountOrganization;
	environment: "local" | "dev" | "prod";
	source?: SynthAccountSource;
	plan?: SynthAccountPlan;
	cloudUsage?: SynthAccountCloudUsage;
	billing?: SynthAccountBilling;
	catalog?: SynthAccountPlanOption[];
	lastUpdated?: string;
	/** True when the cloud facts shown are a cached copy after a failed refresh. */
	stale?: boolean;
	error?: string;
};

export type SynthBillingAction = "upgrade" | "manage";

export type SynthAccountBridge = {
	beginSignIn(): Promise<SynthSignInBegin>;
	pollSignIn(): Promise<SynthSignInPoll>;
	cancelSignIn(): Promise<void>;
	signOut(): Promise<SynthBackendSettings>;
	getSummary(): Promise<SynthAccountSummary>;
	/** Force a snapshot refetch (retry, or return from hosted checkout). */
	refresh?(): Promise<SynthAccountSummary>;
	/** Opens a backend-issued hosted URL in the system browser. */
	openBilling?(action: SynthBillingAction, tier?: string): Promise<string>;
};

declare global {
	interface Window {
		synthDesktop: {
			platform: string;
			chooseWorkspaceDirectory(): Promise<string | null>;
			chooseImageFiles(): Promise<ComposerImageAttachment[]>;
			getInstanceDiagnostics(): Promise<DesktopInstanceDiagnostics>;
		};
		/** Browser fixture/explicit compatibility bridge; not installed by Tauri. */
		synthRuntime?: RuntimeBridge;
		synthLaguna?: LagunaBridge;
		synthWhisper?: WhisperBridge;
		synthSkills?: SkillsBridge;
		synthConfig?: SynthConfigBridge;
		synthWorkspaceScope?: WorkspaceScopeBridge;
		synthAccount?: SynthAccountBridge;
		synthCodex?: CodexBridge;
		synthCore?: CoreBridge;
		synthIntern?: InternBridge;
		synthInventory?: InventoryBridge;
		synthModelPerformance?: ModelPerformanceBridge;
		synthVisuals?: VisualsBridge;
		synthOptimizers?: OptimizersBridge;
		synthTerminal: TerminalBridge;
		__synthEval?: SemanticEvalApi;
		__synthPreferences?: {
			get(): unknown;
			set(raw: unknown): unknown;
			reset(): unknown;
			storageKey: string;
		};
	}
}
