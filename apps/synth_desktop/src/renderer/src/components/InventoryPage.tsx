import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { InferencePanel } from "./InferencePanel";
import type {
	ContainerDeployment,
	TraceV5Record,
	UsageLedgerEntry,
	VisualRecord
} from "@synth/runtime-protocol";

import { CONTAINER_POLL_MS } from "../limits";

export type InventoryTab = "containers" | "traces" | "visuals" | "usage" | "inference";

const CONTAINER_GONE_GRACE_MS = 30_000;

function visibleContainerStatus(status: ContainerDeployment["status"]): { label: string; tone: "ready" | "unknown" | "gone" } {
	if (status === "ready") return { label: "ready", tone: "ready" };
	if (status === "pending" || status === "starting") return { label: "unknown", tone: "unknown" };
	return { label: "gone", tone: "gone" };
}

type Props = {
	initialTab?: InventoryTab;
	onOpenVisual: (visual: VisualRecord) => void;
	onOpenContainer: (containerId: string) => void;
	openContainerId?: string | null;
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

export function InventoryPage({
	initialTab = "containers",
	onOpenVisual,
	onOpenContainer,
	openContainerId = null,
	onBack
}: Props) {
	const [tab, setTab] = useState<InventoryTab>(initialTab);
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
	const [visuals, setVisuals] = useState<VisualRecord[]>([]);
	const [usage, setUsage] = useState<UsageLedgerEntry[]>([]);
	const [counts, setCounts] = useState({ containers: 0, traces: 0, usage: 0 });
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
	const [traceNotice, setTraceNotice] = useState<string | null>(null);
	const [selectedTraceId, setSelectedTraceId] = useState<string | null>(null);
	const [traceErrors, setTraceErrors] = useState<Record<string, string>>({});
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
			if (!window.synthInventory || !window.synthVisuals) {
				throw new Error("Rust inventory is unavailable");
			}
			const [nextContainers, nextTraces, nextVisuals, nextUsage, nextCounts] = await Promise.all([
				window.synthInventory.listContainers(),
				window.synthInventory.listTraces(),
				window.synthVisuals.list({ limit: 500 }),
				window.synthInventory.listUsage(100),
				window.synthInventory.counts()
			]);
			setContainers(nextContainers);
			setTraces(nextTraces);
			setVisuals(nextVisuals);
			setUsage(nextUsage);
			setCounts(nextCounts);
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		}
	}, []);

	useEffect(() => {
		void refresh();
	}, [refresh]);

	useEffect(() => { containersRef.current = containers; }, [containers]);

	useEffect(() => {
		let cancelled = false;
		const poll = async () => {
			const candidates = containersRef.current.filter((container) => container.baseUrl && !archivedContainerIds.has(container.id));
			if (!candidates.length || !window.synthInventory) return;
			const ids = new Set(candidates.map((container) => container.id));
			setContainers((current) => current.map((container) => ids.has(container.id) ? { ...container, status: "pending" } : container));
			const results = await Promise.all(candidates.map(async (container) => {
				try { return await window.synthInventory!.probeContainer(container.id); }
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
	}, [archivedContainerIds]);

	const probe = async (containerId: string) => {
		setBusyId(containerId);
		setError(null);
		try {
			const result = await window.synthInventory?.probeContainer(containerId);
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
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setBusyId(null);
		}
	};

	const attach = async () => {
		setBusyId("attach");
		setError(null);
		try {
			const attached = await window.synthInventory?.registerContainer({ name: attachName, baseUrl: attachUrl, location: "local" });
			if (attached) setArchivedContainerIds((current) => {
				const next = new Set(current); next.delete(attached.id);
				window.localStorage.setItem("synth.archivedContainerIds", JSON.stringify([...next]));
				return next;
			});
			await refresh();
			setAttachOpen(false);
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally { setBusyId(null); }
	};

	const importTrace = async () => {
		setError(null);
		setTraceNotice(null);
		const sourcePath = await window.synthInventory?.chooseTraceInput();
		if (!sourcePath) return;
		setBusyId("trace-import");
		try {
			const result = await window.synthInventory?.ingestTraceBundle({
				sourcePath,
				sourceKind: "desktop_picker"
			});
			if (!result) throw new Error("Trace import did not return a result");
			setTraceNotice(result.trusted
				? `${result.duplicate ? "Already imported" : "Imported"} ${result.traces.length} trusted trace${result.traces.length === 1 ? "" : "s"}.`
				: `Input quarantined (${result.compatibilityLevel}); it was not added to the trusted trace catalog.`);
			await refresh();
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setBusyId(null);
		}
	};

	const openTrace = async (trace: TraceV5Record) => {
		setBusyId(trace.id);
		setTraceErrors((current) => { const next = { ...current }; delete next[trace.id]; return next; });
		try {
			if (!window.synthInventory || !window.synthVisuals) {
				throw new Error("Trace visuals are unavailable");
			}
			let resolvedTrace = trace;
			let projection;
			try {
				projection = await window.synthInventory.resolveTraceProjection(resolvedTrace.digest);
			} catch (reason) {
				const message = reason instanceof Error ? reason.message : String(reason);
				if (!trace.path || !message.includes("trusted Trace V5 archive not found")) throw reason;
				const migrated = await window.synthInventory.ingestTraceBundle({ sourcePath: trace.path, sourceKind: "legacy_catalog_open", title: trace.title });
				if (!migrated.trusted || !migrated.traces.length) {
					throw new Error(`Legacy record is not an inspectable Trace V5 bundle (${migrated.compatibilityLevel}). Re-import its source archive to migrate it.`);
				}
				resolvedTrace = migrated.traces[0];
				projection = await window.synthInventory.resolveTraceProjection(resolvedTrace.digest);
				await refresh();
			}
			const visualId = `tracevis_${resolvedTrace.digest.replace(/^sha256:/, "")}`;
			const bindings = {
				schemaVersion: "synth.visual-bindings.v1" as const,
				slots: [{
					slot: "projection",
					kind: "inline",
					schema: projection.projectionSchema,
					data: projection.payload
				}]
			};
			const metadata = {
				traceDigest: resolvedTrace.digest,
				projectionDigest: projection.payloadDigest,
				projectionSchema: projection.projectionSchema
			};

			let existing: VisualRecord | null = null;
			try {
				existing = await window.synthVisuals.get(visualId);
			} catch {
				// A missing deterministic id is the normal first-open path.
			}
			const visual = existing
				? existing.metadata?.projectionDigest === projection.payloadDigest
					? existing
					: await window.synthVisuals.update(visualId, {
						title: resolvedTrace.title,
						traceId: resolvedTrace.id,
						bindings,
						metadata,
						bumpRevision: true
					})
				: await window.synthVisuals.create({
					id: visualId,
					templateId: "trace.rollout_inspector.v1",
					title: resolvedTrace.title,
					traceId: resolvedTrace.id,
					bindings,
					metadata
				});
			setSelectedTraceId(trace.id);
			onOpenVisual(visual);
		} catch (reason) {
			setTraceErrors((current) => ({ ...current, [trace.id]: reason instanceof Error ? reason.message : String(reason) }));
		} finally {
			setBusyId(null);
		}
	};


	return (
		<div className="inventory-page" data-testid="inventory-page">
			<header className="inventory-head">
				<button type="button" className="desk-back" onClick={onBack}>
					← Back
				</button>
				<div>
					<h1>Inventory</h1>
					<p className="inventory-lede">
						Local containers, Trace V5 records, and visual instances from the runtime vault.
					</p>
				</div>
				<button type="button" className="inventory-refresh" onClick={() => void refresh()}>
					Refresh
				</button>
			</header>

			{error ? (
				<div className="inventory-error" role="alert">
					{error}
				</div>
			) : null}

			<div className="inventory-tabs" role="tablist" aria-label="Inventory sections">
				{(
					[
						["containers", "Containers", activeContainers.length],
						["traces", "Traces", traces.length],
						["visuals", "Visuals", visuals.length]
						,["usage", "Usage", usage.length]
						,["inference", "Inference", null]
					] as const
				).map(([id, label, count]) => (
					<button
						key={id}
						type="button"
						role="tab"
						aria-selected={tab === id}
						className={`inventory-tab${tab === id ? " active" : ""}`}
						onClick={() => setTab(id)}
						data-testid={`inventory-tab-${id}`}
					>
						{label}
						{count == null ? null : <span className="inventory-tab-count">{count}</span>}
					</button>
				))}
			</div>

			{tab === "inference" ? (
				<div className="inventory-panel" data-testid="inventory-inference">
					{/* The panel owns its own subscription and only runs while it
					    is the selected tab. */}
					<InferencePanel visible />
				</div>
			) : null}

			{tab === "containers" ? (
				<div className="inventory-panel" data-testid="inventory-containers">
					<div className="inventory-container-tools">
						<button type="button" className="inventory-row-action" data-testid="attach-container" onClick={() => setAttachOpen((value) => !value)}>Attach container</button>
						{attachOpen ? <form onSubmit={(event) => { event.preventDefault(); void attach(); }} className="inventory-attach-form">
							<label>Name<input value={attachName} onChange={(event) => setAttachName(event.target.value)} /></label>
							<label>Base URL<input value={attachUrl} onChange={(event) => setAttachUrl(event.target.value)} inputMode="url" required /></label>
							<button type="submit" disabled={busyId === "attach"}>{busyId === "attach" ? "Attaching…" : "Attach"}</button>
						</form> : null}
					</div>
					{activeContainers.length === 0 ? (
						<p className="inventory-empty">No containers yet.</p>
					) : (
						<ul className="inventory-list">
							{activeContainers.map((c) => {
								const visibleStatus = visibleContainerStatus(c.status);
								return (
								<li key={c.id} className={`inventory-row inventory-container-row${openContainerId === c.id ? " selected" : ""}`} data-testid={`inventory-container-${c.id}`}>
									<div className="inventory-row-main">
										<button type="button" className="inventory-container-name" onClick={() => onOpenContainer(c.id)} aria-pressed={openContainerId === c.id} aria-label={`Inspect ${c.name}`}><strong>{c.name}</strong></button>
										<span className="inventory-row-meta">
											<span className={`inventory-status-dot ${visibleStatus.tone}`} aria-hidden="true" />{c.location} · {visibleStatus.label}
											{c.taskFamily ? ` · ${c.taskFamily}` : ""}
										</span>
										{c.baseUrl ? <span className="inventory-row-meta">{c.baseUrl}</span> : null}
										{c.lastRolloutId ? <span className="inventory-row-meta">last rollout · {c.lastRolloutId}</span> : null}
										<span className="inventory-row-when">{formatWhen(c.updatedAt)}</span>
									</div>
									<button
										type="button"
										className="inventory-row-action"
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
					{archivedContainers.length ? <details className="inventory-archived-containers">
						<summary>Archived containers ({archivedContainers.length})</summary>
						<ul className="inventory-list">
							{archivedContainers.map((container) => <li key={container.id} className="inventory-row inventory-container-row archived">
								<div className="inventory-row-main"><strong>{container.name}</strong><span className="inventory-row-meta"><span className="inventory-status-dot gone" aria-hidden="true" />known gone · {container.baseUrl}</span></div>
								<button type="button" className="inventory-row-action" onClick={() => { setArchivedContainerIds((current) => { const next = new Set(current); next.delete(container.id); window.localStorage.setItem("synth.archivedContainerIds", JSON.stringify([...next])); return next; }); void probe(container.id); }}>Retry</button>
							</li>)}
						</ul>
					</details> : null}
				</div>
			) : null}

			{tab === "traces" ? (
				<div className="inventory-panel inventory-traces-panel" data-testid="inventory-traces">
					<section className="trace-catalog-hero" aria-label="Trace catalog summary">
						<div>
							<span className="trace-eyebrow">TRACE V5 CATALOG</span>
							<h2>Inspect runs, not files</h2>
							<p>Select a trace to open its event timeline, tool output, evidence, usage, and provenance in the right pane.</p>
						</div>
						<div className="trace-summary-metrics">
							<div><strong>{traces.length}</strong><span>traces</span></div>
							<div><strong>{traceStats.events.toLocaleString()}</strong><span>events</span></div>
							<div><strong>{traceStats.withEvidence}</strong><span>with evidence</span></div>
							<div><strong>{traceStats.models}</strong><span>models</span></div>
						</div>
					</section>
					<div className="trace-catalog-tools">
						<label className="trace-search">
							<span aria-hidden="true">⌕</span>
							<span className="sr-only">Filter traces</span>
							<input
								value={traceFilter}
								onChange={(event) => setTraceFilter(event.target.value)}
								placeholder="Search title, model, digest, or metadata"
								data-testid="filter-traces"
							/>
						</label>
						<button
							type="button"
							className="trace-import-action"
							disabled={busyId === "trace-import"}
							onClick={() => void importTrace()}
							data-testid="import-trace-v5"
						>
							{busyId === "trace-import" ? "Importing…" : "+ Import Trace V5"}
						</button>
					</div>
					<div className="trace-filter-bar" aria-label="Trace filters">
						<label><span>Container</span><select aria-label="Related container" value={traceContainer} onChange={(event) => setTraceContainer(event.target.value)} data-testid="filter-traces-container"><option value="all">All containers</option>{traceContainerOptions.map((option) => <option key={option.id} value={option.id}>{option.name}</option>)}</select></label>
						<label><span>Model</span><select aria-label="Trace model" value={traceModel} onChange={(event) => setTraceModel(event.target.value)} data-testid="filter-traces-model"><option value="all">All models</option>{traceModelOptions.map((model) => <option key={model} value={model}>{model === "unknown" ? "Unknown model" : model}</option>)}</select></label>
						<label><span>Created</span><select aria-label="Time created" value={traceCreated} onChange={(event) => setTraceCreated(event.target.value)} data-testid="filter-traces-created"><option value="all">Any time</option><option value="24h">Last 24 hours</option><option value="7d">Last 7 days</option><option value="30d">Last 30 days</option></select></label>
						<label><span>Source</span><select aria-label="Trace source" value={traceSource} onChange={(event) => setTraceSource(event.target.value)}><option value="all">All sources</option><option value="local">Local</option><option value="cloud">Cloud</option><option value="import">Imported</option></select></label>
						<label><span>Evidence</span><select aria-label="Evidence status" value={traceEvidence} onChange={(event) => setTraceEvidence(event.target.value)}><option value="all">Any evidence</option><option value="yes">Has evidence</option><option value="no">No evidence</option></select></label>
						<div className="trace-filter-result" role="status"><strong>{filteredTraces.length}</strong> of {traces.length}</div>
						{traceFiltersActive ? <button type="button" className="trace-filter-reset" onClick={resetTraceFilters}>Clear filters</button> : null}
					</div>
					{traceNotice ? <p className="inventory-row-meta" role="status">{traceNotice}</p> : null}
					{traces.length === 0 ? (
						<p className="inventory-empty">No traces yet.</p>
					) : filteredTraces.length === 0 ? (
						<p className="inventory-empty">No traces match that filter.</p>
					) : (
						<div className="trace-table-shell">
						<div className="trace-table-head" aria-hidden="true"><span>Run</span><span>Context</span><span>Signals</span><span>Created</span><span /></div>
						<ul className="trace-catalog-list">
							{filteredTraces.map((t) => {
								const meta = traceMeta(t);
								const containerName = t.containerId ? containers.find((container) => container.id === t.containerId)?.name ?? t.containerId : null;
								return <li key={t.id} className={`trace-catalog-card${selectedTraceId === t.id ? " selected" : ""}`} data-testid={`inventory-trace-${t.id}`}>
									<div className="trace-card-identity">
										<div className="trace-card-title-row">
											<span className={`trace-source-badge source-${t.source}`}>{t.source}</span>
											{meta.status ? <span className="trace-status"><i />{meta.status}</span> : null}
											{meta.hasEvidence ? <span className="trace-evidence-badge">evidence</span> : null}
										</div>
										<strong className="trace-card-title">{t.title}</strong>
										<span className="trace-digest">#{shortDigest(t.digest)} · {meta.schemaVersion ?? "Trace V5"}</span>
										{traceErrors[t.id] ? <span className="trace-card-error" role="alert" title={traceErrors[t.id]}>{traceErrors[t.id]}</span> : null}
									</div>
									<div className="trace-card-context"><strong>{meta.model ?? "Unknown model"}</strong><span>{containerName ? `container · ${containerName}` : "no related container"}</span>{meta.benchmark ? <span>{meta.benchmark}</span> : null}</div>
									<div className="trace-card-metrics">
										<span><strong>{meta.events ?? "—"}</strong> events</span>
										<span><strong>{meta.toolCalls ?? meta.spans ?? "—"}</strong> {meta.toolCalls != null ? "tools" : "spans"}</span>
										{t.reward != null ? <span><strong>{t.reward}</strong> reward</span> : null}
										{meta.durationMs != null ? <span><strong>{formatDuration(meta.durationMs)}</strong></span> : null}
										{meta.costUsd != null ? <span><strong>${meta.costUsd.toFixed(4)}</strong></span> : null}
									</div>
									<time className="trace-card-created">{formatWhen(t.createdAt)}</time>
									<button
										type="button"
										className="trace-inspect-action"
										disabled={busyId === t.id}
										onClick={() => void openTrace(t)}
										data-testid={`open-trace-${t.id}`}
									>
										{busyId === t.id ? "Opening…" : traceErrors[t.id] ? "Retry migration" : selectedTraceId === t.id ? "Inspector open →" : "Inspect trace →"}
									</button>
								</li>;
							})}
						</ul>
						</div>
					)}
				</div>
			) : null}

			{tab === "visuals" ? (
				<div className="inventory-panel" data-testid="inventory-visuals">
					{visuals.length === 0 ? (
						<p className="inventory-empty">No visuals yet.</p>
					) : (
						<ul className="inventory-list">
							{visuals.map((v) => (
								<li key={v.id} className="inventory-row" data-testid={`inventory-visual-${v.id}`}>
									<div className="inventory-row-main">
										<strong>{v.title}</strong>
										<span className="inventory-row-meta">{v.templateId}</span>
										<span className="inventory-row-when">{formatWhen(v.updatedAt)}</span>
									</div>
									<button
										type="button"
										className="inventory-row-action"
										onClick={() => onOpenVisual(v)}
										data-testid={`open-visual-${v.id}`}
									>
										Open
									</button>
								</li>
							))}
						</ul>
					)}
				</div>
			) : null}

			{tab === "usage" ? (
				<div className="inventory-panel" data-testid="inventory-usage">
					<div className="storage-summary">
						<strong>Rust CoreRuntime inventory</strong>
						<span>{counts.containers} containers · {counts.traces} traces · {counts.usage} usage entries</span>
					</div>
					{usage.length === 0 ? <p className="inventory-empty">No usage entries yet.</p> : (
						<ul className="inventory-list">
							{usage.map((entry) => (
								<li key={entry.id} className="inventory-row">
									<div className="inventory-row-main"><strong>{entry.model}</strong><span className="inventory-row-meta">{entry.provider} · {entry.totalTokens} tokens{entry.costUsd != null ? ` · $${entry.costUsd.toFixed(4)}` : ""}</span><span className="inventory-row-when">{formatWhen(entry.createdAt)}</span></div>
								</li>
							))}
						</ul>
					)}
				</div>
			) : null}
		</div>
	);
}
