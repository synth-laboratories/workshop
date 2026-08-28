/**
 * Desktop bridge DTOs and Window-facing bridge interfaces (Wave 2b).
 * Moved out of env.d.ts so that file stays Window declarations only.
 */

import type {
	AppEvent,
	CodexActivityEvent,
	ContainerDeployment,
	CoreDiagnostics,
	InternSessionControlRequest,
	InternSessionControlResult,
	InternSessionCreateRequest,
	InternSessionSendRequest,
	InternSessionSendResult,
	RecoveryNotice,
	ResolvedTraceProjection,
	RuntimeEvent,
	Session,
	TraceBundleIngestRequest,
	TraceBundleIngestResult,
	TraceV5Record,
	UsageLedgerEntry,
	UsageSummary,
	UsageWindow,
	VisualRecord,
	VisualRevision,
	OptimizerAlgorithmInfo,
	OptimizerRunRecord
} from "@synth/runtime-protocol";
export type { OptimizerAlgorithmInfo, OptimizerRunRecord };
import type {
	ArtifactMutationReceipt,
	BeginResult,
	BrowserRuntimeStatus,
	CapabilitySummary,
	CodexSessionInfo,
	CodexTurnFailure,
	ComputerUseSnapshot,
	ContextFile,
	ContextSkill,
	ContextSnapshot,
	ConversationWorkspaceScope,
	CookbookContext,
	DesktopPermissionSettings,
	ExperimentRecord,
	ExperimentStatus,
	InstanceDiagnostics,
	HostedTrainingModel,
	HostedTrainingModelCatalog,
	LagunaAdapterStatus,
	LagunaModelHit,
	LagunaPolicy,
	LagunaStatus,
	MaskedImportCandidate,
	McpContextGroup,
	MlxRuntimeStatus,
	ModelPerformanceSummary,
	ModelPerformanceTurnSample,
	ModelMultiAgentSetting,
	MultiAgentVersion,
	PendingGrantSummary,
	PluginPermission,
	PluginStatus,
	ReportAudience,
	ReportAudienceState,
	ReportBlock,
	ReportClaim,
	ReportComment,
	ReportLimitation,
	ReportPromotion,
	ReportRecord,
	ReportRevision,
	ReportRevisionCompare,
	ReportSeal,
	ReportSealBundle,
	ReportSource,
	ReportStatus,
	ReportUpload,
	ReportValidationFinding,
	ReportValidationResult,
	ReportVisibilityRequest,
	ResearchLogEntry,
	OptimizerRunOutputs,
	SavedLoraCheckpoint,
	SavedLoraCheckpointPage,
	SavedLoraDownload,
	SavedLoraRunPage,
	SecretAuditEvent,
	SecretSummary,
	SecretsInbox,
	SkillHit,
	Status,
	TariffCard,
	TemplateMeta,
	TerminalCreateRequest,
	TerminalEvent,
	TerminalInfo,
	TrainingArtifact,
	TrainingModelHit,
	UpdateStatus,
	VisualAnnotation,
	VisualSeal,
	VisualSealBundle,
	VisualUpload,
	WhisperModelHit,
	WhisperRuntimeStatus,
	ProjectSourceApproval,
	ProjectSourceCatalog,
	ProjectSourceInspection,
	ProjectSourceRequest,
	ProjectSourceRow,
	WorkspaceAccessMode,
	WorkspaceAccessSettings,
	WorkspaceAttachment,
	WorkspaceGrantRequest
} from "../generated/protocol";

