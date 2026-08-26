import type { ExperimentGroup, ExperimentNode } from "../generated/protocol";
import { LineageCanvas } from "../lineage/LineageCanvas";
import { NodeInspector } from "./NodeInspector";

const missing = (value: unknown) => (value == null || value === "" ? "—" : String(value));

export function ExperimentWorkspace({
	group,
	lineageNodes,
	edges,
	node,
	onSelectNode,
	onCreateChild,
	onFork,
	onRerun,
	onRelated,
	onShowForest,
	busy,
}: {
	group: ExperimentGroup | null;
	lineageNodes: ExperimentNode[];
	edges: { id: string; sourceId: string; targetId: string; relation: string }[];
	node: ExperimentNode | null;
	onSelectNode: (id: string) => void;
	onCreateChild: () => void;
	onFork: () => void;
	onRerun: () => void;
	onRelated: () => void;
	onShowForest: () => void;
	busy: boolean;
}) {
	return (
		<div className="lineage-workspace" data-testid="experiment-detail">
			<div className="lineage-workspace-main">
				{group ? (
					<header className="lineage-workspace-header">
						<div>
							<span className="eyebrow">{group.status} · LOCAL ONLY</span>
							<h2>{group.title}</h2>
							<p>{missing(group.task)} · {missing(group.model)} · updated {new Date(group.updatedAt).toLocaleString()}</p>
						</div>
						<div className="lineage-workspace-actions">
							<button type="button" onClick={onShowForest}>Experiment forest</button>
							<button type="button" className="experiment-child-button" disabled={busy} onClick={onCreateChild}>
								+ child experiment
							</button>
							<button type="button" data-testid="experiment-fork" disabled={busy} onClick={onFork}>
								+ fork
							</button>
							<button type="button" data-testid="experiment-rerun" disabled={busy} onClick={onRerun}>
								+ rerun
							</button>
						</div>
					</header>
				) : (
					<header className="lineage-workspace-header">
						<div>
							<span className="eyebrow">LINEAGE</span>
							<h2>Experiment forest</h2>
							<p>Select an experiment to inspect members and start a follow-up.</p>
						</div>
					</header>
				)}
				<LineageCanvas
					nodes={lineageNodes.map((item) => ({
						id: item.id,
						kind: item.kind,
						title: item.title,
						status: item.status,
					}))}
					edges={edges}
					selectedId={node?.id ?? null}
					onSelect={onSelectNode}
					label={group ? "Member lineage" : "Experiment lineage"}
				/>
			</div>
			<NodeInspector node={node} group={group} onRelated={onRelated} />
		</div>
	);
}
