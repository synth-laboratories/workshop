import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { fromGenerated, spectaCommands } from "../bridge";
import { copyText } from "../runtime/clipboard";
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

type UnifiedRow = {
	id: string;
	level: string;
	component: string;
	event: string;
	message: string;
	at: string | null;
	count: number;
	failure: FailureRow | null;
};

function clockTime(value: string | null): string | null {
	if (!value) return null;
	const parsed = new Date(value);
	return Number.isNaN(parsed.getTime()) ? value : parsed.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

function timestamp(value: string | null): number {
	const parsed = Date.parse(value ?? "");
	return Number.isFinite(parsed) ? parsed : 0;
}

function copyableRow(row: UnifiedRow): string {
	return [
		`${row.level.toUpperCase()} ${row.component}/${row.event}`,
		row.count > 1 ? `Occurrences: ${row.count}` : null,
		row.at ? `Latest: ${row.at}` : null,
		"",
		row.message
	].filter((line): line is string => line !== null).join("\n");
}

function IconCopy({ copied = false }: { copied?: boolean }) {
	return copied ? (
		<svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path d="m3.25 8.25 3 3 6.5-6.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
		</svg>
	) : (
		<svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden>
			<rect x="5.25" y="2.25" width="8.5" height="9.5" rx="1.5" stroke="currentColor" strokeWidth="1.25" />
			<path d="M10.75 12.25v.5a1.5 1.5 0 0 1-1.5 1.5h-6a1.5 1.5 0 0 1-1.5-1.5v-6a1.5 1.5 0 0 1 1.5-1.5h.5" stroke="currentColor" strokeWidth="1.25" strokeLinecap="round" />
		</svg>
	);
}

export function ErrorsLogsPanel({
	sessionId,
	onOpenContainer
}: {
	sessionId?: string | null;
	onOpenContainer?: (containerId: string) => void;
}) {
	const [failures, setFailures] = useState<FailureRow[]>([]);
	const [logs, setLogs] = useState<LogRow[]>([]);
	const [errorsOnly, setErrorsOnly] = useState(true);
	const [selected, setSelected] = useState<FailureRow | null>(null);
	const [timeline, setTimeline] = useState<Array<Record<string, unknown>>>([]);
	const [mode, setMode] = useState("durable");
	const [error, setError] = useState<string | null>(null);
	const [loading, setLoading] = useState(true);
	const [copiedKey, setCopiedKey] = useState<string | null>(null);
	const copyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

	const load = useCallback(async () => {
		setLoading(true);
		try {
			const status = await spectaCommands.observabilityStatus();
			setMode(status.mode);
			const result = await fromGenerated(spectaCommands.failuresQuery({
				code: null,
				domain: null,
				lifecycleState: null,
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
		} finally {
			setLoading(false);
		}
	}, [sessionId]);

	useEffect(() => {
		void load();
	}, [load]);

	useEffect(() => () => {
		if (copyTimerRef.current) clearTimeout(copyTimerRef.current);
	}, []);

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
		await copyValue(JSON.stringify(bundle, null, 2), `bundle:${selected.failureId}`);
	}

	async function copyValue(value: string, key: string) {
		try {
			await copyText(value);
			setCopiedKey(key);
			if (copyTimerRef.current) clearTimeout(copyTimerRef.current);
			copyTimerRef.current = setTimeout(() => setCopiedKey(null), 1_500);
		} catch (reason) {
			setError(publicError(reason, "Could not copy error content."));
		}
	}

	const rows = useMemo(() => {
		const failureById = new Map(failures.map((failure) => [failure.failureId, failure]));
		const linkedFailures = new Set<string>();
		const grouped = new Map<string, UnifiedRow>();
		for (const log of logs) {
			if (log.failureId) linkedFailures.add(log.failureId);
			const key = [log.level, log.component, log.event, log.message, log.failureId ?? ""].join("\u0000");
			const current = grouped.get(key);
			if (current) {
				current.count += 1;
				if (timestamp(log.at) > timestamp(current.at)) current.at = log.at;
				continue;
			}
			grouped.set(key, {
				id: `log:${log.logId}`,
				level: log.level,
				component: log.component,
				event: log.event,
				message: log.message,
				at: log.at,
				count: 1,
				failure: log.failureId ? failureById.get(log.failureId) ?? null : null
			});
		}
		const unified = [...grouped.values()];
		for (const failure of failures) {
			if (linkedFailures.has(failure.failureId)) continue;
			unified.push({
				id: `failure:${failure.failureId}`,
				level: "error",
				component: failure.category,
				event: failure.code,
				message: failure.message,
				at: null,
				count: 1,
				failure
			});
		}
		return unified
			.filter((row) => !errorsOnly || ["error", "warn", "warning"].includes(row.level.toLowerCase()))
			.sort((a, b) => timestamp(b.at) - timestamp(a.at));
	}, [errorsOnly, failures, logs]);

	return (
		<section className="errors-logs-panel" data-testid="errors-logs-panel">
			<header className="errors-logs-header">
				<div>
					<strong>Error log</strong>
					<span>Grouped errors and local events</span>
				</div>
				<p className="errors-logs-mode" data-testid="observability-mode">{mode === "durable" ? "stored locally" : mode.replaceAll("_", " ")}</p>
			</header>
			<div className="errors-logs-toolbar">
				<label><input type="checkbox" checked={errorsOnly} onChange={(event) => setErrorsOnly(event.target.checked)} /> Errors only</label>
				<span>{rows.length} {rows.length === 1 ? "event" : "events"}</span>
				<button type="button" onClick={() => void copyValue(rows.map(copyableRow).join("\n\n---\n\n"), "visible")} disabled={rows.length === 0}>
					{copiedKey === "visible" ? "Copied" : "Copy visible"}
				</button>
				<button type="button" onClick={() => void load()} disabled={loading}>{loading ? "Loading…" : "Refresh"}</button>
			</div>
			{error ? <p className="errors-logs-error">{error}</p> : null}
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
			<ul className="errors-logs-list" aria-label="Error and diagnostic log">
				{rows.map((row) => {
					const body = <>
						<span className="errors-logs-entry-head">
							<strong className={`errors-logs-level is-${row.level}`}>{row.level}</strong>
							<span className="errors-logs-source">{row.component}/{row.event}</span>
							{row.count > 1 ? <span className="errors-logs-count">×{row.count}</span> : null}
							{clockTime(row.at) ? <time dateTime={row.at ?? undefined}>{clockTime(row.at)}</time> : null}
						</span>
						<span className="errors-logs-message">{row.message}</span>
					</>;
					const copied = copiedKey === row.id;
					return <li className="errors-logs-item" key={row.id}>{row.failure
						? <button type="button" className="errors-logs-entry" onClick={() => void openFailure(row.failure!)}>{body}</button>
						: <div className="errors-logs-entry">{body}</div>}
						<button
							type="button"
							className={`errors-logs-copy${copied ? " is-copied" : ""}`}
							aria-label={copied ? "Copied error content" : "Copy error content"}
							title={copied ? "Copied" : "Copy error content"}
							onClick={() => void copyValue(copyableRow(row), row.id)}
						>
							<IconCopy copied={copied} />
						</button>
					</li>;
				})}
			</ul>
			{loading ? <p className="errors-logs-loading">Loading error log…</p> : null}
			{!loading && !error && rows.length === 0 ? (
				<div className="errors-logs-empty" data-testid="error-log-empty">
					<strong>No matching events</strong>
					<span>{errorsOnly ? "No errors or warnings need attention right now." : "Local events from this session will appear here."}</span>
				</div>
			) : null}
		</section>
	);
}
