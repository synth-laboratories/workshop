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
};

export type LagunaBridge = {
	getStatus(): Promise<LagunaStatus>;
	reload(): Promise<LagunaStatus>;
	onStatus(listener: (status: LagunaStatus) => void): () => void;
	listModels(): Promise<LagunaModelHit[]>;
	chooseModelDirectory(): Promise<string | null>;
	setModelDirectory(path: string): Promise<LagunaModelHit>;
	clearModelDirectory(): Promise<void>;
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
		apiKey?: string;
		openrouterApiKey?: string;
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
	apiKey: string;
	model: string;
	providerName: string;
	providerTitle: string;
	providerEnvKey: string;
	approvalPolicy?: string;
	sandbox?: string;
	threadId?: string;
	multiAgentVersion?: MultiAgentVersion;
};

export type CodexSessionInfo = { sessionId: string; threadId: string; turnId?: string | null };
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
	 */
	sendTurn?(request: CodexSessionStart, prompt: string, effort?: string): Promise<CodexSessionInfo>;
	interrupt(sessionId: string): Promise<void>;
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

export type SynthAccountBridge = {
	beginSignIn(): Promise<SynthSignInBegin>;
	pollSignIn(): Promise<SynthSignInPoll>;
	cancelSignIn(): Promise<void>;
};

declare global {
	interface Window {
		synthDesktop: {
			platform: string;
			chooseWorkspaceDirectory(): Promise<string | null>;
			getInstanceDiagnostics(): Promise<DesktopInstanceDiagnostics>;
		};
		/** Browser fixture/explicit compatibility bridge; not installed by Tauri. */
		synthRuntime?: RuntimeBridge;
		synthLaguna?: LagunaBridge;
		synthConfig?: SynthConfigBridge;
		synthAccount?: SynthAccountBridge;
		synthCodex?: CodexBridge;
		synthCore?: CoreBridge;
		synthIntern?: InternBridge;
		synthInventory?: InventoryBridge;
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
