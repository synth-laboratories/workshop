import { useEffect, useMemo, useState } from "react";
import { bridges } from "../runtime/desktopBridge";
import TraceInspector from "@synth/visual-templates/trace.rollout_inspector.v1/shell";
import type {
	ExperimentRecord,
	ReportBlock,
	ReportComment,
	ReportRecord,
	ReportRevision,
	ReportSeal,
	ReportSealBundle,
	ReportUpload,
	ResearchLogEntry
} from "../bridge";

type Tab = "all" | "draft" | "sealed";
type AppendixView = "ledger" | "lineage" | "inspector";
type LogView = "timeline" | "decisions" | "inspector";

type Props = {
	onBack: () => void;
};

const MISSING = "—";

function displayMissing(value: unknown): string {
	if (value === null || value === undefined || value === "") return MISSING;
	if (typeof value === "number" && Number.isNaN(value)) return MISSING;
	return String(value);
}

type ReportTraceEntry = {
	label?: string;
	projection?: Record<string, unknown>;
	traceDigest?: string;
	traceId?: string;
};

type CompareArm = { id?: string; label?: string; color?: string };
type EffortCell = { arm?: string; effort?: string; mean?: number | null; scored?: number; n?: number; cost?: number };

function traceGroup(trace: ReportTraceEntry): string {
	const label = trace.label || trace.traceId || "Other";
	return label.split(/[·|]/, 1)[0]?.trim() || "Other";
}

function CompareStory({ payload }: { payload: Record<string, unknown> }) {
	const efforts = Array.isArray(payload.efforts) ? payload.efforts.map(String) : ["low", "medium", "high"];
	const arms = Array.isArray(payload.arms) ? payload.arms as CompareArm[] : [];
	const cells = Array.isArray(payload.effortScale) ? payload.effortScale as EffortCell[] : [];
	const themes = Array.isArray(payload.themes) ? payload.themes as Array<Record<string, unknown>> : [];
	return (
		<div className="reports-result" data-testid="reports-compare-story">
			{typeof payload.lede === "string" && payload.lede ? <p>{payload.lede}</p> : null}
			<div className="reports-effort-grid" style={{ gridTemplateColumns: `minmax(110px, .9fr) repeat(${efforts.length}, 1fr)` }}>
				<strong>Model</strong>
				{efforts.map((effort) => <strong key={effort}>{effort}</strong>)}
				{arms.flatMap((arm, armIndex) => [
					<span key={`${arm.id || armIndex}-label`}>{arm.label || arm.id || `arm ${armIndex + 1}`}</span>,
					...efforts.map((effort) => {
						const cell = cells.find((row) => row.arm === arm.id && row.effort === effort);
						const present = typeof cell?.mean === "number" && Number.isFinite(cell.mean);
						return <span key={`${arm.id || armIndex}-${effort}`} className={present ? "" : "reports-empty-cell"}>
							<b>{present ? cell.mean!.toFixed(2) : MISSING}</b>
							<small>{present ? `${displayMissing(cell?.scored)}/${displayMissing(cell?.n)} scored` : "not run"}</small>
						</span>;
					})
				])}
			</div>
			{themes.flatMap((theme, themeIndex) => {
				const clips = Array.isArray(theme.clips) ? theme.clips as Array<Record<string, unknown>> : [];
				return clips.map((clip, clipIndex) => {
					const frames = Array.isArray(clip.frames) ? clip.frames as Array<Record<string, unknown>> : [];
					const frame = frames.find((row) => typeof row.image === "string") || frames[0];
					return <figure key={`${themeIndex}-${clipIndex}`} className="reports-film-frame">
						{frame && typeof frame.image === "string" ? <img src={frame.image} alt={`${String(clip.label || "Craftax")} environment render`} /> : <pre>{frame && typeof frame.map === "string" ? frame.map : MISSING}</pre>}
						<figcaption>{String(clip.label || theme.title || "Observation")} · env render{frame ? ` · t${displayMissing(frame.call)}` : ""}</figcaption>
					</figure>;
				});
			})}
		</div>
	);
}

