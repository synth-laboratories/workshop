import { useEffect, useMemo, useState } from "react";
import { commands, type ExperimentGroup, type ExperimentNode } from "../generated/protocol";
import { fromGenerated, n } from "../bridge";

const missing = (value: unknown) => value == null || value === "" ? "—" : String(value);

export function orderLineageNodes(nodes: ExperimentNode[], edges: ExperimentGroup["edges"]): ExperimentNode[] {
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

export function ExperimentsPage({ initialId, onBack }: { initialId?: string; onBack: () => void }) {
	const [query, setQuery] = useState("");
	const [rows, setRows] = useState<ExperimentGroup[]>([]);
	const [selectedId, setSelectedId] = useState<string | null>(initialId ?? null);
	const [nodeId, setNodeId] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => { void fromGenerated(commands.experimentsList(n(query))).then(setRows).catch((e) => setError(String(e))); }, [query]);
	const selected = useMemo(() => rows.find((row) => row.id === selectedId) ?? null, [rows, selectedId]);
	const lineageNodes = useMemo(() => selected ? orderLineageNodes(selected.nodes, selected.edges) : [], [selected]);
	const node = lineageNodes.find((item) => item.id === nodeId) ?? lineageNodes[0] ?? null;

	if (!selected) return <section className="experiments-page" data-testid="experiments-index">
		<header><button type="button" onClick={onBack}>Back</button><div><span className="eyebrow">LOCAL REGISTRY</span><h1>Experiments</h1><p>Durable comparisons and explicit lineage. Nothing is uploaded.</p></div></header>
		<label className="experiment-search">Search experiments<input autoFocus value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Title, task, model, or status" /></label>
		{error ? <p role="alert">{error}</p> : null}
		<div className="experiment-table" role="table">
			<div className="experiment-row heading" role="row"><span>Experiment</span><span>Task / model</span><span>Status</span><span>Result</span><span>Runs</span><span>Updated</span></div>
			{rows.map((row) => <button className="experiment-row" role="row" key={row.id} onClick={() => { setSelectedId(row.id); setNodeId(null); }}>
				<strong>{row.title}</strong><span>{missing(row.task)} · {missing(row.model)}</span><span className={`status ${row.status}`}>{row.status}</span><span>{row.bestResult ? JSON.stringify(row.bestResult) : "—"}</span><span>{row.nodes.length}</span><time>{new Date(row.updatedAt).toLocaleString()}</time>
			</button>)}
		</div>
	</section>;

	return <section className="experiments-page detail" data-testid="experiment-detail">
		<header><button type="button" onClick={() => { setSelectedId(null); setNodeId(null); }}>All experiments</button><div><span className="eyebrow">{selected.status} · LOCAL ONLY</span><h1>{selected.title}</h1><p>{missing(selected.task)} · {missing(selected.model)} · updated {new Date(selected.updatedAt).toLocaleString()}</p></div></header>
		<div className="lineage-workspace">
			<div className="lineage-canvas" role="listbox" aria-label="Related run lineage">
				{lineageNodes.map((item, index) => <div className="lineage-step" key={item.id}>
					<button role="option" aria-selected={node?.id === item.id} className={`lineage-node ${item.kind}`} onClick={() => setNodeId(item.id)} onKeyDown={(event) => { if (event.key === "ArrowRight") setNodeId(lineageNodes[Math.min(index + 1, lineageNodes.length - 1)]?.id ?? item.id); if (event.key === "ArrowLeft") setNodeId(lineageNodes[Math.max(index - 1, 0)]?.id ?? item.id); }}>
						<span>{item.kind}</span><strong>{item.title}</strong><small>{item.status}</small>
					</button>
					{selected.edges.filter((edge) => edge.sourceNodeId === item.id).map((edge) => <span className="lineage-edge" key={edge.id}>→<em>{edge.relation.replaceAll("_", " ")}</em></span>)}
				</div>)}
			</div>
			<NodeInspector node={node} />
		</div>
	</section>;
}

function NodeInspector({ node }: { node: ExperimentNode | null }) {
	if (!node) return <aside className="experiment-inspector">Select a node</aside>;
	const rows: [string, unknown][] = [["Kind", node.kind], ["Status", node.status], ["Config", JSON.stringify(node.config)], ["Progress / metrics", node.metrics ? JSON.stringify(node.metrics) : null], ["Known cost", node.costUsd == null ? null : `$${node.costUsd.toFixed(4)}`], ["Artifacts", node.artifactRefs.length ? node.artifactRefs.join(", ") : null], ["Traces", node.traceRefs.length ? node.traceRefs.join(", ") : null], ["Provenance", Object.keys(node.provenance ?? {}).length ? JSON.stringify(node.provenance) : null]];
	return <aside className="experiment-inspector" data-testid="experiment-node-inspector"><span className="eyebrow">NODE INSPECTOR</span><h2>{node.title}</h2><dl>{rows.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{missing(value)}</dd></div>)}</dl></aside>;
}
