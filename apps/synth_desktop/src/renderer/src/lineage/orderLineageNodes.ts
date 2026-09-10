import type { ExperimentGroup, ExperimentNode } from "../generated/protocol";

export function orderLineageNodes(
	nodes: ExperimentNode[],
	edges: ExperimentGroup["edges"],
): ExperimentNode[] {
	const byId = new Map(nodes.map((node) => [node.id, node]));
	const incoming = new Set(edges.map((edge) => edge.targetNodeId));
	const ordered: ExperimentNode[] = [];
	const visited = new Set<string>();
	const firstEdge = new Map<string, (typeof edges)[number]>();
	for (const edge of edges) if (!firstEdge.has(edge.sourceNodeId)) firstEdge.set(edge.sourceNodeId, edge);
	let current: ExperimentNode | undefined = nodes.find((node) => !incoming.has(node.id)) ?? nodes[0];
	while (current && !visited.has(current.id)) {
		ordered.push(current);
		visited.add(current.id);
		const edge = firstEdge.get(current.id);
		current = edge ? byId.get(edge.targetNodeId) : undefined;
	}
	return [...ordered, ...nodes.filter((node) => !visited.has(node.id))];
}
