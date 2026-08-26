import { useEffect, useMemo, useState } from "react";
import { commands, type ExperimentGroup, type ExperimentNode } from "../generated/protocol";
import { fromGenerated, n } from "../bridge";
import { orderLineageNodes } from "../lineage/orderLineageNodes";
import { ExperimentIndex } from "./ExperimentIndex";
import { ExperimentWorkspace } from "./ExperimentWorkspace";

export { orderLineageNodes } from "../lineage/orderLineageNodes";

export function ExperimentsPage({ initialId, onBack }: { initialId?: string; onBack: () => void }) {
	const [query, setQuery] = useState("");
	const [rows, setRows] = useState<ExperimentGroup[]>([]);
	const [selectedId, setSelectedId] = useState<string | null>(initialId ?? null);
	const [nodeId, setNodeId] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [busy, setBusy] = useState(false);

	const refresh = (keepId?: string | null) =>
		fromGenerated(commands.experimentsList(n(query)))
			.then((next) => {
				setRows(next);
				const preferred = keepId ?? selectedId;
				if (preferred && next.some((row) => row.id === preferred)) {
					setSelectedId(preferred);
				}
			})
			.catch((e) => setError(String(e)));

	useEffect(() => {
		void refresh();
	}, [query]);

	useEffect(() => {
		if (initialId) setSelectedId(initialId);
	}, [initialId]);

	const selected = useMemo(() => rows.find((row) => row.id === selectedId) ?? null, [rows, selectedId]);
	const memberNodes = useMemo(
		() => (selected ? orderLineageNodes(selected.nodes, selected.edges) : []),
		[selected],
	);
	const memberEdges = useMemo(
		() =>
			(selected?.edges ?? []).map((edge) => ({
				id: edge.id,
				sourceId: edge.sourceNodeId,
				targetId: edge.targetNodeId,
				relation: edge.relation,
			})),
		[selected],
	);
	const forestNodes: ExperimentNode[] = useMemo(
		() =>
			rows.map((row) => ({
				id: row.id,
				kind: "experiment",
				title: row.title,
				status: row.status,
				config: {},
				metrics: row.bestResult,
				costUsd: null,
				artifactRefs: [],
				traceRefs: [],
				evidenceRefs: [],
				provenance: {},
				createdAt: row.createdAt,
				updatedAt: row.updatedAt,
			})),
		[rows],
	);
	const forestEdges = useMemo(
		() =>
			rows.flatMap((row) =>
				(row.lineage ?? []).map((edge) => ({
					id: edge.id,
					sourceId: edge.sourceExperimentId,
					targetId: edge.targetExperimentId,
					relation: edge.relation,
				})),
			),
		[rows],
	);
	const canvasNodes = selected ? memberNodes : forestNodes;
	const canvasEdges = selected ? memberEdges : forestEdges;
	const inspectorNode = canvasNodes.find((item) => item.id === nodeId) ?? canvasNodes[0] ?? null;

	const selectExperiment = (id: string) => {
		const row = rows.find((item) => item.id === id);
		setSelectedId(id);
		setNodeId(null);
		if (!row) return;
		void fromGenerated(commands.experimentsActivate(row.sessionId, row.id)).catch((e) => setError(String(e)));
	};

	const selectCanvasNode = (id: string) => {
		const experiment = rows.find((row) => row.id === id);
		if (experiment) {
			selectExperiment(id);
			return;
		}
		setNodeId(id);
	};

	const createChild = async () => {
		if (!selected) return;
		setBusy(true);
		setError(null);
		try {
			const child = await fromGenerated(
				commands.experimentsCreateChild({
					parentExperimentId: selected.id,
					sessionId: selected.sessionId,
					requestId: `child:${selected.id}:${crypto.randomUUID()}`,
					title: `Follow-up: ${selected.title}`,
					task: selected.task,
					model: selected.model,
					createdAt: new Date().toISOString(),
				}),
			);
			await refresh(child.id);
			setNodeId(null);
		} catch (e) {
			setError(String(e));
		} finally {
			setBusy(false);
		}
	};

	return (
		<section className="experiments-page workbench" data-testid="experiments-workbench">
			<header>
				<button type="button" onClick={onBack}>Back</button>
				<div>
					<span className="eyebrow">LOCAL REGISTRY</span>
					<h1>Experiments</h1>
					<p>Durable comparisons and explicit lineage. Nothing is uploaded.</p>
				</div>
			</header>
			<div className="experiments-workbench">
				<ExperimentIndex
					query={query}
					rows={rows}
					selectedId={selectedId}
					error={error}
					onQuery={setQuery}
					onSelect={selectExperiment}
				/>
				<ExperimentWorkspace
					group={selected}
					lineageNodes={canvasNodes}
					edges={canvasEdges}
					node={inspectorNode}
					onSelectNode={selectCanvasNode}
					onCreateChild={() => void createChild()}
					onShowForest={() => {
						setSelectedId(null);
						setNodeId(null);
					}}
					busy={busy}
				/>
			</div>
		</section>
	);
}
