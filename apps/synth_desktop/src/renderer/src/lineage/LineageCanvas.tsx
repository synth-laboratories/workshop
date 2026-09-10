import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
	NODE_HEIGHT,
	NODE_WIDTH,
	fitRankedToViewport,
	neighborId,
	optionDomId,
	rankDag,
	rankedOrder,
	type LayoutEdge,
	type LayoutNode,
	type ViewTransform,
} from "./layoutDag";

export function LineageCanvas({
	nodes,
	edges,
	selectedId,
	onSelect,
	label,
}: {
	nodes: LayoutNode[];
	edges: LayoutEdge[];
	selectedId: string | null;
	onSelect: (id: string) => void;
	label: string;
}) {
	const ranked = useMemo(() => rankDag(nodes, edges), [nodes, edges]);
	const ordered = useMemo(() => rankedOrder(ranked), [ranked]);
	const [view, setView] = useState<ViewTransform>({ x: 0, y: 0, scale: 1 });
	const drag = useRef<{ x: number; y: number; originX: number; originY: number } | null>(null);
	const canvasRef = useRef<HTMLDivElement>(null);
	const width = useMemo(
		() =>
			Math.max(
				360,
				...ranked.map((node) => node.x + NODE_WIDTH + 24),
			),
		[ranked],
	);
	const height = useMemo(
		() =>
			Math.max(
				240,
				...ranked.map((node) => node.y + NODE_HEIGHT + 24),
			),
		[ranked],
	);

	const viewport = () => {
		const box = canvasRef.current;
		return { width: box?.clientWidth ?? 0, height: box?.clientHeight ?? 0 };
	};

	const fit = useCallback(() => {
		setView(fitRankedToViewport(ranked, viewport()));
	}, [ranked]);

	const recenter = useCallback(() => {
		setView((current) => fitRankedToViewport(ranked, viewport(), { scale: current.scale }));
	}, [ranked]);

	useLayoutEffect(() => {
		fit();
	}, [fit]);

	useLayoutEffect(() => {
		const box = canvasRef.current;
		if (!box || typeof ResizeObserver === "undefined") return;
		const observer = new ResizeObserver(() => fit());
		observer.observe(box);
		return () => observer.disconnect();
	}, [fit]);

	const selectFirst = () => {
		const first = ordered[0];
		if (first) onSelect(first.id);
	};

	const activeId = selectedId && ranked.some((node) => node.id === selectedId) ? selectedId : null;

	return (
		<div className="lineage-canvas" ref={canvasRef}>
			<div
				role="listbox"
				aria-label={label}
				aria-activedescendant={activeId ? optionDomId(activeId) : undefined}
				tabIndex={0}
				style={{ position: "absolute", inset: 0 }}
				onWheel={(event) => {
					event.preventDefault();
					const next = Math.min(2.2, Math.max(0.45, view.scale * (event.deltaY > 0 ? 0.92 : 1.08)));
					setView((current) => ({ ...current, scale: next }));
				}}
				onPointerDown={(event) => {
					if (event.button !== 0 || (event.target as HTMLElement).closest("button")) return;
					drag.current = { x: event.clientX, y: event.clientY, originX: view.x, originY: view.y };
					(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
				}}
				onPointerMove={(event) => {
					if (!drag.current) return;
					setView({
						...view,
						x: drag.current.originX + (event.clientX - drag.current.x),
						y: drag.current.originY + (event.clientY - drag.current.y),
					});
				}}
				onPointerUp={() => {
					drag.current = null;
				}}
				onKeyDown={(event) => {
					if (!ordered.length) return;
					if (event.key === "ArrowUp" || event.key === "ArrowDown" || event.key === "ArrowLeft" || event.key === "ArrowRight" || event.key === "Home" || event.key === "End") {
						event.preventDefault();
						if (!activeId) {
							selectFirst();
							return;
						}
						if (event.key === "Home") {
							onSelect(ordered[0].id);
							return;
						}
						if (event.key === "End") {
							onSelect(ordered[ordered.length - 1].id);
							return;
						}
						if (event.key === "ArrowUp") onSelect(neighborId(ranked, activeId, "up"));
						if (event.key === "ArrowDown") onSelect(neighborId(ranked, activeId, "down"));
						if (event.key === "ArrowLeft") onSelect(neighborId(ranked, activeId, "left"));
						if (event.key === "ArrowRight") onSelect(neighborId(ranked, activeId, "right"));
					}
				}}
			>
				<div
					className="lineage-canvas-world"
					style={{
						width,
						height,
						transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})`,
					}}
				>
					<svg className="lineage-links" width={width} height={height} aria-hidden>
						{edges.map((edge) => {
							const source = ranked.find((node) => node.id === edge.sourceId);
							const target = ranked.find((node) => node.id === edge.targetId);
							if (!source || !target) return null;
							const x1 = source.x + NODE_WIDTH / 2;
							const y1 = source.y + NODE_HEIGHT;
							const x2 = target.x + NODE_WIDTH / 2;
							const y2 = target.y;
							const mid = (y1 + y2) / 2;
							return (
								<g key={edge.id} className={`lineage-link ${edge.relation}`}>
									<path
										d={`M ${x1} ${y1} C ${x1} ${mid}, ${x2} ${mid}, ${x2} ${y2}`}
										fill="none"
									/>
									<text x={(x1 + x2) / 2} y={mid - 6} textAnchor="middle">
										{edge.relation.replaceAll("_", " ")}
									</text>
								</g>
							);
						})}
					</svg>
					{ranked.map((item) => (
						<button
							key={item.id}
							id={optionDomId(item.id)}
							type="button"
							role="option"
							tabIndex={-1}
							aria-selected={activeId === item.id}
							className={`lineage-node ${item.kind}`}
							style={{ left: item.x, top: item.y, width: NODE_WIDTH, minHeight: NODE_HEIGHT }}
							onMouseDown={(event) => event.preventDefault()}
							onClick={() => onSelect(item.id)}
						>
							<span>{item.kind.replaceAll("_", " ")}</span>
							<strong>{item.title}</strong>
							<small>{item.status}</small>
							{item.reason ? <small>{item.reason}</small> : null}
						</button>
					))}
				</div>
			</div>
			<div className="lineage-workspace-actions" style={{ position: "absolute", top: 10, right: 10, zIndex: 1 }}>
				<button type="button" data-testid="lineage-fit" onClick={fit}>Fit</button>
				<button type="button" data-testid="lineage-recenter" onClick={recenter}>Recenter</button>
			</div>
			<div className="lineage-canvas-hint">Scroll to zoom · drag to pan</div>
		</div>
	);
}
