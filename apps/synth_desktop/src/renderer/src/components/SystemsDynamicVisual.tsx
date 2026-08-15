import { useEffect, useMemo, useRef, useState } from "react";
import type { ArtifactRef } from "../types/landing";
import { bridges } from "../runtime/desktopBridge";

type SceneStyle = "warning" | "success" | "missing" | "unproven" | "accent" | "muted" | "dashed";
type RectItem = { id: string; x: number; y: number; width: number; height: number; label?: string; visible?: boolean; opacity?: number; style?: SceneStyle };
type Note = { id?: string; x: number; y: number; width: number; text: string; visible?: boolean; opacity?: number; style?: SceneStyle };
type Edge = { id?: string; from: string; to: string; label?: string; route?: "orthogonal" | "straight"; directed?: boolean; visible?: boolean; opacity?: number; style?: SceneStyle };
type Beat = { id: string; atMs: number; durationMs?: number; caption: string; description?: string };
type Change = { visible?: boolean; x?: number; y?: number; opacity?: number; emphasis?: boolean; style?: string };
type TimelineEvent = { atMs: number; durationMs?: number; easing?: "linear" | "ease-in" | "ease-out" | "ease-in-out" | "step-start" | "step-end"; target: string; changes: Change };
type DynamicScene = {
	theme?: string; canvas: { width: number; height: number }; groups?: RectItem[]; nodes: RectItem[]; edges?: Edge[]; notes?: Note[];
	durationMs: number; posterTimeMs: number; beats: Beat[]; timeline: TimelineEvent[];
	reducedMotion?: "poster" | "final";
};

const decode = (base64: string) => new TextDecoder().decode(Uint8Array.from(atob(base64), (char) => char.charCodeAt(0)));
const finite = (value: unknown): value is number => typeof value === "number" && Number.isFinite(value);

function parseScene(source: string): DynamicScene {
	const scene = JSON.parse(source) as DynamicScene;
	if (!scene || !scene.canvas || !finite(scene.canvas.width) || !finite(scene.canvas.height) || !finite(scene.durationMs) || !finite(scene.posterTimeMs) || !Array.isArray(scene.nodes) || !Array.isArray(scene.beats) || !Array.isArray(scene.timeline)) throw new Error("Dynamic scene source is invalid.");
	return scene;
}

function easedProgress(progress: number, easing: TimelineEvent["easing"]): number {
	if (easing === "step-start") return 1;
	if (easing === "step-end") return progress >= 1 ? 1 : 0;
	if (easing === "linear") return progress;
	if (easing === "ease-in") return progress * progress;
	if (easing === "ease-out") return 1 - (1 - progress) * (1 - progress);
	return progress * progress * (3 - 2 * progress);
}

/** Resolve only allowlisted declarative changes; source is never evaluated as code. */
function stateAt<T extends object>(scene: DynamicScene, target: string, timeMs: number, initial: T): T & Change {
	let state: T & Change = { ...initial };
	for (const event of scene.timeline.filter((item) => item.target === target).sort((a, b) => a.atMs - b.atMs)) {
		if (timeMs < event.atMs) break;
		const duration = finite(event.durationMs) ? Math.max(0, event.durationMs) : 600;
		const raw = duration === 0 ? 1 : Math.min(1, (timeMs - event.atMs) / duration);
		const progress = easedProgress(raw, event.easing);
		const next = { ...state };
		for (const key of ["x", "y", "opacity"] as const) {
			const destination = event.changes[key];
			if (finite(destination)) {
				const origin = finite(state[key]) ? state[key] as number : key === "opacity" ? 1 : 0;
				next[key] = origin + (destination - origin) * progress;
			}
		}
		const discreteReady = event.easing !== "step-end" || raw >= 1;
		if (event.changes.visible !== undefined && discreteReady) next.visible = event.changes.visible;
		if (event.changes.emphasis !== undefined && discreteReady) next.emphasis = event.changes.emphasis;
		if (event.changes.style !== undefined && discreteReady) next.style = event.changes.style;
		state = next;
		if (raw < 1) break;
	}
	return state;
}

function styleClass(style: string | undefined): string {
	return ["warning", "success", "missing", "unproven", "accent", "muted", "dashed"].includes(style ?? "") ? ` style-${style}` : "";
}