function ReportEvidence({ block }: { block: ReportBlock }) {
	if (block.accessState === "missing") return <p className="reports-missing">{MISSING}</p>;
	if (block.kind === "report.prose.v1" || typeof block.payload.markdown === "string") {
		return <p className="reports-prose">{String(block.payload.markdown || "")}</p>;
	}
	if (block.kind === "report.result.v1" && block.payload.schema_version === "craftax.compare-story.v1") {
		return <CompareStory payload={block.payload} />;
	}
	if ((block.kind === "report.visual.v1" || block.kind === "report.diagram.v1") && typeof block.payload.sealedHtml === "string") {
		return <iframe className="reports-sealed-visual" title={block.title || block.kind} sandbox="allow-scripts" srcDoc={block.payload.sealedHtml} />;
	}
	return <div className="reports-evidence-card"><strong>{block.kind}</strong><span>Frozen evidence attached to this revision.</span></div>;
}

function outlineItems(revision: ReportRevision | null) {
	return (revision?.blocks ?? [])
		.filter((block) => block.kind !== "report.outline.v1")
		.map((block) => ({
			anchor: block.anchor,
			title: block.title || block.kind
		}));
}

function traceEntries(block: ReportBlock | undefined): ReportTraceEntry[] {
	const payload = block?.payload ?? {};
	if (Array.isArray(payload.traces)) return payload.traces as ReportTraceEntry[];
	if (payload.projection && typeof payload.projection === "object") {
		return [{
			label: typeof payload.label === "string" ? payload.label : undefined,
			projection: payload.projection as Record<string, unknown>,
			traceDigest: typeof payload.traceDigest === "string" ? payload.traceDigest : undefined,
			traceId: typeof payload.traceId === "string" ? payload.traceId : undefined
		}];
	}
	return [];
}

