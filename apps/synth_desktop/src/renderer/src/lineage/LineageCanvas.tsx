import { useEffect, useMemo, useRef, useState } from "react";
import {
	NODE_HEIGHT,
	NODE_WIDTH,
	neighborId,
	rankDag,
	type LayoutEdge,
	type LayoutNode,
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
	const [view, setView] = useState({ x: 0, y: 0, scale: 1 });
	const drag = useRef<{ x: number; y: number; originX: number; originY: number } | null>(null);
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

	useEffect(() => {
		setView({ x: 0, y: 0, scale: 1 });
	}, [nodes, edges]);

	return (
		<div
			className="lineage-canvas"
			role="listbox"
			aria-label={label}
			tabIndex={0}
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
				const current = selectedId ?? ranked[0]?.id;
				if (!current) return;
				if (event.key === "ArrowUp") {
					event.preventDefault();
					onSelect(neighborId(ranked, current, "up"));
				}
				if (event.key === "ArrowDown") {
					event.preventDefault();
					onSelect(neighborId(ranked, current, "down"));
				}
				if (event.key === "ArrowLeft") {
					event.preventDefault();
					onSelect(neighborId(ranked, current, "left"));
				}
				if (event.key === "ArrowRight") {
					event.preventDefault();
					onSelect(neighborId(ranked, current, "right"));
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
						type="button"
						role="option"
						aria-selected={selectedId === item.id}
						className={`lineage-node ${item.kind}`}
						style={{ left: item.x, top: item.y, width: NODE_WIDTH, minHeight: NODE_HEIGHT }}
						onClick={() => onSelect(item.id)}
					>
						<span>{item.kind.replaceAll("_", " ")}</span>
						<strong>{item.title}</strong>
						<small>{item.status}</small>
					</button>
				))}
			</div>
			<div className="lineage-canvas-hint">Scroll to zoom · drag to pan</div>
		</div>
	);
}
