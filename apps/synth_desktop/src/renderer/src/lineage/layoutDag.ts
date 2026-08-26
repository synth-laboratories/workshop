export type LayoutNode = {
	id: string;
	kind: string;
	title: string;
	status: string;
};

export type LayoutEdge = {
	id: string;
	sourceId: string;
	targetId: string;
	relation: string;
};

export type RankedNode = LayoutNode & {
	rank: number;
	column: number;
	x: number;
	y: number;
};

export const NODE_WIDTH = 168;
export const NODE_HEIGHT = 96;
export const RANK_GAP = 56;
export const COLUMN_GAP = 28;

/** Assign topological ranks from stored edges. Isolated nodes stay in rank 0 in input order. */
export function rankDag(nodes: LayoutNode[], edges: LayoutEdge[]): RankedNode[] {
	const ids = new Set(nodes.map((node) => node.id));
	const incoming = new Map<string, number>();
	const outgoing = new Map<string, string[]>();
	for (const node of nodes) {
		incoming.set(node.id, 0);
		outgoing.set(node.id, []);
	}
	for (const edge of edges) {
		if (!ids.has(edge.sourceId) || !ids.has(edge.targetId) || edge.sourceId === edge.targetId) {
			continue;
		}
		incoming.set(edge.targetId, (incoming.get(edge.targetId) ?? 0) + 1);
		outgoing.get(edge.sourceId)?.push(edge.targetId);
	}
	const rank = new Map<string, number>();
	const queue = nodes.filter((node) => (incoming.get(node.id) ?? 0) === 0).map((node) => node.id);
	for (const id of queue) rank.set(id, 0);
	let seen = 0;
	while (seen < queue.length) {
		const id = queue[seen];
		seen += 1;
		const current = rank.get(id) ?? 0;
		for (const next of outgoing.get(id) ?? []) {
			rank.set(next, Math.max(rank.get(next) ?? 0, current + 1));
			const remain = (incoming.get(next) ?? 1) - 1;
			incoming.set(next, remain);
			if (remain === 0) queue.push(next);
		}
	}
	const columns = new Map<number, number>();
	return nodes.map((node) => {
		const nodeRank = rank.get(node.id) ?? 0;
		const column = columns.get(nodeRank) ?? 0;
		columns.set(nodeRank, column + 1);
		return {
			...node,
			rank: nodeRank,
			column,
			x: 24 + column * (NODE_WIDTH + COLUMN_GAP),
			y: 24 + nodeRank * (NODE_HEIGHT + RANK_GAP),
		};
	});
}

export function neighborId(
	nodes: RankedNode[],
	selectedId: string,
	direction: "up" | "down" | "left" | "right",
): string {
	const current = nodes.find((node) => node.id === selectedId) ?? nodes[0];
	if (!current) return selectedId;
	const ranked = nodes.filter((node) => {
		if (direction === "up") return node.rank === current.rank - 1;
		if (direction === "down") return node.rank === current.rank + 1;
		if (direction === "left") return node.rank === current.rank && node.column === current.column - 1;
		return node.rank === current.rank && node.column === current.column + 1;
	});
	if (direction === "up" || direction === "down") {
		ranked.sort((a, b) => Math.abs(a.column - current.column) - Math.abs(b.column - current.column));
	}
	return ranked[0]?.id ?? current.id;
}
