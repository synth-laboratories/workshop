/**
 * Hosted SFT → `run_progress.v1`.
 *
 * SFT is the workflow where honesty costs the most, so this adapter says
 * "unavailable" more often than the others:
 *
 *   · Queue ETA, training ETA, and evaluation ETA are three different
 *     estimates over three different populations. They are never blended. The
 *     projection carries whichever belongs to the phase the run is in, and the
 *     basis line names it.
 *   · Queue time is displayed as elapsed queue time and is never execution
 *     evidence. Waiting for an accelerator says nothing about step duration.
 *   · Training has no ETA unless the producer declared a step or epoch total.
 *     Today's hosted producers do not, so the card reads "ETA unavailable ·
 *     provider did not declare total steps" rather than extrapolating from a
 *     loss curve.
 *   · A ready checkpoint is not a promoted one, and training success is not
 *     uplift. Only the paired heldout comparison licenses an uplift headline.
 */

import type {
	ProjectedState
} from "@synth/visual-templates/optimizers/_shared/optimizer.run.v1/components/projectEvents.ts";
import { estimatePhaseEta, type EtaEvidence } from "./eta";
import { formatDurationMs } from "./format";
import type {
	RunProgressDetail,
	RunProgressPhase,
	RunProgressProjection,
	RunProgressResult,
	RunProgressWork
} from "./types";
import type { AdapterInput } from "./adapterShared";
import {
	baseProjection,
	lastDisruptionMs,
	milestoneFromEvents,
	rolloutCompletionTimes,
	usageProjection
} from "./adapterShared";

type SftState = NonNullable<ProjectedState["sft"]>;

const CHECKPOINT_ROLLOUT_COMPLETION_TYPES = [
	"sft.checkpoint_rollout.completed",
	"sft.checkpoint_rollout.failed"
];
const SFT_DISRUPTION_TYPES = ["sft.checkpoint_rollout.failed", "rollout.circuit_breaker.tripped"];

/**
 * A declared training denominator, if any producer ever emits one. Read from
 * the compute and dataset snapshots the projection already keeps; absent means
 * absent, and the ETA says so.
 */
function declaredTrainingTotal(sft: SftState): { steps?: number; epochs?: number } {
	const read = (source: Record<string, unknown>, keys: string[]): number | undefined => {
		for (const key of keys) {
			const value = source[key];
			if (typeof value === "number" && Number.isFinite(value) && value > 0) return value;
		}
		return undefined;
	};
	const compute = sft.compute ?? {};
	const dataset = sft.dataset ?? {};
	return {
		steps: read(compute, ["total_steps", "totalSteps", "max_steps", "maxSteps"]) ??
			read(dataset, ["total_steps", "totalSteps"]),
		epochs: read(compute, ["total_epochs", "totalEpochs", "num_epochs", "numEpochs"])
	};
}

const QUEUED_TYPES = new Set(["run.queued", "sft.run.queued", "sft.training.queued"]);
const STARTED_TYPES = new Set([
	"run.started",
	"sft.run.started",
	"sft.training.started",
	"sft.training.resumed"
]);

/**
 * Ms spent waiting for an accelerator. Displayed as its own fact and never
 * folded into the training estimate — a long queue says nothing about step time.
 */
function queueElapsedMs(input: AdapterInput): number | undefined {
	const queued = input.events.find((event) => QUEUED_TYPES.has(event.type));
	if (!queued) return undefined;
	const started = input.events.find((event) => STARTED_TYPES.has(event.type));
	const from = Date.parse(queued.occurredAt);
	if (!Number.isFinite(from)) return undefined;
	const to = started ? Date.parse(started.occurredAt) : input.now;
	return Number.isFinite(to) ? Math.max(0, to - from) : undefined;
}

/**
 * `terminal` and `failed` come from the durable run record, never from the
 * status the event stream reduced to. A hosted run that stopped emitting has a
 * finished record and an unfinished-looking stream; the record wins.
 */
