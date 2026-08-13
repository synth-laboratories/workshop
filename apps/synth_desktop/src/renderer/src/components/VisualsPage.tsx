import { useEffect, useMemo, useState } from "react";
import type { VisualRecord } from "@synth/runtime-protocol";
import { artifactFromVisualRecord, VisualHost } from "./VisualHost";
import { PaneResizeHandle } from "./PaneResizeHandle";
import { bridges } from "../runtime/desktopBridge";

const VISUALS_LIST_WIDTH_KEY = "synth.visuals.list-width";
const DEFAULT_VISUALS_LIST_WIDTH = 560;

function initialListWidth(): number {
	const stored = Number(window.localStorage.getItem(VISUALS_LIST_WIDTH_KEY));
	return Number.isFinite(stored) && stored > 0 ? stored : DEFAULT_VISUALS_LIST_WIDTH;
}

type Tab = "all" | "recent" | "live" | "templates";

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
	const [listWidth, setListWidth] = useState(initialListWidth);
	const updateListWidth = (width: number) => {
		setListWidth(width);
		window.localStorage.setItem(VISUALS_LIST_WIDTH_KEY, String(width));
	};

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
				const rows = await bridge.list({ search: search.trim() || undefined });
				if (!cancelled) {
					setVisuals(rows);
					setError(null);
				}
			} catch (reason) {
				if (!cancelled) setError(String(reason));
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
		return visuals.filter((visual) => {
			if (tab === "live") return visual.status === "live";
			if (tab === "templates") return visual.rendererKind === "template";
			if (tab === "recent") {
				return now - Date.parse(visual.updatedAt) < 1000 * 60 * 60 * 24;
			}
			return visual.status !== "archived";
		});
	}, [tab, visuals]);

	const selected = filtered.find((visual) => visual.id === selectedId) ?? filtered[0] ?? null;
	useEffect(() => {
		if (selected?.metadata?.presentation === "canvas") setFocusVisualId(selected.id);
	}, [selected?.id, selected?.metadata?.presentation]);

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

			<div
				className={`visuals-layout${focusVisualId ? " focus" : ""}`}
				style={focusVisualId ? undefined : { gridTemplateColumns: `${listWidth}px 8px minmax(360px, 1fr)` }}
			>
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
				{selected && !focusVisualId ? (
					<PaneResizeHandle
						value={listWidth}
						onChange={updateListWidth}
						minPrimary={280}
						minSecondary={360}
						ariaLabel="Resize visual list and preview"
						direction="primary"
					/>
				) : null}
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
						<VisualHost artifact={artifactFromVisualRecord(selected)} />
					</div>
				) : null}
			</div>
		</section>
	);
}