export type {
	ArtifactMutationReceipt,
	BrowserRuntimeStatus,
	CodexSessionInfo,
	CodexTurnFailure,
	ComputerUseSnapshot,
	ContextFile,
	ContextSkill,
	ContextSnapshot,
	ConversationWorkspaceScope,
	CookbookContext,
	DesktopPermissionSettings,
	ExperimentRecord,
	ExperimentStatus,
	HostedTrainingModel,
	HostedTrainingModelCatalog,
	LagunaAdapterStatus,
	LagunaModelHit,
	LagunaPolicy,
	LagunaStatus,
	MaskedImportCandidate,
	McpContextGroup,
	MlxRuntimeStatus,
	ModelPerformanceSummary,
	ModelPerformanceTurnSample,
	ModelMultiAgentSetting,
	MultiAgentVersion,
	PendingGrantSummary,
	PluginPermission,
	PluginStatus,
	ReportAudience,
	ReportAudienceState,
	ReportBlock,
	ReportClaim,
	ReportComment,
	ReportLimitation,
	ReportPromotion,
	ReportRecord,
	ReportRevision,
	ReportRevisionCompare,
	ReportSeal,
	ReportSealBundle,
	ReportSource,
	ReportStatus,
	ReportUpload,
	ReportValidationFinding,
	ReportValidationResult,
	ReportVisibilityRequest,
	ResearchLogEntry,
	OptimizerRunOutputs,
	SavedLoraCheckpoint,
	SavedLoraCheckpointPage,
	SavedLoraDownload,
	SavedLoraRunPage,
	SecretAuditEvent,
	SecretSummary,
	SecretsInbox,
	SkillHit,
	TariffCard,
	TerminalCreateRequest,
	TerminalEvent,
	TerminalInfo,
	TrainingArtifact,
	TrainingModelHit,
	UpdateStatus,
	VisualAnnotation,
	VisualSeal,
	VisualSealBundle,
	VisualUpload,
	WhisperModelHit,
	WhisperRuntimeStatus,
	ProjectSourceApproval,
	ProjectSourceCatalog,
	ProjectSourceInspection,
	ProjectSourceRequest,
	ProjectSourceRow,
	WorkspaceAccessMode,
	WorkspaceAccessSettings,
	WorkspaceAttachment,
	WorkspaceGrantRequest
};


export type RequestOptions = {
	method?: "GET" | "POST" | "DELETE";
	body?: unknown;
};

export type EventSubscription = {
	close(): void;
};

/**
 * Desktop identity payload. Narrows specta-generated `InstanceDiagnostics.mode`
 * to the two runtime modes the UI branches on. Source of truth for fields is
 * `generated/protocol.ts` (tauri-specta seed command).
 */
