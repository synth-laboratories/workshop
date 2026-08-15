import { useEffect, useMemo, useRef, useState } from "react";
import type { ArtifactRef } from "../types/landing";
import { bridges } from "../runtime/desktopBridge";

type RenderStatus = "queued" | "rendering" | "ready" | "failed" | string;

function metadataString(metadata: Record<string, unknown> | undefined, key: string): string | null {
	const value = metadata?.[key];
	return typeof value === "string" ? value : null;
}

function decodeBase64Utf8(base64: string): string {
	const bytes = Uint8Array.from(atob(base64), (char) => char.charCodeAt(0));
	return new TextDecoder().decode(bytes);
}

export function MermaidVisual({ artifact }: { artifact: ArtifactRef }) {
	const visualId = artifact.visualId ?? artifact.id;
	const [imageUrl, setImageUrl] = useState<string | null>(null);
	const [source, setSource] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [showSource, setShowSource] = useState(false);
	const [scale, setScale] = useState(1);
	const [offset, setOffset] = useState({ x: 0, y: 0 });
	const [reload, setReload] = useState(0);
	const drag = useRef<{ x: number; y: number; ox: number; oy: number } | null>(null);
	const stage = useRef<HTMLDivElement>(null);
	const retryToken = useRef("");
	retryToken.current = `${visualId}:${String(artifact.metadata?.currentRevision ?? artifact.metadata?.revision ?? "")}`;
	const renderStatus = (metadataString(artifact.metadata, "renderStatus") ?? "queued") as RenderStatus;
	const renderError = metadataString(artifact.metadata, "renderError");
	const diagramKind = metadataString(artifact.metadata, "diagramKind");

	useEffect(() => {
		let cancelled = false;
		const load = async () => {
			if (!bridges.visuals?.rendition || !visualId) return;
			try {
				const rendition = await bridges.visuals.rendition(visualId, "svg", "light", "pane");
				if (cancelled) return;
				setImageUrl(`data:${rendition.mediaType};base64,${rendition.base64}`);
				setError(null);
			} catch (reason) {
				if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
			}
			try {
				const asset = await bridges.visuals.content?.(visualId);
				if (!cancelled && asset) {
					setSource(decodeBase64Utf8(asset.base64));
				}
			} catch {
				if (!cancelled) setSource(null);
			}
		};
		void load();
		return () => {
			cancelled = true;
		};
	}, [visualId, artifact.metadata, reload]);

	const statusLabel = useMemo(() => {
		if (error || renderStatus === "failed") return "Render failed — showing source";
		if (!imageUrl && (renderStatus === "queued" || renderStatus === "rendering")) return "Rendering diagram…";
		return diagramKind ? `Mermaid · ${diagramKind}` : "Mermaid diagram";
	}, [diagramKind, error, imageUrl, renderStatus]);

	const failed = Boolean(error || renderStatus === "failed" || (!imageUrl && source));
	const displaySource = showSource || (failed && !imageUrl);
	const endDrag = (pointerId?: number) => {
		drag.current = null;
		const target = stage.current;
		if (target && pointerId !== undefined && target.hasPointerCapture(pointerId)) target.releasePointerCapture(pointerId);
	};
	useEffect(() => () => endDrag(), []);

	return (
		<div className="mermaid-visual" data-testid="visual-mermaid" data-render-status={renderStatus}>
			<div className="mermaid-visual-toolbar">
				<span className="mermaid-visual-status">{statusLabel}</span>
				<div className="mermaid-visual-actions">
					<button className="mermaid-icon-button" type="button" onClick={() => setScale((value) => Math.min(3, value + 0.15))} aria-label="Zoom in" title="Zoom in">+</button>
					<button className="mermaid-icon-button" type="button" onClick={() => setScale((value) => Math.max(0.5, value - 0.15))} aria-label="Zoom out" title="Zoom out">−</button>
					<button type="button" onClick={() => { setScale(1); setOffset({ x: 0, y: 0 }); }} aria-label="Fit diagram">Fit</button>
					<button type="button" onClick={() => setShowSource((value) => !value)}>{displaySource ? "Diagram" : "Source"}</button>
					<button
						type="button"
						onClick={() => {
							if (source) void navigator.clipboard.writeText(source);
						}}
						disabled={!source}
					>
						Copy source
					</button>
					<button
						type="button"
						onClick={() => {
							if (!imageUrl) return;
							const link = document.createElement("a");
							link.href = imageUrl;
							link.download = `${artifact.title || "diagram"}.svg`;
							link.click();
						}}
						disabled={!imageUrl}
					>
						Export SVG
					</button>
					<button
						type="button"
						onClick={() => {
							const token = retryToken.current;
							void bridges.visuals?.render?.(visualId).then(() => {
								if (retryToken.current !== token) return;
								setReload((value) => value + 1);
							});
						}}
					>
						Retry
					</button>
				</div>
			</div>
			{displaySource ? (
				<pre className="mermaid-visual-source" data-testid="visual-mermaid-source">{source ?? renderError ?? "Source unavailable."}</pre>
			) : imageUrl ? (
				<div
					className="mermaid-visual-stage"
					ref={stage}
					onPointerDown={(event) => {
						drag.current = { x: event.clientX, y: event.clientY, ox: offset.x, oy: offset.y };
						(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
					}}
					onPointerMove={(event) => {
						if (!drag.current) return;
						setOffset({
							x: drag.current.ox + event.clientX - drag.current.x,
							y: drag.current.oy + event.clientY - drag.current.y
						});
					}}
					onPointerUp={(event) => endDrag(event.pointerId)}
					onPointerCancel={(event) => endDrag(event.pointerId)}
					onLostPointerCapture={() => { drag.current = null; }}
				>
					<img
						src={imageUrl}
						alt={artifact.title}
						draggable={false}
						style={{ transform: `translate(${offset.x}px, ${offset.y}px) scale(${scale})` }}
					/>
				</div>
			) : (
				<p className="visual-loading">{renderError ?? "Rendering diagram…"}</p>
			)}
		</div>
	);
}
