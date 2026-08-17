/**
 * The expanded in-chat dialog.
 *
 * It reads the same projection the card does — no second subscription, no
 * second cursor — so opening, closing, and reopening it cannot reset a run or
 * replay its history. Closing returns focus to the card's own trigger.
 *
 * This is where coverage stops being a summary and gets explained: which units
 * reported a figure, out of how many, and who vouched for it.
 */

import { Fragment, useCallback, useEffect, useRef } from "react";
import {
	formatDurationMs,
	formatEta,
	formatWork,
	formatWorkBreakdown,
	statusBadgeClass,
	statusLabel
} from "../../runtime/runProgress/format";
import {
	coverageLabel,
	formatCount,
	formatUsd,
	metricExplanation,
	UNAVAILABLE
} from "../../runtime/runProgress/usage";
import type { CoveredMetric, RunControlIntent, RunProgressProjection } from "../../runtime/runProgress/types";
import type { RunProgressConnectionState } from "../../runtime/runProgress/subscription";
import { RunProgressBar } from "./RunProgressCard";

type Props = {
	projection: RunProgressProjection;
	connection: RunProgressConnectionState;
	intent: RunControlIntent | null;
	onRequestControl: (action: RunControlIntent["action"]) => void;
	onOpenFullRun?: (visualId: string) => void;
	onClose: () => void;
};

const FOCUSABLE =
	'button:not([disabled]), [href], input, select, textarea, [tabindex]:not([tabindex="-1"])';

function UsageRow({
	label,
	metric,
	format,
	unit
}: {
	label: string;
	metric: CoveredMetric;
	format: (value: number) => string;
	unit: string;
}) {
	const coverage = coverageLabel(metric);
	return (
		<>
			<dt>{label}</dt>
			<dd>
				<span>{metric.value == null ? UNAVAILABLE : format(metric.value)}</span>
				{coverage ? <span className="run-progress-faint"> · {coverage} coverage</span> : null}
				<span className="run-progress-coverage">{metricExplanation(metric, unit)}</span>
			</dd>
		</>
	);
}