export type DesktopInstanceDiagnostics = Omit<InstanceDiagnostics, "mode"> & {
	mode: "development" | "canonical";
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

export type LagunaDownloadProgress = {
	modelId: string;
	phase: "preparing" | "provisioning" | "downloading" | "ready" | "error";
	detail: string;
	downloadedBytes?: number;
	totalBytes?: number;
};

export type LagunaBridge = {
	getStatus(): Promise<LagunaStatus>;
	reload(): Promise<LagunaStatus>;
	freeMemory?(): Promise<{ released: boolean; conflict: boolean; detail: string | null }>;
	onStatus(listener: (status: LagunaStatus) => void): () => void;
	listModels(): Promise<LagunaModelHit[]>;
	chooseModelDirectory(): Promise<string | null>;
	setModelDirectory(path: string): Promise<LagunaModelHit>;
	clearModelDirectory(): Promise<void>;
	/** Selectable policies with whatever decode speed has been measured. */
	policies?(): Promise<LagunaPolicy[]>;
	/** Register a Laguna-compatible LoRA under a model id. Registration is not
	 *  selection: which policy a turn uses is decided by that turn's model. */
	registerPolicy?(checkpointId: string, modelId: string): Promise<LagunaPolicy>;
	/** The Synth-published finetune, installed or not. */
	adapterStatus?(): Promise<LagunaAdapterStatus[]>;
	/** Download, verify, install, and register the published finetune. */
	adapterDownload?(modelId: string): Promise<LagunaAdapterStatus>;
	downloadModel(modelId: string): Promise<LagunaModelHit>;
	deleteModel(modelId: string): Promise<void>;
	onDownloadProgress?(listener: (progress: LagunaDownloadProgress) => void): () => void;
};

export type TrainingModelDownloadProgress = {
	modelId: string;
	phase: "preparing" | "downloading" | "ready" | "error";
	detail: string;
	downloadedBytes?: number;
	totalBytes?: number;
};

export type TrainingModelsBridge = {
	listModels(): Promise<TrainingModelHit[]>;
	runtimeStatus(): Promise<MlxRuntimeStatus>;
	installRuntime(confirm: boolean): Promise<MlxRuntimeStatus>;
	downloadModel(modelId: string): Promise<TrainingModelHit>;
	deleteModel(modelId: string): Promise<void>;
	onDownloadProgress(listener: (progress: TrainingModelDownloadProgress) => void): () => void;
};

export type TrainingArtifactsBridge = {
	list(): Promise<TrainingArtifact[]>;
	get(id: string): Promise<TrainingArtifact>;
	launchInference(request: { id: string; message?: string; confirm: boolean }): Promise<{
		artifactId: string;
		policySnapshotId: string;
		reply: string;
		baseModelId: string;
		producingRunId: string;
		configDigest?: string | null;
		digest?: string | null;
	}>;
	export?(request: { id: string; destination: string; expectedDigest?: string; confirm: boolean }): Promise<ArtifactMutationReceipt>;
	delete?(request: { id: string; confirm: boolean }): Promise<ArtifactMutationReceipt>;
};

export type WhisperDownloadProgress = {
	id: string;
	phase: "preparing" | "downloading" | "ready" | "error";
	detail: string;
	downloadedBytes?: number;
	totalBytes?: number;
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

export type SkillsBridge = {
	list(): Promise<SkillHit[]>;
};

export type ContextBridge = {
	snapshot(workspace: string): Promise<ContextSnapshot>;
	updateWorkspaceAgents(workspace: string, content: string): Promise<ContextSnapshot>;
	updateSkill(workspace: string, skillId: string, enabled: boolean, content?: string | null): Promise<ContextSnapshot>;
	updateMcpGroup(workspace: string, groupId: string, enabled: boolean): Promise<ContextSnapshot>;
	installCookbooks(workspace: string): Promise<ContextSnapshot>;
	cancelCookbooks(workspace: string): Promise<ContextSnapshot>;
	setCookbooksEnabled(workspace: string, enabled: boolean): Promise<ContextSnapshot>;
	uninstallCookbooks(workspace: string): Promise<ContextSnapshot>;
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
	defaultModel?: {
		model: string;
		effort: string;
		providers: string[];
	};
};

export type SynthConfigBridge = {
	get(): Promise<SynthBackendSettings>;
	update(request: {
		profile: string;
		backendUrl: string;
		envFile: string;
		apiKeyEnv: string;
		/**
		 * Write-only. The host stores these in the 0600 env file and never
		 * returns them; `SynthBackendSettings` reports only fingerprint and
		 * source. Omit to leave the stored secret untouched.
		 */
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
	getDesktopPermissions(): Promise<DesktopPermissionSettings>;
	updateDesktopPermissions(request: {
		approvalPolicy: DesktopPermissionSettings["approvalPolicy"];
		sandboxMode: DesktopPermissionSettings["sandboxMode"];
	}): Promise<DesktopPermissionSettings>;
};

export type CodexSessionStart = {
	sessionId: string;
	workspace: string;
	baseUrl: string;
	apiKey?: string;
	model: string;
	providerName: string;
	providerTitle: string;
	providerEnvKey: string;
	approvalPolicy?: string;
	sandbox?: string;
	serviceTier?: "default" | "fast";
	threadId?: string;
	multiAgentVersion?: MultiAgentVersion;
	autoCompactTokenLimit: number;
	/** This Mac Laguna catalog id. Null loads the base Laguna XS weights. */
	adapter?: string | null;
};

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
	presentationEmotion?: string | null;
	presentationSummary?: string | null;
	/** This Mac Laguna catalog id. Null is the base model. */
	adapter?: string | null;
	/** Set when a previous process died holding this chat's turn. */
	recovery?: RecoveryNotice | null;
};
export type CodexEvent = { sessionId: string; method: string; params: Record<string, unknown>; createdAt?: string };
/** Typed rejection payload of `codex_turn_send`. */

export type CodexBridge = {
	defaultWorkspace(): Promise<string>;
	list(): Promise<PersistedCodexSession[]>;
	start(request: CodexSessionStart): Promise<CodexSessionInfo>;
	startTurn(
		sessionId: string,
		prompt: string,
		effort?: string,
		options?: { clientMessageId?: string }
	): Promise<CodexSessionInfo>;
	/**
	 * Atomic attach-or-resume plus turn start. Optional because browser demo
	 * adapters and older test fixtures only implement start + startTurn.
	 * `options.compactBeforeModelSwitch` asks Rust to run `thread/compact/start`
	 * on the live source model before rebinding when the start request targets
	 * a different model (see `modelSwitchPlan.ts`).
	 * `options.clientMessageId` is the optimistic transcript bubble id; Rust
	 * reuses it when journaling `message.created` so the host event collapses
	 * onto the same bubble.
	 */
	sendTurn?(
		request: CodexSessionStart,
		prompt: string,
		effort?: string,
		options?: { compactBeforeModelSwitch?: boolean; clientMessageId?: string }
	): Promise<CodexSessionInfo>;
	interrupt(sessionId: string): Promise<void>;
	/** Atomically attaches/resumes a Codex thread and starts ad-hoc compaction. */
	compact?(request: CodexSessionStart): Promise<void>;
	/** Ownership-checked child thread read. Optional on browser fixtures. */
	readThread?(sessionId: string, threadId: string, includeTurns?: boolean): Promise<unknown>;
	/** Paginated child thread items. Optional on browser fixtures. */
	listThreadItems?(sessionId: string, threadId: string, cursor?: string, limit?: number): Promise<unknown>;
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
	sessionEventsTail(sessionId: string, limit?: number): Promise<AppEvent[]>;
	sessionEventsBefore(sessionId: string, beforeSequence: number, limit?: number): Promise<AppEvent[]>;
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
	materializeContainerTrace(containerId: string, rolloutId: string): Promise<{ inspectable?: boolean; note?: string; traces?: Array<{ traceId?: string }> }>;
	chooseTraceInput(): Promise<string | null>;
	ingestTraceBundle(request: TraceBundleIngestRequest): Promise<TraceBundleIngestResult>;
	resolveTraceProjection(traceDigest: string, projectionKind?: string): Promise<ResolvedTraceProjection>;
	listUsage(limit?: number): Promise<UsageLedgerEntry[]>;
	counts(): Promise<InventoryCounts>;
};

export type ModelPerformanceBridge = {
	summaries(): Promise<ModelPerformanceSummary[]>;
	turnSamples(sessionId: string): Promise<ModelPerformanceTurnSample[]>;
};

/** Device-wide usage dashboard, aggregated natively over `usage_records`. */
export type UsageBridge = {
	summary(window: UsageWindow): Promise<UsageSummary>;
};

/** One provider price card, served from the native tariff catalog — the same
 * numbers the cost estimator prices with. */

export type TariffsBridge = {
	catalog(): Promise<TariffCard[]>;
};

/** Passive release check: version facts only. The download action always
 * opens the fixed public download page. */

export type UpdatesBridge = {
	status(): Promise<UpdateStatus>;
	openDownload(): Promise<void>;
};

export type VisualTemplateMeta = TemplateMeta;

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
	}): Promise<VisualRecord[]>;
	get(visualId: string): Promise<VisualRecord>;
	reportObservation(observation: {
		schemaVersion: "synth.rendered-visual-observation.v1";
		visualId: string;
		renderedRevision: number;
		bindingsDigest: string;
		transportState: string;
		rolloutCount: number;
		renderedFrameCount: number;
		semanticEventCount: number;
		terminal: boolean;
		error?: string | null;
		observedAt: string;
	}): Promise<void>;
	revisions(visualId: string): Promise<VisualRevision[]>;
	annotations(visualId: string): Promise<VisualAnnotation[]>;
	createAnnotation(visualId: string, request: {
		visualRevision: number;
		sourceDigest?: string | null;
		selector: Record<string, unknown>;
		kind: VisualAnnotation["kind"];
		body?: string | null;
		metadata?: Record<string, unknown>;
		authorId?: string;
		supersedesId?: string | null;
	}): Promise<VisualAnnotation>;
	listSeals(visualId?: string | null): Promise<VisualSeal[]>;
	seal(visualId: string, revision: number): Promise<VisualSeal>;
	getSeal(receiptDigest: string): Promise<VisualSealBundle>;
	uploadStatus(receiptDigest: string): Promise<VisualUpload | null>;
	shareSeal(receiptDigest: string): Promise<VisualUpload>;
	openShared(committedUrl: string): Promise<VisualSealBundle>;
	create(request: {
		templateId: string;
		title?: string;
		bindings?: Record<string, unknown>;
		id?: string;
		sessionId?: string;
		status?: string;
		traceId?: string;
		metadata?: Record<string, unknown>;
		content?: string;
	}): Promise<VisualRecord>;
	update(visualId: string, request: Record<string, unknown>): Promise<VisualRecord>;
	save(visualId: string, tsx?: string | null): Promise<VisualRecord>;
	fork(visualId: string, title?: string | null, sessionId?: string | null): Promise<VisualRecord>;
	archive(visualId: string): Promise<VisualRecord>;
	show(visualId: string, sessionId?: string | null): Promise<VisualRecord>;
	content(visualId: string): Promise<{
		visualId: string;
		revision: number;
		format: string;
		mediaType: string;
		digest: string;
		base64: string;
	}>;
	renditions(visualId: string): Promise<Array<Record<string, unknown>>>;
	rendition(
		visualId: string,
		format?: string | null,
		theme?: string | null,
		sizeClass?: string | null
	): Promise<{
		visualId: string;
		revision: number;
		format: string;
		mediaType: string;
		digest: string;
		base64: string;
	}>;
	render(visualId: string): Promise<VisualRecord>;
	pollStream(request: { visualId: string; pollUrl: string; after: number; limit: number }): Promise<unknown>;
	onEvent(listener: (event: AppEvent) => void, onAttached?: () => void): () => void;
	onShow(listener: (event: AppEvent) => void): () => void;
};

