import type { ProgressAgreement } from "./runProgress/project";
import type { RunRecord } from "./runProgress/subscription";

export type VisualEvidenceFailure = {
	seed?: number;
	rolloutId?: string;
	trialId?: string;
	code: string;
	sequence?: number;
	detail: string;
};

export type VisualRunLifecycle = {
	status: string;
	terminal: boolean;
	failed: boolean;
	reason?: string;
	work: { planned?: number; failed?: number; succeeded?: number };
	evidence: {
		state: "pending" | "accepted" | "partial" | "missing" | "rejected";
		valid: number;
		rejected: number;
		missing: number;
		sealedTraces: number;
		failures: VisualEvidenceFailure[];
	};
	usage: {
		calls?: number;
		costUsd?: number;
		costCapUsd?: number;
		costSource: "workshop_proxy" | "provider" | "container" | "unavailable";
		provider?: string;
	};
};

function record(value: unknown): Record<string, unknown> {
	return value && typeof value === "object" && !Array.isArray(value)
		? value as Record<string, unknown>
		: {};
}

function rows(value: unknown): Record<string, unknown>[] {
	return Array.isArray(value) ? value.map(record) : [];
}

function finite(value: unknown): number | undefined {
	return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function text(value: unknown): string | undefined {
	return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function terminalStatus(status: string): boolean {
	return ["completed", "failed", "failed_evidence", "cancelled", "degraded", "interrupted", "infrastructure_lost", "cap_reached"].includes(status);
}

function failureCode(detail: string): string {
	if (/journal event digest mismatch/i.test(detail)) return "journal_digest_mismatch";
	if (/integrity/i.test(detail)) return "integrity_validation_failed";
	if (/evidence/i.test(detail)) return "evidence_unusable";
	return "rollout_failed";
}

function isRejectedEvidence(row: Record<string, unknown>, detail: string): boolean {
	const explicit = text(row.evidenceState)?.toLowerCase();
	const outcome = record(row.evidenceOutcome);
	const reason = text(outcome.reason)?.toLowerCase() ?? "";
	return explicit === "rejected"
		|| /digest mismatch|integrity validation|evidence rejected|unusable evidence/i.test(detail)
		|| /rejected|integrity|unusable/.test(reason);
}

/**
 * Translate the backend-owned optimizer record into the small lifecycle
 * vocabulary visual templates consume. Transport state is deliberately absent:
 * an open rollout socket cannot overrule a terminal optimizer journal.
 */
export function projectVisualRunLifecycle(
	run: RunRecord | null | undefined,
	progress?: ProgressAgreement | null
): VisualRunLifecycle | undefined {
	if (!run) return undefined;
	const summary = record(run.summary);
	const manifest = record(summary.terminalManifest);
	const terminal = record(manifest.terminal);
	const manifestEvidence = record(manifest.evidence);
	const manifestUsage = record(manifest.usage);
	const runUsage = record(run.usage);
	const usageExtra = record(runUsage.extra);
	const receipt = record(manifestUsage.providerReceipt ?? usageExtra.providerUsageReceipt);
	const receiptCapabilities = rows(receipt.capabilities);
	const credentialChain = record(summary.credentialChain);
	const bounds = record(summary.bounds);
	const work = record(manifest.work);
	const lifecycleStatus = text(terminal.kind) ?? progress?.status ?? text(run.status) ?? "unknown";
	const isTerminal = text(summary.progress && record(summary.progress).authoritative
		? record(record(summary.progress).authoritative).runState
		: undefined) === "terminal"
		|| progress?.terminal === true
		|| terminalStatus(lifecycleStatus)
		|| terminalStatus(run.status);
	const failed = ["failed", "failed_evidence", "degraded", "interrupted", "infrastructure_lost", "cap_reached"].includes(lifecycleStatus)
		|| ["failed", "failed_evidence", "degraded", "interrupted", "infrastructure_lost", "cap_reached"].includes(run.status);

	const failures = rows(summary.records).flatMap((item): VisualEvidenceFailure[] => {
		const detail = text(item.error)
			?? text(record(item.evidenceOutcome).detail)
			?? text(record(item.evaluatorOutcome).detail)
			?? "Rollout evidence was not retained.";
		if (!isRejectedEvidence(item, detail)) return [];
		const sequenceMatch = detail.match(/sequence\s+(\d+)/i);
		const seed = finite(item.seed);
		const rolloutId = text(item.rolloutId);
		const trialId = text(item.trialId);
		return [{
			...(seed != null ? { seed } : {}),
			...(rolloutId ? { rolloutId } : {}),
			...(trialId ? { trialId } : {}),
			code: failureCode(detail),
			...(sequenceMatch ? { sequence: Number(sequenceMatch[1]) } : {}),
			detail
		}];
	});
	const ledger = rows(manifest.evidenceLedger);
	const ledgerCounts = ledger.reduce<{ valid: number; missing: number }>((counts, item) => {
		const state = text(item.state)?.toLowerCase();
		if (state === "complete") counts.valid += 1;
		else if (state === "missing") counts.missing += 1;
		return counts;
	}, { valid: 0, missing: 0 });
	const sealedTraces = rows(summary.records).filter((item) => {
		const sealed = record(item.sealedTrace);
		return sealed.imported === true || rows(sealed.traces).length > 0;
	}).length || rows(manifestEvidence.refs).length;
	const rejected = failures.length;
	const valid = ledgerCounts.valid;
	const missing = Math.max(0, ledgerCounts.missing - rejected);
	const evidenceState: VisualRunLifecycle["evidence"]["state"] = rejected > 0
		? "rejected"
		: !isTerminal
			? "pending"
			: valid > 0 && missing === 0
				? "accepted"
				: valid > 0
					? "partial"
					: "missing";
	const receiptAuthority = text(receipt.authority);
	const costUsd = finite(receipt.costUsd) ?? finite(manifestUsage.costUsd) ?? finite(runUsage.costUsd);
	const costSource = receiptAuthority === "workshop.secrets_proxy"
		? "workshop_proxy" as const
		: costUsd == null
			? "unavailable" as const
			: receiptAuthority
				? "provider" as const
				: "container" as const;

	return {
		status: lifecycleStatus,
		terminal: isTerminal,
		failed,
		reason: text(record(manifest.error).message)
			?? text(record(run.error).message)
			?? text(manifestEvidence.reason),
		work: {
			planned: finite(work.planned),
			failed: finite(work.failed),
			succeeded: finite(work.succeeded)
		},
		evidence: { state: evidenceState, valid, rejected, missing, sealedTraces, failures },
		usage: {
			calls: finite(receipt.calls) ?? finite(manifestUsage.calls) ?? finite(runUsage.calls),
			costUsd,
			costCapUsd: finite(bounds.hardTotalCostUsd) ?? finite(summary.costCeilingUsd),
			costSource,
			provider: text(receiptCapabilities[0]?.provider) ?? text(credentialChain.provider)
		}
	};
}
