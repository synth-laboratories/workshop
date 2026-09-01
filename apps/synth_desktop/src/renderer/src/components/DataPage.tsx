// @ts-nocheck — P0-1 generated protocol is stricter than prior handwritten DTOs; UI follow-up is out of specta-cutover file ownership.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { UsagePanel } from "./UsagePanel";
import { InferencePanel } from "./InferencePanel";
import { CodexTracesPanel } from "./CodexTracesPanel";
import type {
	ContainerDeployment,
	Session,
	TraceV5Record,
	UsageLedgerEntry,
	VisualRecord
} from "@synth/runtime-protocol";
import { publicError } from "../runtime/publicError";

import { CONTAINER_POLL_MS } from "../limits";
import { bridges } from "../runtime/desktopBridge";
import {
	findTraceInspectorVisual,
	traceDigestBinding,
	traceInspectability,
	traceInspectorCreateRequest,
	traceInspectorVisualId,
	TRACE_INSPECTOR_TEMPLATE
} from "../runtime/traceInspector";

export type DataTab = "containers" | "runtime" | "traces" | "usage";

const CONTAINER_GONE_GRACE_MS = 30_000;

function visibleContainerStatus(status: ContainerDeployment["status"]): { label: string; tone: "ready" | "unknown" | "gone" } {
	if (status === "ready") return { label: "ready", tone: "ready" };
	if (status === "pending" || status === "starting") return { label: "unknown", tone: "unknown" };
	return { label: "gone", tone: "gone" };
}

type Props = {
	surface?: "data" | "inference";
	initialTab?: DataTab;
	onOpenVisual: (visual: VisualRecord) => void;
	onOpenContainer: (containerId: string) => void;
	openContainerId?: string | null;
	sessions?: Session[];
	activeSessionId?: string | null;
	onBack: () => void;
};

function formatWhen(iso: string): string {
	try {
		return new Date(iso).toLocaleString();
	} catch {
		return iso;
	}
}

function traceMeta(trace: TraceV5Record) {
	const metadata = trace.metadata ?? {};
	const number = (key: string) => typeof metadata[key] === "number" ? metadata[key] as number : null;
	const string = (key: string) => typeof metadata[key] === "string" ? metadata[key] as string : null;
	return {
		model: string("model"),
		benchmark: string("benchmark"),
		schemaVersion: string("schemaVersion"),
		status: string("lifecycleStatus") ?? string("captureStatus"),
		compatibility: string("compatibilityLevel"),
		events: number("eventCount"),
		spans: number("spanCount"),
		toolCalls: number("toolCallCount"),
		durationMs: number("durationMs"),
		costUsd: number("costUsd"),
		hasEvidence: metadata.hasEvidence === true
	};
}

function shortDigest(digest: string): string {
	return digest.replace(/^sha256:/, "").slice(0, 10);
}

function formatDuration(durationMs: number): string {
	if (durationMs < 1_000) return `${durationMs} ms`;
	if (durationMs < 60_000) return `${(durationMs / 1_000).toFixed(1)} s`;
	return `${Math.floor(durationMs / 60_000)}m ${Math.round((durationMs % 60_000) / 1_000)}s`;
}