/** `PluginPermission` from src-tauri/src/plugins/types.rs. */
export type PluginPermissionState = "granted" | "denied" | "not_determined" | "not_applicable";

/** Mirrors the operations `PluginService::manage` accepts. */
export type PluginLifecycleOperation =
	| "enable"
	| "disable"
	| "install"
	| "start"
	| "stop"
	| "update"
	| "remove";

/** `PluginActionReceipt` from src-tauri/src/plugins/types.rs. */
export type PluginActionReceipt = {
	schemaVersion: string;
	receiptId?: string;
	pluginId: string;
	action: string;
	version?: string | null;
	digest?: string | null;
	approvalReceiptId?: string | null;
	startedAt?: string;
	finishedAt?: string;
	result: string;
	retainedData?: string;
	status?: PluginStatus | null;
	error?: string | null;
};

/** What `remove` actually did, so the page can report residue honestly. */
export type ComputerUseRemovalReport = {
	bundleRemoved: boolean;
	tccReset: string[];
	/** Grants macOS refused to reset. Surfaced, never swallowed. */
	tccResetFailed: string[];
	allowlistEntriesRemoved: number;
};

/**
 * Computer Use is human-only: there is no agent path to any of these. The
 * agent's MCP surface offers status and nothing else.
 */
export type ComputerUseBridge = {
	status(sessionId?: string | null): Promise<ComputerUseSnapshot>;
	install(): Promise<PluginStatus>;
	remove(): Promise<ComputerUseRemovalReport>;
	revokeApp(bundleId: string): Promise<number>;
	openSettings(permissionId: string): Promise<void>;
};

