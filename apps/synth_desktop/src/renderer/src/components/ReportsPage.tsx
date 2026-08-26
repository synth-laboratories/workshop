// @ts-nocheck — P0-1 generated protocol is stricter than prior handwritten DTOs; UI follow-up is out of specta-cutover file ownership.
import { useEffect, useMemo, useState } from "react";
import { bridges } from "../runtime/desktopBridge";
import TraceInspector from "@synth/visual-templates/analysis/trace.rollout_inspector.v1/shell";
import type {
	ExperimentRecord,
	ReportBlock,
	ReportComment,
	ReportRecord,
	ReportPromotion,
	ReportRevision,
	ReportSeal,
	ReportSealBundle,
	ReportUpload,
	ReportValidationResult,
	ReportVisibilityRequest,
	ResearchLogEntry
} from "../bridge";
import { publicError } from "../runtime/publicError";
import { formatVisualAdmissionIdentity } from "../types/landing";

type Tab = "all" | "draft" | "sealed" | "archived";
type AppendixView = "ledger" | "lineage" | "inspector";
type LogView = "timeline" | "decisions" | "inspector";

type Props = {
	onBack: () => void;
	/** Stable report id supplied by Outputs after a restart or navigation. */
	initialReportId?: string;
};

const MISSING = "—";

function reportSlug(title: string): string {
	return title
		.toLowerCase()
		.normalize("NFKD")
		.replace(/[^a-z0-9]+/g, "-")
		.replace(/^-|-$/g, "")
		.slice(0, 96) || "report";
}

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
	if (block.kind === "report.visual.v1" || block.kind === "report.diagram.v1") {
		const visualId = typeof block.payload.visualId === "string" ? block.payload.visualId : undefined;
		const pinnedVerified = block.referenceMode === "pinned" && block.integrityState === "verified";
		return <div className="reports-evidence-card">
			<strong>{block.kind}</strong>
			<span data-testid="reports-visual-pointer">
				{pinnedVerified
					? `Pinned · ${formatVisualAdmissionIdentity({ visualId, revision: block.sourceRevision, receiptDigest: block.sourceDigest })}`
					: `Live pointer · ${formatVisualAdmissionIdentity({ visualId, revision: block.sourceRevision ?? block.payload.visualRevision, receiptDigest: block.sourceDigest })}`}
			</span>
		</div>;
	}
	return <div className="reports-evidence-card"><strong>{block.kind}</strong><span>Evidence attached to this revision.</span></div>;
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

