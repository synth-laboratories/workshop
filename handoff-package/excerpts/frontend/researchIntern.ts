import type { components } from "@/lib/generated/smr-openapi";

/** Canonical organization Research Intern. */
export type ResearchIntern = components["schemas"]["ResearchInternResponse"];

/** Interactive Sync runtime resource; not an Async mode flag. */
export type InternSyncSession = components["schemas"]["SyncSessionResponse"] & {
	metadata: Record<string, unknown>;
};

/** Browser connection claim used to acquire, renew, or release Sync presence. */
export type InternSyncPresenceLeaseRequest
	= components["schemas"]["SyncPresenceLeaseRequest"];

/** Expiring backend authority proving the authenticated operator is present. */
export type InternSyncPresenceLease
	= components["schemas"]["SyncPresenceLeaseResponse"];

/** One durable capability-gated MCP action proposed by either Intern runtime. */
export type InternMcpAction
	= components["schemas"]["InternMcpActionResponse"];

/** Exact typed operator decision card for one held Sync action. */
export type InternSyncApprovalCard
	= components["schemas"]["SyncApprovalCardV1"];

/** Operator-amended proposal fields carried by an `edit` decision. */
export type InternSyncExperimentAmendment = {
	title?: string | null;
	hypothesis?: string | null;
	proposed_diff_ref?: string | null;
};

/**
 * Explicit decision over one immutable Sync action snapshot. The
 * `edit` decision (approve-with-amendment, backend contract chain) is
 * typed locally until the generated schema catches up.
 */
export type InternSyncApprovalDecisionRequest
	= components["schemas"]["SyncApprovalDecisionRequestV1"]
	| {
		decision: "edit";
		comment?: string | null;
		amendment: InternSyncExperimentAmendment;
	};

/** Request to create an operator-driven Sync runtime resource. */
export type InternSyncSessionCreateRequest
	= components["schemas"]["SyncSessionCreateRequest"];

/** Autonomous, leave-safe organization-singleton Async runtime resource. */
export type InternAsyncRuntime
	= components["schemas"]["AsyncRuntimeResponse"];

/** Request to ensure the organization's one autonomous Async runtime. */
export type InternAsyncRuntimeEnsureRequest
	= components["schemas"]["AsyncRuntimeEnsureRequest"];

/** Async spend ceilings including day/month cost caps. */
export type InternAsyncRuntimeBudget
	= components["schemas"]["AsyncRuntimeBudget"];

/** Observed Async burn against day/month ceilings (includes sticky host idle). */
export type InternAsyncRuntimeSpend
	= components["schemas"]["AsyncRuntimeSpend"];

/** Sticky exe.dev host lease on the org Async projection. */
export type InternAsyncRuntimeHostLease
	= components["schemas"]["AsyncRuntimeHostLease"];

/** One open ask-and-continue judgment item on the Async projection. */
export type InternAsyncJudgmentItem
	= components["schemas"]["AsyncJudgmentItemResponse"];
/** Per-Effort work summary on the Async projection (pre–WP6 Effort board). */
export type InternAsyncEffortWorkSummary
	= components["schemas"]["AsyncEffortWorkSummary"];

/** Typed Async blocker surfaced by the autonomous runtime authority. */
export type InternAsyncBlocker
	= components["schemas"]["AsyncBlockerResponse"];

/** Idempotent request to open one exact Async blocker in Sync. */
export type InternAsyncBlockerOpenSyncRequest
	= components["schemas"]["AsyncBlockerOpenSyncRequest"];

/** Backend-owned blocker handoff, Sync session, and immutable receipt. */
export type InternAsyncBlockerOpenSyncResponse
	= components["schemas"]["AsyncBlockerOpenSyncResponse"];

/** Explicit operator disposition for one Sync-reviewed Async blocker. */
export type InternAsyncBlockerResolveRequest
	= components["schemas"]["AsyncBlockerResolveRequest"];

/** Terminal blocker receipt plus the durably admitted Async continuation. */
export type InternAsyncBlockerResolveResponse
	= components["schemas"]["AsyncBlockerResolveResponse"];

/** One of the Intern's durable Sync or Async conversation graphs. */
export type InternMetaThread
	= components["schemas"]["MetaThreadResponseV1"];

/** A branchable Sync lane or the singleton Async spine. */
export type InternMetaThreadSegment
	= components["schemas"]["MetaThreadSegmentResponseV1"];

/** Immutable summary produced when a Sync branch is sealed and merged. */
export type InternMetaHandoff
	= components["schemas"]["MetaHandoffResponseV1"]
	& {
		destination_segment_id?: string | null;
		parent_agent_config?: {
			agent_role: string;
			harness: string;
			model: string;
			reasoning_effort: string;
			segment_role?: string | null;
		} | null;
		child_agent_config?: {
			agent_role: string;
			harness: string;
			model: string;
			reasoning_effort: string;
			segment_role?: string | null;
		} | null;
		approved_at?: string | null;
		continued_at?: string | null;
		status:
			| "needs_review"
			| "approved"
			| "continued"
			| "rejected"
			| "superseded"
			| "merged"
			| "sealed";
	};

/** Product verb: unattended Async model/effort switch. */
export type InternAsyncHandoffModelRequest = {
	command_id: string;
	idempotency_key: string;
	expected_generation: number;
	summary: string;
	agent_config: {
		agent_role: string;
		harness: string;
		model: string;
		reasoning_effort: string;
		segment_role?: string | null;
	};
	evidence_references?: unknown[];
	require_review?: boolean;
};

/** Attended seal: park model switch at needs_review. */
export type InternAsyncHandoffReviewRequest = {
	idempotency_key: string;
	summary: string;
	agent_config: {
		agent_role: string;
		harness: string;
		model: string;
		reasoning_effort: string;
		segment_role?: string | null;
	};
	evidence_references?: unknown[];
};

/** Attended continue after approve. */
export type InternMetaHandoffContinueRequest = {
	child_agent_config?: InternAsyncHandoffReviewRequest["agent_config"] | null;
	summary?: string | null;
};

/** Durable message crossing the Intern's Sync and Async graph boundary. */
export type InternCrossMetaThreadMessage
	= components["schemas"]["CrossMetaThreadMessageResponseV1"];

/** Caller-owned cross-thread message identity and payload. */
export type InternCrossMetaThreadMessageCreateRequest
	= components["schemas"]["CrossMetaThreadMessageCreateRequestV1"];

/** Durable operator command admitted before Temporal receives its signal. */
export type InternRuntimeCommandRequest
	= components["schemas"]["InternRuntimeCommandRequest"];