/** Human-only browser setup. Agent tools can consume policy but cannot mutate it. */
export type BrowserAdminBridge = {
	status(): Promise<BrowserRuntimeStatus>;
	allowOrigin(origin: string): Promise<BrowserRuntimeStatus>;
	revokeOrigin(origin: string): Promise<BrowserRuntimeStatus>;
};

export type PluginsBridge = {
	status(pluginId?: string | null): Promise<PluginStatus>;
	list(): Promise<PluginStatus[]>;
	setReleaseChannel(pluginId: "optimizers", channel: "official" | "dev"): Promise<PluginStatus>;
	/**
	 * Human-triggered lifecycle. Approval policy, active-run guards, retention
	 * classes, and receipts are enforced natively — the renderer never decides
	 * whether an action is permitted, only whether to offer it.
	 */
	manage?(
		operation: PluginLifecycleOperation,
		pluginId: string,
		version?: string | null
	): Promise<PluginActionReceipt>;
	/**
	 * Fires whenever the native sidecar status changes. Subscribing is what
	 * replaces polling the registry: every poll runs a live sidecar probe.
	 */
	onStatusChanged?(listener: () => void): () => void;
};

export type ReportReferenceMode = "live" | "pinned";
export type ReportAccessState = "available" | "redacted" | "forbidden" | "missing";
export type ReportIntegrityState = "verified" | "digest_mismatch" | "unresolved" | "unsupported" | "source_changed";