export function RunProgressDialog({
	projection,
	connection,
	intent,
	onRequestControl,
	onOpenFullRun,
	onClose
}: Props) {
	const dialogRef = useRef<HTMLDivElement>(null);
	const closeRef = useRef<HTMLButtonElement>(null);
	const titleId = `run-progress-dialog-title-${projection.runId}`;

	const trapFocus = useCallback((event: KeyboardEvent) => {
		if (event.key !== "Tab") return;
		const nodes = dialogRef.current?.querySelectorAll<HTMLElement>(FOCUSABLE);
		if (!nodes || nodes.length === 0) return;
		const first = nodes[0]!;
		const last = nodes[nodes.length - 1]!;
		if (event.shiftKey && document.activeElement === first) {
			event.preventDefault();
			last.focus();
		} else if (!event.shiftKey && document.activeElement === last) {
			event.preventDefault();
			first.focus();
		}
	}, []);

	useEffect(() => {
		const onKey = (event: KeyboardEvent) => {
			if (event.key === "Escape") {
				event.preventDefault();
				onClose();
				return;
			}
			trapFocus(event);
		};
		document.addEventListener("keydown", onKey);
		requestAnimationFrame(() => closeRef.current?.focus());
		return () => document.removeEventListener("keydown", onKey);
	}, [onClose, trapFocus]);

	const eta = projection.timing.eta;
	const work = formatWork(projection);
	const breakdown = formatWorkBreakdown(projection);
	const unit = projection.work.unit?.replace(/s$/, "") ?? "unit";

	return (
		<div
			className="ws-dialog-scrim"
			onMouseDown={(event) => {
				if (event.target === event.currentTarget) onClose();
			}}
			data-testid={`run-progress-dialog-scrim-${projection.runId}`}
		>
			<div
				ref={dialogRef}
				className="ws-dialog run-progress-dialog"
				role="dialog"
				aria-modal="true"
				aria-labelledby={titleId}
				data-testid={`run-progress-dialog-${projection.runId}`}
				data-connection-state={connection}
			>
				<div className="ws-dialog-head">
					<div>
						<span className="ws-eyebrow">{projection.runKind}</span>
						<h2 className="ws-dialog-title" id={titleId}>{projection.title}</h2>
					</div>
					<div className="run-progress-dialog-head-aside">
						<span className={statusBadgeClass(projection.status)}>{statusLabel(projection.status)}</span>
						<button
							ref={closeRef}
							type="button"
							className="ws-btn ws-btn-ghost ws-btn-small"
							onClick={onClose}
							aria-label="Close run progress"
							data-testid={`run-progress-dialog-close-${projection.runId}`}
						>
							Close
						</button>
					</div>
				</div>

				<section className="run-progress-section" aria-label="Progress">
					{projection.terminal ? null : <RunProgressBar projection={projection} />}
					<dl className="ws-kv">
						{work ? (
							<>
								<dt>{projection.progress?.semantics ?? "Work"}</dt>
								<dd>
									{work}
									{breakdown ? <span className="run-progress-faint"> · {breakdown}</span> : null}
								</dd>
							</>
						) : null}
						<dt>{projection.terminal ? "Wall time" : "Elapsed"}</dt>
						<dd>{formatDurationMs(projection.timing.elapsedMs)}</dd>
						{projection.terminal ? null : (
							<>
								<dt>Estimate</dt>
								<dd data-testid={`run-progress-dialog-eta-${projection.runId}`}>
									{formatEta(eta)}
									{eta ? (
										<span className="run-progress-coverage">
											{eta.state === "unavailable" && eta.unavailableReason
												? eta.unavailableReason
												: `${eta.basis} · ${eta.confidence} confidence`}
										</span>
									) : null}
								</dd>
							</>
						)}
						{projection.throughput ? (
							<>
								<dt>Throughput</dt>
								<dd>
									{projection.throughput.label}
									{projection.throughput.detail ? (
										<span className="run-progress-faint"> · {projection.throughput.detail}</span>
									) : null}
								</dd>
							</>
						) : null}
					</dl>
				</section>

				{projection.phases.length > 0 ? (
					<section className="run-progress-section" aria-label="Phases">
						<h3 className="ws-section-title">Phases</h3>
						<ol className="run-progress-timeline" data-testid={`run-progress-phases-${projection.runId}`}>
							{projection.phases.map((phase) => (
								<li key={phase.id} data-phase-status={phase.status}>
									<span className={`ws-dot${phase.status === "active" ? " ws-dot-running" : phase.status === "failed" ? " ws-dot-danger" : phase.status === "completed" ? " ws-dot-success" : ""}`} aria-hidden />
									<span className="run-progress-timeline-label">{phase.label}</span>
									<span className="run-progress-faint">
										{phase.status}
										{phase.detail ? ` · ${phase.detail}` : ""}
									</span>
								</li>
							))}
						</ol>
					</section>
				) : null}

				<section className="run-progress-section" aria-label="Usage and coverage">
					<h3 className="ws-section-title">Usage</h3>
					<dl className="ws-kv" data-testid={`run-progress-usage-detail-${projection.runId}`}>
						<UsageRow label="Cost" metric={projection.usage.costUsd} format={formatUsd} unit={unit} />
						<UsageRow label="Prompt tokens" metric={projection.usage.promptTokens} format={formatCount} unit={unit} />
						<UsageRow label="Completion tokens" metric={projection.usage.completionTokens} format={formatCount} unit={unit} />
						<UsageRow label="Rollouts" metric={projection.usage.rollouts} format={formatCount} unit={unit} />
					</dl>
				</section>

				{projection.details.length > 0 ? (
					<section className="run-progress-section" aria-label="Run details">
						<h3 className="ws-section-title">Details</h3>
						<dl className="ws-kv" data-testid={`run-progress-details-${projection.runId}`}>
							{projection.details.map((detail) => (
								<Fragment key={detail.label}>
									<dt>{detail.label}</dt>
									<dd>
										<span className="ws-mono">{detail.value}</span>
										{detail.note ? <span className="run-progress-coverage">{detail.note}</span> : null}
									</dd>
								</Fragment>
							))}
						</dl>
					</section>
				) : null}

				{projection.milestones.length > 0 ? (
					<section className="run-progress-section" aria-label="Milestones">
						<h3 className="ws-section-title">Milestones</h3>
						<ul className="run-progress-milestones">
							{projection.milestones.slice(-5).reverse().map((milestone, index) => (
								<li key={`${milestone.sequence ?? index}-${milestone.label}`}>
									{milestone.label}
									{milestone.detail ? <span className="run-progress-faint"> · {milestone.detail}</span> : null}
								</li>
							))}
						</ul>
					</section>
				) : null}

				{projection.warnings.length > 0 ? (
					<section className="run-progress-section" aria-label="Warnings">
						{projection.warnings.map((warning) => (
							<p className="ws-note ws-note-warn" key={warning}>{warning}</p>
						))}
					</section>
				) : null}

				{projection.result ? (
					<section className="run-progress-section" aria-label="Result">
						<h3 className="ws-section-title">Result</h3>
						<p data-testid={`run-progress-dialog-result-${projection.runId}`}>
							{projection.result.headline ?? projection.result.absentReason}
							{projection.result.headline && projection.result.absentReason ? (
								<span className="run-progress-coverage">{projection.result.absentReason}</span>
							) : null}
							{projection.result.detail ? (
								<span className="run-progress-coverage">{projection.result.detail}</span>
							) : null}
						</p>
					</section>
				) : null}

				{intent ? (
					<p
						className={intent.state === "failed" ? "ws-note ws-note-danger" : "ws-note"}
						data-testid={`run-progress-dialog-intent-${projection.runId}`}
					>
						{intent.state === "failed"
							? `${intent.action} failed · ${intent.error ?? "the producer rejected the request"}`
							: intent.state === "acknowledged"
								? `${intent.action} acknowledged · waiting for the run to report it`
								: `${intent.action} requested`}
					</p>
				) : null}

				<div className="ws-btn-row ws-btn-row-end">
					{projection.fullVisualRef && onOpenFullRun ? (
						<button
							type="button"
							className="ws-btn ws-btn-secondary"
							onClick={() => onOpenFullRun(projection.fullVisualRef!)}
							data-testid={`run-progress-dialog-open-full-${projection.runId}`}
						>
							Open full run
						</button>
					) : null}
					{!projection.terminal && projection.capabilities.pause && projection.status !== "paused" ? (
						<button
							type="button"
							className="ws-btn ws-btn-secondary"
							onClick={() => onRequestControl("pause")}
							data-testid={`run-progress-dialog-pause-${projection.runId}`}
						>
							Pause
						</button>
					) : null}
					{!projection.terminal && projection.capabilities.resume && projection.status === "paused" ? (
						<button
							type="button"
							className="ws-btn ws-btn-primary"
							onClick={() => onRequestControl("resume")}
							data-testid={`run-progress-dialog-resume-${projection.runId}`}
						>
							Resume
						</button>
					) : null}
					{!projection.terminal && projection.capabilities.cancel ? (
						<button
							type="button"
							className="ws-btn ws-btn-danger"
							onClick={() => onRequestControl("cancel")}
							data-testid={`run-progress-dialog-cancel-${projection.runId}`}
						>
							Cancel run
						</button>
					) : null}
				</div>
			</div>
		</div>
	);
}