/** Sync-only command envelope carrying execution mode and exact Visual context. */
export type InternSyncRuntimeCommandRequest
	= components["schemas"]["SyncRuntimeCommandRequest"];

/** Product-visible reducer decision for an Intern runtime command. */
export type InternRuntimeCommandReceipt
	= components["schemas"]["InternRuntimeCommandReceipt"];

/** Typed renderer failure for one exact immutable Visual revision. */
export type InternVisualRenderDiagnostic
	= components["schemas"]["VisualRenderDiagnosticV1"];

/** Sync operator request to repair one exact failed Visual revision. */
export type InternVisualRepairRequest
	= components["schemas"]["VisualRepairRequestV1"];

/** One ordered event from either independent Intern runtime authority. */
export type InternRuntimeEvent
	= components["schemas"]["InternRuntimeEventResponse"];

/** Non-mutating request from Sync to focus one exact authoritative resource. */
export type InternResourcePresentation = {
	schema_version: "smr.intern-resource-presentation.v1";
	resource_kind: "experiment" | "visual_revision" | "run" | "report" | "evidence";
	project_id: string;
	resource_id: string;
	revision?: number | null;
};

export type InternPresentationPane = "experiments" | "visuals" | "run" | "evidence";

export type InternResourcePresentationRequest = {
	eventId: string;
	presentation: InternResourcePresentation;
};

/** Strictly decodes the presentation payload; malformed outcomes are ignored. */
export function parseInternResourcePresentation(value: unknown): InternResourcePresentation | null {
	if (!isRecord(value)) return null;
	const schemaVersion = value.schema_version;
	const resourceKind = value.resource_kind;
	const projectId = value.project_id;
	const resourceId = value.resource_id;
	const revision = value.revision;
	if (
		schemaVersion !== "smr.intern-resource-presentation.v1"
		|| !["experiment", "visual_revision", "run", "report", "evidence"].includes(String(resourceKind))
		|| typeof projectId !== "string"
		|| !projectId
		|| typeof resourceId !== "string"
		|| !resourceId
	) return null;
	if (resourceKind === "visual_revision") {
		if (!Number.isInteger(revision) || Number(revision) < 1) return null;
	} else if (revision !== undefined && revision !== null) return null;

	return {
		schema_version: schemaVersion,
		resource_kind: resourceKind as InternResourcePresentation["resource_kind"],
		project_id: projectId,
		resource_id: resourceId,
		...(resourceKind === "visual_revision" ? { revision: Number(revision) } : {})
	};
}

/** Returns the latest valid presentation intent from the durable event stream. */
export function latestInternResourcePresentation(
	events: readonly InternRuntimeEvent[]
): InternResourcePresentation | null {
	return latestInternResourcePresentationRequest(events)?.presentation ?? null;
}

/** Returns the durable event identity with the latest valid focus request. */
export function latestInternResourcePresentationRequest(
	events: readonly InternRuntimeEvent[]
): InternResourcePresentationRequest | null {
	for (let index = events.length - 1;index >= 0;index -= 1) {
		const outer = events[index]?.payload;
		const detail = isRecord(outer?.payload) ? outer.payload : {};
		const parsed = parseInternResourcePresentation(detail.presentation);
		if (parsed) return { eventId: events[index].event_id, presentation: parsed };
	}

	return null;
}

/**
 * Resolves focus only after the caller has loaded owner-authoritative resources.
 * Stale, cross-project, or inaccessible IDs intentionally reduce to no action.
 */
export function resolveInternResourcePresentation(
	presentation: InternResourcePresentation,
	inventory: {
		projectId: string;
		experimentIds?: readonly string[];
		visualRevisions?: readonly { visualId: string;revision: number }[];
		runIds?: readonly string[];
		reportIds?: readonly string[];
		evidenceIds?: readonly string[];
	}
): InternPresentationPane | null {
	if (presentation.project_id !== inventory.projectId) return null;
	if (presentation.resource_kind === "experiment") {
		return inventory.experimentIds?.includes(presentation.resource_id) ? "experiments" : null;
	}
	if (presentation.resource_kind === "visual_revision") {
		return inventory.visualRevisions?.some((item) =>
			item.visualId === presentation.resource_id && item.revision === presentation.revision
		) ? "visuals" : null;
	}
	if (presentation.resource_kind === "run") {
		return inventory.runIds?.includes(presentation.resource_id) ? "run" : null;
	}
	if (presentation.resource_kind === "evidence") {
		return inventory.evidenceIds?.includes(presentation.resource_id) ? "evidence" : null;
	}

	return inventory.reportIds?.includes(presentation.resource_id) ? "evidence" : null;
}

/** SSE envelope over one ordered Intern runtime event. */
export type InternRuntimeEventStreamEnvelope
	= components["schemas"]["InternRuntimeEventStreamEnvelope"];

/** Owner-authored experiment history for a bound Sync project. */
export type InternExperimentHistory
	= components["schemas"]["SmrExperimentHistoryResponse"];

/** Owner-authored visual projection for a bound Sync project. */
export type InternVisualList
	= components["schemas"]["SmrVisualLibraryResponse"];

/** Typed action awaiting an operator decision in an active Sync session. */
export type InternSyncApproval
	= components["schemas"]["SyncApprovalCardV1"];

/** Durable handoff from an autonomous blocker into an operator-present session. */
export type InternAsyncBlockerOpenSync
	= components["schemas"]["AsyncBlockerOpenSyncResponse"];

export type InternAsyncBlockerHandoff
	= components["schemas"]["AsyncBlockerHandoffReceiptV1"];

/** Data binding visible to the organization's Intern. */
export type InternDataBinding
	= components["schemas"]["DataBindingResponse"];

/** Immutable dataset revision owned by the data control plane. */
export type InternDatasetRevision
	= components["schemas"]["DatasetRevisionResponse"];

/** Evidence inventory for an exact bound run. */
export type InternRunEvidence
	= components["schemas"]["SmrSwarmEvidenceResponse"];

/** Authoritative cost and meter projection for an exact bound run. */
export type InternRunUsage
	= components["schemas"]["SmrSwarmUsageResponse"];

/** A Factory attached to the canonical Research Intern. */
export type ResearchInternFactoryMembership
	= components["schemas"]["ResearchInternFactoryMembershipResponse"];

/** Factory visible to the current organization. */
export type ResearchFactory = components["schemas"]["SmrFactoryResponse"];

/** Project bound to a Factory. */
export type ResearchFactoryProject = components["schemas"]["SmrFactoryProjectResponse"];

/** Bounded recurring or one-shot Factory effort. */
export type ResearchFactoryEffort = components["schemas"]["SmrEffortResponse"];