function sftPhases(
	sft: SftState,
	terminal: boolean,
	failed: boolean,
	promoted: string | undefined
): RunProgressPhase[] {
	const baselineScored = (sft.baseline?.seeds.length ?? 0) > 0;
	const collected = sft.curation.collected ?? 0;
	const curated = (sft.curation.accepted ?? 0) > 0;
	const datasetReady = Object.keys((sft.dataset.splits as Record<string, unknown> | undefined) ?? {}).length > 0;
	const training = sft.points.length > 0;
	const checkpointCount = sft.checkpoints.length;
	const readyCount = sft.checkpoints.filter((entry) => entry.ready === true || entry.promoted === true).length;
	const campaignCount = sft.campaigns.length;
	const campaignsSettled = campaignCount > 0 && sft.campaigns.every((campaign) =>
		["completed", "failed"].includes(String(campaign.status ?? ""))
	);
	const isPromoted = promoted != null || sft.checkpoints.some((entry) => entry.promoted === true);
	const comparisonPairs = sft.comparison?.pairs.length ?? 0;

	const settle = (
		id: string,
		label: string,
		started: boolean,
		done: boolean,
		detail?: string
	): RunProgressPhase => {
		if (done) return { id, label, status: "completed", detail };
		if (started) return { id, label, status: terminal ? (failed ? "failed" : "completed") : "active", detail };
		return { id, label, status: terminal ? "skipped" : "pending", detail };
	};

	return [
		settle("queue", "Queue", true, training || checkpointCount > 0 || baselineScored, "waiting for an accelerator"),
		settle("baseline", "Baseline", baselineScored, baselineScored, baselineScored ? `${sft.baseline?.seeds.length} seeds scored` : undefined),
		settle("collection", "Collection", collected > 0, collected > 0 && curated, collected > 0 ? `${collected} teacher rollouts` : undefined),
		settle(
			"curation",
			"Curation",
			(sft.curation.considered ?? 0) > 0,
			curated,
			sft.curation.accepted != null && sft.curation.considered != null
				? `${sft.curation.accepted}/${sft.curation.considered} retained`
				: undefined
		),
		{
			id: "dataset",
			label: "Dataset",
			status: datasetReady ? "completed" : terminal ? "skipped" : "pending"
		},
		settle("training", "Training", training, training && terminal && !failed, training ? `${sft.points.length} metric records` : undefined),
		settle(
			"checkpoints",
			"Checkpoints",
			checkpointCount > 0,
			checkpointCount > 0 && readyCount === checkpointCount && terminal,
			checkpointCount > 0 ? `${readyCount}/${checkpointCount} ready` : undefined
		),
		settle(
			"evaluation",
			"Eval campaigns",
			campaignCount > 0,
			campaignsSettled,
			campaignCount > 0 ? `${campaignCount} campaign${campaignCount === 1 ? "" : "s"}` : undefined
		),
		{
			id: "promotion",
			label: "Promotion",
			status: isPromoted ? "completed" : terminal ? "skipped" : "pending",
			detail: isPromoted ? undefined : "requires an explicit promote event — 'ready' is not promotion"
		},
		settle(
			"heldout",
			"Heldout comparison",
			comparisonPairs > 0,
			comparisonPairs > 0,
			comparisonPairs > 0 ? `${comparisonPairs} paired seeds` : "the only evidence for an uplift claim"
		)
	];
}

/** The newest step record. Training's own progress line, with no denominator. */
function latestPoint(sft: SftState) {
	return sft.points.at(-1);
}

/**
 * How many steps a producer covers per metric record, from the median gap
 * between reported steps. Null until two records exist — the reporting interval
 * is observed, never assumed to be 1.
 */
function stepsBetweenRecords(sft: SftState): number | null {
	const steps = sft.points
		.map((point) => point.step)
		.filter((step): step is number => typeof step === "number" && Number.isFinite(step))
		.sort((left, right) => left - right);
	const gaps: number[] = [];
	for (let index = 1; index < steps.length; index += 1) {
		const gap = steps[index]! - steps[index - 1]!;
		if (gap > 0) gaps.push(gap);
	}
	if (gaps.length === 0) return null;
	gaps.sort((left, right) => left - right);
	const middle = Math.floor(gaps.length / 2);
	return gaps.length % 2 === 0 ? (gaps[middle - 1]! + gaps[middle]!) / 2 : gaps[middle]!;
}

function sftWork(sft: SftState, phaseId: string): RunProgressWork {
	if (phaseId === "evaluation") {
		const children = sft.campaigns.flatMap((campaign) => campaign.children);
		const scored = children.filter((child) => child.attributes?.reward != null).length;
		return {
			completed: scored,
			active: children.length - scored,
			total: children.length > 0 ? children.length : undefined,
			unit: "rollouts"
		};
	}
	if (phaseId === "training") {
		const declared = declaredTrainingTotal(sft);
		const point = latestPoint(sft);
		return {
			...(point?.step != null ? { completed: point.step } : {}),
			...(declared.steps != null ? { total: declared.steps } : {}),
			unit: "steps"
		};
	}
	if (phaseId === "curation" || phaseId === "collection") {
		return {
			...(sft.curation.accepted != null ? { completed: sft.curation.accepted } : {}),
			...(sft.curation.considered != null ? { total: sft.curation.considered } : {}),
			unit: "trajectories"
		};
	}
	if (phaseId === "baseline") {
		const seeds = sft.baseline?.seeds ?? [];
		return {
			completed: seeds.filter((seed) => seed.reward != null).length,
			total: seeds.length > 0 ? seeds.length : undefined,
			unit: "seeds"
		};
	}
	return {};
}