export type ReportsBridge = {
	list(query?: { status?: string; search?: string; limit?: number; includeArchived?: boolean }): Promise<ReportRecord[]>;
	get(reportId: string): Promise<ReportRecord>;
	getRevision(reportId: string, revision?: number | null): Promise<ReportRevision>;
	validate(reportId: string, revision?: number | null): Promise<ReportValidationResult>;
	pinAll(reportId: string): Promise<ReportRecord>;
	create(request: {
		title?: string;
		summary?: string;
		authors?: string[];
		projectRef?: string;
		id?: string;
		blocks?: ReportBlock[];
	}): Promise<ReportRecord>;
	update(reportId: string, request: {
		expectedRevision?: number;
		title?: string;
		summary?: string | null;
		authors?: string[];
		projectRef?: string;
		blocks?: ReportBlock[];
		sources?: ReportSource[];
		claims?: ReportClaim[];
		limitations?: ReportLimitation[];
	}): Promise<ReportRecord>;
	archive(reportId: string): Promise<ReportRecord>;
	restore(reportId: string): Promise<ReportRecord>;
	listVisibilityRequests(reportId?: string | null): Promise<ReportVisibilityRequest[]>;
	requestVisibility(reportId: string, request: {
		receiptDigest: string;
		target: "private" | "public" | "unpublished";
		slug?: string;
		reason?: string;
		requestedBy?: string;
	}): Promise<ReportVisibilityRequest>;
	decideVisibility(requestId: string, approved: boolean): Promise<ReportVisibilityRequest>;
	seal(reportId: string, revision: number): Promise<ReportSeal>;
	listSeals(reportId?: string | null): Promise<ReportSeal[]>;
	getSeal(receiptDigest: string): Promise<ReportSealBundle>;
	compareSeals(leftDigest: string, rightDigest: string): Promise<ReportRevisionCompare>;
	uploadStatus(receiptDigest: string): Promise<ReportUpload | null>;
	shareSeal(receiptDigest: string): Promise<ReportUpload>;
	setAudience(publicationId: string, request: {
		receiptDigest: string;
		audience: ReportAudience;
		redactionPolicyVersion: string;
	}): Promise<ReportAudienceState>;
	revokeAudience(publicationId: string, receiptDigest: string): Promise<ReportAudienceState>;
	promote(publicationId: string, slug: string): Promise<ReportPromotion>;
	openShared(committedUrl: string): Promise<ReportSealBundle>;
	listComments(reportId: string, revision?: number | null): Promise<ReportComment[]>;
	createComment(reportId: string, revision: number, request: {
		body: string;
		anchor?: string;
		authorId?: string;
		receiptDigest?: string;
		publicationId?: string;
	}): Promise<ReportComment>;
	listExperiments(reportId: string): Promise<ExperimentRecord[]>;
	upsertExperiment(reportId: string, request: {
		experimentId?: string;
		title: string;
		hypothesis?: string;
		status?: string;
		protocolDigest?: string;
		arms?: unknown;
		runs?: unknown;
		results?: unknown;
		evaluatorRefs?: unknown;
		traceCollectionRefs?: unknown;
		claimRefs?: unknown;
		researchLogRefs?: unknown;
		limitations?: unknown;
	}): Promise<ExperimentRecord>;
	listLog(reportId: string): Promise<ResearchLogEntry[]>;
	appendLog(reportId: string, request: {
		occurredAt?: string;
		author?: string;
		actorKind?: string;
		entryKind: string;
		title: string;
		body: string;
		tags?: string[];
		links?: unknown;
		claimEffect?: string;
		supersedesEntryId?: string;
	}): Promise<ResearchLogEntry>;
	onEvent?(listener: (event: AppEvent) => void): () => void;
};

export type OptimizerRecipeInfo = {
	id: string;
	title: string;
	algorithmId: string;
	task?: string;
	source?: string;
	semantics?: string;
	availability: string;
	availabilityReason?: string | null;
	description?: string;
	limits?: Record<string, unknown>;
	prerequisites?: string[];
};

export type OptimizerInferDelta = {
	checkpointId: string;
	family: string;
	delta: string;
	done: boolean;
};