/** Durable Factory progress event. */
export type ResearchFactoryEvent = components["schemas"]["SmrFactoryEventResponse"];

/** Factory cost projection. */
export type ResearchFactoryUsage = components["schemas"]["SmrFactoryUsageResponse"];

/** Candidate emitted by a Factory run. */
export type ResearchFactoryCandidate = components["schemas"]["SmrFactoryCandidateResponse"];

/** User-facing Factory result. */
export type ResearchFactoryResult = components["schemas"]["SmrFactoryResultResponse"];

/** Canonical backend projection for the exact Factory run an Intern will bind. */
export type ResearchInternRun = components["schemas"]["SmrRunResponse"];

/** Generated run identity and lifecycle fields exposed by the project run list. */
export type ResearchInternRunSummary = Pick<
	ResearchInternRun,
	"run_id" | "project_id" | "effort_id" | "created_at" | "started_at" | "finished_at"
> & {
	state: ResearchInternRun["public_state"];
};

/** Canonical backend actor projection used to prove runtime bindability. */
export type ResearchInternRunActors = components["schemas"]["SmrActorStatusListResponse"];

/** Research Intern proof for one completed Factory role. */
export type ResearchInternRoleReceipt = components["schemas"]["FactoryRoleReceiptResponse"];

/** Failure class rendered by the Research Intern UI. */
export type ResearchInternFailureKind
	= | "conflict"
	  | "denied"
	  | "not_found"
	  | "terminal"
	  | "rate_limited"
	  | "validation"
	  | "network"
	  | "failure";

/** Typed API failure that keeps operator-actionable HTTP semantics. */
export class ResearchInternApiError extends Error {
	readonly kind: ResearchInternFailureKind;

	readonly status: number | null;

	readonly retryable: boolean;

	/** Backend-typed condition code (e.g. intern_task_template_unknown). */
	readonly code: string | null;

	/** Raw typed error context forwarded by the proxy (e.g. known_templates). */
	readonly detail: unknown;

	constructor(
		message: string,
		options: {
			kind: ResearchInternFailureKind;
			status?: number | null;
			retryable?: boolean;
			code?: string | null;
			detail?: unknown;
		}
	) {
		super(message);
		this.name = "ResearchInternApiError";
		this.kind = options.kind;
		this.status = options.status ?? null;
		this.retryable = options.retryable ?? false;
		this.code = options.code ?? null;
		this.detail = options.detail ?? null;
	}
}

function isRecord(value: unknown): value is Record<string, unknown> {
	return Boolean(value && typeof value === "object" && !Array.isArray(value));
}

function failureKind(status: number): ResearchInternFailureKind {
	if (status === 401 || status === 403) return "denied";
	if (status === 404) return "not_found";
	if (status === 409 || status === 412) return "conflict";
	if (status === 410) return "terminal";
	if (status === 422) return "validation";
	if (status === 429) return "rate_limited";

	return "failure";
}

function errorMessage(
	payload: unknown,
	status: number
): string {
	if (payload && typeof payload === "object" && !Array.isArray(payload)) {
		const record = payload as Record<string, unknown>;
		for (const key of ["error", "message", "detail"]) {
			const value = record[key];
			if (typeof value === "string" && value.trim()) return value;
		}
		const detail = record.detail;
		if (detail && typeof detail === "object" && !Array.isArray(detail)) {
			const detailRecord = detail as Record<string, unknown>;
			for (const key of ["message", "error", "error_code"]) {
				const value = detailRecord[key];
				if (typeof value === "string" && value.trim()) return value;
			}
		}
		if (Array.isArray(detail)) {
			for (const item of detail) {
				if (!item || typeof item !== "object" || Array.isArray(item)) continue;
				const message = (item as Record<string, unknown>).msg;
				if (typeof message === "string" && message.trim()) return message;
			}
		}
	}

	return `Research Intern request failed (${status})`;
}

async function requestJson<T>(
	path: string,
	init?: RequestInit
): Promise<T> {
	let response: Response;
	try {
		response = await fetch(
			path,
			{
				cache: "no-store",
				...init,
				headers: init?.body == null
					? init?.headers
					: {
						"Content-Type": "application/json",
						...init.headers
					}
			}
		);
	} catch (error) {
		throw new ResearchInternApiError(
			error instanceof Error
				? error.message
				: "Research Intern network request failed",
			{
				kind: "network",
				retryable: true
			}
		);
	}
	const payload = (await response.json()
		.catch(() => null)) as unknown;
	if (!response.ok) {
		const record = isRecord(payload) ? payload : null;
		const nestedDetail = record && isRecord(record.detail)
			? record.detail
			: null;
		const code = [record?.error_code, nestedDetail?.error_code]
			.find((value): value is string => typeof value === "string" && value.trim() !== "") ?? null;
		throw new ResearchInternApiError(
			errorMessage(
				payload,
				response.status
			),
			{
				kind: failureKind(response.status),
				status: response.status,
				retryable: response.status === 409 || response.status === 429 || response.status >= 500,
				code,
				detail: nestedDetail ?? record?.detail ?? null
			}
		);
	}

	return payload as T;
}

/** Stable-enough idempotency key for one browser-originated mutation. */
export function createInternIdempotencyKey(prefix: string): string {
	const suffix
		= typeof crypto !== "undefined" && "randomUUID" in crypto
			? crypto.randomUUID()
			: `${Date.now()
				.toString(36)}-${Math.random()
				.toString(36)
				.slice(2)}`;

	return `${prefix}:${suffix}`;
}

/** Fetches the canonical Research Intern. */
export function getResearchIntern(): Promise<ResearchIntern> {
	return requestJson("/api/smr/research-intern");
}

/** Creates the canonical Research Intern for an organization. */
export function provisionResearchIntern(): Promise<ResearchIntern> {
	return requestJson(
		"/api/smr/research-intern",
		{
			method: "POST",
			body: JSON.stringify({
				display_name: "Research Intern",
				policies: {},
				metadata: { surface: "frontend" }
			})
		}
	);
}

/** Lists the independent interactive Sync runtime resources. */
export function listInternSyncSessions(): Promise<InternSyncSession[]> {
	return requestJson("/api/smr/research-intern/sync-sessions");
}

/** Lists the Intern's exactly one Sync and one Async MetaThread. */
export function listInternMetaThreads(): Promise<InternMetaThread[]> {
	return requestJson("/api/smr/research-intern/meta-threads");
}

/** Lists the root and branch/spine segments for one MetaThread. */
export function listInternMetaThreadSegments(metaThreadId: string): Promise<InternMetaThreadSegment[]> {
	return requestJson(`/api/smr/research-intern/meta-threads/${encodeURIComponent(metaThreadId)}/segments`);
}