function wrapLabel(label: string | undefined, width: number, height: number, fontSize = 14): string[] {
	if (!label) return [];
	const normalized = label.replace(/\s+/g, " ").trim();
	const maxChars = Math.max(6, Math.floor((width - 24) / (fontSize * 0.62)));
	const maxLines = Math.max(1, Math.floor((height - 18) / (fontSize * 1.3)));
	const words = normalized.split(" ");
	const lines: string[] = [];
	let line = "";
	for (const word of words) {
		const pieces = word.length > maxChars
			? Array.from({ length: Math.ceil(word.length / maxChars) }, (_, index) => word.slice(index * maxChars, (index + 1) * maxChars))
			: [word];
		for (const piece of pieces) {
			const candidate = line ? `${line} ${piece}` : piece;
			if (candidate.length <= maxChars) line = candidate;
			else { if (line) lines.push(line); line = piece; }
		}
	}
	if (line) lines.push(line);
	if (lines.length <= maxLines) return lines;
	const visible = lines.slice(0, maxLines);
	visible[maxLines - 1] = `${visible[maxLines - 1].slice(0, Math.max(1, maxChars - 1)).trimEnd()}…`;
	return visible;
}

function SvgWrappedLabel({ label, x, y, width, height, className, fontSize = 14 }: { label?: string; x: number; y: number; width: number; height: number; className: string; fontSize?: number }) {
	const lines = wrapLabel(label, width, height, fontSize);
	const lineHeight = fontSize * 1.3;
	const firstY = y + height / 2 - ((lines.length - 1) * lineHeight) / 2;
	return <text className={className} x={x + width / 2} y={firstY} textAnchor="middle" dominantBaseline="middle" aria-label={label}>
		{label ? <title>{label}</title> : null}
		{lines.map((line, index) => <tspan key={`${line}-${index}`} x={x + width / 2} dy={index === 0 ? 0 : lineHeight}>{line}</tspan>)}
	</text>;
}

function compactEdgeLabel(label: string): string {
	const normalized = label.replace(/\s+/g, " ").trim();
	return normalized.length <= 24 ? normalized : `${normalized.slice(0, 23).trimEnd()}…`;
}

function edgeGeometry(from: RectItem & Change, to: RectItem & Change, route: Edge["route"]): { path: string; labelX: number; labelY: number } {
	const ac = { x: from.x + from.width / 2, y: from.y + from.height / 2 };
	const bc = { x: to.x + to.width / 2, y: to.y + to.height / 2 };
	let x1 = ac.x; let y1 = ac.y; let x2 = bc.x; let y2 = bc.y;
	if (Math.abs(bc.x - ac.x) >= Math.abs(bc.y - ac.y)) {
		x1 = bc.x >= ac.x ? from.x + from.width : from.x;
		x2 = bc.x >= ac.x ? to.x : to.x + to.width;
	} else {
		y1 = bc.y >= ac.y ? from.y + from.height : from.y;
		y2 = bc.y >= ac.y ? to.y : to.y + to.height;
	}
	const path = route === "straight" ? `M ${x1} ${y1} L ${x2} ${y2}` : Math.abs(x2 - x1) >= Math.abs(y2 - y1) ? `M ${x1} ${y1} H ${(x1 + x2) / 2} V ${y2} H ${x2}` : `M ${x1} ${y1} V ${(y1 + y2) / 2} H ${x2} V ${y2}`;
	return { path, labelX: (x1 + x2) / 2, labelY: (y1 + y2) / 2 - 8 };
}