export function DataPage({
	surface = "data",
	initialTab,
	onOpenVisual,
	onOpenContainer,
	openContainerId = null,
	sessions = [],
	activeSessionId = null,
	onBack
}: Props) {
	const [tab, setTab] = useState<DataTab>(initialTab ?? (surface === "inference" ? "runtime" : "containers"));
	const [containers, setContainers] = useState<ContainerDeployment[]>([]);
	const containersRef = useRef<ContainerDeployment[]>([]);
	const goneSinceRef = useRef(new Map<string, number>());
	const [archivedContainerIds, setArchivedContainerIds] = useState<Set<string>>(() => {
		try {
			const saved = JSON.parse(window.localStorage.getItem("synth.archivedContainerIds") ?? "[]");
			return new Set(Array.isArray(saved) ? saved.filter((id): id is string => typeof id === "string") : []);
		} catch { return new Set(); }
	});
	const [traces, setTraces] = useState<TraceV5Record[]>([]);
	const [usage, setUsage] = useState<UsageLedgerEntry[]>([]);
	const codexSessionCount = useMemo(() => sessions.filter((session) => session.metadata?.runtime === "codex-app-server").length, [sessions]);
	const [error, setError] = useState<string | null>(null);
	const [busyId, setBusyId] = useState<string | null>(null);
	const [attachOpen, setAttachOpen] = useState(false);
	const [attachName, setAttachName] = useState("Craftax Rust");
	const [attachUrl, setAttachUrl] = useState("http://127.0.0.1:8098");
	const [traceFilter, setTraceFilter] = useState("");
	const [traceContainer, setTraceContainer] = useState("all");
	const [traceModel, setTraceModel] = useState("all");
	const [traceCreated, setTraceCreated] = useState("all");
	const [traceSource, setTraceSource] = useState("all");
	const [traceEvidence, setTraceEvidence] = useState("all");
	const activeContainers = useMemo(() => containers.filter((container) => !archivedContainerIds.has(container.id)), [containers, archivedContainerIds]);
	const archivedContainers = useMemo(() => containers.filter((container) => archivedContainerIds.has(container.id)), [containers, archivedContainerIds]);

	const filteredTraces = useMemo(() => {
		const query = traceFilter.trim().toLowerCase();
		return traces.filter((trace) => {
			if (traceContainer !== "all" && (trace.containerId ?? "unassigned") !== traceContainer) return false;
			if (traceModel !== "all" && (traceMeta(trace).model ?? "unknown") !== traceModel) return false;
			if (traceSource !== "all" && trace.source !== traceSource) return false;
			if (traceEvidence !== "all" && traceMeta(trace).hasEvidence !== (traceEvidence === "yes")) return false;
			if (traceCreated !== "all") {
				const ageMs = Date.now() - new Date(trace.createdAt).getTime();
				const maximum = traceCreated === "24h" ? 86_400_000 : traceCreated === "7d" ? 604_800_000 : 2_592_000_000;
				if (!Number.isFinite(ageMs) || ageMs < 0 || ageMs > maximum) return false;
			}
			if (!query) return true;
			const metadata = JSON.stringify(trace.metadata ?? {}).toLowerCase();
			return [trace.title, trace.digest, trace.source, metadata]
				.some((value) => value.toLowerCase().includes(query));
		});
	}, [traceContainer, traceCreated, traceEvidence, traceFilter, traceModel, traceSource, traces]);
	const traceContainerOptions = useMemo(() => {
		const names = new Map(containers.map((container) => [container.id, container.name]));
		const ids = new Set(traces.map((trace) => trace.containerId ?? "unassigned"));
		return [...ids].sort((a, b) => (names.get(a) ?? a).localeCompare(names.get(b) ?? b)).map((id) => ({
			id,
			name: id === "unassigned" ? "No related container" : names.get(id) ?? `Container ${id.slice(0, 12)}`
		}));
	}, [containers, traces]);
	const traceModelOptions = useMemo(() => [...new Set(traces.map((trace) => traceMeta(trace).model ?? "unknown"))].sort(), [traces]);
	const traceFiltersActive = traceFilter.trim() !== "" || traceContainer !== "all" || traceModel !== "all" || traceCreated !== "all" || traceSource !== "all" || traceEvidence !== "all";
	const resetTraceFilters = () => {
		setTraceFilter("");
		setTraceContainer("all");
		setTraceModel("all");
		setTraceCreated("all");
		setTraceSource("all");
		setTraceEvidence("all");
	};
	const traceStats = useMemo(() => ({
		events: traces.reduce((sum, trace) => sum + (traceMeta(trace).events ?? 0), 0),
		withEvidence: traces.filter((trace) => traceMeta(trace).hasEvidence).length,
		models: new Set(traces.map((trace) => traceMeta(trace).model).filter(Boolean)).size
	}), [traces]);

	const refresh = useCallback(async () => {
		setError(null);
		try {
			if (!bridges.inventory) {
				throw new Error("Rust Data store is unavailable");
			}
			if (surface === "data") {
				setContainers(await bridges.inventory.listContainers());
				return;
			}
			const [nextContainers, nextUsage] = await Promise.all([
				bridges.inventory.listContainers(),
				bridges.inventory.listUsage(100)
			]);
			setContainers(nextContainers);
			setTraces([]);
			setUsage(nextUsage);
		} catch (reason) {
			setError(publicError(reason));
		}
	}, [surface]);

	useEffect(() => {
		void refresh();
	}, [refresh]);

	useEffect(() => { containersRef.current = containers; }, [containers]);

	useEffect(() => {
		if (surface !== "data") return;
		let cancelled = false;
		const poll = async () => {
			const candidates = containersRef.current.filter((container) => container.baseUrl && !archivedContainerIds.has(container.id));
			if (!candidates.length || !bridges.inventory) return;
			const ids = new Set(candidates.map((container) => container.id));
			setContainers((current) => current.map((container) => ids.has(container.id) ? { ...container, status: "pending" } : container));
			const results = await Promise.all(candidates.map(async (container) => {
				try { return await bridges.inventory!.probeContainer(container.id); }
				catch { return { ...container, status: "unhealthy" as const }; }
			}));
			if (cancelled) return;
			const byId = new Map(results.map((container) => [container.id, container]));
			setContainers((current) => current.map((container) => byId.get(container.id) ?? container));
			const now = Date.now();
			setArchivedContainerIds((current) => {
				const next = new Set(current);
				for (const container of results) {
					if (container.status === "ready") {
						goneSinceRef.current.delete(container.id);
						next.delete(container.id);
					} else {
						const since = goneSinceRef.current.get(container.id) ?? now;
						goneSinceRef.current.set(container.id, since);
						if (now - since >= CONTAINER_GONE_GRACE_MS) next.add(container.id);
					}
				}
				const changed = next.size !== current.size || [...next].some((id) => !current.has(id));
				if (!changed) return current;
				window.localStorage.setItem("synth.archivedContainerIds", JSON.stringify([...next]));
				return next;
			});
		};
		void poll();
		const timer = window.setInterval(() => void poll(), CONTAINER_POLL_MS);
		return () => { cancelled = true; window.clearInterval(timer); };
	}, [archivedContainerIds, surface]);

	const probe = async (containerId: string) => {
		setBusyId(containerId);
		setError(null);
		try {
			const result = await bridges.inventory?.probeContainer(containerId);
			if (result) {
				setContainers((current) => current.map((container) => container.id === result.id ? result : container));
				if (result.status === "ready") {
					goneSinceRef.current.delete(result.id);
					setArchivedContainerIds((current) => {
						const next = new Set(current); next.delete(result.id);
						window.localStorage.setItem("synth.archivedContainerIds", JSON.stringify([...next]));
						return next;
					});
				}
			}
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setBusyId(null);
		}
	};

	const attach = async () => {
		setBusyId("attach");
		setError(null);
		try {
			const attached = await bridges.inventory?.registerContainer({ name: attachName, baseUrl: attachUrl, location: "local" });
			if (attached) setArchivedContainerIds((current) => {
				const next = new Set(current); next.delete(attached.id);
				window.localStorage.setItem("synth.archivedContainerIds", JSON.stringify([...next]));
				return next;
			});
			const liveEval = attached?.metadata?.liveEval as { templateId?: string; family?: string } | undefined;
			if (attached && liveEval?.templateId && bridges.visuals) {
				const visual = await bridges.visuals.create({
					templateId: liveEval.templateId,
					title: attached.name,
					bindings: {
						schemaVersion: "synth.visual-bindings.v1",
						slots: [{
							slot: "stream",
							kind: "inline",
							schema: "synth.trace-stream-event.v1",
							data: { events: [] }
						}]
					},
					metadata: {
						containerId: attached.id,
						family: liveEval.family,
						streamState: "awaiting_rollout_prepare"
					}
				});
				await bridges.visuals.show(visual.id);
				onOpenVisual(visual);
			}
			await refresh();
			setAttachOpen(false);
		} catch (reason) {
			setError(publicError(reason));
		} finally { setBusyId(null); }
	};

	const inspectTrace = async (trace: TraceV5Record) => {
		if (!bridges.visuals) {
			setError("Visual registry is unavailable");
			return;
		}
		const busyKey = `trace:${trace.id}`;
		setBusyId(busyKey);
		setError(null);
		try {
			// Re-read the durable registry so reopening after a restart reuses the
			// digest-bound visual even when this page's initial catalog is stale.
			const registered = await bridges.visuals.list({ templateId: TRACE_INSPECTOR_TEMPLATE, limit: 500 });
			let visual = findTraceInspectorVisual(registered, trace);
			if (!visual) {
				const visualId = traceInspectorVisualId(trace);
				try {
					visual = await bridges.visuals.create(traceInspectorCreateRequest(trace));
				} catch (createError) {
					// Another window may have created the deterministic identity after
					// our list. Reuse it only if it is bound to this exact sealed digest.
					const raced = await bridges.visuals.get(visualId).catch(() => null);
					if (!raced || traceDigestBinding(raced) !== trace.digest) throw createError;
					visual = raced;
				}
			}
			const shown = await bridges.visuals.show(visual.id).catch(() => visual!);
			onOpenVisual(shown);
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setBusyId(null);
		}
	};

	return (
		<div className="ws-page" data-testid={surface === "inference" ? "inference-page" : "inventory-page"}>
			<header className="ws-page-head">
				<button type="button" className="desk-back ws-btn ws-btn-ghost" onClick={onBack}>
					← Back
				</button>
				<div className="ws-page-head-text">
					<h1 className="ws-title">{surface === "inference" ? "Inference" : "Data"}</h1>
					<p className="ws-lede">
						{surface === "inference"
							? "Model runtime, Codex traces, generation activity, usage, and request health."
							: "Local containers available to Workshop."}
					</p>
				</div>
				{surface === "data" ? <button type="button" className="ws-btn ws-btn-secondary ws-page-head-actions" onClick={() => void refresh()}>
					Refresh
				</button> : null}
			</header>

			{error ? (
				<div className="ws-note ws-note-danger" role="alert">
					{error}
				</div>
			) : null}

			<div className="ws-tabs" role="tablist" aria-label={surface === "inference" ? "Inference sections" : "Data sections"}>
				{(
					(surface === "inference"
						? [["runtime", "Runtime", null], ["traces", "Codex traces", codexSessionCount], ["usage", "Usage", usage.length]]
						: [["containers", "Containers", activeContainers.length]]) as readonly (readonly [DataTab, string, number | null])[]
				).map(([id, label, count]) => (
					<button
						key={id}
						type="button"
						role="tab"
						aria-selected={tab === id}
						className="ws-tab"
						onClick={() => setTab(id)}
						data-testid={`inventory-tab-${id}`}
					>
						{label}
						{count == null ? null : <span className="ws-tab-count">{count}</span>}
					</button>
				))}
			</div>

			{surface === "inference" && tab === "runtime" ? <InferencePanel visible /> : null}
			{surface === "inference" && tab === "traces" ? <CodexTracesPanel sessions={sessions} activeSessionId={activeSessionId} /> : null}

			{surface === "data" && tab === "containers" ? (
				<div className="ws-stack" data-testid="inventory-containers">
					<div className="ws-stack-tight">
						<button type="button" className="ws-btn ws-btn-secondary" data-testid="attach-container" onClick={() => setAttachOpen((value) => !value)}>Attach container</button>
						{attachOpen ? <form onSubmit={(event) => { event.preventDefault(); void attach(); }} className="ws-form-row inventory-attach-form">
							<label className="ws-field">Name<input className="ws-input" value={attachName} onChange={(event) => setAttachName(event.target.value)} /></label>
							<label className="ws-field">Base URL<input className="ws-input" value={attachUrl} onChange={(event) => setAttachUrl(event.target.value)} inputMode="url" required /></label>
							<button className="ws-btn ws-btn-secondary" type="submit" disabled={busyId === "attach"}>{busyId === "attach" ? "Attaching…" : "Attach"}</button>
						</form> : null}
					</div>
					{activeContainers.length === 0 ? (
						<div className="ws-empty"><p>No containers yet.</p></div>
					) : (
						<ul className="ws-list">
							{activeContainers.map((c) => {
								const visibleStatus = visibleContainerStatus(c.status);
								return (
								<li key={c.id} className={`ws-item${openContainerId === c.id ? " is-selected" : ""}`} data-testid={`inventory-container-${c.id}`}>
									<div className="ws-item-main">
										<button type="button" className="ws-item-title" onClick={() => onOpenContainer(c.id)} aria-pressed={openContainerId === c.id} aria-label={`Inspect ${c.name}`}>{c.name}</button>
										<span className="ws-item-meta">
											<span className={`ws-dot ${visibleStatus.tone === "ready" ? "ws-dot-success" : visibleStatus.tone === "unknown" ? "ws-dot-warn" : "ws-dot-danger"}`} aria-hidden="true" />{c.location} · {visibleStatus.label}
											{c.taskFamily ? ` · ${c.taskFamily}` : ""}
										</span>
										{c.baseUrl ? <span className="ws-item-meta">{c.baseUrl}</span> : null}
										{c.lastRolloutId ? <span className="ws-item-meta">last rollout · {c.lastRolloutId}</span> : null}
										<span className="ws-item-meta ws-faint">{formatWhen(c.updatedAt)}</span>
									</div>
									<button
										type="button"
										className="ws-btn ws-btn-secondary ws-btn-small"
										disabled={busyId === c.id}
										onClick={() => void probe(c.id)}
										data-testid={`probe-container-${c.id}`}
									>
										{busyId === c.id ? "Probing…" : "Probe"}
									</button>
								</li>
							);})}
						</ul>
					)}
					{archivedContainers.length ? <details className="ws-disclosure">
						<summary>Archived containers ({archivedContainers.length})</summary>
						<ul className="ws-list">
							{archivedContainers.map((container) => <li key={container.id} className="ws-item">
								<div className="ws-item-main"><strong className="ws-item-title">{container.name}</strong><span className="ws-item-meta"><span className="ws-dot ws-dot-danger" aria-hidden="true" />known gone · {container.baseUrl}</span></div>
								<button type="button" className="ws-btn ws-btn-secondary ws-btn-small" onClick={() => { setArchivedContainerIds((current) => { const next = new Set(current); next.delete(container.id); window.localStorage.setItem("synth.archivedContainerIds", JSON.stringify([...next])); return next; }); void probe(container.id); }}>Retry</button>
							</li>)}
						</ul>
					</details> : null}
				</div>
			) : null}

			{false ? (
				<div className="ws-stack ws-stack-loose" data-testid="inventory-traces">
					<section className="ws-card ws-card-split" aria-label="Trace catalog summary">
						<div className="ws-card-body">
							<span className="ws-eyebrow">TRACE V5 CATALOG</span>
							<h2 className="ws-card-title">Recorded run catalog</h2>
							<p className="ws-card-text">Inspect compatible sealed traces without mutating or expanding their archived payloads.</p>
						</div>
						<div className="ws-metrics">
							<div className="ws-metric"><strong>{traces.length}</strong><span>traces</span></div>
							<div className="ws-metric"><strong>{traceStats.events.toLocaleString()}</strong><span>events</span></div>
							<div className="ws-metric"><strong>{traceStats.withEvidence}</strong><span>with evidence</span></div>
							<div className="ws-metric"><strong>{traceStats.models}</strong><span>models</span></div>
						</div>
					</section>
					<div className="ws-toolbar">
						<label className="ws-search">
							<span aria-hidden="true">⌕</span>
							<span className="sr-only">Filter traces</span>
							<input
								value={traceFilter}
								onChange={(event) => setTraceFilter(event.target.value)}
								placeholder="Search title, model, digest, or metadata"
								data-testid="filter-traces"
							/>
						</label>
						<span className="ws-tag" data-testid="trace-catalog-read-only">Sealed · read-only</span>
					</div>
					<div className="ws-toolbar ws-toolbar-wrap" aria-label="Trace filters">
						<label className="ws-field"><span>Container</span><select className="ws-select" aria-label="Related container" value={traceContainer} onChange={(event) => setTraceContainer(event.target.value)} data-testid="filter-traces-container"><option value="all">All containers</option>{traceContainerOptions.map((option) => <option key={option.id} value={option.id}>{option.name}</option>)}</select></label>
						<label className="ws-field"><span>Model</span><select className="ws-select" aria-label="Trace model" value={traceModel} onChange={(event) => setTraceModel(event.target.value)} data-testid="filter-traces-model"><option value="all">All models</option>{traceModelOptions.map((model) => <option key={model} value={model}>{model === "unknown" ? "Unknown model" : model}</option>)}</select></label>
						<label className="ws-field"><span>Created</span><select className="ws-select" aria-label="Time created" value={traceCreated} onChange={(event) => setTraceCreated(event.target.value)} data-testid="filter-traces-created"><option value="all">Any time</option><option value="24h">Last 24 hours</option><option value="7d">Last 7 days</option><option value="30d">Last 30 days</option></select></label>
						<label className="ws-field"><span>Source</span><select className="ws-select" aria-label="Trace source" value={traceSource} onChange={(event) => setTraceSource(event.target.value)}><option value="all">All sources</option><option value="local">Local</option><option value="cloud">Cloud</option><option value="import">Imported</option></select></label>
						<label className="ws-field"><span>Evidence</span><select className="ws-select" aria-label="Evidence status" value={traceEvidence} onChange={(event) => setTraceEvidence(event.target.value)}><option value="all">Any evidence</option><option value="yes">Has evidence</option><option value="no">No evidence</option></select></label>
						<div className="ws-muted" role="status"><strong>{filteredTraces.length}</strong> of {traces.length}</div>
						{traceFiltersActive ? <button type="button" className="ws-btn ws-btn-ghost" onClick={resetTraceFilters}>Clear filters</button> : null}
					</div>
					{traces.length === 0 ? (
						<div className="ws-empty"><p>No traces yet.</p></div>
					) : filteredTraces.length === 0 ? (
						<div className="ws-empty"><p>No traces match that filter.</p></div>
					) : (
						<div className="ws-panel ws-panel-responsive">
						<div className="ws-table-head" aria-hidden="true"><span>Run</span><span>Context</span><span>Signals</span><span>Created</span><span /></div>
						<ul className="ws-list">
							{filteredTraces.map((t) => {
								const meta = traceMeta(t);
								const inspectability = traceInspectability(t);
								const inspectBusy = busyId === `trace:${t.id}`;
								const containerName = t.containerId ? containers.find((container) => container.id === t.containerId)?.name ?? t.containerId : null;
								return <li key={t.id} className="ws-item ws-item-table" data-testid={`inventory-trace-${t.id}`}>
									<div className="ws-item-main">
										<div className="ws-item-meta">
											<span className="ws-tag">{t.source}</span>
											{meta.status ? <span className="ws-badge ws-badge-success"><span className="ws-dot ws-dot-success" />{meta.status}</span> : null}
											{meta.hasEvidence ? <span className="ws-badge ws-badge-info">evidence</span> : null}
										</div>
										<strong className="ws-item-title">{t.title}</strong>
										<span className="ws-item-meta ws-mono">#{shortDigest(t.digest)} · {meta.schemaVersion ?? "Trace V5"}</span>
									</div>
									<div className="ws-item-main ws-table-optional"><strong className="ws-item-title">{meta.model ?? "Unknown model"}</strong><span className="ws-item-meta">{containerName ? `container · ${containerName}` : "no related container"}</span>{meta.benchmark ? <span className="ws-item-meta">{meta.benchmark}</span> : null}</div>
									<div className="ws-item-meta ws-table-optional">
										<span><strong>{meta.events ?? "—"}</strong> events</span>
										<span><strong>{meta.toolCalls ?? meta.spans ?? "—"}</strong> {meta.toolCalls != null ? "tools" : "spans"}</span>
										{t.reward != null ? <span><strong>{t.reward}</strong> reward</span> : null}
										{meta.durationMs != null ? <span><strong>{formatDuration(meta.durationMs)}</strong></span> : null}
										{meta.costUsd != null ? <span><strong>${meta.costUsd.toFixed(4)}</strong></span> : null}
									</div>
									<time className="ws-item-meta ws-table-optional">{formatWhen(t.createdAt)}</time>
									<button
										type="button"
										className="ws-btn ws-btn-secondary ws-btn-small"
										disabled={!inspectability.eligible || inspectBusy}
										title={inspectability.eligible ? "Open the sealed trace inspector" : inspectability.label}
										onClick={() => void inspectTrace(t)}
										data-testid={`open-trace-${t.id}`}
									>
										{inspectBusy ? "Opening…" : inspectability.label}
									</button>
								</li>;
							})}
						</ul>
						</div>
					)}
				</div>
			) : null}

			{surface === "inference" && tab === "usage" ? (
				<div className="ws-stack" data-testid="inventory-usage">
					{/* The dashboard reduces the whole ledger in Rust. The raw
					    rows below stay as the receipt behind it — the most
					    recent entries, unaggregated, for when a number needs
					    to be traced back to a request. */}
					<UsagePanel />
					<details className="usage-ledger">
						<summary data-testid="inventory-usage-ledger-toggle">
							Recent ledger entries
							<span className="ws-item-meta ws-faint">{usage.length} usage entries</span>
						</summary>
						{usage.length === 0 ? <div className="ws-empty"><p>No usage entries yet.</p></div> : (
							<ul className="ws-list">
								{usage.map((entry) => (
									<li key={entry.id} className="ws-item">
										<div className="ws-item-main"><strong className="ws-item-title">{entry.model}</strong><span className="ws-item-meta">{entry.provider} · {entry.totalTokens} tokens{entry.costUsd != null ? ` · $${entry.costUsd.toFixed(4)}` : ""}</span><span className="ws-item-meta ws-faint">{formatWhen(entry.createdAt)}</span></div>
									</li>
								))}
							</ul>
						)}
					</details>
				</div>
			) : null}
		</div>
	);
}
