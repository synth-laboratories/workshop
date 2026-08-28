import { useEffect, useMemo, useRef, useState } from "react";
import type { ArtifactRef } from "../types/landing";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";

function decodeBase64Utf8(base64: string): string {
	const bytes = Uint8Array.from(atob(base64), (char) => char.charCodeAt(0));
	return new TextDecoder().decode(bytes);
}

/**
 * Ad-hoc chart panes display the runtime's SVG rendition — the same bytes a
 * `capture_review` photographs — so a human and an agent are never looking at
 * two different renderings of one spec.
 */
export function ChartVisual({ artifact }: { artifact: ArtifactRef }) {
	const visualId = artifact.visualId ?? artifact.id;
	const [imageUrl, setImageUrl] = useState<string | null>(null);
	const [spec, setSpec] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [showSpec, setShowSpec] = useState(false);
	const [reload, setReload] = useState(0);
	const retryToken = useRef("");
	retryToken.current = `${visualId}:${String(artifact.metadata?.currentRevision ?? artifact.metadata?.revision ?? "")}`;
	const renderStatus = typeof artifact.metadata?.renderStatus === "string" ? artifact.metadata.renderStatus : "queued";
	const renderError = typeof artifact.metadata?.renderError === "string" ? artifact.metadata.renderError : null;

	useEffect(() => {
		let cancelled = false;
		void (async () => {
			try {
				const asset = await bridges.visuals?.content?.(visualId);
				if (!cancelled) setSpec(asset ? decodeBase64Utf8(asset.base64) : null);
			} catch {
				if (!cancelled) setSpec(null);
			}
			try {
				// Theme is omitted: the spec declares it, and the runtime keys the
				// rendition on that declaration.
				const rendition = await bridges.visuals?.rendition?.(visualId, "svg", null, "pane");
				if (!cancelled && rendition) {
					setImageUrl(`data:${rendition.mediaType};base64,${rendition.base64}`);
					setError(null);
				}
			} catch (reason) {
				if (!cancelled) setError(publicError(reason));
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [visualId, artifact.metadata, reload]);

	const failed = Boolean(error || renderStatus === "failed");
	const displaySpec = showSpec || (failed && !imageUrl);
	const statusLabel = useMemo(() => {
		if (failed) return "CHART · SPEC";
		if (!imageUrl) return "CHART · RENDERING";
		return "CHART";
	}, [failed, imageUrl]);

	const exportSvg = () => {
		if (!imageUrl) return;
		const link = document.createElement("a");
		link.href = imageUrl;
		link.download = `${artifact.title || "chart"}.svg`;
		link.click();
	};

	return (
		<div className="chart-visual" data-testid="visual-chart" data-render-status={renderStatus}>
			<div className="chart-visual-toolbar">
				<span className="chart-visual-status">{statusLabel}</span>
				<div className="chart-visual-actions">
					<button className="ws-btn ws-btn-small ws-btn-ghost" type="button" onClick={() => setShowSpec((value) => !value)}>
						{displaySpec ? "Chart" : "Spec"}
					</button>
					<button className="ws-btn ws-btn-small ws-btn-ghost" type="button" disabled={!spec} onClick={() => { if (spec) void navigator.clipboard.writeText(spec); }}>
						Copy spec
					</button>
					<button className="ws-btn ws-btn-small ws-btn-ghost" type="button" disabled={!imageUrl} onClick={exportSvg}>
						Export SVG
					</button>
					<button
						className="ws-btn ws-btn-small ws-btn-ghost"
						type="button"
						onClick={() => {
							const token = retryToken.current;
							void (async () => {
								await bridges.visuals?.render?.(visualId);
								if (retryToken.current === token) setReload((value) => value + 1);
							})();
						}}
					>
						Retry
					</button>
				</div>
			</div>
			{displaySpec ? (
				<pre className="chart-visual-spec" data-testid="visual-chart-spec">{spec ?? renderError ?? error ?? "Spec unavailable."}</pre>
			) : imageUrl ? (
				<div className="chart-visual-stage">
					<img src={imageUrl} alt={artifact.title} draggable={false} />
				</div>
			) : (
				<p className="visual-loading">{renderError ?? error ?? "Rendering chart…"}</p>
			)}
		</div>
	);
}