function sftDetails(sft: SftState, input: AdapterInput): RunProgressDetail[] {
	const details: RunProgressDetail[] = [];
	const point = latestPoint(sft);
	if (point?.step != null) {
		details.push({
			label: "Training",
			value: [
				`step ${point.step}`,
				point.epoch != null ? `epoch ${point.epoch}` : null,
				point.trainLoss != null ? `train loss ${point.trainLoss.toFixed(3)}` : null,
				point.validationLoss != null ? `val loss ${point.validationLoss.toFixed(3)}` : null
			].filter(Boolean).join(" · ")
		});
	}
	const queue = queueElapsedMs(input);
	if (queue != null) {
		details.push({
			label: "Queued for",
			value: formatDurationMs(queue),
			note: "excluded from the training estimate"
		});
	}
	if (sft.lineage?.baseModel) details.push({ label: "Base model", value: String(sft.lineage.baseModel) });
	const splits = (sft.dataset.splits as Record<string, { count?: number }> | undefined) ?? {};
	for (const [name, split] of Object.entries(splits)) {
		if (split?.count != null) details.push({ label: `Dataset · ${name}`, value: String(split.count) });
	}
	if (sft.checkpoints.length > 0) {
		const ready = sft.checkpoints.filter((entry) => entry.ready === true || entry.promoted === true).length;
		details.push({ label: "Checkpoints", value: `${ready} ready of ${sft.checkpoints.length}` });
	}
	if (sft.curation.seedsCovered != null) {
		details.push({ label: "Seeds covered", value: String(sft.curation.seedsCovered) });
	}
	return details;
}

function sftResult(sft: SftState, failed: boolean, promoted: string | undefined): RunProgressResult {
	if (failed) return { headline: "Training run failed", partial: true };
	const comparison = sft.comparison;
	const paired = (comparison?.pairs ?? []).filter((pair) => pair.base?.reward != null && pair.trained?.reward != null);
	if (comparison && paired.length > 0) {
		const baseMean = paired.reduce((sum, pair) => sum + pair.base!.reward!, 0) / paired.length;
		const trainedMean = paired.reduce((sum, pair) => sum + pair.trained!.reward!, 0) / paired.length;
		const uplift = trainedMean - baseMean;
		return {
			headline: `${uplift >= 0 ? "+" : ""}${uplift.toFixed(3)} heldout uplift`,
			detail: `${comparison.trainedLabel} ${trainedMean.toFixed(3)} vs ${comparison.baseLabel} ${baseMean.toFixed(3)} over ${paired.length} paired seeds`,
			partial: paired.length < (comparison.pairs.length ?? 0)
		};
	}
	if (promoted) {
		return {
			headline: `Promoted ${promoted}`,
			absentReason: "no paired heldout comparison was emitted, so no uplift is claimed",
			partial: true
		};
	}
	const ready = sft.checkpoints.filter((entry) => entry.ready === true);
	if (ready.length > 0) {
		return {
			absentReason: "checkpoints are ready but none was promoted and no heldout comparison ran",
			detail: `${ready.length} ready checkpoint${ready.length === 1 ? "" : "s"}`,
			partial: true
		};
	}
	return { absentReason: "no checkpoint reached ready", partial: true };
}

