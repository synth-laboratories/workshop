import { useEffect, useMemo, useRef, useState } from "react";
import type { ArtifactRef } from "../types/landing";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";

function decodeBase64Utf8(base64: string): string {
	const bytes = Uint8Array.from(atob(base64), (char) => char.charCodeAt(0));
	return new TextDecoder().decode(bytes);
}

function sourceTheme(source: string | null): "dark" | "light" {
	if (!source) return "light";
	try {
		return JSON.parse(source).theme === "technical-dark" ? "dark" : "light";
	} catch {
		return "light";
	}
}

export function SystemsMapVisual({ artifact }: { artifact: ArtifactRef }) {
	const visualId = artifact.visualId ?? artifact.id;
	const [imageUrl, setImageUrl] = useState<string | null>(null);
	const [source, setSource] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [showSource, setShowSource] = useState(false);
	const [scale, setScale] = useState(1);
	const [offset, setOffset] = useState({ x: 0, y: 0 });
	const [reload, setReload] = useState(0);
	const [notice, setNotice] = useState<string | null>(null);
	const drag = useRef<{ x: number; y: number; ox: number; oy: number } | null>(null);
	const stage = useRef<HTMLDivElement>(null);
	const retryToken = useRef("");
	retryToken.current = `${visualId}:${String(artifact.metadata?.currentRevision ?? artifact.metadata?.revision ?? "")}`;
	const renderStatus = typeof artifact.metadata?.renderStatus === "string" ? artifact.metadata.renderStatus : "queued";
	const renderError = typeof artifact.metadata?.renderError === "string" ? artifact.metadata.renderError : null;

	useEffect(() => {
		let cancelled = false;
		void (async () => {
			let canonical: string | null = null;
			try {
				const asset = await bridges.visuals?.content?.(visualId);
				canonical = asset ? decodeBase64Utf8(asset.base64) : null;
				if (!cancelled) setSource(canonical);
			} catch {
				if (!cancelled) setSource(null);
			}
			try {
				const rendition = await bridges.visuals?.rendition?.(visualId, "svg", sourceTheme(canonical), "pane");
				if (!cancelled && rendition) {
					setImageUrl(`data:${rendition.mediaType};base64,${rendition.base64}`);
					setError(null);
				}
			} catch (reason) {
				if (!cancelled) setError(publicError(reason));
			}
		})();
		return () => { cancelled = true; };
	}, [visualId, artifact.metadata, reload]);

	const failed = Boolean(error || renderStatus === "failed" || (!imageUrl && source));
	const displaySource = showSource || (failed && !imageUrl);
	const statusLabel = useMemo(() => {
		if (failed) return "SYSTEMS MAP · SOURCE";
		if (!imageUrl) return "SYSTEMS MAP · RENDERING";
		return "SYSTEMS MAP · 2D";
	}, [failed, imageUrl]);

	const exportSvg = () => {
		if (!imageUrl) return;
		try {
			const link = document.createElement("a");
			link.href = imageUrl;
			link.download = `${artifact.title || "systems-map"}.svg`;
			link.click();
			setNotice(`Exported ${link.download}`);
		} catch (reason) {
			setNotice(`Export failed: ${publicError(reason)}`);
		}
	};
	const endDrag = (pointerId?: number) => {
		drag.current = null;
		const target = stage.current;
		if (target && pointerId !== undefined && target.hasPointerCapture(pointerId)) target.releasePointerCapture(pointerId);
	};
	useEffect(() => () => endDrag(), []);

	return (
		<div className="systems-visual" data-testid="visual-systems-map" data-render-status={renderStatus}>
			<div className="systems-visual-toolbar">
				<span className="systems-visual-status">{statusLabel}</span>
				<div className="systems-visual-actions">
					<button className="systems-icon-button" type="button" onClick={() => setScale((value) => Math.min(3, value + 0.15))} aria-label="Zoom in">+</button>
					<button className="systems-icon-button" type="button" onClick={() => setScale((value) => Math.max(0.5, value - 0.15))} aria-label="Zoom out">−</button>
					<button type="button" onClick={() => { setScale(1); setOffset({ x: 0, y: 0 }); }}>Fit</button>
					<button type="button" onClick={() => setShowSource((value) => !value)}>{displaySource ? "Map" : "Source"}</button>
					<button type="button" disabled={!source} onClick={() => { if (source) void navigator.clipboard.writeText(source); }}>Copy source</button>
					<button type="button" disabled={!imageUrl} onClick={exportSvg}>Export SVG</button>
					<button type="button" onClick={() => { const token = retryToken.current; void (async () => { await bridges.visuals?.render?.(visualId); if (retryToken.current === token) setReload((value) => value + 1); })(); }}>Retry</button>
				</div>
			</div>
			{notice ? <p className="systems-visual-notice" role="status">{notice}</p> : null}
			{displaySource ? (
				<pre className="systems-visual-source" data-testid="visual-systems-map-source">{source ?? renderError ?? error ?? "Source unavailable."}</pre>
			) : imageUrl ? (
				<div ref={stage} className="systems-visual-stage" onPointerDown={(event) => { drag.current = { x: event.clientX, y: event.clientY, ox: offset.x, oy: offset.y }; event.currentTarget.setPointerCapture(event.pointerId); }} onPointerMove={(event) => { if (drag.current) setOffset({ x: drag.current.ox + event.clientX - drag.current.x, y: drag.current.oy + event.clientY - drag.current.y }); }} onPointerUp={(event) => endDrag(event.pointerId)} onPointerCancel={(event) => endDrag(event.pointerId)} onLostPointerCapture={() => { drag.current = null; }}>
					<img src={imageUrl} alt={artifact.title} draggable={false} style={{ transform: `translate(${offset.x}px, ${offset.y}px) scale(${scale})` }} />
				</div>
			) : <p className="visual-loading">{renderError ?? error ?? "Rendering systems map…"}</p>}
		</div>
	);
}
