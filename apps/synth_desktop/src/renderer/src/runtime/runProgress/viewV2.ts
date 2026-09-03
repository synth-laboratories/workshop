import type {
	OptimizerRunHeader,
	OptimizerRunViewV2,
	UsageCompleteness
} from "../../generated/protocol";
import type { RunRecord } from "./subscription";
import { primaryVisualRef } from "./adapterShared";
import {
	RUN_PROGRESS_SCHEMA_VERSION,
	type CoveredMetric,
	type RunKind,
	type RunProgressEvidence,
	type RunProgressPhase,
	type RunProgressProjection,
	type RunProgressResult,
	type RunProgressStatus
} from "./types";

function label(value: string): string {
	return value
		.split("_")
		.map((part) => part ? `${part[0]!.toUpperCase()}${part.slice(1)}` : part)
		.join(" ");
}

function algorithmLabel(kind: Exclude<RunKind, "environment">): string {
	if (kind === "go-ex") return "GELO";
	if (kind === "gepa") return "GEPA";
	if (kind === "sft") return "SFT";
	if (kind === "cispo") return "CISPO";
	return "Evaluation";
}

function statusOf(header: OptimizerRunHeader): RunProgressStatus {
	if (header.lifecycle === "terminal") return header.terminal?.kind ?? "unknown";
	if (header.lifecycle === "queued" || header.lifecycle === "starting") return "queued";
	if (header.lifecycle === "paused") return "paused";
	return "running";
}

function phaseOf(header: OptimizerRunHeader, status: RunProgressStatus): RunProgressPhase {
	const id = header.phase ?? header.lifecycle;
	const terminal = header.lifecycle === "terminal";
	return {
		id,
		label: terminal ? "Finished" : label(id),
		status: terminal
			? status === "failed" ? "failed" : "completed"
			: header.lifecycle === "queued" ? "pending" : "active",
		...(header.condition !== "healthy" ? { detail: label(header.condition) } : {})
	};
}

function metric(value: number | null | undefined, source: CoveredMetric["source"]): CoveredMetric {
	return value == null
		? { source: "unavailable", observedUnits: 0 }
		: { value, source, observedUnits: 1 };
}

