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
import type { InstanceDiagnostics } from "../generated/protocol";

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

export type LagunaBridge = {
	getStatus(): Promise<LagunaStatus>;
	reload(): Promise<LagunaStatus>;
	freeMemory?(): Promise<{ released: boolean; conflict: boolean; detail: string | null }>;
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

export type ContextFile = { path: string; content: string; state: "bundled" | "absent" | "empty" | "overriding"; editable: boolean; version?: string | null };
export type ContextSkill = { id: string; name: string; description: string; source: "bundled" | "cookbook" | "yours"; enabled: boolean; editable: boolean; content: string; path?: string | null };
export type McpContextGroup = { id: string; label: string; enabled: boolean; servers: string[]; enabledTools: Record<string, string[]> };
export type CookbookContext = { enabled: boolean; installed: boolean; phase: string; pin?: string | null; digest?: string | null; path?: string | null; lastFetch?: string | null; detail?: string | null };
export type ContextSnapshot = { workshopAgents: ContextFile; workspaceAgents: ContextFile; cookbooks: CookbookContext; skills: ContextSkill[]; mcpGroups: McpContextGroup[] };
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

export type DesktopPermissionSettings = {
	configPath: string;
	approvalPolicy: "untrusted" | "on-request" | "never";
	sandboxMode: "read-only" | "workspace-write" | "danger-full-access";
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

/** Device-wide usage dashboard, aggregated natively over `usage_records`. */
export type UsageBridge = {
	summary(window: UsageWindow): Promise<UsageSummary>;
};

/** One provider price card, served from the native tariff catalog — the same
 * numbers the cost estimator prices with. */
export type TariffCard = {
	provider: string;
	modelId: string;
	inputUsdPerM: number;
	outputUsdPerM: number;
	cachedInputUsdPerM: number | null;
	cacheWriteUsdPerM: number | null;
};

export type TariffsBridge = {
	catalog(): Promise<TariffCard[]>;
};

/** Passive release check: version facts only. The download action always
 * opens the fixed public download page. */
export type UpdateStatus = {
	currentVersion: string;
	channel: string;
	latestVersion: string | null;
	updateAvailable: boolean;
};

export type UpdatesBridge = {
	status(): Promise<UpdateStatus>;
	openDownload(): Promise<void>;
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

export type VisualAnnotation = {
	id: string;
	visualId: string;
	visualRevision: number;
	sourceDigest?: string | null;
	selector: Record<string, unknown>;
	kind: "note" | "bug" | "highlight" | "reward" | "acceptance";
	body?: string | null;
	metadata: Record<string, unknown>;
	authorId: string;
	supersedesId?: string | null;
	tombstoned: boolean;
	createdAt: string;
	updatedAt: string;
};

export type VisualSeal = {
	receiptDigest: string;
	visualId: string;
	visualRevision: number;
	artifactId: string;
	schemaVersion: "synth.artifact-bundle.v1";
	compilerName: string;
	compilerVersion: string;
	runtimeDigest: string;
	indexDigest: string;
	dataDigest: string;
	receiptSizeBytes: number;
	totalSizeBytes: number;
	createdAt: string;
};

export type VisualSealBundle = {
	seal: VisualSeal;
	indexHtml: string;
	data: Record<string, unknown>;
	receipt: Record<string, unknown>;
};

export type VisualUpload = {
	receiptDigest: string;
	collectionId?: string | null;
	publicationId?: string | null;
	publicationRevision?: number | null;
	state: "prepared" | "uploading" | "finalizing" | "committed" | "failed";
	committedUrl?: string | null;
	error?: string | null;
	updatedAt: string;
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
	}): Promise<VisualRecord[]>;
	get(visualId: string): Promise<VisualRecord>;
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
	onEvent(listener: (event: AppEvent) => void): () => void;
	onShow(listener: (event: AppEvent) => void): () => void;
};

export type PluginStatus = {
	schemaVersion: string;
	pluginId: string;
	enabled: boolean;
	phase: string;
	installedVersion?: string | null;
	selectedVersion?: string | null;
	releaseChannel: "official" | "dev";
	catalogVersion: string;
	digest?: string | null;
	service: { phase: string; startedAt?: string | null; activeRuns: number };
	capabilitiesDigest?: string | null;
	algorithms: string[];
	templates: string[];
	lastActionReceiptId?: string | null;
	detail?: string | null;
};

export type PluginsBridge = {
	status(pluginId?: string | null): Promise<PluginStatus>;
	list(): Promise<PluginStatus[]>;
	setReleaseChannel(pluginId: "optimizers", channel: "official" | "dev"): Promise<PluginStatus>;
};

export type ReportStatus = "draft" | "sealed";
export type ExperimentStatus =
	| "planned"
	| "running"
	| "completed"
	| "failed"
	| "aborted"
	| "superseded"
	| "excluded";

export type ReportBlock = {
	blockId: string;
	kind: string;
	anchor: string;
	title?: string | null;
	payload: Record<string, unknown>;
	sourceRevision?: string | null;
	sourceDigest?: string | null;
	accessState: string;
	integrityState: string;
};

export type ReportSource = {
	sourceId: string;
	resourceKind: string;
	resourceId: string;
	resourceRevision?: string | null;
	resourceDigest?: string | null;
	relation: string;
	accessState: string;
	integrityState: string;
};

export type ReportClaim = {
	claimId: string;
	statement: string;
	status: string;
	evidenceRefs: string[];
};

export type ReportLimitation = {
	limitationId: string;
	body: string;
};

export type ReportRecord = {
	schemaVersion: string;
	id: string;
	projectRef?: string | null;
	currentRevision: number;
	title: string;
	summary?: string | null;
	authors: string[];
	status: ReportStatus;
	createdBy: string;
	createdAt: string;
	updatedAt: string;
	archivedAt?: string | null;
};

export type ReportRevision = {
	schemaVersion: string;
	reportId: string;
	revision: number;
	title: string;
	summary?: string | null;
	authors: string[];
	status: ReportStatus;
	blocks: ReportBlock[];
	sources: ReportSource[];
	claims: ReportClaim[];
	limitations: ReportLimitation[];
	contentDigest?: string | null;
	compilerName?: string | null;
	compilerVersion?: string | null;
	createdBy: string;
	createdAt: string;
};

export type ExperimentRecord = {
	experimentId: string;
	reportId?: string | null;
	revision?: number | null;
	title: string;
	hypothesis?: string | null;
	status: ExperimentStatus;
	protocolDigest?: string | null;
	arms: unknown;
	runs: unknown;
	results: unknown;
	evaluatorRefs: unknown;
	traceCollectionRefs: unknown;
	claimRefs: unknown;
	researchLogRefs: unknown;
	limitations: unknown;
	createdAt: string;
	createdBy: string;
};

export type ResearchLogEntry = {
	entryId: string;
	reportId?: string | null;
	sequence: number;
	occurredAt: string;
	recordedAt: string;
	author: string;
	actorKind: "human" | "agent" | string;
	entryKind: string;
	title: string;
	body: string;
	tags: string[];
	links: unknown;
	claimEffect?: string | null;
	supersedesEntryId?: string | null;
	sourceDigest?: string | null;
};

export type ReportSeal = {
	receiptDigest: string;
	reportId: string;
	reportRevision: number;
	schemaVersion: string;
	compilerName: string;
	compilerVersion: string;
	runtimeDigest: string;
	indexDigest: string;
	dataDigest: string;
	receiptSizeBytes: number;
	totalSizeBytes: number;
	createdAt: string;
};

export type ReportSealBundle = {
	seal: ReportSeal;
	indexHtml: string;
	data: Record<string, unknown>;
	receipt: Record<string, unknown>;
};

export type ReportRevisionCompare = {
	left: ReportSealBundle;
	right: ReportSealBundle;
	sameDigest: boolean;
};

export type ReportUpload = {
	receiptDigest: string;
	collectionId?: string | null;
	publicationId?: string | null;
	publicationRevision?: number | null;
	state: "prepared" | "uploading" | "finalizing" | "committed" | "failed";
	committedUrl?: string | null;
	error?: string | null;
	updatedAt: string;
};

export type ReportPromotion = {
	publicationId: string;
	slug: string;
	status: "published" | "unpublished";
	publicUrl: string;
};

export type ReportVisibilityRequest = {
	requestId: string;
	reportId: string;
	reportRevision: number;
	receiptDigest: string;
	target: "private" | "public" | "unpublished";
	slug?: string | null;
	reason?: string | null;
	requestedBy: string;
	status: "pending" | "approved" | "denied" | "executed" | "failed" | "expired";
	decisionBy?: string | null;
	error?: string | null;
	createdAt: string;
	updatedAt: string;
	expiresAt: string;
};

export type ReportComment = {
	commentId: string;
	reportId: string;
	reportRevision: number;
	receiptDigest?: string | null;
	publicationId?: string | null;
	anchor?: string | null;
	body: string;
	authorId: string;
	createdAt: string;
};

export type ReportsBridge = {
	list(query?: { status?: string; search?: string; limit?: number; includeArchived?: boolean }): Promise<ReportRecord[]>;
	get(reportId: string): Promise<ReportRecord>;
	getRevision(reportId: string, revision?: number | null): Promise<ReportRevision>;
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

export type OptimizersBridge = {
	listAlgorithms(): Promise<OptimizerAlgorithmInfo[]>;
	listRecipes(): Promise<Array<{
		id: string;
		title: string;
		algorithmId: string;
		task: string;
		availability: string;
		limits: Record<string, number>;
	}>>;
	startRecipe(request: { recipeId: string; sessionRef?: string; openVisual?: boolean; baseModel?: string }): Promise<OptimizerRunRecord>;
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

export type CodexOauthBegin = {
	authorizeUrl: string;
	mode: "auto" | "manual";
};

export type CodexOauthStatus = {
	state: "disconnected" | "authenticating" | "ready" | "expiring" | "expired" | "refresh_failed";
	action: "connect" | "wait" | "none" | "reauthenticate" | "retry";
	canUseModels: boolean;
	guidance: string;
	configured: boolean;
	accountHint?: string | null;
	lastRefresh?: string | null;
	expiresAt?: string | null;
};

export type CodexOauthBridge = {
	begin(): Promise<CodexOauthBegin>;
	completeManual(redirectUrl: string): Promise<CodexOauthStatus>;
	status(): Promise<CodexOauthStatus>;
	ensureReady(): Promise<CodexOauthStatus>;
	disconnect(): Promise<CodexOauthStatus>;
	cancel(): Promise<void>;
};
