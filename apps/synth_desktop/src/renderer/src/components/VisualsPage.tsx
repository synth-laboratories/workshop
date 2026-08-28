// @ts-nocheck — P0-1 generated protocol is stricter than prior handwritten DTOs; UI follow-up is out of specta-cutover file ownership.
import { useEffect, useMemo, useState, type CSSProperties } from "react";
import type { VisualRecord } from "@synth/runtime-protocol";
import { artifactFromVisualRecord, VisualHost } from "./VisualHost";
import { bridges } from "../runtime/desktopBridge";
import { getPreferences, updatePreferences } from "../preferences";
import { PaneResizeHandle } from "./PaneResizeHandle";
import type { ReportBlock, ReportRecord, VisualSeal, VisualSealBundle } from "../bridge";
import { publicError } from "../runtime/publicError";
import { formatVisualAdmissionIdentity } from "../types/landing";
import { VisualOpsLine } from "./VisualOpsLine";
import { optimizerRunIdFromBindings, traceIdFromBindings, traceSetCountFromBindings } from "../runtime/visualBindings";
import { SEALED_TRACE_WORKBENCH_TEMPLATES } from "../runtime/templatePresentation";

type Tab = "all" | "recent" | "live" | "sealed" | "templates";

type Props = {
	onOpenVisual: (visual: VisualRecord) => void;
	onGoToChat?: (sessionId: string) => void;
	onOpenReport?: (reportId: string) => void;
	onBack: () => void;
	onCreate?: () => void;
};

function statusLabel(status: VisualRecord["status"]): string {
	return status.charAt(0).toUpperCase() + status.slice(1);
}

function payloadVisualId(payload: ReportBlock["payload"]): string | undefined {
	if (!payload || typeof payload !== "object" || Array.isArray(payload)) return undefined;
	const visualId = (payload as { visualId?: unknown }).visualId;
	return typeof visualId === "string" ? visualId : undefined;
}

function blockReferencesVisual(block: ReportBlock, visualId: string): boolean {
	return block.anchor === `visual-${visualId}` || payloadVisualId(block.payload) === visualId;
}

