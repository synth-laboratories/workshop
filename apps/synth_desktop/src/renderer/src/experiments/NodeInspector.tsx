import { useMemo, useState } from "react";
import { commands, type ExperimentEvidenceRef, type ExperimentGroup, type ExperimentNode } from "../generated/protocol";
import { fromGenerated } from "../bridge";
import { bridges } from "../runtime/desktopBridge";
import { openTraceReference, VISUAL_REFERENCE_OPENED_EVENT } from "../runtime/visualReferences";

const missing = (value: unknown) => (value == null || value === "" ? "—" : String(value));

export function NodeInspector({
	node,
	group,
	onRelated,
}: {
	node: ExperimentNode | null;
	group: ExperimentGroup | null;
	onRelated?: () => void;
}) {
	const [openError, setOpenError] = useState<string | null>(null);
	const [selectedCandidates, setSelectedCandidates] = useState<string[]>([]);
	const [memberTargetId, setMemberTargetId] = useState("");
	const [memberRelation, setMemberRelation] = useState<"compared_with" | "promoted_to">("compared_with");
	const [busy, setBusy] = useState(false);
	const candidates = node?.candidates ?? [];
	const otherNodes = useMemo(
		() => (group?.nodes ?? []).filter((item) => item.id !== node?.id),
		[group, node],
	);

	if (!node) return <aside className="experiment-inspector">Select a node</aside>;

	const rows: [string, unknown][] = [
		["Kind", node.kind],
		["Status", node.status],
		["Config", JSON.stringify(node.config)],
		["Progress / metrics", node.metrics ? JSON.stringify(node.metrics) : null],
		["Known cost", node.costUsd == null ? null : `$${node.costUsd.toFixed(4)}`],
		["Provenance", Object.keys(node.provenance ?? {}).length ? JSON.stringify(node.provenance) : null],
	];

	const toggleCandidate = (id: string) => {
		setSelectedCandidates((current) =>
			current.includes(id) ? current.filter((item) => item !== id) : [...current, id],
		);
	};

	const relate = async (request: {
		relation: "compared_with" | "promoted_to";
		sourceKind: "member" | "candidate";
		sourceId: string;
		targetKind: "member" | "candidate";
		targetId: string;
	}) => {
		if (!group) return;
		setBusy(true);
		setOpenError(null);
		try {
			await fromGenerated(
				commands.experimentsRelate({
					experimentId: group.id,
					relation: request.relation,
					sourceKind: request.sourceKind,
					sourceId: request.sourceId,
					targetKind: request.targetKind,
					targetId: request.targetId,
					createdAt: new Date().toISOString(),
				}),
			);
			onRelated?.();
		} catch (error) {
			setOpenError(String(error));
		} finally {
			setBusy(false);
		}
	};

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

	const pair = selectedCandidates.slice(0, 2);

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
			{group && node.kind !== "experiment" ? (
				<section className="experiment-member-relate">
					<h3>Relate members</h3>
					<label>
						Target node
						<select value={memberTargetId} onChange={(event) => setMemberTargetId(event.target.value)}>
							<option value="">Select a node</option>
							{otherNodes.map((item) => (
								<option key={item.id} value={item.id}>{item.title}</option>
							))}
						</select>
					</label>
					<label>
						Relation
						<select
							value={memberRelation}
							onChange={(event) => setMemberRelation(event.target.value as "compared_with" | "promoted_to")}
						>
							<option value="compared_with">compared_with</option>
							<option value="promoted_to">promoted_to</option>
						</select>
					</label>
					<button
						type="button"
						data-testid="experiment-member-relate"
						disabled={busy || !memberTargetId}
						onClick={() =>
							void relate({
								relation: memberRelation,
								sourceKind: "member",
								sourceId: node.id,
								targetKind: "member",
								targetId: memberTargetId,
							})
						}
					>
						Relate
					</button>
				</section>
			) : null}
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
			{node.kind === "optimizer_run" ? (
				<section className="experiment-candidates" data-testid="experiment-candidate-list">
					<h3>Candidates</h3>
					{candidates.length ? (
						<>
							{candidates.map((candidate) => (
								<label className="experiment-candidate-row" key={candidate.id}>
									<input
										type="checkbox"
										checked={selectedCandidates.includes(candidate.id)}
										onChange={() => toggleCandidate(candidate.id)}
									/>
									<div>
										<strong>{candidate.producerCandidateId}</strong>
										<small>{missing(candidate.status)}</small>
										<small>compared_with {candidate.comparedWith?.length ? candidate.comparedWith.join(", ") : "—"}</small>
										<small>promoted_to {missing(candidate.promotedTo)}</small>
									</div>
									<button
										type="button"
										data-testid="experiment-candidate-promote"
										disabled={busy || pair.length !== 2 || !pair.includes(candidate.id)}
										onClick={() => {
											const targetId = pair.find((id) => id !== candidate.id);
											if (!targetId) return;
											void relate({
												relation: "promoted_to",
												sourceKind: "candidate",
												sourceId: candidate.id,
												targetKind: "candidate",
												targetId,
											});
										}}
									>
										Promote
									</button>
								</label>
							))}
							<button
								type="button"
								data-testid="experiment-candidate-compare"
								disabled={busy || pair.length !== 2}
								onClick={() =>
									void relate({
										relation: "compared_with",
										sourceKind: "candidate",
										sourceId: pair[0],
										targetKind: "candidate",
										targetId: pair[1],
									})
								}
							>
								Compare
							</button>
						</>
					) : (
						<p>—</p>
					)}
				</section>
			) : null}
		</aside>
	);
}