/** Lists immutable Sync merge handoffs for one MetaThread. */
export function listInternMetaThreadHandoffs(metaThreadId: string): Promise<InternMetaHandoff[]> {
	return requestJson(`/api/smr/research-intern/meta-threads/${encodeURIComponent(metaThreadId)}/handoffs`);
}

/** Lists cross-thread protocol messages visible from one MetaThread. */
export function listInternMetaThreadMessages(metaThreadId: string): Promise<InternCrossMetaThreadMessage[]> {
	return requestJson(`/api/smr/research-intern/meta-threads/${encodeURIComponent(metaThreadId)}/messages`);
}

/** Sends a cross-thread message without replacing the caller's message ID. */
export function sendInternMetaThreadMessage(
	request: InternCrossMetaThreadMessageCreateRequest
): Promise<InternCrossMetaThreadMessage> {
	return requestJson(
		"/api/smr/research-intern/meta-threads/messages",
		{
			method: "POST",
			body: JSON.stringify(request)
		}
	);
}

/** Projects unresolved Async decision requests without inventing a second state store. */
export function pendingInternSyncRequests(
	messages: readonly InternCrossMetaThreadMessage[]
): InternCrossMetaThreadMessage[] {
	const resolved = new Set(messages
		.filter((message) => message.kind === "decision_resolved" && message.linked_message_id)
		.map((message) => message.linked_message_id));

	return messages.filter((message) =>
		message.kind === "request_decision" && !resolved.has(message.message_id)
	);
}

/** Creates a Sync session and starts its dedicated Temporal workflow. */
export function createInternSyncSession(request: InternSyncSessionCreateRequest): Promise<InternSyncSession> {
	return requestJson(
		"/api/smr/research-intern/sync-sessions",
		{
			method: "POST",
			body: JSON.stringify(request)
		}
	);
}

/** Refreshes one Sync session projection and its optimistic generation. */
export function getInternSyncSession(syncSessionId: string): Promise<InternSyncSession> {
	return requestJson(`/api/smr/research-intern/sync-sessions/${encodeURIComponent(syncSessionId)}`);
}

/** Acquires or renews the authenticated operator's short-lived Sync lease. */
export function acquireOrRenewInternSyncPresence(
	syncSessionId: string,
	request: InternSyncPresenceLeaseRequest
): Promise<InternSyncPresenceLease> {
	return requestJson(
		`/api/smr/research-intern/sync-sessions/${encodeURIComponent(syncSessionId)}/presence`,
		{
			method: "PUT",
			body: JSON.stringify(request)
		}
	);
}

/** Releases one exact browser connection's Sync presence lease. */
export function releaseInternSyncPresence(
	syncSessionId: string,
	request: InternSyncPresenceLeaseRequest,
	options: { keepalive?: boolean } = {}
): Promise<InternSyncPresenceLease> {
	return requestJson(
		`/api/smr/research-intern/sync-sessions/${encodeURIComponent(syncSessionId)}/presence/release`,
		{
			method: "POST",
			body: JSON.stringify(request),
			keepalive: options.keepalive
		}
	);
}

/** Lists durable MCP actions for one exact independent Intern runtime. */
export function listInternMcpActions(
	runtimeKind: "sync" | "async",
	runtimeId: string
): Promise<InternMcpAction[]> {
	return requestJson(
		`/api/smr/research-intern/runtimes/${runtimeKind}/${encodeURIComponent(runtimeId)}/mcp-actions?limit=500`
	);
}

/** Reads the immutable action snapshot and current application receipts. */
export function getInternSyncApproval(
	approvalId: string
): Promise<InternSyncApprovalCard> {
	return requestJson(
		`/api/smr/research-intern/sync-approvals/${encodeURIComponent(approvalId)}`
	);
}

/** Applies the authenticated operator's explicit decision to one Sync action. */
export function decideInternSyncApproval(
	approvalId: string,
	request: InternSyncApprovalDecisionRequest
): Promise<InternSyncApprovalCard> {
	return requestJson(
		`/api/smr/research-intern/sync-approvals/${encodeURIComponent(approvalId)}/decision`,
		{
			method: "POST",
			body: JSON.stringify(request)
		}
	);
}

/** Sends a durably admitted operator command to one Sync workflow. */
export function commandInternSyncSession(
	syncSessionId: string,
	request: InternSyncRuntimeCommandRequest
): Promise<InternRuntimeCommandReceipt> {
	return requestJson(
		`/api/smr/research-intern/sync-sessions/${encodeURIComponent(syncSessionId)}/commands`,
		{
			method: "POST",
			body: JSON.stringify(request)
		}
	);
}

/** Admits a failed-Visual repair request to Sync without executing it in the browser. */
export function requestInternVisualRepair(
	syncSessionId: string,
	request: InternVisualRepairRequest
): Promise<InternRuntimeCommandReceipt> {
	return requestJson(
		`/api/smr/research-intern/sync-sessions/${encodeURIComponent(syncSessionId)}/visual-repairs`,
		{
			method: "POST",
			body: JSON.stringify(request)
		}
	);
}

/** Reads owner-authored experiment history for a bound Sync project. */
export function getInternExperimentHistory(projectId: string): Promise<InternExperimentHistory> {
	return requestJson(`/api/smr/projects/${encodeURIComponent(projectId)}/experiment-bundles`);
}

/** Reads owner-authored Visuals for a bound Sync project. */
export function getInternProjectVisuals(projectId: string): Promise<InternVisualList> {
	return requestJson(`/api/smr/projects/${encodeURIComponent(projectId)}/visuals?limit=25`);
}

/** Lists typed approvals scoped to one exact Sync session. */
export function listInternSyncApprovals(syncSessionId: string): Promise<InternSyncApproval[]> {
	return requestJson(`/api/smr/research-intern/sync-sessions/${encodeURIComponent(syncSessionId)}/approvals`);
}



/** Reads immutable blocker context rehydrated by a handed-off Sync session. */
export function getInternAsyncBlockerHandoff(syncSessionId: string): Promise<InternAsyncBlockerHandoff> {
	return requestJson(`/api/smr/research-intern/sync-sessions/${encodeURIComponent(syncSessionId)}/async-blocker-handoff`);
}

/** Reads owner-authored data bindings for a bound Sync project. */
export function listInternDataBindings(projectId: string): Promise<InternDataBinding[]> {
	return requestJson(`/api/smr/projects/${encodeURIComponent(projectId)}/data-bindings`);
}

