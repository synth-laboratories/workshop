/**
 * The optimizer run inspector: identity, status, live progress, and per-trial
 * work for one selected run.
 *
 * Sources, in authority order:
 *   · the durable run record (identity, coarse status, usage) passed in;
 *   · `bridges.optimizers.runViewV2` — the kernel V2 view carrying lifecycle,
 *     work summary, spec digest, placement, and per-trial work items;
 *   · the `runProgress` projection pipeline (the same one the chat card
 *     reads), for the phase strip, work counts, and warnings — one reduction,
 *     not a second one invented here;
 *   · the `eval.trials` state slice, for per-seed rows on eval campaigns.
 *
 * Page-owned blocks that need the page's mutation handlers (hosted training,
 * cloud outputs, the eval scorecard, and the action row) render as children so
 * their state stays where their handlers live.
 */

import { useEffect, useState, type ReactNode } from "react";
import type { OptimizerRunRecord } from "@synth/runtime-protocol";
import type { OptimizerRunViewV2 } from "../../generated/protocol";
import { bridges } from "../../runtime/desktopBridge";
import { publicError } from "../../runtime/publicError";
import { useRunProgress } from "../../hooks/useRunProgress";
import { RunProgressBar } from "../runProgress/RunProgressCard";
import {
	formatDurationMs,
	formatWork,
	formatWorkBreakdown,
	progressUnavailableLine,
	statusBadgeClass,
	statusLabel
} from "../../runtime/runProgress/format";
import { costSummary } from "../../runtime/runProgress/usage";
import { algorithmLabel, formatWhen, runTitle, statusChipClass, statusText } from "./runPresentation";

type Props = {
	run: OptimizerRunRecord;
	/** The page's execution-binding label; derived where the bindings live. */
	executionLabel: string | null;
	/** Page-owned blocks: hosted training, cloud outputs, scorecard, actions. */
	children?: ReactNode;
};

type TrialRow = {
	id: string;
	candidate: string | null;
	seed: string | null;
	stage: string | null;
	status: string;
	reward: number | null;
};

type OptimizerDiagnostic = {
	title: string;
	message: string;
	field?: string;
	raw?: string;
	logPath?: string;
};

function stringOrNull(value: unknown): string | null {
	return typeof value === "string" && value.length > 0 ? value : null;
}

