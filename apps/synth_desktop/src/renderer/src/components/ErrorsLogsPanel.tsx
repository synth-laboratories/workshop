import { useCallback, useEffect, useState, type KeyboardEvent } from "react";
import { fromGenerated, spectaCommands } from "../bridge";
import { publicError } from "../runtime/publicError";
import "./ErrorsLogsPanel.css";

type FailureRow = {
	failureId: string;
	code: string;
	category: string;
	disposition: string;
	lifecycleState: string;
	message: string;
	diagnosticReference: string;
	safeContext?: { containerId?: string | null; sessionId?: string | null };
	remediation?: { kind?: string; label?: string; containerId?: string | null } | null;
};

type LogRow = {
	logId: string;
	level: string;
	component: string;
	event: string;
	message: string;
	failureId?: string | null;
	at: string;
};

export function ErrorsLogsPanel({
	sessionId,
	onOpenContainer
}: {
	sessionId?: string | null;
	onOpenContainer?: (containerId: string) => void;
}) {
	const [tab, setTab] = useState<"errors" | "logs">("errors");
	const [lifecycle, setLifecycle] = useState("");
	const [failures, setFailures] = useState<FailureRow[]>([]);
	const [logs, setLogs] = useState<LogRow[]>([]);
	const [selected, setSelected] = useState<FailureRow | null>(null);
	const [timeline, setTimeline] = useState<Array<Record<string, unknown>>>([]);
	const [mode, setMode] = useState("durable");
	const [error, setError] = useState<string | null>(null);

	const load = useCallback(async () => {
		try {
			const status = await spectaCommands.observabilityStatus();
			setMode(status.mode);
			const result = await fromGenerated(spectaCommands.failuresQuery({
				code: null,
				domain: null,
				lifecycleState: lifecycle || null,
				sessionId: sessionId ?? null,
				containerId: null,
				evaluationId: null,
				rolloutId: null,
				visualId: null,
				since: null,
				until: null,
				limit: 100
			}));
			setFailures(result.failures ?? []);
			const logResult = await fromGenerated(spectaCommands.logsQuery({
				level: null,
				component: null,
				operationId: null,
				failureId: null,
				since: null,
				until: null,
				limit: 100
			}));
			setLogs(logResult.records ?? []);
			setError(null);
		} catch (reason) {
			setError(publicError(reason, "Could not load errors and logs."));
		}
	}, [sessionId, lifecycle]);

	useEffect(() => {
		void load();
	}, [load]);

	async function openFailure(row: FailureRow) {
		setSelected(row);
		try {
			const events = await fromGenerated(spectaCommands.failuresTimeline(row.failureId));
			setTimeline(Array.isArray(events) ? events as Array<Record<string, unknown>> : []);
		} catch {
			setTimeline([]);
		}
	}

	async function exportBundle() {
		if (!selected) return;
		const bundle = await fromGenerated(spectaCommands.failureExportBundle(selected.failureId));
		await navigator.clipboard.writeText(JSON.stringify(bundle, null, 2));
	}

	function moveTabFocus(event: KeyboardEvent<HTMLButtonElement>) {
		let nextTab: "errors" | "logs" | null = null;
		if (event.key === "ArrowLeft" || event.key === "ArrowRight" || event.key === "ArrowUp" || event.key === "ArrowDown") {
			nextTab = tab === "errors" ? "logs" : "errors";
		}
		if (event.key === "Home") nextTab = "errors";
		if (event.key === "End") nextTab = "logs";
		if (!nextTab) return;
		event.preventDefault();
		setTab(nextTab);
		const buttons = event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('[role="tab"]');
		buttons?.[nextTab === "errors" ? 0 : 1]?.focus();
	}

	return (
		<section className="errors-logs-panel" data-testid="errors-logs-panel">
			<header className="errors-logs-header">
				<div className="errors-logs-tabs" role="tablist" aria-label="Failure evidence">
					<button type="button" role="tab" id="failure-evidence-tab-occurrences" aria-controls="failure-evidence-panel-occurrences" aria-selected={tab === "errors"} tabIndex={tab === "errors" ? 0 : -1} onKeyDown={moveTabFocus} onClick={() => setTab("errors")}>Occurrences</button>
					<button type="button" role="tab" id="failure-evidence-tab-logs" aria-controls="failure-evidence-panel-logs" aria-selected={tab === "logs"} tabIndex={tab === "logs" ? 0 : -1} onKeyDown={moveTabFocus} onClick={() => setTab("logs")}>Logs</button>
				</div>
				<p className="errors-logs-mode" data-testid="observability-mode">{mode}</p>
			</header>
			{error ? <p className="errors-logs-error">{error}</p> : null}
			{tab === "errors" ? (
				<div className="errors-logs-body" role="tabpanel" id="failure-evidence-panel-occurrences" aria-labelledby="failure-evidence-tab-occurrences">
					<label>
						Lifecycle
						<select value={lifecycle} onChange={(event) => setLifecycle(event.target.value)}>
							<option value="">all</option>
							<option value="open">open</option>
							<option value="awaiting_approval">awaiting approval</option>
							<option value="resolved">resolved</option>
							<option value="terminalized">terminalized</option>
						</select>
					</label>
					<ul className="errors-logs-list">
						{failures.map((row) => (
							<li key={row.failureId}>
								<button type="button" onClick={() => void openFailure(row)}>
									<strong>{row.code}</strong>
									<span>{row.lifecycleState}</span>
									<p>{row.message}</p>
								</button>
							</li>
						))}
					</ul>
					{selected ? (
						<article className="errors-logs-detail">
							<p><code>{selected.failureId}</code></p>
							<p>{selected.message}</p>
							{selected.remediation?.kind === "approve" && selected.remediation.containerId && onOpenContainer ? (
								<button type="button" onClick={() => onOpenContainer(selected.remediation!.containerId!)}>
									{selected.remediation.label ?? "Repair"}
								</button>
							) : null}
							<button type="button" onClick={() => void exportBundle()}>Copy redacted bundle</button>
							<ol>
								{timeline.map((event, index) => (
									<li key={index}>{String(event.reason ?? "")} → {String(event.to ?? "")}</li>
								))}
							</ol>
						</article>
					) : null}
				</div>
			) : (
				<ul className="errors-logs-list" role="tabpanel" id="failure-evidence-panel-logs" aria-labelledby="failure-evidence-tab-logs">
					{logs.map((row) => (
						<li key={row.logId}>
							<strong>{row.level}</strong> {row.component}/{row.event}: {row.message}
						</li>
					))}
				</ul>
			)}
		</section>
	);
}