export function ReportsPage({ onBack }: Props) {
	const [tab, setTab] = useState<Tab>("all");
	const [search, setSearch] = useState("");
	const [reports, setReports] = useState<ReportRecord[]>([]);
	const [selectedId, setSelectedId] = useState<string | null>(null);
	const [revision, setRevision] = useState<ReportRevision | null>(null);
	const [experiments, setExperiments] = useState<ExperimentRecord[]>([]);
	const [log, setLog] = useState<ResearchLogEntry[]>([]);
	const [seals, setSeals] = useState<ReportSeal[]>([]);
	const [sealedBundle, setSealedBundle] = useState<ReportSealBundle | null>(null);
	const [compareBundle, setCompareBundle] = useState<ReportSealBundle | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [loading, setLoading] = useState(true);
	const [selectedExperimentId, setSelectedExperimentId] = useState<string | null>(null);
	const [selectedLogId, setSelectedLogId] = useState<string | null>(null);
	const [appendixView, setAppendixView] = useState<AppendixView>("ledger");
	const [logView, setLogView] = useState<LogView>("timeline");
	const [draftTitle, setDraftTitle] = useState("");
	const [draftSummary, setDraftSummary] = useState("");
	const [draftFindings, setDraftFindings] = useState("");
	const [draftMethods, setDraftMethods] = useState("");
	const [experimentTitle, setExperimentTitle] = useState("");
	const [logTitle, setLogTitle] = useState("");
	const [logBody, setLogBody] = useState("");
	const [shareUpload, setShareUpload] = useState<ReportUpload | null>(null);
	const [sharedUrl, setSharedUrl] = useState("");
	const [comments, setComments] = useState<ReportComment[]>([]);
	const [commentBody, setCommentBody] = useState("");
	const [traceDigest, setTraceDigest] = useState("");
	const [traceLabel, setTraceLabel] = useState("");
	const [selectedTraceIndex, setSelectedTraceIndex] = useState(0);

	async function load(reportId?: string | null) {
		const bridge = bridges.reports;
		if (!bridge) {
			setError("Report registry requires Synth Desktop");
			setReports([]);
			return;
		}
		setLoading(true);
		try {
			const rows = await bridge.list({ search: search.trim() || undefined });
			const sealedRows = await bridge.listSeals();
			setReports(rows);
			setSeals(sealedRows);
			const nextId = reportId ?? selectedId ?? rows[0]?.id ?? null;
			setSelectedId(nextId);
			if (nextId) {
				const [nextRevision, nextExperiments, nextLog] = await Promise.all([
					bridge.getRevision(nextId),
					bridge.listExperiments(nextId),
					bridge.listLog(nextId)
				]);
				setRevision(nextRevision);
				setExperiments(nextExperiments);
				setLog(nextLog);
				setDraftTitle(nextRevision.title);
				setDraftSummary(nextRevision.summary ?? "");
				const findings = nextRevision.blocks.find((block) => block.anchor === "findings");
				const methods = nextRevision.blocks.find((block) => block.anchor === "methods");
				setDraftFindings(typeof findings?.payload?.markdown === "string" ? findings.payload.markdown : "");
				setDraftMethods(typeof methods?.payload?.markdown === "string" ? methods.payload.markdown : "");
				const nextComments = await bridge.listComments(nextId, nextRevision.revision);
				setComments(nextComments);
				const latestSeal = sealedRows.find((seal) => seal.reportId === nextId);
				if (latestSeal) {
					const upload = await bridge.uploadStatus(latestSeal.receiptDigest);
					setShareUpload(upload);
				}
			} else {
				setRevision(null);
			}
			setError(null);
		} catch (reason) {
			setError(String(reason));
		} finally {
			setLoading(false);
		}
	}

	useEffect(() => {
		void load();
		const unlisten = bridges.reports?.onEvent?.((event) => {
			if (event.kind.startsWith("report.")) void load();
		});
		return () => unlisten?.();
	}, [search]);

	const filtered = useMemo(() => {
		return reports.filter((report) => {
			if (tab === "draft") return report.status === "draft";
			if (tab === "sealed") return report.status === "sealed";
			return true;
		});
	}, [reports, tab]);

	const selected = filtered.find((report) => report.id === selectedId) ?? filtered[0] ?? null;
	const selectedExperiment =
		experiments.find((row) => row.experimentId === selectedExperimentId) ?? experiments[0] ?? null;
	const selectedLog = log.find((row) => row.entryId === selectedLogId) ?? log[0] ?? null;
	const decisionKinds = new Set(["hypothesis", "decision", "protocol_change", "correction", "claim_decision", "limitation"]);

	async function createReport() {
		try {
			const created = await bridges.reports!.create({ title: "Untitled report" });
			setSealedBundle(null);
			setCompareBundle(null);
			await load(created.id);
		} catch (reason) {
			setError(String(reason));
		}
	}

	async function saveDraft() {
		if (!selected || !revision) return;
		const blocks: ReportBlock[] = revision.blocks.map((block) => {
			if (block.anchor === "findings") {
				return { ...block, payload: { ...block.payload, markdown: draftFindings } };
			}
			if (block.anchor === "methods") {
				return { ...block, payload: { ...block.payload, markdown: draftMethods } };
			}
			return block;
		});
		try {
			await bridges.reports!.update(selected.id, {
				title: draftTitle,
				summary: draftSummary,
				blocks
			});
			await load(selected.id);
		} catch (reason) {
			setError(String(reason));
		}
	}

	async function sealReport() {
		if (!selected || !revision) return;
		try {
			await saveDraft();
			const seal = await bridges.reports!.seal(selected.id, revision.revision);
			setSealedBundle(await bridges.reports!.getSeal(seal.receiptDigest));
			setCompareBundle(null);
			await load(selected.id);
		} catch (reason) {
			setError(String(reason));
		}
	}

	async function reopenSeal(receiptDigest: string) {
		try {
			setSealedBundle(await bridges.reports!.getSeal(receiptDigest));
			setCompareBundle(null);
		} catch (reason) {
			setError(String(reason));
		}
	}

	async function compareSeal(receiptDigest: string) {
		try {
			const bundle = await bridges.reports!.getSeal(receiptDigest);
			if (!sealedBundle) setSealedBundle(bundle);
			else setCompareBundle(bundle);
		} catch (reason) {
			setError(String(reason));
		}
	}

	async function shareCurrentSeal() {
		const seal = seals.find((row) => row.reportId === selected?.id);
		if (!seal) return;
		try {
			const upload = await bridges.reports!.shareSeal(seal.receiptDigest);
			setShareUpload(upload);
			if (upload.committedUrl) setSharedUrl(upload.committedUrl);
			await load(selected?.id);
		} catch (reason) {
			setError(String(reason));
		}
	}

	async function openSharedUrl() {
		if (!sharedUrl.trim()) return;
		try {
			const bundle = await bridges.reports!.openShared(sharedUrl.trim());
			setSealedBundle(bundle);
			setCompareBundle(null);
		} catch (reason) {
			setError(String(reason));
		}
	}

	async function attachTrace() {
		if (!selected || !revision || !traceDigest.trim()) return;
		try {
			const resolved = await bridges.inventory!.resolveTraceProjection(traceDigest.trim(), "rollout-inspector");
			const existing = revision.blocks.find((block) => block.kind === "report.trace-v5.v1");
			const traces = traceEntries(existing);
			const entry: ReportTraceEntry = {
				traceDigest: resolved.traceDigest,
				traceId: resolved.traceDigest,
				label: traceLabel.trim() || resolved.traceDigest.slice(0, 23),
				projection: resolved.payload as Record<string, unknown>
			};
			const nextTraces = [...traces.filter((row) => row.traceDigest !== resolved.traceDigest), entry];
			const traceBlock: ReportBlock = {
				blockId: existing?.blockId ?? "blk_traces",
				kind: "report.trace-v5.v1",
				anchor: "traces",
				title: existing?.title ?? "Trace evidence",
				payload: {
					projectionKind: "rollout-inspector",
					traces: nextTraces
				},
				sourceDigest: resolved.traceDigest,
				accessState: "accessible",
				integrityState: "verified"
			};
			const blocks = existing
				? revision.blocks.map((block) => (block.kind === "report.trace-v5.v1" ? traceBlock : block))
				: [...revision.blocks, traceBlock];
			await bridges.reports!.update(selected.id, { blocks });
			setTraceDigest("");
			setTraceLabel("");
			setSelectedTraceIndex(Math.max(nextTraces.length - 1, 0));
			await load(selected.id);
		} catch (reason) {
			setError(String(reason));
		}
	}

	async function addComment() {
		if (!selected || !revision || !commentBody.trim()) return;
		try {
			await bridges.reports!.createComment(selected.id, revision.revision, {
				body: commentBody.trim(),
				receiptDigest: seals.find((row) => row.reportId === selected.id)?.receiptDigest
			});
			setCommentBody("");
			await load(selected.id);
		} catch (reason) {
			setError(String(reason));
		}
	}

	async function addExperiment() {
		if (!selected || !experimentTitle.trim()) return;
		try {
			await bridges.reports!.upsertExperiment(selected.id, {
				title: experimentTitle.trim(),
				status: "planned",
				arms: [],
				runs: [],
				results: []
			});
			setExperimentTitle("");
			await load(selected.id);
		} catch (reason) {
			setError(String(reason));
		}
	}

	async function appendLog() {
		if (!selected || !logTitle.trim() || !logBody.trim()) return;
		try {
			await bridges.reports!.appendLog(selected.id, {
				entryKind: "observation",
				title: logTitle.trim(),
				body: logBody.trim(),
				actorKind: "human"
			});
			setLogTitle("");
			setLogBody("");
			await load(selected.id);
		} catch (reason) {
			setError(String(reason));
		}
	}

	const readerRevision = sealedBundle
		? ((sealedBundle.data.revision as ReportRevision | undefined) ?? revision)
		: revision;

	return (
		<section className="visuals-page reports-page" data-testid="reports-page">
			<header className="visuals-page-head">
				<div>
					<button type="button" className="ghost-button" onClick={onBack}>Back</button>
					<h1>Reports</h1>
					<p>Author, seal, and reopen a curated research synthesis offline.</p>
				</div>
				<div className="visuals-page-actions">
					<input
						data-testid="reports-search"
						value={search}
						onChange={(event) => setSearch(event.target.value)}
						placeholder="Search…"
						aria-label="Search reports"
					/>
					<button type="button" data-testid="reports-new" onClick={() => void createReport()}>+ New report</button>
				</div>
			</header>

			<nav className="visuals-tabs" aria-label="Report filters">
				{([
					["all", "All"],
					["draft", "Drafts"],
					["sealed", "Sealed"]
				] as const).map(([id, label]) => (
					<button
						key={id}
						type="button"
						className={tab === id ? "active" : undefined}
						aria-pressed={tab === id}
						onClick={() => setTab(id)}
					>
						{label}
					</button>
				))}
			</nav>

			{error ? <p className="visuals-error">{error}</p> : null}
			{loading ? <p className="visuals-loading">Loading reports…</p> : null}

			<div className="visuals-layout reports-layout">
				<div className="visuals-grid" data-testid="reports-grid">
					{filtered.length === 0 && !loading ? (
						<p className="visuals-empty">No reports yet. Create one to freeze narrative plus evidence.</p>
					) : null}
					{filtered.map((report) => (
						<article
							key={report.id}
							className={`visuals-card${selected?.id === report.id ? " active" : ""}`}
							data-testid={`reports-card-${report.id}`}
						>
							<button type="button" className="visuals-card-main" onClick={() => void load(report.id)}>
								<strong>{report.title}</strong>
								<span>{report.status} · rev {report.currentRevision}</span>
								<span>{new Date(report.updatedAt).toLocaleString()}</span>
							</button>
						</article>
					))}
				</div>

				{selected && readerRevision ? (
					<div className="visuals-preview reports-preview" data-testid="reports-preview">
						<header>
							<div>
								<input
									className="reports-title-input"
									value={draftTitle}
									onChange={(event) => setDraftTitle(event.target.value)}
									aria-label="Report title"
									disabled={Boolean(sealedBundle)}
								/>
								<p>{selected.status} · {readerRevision.schemaVersion} · rev {readerRevision.revision}</p>
							</div>
							<div className="reports-actions">
								<button type="button" onClick={() => void saveDraft()} disabled={Boolean(sealedBundle)}>Save draft</button>
								<button type="button" data-testid="reports-seal" onClick={() => void sealReport()}>Seal report</button>
								<button
									type="button"
									data-testid="reports-share"
									onClick={() => void shareCurrentSeal()}
									disabled={!seals.some((seal) => seal.reportId === selected.id)}
									title="Human Share uploads this sealed digest privately"
								>
									{shareUpload?.state === "committed" ? "Shared privately" : "Share report"}
								</button>
							</div>
						</header>

						{seals.some((seal) => seal.reportId === selected.id) ? (
							<div className="visual-seal-strip" aria-label="Offline report revisions">
								<button type="button" onClick={() => { setSealedBundle(null); setCompareBundle(null); }}>Working copy</button>
								{seals.filter((seal) => seal.reportId === selected.id).map((seal) => (
									<span key={seal.receiptDigest} className="visual-seal-choice">
										<button type="button" onClick={() => void reopenSeal(seal.receiptDigest)}>
											Open report rev {seal.reportRevision}
										</button>
										<button type="button" onClick={() => void compareSeal(seal.receiptDigest)}>Compare</button>
									</span>
								))}
							</div>
						) : null}

						<form className="reports-inline-form" onSubmit={(event) => { event.preventDefault(); void openSharedUrl(); }}>
							<input
								value={sharedUrl}
								onChange={(event) => setSharedUrl(event.target.value)}
								placeholder="Open private Report URL"
								aria-label="Private Report URL"
							/>
							<button type="submit">Open report</button>
						</form>
						{shareUpload?.committedUrl ? (
							<p className="reports-provenance" data-testid="reports-shared-url">{shareUpload.committedUrl}</p>
						) : null}
						{shareUpload?.state === "failed" ? (
							<p className="visuals-error">{shareUpload.error || "Share failed; no Report URL was created."}</p>
						) : null}

						<nav className="reports-outline" aria-label="Generated outline">
							<strong>Outline</strong>
							<ol>
								{outlineItems(readerRevision).map((item) => (
									<li key={item.anchor}>
										<a href={`#${item.anchor}`}>{item.title}</a>
									</li>
								))}
							</ol>
						</nav>

						<label className="reports-field">
							Summary
							<textarea
								value={draftSummary}
								onChange={(event) => setDraftSummary(event.target.value)}
								disabled={Boolean(sealedBundle)}
							/>
						</label>
						<section id="findings" className="reports-section">
							<h2>Findings</h2>
							<textarea
								data-testid="reports-findings"
								value={draftFindings}
								onChange={(event) => setDraftFindings(event.target.value)}
								disabled={Boolean(sealedBundle)}
							/>
						</section>
						<section id="methods" className="reports-section">
							<h2>Methods</h2>
							<textarea
								data-testid="reports-methods"
								value={draftMethods}
								onChange={(event) => setDraftMethods(event.target.value)}
								disabled={Boolean(sealedBundle)}
							/>
						</section>

						{readerRevision.blocks.filter((block) => !["findings", "methods", "outline", "experiment-records", "research-log", "traces"].includes(block.anchor)).map((block) => (
							<section key={block.blockId} id={block.anchor} className="reports-section">
								<h2>{block.title || block.kind}</h2>
								<ReportEvidence block={block} />
							</section>
						))}

						<section id="traces" className="reports-section" data-testid="reports-traces">
							<h2>{readerRevision.blocks.find((block) => block.kind === "report.trace-v5.v1")?.title || "Trace evidence"}</h2>
							<div className="reports-inline-form">
								<input value={traceDigest} onChange={(event) => setTraceDigest(event.target.value)} placeholder="Trace digest (sha256:…)" disabled={Boolean(sealedBundle)} />
								<input value={traceLabel} onChange={(event) => setTraceLabel(event.target.value)} placeholder="Label (OSS-20B · seed 0)" disabled={Boolean(sealedBundle)} />
								<button type="button" data-testid="reports-attach-trace" onClick={() => void attachTrace()} disabled={Boolean(sealedBundle)}>Attach Trace V5</button>
							</div>
							{(() => {
								const traces = traceEntries(readerRevision.blocks.find((block) => block.kind === "report.trace-v5.v1"));
								const selected = traces[Math.min(selectedTraceIndex, Math.max(traces.length - 1, 0))];
								if (!traces.length) return <p className="reports-missing">{MISSING}</p>;
								return (
									<div className="reports-trace-inspector">
										{traces.length > 1 ? (
											<label className="reports-field">
												Trace
												<select value={String(Math.min(selectedTraceIndex, traces.length - 1))} onChange={(event) => setSelectedTraceIndex(Number(event.target.value))}>
											{Array.from(new Set(traces.map(traceGroup))).map((group) => (
												<optgroup key={group} label={group}>
													{traces.map((row, index) => ({ row, index })).filter(({ row }) => traceGroup(row) === group).map(({ row, index }) => (
														<option key={row.traceDigest || row.label || index} value={index}>{row.label || row.traceId || row.traceDigest || `trace ${index + 1}`}</option>
													))}
												</optgroup>
											))}
												</select>
											</label>
										) : null}
										{selected?.projection ? (
											<TraceInspector title={selected.label} projection={selected.projection as never} />
										) : (
											<p className="reports-missing">{MISSING}</p>
										)}
									</div>
								);
							})()}
						</section>

						<section id="limitations" className="reports-section" data-testid="reports-limitations">
							<h2>Limitations</h2>
							{readerRevision.limitations.length === 0 ? (
								<p className="reports-missing">{MISSING}</p>
							) : (
								<ul>
									{readerRevision.limitations.map((item) => (
										<li key={item.limitationId}>{item.body}</li>
									))}
								</ul>
							)}
						</section>
						{readerRevision.claims.length > 0 ? (
							<section id="claims" className="reports-section" data-testid="reports-claims">
								<h2>Claims</h2>
								<ul>
									{readerRevision.claims.map((claim) => (
										<li key={claim.claimId}>
											<strong>{claim.status}</strong> {claim.statement}
										</li>
									))}
								</ul>
							</section>
						) : null}

						<section id="review-comments" className="reports-section" data-testid="reports-comments">
							<h2>Private review</h2>
							<p className="reports-missing">Comments overlay the sealed revision and do not change its digest.</p>
							<div className="reports-inline-form">
								<input value={commentBody} onChange={(event) => setCommentBody(event.target.value)} placeholder="Add a private review comment" />
								<button type="button" onClick={() => void addComment()}>Add comment</button>
							</div>
							<ol className="reports-log">
								{comments.map((comment) => (
									<li key={comment.commentId}>
										<strong>{comment.authorId}</strong>
										<span>{comment.anchor ? `#${comment.anchor}` : "report"} · {new Date(comment.createdAt).toLocaleString()}</span>
										<p>{comment.body}</p>
									</li>
								))}
							</ol>
						</section>

						<section id="experiment-records" className="reports-section" data-testid="experiment-records">
							<header className="reports-section-head">
								<h2>Experiment Records</h2>
								<nav className="visuals-tabs">
									{([
										["ledger", "Ledger"],
										["lineage", "Lineage"],
										["inspector", "Run inspector"]
									] as const).map(([id, label]) => (
										<button key={id} type="button" className={appendixView === id ? "active" : undefined} onClick={() => setAppendixView(id)}>{label}</button>
									))}
								</nav>
							</header>
							<div className="reports-inline-form">
								<input value={experimentTitle} onChange={(event) => setExperimentTitle(event.target.value)} placeholder="Experiment title" disabled={Boolean(sealedBundle)} />
								<button type="button" onClick={() => void addExperiment()} disabled={Boolean(sealedBundle)}>Add record</button>
							</div>
							{appendixView === "ledger" ? (
								<table className="reports-table">
									<thead>
										<tr>
											<th>Experiment</th>
											<th>Status</th>
											<th>Protocol</th>
											<th>Primary result</th>
										</tr>
									</thead>
									<tbody>
										{experiments.map((row) => {
											const result = Array.isArray(row.results) ? row.results[0] as Record<string, unknown> | undefined : undefined;
											return (
												<tr key={row.experimentId} className={selectedExperiment?.experimentId === row.experimentId ? "active" : undefined} onClick={() => { setSelectedExperimentId(row.experimentId); setAppendixView("inspector"); }}>
													<td>{row.title}</td>
													<td>{row.status}</td>
													<td>{displayMissing(row.protocolDigest)}</td>
													<td>{displayMissing(result?.reward ?? result?.primaryMetric)}</td>
												</tr>
											);
										})}
									</tbody>
								</table>
							) : null}
							{appendixView === "lineage" ? (
								<ul className="reports-lineage">
									{experiments.map((row) => (
										<li key={row.experimentId}>
											<strong>{row.title}</strong>
											<div>protocol {displayMissing(row.protocolDigest)}</div>
											<div>arms {Array.isArray(row.arms) ? row.arms.length : MISSING} → runs {Array.isArray(row.runs) ? row.runs.length : MISSING} → claims {Array.isArray(row.claimRefs) ? row.claimRefs.length : MISSING}</div>
										</li>
									))}
								</ul>
							) : null}
							{appendixView === "inspector" && selectedExperiment ? (
								<dl className="reports-inspector">
									<dt>Experiment</dt><dd>{selectedExperiment.experimentId}</dd>
									<dt>Hypothesis</dt><dd>{displayMissing(selectedExperiment.hypothesis)}</dd>
									<dt>Status</dt><dd>{selectedExperiment.status}</dd>
									<dt>Protocol digest</dt><dd>{displayMissing(selectedExperiment.protocolDigest)}</dd>
									<dt>Results</dt><dd><pre>{JSON.stringify(selectedExperiment.results, null, 2)}</pre></dd>
								</dl>
							) : null}
							{experiments.length === 0 ? <p className="reports-missing">No experiment records yet.</p> : null}
						</section>

						<section id="research-log" className="reports-section" data-testid="research-log">
							<header className="reports-section-head">
								<h2>Research Log</h2>
								<nav className="visuals-tabs">
									{([
										["timeline", "Timeline"],
										["decisions", "Decision trail"],
										["inspector", "Entry inspector"]
									] as const).map(([id, label]) => (
										<button key={id} type="button" className={logView === id ? "active" : undefined} onClick={() => setLogView(id)}>{label}</button>
									))}
								</nav>
							</header>
							<div className="reports-inline-form">
								<input value={logTitle} onChange={(event) => setLogTitle(event.target.value)} placeholder="Log title" disabled={Boolean(sealedBundle)} />
								<input value={logBody} onChange={(event) => setLogBody(event.target.value)} placeholder="What happened" disabled={Boolean(sealedBundle)} />
								<button type="button" onClick={() => void appendLog()} disabled={Boolean(sealedBundle)}>Append entry</button>
							</div>
							<ol className="reports-log">
								{(logView === "decisions" ? log.filter((entry) => decisionKinds.has(entry.entryKind)) : log).map((entry) => (
									<li key={entry.entryId} className={selectedLog?.entryId === entry.entryId ? "active" : undefined} onClick={() => { setSelectedLogId(entry.entryId); setLogView("inspector"); }}>
										<strong>{entry.title}</strong>
										<span>{entry.entryKind} · {entry.author} · {new Date(entry.occurredAt).toLocaleString()}</span>
										<p>{entry.body}</p>
										{entry.supersedesEntryId ? <p className="reports-missing">Corrects {entry.supersedesEntryId}</p> : null}
									</li>
								))}
							</ol>
							{logView === "inspector" && selectedLog ? (
								<dl className="reports-inspector">
									<dt>Entry</dt><dd>{selectedLog.entryId}</dd>
									<dt>Kind</dt><dd>{selectedLog.entryKind}</dd>
									<dt>Claim effect</dt><dd>{displayMissing(selectedLog.claimEffect)}</dd>
									<dt>Links</dt><dd><pre>{JSON.stringify(selectedLog.links, null, 2)}</pre></dd>
								</dl>
							) : null}
						</section>

						<footer className="reports-provenance">
							Report {selected.id} · revision {readerRevision.revision}
							{readerRevision.contentDigest ? ` · digest ${readerRevision.contentDigest}` : ""}
							{readerRevision.compilerName ? ` · ${readerRevision.compilerName} ${readerRevision.compilerVersion ?? ""}` : ""}
						</footer>

						{compareBundle ? (
							<aside className="reports-compare" data-testid="reports-compare">
								<h3>Compare sealed revisions</h3>
								<p>{sealedBundle?.seal.reportRevision} vs {compareBundle.seal.reportRevision}</p>
								<p>{sealedBundle?.seal.receiptDigest === compareBundle.seal.receiptDigest ? "Identical digest" : "Digests differ"}</p>
							</aside>
						) : null}
					</div>
				) : null}
			</div>
		</section>
	);
}