export function ReportsPage({ onBack, initialReportId }: Props) {
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
	const [promotion, setPromotion] = useState<ReportPromotion | null>(null);
	const [publicSlug, setPublicSlug] = useState("");
	const [sharedUrl, setSharedUrl] = useState("");
	const [comments, setComments] = useState<ReportComment[]>([]);
	const [commentBody, setCommentBody] = useState("");
	const [traceDigest, setTraceDigest] = useState("");
	const [traceLabel, setTraceLabel] = useState("");
	const [selectedTraceIndex, setSelectedTraceIndex] = useState(0);
	const [visibilityRequests, setVisibilityRequests] = useState<ReportVisibilityRequest[]>([]);
	const [validation, setValidation] = useState<ReportValidationResult | null>(null);
	const [newBlockKind, setNewBlockKind] = useState("report.prose.v1");
	const [newBlockTitle, setNewBlockTitle] = useState("");
	const [claimStatement, setClaimStatement] = useState("");
	const [claimStatus, setClaimStatus] = useState<"true" | "false" | "needs_more_analysis" | "unresolved">("unresolved");
	const [claimConfidence, setClaimConfidence] = useState<"low" | "medium" | "high" | "overwhelming">("low");
	const [claimWhy, setClaimWhy] = useState("");
	const [claimEvidence, setClaimEvidence] = useState("");
	const [limitationBody, setLimitationBody] = useState("");
	const [audienceKind, setAudienceKind] = useState<"private" | "workspace" | "members">("private");
	const [workspaceId, setWorkspaceId] = useState("");
	const [memberIds, setMemberIds] = useState("");
	const [audienceStatus, setAudienceStatus] = useState<string | null>(null);

	async function load(reportId?: string | null) {
		const bridge = bridges.reports;
		if (!bridge) {
			setError("Report registry requires Synth Desktop");
			setReports([]);
			return;
		}
		setLoading(true);
		try {
			const rows = await bridge.list({ search: search.trim() || undefined, includeArchived: true });
			const sealedRows = await bridge.listSeals();
			setReports(rows);
			setSeals(sealedRows);
			const nextId = reportId ?? selectedId ?? rows[0]?.id ?? null;
			setSelectedId(nextId);
			if (nextId) {
				const [nextRevision, nextExperiments, nextLog, nextVisibilityRequests, nextValidation] = await Promise.all([
					bridge.getRevision(nextId),
					bridge.listExperiments(nextId),
					bridge.listLog(nextId),
					bridge.listVisibilityRequests(nextId),
					bridge.validate(nextId)
				]);
				setRevision(nextRevision);
				setExperiments(nextExperiments);
				setLog(nextLog);
				setVisibilityRequests(nextVisibilityRequests);
				setValidation(nextValidation);
				setDraftTitle(nextRevision.title);
				setPublicSlug(reportSlug(nextRevision.title));
				setPromotion(null);
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
				setVisibilityRequests([]);
				setValidation(null);
			}
			setError(null);
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setLoading(false);
		}
	}

	useEffect(() => {
		void load(initialReportId);
		const unlisten = bridges.reports?.onEvent?.((event) => {
			if (event.kind.startsWith("report.")) void load();
		});
		return () => unlisten?.();
	}, [initialReportId, search]);

	const filtered = useMemo(() => {
		return reports.filter((report) => {
			if (tab === "archived") return Boolean(report.archivedAt);
			if (report.archivedAt) return false;
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
			setError(publicError(reason));
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
				expectedRevision: revision.revision,
				title: draftTitle,
				summary: draftSummary,
				blocks
			});
			await load(selected.id);
		} catch (reason) {
			setError(publicError(reason));
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
			setError(publicError(reason));
		}
	}

	async function runPreflight() {
		if (!selected || !revision) return;
		try {
			setValidation(await bridges.reports!.validate(selected.id, revision.revision));
			setError(null);
		} catch (reason) {
			setError(publicError(reason));
		}
	}

	async function pinAllEvidence() {
		if (!selected) return;
		try {
			await bridges.reports!.pinAll(selected.id);
			await load(selected.id);
		} catch (reason) {
			setError(publicError(reason));
		}
	}

	async function updateComposition(changes: { blocks?: ReportBlock[]; claims?: ReportRevision["claims"]; limitations?: ReportRevision["limitations"] }) {
		if (!selected || !revision) return;
		await bridges.reports!.update(selected.id, {
			expectedRevision: revision.revision,
			...changes
		});
		await load(selected.id);
	}

	async function insertBlock() {
		if (!revision || !newBlockTitle.trim()) return;
		const suffix = crypto.randomUUID().replaceAll("-", "").slice(0, 12);
		const payload = newBlockKind === "report.prose.v1" ? { markdown: "" } : {};
		const evidence = newBlockKind !== "report.prose.v1";
		const block: ReportBlock = {
			blockId: `blk_${suffix}`,
			kind: newBlockKind,
			anchor: `${reportSlug(newBlockTitle)}-${suffix.slice(0, 5)}`,
			title: newBlockTitle.trim(),
			payload,
			referenceMode: "live",
			accessState: evidence ? "missing" : "available",
			integrityState: evidence ? "unresolved" : "verified"
		};
		await updateComposition({ blocks: [...revision.blocks, block] });
		setNewBlockTitle("");
	}

	async function moveBlock(blockId: string, direction: -1 | 1) {
		if (!revision) return;
		const blocks = [...revision.blocks];
		const index = blocks.findIndex((block) => block.blockId === blockId);
		const target = index + direction;
		if (index < 0 || target < 0 || target >= blocks.length) return;
		[blocks[index], blocks[target]] = [blocks[target]!, blocks[index]!];
		await updateComposition({ blocks });
	}

	async function removeBlock(blockId: string) {
		if (!revision) return;
		await updateComposition({ blocks: revision.blocks.filter((block) => block.blockId !== blockId) });
	}

	async function addClaim() {
		if (!revision || !claimStatement.trim() || !claimWhy.trim() || !claimEvidence) return;
		await updateComposition({
			claims: [...revision.claims, {
				claimId: `claim_${crypto.randomUUID().replaceAll("-", "").slice(0, 12)}`,
				statement: claimStatement.trim(),
				status: claimStatus,
				confidence: claimConfidence,
				why: claimWhy.trim(),
				evidenceRefs: [claimEvidence]
			}]
		});
		setClaimStatement("");
		setClaimWhy("");
	}

	async function addLimitation() {
		if (!revision || !limitationBody.trim()) return;
		await updateComposition({ limitations: [...revision.limitations, {
			limitationId: `lim_${crypto.randomUUID().replaceAll("-", "").slice(0, 12)}`,
			body: limitationBody.trim()
		}] });
		setLimitationBody("");
	}

	async function reopenSeal(receiptDigest: string) {
		try {
			setSealedBundle(await bridges.reports!.getSeal(receiptDigest));
			setCompareBundle(null);
		} catch (reason) {
			setError(publicError(reason));
		}
	}

	async function compareSeal(receiptDigest: string) {
		try {
			const bundle = await bridges.reports!.getSeal(receiptDigest);
			if (!sealedBundle) setSealedBundle(bundle);
			else setCompareBundle(bundle);
		} catch (reason) {
			setError(publicError(reason));
		}
	}

	async function requestVisibility(target: "private" | "public" | "unpublished") {
		const seal = seals.find((row) => row.reportId === selected?.id);
		if (!seal) return;
		try {
			await bridges.reports!.requestVisibility(selected!.id, {
				receiptDigest: seal.receiptDigest,
				target,
				slug: target === "public" ? publicSlug.trim() : undefined,
				reason: `Requested from Workshop Reports UI`,
				requestedBy: "human"
			});
			await load(selected?.id);
		} catch (reason) {
			setError(publicError(reason));
		}
	}

	async function openSharedUrl() {
		if (!sharedUrl.trim()) return;
		try {
			const bundle = await bridges.reports!.openShared(sharedUrl.trim());
			setSealedBundle(bundle);
			setCompareBundle(null);
		} catch (reason) {
			setError(publicError(reason));
		}
	}

	async function setReportAudience() {
		const publicationId = shareUpload?.publicationId;
		const seal = seals.find((row) => row.reportId === selected?.id);
		if (!publicationId || !seal) return;
		try {
			const audience = audienceKind === "workspace"
				? { kind: "workspace" as const, workspaceId: workspaceId.trim() }
				: audienceKind === "members"
					? { kind: "members" as const, memberIds: memberIds.split(",").map((value) => value.trim()).filter(Boolean) }
					: { kind: "private" as const };
			const state = await bridges.reports!.setAudience(publicationId, {
				receiptDigest: seal.receiptDigest,
				audience,
				redactionPolicyVersion: "source-aware.v1"
			});
			setAudienceStatus(`${state.status}: ${state.audience.kind}`);
			setError(null);
		} catch (reason) {
			setError(publicError(reason));
		}
	}

	async function revokeReportAudience() {
		const publicationId = shareUpload?.publicationId;
		const seal = seals.find((row) => row.reportId === selected?.id);
		if (!publicationId || !seal) return;
		try {
			const state = await bridges.reports!.revokeAudience(publicationId, seal.receiptDigest);
			setAudienceKind("private");
			setAudienceStatus(`${state.status}: owner only`);
			setError(null);
		} catch (reason) {
			setError(publicError(reason));
		}
	}

	async function decideVisibility(requestId: string, approved: boolean) {
		try {
			const decided = await bridges.reports!.decideVisibility(requestId, approved);
			if (decided.status === "executed" && decided.target === "public" && decided.slug) {
				setPromotion({ publicationId: "approved", slug: decided.slug, status: "published", publicUrl: `/reports/${decided.slug}` });
			}
			await load(selected?.id);
		} catch (reason) {
			setError(publicError(reason));
		}
	}

	async function toggleArchived() {
		if (!selected) return;
		try {
			if (selected.archivedAt) await bridges.reports!.restore(selected.id);
			else await bridges.reports!.archive(selected.id);
			await load(selected.id);
		} catch (reason) {
			setError(publicError(reason));
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
				accessState: "available",
				integrityState: "verified"
			};
			const blocks = existing
				? revision.blocks.map((block) => (block.kind === "report.trace-v5.v1" ? traceBlock : block))
				: [...revision.blocks, traceBlock];
			await bridges.reports!.update(selected.id, { expectedRevision: revision.revision, blocks });
			setTraceDigest("");
			setTraceLabel("");
			setSelectedTraceIndex(Math.max(nextTraces.length - 1, 0));
			await load(selected.id);
		} catch (reason) {
			setError(publicError(reason));
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
			setError(publicError(reason));
		}
	}

	async function addExperiment() {
		if (!selected || !experimentTitle.trim()) return;
		try {
			const value = experimentTitle.trim();
			const looksLikeId = value.startsWith("exp_");
			await bridges.reports!.upsertExperiment(selected.id, {
				title: value,
				status: "planned",
				arms: [],
				runs: [],
				results: [],
				experimentGroupId: looksLikeId ? value : undefined
			});
			setExperimentTitle("");
			await load(selected.id);
		} catch (reason) {
			setError(publicError(reason));
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
			setError(publicError(reason));
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
					,["archived", "Archived"]
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
								<button type="button" data-testid="reports-preflight" onClick={() => void runPreflight()}>Run preflight</button>
								<button type="button" data-testid="reports-pin-all" onClick={() => void pinAllEvidence()} disabled={Boolean(sealedBundle)} title="Pin freezes visual/diagram blocks to a VisualSeal receipt digest">Pin all evidence</button>
								<button type="button" data-testid="reports-seal" onClick={() => void sealReport()} disabled={validation?.sealable === false} title={validation?.sealable === false ? "Resolve unresolved visual evidence before sealing" : "Seal this report revision"}>Seal report</button>
								<button
									type="button"
									data-testid="reports-share"
									onClick={() => void requestVisibility("private")}
									disabled={!seals.some((seal) => seal.reportId === selected.id)}
									title="Human Share uploads this sealed digest privately"
								>
									Request private share
								</button>
								<button
									type="button"
									data-testid="reports-publish"
									onClick={() => void requestVisibility("public")}
									disabled={!seals.some((seal) => seal.reportId === selected.id) || !publicSlug.trim()}
									title="Publish the committed private seal at a stable public Report URL"
								>
									Request publication
								</button>
								<button type="button" onClick={() => void toggleArchived()}>
									{selected.archivedAt ? "Restore report" : "Archive report"}
								</button>
							</div>
							{readerRevision.blocks.some((block) => block.kind === "report.visual.v1" || block.kind === "report.diagram.v1") ? (
								<p className="reports-provenance" data-testid="reports-pin-seal-identity">
									{(readerRevision.blocks.filter((block) => block.kind === "report.visual.v1" || block.kind === "report.diagram.v1")).map((block) => formatVisualAdmissionIdentity({
										visualId: typeof block.payload.visualId === "string" ? block.payload.visualId : undefined,
										revision: block.sourceRevision ?? block.payload.visualRevision,
										receiptDigest: block.sourceDigest
									})).join(" · ")}
								</p>
							) : null}
						</header>
						{validation ? (
							<section className="reports-validation" aria-label="Report validation" data-testid="reports-validation">
								<strong>{validation.sealable ? "Ready to seal" : "Resolve validation errors"}</strong>
								{validation.findings.length ? (
									<ul>
										{validation.findings.map((finding, index) => (
											<li key={`${finding.code}-${finding.blockId ?? finding.claimId ?? index}`}>
												<span>{finding.severity}: {finding.message}{finding.visualId ? ` · ${finding.visualId}` : ""}</span>
												{finding.remediation ? <small>{finding.remediation}</small> : null}
											</li>
										))}
									</ul>
								) : <span>No blocking findings.</span>}
							</section>
						) : null}

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
							<>
								<p className="reports-provenance" data-testid="reports-shared-url">{shareUpload.committedUrl}</p>
								<section className="reports-section" data-testid="reports-audience-controls">
									<h2>Private and team access</h2>
									<p>Audience changes apply to this sealed publication. Revocation returns it to owner-only access.</p>
									<div className="reports-inline-form">
										<select value={audienceKind} onChange={(event) => setAudienceKind(event.target.value as typeof audienceKind)} aria-label="Report audience">
											<option value="private">Owner only</option>
											<option value="workspace">Workspace</option>
											<option value="members">Named members</option>
										</select>
										{audienceKind === "workspace" ? <input value={workspaceId} onChange={(event) => setWorkspaceId(event.target.value)} placeholder="Workspace ID" aria-label="Workspace ID" /> : null}
										{audienceKind === "members" ? <input value={memberIds} onChange={(event) => setMemberIds(event.target.value)} placeholder="Member IDs, comma separated" aria-label="Member IDs" /> : null}
										<button type="button" onClick={() => void setReportAudience()} disabled={audienceKind === "workspace" ? !workspaceId.trim() : audienceKind === "members" ? !memberIds.trim() : false}>Apply audience</button>
										<button type="button" className="ghost-button" onClick={() => void revokeReportAudience()}>Revoke shared access</button>
									</div>
									{audienceStatus ? <p className="reports-provenance" role="status">{audienceStatus}</p> : null}
								</section>
							</>
						) : null}
						<div className="reports-inline-form">
							<input
								value={publicSlug}
								onChange={(event) => { setPublicSlug(event.target.value); setPromotion(null); }}
								placeholder="Public report slug"
								aria-label="Public report slug"
							/>
							<span>/reports/{publicSlug || "report"}</span>
						</div>
						{promotion?.publicUrl ? (
							<p className="reports-provenance" data-testid="reports-public-url">{promotion.publicUrl}</p>
						) : null}
						{shareUpload?.state === "failed" ? (
							<p className="visuals-error">{shareUpload.error || "Share failed; no Report URL was created."}</p>
						) : null}

						{visibilityRequests.length ? (
							<section className="reports-section" data-testid="reports-visibility-requests">
								<h2>Visibility approvals</h2>
								<ol className="reports-log">
									{visibilityRequests.map((request) => (
										<li key={request.requestId}>
											<strong>{request.target}{request.slug ? ` · /reports/${request.slug}` : ""}</strong>
											<span>{request.status} · sealed rev {request.reportRevision} · requested by {request.requestedBy}</span>
											{request.reason ? <p>{request.reason}</p> : null}
											{request.error ? <p className="visuals-error">{request.error}</p> : null}
											{request.status === "pending" ? <div className="reports-actions">
												<button type="button" onClick={() => void decideVisibility(request.requestId, true)}>Approve and execute</button>
												<button type="button" className="ghost-button" onClick={() => void decideVisibility(request.requestId, false)}>Deny</button>
											</div> : null}
										</li>
									))}
								</ol>
								{shareUpload?.state === "committed" ? <button type="button" onClick={() => void requestVisibility("unpublished")}>Request unpublish</button> : null}
							</section>
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
						<section className="reports-section" aria-label="Insert report block">
							<h2>Compose</h2>
							<div className="reports-inline-form">
								<select value={newBlockKind} onChange={(event) => setNewBlockKind(event.target.value)} disabled={Boolean(sealedBundle)}>
									<option value="report.prose.v1">Prose</option>
									<option value="report.result.v1">Result evidence</option>
									<option value="report.visual.v1">Visual evidence</option>
									<option value="report.diagram.v1">Diagram</option>
									<option value="report.attachment.v1">Attachment</option>
								</select>
								<input value={newBlockTitle} onChange={(event) => setNewBlockTitle(event.target.value)} placeholder="Block title" disabled={Boolean(sealedBundle)} />
								<button type="button" onClick={() => void insertBlock()} disabled={Boolean(sealedBundle) || !newBlockTitle.trim()}>Insert block</button>
							</div>
						</section>

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
								<header className="reports-section-head"><h2>{block.title || block.kind}</h2><span>{block.referenceMode ?? "live"} · {block.accessState} · {block.integrityState}{(block.kind === "report.visual.v1" || block.kind === "report.diagram.v1") ? ` · ${formatVisualAdmissionIdentity({ visualId: typeof block.payload.visualId === "string" ? block.payload.visualId : undefined, revision: block.sourceRevision ?? block.payload.visualRevision, receiptDigest: block.sourceDigest })}` : ""}</span>{!sealedBundle ? <><button type="button" onClick={() => void moveBlock(block.blockId, -1)}>↑</button><button type="button" onClick={() => void moveBlock(block.blockId, 1)}>↓</button><button type="button" onClick={() => void removeBlock(block.blockId)}>Remove</button></> : null}</header>
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
							<div className="reports-inline-form"><input value={limitationBody} onChange={(event) => setLimitationBody(event.target.value)} placeholder="Limitation affecting interpretation" disabled={Boolean(sealedBundle)} /><button type="button" onClick={() => void addLimitation()} disabled={Boolean(sealedBundle) || !limitationBody.trim()}>Add limitation</button></div>
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
						<section id="claims" className="reports-section" data-testid="reports-claims">
								<h2>Claims</h2>
								<div className="reports-claim-form"><input value={claimStatement} onChange={(event) => setClaimStatement(event.target.value)} placeholder="Hypothesis or claim" disabled={Boolean(sealedBundle)} /><select value={claimStatus} onChange={(event) => setClaimStatus(event.target.value as typeof claimStatus)} disabled={Boolean(sealedBundle)}><option value="true">True</option><option value="false">False</option><option value="needs_more_analysis">Needs more analysis</option><option value="unresolved">Unresolved</option></select><select value={claimConfidence} onChange={(event) => setClaimConfidence(event.target.value as typeof claimConfidence)} disabled={Boolean(sealedBundle)}><option value="low">Low confidence</option><option value="medium">Medium confidence</option><option value="high">High confidence</option><option value="overwhelming">Overwhelming</option></select><select value={claimEvidence} onChange={(event) => setClaimEvidence(event.target.value)} disabled={Boolean(sealedBundle)}><option value="">Select evidence</option>{readerRevision.blocks.filter((block) => block.kind !== "report.prose.v1" && block.kind !== "report.outline.v1").map((block) => {
									const visual = block.kind === "report.visual.v1" || block.kind === "report.diagram.v1";
									const unresolvedVisual = visual && (block.integrityState === "unresolved" || block.integrityState === "unknown" || !block.sourceDigest);
									const identity = visual
										? formatVisualAdmissionIdentity({
											visualId: typeof block.payload.visualId === "string" ? block.payload.visualId : undefined,
											revision: block.sourceRevision ?? block.payload.visualRevision,
											receiptDigest: block.sourceDigest
										})
										: (block.title || block.anchor);
									return <option key={block.blockId} value={block.blockId}>{identity}{unresolvedVisual ? " · unresolved — not sealable" : ""}</option>;
								})}</select><input value={claimWhy} onChange={(event) => setClaimWhy(event.target.value)} placeholder="Why the evidence supports this verdict" disabled={Boolean(sealedBundle)} /><button type="button" onClick={() => void addClaim()} disabled={Boolean(sealedBundle) || !claimStatement.trim() || !claimWhy.trim() || !claimEvidence}>Add claim</button></div>
								<ul>
									{readerRevision.claims.map((claim) => (
										<li key={claim.claimId}>
											<strong>{claim.status} · {claim.confidence ?? "low"}</strong> {claim.statement}<small>{claim.why}</small>
										</li>
									))}
								</ul>
							</section>

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
								<input value={experimentTitle} onChange={(event) => setExperimentTitle(event.target.value)} placeholder="Experiment title or exp_…" disabled={Boolean(sealedBundle)} />
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
													<td>
														{row.title}
														<div data-testid="report-experiment-group-id">
															{row.experimentGroupId ?? "appendix · unlinked"}
														</div>
													</td>
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
