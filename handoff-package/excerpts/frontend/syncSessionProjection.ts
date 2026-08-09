/**
 * Defensive readers over the optional Sync product-layer projection
 * fields (task template, provisioning receipt, …).
 *
 * The backing backend contract chain is landing separately, and every
 * field is optional on the session projection — these readers return
 * null/empty for old backends so the UI degrades gracefully and lights
 * up when the fields appear.
 */

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

function asString(value: unknown): string | null {
	return typeof value === "string" && value ? value : null;
}

function asNumber(value: unknown): number | null {
	return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function recordArray(value: unknown): Record<string, unknown>[] {
	return Array.isArray(value) ? value.filter(isRecord) : [];
}

export type SyncTemplateProvisionedResource = {
	resource_kind: string;
	status: "created" | "reused" | "unsupported";
	resource_id: string | null;
	error_code: string | null;
};

export type SyncTemplateProvisioningReceipt = {
	task_template: string;
	resources: SyncTemplateProvisionedResource[];
	provisioned_at: string | null;
};

/** The task template a session was started from, if any. */
export function sessionTaskTemplate(session: object): string | null {
	return asString((session as Record<string, unknown>).task_template);
}

/** The durable template provisioning receipt, if the session has one. */
export function sessionProvisioning(session: object): SyncTemplateProvisioningReceipt | null {
	const raw = (session as Record<string, unknown>).provisioning;
	if (!isRecord(raw)) return null;
	const taskTemplate = asString(raw.task_template);
	if (!taskTemplate) return null;
	const resources = Array.isArray(raw.resources)
		? raw.resources.filter(isRecord)
			.flatMap((resource): SyncTemplateProvisionedResource[] => {
				const kind = asString(resource.resource_kind);
				const status = asString(resource.status);
				if (!kind || !status) return [];
				if (status !== "created" && status !== "reused" && status !== "unsupported") {
					return [];
				}

				return [{
					resource_kind: kind,
					status,
					resource_id: asString(resource.resource_id),
					error_code: asString(resource.error_code)
				}];
			})
		: [];

	return {
		task_template: taskTemplate,
		resources,
		provisioned_at: asString(raw.provisioned_at)
	};
}

export type SyncTracePublication = {
	status: "published" | "failed";
	trace_id: string | null;
	trace_digest: string | null;
	publication_id: string | null;
	error_code: string | null;
	published_at: string | null;
};

export type SyncSessionVisualSummary = {
	visual_id: string;
	kind: "score_series" | "visual";
	title: string;
	current_revision: number | null;
	series_point_count: number | null;
	updated_at: string | null;
};

export type SyncExperimentScores = {
	baseline: number | null;
	result: number | null;
	delta: number | null;
};

export const SYNC_EXPERIMENT_STATUSES = [
	"proposed",
	"approved",
	"rejected",
	"superseded",
	"running",
	"scored",
	"promoted",
	"stopped"
] as const;

export type SyncExperimentStatus = (typeof SYNC_EXPERIMENT_STATUSES)[number];

export type SyncExperimentSummary = {
	experiment_id: string;
	title: string;
	kind: "baseline" | "experiment";
	status: SyncExperimentStatus;
	scores: SyncExperimentScores | null;
	updated_at: string | null;
};

/** Parses one score summary; tolerates absent or partial objects. */
export function parseExperimentScores(value: unknown): SyncExperimentScores | null {
	if (!isRecord(value)) return null;

	return {
		baseline: asNumber(value.baseline),
		result: asNumber(value.result),
		delta: asNumber(value.delta)
	};
}

/** Terminal Trace V5 publication receipt, observed post-close. */
export function sessionTracePublication(session: object): SyncTracePublication | null {
	const raw = (session as Record<string, unknown>).trace_publication;
	if (!isRecord(raw)) return null;
	const status = asString(raw.status);
	if (status !== "published" && status !== "failed") return null;

	return {
		status,
		trace_id: asString(raw.trace_id),
		trace_digest: asString(raw.trace_digest),
		publication_id: asString(raw.publication_id),
		error_code: asString(raw.error_code),
		published_at: asString(raw.published_at)
	};
}

/** Session-scoped visual summaries (score series and plain visuals). */
export function sessionVisuals(session: object): SyncSessionVisualSummary[] {
	return recordArray((session as Record<string, unknown>).visuals)
		.flatMap((raw): SyncSessionVisualSummary[] => {
			const visualId = asString(raw.visual_id);
			const kind = asString(raw.kind);
			if (!visualId || (kind !== "score_series" && kind !== "visual")) {
				return [];
			}

			return [{
				visual_id: visualId,
				kind,
				title: asString(raw.title) ?? visualId,
				current_revision: asNumber(raw.current_revision),
				series_point_count: asNumber(raw.series_point_count),
				updated_at: asString(raw.updated_at)
			}];
		});
}

/** Shortens a sha256 digest for display while keeping its prefix. */
export function shortDigest(digest: string | null): string | null {
	if (!digest) return null;
	const [algorithm, hex] = digest.split(":");
	if (!hex) return digest.length > 16 ? `${digest.slice(0, 16)}…` : digest;

	return `${algorithm}:${hex.slice(0, 12)}…`;
}

/** Experiment ledger summaries projected onto the session. */
export function sessionExperiments(session: object): SyncExperimentSummary[] {
	return recordArray((session as Record<string, unknown>).experiments)
		.flatMap((raw): SyncExperimentSummary[] => {
			const experimentId = asString(raw.experiment_id);
			const kind = asString(raw.kind);
			const status = asString(raw.status);
			if (
				!experimentId
				|| (kind !== "baseline" && kind !== "experiment")
				|| !SYNC_EXPERIMENT_STATUSES.some((known) => known === status)
			) return [];

			return [{
				experiment_id: experimentId,
				title: asString(raw.title) ?? experimentId,
				kind,
				status: status as SyncExperimentStatus,
				scores: parseExperimentScores(raw.scores),
				updated_at: asString(raw.updated_at)
			}];
		});
}