/** Reads immutable dataset revisions for one exact owner data binding. */
export function listInternDatasetRevisions(
	projectId: string,
	dataBindingId: string
): Promise<InternDatasetRevision[]> {
	return requestJson(`/api/smr/projects/${encodeURIComponent(projectId)}/data-bindings/${encodeURIComponent(dataBindingId)}/revisions`);
}

/** Reads the owner evidence inventory for an exact bound run. */
export function getInternRunEvidence(runId: string): Promise<InternRunEvidence> {
	return requestJson(`/api/smr/runs/${encodeURIComponent(runId)}/evidence`);
}

/** Reads authoritative cost and meter state for an exact bound run. */
export function getInternRunUsage(runId: string): Promise<InternRunUsage> {
	return requestJson(`/api/smr/runs/${encodeURIComponent(runId)}/usage-summary`);
}

/**
 * Lists the first-class Sync experiment ledger for a project.
 * Backend contract chain endpoint; payloads are parsed defensively
 * client-side and absence is feature-detected via 404.
 */
export function listInternProjectExperiments(projectId: string): Promise<unknown> {
	return requestJson(`/api/smr/projects/${encodeURIComponent(projectId)}/experiments?limit=100`);
}

/** Reads one Sync experiment with hypothesis, diff ref, and disposition. */
export function getInternProjectExperiment(
	projectId: string,
	experimentId: string
): Promise<unknown> {
	return requestJson(`/api/smr/projects/${encodeURIComponent(projectId)}/experiments/${encodeURIComponent(experimentId)}`);
}

/**
 * Applies an operator progress decision (promote/stop) to one scored
 * experiment. The backend action endpoints are an in-flight sibling;
 * callers feature-detect absence via 404/501.
 */
export function transitionInternExperiment(
	projectId: string,
	experimentId: string,
	transition: "promote" | "stop"
): Promise<unknown> {
	return requestJson(
		`/api/smr/projects/${encodeURIComponent(projectId)}/experiments/${encodeURIComponent(experimentId)}/transition`,
		{
			method: "POST",
			body: JSON.stringify({ transition })
		}
	);
}

/**
 * Reads container pool and live lease visibility for a bound project.
 * The endpoint ships in the backend PR chain; callers feature-detect
 * absence via ResearchInternApiError.status 404/501.
 */
export function getInternContainerVisibility(projectId: string): Promise<unknown> {
	return requestJson(`/api/smr/container-leases/visibility?project_id=${encodeURIComponent(projectId)}`);
}

/** Ensures the organization's one leave-safe Async runtime. */
export function ensureInternAsyncRuntime(request: InternAsyncRuntimeEnsureRequest): Promise<InternAsyncRuntime> {
	return requestJson(
		"/api/smr/research-intern/async",
		{
			method: "POST",
			body: JSON.stringify(request)
		}
	);
}

/** Gets the organization's singleton Async runtime without an assignment id. */
export function getInternAsyncRuntime(): Promise<InternAsyncRuntime> {
	return requestJson("/api/smr/research-intern/async");
}

/** Sends a durably admitted command to the singleton Async inbox. */
export function commandInternAsyncRuntime(request: InternRuntimeCommandRequest): Promise<InternRuntimeCommandReceipt> {
	return requestJson(
		"/api/smr/research-intern/async/commands",
		{
			method: "POST",
			body: JSON.stringify(request)
		}
	);
}

/** Unattended Async model/effort switch via spine handoff (no meta-thread id). */
export function handoffInternAsyncModel(
	request: InternAsyncHandoffModelRequest
): Promise<InternRuntimeCommandReceipt> {
	return requestJson(
		"/api/smr/research-intern/async/handoff-model",
		{
			method: "POST",
			body: JSON.stringify(request)
		}
	);
}

/** Attended seal: park a model switch at needs_review. */
export function sealInternAsyncHandoffForReview(
	request: InternAsyncHandoffReviewRequest
): Promise<InternMetaHandoff> {
	return requestJson(
		"/api/smr/research-intern/async/handoffs/review",
		{
			method: "POST",
			body: JSON.stringify(request)
		}
	);
}

/** Lists Async spine handoffs for the org singleton. */
export function listInternAsyncHandoffs(): Promise<InternMetaHandoff[]> {
	return requestJson("/api/smr/research-intern/async/handoffs");
}

/** Approves a needs_review Async handoff. */
export function approveInternAsyncHandoff(handoffId: string): Promise<InternMetaHandoff> {
	return requestJson(
		`/api/smr/research-intern/async/handoffs/${encodeURIComponent(handoffId)}/approve`,
		{ method: "POST", body: "{}" }
	);
}

/** Rejects a needs_review Async handoff. */
export function rejectInternAsyncHandoff(handoffId: string): Promise<InternMetaHandoff> {
	return requestJson(
		`/api/smr/research-intern/async/handoffs/${encodeURIComponent(handoffId)}/reject`,
		{ method: "POST", body: "{}" }
	);
}

/** Continues an approved (or needs_review) Async handoff onto a new spine segment. */
export function continueInternAsyncHandoff(
	handoffId: string,
	request: InternMetaHandoffContinueRequest = {}
): Promise<InternMetaHandoff> {
	return requestJson(
		`/api/smr/research-intern/async/handoffs/${encodeURIComponent(handoffId)}/continue`,
		{
			method: "POST",
			body: JSON.stringify(request)
		}
	);
}

/** Opens one typed Async blocker in an operator-driven Sync session. */
export function openInternAsyncBlockerInSync(
	blockerId: string,
	request: InternAsyncBlockerOpenSyncRequest
): Promise<InternAsyncBlockerOpenSyncResponse> {
	if (!blockerId.trim()) throw new TypeError("Async blocker handoff requires a blocker ID.");

	return requestJson(
		`/api/smr/research-intern/async/blockers/${encodeURIComponent(blockerId)}/open-sync`,
		{
			method: "POST",
			body: JSON.stringify(request)
		}
	);
}

/** Reads one exact Async blocker, including any durable terminal receipt. */
export function getInternAsyncBlocker(blockerId: string): Promise<InternAsyncBlocker> {
	if (!blockerId.trim()) throw new TypeError("Async blocker read requires a blocker ID.");

	return requestJson(`/api/smr/research-intern/async/blockers/${encodeURIComponent(blockerId)}`);
}

/** Resolves one reviewed blocker and atomically admits Async continuation. */
export function resolveInternAsyncBlocker(
	blockerId: string,
	request: InternAsyncBlockerResolveRequest
): Promise<InternAsyncBlockerResolveResponse> {
	if (!blockerId.trim()) throw new TypeError("Async blocker resolution requires a blocker ID.");

	return requestJson(
		`/api/smr/research-intern/async/blockers/${encodeURIComponent(blockerId)}/resolve`,
		{
			method: "POST",
			body: JSON.stringify(request)
		}
	);
}

