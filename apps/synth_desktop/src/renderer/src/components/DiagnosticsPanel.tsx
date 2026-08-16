import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { COMMANDS, invokeCommand } from "../bridge";
import "./DiagnosticsPanel.css";

/**
 * The Diagnostics surface.
 *
 * Kept off the core render path in both directions: nothing here runs until the
 * tab is mounted, and every query is cancellable so closing the pane mid-fetch
 * abandons the work rather than committing it to a dead component.
 *
 * The pane and the agent read the same typed query contract, so what a human
 * sees here and what `synth_diagnostics` returns cannot drift.
 */

type DiagnosticStatus = {
	state: "starting" | "ready" | "degraded" | "stopped";
	reason?: string | null;
	localOnly?: boolean;
	local_only?: boolean;
	retention_days?: number;
	quota_bytes?: number;
	index_bytes?: number;
	index_lag?: number;
	stored_events?: number;
	queue?: { depth?: number; capacity?: number };
};

type DiagnosticGroup = {
	code: string;
	count: number;
	severity: string;
	component: string;
	message: string;
	first_seen: string;
};

type DiagnosticEvent = {
	journal_sequence: number;
	event_id: string;
	timestamp: string;
	severity: string;
	component: string;
	event: string;
	code: string;
	message: string;
	visual_id?: string;
	container_id?: string;
	rollout_id?: string;
	stream_id?: string;
	optimizer_run_id?: string;
	trace_id?: string;
	session_id?: string;
	details?: Record<string, unknown>;
};

type DiagnosticExplanation = {
	cause: (DiagnosticEvent & { rank: number; correlation: Record<string, string> }) | null;
	symptoms: Array<DiagnosticEvent & { rank: number }>;
	remediation: string | null;
	matched: number;
	identities: Record<string, string[]>;
};

type DiagnosticResult = {
	source: "victorialogs" | "journal";
	count: number;
	truncated: boolean;
	groups: DiagnosticGroup[];
	events: DiagnosticEvent[];
};

const SCOPES = [
	{ id: "", label: "All" },
	{ id: "visuals", label: "Visuals" },
	{ id: "containers", label: "Containers" },
	{ id: "streams", label: "Streams" },
	{ id: "mcp", label: "MCP" },
	{ id: "optimizers", label: "Optimizers" },
	{ id: "providers", label: "Providers" }
] as const;

const WINDOWS = [
	{ id: "20m", label: "20m" },
	{ id: "2h", label: "2h" },
	{ id: "24h", label: "24h" },
	{ id: "7d", label: "7d" }
] as const;

function megabytes(bytes: number | undefined): string {
	if (!bytes) return "0 MB";
	return `${(bytes / (1024 * 1024)).toFixed(bytes < 10 * 1024 * 1024 ? 1 : 0)} MB`;
}

function clockTime(timestamp: string): string {
	const parsed = new Date(timestamp);
	return Number.isNaN(parsed.getTime()) ? timestamp : parsed.toLocaleTimeString();
}

export type DiagnosticsPanelProps = {
	sessionId?: string | null;
	visualId?: string | null;
	onOpenVisual?: (visualId: string) => void;
	onOpenOptimizer?: (optimizerRunId: string) => void;
	onOpenContainer?: (containerId: string) => void;
	onOpenTrace?: (traceId: string) => void;
};

