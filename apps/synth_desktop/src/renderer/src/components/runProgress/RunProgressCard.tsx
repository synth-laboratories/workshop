/**
 * The compact transcript card.
 *
 * One card per run, updating in place. It never appends a chat message, so a
 * two-hour GEPA run costs the transcript one row rather than four hundred.
 *
 * What it will not do:
 *   · claim a percentage when the bar is indeterminate;
 *   · print a number for an ETA it does not have;
 *   · show $0.00 for cost nobody reported;
 *   · keep a spinner after the durable record says the run ended.
 */

import { useEffect, useRef, useState } from "react";
import { useRunProgress } from "../../hooks/useRunProgress";
import {
	formatDurationMs,
	formatEta,
	formatWork,
	formatWorkBreakdown,
	progressAriaText,
	statusBadgeClass,
	statusLabel
} from "../../runtime/runProgress/format";
import { costSummary } from "../../runtime/runProgress/usage";
import type { RunProgressProjection } from "../../runtime/runProgress/types";
import { RunProgressDialog } from "./RunProgressDialog";

type Props = {
	runId: string;
	/** The conversation this card lives in; the ownership gate reads it. */
	sessionRef?: string;
	/** Opens the full visual workspace in the side panel. */
	onOpenFullRun?: (visualId: string) => void;
};

export function RunProgressBar({ projection }: { projection: RunProgressProjection }) {
	const progress = projection.progress;
	const determinate = progress?.determinate === true && progress.fraction != null;
	const percent = determinate ? Math.round(progress!.fraction! * 100) : undefined;
	return (
		<div
			className={`run-progress-bar${determinate ? "" : " is-indeterminate"}`}
			role="progressbar"
			aria-label={progress?.semantics ?? "run progress"}
			aria-valuetext={progressAriaText(projection)}
			{...(determinate
				? { "aria-valuenow": percent, "aria-valuemin": 0, "aria-valuemax": 100 }
				: {})}
			data-testid={`run-progress-bar-${projection.runId}`}
			data-determinate={determinate ? "true" : "false"}
		>
			<span className="run-progress-bar-fill" style={determinate ? { width: `${percent}%` } : undefined} />
		</div>
	);
}