/** Replays a bounded, ordered page from an Intern runtime ledger. */
export function listInternRuntimeEvents(
	runtimeKind: "sync" | "async",
	runtimeId: string,
	afterSequence = 0,
	limit = 200
): Promise<InternRuntimeEvent[]> {
	if (!Number.isSafeInteger(afterSequence) || afterSequence < 0) {
		throw new RangeError("Intern runtime event sequence must be non-negative.");
	}
	const parameters = new URLSearchParams({
		after_sequence: String(afterSequence),
		limit: String(limit)
	});

	return requestJson(`/api/smr/research-intern/runtimes/${runtimeKind}/${encodeURIComponent(runtimeId)}/events?${parameters.toString()}`);
}

/** Replays a bounded page from the singleton Async event ledger. */
export function listInternAsyncRuntimeEvents(
	afterSequence = 0,
	limit = 200
): Promise<InternRuntimeEvent[]> {
	const parameters = new URLSearchParams({
		after_sequence: String(afterSequence),
		limit: String(limit)
	});

	return requestJson(`/api/smr/research-intern/async/events?${parameters.toString()}`);
}

/** Opens replay-then-tail SSE from the exact retained event cursor. */
export function openInternRuntimeEventStream(
	runtimeKind: "sync" | "async",
	runtimeId: string,
	afterSequence: number,
	onEvent: (event: InternRuntimeEvent) => void,
	onError: () => void
): () => void {
	const path = runtimeKind === "async"
		? "/api/smr/research-intern/async/events/stream"
		: `/api/smr/research-intern/runtimes/sync/${encodeURIComponent(runtimeId)}/events/stream`;
	const controller = new AbortController();
	let cursor = afterSequence;
	let closed = false;

	async function tail(): Promise<void> {
		while (!closed) {
			try {
				const response = await fetch(
					`${path}?after_sequence=${encodeURIComponent(String(cursor))}`,
					{
						signal: controller.signal,
						headers: {
							Accept: "text/event-stream",
							...cursor > 0
								? { "Last-Event-ID": String(cursor) }
								: {}
						},
						cache: "no-store"
					}
				);
				if (!response.ok || !response.body) throw new Error("Intern runtime stream unavailable");
				const reader = response.body.getReader();
				const decoder = new TextDecoder();
				let buffer = "";
				while (!closed) {
					const chunk = await reader.read();
					if (chunk.done) break;
					buffer += decoder.decode(
						chunk.value,
						{ stream: true }
					)
						.replaceAll(
							"\r\n",
							"\n"
						);
					let boundary = buffer.indexOf("\n\n");
					while (boundary >= 0) {
						const frame = buffer.slice(
							0,
							boundary
						);
						buffer = buffer.slice(boundary + 2);
						const serialized = frame
							.split("\n")
							.filter((line) => line.startsWith("data:"))
							.map((line) => line.slice(5)
								.trimStart())
							.join("\n");
						if (serialized) {
							const payload: unknown = JSON.parse(serialized);
							if (
								!isRecord(payload)
								|| payload.schema_version !== "smr.intern-runtime-event-stream.v1"
								|| !isRecord(payload.event)
							) {
								throw new Error("Intern runtime stream envelope invalid");
							}
							const rawEvent = payload.event;
							if (
								typeof rawEvent.runtime_kind !== "string"
								|| typeof rawEvent.runtime_id !== "string"
								|| typeof rawEvent.event_id !== "string"
								|| typeof rawEvent.event_kind !== "string"
								|| typeof rawEvent.sequence !== "number"
								|| !Number.isSafeInteger(rawEvent.sequence)
								|| rawEvent.sequence < 1
							) {
								throw new Error("Intern runtime stream event invalid");
							}
							const event = rawEvent as InternRuntimeEvent;
							if (
								event.runtime_kind !== runtimeKind
								|| event.runtime_id !== runtimeId
							) {
								throw new Error("Intern runtime stream cursor invalid");
							}

							/*
							 * Duplicates are ignored. Gaps are forwarded so callers can
							 * backfill from PostgreSQL instead of tearing down the stream.
							 */
							if (event.sequence <= cursor) {
								boundary = buffer.indexOf("\n\n");
								continue;
							}
							cursor = event.sequence;
							onEvent(event);
						}
						boundary = buffer.indexOf("\n\n");
					}
				}
			} catch {
				if (!closed) onError();
			}
			if (!closed) await new Promise((resolve) => setTimeout(
				resolve,
				750
			));
		}
	}

	void tail();

	return () => {
		closed = true;
		controller.abort();
	};
}

/** Fetches all organization Factories. */
export function listResearchFactories(): Promise<ResearchFactory[]> {
	return requestJson("/api/smr/factories");
}

/** Fetches Factory memberships owned by the canonical Intern. */
export function listResearchInternFactories(): Promise<ResearchInternFactoryMembership[]> {
	return requestJson("/api/smr/research-intern/factories");
}

/** Attaches one Factory to the canonical Intern. */
export function attachResearchInternFactory(factoryId: string): Promise<ResearchInternFactoryMembership> {
	return requestJson(
		`/api/smr/research-intern/factories/${encodeURIComponent(factoryId)}`,
		{
			method: "POST"
		}
	);
}

/** Lists active and historical projects for a Factory. */
export function listResearchFactoryProjects(factoryId: string): Promise<ResearchFactoryProject[]> {
	return requestJson(`/api/smr/factories/${encodeURIComponent(factoryId)}/projects?include_archived=false`);
}

/** Lists the objective-bearing efforts for a Factory. */
export function listResearchFactoryEfforts(factoryId: string): Promise<ResearchFactoryEffort[]> {
	return requestJson(`/api/smr/factories/${encodeURIComponent(factoryId)}/efforts`);
}

/** Fetches the canonical lifecycle projection for an exact Factory run. */
export function getResearchInternRun(
	projectId: string,
	runId: string
): Promise<ResearchInternRun> {
	return requestJson(`/api/smr/projects/${encodeURIComponent(projectId)}/runs/${encodeURIComponent(runId)}`);
}

/** Lists canonical project runs using the generated backend run contract. */
export function listResearchInternProjectRuns(projectId: string): Promise<ResearchInternRunSummary[]> {
	return requestJson(`/api/smr/projects/${encodeURIComponent(projectId)}/runs`);
}

/** Fetches canonical actor liveness for an exact Factory run. */
export function listResearchInternRunActors(
	projectId: string,
	runId: string
): Promise<ResearchInternRunActors> {
	return requestJson(`/api/smr/projects/${encodeURIComponent(projectId)}/runs/${encodeURIComponent(runId)}/actors`);
}