function numberOrNull(value: unknown): number | null {
	return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function fileName(path: string): string {
	return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

/** Structured native failures become one actionable block, never `[object Object]`. */
function optimizerDiagnostic(error: unknown): OptimizerDiagnostic | null {
	if (!error) return null;
	const value = typeof error === "object" ? error as Record<string, unknown> : {};
	const message = typeof error === "string"
		? error
		: typeof value.message === "string" ? value.message : publicError(error);
	const raw = typeof value.stderrTail === "string" ? value.stderrTail : message;
	const missingField = raw.match(/configuration error:\s*([a-z0-9_.]+)\s+is required and must be positive/i)?.[1];
	if (missingField) {
		const estimate = missingField.includes("rollout") ? "rollout" : missingField.includes("proposer") ? "proposer" : "optimizer";
		return {
			title: `Missing ${estimate} cost estimate`,
			message: "The safety budget rejected this recipe before compute started.",
			field: missingField,
			raw,
			logPath: typeof value.logPath === "string" ? value.logPath : undefined
		};
	}
	return {
		title: "Optimizer run failed",
		message,
		raw: raw !== message ? raw : undefined,
		logPath: typeof value.logPath === "string" ? value.logPath : undefined
	};
}

/** Rows of the `eval.trials` state slice — per-seed truth for eval campaigns. */
function trialRowsFromSlice(slice: unknown): TrialRow[] {
	const data = (slice as { data?: unknown } | null | undefined)?.data;
	const trials = data && typeof data === "object" ? (data as Record<string, unknown>).trials : null;
	if (!Array.isArray(trials)) return [];
	const rows: TrialRow[] = [];
	for (const entry of trials) {
		if (!entry || typeof entry !== "object") continue;
		const row = entry as Record<string, unknown>;
		const id = stringOrNull(row.id);
		if (!id) continue;
		const metrics = row.metrics && typeof row.metrics === "object"
			? row.metrics as Record<string, unknown>
			: {};
		rows.push({
			id,
			candidate: stringOrNull(row.candidateId ?? row.candidate_id),
			seed: row.seed == null ? null : String(row.seed as string | number),
			stage: stringOrNull(row.stage),
			status: stringOrNull(row.status) ?? "unknown",
			// A reward nothing measured is unknown, not zero.
			reward: numberOrNull(row.reward) ?? numberOrNull(metrics.reward) ?? numberOrNull(metrics.score)
		});
	}
	return rows.sort((left, right) =>
		(left.candidate ?? "").localeCompare(right.candidate ?? "")
			|| (Number(left.seed) || 0) - (Number(right.seed) || 0)
			|| left.id.localeCompare(right.id));
}

export function RunInspector({ run, executionLabel, children }: Props) {
	const [view, setView] = useState<OptimizerRunViewV2 | null>(null);
	const [viewError, setViewError] = useState<string | null>(null);
	const [trials, setTrials] = useState<TrialRow[] | null>(null);
	const [copied, setCopied] = useState(false);
	const { projection } = useRunProgress(run.id);

	useEffect(() => {
		setCopied(false);
	}, [run.id]);

	useEffect(() => {
		const api = bridges.optimizers;
		let live = true;
		if (!api) {
			setView(null);
			setTrials(null);
			return () => { live = false; };
		}
		if (typeof api.runViewV2 === "function") {
			void api.runViewV2(run.id).then((next) => {
				if (!live) return;
				setView(next);
				setViewError(null);
			}).catch((reason) => {
				if (!live) return;
				setView(null);
				setViewError(publicError(reason));
			});
		}
		if (run.algorithmId === "eval" && typeof api.getStateBatch === "function") {
			void api.getStateBatch(run.id, ["eval.trials"]).then((slices) => {
				if (!live) return;
				const slice = (slices as Array<{ sliceId?: string } | null>)
					.find((candidate) => candidate?.sliceId === "eval.trials");
				setTrials(slice ? trialRowsFromSlice(slice) : []);
			}).catch(() => {
				if (live) setTrials(null);
			});
		} else {
			setTrials(null);
		}
		return () => { live = false; };
	}, [run.id, run.algorithmId, run.status, run.cursorSeq]);

	const copyRunId = () => {
		void navigator.clipboard?.writeText(run.id).then(() => {
			setCopied(true);
			window.setTimeout(() => setCopied(false), 1500);
		}).catch(() => undefined);
	};

	const header = view?.header ?? null;
	const workItems = view?.projection.workItems ?? [];
	const diagnostic = optimizerDiagnostic(run.error);
	const summary = run.summary && typeof run.summary === "object"
		? run.summary as Record<string, unknown>
		: {};
	const runDirectory = typeof summary.runDirectory === "string" ? summary.runDirectory : null;
	const costUsd = run.usage?.costUsd ?? header?.usage.costUsd ?? null;
	const promptTokens = run.usage?.promptTokens ?? header?.usage.promptTokens;
	const completionTokens = run.usage?.completionTokens ?? header?.usage.completionTokens;
	const work = header?.work;
	const workSummary = work
		? [
				work.succeeded != null ? `${work.succeeded} succeeded` : null,
				work.failed ? `${work.failed} failed` : null,
				work.cancelled ? `${work.cancelled} cancelled` : null,
				work.running ? `${work.running} running` : null,
				work.planned != null ? `${work.planned} planned` : null
			].filter((part): part is string => part != null).join(" · ")
		: "";
	const showEvalTrials = trials != null && trials.length > 0;
	const showWorkItems = !showEvalTrials && workItems.length > 0;

	return (
		<div data-testid="optimizer-inspector">
			<span className="optimizer-eyebrow">Run details</span>
			<h2>{algorithmLabel(run.algorithmId)}</h2>
			<p>{runTitle(run)}</p>

			<div className="optimizer-run-id-row">
				<code className="optimizer-run-id" data-testid="optimizer-run-id">{run.id}</code>
				<button
					type="button"
					className="secondary-button"
					onClick={copyRunId}
					data-testid="copy-optimizer-run-id"
					aria-label={`Copy run id ${run.id}`}
				>
					{copied ? "Copied" : "Copy"}
				</button>
			</div>

			<dl>
				<dt>Status</dt>
				<dd><span className={statusChipClass(run.status)} data-testid="optimizer-inspector-status">{statusText(run.status)}</span></dd>
				<dt>Source</dt><dd>{run.source}</dd>
				<dt>Execution</dt><dd data-testid="optimizer-execution-mode">{executionLabel}</dd>
				{header ? <><dt>Placement</dt><dd>{statusText(header.placement)}</dd></> : null}
				<dt>Live events</dt><dd>{run.capabilities?.streamEvents ? "Available" : "Replay / refresh"}</dd>
				<dt>Cursor</dt><dd>{run.cursorSeq ?? "—"}</dd>
				<dt>Cost</dt><dd>{costUsd == null ? "—" : `$${costUsd.toFixed(2)}`}</dd>
				{promptTokens != null || completionTokens != null ? (
					<><dt>Tokens</dt><dd>{promptTokens ?? "—"} in · {completionTokens ?? "—"} out</dd></>
				) : null}
				<dt>Created</dt><dd>{formatWhen(run.createdAt)}</dd>
				<dt>Started</dt><dd>{formatWhen(run.startedAt)}</dd>
				<dt>Finished</dt><dd>{formatWhen(run.finishedAt)}</dd>
				{header ? <><dt>Spec digest</dt><dd><code className="optimizer-inspector-digest">{header.specDigest}</code></dd></> : null}
			</dl>
			{viewError ? (
				<p className="optimizer-inspector-view-error" data-testid="optimizer-view-error">
					Kernel view unavailable · {viewError}
				</p>
			) : null}

			{projection ? (
				<section className="optimizer-training-progress optimizer-run-progress" data-testid="optimizer-run-progress">
					<div className="optimizer-training-title">
						<span className="optimizer-eyebrow">Progress</span>
						<span className={statusBadgeClass(projection.status)} data-testid="optimizer-progress-status">
							{statusLabel(projection.status)}
						</span>
					</div>
					<ul className="optimizer-phase-strip" data-testid="optimizer-phase-strip">
						{projection.phases.map((phase) => (
							<li key={phase.id} data-phase-status={phase.status}>
								{phase.label}
								{phase.detail ? <small>{phase.detail}</small> : null}
							</li>
						))}
					</ul>
					{projection.terminal ? null : <RunProgressBar projection={projection} />}
					<div className="run-progress-metrics">
						{formatWork(projection) ? <span data-testid="optimizer-progress-work">{formatWork(projection)}</span> : null}
						{progressUnavailableLine(projection) ? (
							<span className="run-progress-faint" title={projection.evidence.diagnostic}>
								{progressUnavailableLine(projection)}
							</span>
						) : null}
						{formatWorkBreakdown(projection) ? <span className="run-progress-faint">{formatWorkBreakdown(projection)}</span> : null}
					</div>
					<div className="run-progress-metrics">
						<span>{formatDurationMs(projection.timing.elapsedMs)} {projection.terminal ? "wall time" : "elapsed"}</span>
						<span className="run-progress-faint">
							{costSummary(projection.usage.costUsd, projection.work.unit?.replace(/s$/, "") ?? "unit")}
						</span>
					</div>
					{projection.warning ? (
						<p className="ws-note ws-note-warn" data-testid="optimizer-progress-warning">{projection.warning}</p>
					) : null}
				</section>
			) : null}

			{showEvalTrials || showWorkItems ? (
				<section className="optimizer-trials" data-testid="optimizer-trials">
					<div className="optimizer-training-title">
						<span className="optimizer-eyebrow">Trials</span>
						{workSummary ? <small>{workSummary}</small> : null}
					</div>
					<div className="optimizer-trials-scroll">
						{showEvalTrials ? (
							<table>
								<thead>
									<tr><th>Seed</th><th>Candidate</th><th>Stage</th><th>Status</th><th>Reward</th></tr>
								</thead>
								<tbody>
									{trials.map((trial) => (
										<tr key={trial.id} data-testid={`optimizer-trial-${trial.id}`}>
											<td>{trial.seed ?? "—"}</td>
											<td>{trial.candidate ?? "—"}</td>
											<td>{trial.stage ?? "—"}</td>
											<td>{statusText(trial.status)}</td>
											<td>{trial.reward == null ? "—" : trial.reward.toFixed(3)}</td>
										</tr>
									))}
								</tbody>
							</table>
						) : (
							<table>
								<thead>
									<tr><th>Work item</th><th>Kind</th><th>State</th></tr>
								</thead>
								<tbody>
									{workItems.map((item) => (
										<tr key={item.workItemId} data-testid={`optimizer-work-item-${item.workItemId}`}>
											<td>{item.workItemId}</td>
											<td>{statusText(item.kind)}</td>
											<td>{statusText(item.terminal ?? item.lifecycle)}</td>
										</tr>
									))}
								</tbody>
							</table>
						)}
					</div>
				</section>
			) : null}

			{diagnostic ? (
				<section className="optimizer-diagnostic" role="alert" data-testid="optimizer-diagnostic">
					<span className="optimizer-diagnostic-kicker">Why it stopped</span>
					<strong>{diagnostic.title}</strong>
					<p>{diagnostic.message}</p>
					{diagnostic.field ? <code className="optimizer-diagnostic-field">{diagnostic.field}</code> : null}
					{diagnostic.raw ? (
						<details className="optimizer-diagnostic-details">
							<summary>Show technical details</summary>
							<pre data-testid="optimizer-stderr-tail">{diagnostic.raw}</pre>
						</details>
					) : null}
					{diagnostic.logPath ? <small>Log · {fileName(diagnostic.logPath)}</small> : null}
				</section>
			) : null}
			{runDirectory ? (
				<details className="optimizer-run-files" data-testid="optimizer-run-files">
					<summary>Logs &amp; artifacts</summary>
					<code>{runDirectory}</code>
					<ul><li>workshop.stdout.log</li><li>workshop.stderr.log</li><li>events.jsonl</li><li>result_manifest.json</li></ul>
				</details>
			) : null}
			{children}
		</div>
	);
}
