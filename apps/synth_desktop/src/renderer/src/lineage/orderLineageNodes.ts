import type { ExperimentGroup, ExperimentNode } from "../generated/protocol";

export function orderLineageNodes(
	nodes: ExperimentNode[],
	edges: ExperimentGroup["edges"],
): ExperimentNode[] {
	const byId = new Map(nodes.map((node) => [node.id, node]));
	const incoming = new Set(edges.map((edge) => edge.targetNodeId));
	const ordered: ExperimentNode[] = [];
	let current: ExperimentNode | undefined = nodes.find((node) => !incoming.has(node.id)) ?? nodes[0];
	while (current && !ordered.some((node) => node.id === current?.id)) {
		ordered.push(current);
		const edge = edges.find((item) => item.sourceNodeId === current?.id);
		current = edge ? byId.get(edge.targetNodeId) : undefined;
	}
	return [...ordered, ...nodes.filter((node) => !ordered.some((item) => item.id === node.id))];
}