/** Lists durable role receipts for a Factory. */
export function listResearchInternRoleReceipts(factoryId: string): Promise<ResearchInternRoleReceipt[]> {
	return requestJson(`/api/smr/research-intern/factories/${encodeURIComponent(factoryId)}/role-receipts`);
}

/** Loads recent durable Factory progress events. */
export async function listResearchFactoryEvents(factoryId: string): Promise<ResearchFactoryEvent[]> {
	const page = await requestJson<{
		events?: ResearchFactoryEvent[];
	}>(`/api/smr/factories/${encodeURIComponent(factoryId)}/events?limit=200`);

	return page.events ?? [];
}

/** Loads current Factory spend and budget status. */
export function getResearchFactoryUsage(factoryId: string): Promise<ResearchFactoryUsage> {
	return requestJson(`/api/smr/factories/${encodeURIComponent(factoryId)}/usage?window=month_to_date`);
}

/** Loads candidate/grader projections for a Factory. */
export function listResearchFactoryCandidates(factoryId: string): Promise<ResearchFactoryCandidate[]> {
	return requestJson(`/api/smr/factories/${encodeURIComponent(factoryId)}/candidates?limit=25`);
}

/** Loads user-facing evidence and Result Visual projections for a Factory. */
export function listResearchFactoryResults(factoryId: string): Promise<ResearchFactoryResult[]> {
	return requestJson(`/api/smr/factories/${encodeURIComponent(factoryId)}/results?limit=25`);
}

/*
 * ─────────────────────────────────────────────────────────────
 * WP6 · Effort program board (Async Intern primary surface)
 *
 * Every shape below is the generated backend contract
 * (`smr.intern-effort-board.v1` / `smr.intern-effort-detail.v1`).
 * Two vocabularies stay local because the generated schema widens
 * them to `string`: see `InternEffortStatus` / `InternEffortType`.
 * ─────────────────────────────────────────────────────────────
 */

/**
 * Effort lifecycle vocabulary owned by SMR (`smr_efforts.status`).
 * `InternEffortRef.status` is generated as a bare `string`, so this
 * union is the FE's filter vocabulary, not a decode guarantee.
 */
export type InternEffortStatus
	= | "active"
	  | "paused"
	  | "waiting"
	  | "blocked"
	  | "ready_for_review"
	  | "archived_reference";

/** Effort program kind owned by SMR; also generated as a bare `string`. */
export type InternEffortType = "research" | "eval_factory" | "optimizer";

/** Which of the three Effort board segments an Effort belongs in. */
export type InternEffortSegment = "active" | "blocked" | "done";

/**
 * The `smr_efforts` projection carried by every Effort-shaped payload.
 * This is the nested `effort` block of the detail rollup — deliberately
 * narrower than `InternEffortSummary`, which adds the board counters.
 */
export type InternEffortRef = components["schemas"]["InternEffortRef"];

/** One row of the Effort board list: an `InternEffortRef` plus counters. */
export type InternEffortSummary = components["schemas"]["InternEffortSummary"];

/** `GET /smr/research-intern/efforts` — the paged Effort list surface. */
export type InternEffortBoard = components["schemas"]["InternEffortBoardResponse"];

/** Intern objective discriminator; both kinds share one row adapter. */
export type InternObjectiveKind = components["schemas"]["InternObjectiveKind"];

/** Intern objective status vocabulary (`ParentObjectiveStatus`). */
export type InternObjectiveStatus = components["schemas"]["InternObjectiveStatus"];

/** Intern objective evaluation vocabulary. */
export type InternObjectiveEvaluationState
	= components["schemas"]["InternObjectiveEvaluationState"];

/** Intern milestone lifecycle vocabulary (`MilestoneLifecycleState`). */
export type InternMilestoneState = components["schemas"]["InternMilestoneState"];

/** Subquestion / suboutcome milestone discriminator. */
export type InternMilestoneKind = components["schemas"]["InternMilestoneKind"];

/** Intern planner checklist state vocabulary (`TaskLifecycleState`). */
export type InternEffortTaskState = components["schemas"]["InternEffortTaskState"];

/** Reference role an Intern objective link carries (`ObjectiveRunScopeRole`). */
export type InternObjectiveLinkRole = components["schemas"]["InternObjectiveLinkRole"];

/** Reference target kinds an Intern objective may point at. */
export type InternObjectiveLinkKind = components["schemas"]["InternObjectiveLinkKind"];

/** One Intern objective as rolled up on the Progress tab. */
export type InternEffortObjectiveProgress
	= components["schemas"]["InternEffortObjectiveProgress"];

/** One Intern milestone as rolled up on the Progress tab. */
export type InternEffortMilestoneProgress
	= components["schemas"]["InternEffortMilestoneProgress"];

/** One Intern planner task as rolled up on the Progress tab. */
export type InternEffortTaskProgress = components["schemas"]["InternEffortTaskProgress"];

/** One append-only Intern progress claim as rolled up on the Progress tab. */
export type InternEffortClaimProgress = components["schemas"]["InternEffortClaimProgress"];

/** One parked ask-and-continue question scoped to this Effort (WP-AC). */
export type InternEffortOpenQuestion = components["schemas"]["InternEffortOpenQuestion"];

/** Progress tab body: objectives, milestones, claims, open questions. */
export type InternEffortProgressRollup
	= components["schemas"]["InternEffortProgressRollup"];

/** One work product produced under this Effort's runs. */
export type InternEffortWorkProduct = components["schemas"]["InternEffortWorkProduct"];

/** One Intern work summary. Scoped to the Intern, not to the Effort. */
export type InternEffortWorkSummary = components["schemas"]["InternEffortWorkSummary"];

/** Results tab body: work products, summaries, and the latest report. */
export type InternEffortResultsRollup = components["schemas"]["InternEffortResultsRollup"];

/** Experiments tab body: SMR-owned runs and experiments, read-only. */
export type InternEffortExperimentsRollup
	= components["schemas"]["InternEffortExperimentsRollup"];

/** Knowledge tab body: memory hits, experiment log, evidence refs. */
export type InternEffortKnowledgeRollup
	= components["schemas"]["InternEffortKnowledgeRollup"];

/** Secondary ops projection; a Postgres projection, never Temporal history. */
export type InternEffortRuntimeStrip = components["schemas"]["InternEffortRuntimeStrip"];

/** Day/month spend against the Async ceilings. */
export type InternEffortSpend = components["schemas"]["InternEffortSpend"];