export function projectSft(input: AdapterInput, projected: ProjectedState): RunProgressProjection {
	const sft = projected.sft;
	const base = baseProjection(input, "sft");
	if (!sft) return base;

	// `projected.summary.summary` is the producer-authored summary the reducer
	// accumulated; `projected.summary` itself is the reducer's own envelope.
	const runSummary = (projected.summary.summary ?? {}) as Record<string, unknown>;
	const explicitPromotion = [...input.events]
		.reverse()
		.find((event) => event.type === "sft.checkpoint.promoted");
	const explicitPromotionId = explicitPromotion?.item?.id;
	const promoted = typeof runSummary.promotedCheckpointId === "string"
		? runSummary.promotedCheckpointId
		: typeof explicitPromotionId === "string" ? explicitPromotionId : undefined;
	const phases = sftPhases(sft, base.terminal, base.status === "failed", promoted);
	const active = phases.find((phase) => phase.status === "active");
	const phaseId = active?.id ?? (base.terminal ? "heldout" : "queue");
	const work = sftWork(sft, phaseId);
	const determinate = work.total != null && work.total > 0 && work.completed != null;
	const fraction = determinate ? Math.min(1, work.completed! / work.total!) : undefined;

	// Three phases, three estimate bases. Only the one the run is in is offered.
	const evidence: EtaEvidence = (() => {
		if (phaseId === "evaluation") {
			return {
				phaseId: "checkpoint evaluation",
				completions: rolloutCompletionTimes(input.events, CHECKPOINT_ROLLOUT_COMPLETION_TYPES),
				remainingUnits: determinate ? Math.max(0, work.total! - work.completed!) : undefined,
				unit: "rollout",
				disruptedAtMs: lastDisruptionMs(input.events, SFT_DISRUPTION_TYPES),
				paused: base.status === "paused",
				unavailableReason: determinate ? undefined : "no checkpoint-evaluation rollouts were allocated"
			};
		}
		if (phaseId === "training") {
			const declared = declaredTrainingTotal(sft);
			// Metric records are the only training clock, and queue time is excluded
			// by construction: no metric record exists while the run is queued.
			//
			// The interval between records measures a *record*, not a step, so the
			// remaining work must be converted into records too. A producer that
			// reports every 100 steps and a producer that reports every step are
			// both handled by dividing by the observed steps-per-record.
			const completions = input.events
				.filter((event) => event.type === "sft.step.metrics" || event.type === "sft.training.metrics")
				.map((event) => Date.parse(event.occurredAt))
				.filter((value) => Number.isFinite(value));
			const point = latestPoint(sft);
			const stepsPerRecord = stepsBetweenRecords(sft);
			const remainingRecords = declared.steps != null && point?.step != null && stepsPerRecord != null
				? Math.max(0, Math.ceil((declared.steps - point.step) / stepsPerRecord))
				: undefined;
			return {
				phaseId: "training",
				completions,
				remainingUnits: remainingRecords,
				unit: "metric record",
				paused: base.status === "paused",
				unavailableReason: declared.steps == null
					? "provider did not declare total steps"
					: stepsPerRecord == null
						? "two metric records are needed to know the reporting interval"
						: undefined
			};
		}
		if (phaseId === "queue") {
			return {
				phaseId: "queue",
				completions: [],
				unit: "step",
				paused: base.status === "paused",
				unavailableReason: "queue position and accelerator availability are not reported"
			};
		}
		return {
			phaseId,
			completions: [],
			unit: "unit",
			paused: base.status === "paused",
			unavailableReason: `${phaseId} does not report a bounded unit count`
		};
	})();

	const warnings = [...base.warnings];
	if (promoted && (sft.comparison?.pairs.length ?? 0) === 0) {
		warnings.push("a checkpoint was promoted without a paired heldout comparison");
	}
	const unpaired = (sft.comparison?.pairs ?? []).filter(
		(pair) => pair.base?.reward == null || pair.trained?.reward == null
	).length;
	if (unpaired > 0) {
		warnings.push(`${unpaired} heldout seed${unpaired === 1 ? "" : "s"} missing a reward on one arm`);
	}

	const milestone = (() => {
		if (promoted) return { label: `Promoted checkpoint ${promoted}` };
		const ready = sft.checkpoints.filter((entry) => entry.ready === true).at(-1);
		if (ready?.id) return { label: `Checkpoint ${String(ready.id)} ready`, detail: "ready is not promoted" };
		const point = latestPoint(sft);
		if (point?.trainLoss != null) {
			return { label: `Step ${point.step} · train loss ${point.trainLoss.toFixed(3)}` };
		}
		return milestoneFromEvents(input.events);
	})();
	const milestones = milestone ? [...base.milestones, milestone] : base.milestones;

	const throughput = (() => {
		if (phaseId !== "training") return undefined;
		const stepTimes = evidence.completions;
		if (stepTimes.length < 2) return undefined;
		const span = stepTimes.at(-1)! - stepTimes[0]!;
		if (span <= 0) return undefined;
		const perMinute = ((stepTimes.length - 1) * 60_000) / span;
		return { label: `${perMinute >= 10 ? perMinute.toFixed(0) : perMinute.toFixed(1)} metric records/min` };
	})();

	return {
		...base,
		phase: active ?? {
			id: phaseId,
			label: base.terminal ? "Finished" : "Queued",
			status: base.terminal ? "completed" : "pending",
			detail: base.terminal ? undefined : "waiting for an accelerator"
		},
		phases,
		work,
		progress: {
			...(fraction != null ? { fraction } : {}),
			semantics: phaseId === "training"
				? "declared training steps"
				: phaseId === "evaluation"
					? "checkpoint-evaluation rollouts"
					: `${phaseId} completion`,
			determinate
		},
		timing: {
			...base.timing,
			...(base.terminal ? {} : { eta: estimatePhaseEta(evidence) })
		},
		// Hosted SFT usage is the provider's to report. Nothing is derived from
		// step counts or dataset size; unreported stays unavailable.
		usage: usageProjection(projected, input.events, work.total, "provider"),
		...(throughput ? { throughput } : {}),
		milestone: milestones.at(-1),
		milestones,
		warning: warnings[0],
		warnings,
		details: sftDetails(sft, input),
		...(base.terminal ? { result: sftResult(sft, base.status === "failed", promoted) } : {})
	};
}