export function RunProgressCard({ runId, sessionRef, onOpenFullRun }: Props) {
	const { projection, connection, error, unavailableReason, intent, requestControl } = useRunProgress(
		runId,
		sessionRef
	);
	const [dialogOpen, setDialogOpen] = useState(false);
	const expandRef = useRef<HTMLButtonElement>(null);
	const announcedRef = useRef<string | null>(null);
	const [announcement, setAnnouncement] = useState("");

	// Milestones are announced; individual events are not. A screen reader must
	// not be read four hundred rollout completions.
	useEffect(() => {
		const milestone = projection?.milestone?.label;
		if (!milestone || milestone === announcedRef.current) return;
		announcedRef.current = milestone;
		setAnnouncement(`${projection?.title ?? "Run"}: ${milestone}`);
	}, [projection?.milestone?.label, projection?.title]);

	if (unavailableReason) {
		return (
			<div className="run-progress-card is-unavailable" data-testid={`run-progress-${runId}`}>
				<span className="run-progress-title">Run unavailable</span>
				<p className="run-progress-unavailable">{unavailableReason}</p>
			</div>
		);
	}

	if (!projection) {
		return (
			<div className="run-progress-card is-loading" data-testid={`run-progress-${runId}`} role="status">
				<span className="run-progress-title">Loading run…</span>
				{connection === "failed" && error ? <p className="run-progress-unavailable">{error}</p> : null}
			</div>
		);
	}

	const work = formatWork(projection);
	const breakdown = formatWorkBreakdown(projection);
	const eta = projection.terminal ? null : formatEta(projection.timing.eta);
	const elapsed = formatDurationMs(projection.timing.elapsedMs);
	const canControl =
		!projection.terminal &&
		(projection.capabilities.pause || projection.capabilities.resume || projection.capabilities.cancel);

	return (
		<div
			className={`run-progress-card${projection.terminal ? " is-terminal" : ""}`}
			data-testid={`run-progress-${runId}`}
			data-run-kind={projection.runKind}
			data-run-status={projection.status}
			data-connection-state={connection}
		>
			<div className="run-progress-head">
				<span className="run-progress-title" data-testid={`run-progress-title-${runId}`}>
					{projection.title}
				</span>
				<span
					className={statusBadgeClass(projection.status)}
					data-testid={`run-progress-status-${runId}`}
				>
					{statusLabel(projection.status)}
				</span>
			</div>

			<div className="run-progress-phase" data-testid={`run-progress-phase-${runId}`}>
				<span>{projection.phase.label}</span>
				{projection.phase.detail ? <span className="run-progress-faint">{projection.phase.detail}</span> : null}
			</div>

			{projection.terminal ? null : <RunProgressBar projection={projection} />}

			<div className="run-progress-metrics">
				{work ? <span data-testid={`run-progress-work-${runId}`}>{work}</span> : null}
				{breakdown ? <span className="run-progress-faint">{breakdown}</span> : null}
				{projection.throughput ? (
					<span className="run-progress-faint" data-testid={`run-progress-throughput-${runId}`}>
						{projection.throughput.label}
						{projection.throughput.detail ? ` · ${projection.throughput.detail}` : ""}
					</span>
				) : null}
			</div>

			<div className="run-progress-metrics">
				<span data-testid={`run-progress-elapsed-${runId}`}>
					{projection.terminal ? `${elapsed} wall time` : `${elapsed} elapsed`}
				</span>
				{eta ? (
					<span className="run-progress-faint" data-testid={`run-progress-eta-${runId}`}>
						{eta}
					</span>
				) : null}
				<span className="run-progress-faint" data-testid={`run-progress-usage-${runId}`}>
					{costSummary(projection.usage.costUsd, projection.work.unit?.replace(/s$/, "") ?? "unit")}
				</span>
			</div>

			{projection.result ? (
				<p className="run-progress-result" data-testid={`run-progress-result-${runId}`}>
					{projection.result.headline ?? projection.result.absentReason}
					{projection.result.detail ? <span className="run-progress-faint"> · {projection.result.detail}</span> : null}
					{projection.result.partial ? (
						<span className="ws-badge ws-badge-warn run-progress-partial">Partial</span>
					) : null}
				</p>
			) : projection.milestone ? (
				<p className="run-progress-milestone" data-testid={`run-progress-milestone-${runId}`}>
					{projection.milestone.label}
					{projection.milestone.detail ? <span className="run-progress-faint"> · {projection.milestone.detail}</span> : null}
				</p>
			) : null}

			{projection.warning ? (
				<p className="ws-note ws-note-warn" data-testid={`run-progress-warning-${runId}`}>
					{projection.warning}
				</p>
			) : null}

			<div className="run-progress-actions">
				<button
					ref={expandRef}
					type="button"
					className="ws-btn ws-btn-secondary ws-btn-small"
					onClick={() => setDialogOpen(true)}
					aria-haspopup="dialog"
					data-testid={`run-progress-expand-${runId}`}
				>
					View progress
				</button>
				{projection.fullVisualRef && onOpenFullRun ? (
					<button
						type="button"
						className="ws-btn ws-btn-ghost ws-btn-small"
						onClick={() => onOpenFullRun(projection.fullVisualRef!)}
						data-testid={`run-progress-open-full-${runId}`}
					>
						Open full run
					</button>
				) : null}
				{canControl && projection.capabilities.cancel ? (
					<button
						type="button"
						className="ws-btn ws-btn-ghost ws-btn-small"
						onClick={() => requestControl("cancel")}
						data-testid={`run-progress-cancel-${runId}`}
					>
						Cancel
					</button>
				) : null}
			</div>

			{intent ? (
				<p
					className={intent.state === "failed" ? "ws-note ws-note-danger" : "ws-note"}
					data-testid={`run-progress-intent-${runId}`}
				>
					{intent.state === "failed"
						? `${intent.action} failed · ${intent.error ?? "the producer rejected the request"}`
						: intent.state === "acknowledged"
							? `${intent.action} acknowledged · waiting for the run to report it`
							: `${intent.action} requested`}
				</p>
			) : null}

			<div className="sr-only" role="status" aria-live="polite" data-testid={`run-progress-live-${runId}`}>
				{announcement}
			</div>

			{dialogOpen ? (
				<RunProgressDialog
					projection={projection}
					connection={connection}
					intent={intent}
					onRequestControl={requestControl}
					onOpenFullRun={onOpenFullRun}
					onClose={() => {
						setDialogOpen(false);
						requestAnimationFrame(() => expandRef.current?.focus());
					}}
				/>
			) : null}
		</div>
	);
}