export function DiagnosticsPanel({
	sessionId,
	visualId,
	onOpenVisual,
	onOpenOptimizer,
	onOpenContainer,
	onOpenTrace
}: DiagnosticsPanelProps) {
	const [status, setStatus] = useState<DiagnosticStatus | null>(null);
	const [result, setResult] = useState<DiagnosticResult | null>(null);
	const [scope, setScope] = useState<string>("");
	const [since, setSince] = useState<string>("2h");
	const [scoped, setScoped] = useState(Boolean(sessionId || visualId));
	const [errorsOnly, setErrorsOnly] = useState(true);
	const [selectedCode, setSelectedCode] = useState<string | null>(null);
	const [loading, setLoading] = useState(false);
	const [failure, setFailure] = useState<string | null>(null);
	const [bundlePath, setBundlePath] = useState<string | null>(null);
	const [explanation, setExplanation] = useState<DiagnosticExplanation | null>(null);
	const [explaining, setExplaining] = useState<string | null>(null);
	const generation = useRef(0);

	const query = useMemo(() => {
		const request: Record<string, unknown> = { since, limit: 200 };
		if (scope) request.scope = [scope];
		if (errorsOnly) request.severity = ["error", "warn"];
		if (selectedCode) request.code = [selectedCode];
		if (scoped) {
			if (visualId) request.visual_id = visualId;
			else if (sessionId) request.session_id = sessionId;
		}
		return request;
	}, [scope, since, errorsOnly, selectedCode, scoped, sessionId, visualId]);

	const refresh = useCallback(async () => {
		const ticket = ++generation.current;
		setLoading(true);
		setFailure(null);
		try {
			const [nextStatus, nextResult] = await Promise.all([
				invokeCommand<DiagnosticStatus>(COMMANDS.DIAGNOSTICS_STATUS),
				invokeCommand<DiagnosticResult>(COMMANDS.DIAGNOSTICS_QUERY, { request: query })
			]);
			// A superseded query never writes: closing the pane or changing a
			// filter mid-fetch abandons the answer instead of flashing it.
			if (ticket !== generation.current) return;
			setStatus(nextStatus);
			setResult(nextResult);
		} catch (reason) {
			if (ticket !== generation.current) return;
			setFailure(reason instanceof Error ? reason.message : String(reason));
		} finally {
			if (ticket === generation.current) setLoading(false);
		}
	}, [query]);

	useEffect(() => {
		void refresh();
		return () => {
			generation.current += 1;
		};
	}, [refresh]);

	const copyBundle = useCallback(async () => {
		try {
			const receipt = await invokeCommand<{ path: string }>(COMMANDS.DIAGNOSTICS_BUNDLE, { request: query });
			setBundlePath(receipt.path);
			await navigator.clipboard?.writeText(receipt.path).catch(() => undefined);
		} catch (reason) {
			setFailure(reason instanceof Error ? reason.message : String(reason));
		}
	}, [query]);

	/**
	 * Explain is the operation the system exists for. The pane sends the same
	 * identities the agent would, so a human and an agent get the same answer.
	 */
	const explain = useCallback(async (event: DiagnosticEvent) => {
		const identities: Record<string, string> = {};
		for (const field of ["visual_id", "rollout_id", "stream_id", "container_id", "optimizer_run_id", "trace_id", "session_id"] as const) {
			const value = event[field];
			if (value) identities[field] = value;
		}
		if (Object.keys(identities).length === 0) return;
		setExplaining(event.event_id);
		setExplanation(null);
		try {
			setExplanation(
				await invokeCommand<DiagnosticExplanation>(COMMANDS.DIAGNOSTICS_EXPLAIN, {
					request: { ...identities, since }
				})
			);
		} catch (reason) {
			setFailure(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setExplaining(null);
		}
	}, [since]);

	const clearIndex = useCallback(async () => {
		try {
			await invokeCommand(COMMANDS.DIAGNOSTICS_CLEAR_INDEX);
			await refresh();
		} catch (reason) {
			setFailure(reason instanceof Error ? reason.message : String(reason));
		}
	}, [refresh]);

	const state = status?.state ?? "stopped";

	return (
		<section className="diagnostics-panel" data-testid="diagnostics-panel" aria-label="Diagnostics">
			<header className="diagnostics-status" data-testid="diagnostics-status" data-state={state}>
				<span className={`diagnostics-state diagnostics-state-${state}`}>{state}</span>
				{status?.reason ? <span className="diagnostics-reason">{status.reason}</span> : null}
				<span className="diagnostics-badge">Local only</span>
				<span className="diagnostics-meta">
					{status?.retention_days ?? 7}d · {megabytes(status?.index_bytes)} / {megabytes(status?.quota_bytes)}
				</span>
				<span className="diagnostics-meta">{status?.stored_events ?? 0} events</span>
				{result ? <span className="diagnostics-meta">{result.source}</span> : null}
			</header>

			<div className="diagnostics-filters" role="group" aria-label="Diagnostic filters">
				<select aria-label="Scope" value={scope} onChange={(event) => setScope(event.target.value)}>
					{SCOPES.map((entry) => (
						<option key={entry.id} value={entry.id}>{entry.label}</option>
					))}
				</select>
				<select aria-label="Window" value={since} onChange={(event) => setSince(event.target.value)}>
					{WINDOWS.map((entry) => (
						<option key={entry.id} value={entry.id}>{entry.label}</option>
					))}
				</select>
				<label>
					<input type="checkbox" checked={errorsOnly} onChange={(event) => setErrorsOnly(event.target.checked)} />
					Errors
				</label>
				{sessionId || visualId ? (
					<label>
						<input type="checkbox" checked={scoped} onChange={(event) => setScoped(event.target.checked)} />
						This task
					</label>
				) : null}
				<button type="button" onClick={() => void refresh()} disabled={loading} data-testid="diagnostics-refresh">
					{loading ? "Loading…" : "Refresh"}
				</button>
				<button type="button" onClick={() => void copyBundle()} data-testid="diagnostics-bundle">Copy bundle</button>
				<button type="button" onClick={() => void clearIndex()} data-testid="diagnostics-clear">Clear index</button>
			</div>

			{failure ? <p className="diagnostics-failure" role="alert">{failure}</p> : null}
			{bundlePath ? <p className="diagnostics-bundle-path" data-testid="diagnostics-bundle-path">{bundlePath}</p> : null}

			{selectedCode ? (
				<button type="button" className="diagnostics-clear-code" onClick={() => setSelectedCode(null)}>
					← {selectedCode}
				</button>
			) : (
				<ul className="diagnostics-groups" data-testid="diagnostics-groups">
					{(result?.groups ?? []).map((group) => (
						<li key={group.code}>
							<button type="button" onClick={() => setSelectedCode(group.code)} data-testid={`diagnostics-group-${group.code}`}>
								<span className={`diagnostics-severity diagnostics-severity-${group.severity}`}>{group.severity}</span>
								<span className="diagnostics-code">{group.code}</span>
								<span className="diagnostics-component">{group.component}</span>
								<span className="diagnostics-count">{group.count}</span>
							</button>
						</li>
					))}
				</ul>
			)}

			<ol className="diagnostics-events" data-testid="diagnostics-events">
				{(result?.events ?? []).map((event) => (
					<li key={event.event_id} data-testid="diagnostics-event">
						<div className="diagnostics-event-head">
							<span className={`diagnostics-severity diagnostics-severity-${event.severity}`}>{event.severity}</span>
							<span className="diagnostics-code">{event.code}</span>
							<time dateTime={event.timestamp}>{clockTime(event.timestamp)}</time>
						</div>
						<p className="diagnostics-event-message">{event.message}</p>
						<div className="diagnostics-links">
							<button
								type="button"
								onClick={() => void explain(event)}
								disabled={explaining === event.event_id}
								data-testid="diagnostics-explain"
							>
								{explaining === event.event_id ? "…" : "explain"}
							</button>
							{event.visual_id && onOpenVisual ? (
								<button type="button" onClick={() => onOpenVisual(event.visual_id!)}>visual</button>
							) : null}
							{event.container_id && onOpenContainer ? (
								<button type="button" onClick={() => onOpenContainer(event.container_id!)}>container</button>
							) : null}
							{event.optimizer_run_id && onOpenOptimizer ? (
								<button type="button" onClick={() => onOpenOptimizer(event.optimizer_run_id!)}>optimizer</button>
							) : null}
							{event.trace_id && onOpenTrace ? (
								<button type="button" onClick={() => onOpenTrace(event.trace_id!)}>trace</button>
							) : null}
							{event.rollout_id ? <span className="diagnostics-identity">{event.rollout_id}</span> : null}
						</div>
					</li>
				))}
			</ol>

			{explanation ? (
				<section className="diagnostics-explanation" data-testid="diagnostics-explanation">
					<header>
						<span className="diagnostics-code">{explanation.cause?.code ?? "no cause"}</span>
						<button type="button" onClick={() => setExplanation(null)} aria-label="Dismiss explanation">×</button>
					</header>
					{explanation.cause ? <p className="diagnostics-event-message">{explanation.cause.message}</p> : null}
					{explanation.remediation ? <p className="diagnostics-remediation">{explanation.remediation}</p> : null}
					{explanation.symptoms.length > 0 ? (
						<ul className="diagnostics-symptoms">
							{explanation.symptoms.slice(0, 6).map((symptom) => (
								<li key={symptom.event_id}>
									<span className="diagnostics-code">{symptom.code}</span>
									<span className="diagnostics-component">{symptom.component}</span>
								</li>
							))}
						</ul>
					) : null}
				</section>
			) : null}

			{result && result.count === 0 && !loading ? (
				<p className="diagnostics-empty" data-testid="diagnostics-empty">No diagnostics in this window.</p>
			) : null}
			{result?.truncated ? <p className="diagnostics-meta">Truncated</p> : null}
		</section>
	);
}