export type OptimizersBridge = {
	listAlgorithms(): Promise<OptimizerAlgorithmInfo[]>;
	listRecipes(): Promise<OptimizerRecipeInfo[]>;
	startRecipe(request: {
		recipeId: string;
		sessionRef?: string;
		openVisual?: boolean;
		baseModel?: string;
		/** Required by `eval.*` recipes unless `trainingArtifactId` is set. */
		candidateSetId?: string;
		/** Registered-container identity for workspace baseline evals. */
		containerId?: string;
		/** Managed training adapter. Eval stages it and retains identity in the receipt. */
		trainingArtifactId?: string;
	}): Promise<OptimizerRunRecord>;
	stageEvalCandidates(request: {
		sessionRef: string;
		candidates: Array<{
			label: string;
			/** Workspace-relative file or directory. */
			path: string;
			entrypoint?: string;
			kind?: string;
			baseline?: boolean;
		}>;
	}): Promise<{ id: string; candidates: Array<{ id: string; label: string }> }>;
	list(query?: {
		status?: string;
		algorithmId?: string;
		source?: string;
		search?: string;
		sessionRef?: string;
		limit?: number;
		offset?: number;
	}): Promise<OptimizerRunRecord[]>;
	get(optimizerRunId: string): Promise<OptimizerRunRecord>;
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
	}): Promise<OptimizerRunRecord>;
	refresh(optimizerRunId: string): Promise<OptimizerRunRecord>;
	eventsAfter(optimizerRunId: string, afterSeq?: number, limit?: number): Promise<unknown[]>;
	getState(optimizerRunId: string, sliceId: string, atSeq?: number): Promise<unknown>;
	getStateBatch(optimizerRunId: string, slices?: string[], atSeq?: number): Promise<unknown[]>;
	cancel(optimizerRunId: string): Promise<OptimizerRunRecord>;
	pause(optimizerRunId: string): Promise<OptimizerRunRecord>;
	resume(optimizerRunId: string): Promise<OptimizerRunRecord>;
	openVisual(optimizerRunId: string): Promise<OptimizerRunRecord>;
	importLocal(request: { path: string; sessionRef?: string; openVisual?: boolean }): Promise<OptimizerRunRecord>;
	reconcileCloud(request: { optimizerRunId: string; afterSeq?: number; openVisual?: boolean }): Promise<OptimizerRunRecord>;
	listCloud(query?: { algorithm?: string; status?: string; limit?: number }): Promise<unknown[]>;
	searchSavedLoras?(query?: {
		search?: string;
		scope?: "all" | "mine" | "org";
		placement?: "all" | "this_mac" | "hosted";
		provider?: string;
		checkpointKind?: string;
		baseModel?: string;
		runId?: string;
		attemptId?: string;
		sourceCheckpointId?: string;
		optimizerAlgorithm?: "sft" | "cispo" | "ppo";
		status?: string;
		tags?: string[];
		limit?: number;
		offset?: number;
	}): Promise<SavedLoraCheckpointPage>;
	listRunCheckpoints(optimizerRunId: string): Promise<SavedLoraRunPage>;
	runOutputs(optimizerRunId: string): Promise<OptimizerRunOutputs>;
	hostedTrainingModels(): Promise<HostedTrainingModelCatalog>;
	archiveSavedLora(checkpointId: string): Promise<SavedLoraCheckpoint>;
	savedLoraDownload(checkpointId: string): Promise<SavedLoraDownload>;
	importSavedLora(path: string): Promise<SavedLoraCheckpoint>;
	patchSavedLora?(checkpointId: string, patch: { name?: string; description?: string; tags?: string[] }): Promise<SavedLoraCheckpoint>;
	publishSavedLora?(checkpointId: string): Promise<SavedLoraCheckpoint>;
	inferCheckpoint(request: { checkpointId: string; family: "chat_completions" | "responses"; body: Record<string, unknown> }): Promise<unknown>;
	onInferDelta?(listener: (event: OptimizerInferDelta) => void): () => void;
	reconcileTraining(optimizerRunId: string): Promise<{
		schemaVersion: "workshop.training_snapshot.v1";
		runId: string;
		projection: TrainingProjection;
	}>;
	recordVisualReady?(request: {
		visualId: string;
		optimizerRunId: string;
		templateId: string;
		replayedThrough: number;
		subscribedFrom: number;
		templateDigest?: string;
	}): Promise<unknown>;
	onEvent(listener: (event: AppEvent) => void): () => void;
};