function finite(value: unknown): number | undefined {
	return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function usageOf(
	usage: UsageCompleteness,
	reportedTerminalWork: number | undefined,
	run: RunRecord
): RunProgressProjection["usage"] {
	const extra = run.usage?.extra && typeof run.usage.extra === "object"
		? run.usage.extra as Record<string, unknown>
		: {};
	const summary = run.summary ?? {};
	const manifest = summary.terminalManifest && typeof summary.terminalManifest === "object"
		? summary.terminalManifest as Record<string, unknown>
		: {};
	const manifestUsage = manifest.usage && typeof manifest.usage === "object"
		? manifest.usage as Record<string, unknown>
		: {};
	const receipt = manifestUsage.providerReceipt ?? extra.providerUsageReceipt;
	const receiptRecord = receipt && typeof receipt === "object" ? receipt as Record<string, unknown> : undefined;
	const receiptAuthority = receiptRecord
		? receiptRecord.authority
		: undefined;
	const usageSource: CoveredMetric["source"] = receiptAuthority === "workshop.secrets_proxy" ? "proxy" : "provider";
	const receiptCalls = typeof receiptRecord?.calls === "number" && Number.isFinite(receiptRecord.calls)
		? receiptRecord.calls
		: undefined;
	// V2 projections may briefly omit usage while a newer partial reducer page
	// lands. The run record and proxy receipt are cumulative durable facts, so a
	// missing field must not erase a previously known subtotal from the card.
	const costUsd = metric(
		finite(usage.costUsd) ?? finite(receiptRecord?.costUsd) ?? finite(run.usage?.costUsd),
		usageSource
	);
	const promptTokens = metric(
		finite(usage.promptTokens) ?? finite(receiptRecord?.promptTokens) ?? finite(run.usage?.promptTokens),
		usageSource
	);
	const completionTokens = metric(
		finite(usage.completionTokens) ?? finite(receiptRecord?.completionTokens) ?? finite(run.usage?.completionTokens),
		usageSource
	);
	const withReceipt = (value: CoveredMetric): CoveredMetric => receiptCalls == null
		? value
		: { ...value, receiptCalls };
	return {
		costUsd: withReceipt(costUsd),
		promptTokens: withReceipt(promptTokens),
		completionTokens: withReceipt(completionTokens),
		rollouts: metric(reportedTerminalWork, "derived")
	};
}

function evidenceOf(header: OptimizerRunHeader): RunProgressEvidence {
	const reason = header.evidence.reason ?? undefined;
	if (header.evidence.completeness === "unusable") {
		return { state: "degraded", reason: reason ?? "run evidence is unusable" };
	}
	if (header.evidence.completeness === "absent") {
		return { state: "unavailable", reason: reason ?? "no run evidence was reported" };
	}
	if (header.evidence.completeness === "partial" && header.lifecycle === "terminal") {
		return { state: "degraded", reason: reason ?? "terminal evidence is incomplete" };
	}
	return { state: "present" };
}

function resultOf(view: OptimizerRunViewV2): RunProgressResult | undefined {
	const partial = view.header.evidence.completeness !== "complete";
	if (!view.result) return undefined;
	switch (view.algorithm) {
		case "eval":
			return view.result.meanReward != null
				? {
					headline: `Mean reward ${view.result.meanReward.toFixed(3)}`,
					detail: view.result.selection.replaceAll("_", " "),
					partial
				}
				: {
					absentReason: view.result.selection === "promotion_not_applicable"
						? "baseline evaluation; promotion is not applicable"
						: "no aggregate reward was reported",
					partial
				};
		case "gepa":
			return {
				...(view.result.selectedCandidateId
					? { headline: `Selected ${view.result.selectedCandidateId}` }
					: { absentReason: view.result.verdict.replaceAll("_", " ") }),
				verdict: view.result.verdict,
				detail: `${view.result.candidates} candidates`,
				partial
			};
		case "go-ex":
			return {
				...(view.result.selectedCandidateId
					? { headline: `Selected ${view.result.selectedCandidateId}` }
					: { absentReason: "no GO-EX candidate was selected" }),
				detail: `${view.result.candidates} candidates · ${view.result.themes} themes`,
				partial
			};
		case "sft":
			return {
				...(view.result.producedAdapter
					? { headline: `Adapter ${view.result.producedAdapter}` }
					: view.result.selectedCheckpointId
						? { headline: `Checkpoint ${view.result.selectedCheckpointId}` }
						: { absentReason: "no model artifact was reported" }),
				...(view.result.trainLoss != null ? { detail: `Train loss ${view.result.trainLoss.toFixed(4)}` } : {}),
				partial
			};
		case "cispo":
			return {
				...(view.result.policyCheckpointId
					? { headline: `Policy ${view.result.policyCheckpointId}` }
					: { absentReason: view.result.noLearningSignal
						? "no learning signal was measured"
						: "no policy checkpoint was reported" }),
				...(view.result.meanAdvantage != null
					? { detail: `Mean advantage ${view.result.meanAdvantage.toFixed(4)}` }
					: {}),
				partial
			};
	}
}

function elapsedMs(run: RunRecord, header: OptimizerRunHeader, now: number): number | undefined {
	const started = Date.parse(run.startedAt ?? run.createdAt ?? "");
	if (!Number.isFinite(started)) return undefined;
	const terminalAt = Date.parse(header.terminal?.sealedAt ?? run.finishedAt ?? "");
	const end = header.lifecycle === "terminal" && Number.isFinite(terminalAt) ? terminalAt : now;
	return Math.max(0, end - started);
}

/** Map the backend-owned run projection into the presentation-only chat shape. */
export function projectRunViewV2(
	view: OptimizerRunViewV2,
	run: RunRecord,
	now: number
): RunProgressProjection {
	const header = view.header;
	if (
		header.runId !== run.id
		|| header.algorithm !== view.algorithm
		|| run.algorithmId !== view.algorithm
	) {
		throw new Error("optimizer V2 view identity does not match the bound run");
	}
	const kind = view.algorithm;
	const status = statusOf(header);
	const terminal = header.lifecycle === "terminal";
	const values = [header.work.succeeded, header.work.failed, header.work.cancelled]
		.filter((value): value is number => value != null);
	const terminalWork = values.length > 0
		? values.reduce((total, value) => total + value, 0)
		: undefined;
	const determinate = header.work.fixedDenominator === true
		&& header.work.planned != null
		&& header.work.planned > 0;
	const phase = phaseOf(header, status);
	const evidence = evidenceOf(header);
	const warnings = [
		...(header.condition !== "healthy" ? [`Execution condition: ${label(header.condition)}`] : []),
		...(evidence.state !== "present" && evidence.reason ? [evidence.reason] : [])
	];
	const milestone = terminal && header.terminal
		? {
			label: `${algorithmLabel(kind)} ${header.terminal.kind}`,
			occurredAt: header.terminal.sealedAt,
			sequence: header.terminal.finalSequence
		}
		: undefined;
	const fullVisualRef = primaryVisualRef(run, header.visualRefs);
	return {
		schemaVersion: RUN_PROGRESS_SCHEMA_VERSION,
		runId: header.runId,
		runKind: kind,
		title: `${algorithmLabel(kind)} · ${run.objective || header.runId}`,
		status,
		terminal,
		phase,
		phases: [phase],
		work: {
			...(terminalWork != null ? { completed: terminalWork } : {}),
			...(header.work.running != null ? { active: header.work.running } : {}),
			...(header.work.queued != null ? { queued: header.work.queued } : {}),
			...(header.work.failed != null ? { failed: header.work.failed } : {}),
			...(determinate ? { total: header.work.planned } : {}),
			...(header.work.unit ? { unit: header.work.unit } : {})
		},
		evidence,
		...(determinate
			? {
				progress: {
					fraction: Math.min(1, Math.max(0, (terminalWork ?? 0) / header.work.planned!)),
					semantics: `${header.work.unit ?? "work"} completion`,
					determinate: true
				}
			}
			: {}),
		timing: {
			startedAt: run.startedAt ?? run.createdAt,
			elapsedMs: elapsedMs(run, header, now)
		},
		usage: usageOf(header.usage, terminalWork, run),
		capabilities: {
			pause: run.capabilities?.pause === true,
			resume: run.capabilities?.resume === true,
			cancel: run.capabilities?.cancel === true
		},
		...(milestone ? { milestone } : {}),
		milestones: milestone ? [milestone] : [],
		warning: warnings[0],
		warnings,
		details: [
			{ label: "Placement", value: label(header.placement) },
			{ label: "Spec", value: header.specDigest },
			{ label: "Projection", value: `${header.projectionSchemaVersion} · revision ${header.projectionRevision}` }
		],
		...(resultOf(view) ? { result: resultOf(view) } : {}),
		...(fullVisualRef ? { fullVisualRef } : {}),
		cursorSeq: header.asOfSequence,
		terminalCursor: header.terminal?.finalSequence ?? header.asOfSequence,
		stale: false
	};
}