/** Sticky exe.dev host lease state for the Async runtime. */
export type InternEffortStickyHost = components["schemas"]["InternEffortStickyHost"];

/** `GET /smr/research-intern/efforts/{effort_id}` — the four-tab rollup. */
export type InternEffortDetail = components["schemas"]["InternEffortDetailResponse"];

/** One Intern objective row; every row carries a non-null `effort_id`. */
export type InternObjective = components["schemas"]["InternObjectiveResponse"];

/** Effort-bound Intern objective create body; the path carries `effort_id`. */
export type InternObjectiveCreateRequest
	= components["schemas"]["InternObjectiveCreateRequest"];

/** One Intern milestone row under an Effort. */
export type InternMilestone = components["schemas"]["InternMilestoneResponse"];

/** Effort-bound Intern milestone create body. */
export type InternMilestoneCreateRequest
	= components["schemas"]["InternMilestoneCreateRequest"];

/** One Intern planner task; this is not an `smr_run_tasks` row. */
export type InternEffortTask = components["schemas"]["InternEffortTaskResponse"];

/** Effort-bound Intern planner task create body. */
export type InternEffortTaskCreateRequest
	= components["schemas"]["InternEffortTaskCreateRequest"];

/**
 * Names the provenance of a rolled-up run or work product.
 *
 * `effort_binding` means this Effort's own work produced it;
 * `intern_objective_link` means the Intern only *cited* it from an objective.
 * The board must never let the second read as the first — an Effort that
 * folded in someone else's artifact did not schedule it.
 */
export function internLinkSourceLabel(linkSource: string): string {
	if (linkSource === "effort_binding") return "Ran under this Effort";
	if (linkSource === "intern_objective_link") return "Cited by an objective";

	return linkSource.replaceAll(
		"_",
		" "
	);
}

/**
 * Resolves a browser-openable href for one rolled-up work product.
 *
 * The rollup reports a **backend-relative** path
 * (`/smr/work-products/{id}/content`). The browser can only reach the Next
 * proxy at `/api/smr/...`, so rendering the raw value would produce a link
 * into the frontend's own route table and 404. Anything that is neither a
 * known backend path nor an absolute URL resolves to `null` so the caller
 * renders "no openable content" instead of a dead link.
 */
export function internWorkProductHref(product: InternEffortWorkProduct): string | null {
	const url = product.url?.trim();
	if (!url) return null;
	if (url.startsWith("https://") || url.startsWith("http://")) return url;
	if (url.startsWith("/api/smr/")) return url;
	if (url.startsWith("/smr/")) return `/api${url}`;

	return null;
}

/**
 * Segments one Effort for the board. `blocked` is the rollup's own
 * ask-and-continue signal, so a blocked Effort never hides in `active`
 * and a terminal Effort never keeps a live-work segment warm.
 */
export function internEffortSegment(effort: InternEffortSummary): InternEffortSegment {
	if (effort.status === "archived_reference") return "done";
	if (effort.blocked || effort.status === "blocked") return "blocked";

	return "active";
}

function searchParameters(entries: Record<string, string | number | undefined>): string {
	const parameters = new URLSearchParams();
	for (const [key, value] of Object.entries(entries)) {
		if (value === undefined || value === "") continue;
		parameters.set(
			key,
			String(value)
		);
	}
	const serialized = parameters.toString();

	return serialized
		? `?${serialized}`
		: "";
}

/** Lists the Effort board, optionally narrowed to one Factory or status. */
export function listInternEfforts(options: {
	factoryId?: string;
	status?: InternEffortStatus;
	limit?: number;
	cursor?: string;
} = {}): Promise<InternEffortBoard> {
	return requestJson(`/api/smr/research-intern/efforts${searchParameters({
		factory_id: options.factoryId,
		status: options.status,
		limit: options.limit,
		cursor: options.cursor
	})}`);
}

/** Reads the Postgres-only four-tab rollup for one exact Effort. */
export function getInternEffortDetail(effortId: string): Promise<InternEffortDetail> {
	if (!effortId.trim()) throw new TypeError("Effort detail requires an Effort ID.");

	return requestJson(`/api/smr/research-intern/efforts/${encodeURIComponent(effortId)}`);
}

/** Lists the Intern objectives bound to one Effort. */
export function listInternEffortObjectives(
	effortId: string,
	options: {
		status?: InternObjectiveStatus;
		limit?: number;
	} = {}
): Promise<InternObjective[]> {
	return requestJson(`/api/smr/research-intern/efforts/${encodeURIComponent(effortId)}/objectives${searchParameters({
		status: options.status,
		limit: options.limit
	})}`);
}

/** Creates one Intern objective under an Effort; the path is the binding. */
export function createInternEffortObjective(
	effortId: string,
	request: InternObjectiveCreateRequest
): Promise<InternObjective> {
	return requestJson(
		`/api/smr/research-intern/efforts/${encodeURIComponent(effortId)}/objectives`,
		{
			method: "POST",
			body: JSON.stringify(request)
		}
	);
}

/** Lists the Intern milestones bound to one Effort. */
export function listInternEffortMilestones(
	effortId: string,
	options: {
		state?: InternMilestoneState;
		limit?: number;
	} = {}
): Promise<InternMilestone[]> {
	return requestJson(`/api/smr/research-intern/efforts/${encodeURIComponent(effortId)}/milestones${searchParameters({
		state: options.state,
		limit: options.limit
	})}`);
}

/** Creates one Intern milestone under an Effort. */
export function createInternEffortMilestone(
	effortId: string,
	request: InternMilestoneCreateRequest
): Promise<InternMilestone> {
	return requestJson(
		`/api/smr/research-intern/efforts/${encodeURIComponent(effortId)}/milestones`,
		{
			method: "POST",
			body: JSON.stringify(request)
		}
	);
}

/** Lists the Intern planner checklist bound to one Effort. */
export function listInternEffortTasks(
	effortId: string,
	options: {
		state?: InternEffortTaskState;
		milestoneId?: string;
		limit?: number;
	} = {}
): Promise<InternEffortTask[]> {
	return requestJson(`/api/smr/research-intern/efforts/${encodeURIComponent(effortId)}/tasks${searchParameters({
		state: options.state,
		milestone_id: options.milestoneId,
		limit: options.limit
	})}`);
}

/** Creates one Intern planner task under an Effort. */
export function createInternEffortTask(
	effortId: string,
	request: InternEffortTaskCreateRequest
): Promise<InternEffortTask> {
	return requestJson(
		`/api/smr/research-intern/efforts/${encodeURIComponent(effortId)}/tasks`,
		{
			method: "POST",
			body: JSON.stringify(request)
		}
	);
}