export type TrainingProjection = {
	lifecycle: string;
	phase?: string | null;
	last_sequence: number;
	last_event_id?: string | null;
	attempt_id?: string | null;
	metrics: Record<string, number>;
	checkpoints: unknown[];
	evaluations: Array<{
		phase?: "baseline" | "checkpoint" | "final" | string;
		checkpoint_id?: string | null;
		artifact_digest?: string | null;
		step?: number | null;
		evaluator?: string | null;
		metric?: string;
		score?: number | null;
		loss?: number | null;
		baseline_score?: number | null;
		delta?: number | null;
		sample_count?: number | null;
		status?: string;
	}>;
	warnings: unknown[];
	latest_rollout?: unknown;
	tunnel_health?: { status?: string; occurred_at?: string; detail?: unknown } | null;
	provider_usage?: Record<string, unknown> | null;
	terminal_summary?: unknown;
	attempt_history: unknown[];
};

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

/**
 * Project sources: folders Workshop may discover executable declarations in.
 *
 * Deliberately separate from `WorkspaceScopeBridge`. A workspace attachment
 * grants file access to one conversation; a project source additionally lets
 * declared container commands from that folder be started. `approve` opens the
 * native picker and admits only if the selection matches the requested folder,
 * so no method here can widen a grant on its own.
 */
export type ProjectSourcesBridge = {
	get(): Promise<ProjectSourceCatalog>;
	refresh(): Promise<ProjectSourceCatalog>;
	add(containers: boolean, recipes: boolean): Promise<ProjectSourceCatalog | null>;
	remove(path: string): Promise<ProjectSourceCatalog>;
	listRequests(sessionId: string | null): Promise<ProjectSourceRequest[]>;
	approveRequest(requestId: string): Promise<ProjectSourceApproval | null>;
	denyRequest(requestId: string): Promise<ProjectSourceRequest>;
};

export type SemanticEvalApi = {
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
	usedUsd?: number;
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
	finalizedUsd?: number;
	pendingUsd?: number;
	tokens?: number;
	runtimeSeconds?: number;
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
	sessionHealth?: "local_only" | "signed_out" | "active" | "revoked" | "offline" | "malformed";
	failureKind?: "none" | "auth" | "entitlement" | "quota" | "outage" | "malformed";
	quotaExhausted?: boolean;
	reconciliation?: "ok" | "stale" | "failed";
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

export type ProductTelemetryPolicy = {
	dictionaryVersion: string;
	collectionPolicyVersion: string;
	optionalEnabled: boolean;
	consentVersion: string;
};

export type ProductTelemetryBridge = {
	getPolicy(): Promise<ProductTelemetryPolicy>;
	setOptOut(optOut: boolean): Promise<ProductTelemetryPolicy>;
};

export type CodexOauthBegin = BeginResult;

export type CodexOauthStatus = Status;

export type CodexOauthBridge = {
	begin(): Promise<CodexOauthBegin>;
	completeManual(redirectUrl: string): Promise<CodexOauthStatus>;
	status(): Promise<CodexOauthStatus>;
	ensureReady(): Promise<CodexOauthStatus>;
	disconnect(): Promise<CodexOauthStatus>;
	cancel(): Promise<void>;
};

export type SecretCapabilitySummary = CapabilitySummary;

export type SecretImportPreview = {
	requestId: string;
	status: string;
	sourcePath: string;
	candidates: MaskedImportCandidate[];
	sourceRemainsReadable: boolean;
	warning?: string | null;
	cleanupDiff?: string | null;
};

export type SecretsBridge = {
	list(provider?: string, scope?: string): Promise<SecretSummary[]>;
	create(request: { alias: string; provider: string; scope?: string; value: string }): Promise<SecretSummary>;
	replace(secretId: string, value: string): Promise<SecretSummary>;
	delete(secretId: string): Promise<void>;
	test(secretId: string): Promise<SecretSummary>;
	requestEnvImport(sourcePath: string, variableNames?: string[]): Promise<SecretImportPreview>;
	commitEnvImport(requestId: string, selected: string[], after: "keep" | "replace_aliases" | "remove_entries", confirm?: boolean): Promise<SecretSummary[]>;
	denyEnvImport(requestId: string): Promise<void>;
	pending(): Promise<SecretsInbox>;
	capabilities(): Promise<SecretCapabilitySummary[]>;
	revokeCapability(capabilityId: string): Promise<void>;
	audit(limit?: number): Promise<SecretAuditEvent[]>;
	grantUse(secretId: string, runId: string, recipeId: string, rememberRecipe: boolean, requestId?: string): Promise<unknown>;
	denyUse(secretId: string): Promise<unknown>;
};
