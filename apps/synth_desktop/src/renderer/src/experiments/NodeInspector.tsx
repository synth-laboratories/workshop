import { useState } from "react";
import { type ExperimentEvidenceRef, type ExperimentNode } from "../generated/protocol";
import { bridges } from "../runtime/desktopBridge";
import { openTraceReference, VISUAL_REFERENCE_OPENED_EVENT } from "../runtime/visualReferences";

const missing = (value: unknown) => (value == null || value === "" ? "—" : String(value));

export function NodeInspector({ node }: { node: ExperimentNode | null }) {
	const [openError, setOpenError] = useState<string | null>(null);
	if (!node) return <aside className="experiment-inspector">Select a node</aside>;
	const rows: [string, unknown][] = [
		["Kind", node.kind],
		["Status", node.status],
		["Config", JSON.stringify(node.config)],
		["Progress / metrics", node.metrics ? JSON.stringify(node.metrics) : null],
		["Known cost", node.costUsd == null ? null : `$${node.costUsd.toFixed(4)}`],
		["Provenance", Object.keys(node.provenance ?? {}).length ? JSON.stringify(node.provenance) : null],
	];
	const openEvidence = async (evidence: ExperimentEvidenceRef) => {
		setOpenError(null);
		try {
			if (evidence.kind === "trace") {
				const reference = evidence.rolloutId ?? evidence.traceId;
				if (!reference) throw new Error("Trace reference is unavailable.");
				const visual = await openTraceReference(reference, evidence.containerId ?? undefined);
				window.dispatchEvent(new CustomEvent(VISUAL_REFERENCE_OPENED_EVENT, { detail: visual }));
				return;
			}
			if (evidence.kind === "visual" && evidence.visualId) {
				if (!bridges.visuals) throw new Error("The local visual registry is unavailable.");
				const visual = await bridges.visuals.get(evidence.visualId);
				await bridges.visuals.show(visual.id).catch(() => visual);
				window.dispatchEvent(new CustomEvent(VISUAL_REFERENCE_OPENED_EVENT, { detail: visual }));
				return;
			}
			throw new Error(evidence.artifactUri ? `Artifact retained at ${evidence.artifactUri}` : "Evidence is unavailable.");
		} catch (error) {
			setOpenError(String(error));
		}
	};
	return (
		<aside className="experiment-inspector" data-testid="experiment-node-inspector">
			<span className="eyebrow">NODE INSPECTOR</span>
			<h2>{node.title}</h2>
			<dl>
				{rows.map(([label, value]) => (
					<div key={label}>
						<dt>{label}</dt>
						<dd>{missing(value)}</dd>
					</div>
				))}
			</dl>
			<section className="experiment-evidence">
				<h3>Evidence</h3>
				{node.evidenceRefs.length ? (
					node.evidenceRefs.map((evidence) => (
						<div className="experiment-evidence-row" key={evidence.evidenceId}>
							<div>
								<strong>{evidence.label}</strong>
								<small>{evidence.kind} · {evidence.digest ?? "digest —"}</small>
							</div>
							<button type="button" onClick={() => void openEvidence(evidence)}>
								Open {evidence.kind === "visual" ? "plot" : evidence.kind}
							</button>
						</div>
					))
				) : (
					<p>—</p>
				)}
				{openError ? <p role="alert">{openError}</p> : null}
			</section>
		</aside>
	);
}