export function SystemsDynamicVisual({ artifact }: { artifact: ArtifactRef }) {
	const visualId = artifact.visualId ?? artifact.id;
	const [source, setSource] = useState<string | null>(null);
	const [posterUrl, setPosterUrl] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [showSource, setShowSource] = useState(false);
	const [playing, setPlaying] = useState(false);
	const [timeMs, setTimeMs] = useState(0);
	const [reduceMotion, setReduceMotion] = useState(false);
	const [notice, setNotice] = useState<string | null>(null);
	const [reload, setReload] = useState(0);
	const clock = useRef<{ started: number; from: number } | null>(null);
	const retryToken = useRef("");
	retryToken.current = `${visualId}:${String(artifact.metadata?.currentRevision ?? artifact.metadata?.revision ?? "")}`;
	const parsedScene = useMemo(() => {
		try { return { scene: source ? parseScene(source) : null, error: null }; }
		catch (reason) { return { scene: null, error: reason instanceof Error ? reason.message : String(reason) }; }
	}, [source]);
	const scene = parsedScene.scene;

	useEffect(() => {
		const query = window.matchMedia("(prefers-reduced-motion: reduce)");
		setReduceMotion(query.matches);
		const listener = (event: MediaQueryListEvent) => setReduceMotion(event.matches);
		query.addEventListener?.("change", listener);
		return () => query.removeEventListener?.("change", listener);
	}, []);

	useEffect(() => {
		let cancelled = false;
		void (async () => {
			let theme: "dark" | "light" = "light";
			try {
				const asset = await bridges.visuals?.content?.(visualId);
				const canonical = asset ? decode(asset.base64) : null;
				if (cancelled) return;
				setSource(canonical);
				const parsed = canonical ? parseScene(canonical) : null;
				theme = parsed?.theme === "technical-dark" ? "dark" : "light";
				setTimeMs(parsed?.posterTimeMs ?? 0);
				setPlaying(false);
			} catch (reason) {
				if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
			}
			try {
				const rendition = await bridges.visuals?.rendition?.(visualId, "svg", theme, "pane");
				if (!cancelled && rendition) setPosterUrl(`data:${rendition.mediaType};base64,${rendition.base64}`);
				if (!cancelled) setError(null);
			} catch (reason) {
				if (!cancelled && !source) setError(reason instanceof Error ? reason.message : String(reason));
			}
		})();
		return () => { cancelled = true; };
	}, [visualId, artifact.metadata, reload]);

	useEffect(() => {
		if (!playing || !scene || reduceMotion) return;
		clock.current = { started: performance.now(), from: timeMs };
		let frame = 0;
		const tick = (now: number) => {
			const next = Math.min(scene.durationMs, (clock.current?.from ?? 0) + now - (clock.current?.started ?? now));
			setTimeMs(next);
			if (next >= scene.durationMs) setPlaying(false); else frame = requestAnimationFrame(tick);
		};
		frame = requestAnimationFrame(tick);
		return () => cancelAnimationFrame(frame);
	}, [playing, reduceMotion, scene]);

	useEffect(() => {
		if (!scene || !reduceMotion) return;
		setPlaying(false);
		setTimeMs(scene.reducedMotion === "final" ? scene.durationMs : scene.posterTimeMs);
	}, [reduceMotion, scene]);

	const activeBeatIndex = scene ? scene.beats.reduce((latest, beat, index) => beat.atMs <= timeMs ? index : latest, 0) : 0;
	const activeBeat = scene?.beats[activeBeatIndex];
	const nodeMap = new Map(scene?.nodes.map((node) => [node.id, stateAt(scene, node.id, timeMs, node)]) ?? []);
	const seekBeat = (index: number) => { const beat = scene?.beats[index]; if (beat) { setPlaying(false); setTimeMs(beat.atMs); } };
	const exportStill = () => {
		if (!posterUrl) return;
		try { const link = document.createElement("a"); link.href = posterUrl; link.download = `${artifact.title || "systems-explainer"}-still.svg`; link.click(); setNotice(`Exported ${link.download}`); }
		catch (reason) { setNotice(`Export failed: ${reason instanceof Error ? reason.message : String(reason)}`); }
	};

	return (
		<div className="systems-dynamic" data-testid="visual-systems-dynamic" data-theme={scene?.theme === "technical-dark" ? "dark" : "light"}>
			<div className="systems-dynamic-toolbar">
				<span className="systems-dynamic-status">BENJAMIN DICKEN STYLE</span>
				<div className="systems-visual-actions">
					<button type="button" disabled={!scene} onClick={() => { if (reduceMotion && scene) setTimeMs(scene.reducedMotion === "final" ? scene.durationMs : scene.posterTimeMs); else setPlaying((value) => !value); }}>{playing ? "Pause" : "Play"}</button>
					<button type="button" disabled={!scene} onClick={() => { setTimeMs(0); setPlaying(!reduceMotion); }}>Replay</button>
					<button type="button" disabled={!scene || activeBeatIndex <= 0} onClick={() => seekBeat(activeBeatIndex - 1)} aria-label="Previous beat">← Beat</button>
					<button type="button" disabled={!scene || activeBeatIndex >= (scene?.beats.length ?? 0) - 1} onClick={() => seekBeat(activeBeatIndex + 1)} aria-label="Next beat">Beat →</button>
					<button type="button" aria-pressed={reduceMotion} onClick={() => setReduceMotion((value) => !value)}>Reduced motion</button>
					<button type="button" onClick={() => setShowSource((value) => !value)}>{showSource ? "Explainer" : "Source"}</button>
					<button type="button" disabled={!source} onClick={() => { if (source) void navigator.clipboard.writeText(source); }}>Copy source</button>
					<button type="button" disabled={!posterUrl} onClick={exportStill}>Export still</button>
					<button type="button" onClick={() => { const token = retryToken.current; void (async () => { await bridges.visuals?.render?.(visualId); if (retryToken.current === token) setReload((value) => value + 1); })(); }}>Retry</button>
				</div>
			</div>
			{notice ? <p className="systems-visual-notice" role="status">{notice}</p> : null}
			{showSource ? <pre className="systems-visual-source" data-testid="visual-systems-dynamic-source">{source ?? error ?? "Source unavailable."}</pre> : scene ? (
				<>
					<div className="systems-dynamic-stage">
						<svg viewBox={`0 0 ${scene.canvas.width} ${scene.canvas.height}`} role="img" aria-label={artifact.title}>
							<defs><marker id={`arrow-${artifact.id}`} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" /></marker></defs>
							{scene.groups?.map((group) => { const state = stateAt(scene, group.id, timeMs, group); return state.visible === false ? null : <g key={group.id} opacity={state.opacity ?? 1} className={`${state.emphasis ? "is-emphasized" : ""}${styleClass(state.style)}`}><rect className="systems-dynamic-group" x={state.x} y={state.y} width={state.width} height={state.height} rx="12"/><text className="systems-dynamic-group-label" x={state.x + 14} y={state.y + 24}>{state.label}</text></g>; })}
							{scene.edges?.map((edge, index) => { const from = nodeMap.get(edge.from); const to = nodeMap.get(edge.to); const state = stateAt(scene, edge.id ?? `${edge.from}-${edge.to}`, timeMs, edge); if (!from || !to || state.visible === false) return null; const geometry = edgeGeometry(from, to, state.route); return <g key={edge.id ?? index} opacity={state.opacity ?? 1} className={`${state.emphasis ? "is-emphasized" : ""}${styleClass(state.style)}`}><title>{edge.label}</title><path className="systems-dynamic-edge" d={geometry.path} markerEnd={state.directed === false ? undefined : `url(#arrow-${artifact.id})`}/>{edge.label ? <text className="systems-dynamic-edge-label" x={geometry.labelX} y={geometry.labelY} textAnchor="middle">{compactEdgeLabel(edge.label)}</text> : null}</g>; })}
							{scene.nodes.map((node) => { const state = nodeMap.get(node.id)!; return state.visible === false ? null : <g key={node.id} opacity={state.opacity ?? 1} className={`${state.emphasis ? "is-emphasized" : ""}${styleClass(state.style)}`}><rect className="systems-dynamic-node" x={state.x} y={state.y} width={state.width} height={state.height} rx="8"/><SvgWrappedLabel className="systems-dynamic-node-label" label={state.label} x={state.x} y={state.y} width={state.width} height={state.height}/></g>; })}
							{scene.notes?.map((note, index) => { const target = note.id ?? `note-${index}`; const state = stateAt(scene, target, timeMs, note); const noteHeight = 56; return state.visible === false ? null : <g key={target} opacity={state.opacity ?? 1} className={`${state.emphasis ? "is-emphasized" : ""}${styleClass(state.style)}`}><rect className="systems-dynamic-note" x={state.x} y={state.y} width={state.width} height={noteHeight} rx="6"/><SvgWrappedLabel className="systems-dynamic-note-label" label={state.text} x={state.x} y={state.y} width={state.width} height={noteHeight} fontSize={12}/></g>; })}
						</svg>
					</div>
					<div className="systems-dynamic-timeline">
						<input type="range" min="0" max={scene.durationMs} step="10" value={timeMs} aria-label="Explainer timeline" onChange={(event) => { setPlaying(false); setTimeMs(Number(event.currentTarget.value)); }}/>
						<span>{Math.round(timeMs / 100) / 10}s / {Math.round(scene.durationMs / 100) / 10}s</span>
					</div>
					{activeBeat ? <div className="systems-dynamic-caption" aria-live="polite"><strong>{activeBeatIndex + 1}/{scene.beats.length} · {activeBeat.caption}</strong>{activeBeat.description ? <span>{activeBeat.description}</span> : null}</div> : null}
				</>
			) : posterUrl ? <div className="systems-dynamic-stage"><img src={posterUrl} alt={artifact.title} /></div> : <p className="visual-loading">{parsedScene.error ?? error ?? "Loading systems explainer…"}</p>}
		</div>
	);
}
