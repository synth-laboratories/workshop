import { useEffect, useMemo, useState, type CSSProperties } from "react";
import type { VisualRecord } from "@synth/runtime-protocol";
import { artifactFromVisualRecord, VisualHost } from "./VisualHost";
import { bridges } from "../runtime/desktopBridge";
import { getPreferences, updatePreferences } from "../preferences";
import { PaneResizeHandle } from "./PaneResizeHandle";
import type { VisualSeal, VisualSealBundle } from "../bridge";
import { publicError } from "../runtime/publicError";

type Tab = "all" | "recent" | "live" | "sealed" | "templates";

type Props = {
	onOpenVisual: (visual: VisualRecord) => void;
	onGoToChat?: (sessionId: string) => void;
	onBack: () => void;
	onCreate?: () => void;
};

function statusLabel(status: VisualRecord["status"]): string {
	return status.charAt(0).toUpperCase() + status.slice(1);
}

export function VisualsPage({ onOpenVisual, onGoToChat, onBack, onCreate }: Props) {
	const [tab, setTab] = useState<Tab>("all");
	const [search, setSearch] = useState("");
	const [visuals, setVisuals] = useState<VisualRecord[]>([]);
	const [selectedId, setSelectedId] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [loading, setLoading] = useState(true);
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
				const [rows, sealedRows] = await Promise.all([
					bridge.list({ search: search.trim() || undefined }),
					bridge.listSeals()
				]);
				if (!cancelled) {
					setVisuals(rows);
					setSeals(sealedRows);
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
	}, [search]);

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
	useEffect(() => {
		if (selected?.metadata?.presentation === "canvas") setFocusVisualId(selected.id);
		setSealedBundle(null);
		setCompareBundle(null);
	}, [selected?.id, selected?.metadata?.presentation]);

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
					["templates", "Templates"]
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
					{filtered.length === 0 && !loading ? (
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
								<span>{statusLabel(visual.status)} · rev {visual.currentRevision}</span>
								<span>{visual.templateId}</span>
								<span>{new Date(visual.updatedAt).toLocaleString()}</span>
							</button>
							<div className="visuals-card-actions">
								<button type="button" onClick={() => onOpenVisual(visual)}>Open</button>
								{visual.sessionId && onGoToChat ? (
									<button type="button" onClick={() => onGoToChat(visual.sessionId!)}>Go to chat</button>
								) : null}
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
							<button
								type="button"
								className="ghost-button"
								onClick={() => setFocusVisualId(focusVisualId ? null : selected.id)}
							>
								{focusVisualId ? "Exit canvas" : "Open canvas"}
							</button>
						</header>
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