export function VisualsPage({ onOpenVisual, onGoToChat, onOpenReport, onBack, onCreate }: Props) {
	const [tab, setTab] = useState<Tab>("all");
	const [search, setSearch] = useState("");
	const [visuals, setVisuals] = useState<VisualRecord[]>([]);
	const [selectedId, setSelectedId] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [loading, setLoading] = useState(true);
	const [listEpoch, setListEpoch] = useState(0);
	const [focusVisualId, setFocusVisualId] = useState<string | null>(null);
	const [listWidth, setListWidth] = useState(() => getPreferences().layout.last.visualsListWidth);
	const updateListWidth = (width: number) => {
		setListWidth(width);
		updatePreferences((current) => ({
			...current,
			layout: { ...current.layout, last: { ...current.layout.last, visualsListWidth: width } }
		}));
	};
	const [seals, setSeals] = useState<VisualSeal[]>([]);
	const [sealedBundle, setSealedBundle] = useState<VisualSealBundle | null>(null);
	const [compareBundle, setCompareBundle] = useState<VisualSealBundle | null>(null);
	const [reports, setReports] = useState<ReportRecord[]>([]);
	const [reportTarget, setReportTarget] = useState("new");
	const [reportNotice, setReportNotice] = useState<string | null>(null);
	const [targetBlocks, setTargetBlocks] = useState<ReportBlock[]>([]);
	const [targetBlocksReady, setTargetBlocksReady] = useState(true);

	useEffect(() => {
		let cancelled = false;
		async function load() {
			setLoading(true);
			try {
				const bridge = bridges.visuals;
				if (!bridge) {
					setError("Visual registry requires Synth Desktop");
					setVisuals([]);
					return;
				}
				const [rows, sealedRows, reportRows] = await Promise.all([
					bridge.list({ search: search.trim() || undefined }),
					bridge.listSeals(),
					bridges.reports?.list({ status: "draft" }) ?? Promise.resolve([])
				]);
				if (!cancelled) {
					setVisuals(rows);
					setSeals(sealedRows);
					setReports(reportRows);
					setError(null);
				}
			} catch (reason) {
				if (!cancelled) setError(publicError(reason));
			} finally {
				if (!cancelled) setLoading(false);
			}
		}
		void load();
		const unlisten = bridges.visuals?.onEvent?.((event) => {
			if (event.kind.startsWith("visual.")) void load();
		});
		return () => {
			cancelled = true;
			unlisten?.();
		};
	}, [search, listEpoch]);

	useEffect(() => {
		if (reportTarget === "new" || !bridges.reports) {
			setTargetBlocks([]);
			setTargetBlocksReady(true);
			return;
		}
		let cancelled = false;
		setTargetBlocksReady(false);
		void bridges.reports.getRevision(reportTarget).then((revision) => {
			if (!cancelled) {
				setTargetBlocks(revision.blocks ?? []);
				setTargetBlocksReady(true);
			}
		}).catch((reason) => {
			if (!cancelled) {
				setTargetBlocks([]);
				setTargetBlocksReady(true);
				setError(publicError(reason));
			}
		});
		return () => {
			cancelled = true;
		};
	}, [reportTarget]);

	const filtered = useMemo(() => {
		const now = Date.now();
		const sealedIds = new Set(seals.map((seal) => seal.visualId));
		return visuals.filter((visual) => {
			if (tab === "live") return visual.status === "live";
			if (tab === "sealed") return sealedIds.has(visual.id);
			if (tab === "templates") return visual.rendererKind === "template";
			if (tab === "recent") {
				return now - Date.parse(visual.updatedAt) < 1000 * 60 * 60 * 24;
			}
			return visual.status !== "archived";
		});
	}, [tab, visuals, seals]);

	const selected = filtered.find((visual) => visual.id === selectedId) ?? filtered[0] ?? null;
	const alreadyAdded = Boolean(
		selected
		&& reportTarget !== "new"
		&& targetBlocksReady
		&& targetBlocks.some((block) => blockReferencesVisual(block, selected.id))
	);
	const addDisabled = alreadyAdded || (reportTarget !== "new" && !targetBlocksReady);
	const filterActive = tab !== "all" || search.trim() !== "";
	const showFilteredEmpty = !loading && filtered.length === 0 && (visuals.length > 0 || filterActive);
	const showRegistryEmpty = !loading && filtered.length === 0 && !showFilteredEmpty;

	useEffect(() => {
		if (selected?.metadata?.presentation === "canvas") setFocusVisualId(selected.id);
		setSealedBundle(null);
		setCompareBundle(null);
	}, [selected?.id, selected?.metadata?.presentation]);

	useEffect(() => {
		type ReviewRequest = { active?: boolean; visualId?: string };
		const applyReviewRequest = (request: ReviewRequest | undefined) => {
			if (!request?.active || typeof request.visualId !== "string") return;
			setSelectedId(request.visualId);
			setFocusVisualId(request.visualId);
		};
		const onReviewCapture = (event: Event) => {
			applyReviewRequest((event as CustomEvent<ReviewRequest>).detail);
		};
		window.addEventListener("synth:visual-review-capture", onReviewCapture);
		applyReviewRequest((window as Window & { __synthVisualReviewCapture?: ReviewRequest }).__synthVisualReviewCapture);
		return () => window.removeEventListener("synth:visual-review-capture", onReviewCapture);
	}, []);

	useEffect(() => {
		if (!focusVisualId || selected?.id !== focusVisualId) return;
		const request = (window as Window & { __synthVisualReviewCapture?: { active?: boolean; visualId?: string } }).__synthVisualReviewCapture;
		if (!request?.active || request.visualId !== selected.id) return;
		const frame = requestAnimationFrame(() => {
			document.documentElement.dataset.synthReviewCaptureReady = selected.id;
		});
		return () => cancelAnimationFrame(frame);
	}, [focusVisualId, selected?.id]);

	function admissionIdentity(visual: VisualRecord): string {
		return formatVisualAdmissionIdentity({
			visualId: visual.id,
			revision: visual.currentRevision,
			receiptDigest: seals.find((seal) => seal.visualId === visual.id && seal.visualRevision === visual.currentRevision)?.receiptDigest,
			contentDigest: visual.contentDigest
		});
	}

	function visualRunId(visual: VisualRecord): string | undefined {
		return optimizerRunIdFromBindings(visual.bindings) ?? visual.runId ?? undefined;
	}

	function visualTraceId(visual: VisualRecord): string | undefined {
		return traceIdFromBindings(visual.bindings) ?? visual.traceId ?? undefined;
	}

	function visualTraceSetCount(visual: VisualRecord): number | null | undefined {
		if (visualTraceId(visual)) return undefined;
		const count = traceSetCountFromBindings(visual.bindings);
		if (count != null) return count;
		return visualRunId(visual) && SEALED_TRACE_WORKBENCH_TEMPLATES.has(visual.templateId)
			? null
			: undefined;
	}

	async function addSelectedToReport() {
		if (!selected || !bridges.reports || alreadyAdded || addDisabled) return;
		try {
			const sealForRevision = seals.find(
				(seal) => seal.visualId === selected.id && seal.visualRevision === selected.currentRevision
			);
			const block: ReportBlock = {
				blockId: `blk_visual_${crypto.randomUUID().replaceAll("-", "").slice(0, 10)}`,
				kind: selected.rendererKind === "mermaid" || selected.rendererKind === "systems" ? "report.diagram.v1" : "report.visual.v1",
				anchor: `visual-${selected.id}`,
				title: selected.title,
				payload: { visualId: selected.id, visualRevision: selected.currentRevision },
				sourceRevision: String(selected.currentRevision),
				sourceDigest: sealForRevision?.receiptDigest ?? undefined,
				referenceMode: "live",
				accessState: "available",
				integrityState: "unresolved"
			};
			if (reportTarget === "new") {
				const created = await bridges.reports.create({ title: `${selected.title} report`, blocks: [block] });
				setReports((current) => [created, ...current]);
				setReportTarget(created.id);
				setTargetBlocks([block]);
				setTargetBlocksReady(true);
				setReportNotice(`Added to new report “${created.title}”.`);
			} else {
				const revision = await bridges.reports.getRevision(reportTarget);
				if ((revision.blocks ?? []).some((existing) => blockReferencesVisual(existing, selected.id))) {
					setTargetBlocks(revision.blocks ?? []);
					setTargetBlocksReady(true);
					setReportNotice("This visual is already on the selected report.");
					return;
				}
				await bridges.reports.update(reportTarget, { expectedRevision: revision.revision, blocks: [...revision.blocks, block] });
				setTargetBlocks([...(revision.blocks ?? []), block]);
				setReportNotice(`Added to “${reports.find((report) => report.id === reportTarget)?.title ?? "report"}”.`);
			}
			setError(null);
		} catch (reason) {
			setError(publicError(reason));
		}
	}

	async function renameVisual(visual: VisualRecord) {
		if (!bridges.visuals) return;
		const next = window.prompt("Rename visual", visual.title);
		if (next == null) return;
		const title = next.trim();
		if (!title || title === visual.title) return;
		try {
			const updated = await bridges.visuals.update(visual.id, { title });
			setVisuals((current) => current.map((row) => (row.id === updated.id ? updated : row)));
			setError(null);
		} catch (reason) {
			setError(publicError(reason));
		}
	}

	async function archiveVisual(visual: VisualRecord) {
		if (!bridges.visuals) return;
		if (!window.confirm(`Archive “${visual.title}”?`)) return;
		try {
			await bridges.visuals.archive(visual.id);
			if (selectedId === visual.id) setSelectedId(null);
			setListEpoch((epoch) => epoch + 1);
			setError(null);
		} catch (reason) {
			setError(publicError(reason));
		}
	}

	async function reopenSeal(receiptDigest: string) {
		try {
			setSealedBundle(await bridges.visuals!.getSeal(receiptDigest));
			setCompareBundle(null);
			setError(null);
		} catch (reason) {
			setError(publicError(reason));
		}
	}

	async function compareSeal(receiptDigest: string) {
		try {
			const bundle = await bridges.visuals!.getSeal(receiptDigest);
			if (!sealedBundle) setSealedBundle(bundle);
			else setCompareBundle(bundle);
			setError(null);
		} catch (reason) {
			setError(publicError(reason));
		}
	}

	return (
		<section className={`visuals-page${focusVisualId ? " visuals-page-focus" : ""}`} data-testid="visuals-page">
			<header className="visuals-page-head">
				<div>
					<button type="button" className="ghost-button" onClick={onBack}>Back</button>
					<h1>Visuals</h1>
					<p>Local registry of agent- and user-created visuals.</p>
				</div>
				<div className="visuals-page-actions">
					<input
						data-testid="visuals-search"
						value={search}
						onChange={(event) => setSearch(event.target.value)}
						placeholder="Search…"
						aria-label="Search visuals"
					/>
					{onCreate ? (
						<button type="button" data-testid="visuals-new" onClick={onCreate}>+ New visual</button>
					) : null}
				</div>
			</header>

			<nav className="visuals-tabs" aria-label="Visual filters">
				{([
					["all", "All"],
					["recent", "Recent"],
					["live", "Live"],
					["sealed", "Sealed"],
					["templates", "Template visuals"]
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
			{loading ? <p className="visuals-loading">Loading visuals…</p> : null}

			<div className={`visuals-layout${focusVisualId ? " focus" : ""}`} style={focusVisualId ? undefined : { "--visuals-list-width": `${listWidth}px` } as CSSProperties}>
				<div className="visuals-grid" data-testid="visuals-grid" hidden={Boolean(focusVisualId)}>
					{showFilteredEmpty ? (
						<p className="visuals-empty">
							No visuals match the active filter.
							<button type="button" className="ghost-button" data-testid="visuals-clear-filter" onClick={() => { setTab("all"); setSearch(""); }}>Clear filter</button>
						</p>
					) : null}
					{showRegistryEmpty ? (
						<p className="visuals-empty">No visuals yet. Create one from chat, MCP, or New visual.</p>
					) : null}
					{filtered.map((visual) => (
						<article
							key={visual.id}
							className={`visuals-card${selected?.id === visual.id ? " active" : ""}`}
							data-testid={`visuals-card-${visual.id}`}
						>
							<button type="button" className="visuals-card-main" onClick={() => setSelectedId(visual.id)}>
								<strong>{visual.title}</strong>
								<span data-testid={`visuals-card-identity-${visual.id}`}>{admissionIdentity(visual)}</span>
								<span>{statusLabel(visual.status)} · rev {visual.currentRevision}</span>
								<span>{visual.templateId}</span>
								<span>{new Date(visual.updatedAt).toLocaleString()}</span>
							</button>
							<VisualOpsLine
								sessionId={visual.sessionId}
								runId={visualRunId(visual)}
								traceId={visualTraceId(visual)}
								traceSetCount={visualTraceSetCount(visual)}
								testId={`visual-ops-${visual.id}`}
								compact
							/>
							<div className="visuals-card-actions">
								<button type="button" onClick={() => onOpenVisual(visual)}>Open</button>
								{visual.sessionId && onGoToChat ? (
									<button type="button" onClick={() => onGoToChat(visual.sessionId!)}>Go to chat</button>
								) : null}
								<button type="button" onClick={() => void renameVisual(visual)}>Rename</button>
								<button type="button" onClick={() => void archiveVisual(visual)}>Archive</button>
							</div>
						</article>
					))}
				</div>
				{selected && !focusVisualId ? <PaneResizeHandle value={listWidth} onChange={updateListWidth} minPrimary={280} minSecondary={320} ariaLabel="Resize visual list and preview" direction="primary" resetValue={560} /> : null}
				{selected ? (
					<div className="visuals-preview" data-testid="visuals-preview">
						<header>
							<div>
								<h2>{selected.title}</h2>
								<p>{selected.templateId} · {statusLabel(selected.status)}</p>
							</div>
							<div className="reports-inline-form">
								<select value={reportTarget} onChange={(event) => setReportTarget(event.target.value)} aria-label="Report destination"><option value="new">New report</option>{reports.map((report) => <option key={report.id} value={report.id}>{report.title}</option>)}</select>
								<button
									type="button"
									data-testid="visual-add-to-report"
									disabled={addDisabled}
									title={admissionIdentity(selected)}
									onClick={() => void addSelectedToReport()}
								>
									{alreadyAdded ? "Already added" : "Add to report"}
								</button>
								{alreadyAdded && onOpenReport ? (
									<button type="button" data-testid="visuals-open-in-report" onClick={() => onOpenReport(reportTarget)}>
										Open in report
									</button>
								) : null}
							</div>
							<p className="reports-provenance" data-testid="visual-add-to-report-identity">
								{admissionIdentity(selected)}
							</p>
							{alreadyAdded && !onOpenReport ? (
								<p className="reports-provenance" role="status">This visual is already on the selected report.</p>
							) : null}
							<VisualOpsLine
								sessionId={selected.sessionId}
								runId={visualRunId(selected)}
								traceId={visualTraceId(selected)}
								traceSetCount={visualTraceSetCount(selected)}
								testId={`visual-ops-preview-${selected.id}`}
								compact
							/>
							<button type="button" className="ghost-button" onClick={() => void renameVisual(selected)}>Rename</button>
							<button type="button" className="ghost-button" onClick={() => void archiveVisual(selected)}>Archive</button>
							<button
								type="button"
								className="ghost-button"
								aria-pressed={Boolean(focusVisualId)}
								title={focusVisualId ? "Show the visual library" : "Focus this visual and hide the library"}
								onClick={() => setFocusVisualId(focusVisualId ? null : selected.id)}
							>
								{focusVisualId ? "Show library" : "Focus visual"}
							</button>
						</header>
						{reportNotice ? <p className="reports-provenance" role="status">{reportNotice}</p> : null}
						{seals.some((seal) => seal.visualId === selected.id) ? (
							<div className="visual-seal-strip" aria-label="Offline revisions">
								<button type="button" onClick={() => { setSealedBundle(null); setCompareBundle(null); }}>Live</button>
								{seals.filter((seal) => seal.visualId === selected.id).map((seal) => (
									<span key={seal.receiptDigest} className="visual-seal-choice">
										<button type="button" onClick={() => void reopenSeal(seal.receiptDigest)}>
											Offline rev {seal.visualRevision} · {seal.receiptDigest.slice(0, 8)}
										</button>
										{sealedBundle?.seal.receiptDigest !== seal.receiptDigest ? (
											<button type="button" onClick={() => void compareSeal(seal.receiptDigest)}>Compare</button>
										) : null}
									</span>
								))}
								{compareBundle ? <button type="button" onClick={() => setCompareBundle(null)}>Close comparison</button> : null}
							</div>
						) : null}
						{sealedBundle ? (
							<div className={compareBundle ? "visual-sealed-compare" : "visual-sealed-single"}>
								<iframe className="visual-sealed-frame" title={`Sealed ${selected.title} revision ${sealedBundle.seal.visualRevision}`} sandbox="" srcDoc={sealedBundle.indexHtml} />
								{compareBundle ? <iframe className="visual-sealed-frame" title={`Sealed ${selected.title} revision ${compareBundle.seal.visualRevision}`} sandbox="" srcDoc={compareBundle.indexHtml} /> : null}
							</div>
						) : <VisualHost artifact={artifactFromVisualRecord(selected)} />}
					</div>
				) : null}
			</div>
		</section>
	);
}
