import { useEffect, useRef, useState } from "react";
import { traceResearchClient } from "../runtime/traceResearch";
import { openTraceReference, VISUAL_REFERENCE_OPENED_EVENT } from "../runtime/visualReferences";

type EvaluationPoint = {
	phase?: string;
	step?: number | null;
	score?: number | null;
	loss?: number | null;
	delta?: number | null;
	macro_f1?: number | null;
	ci_low?: number | null;
	ci_high?: number | null;
	confidence?: number | null;
	paired_n?: number | null;
	verdict?: string | null;
	claim_ready?: boolean | null;
	checkpointId?: string | null;
	checkpoint_id?: string | null;
	digest?: string | null;
	artifact_digest?: string | null;
	metric?: string;
	evaluator?: string | null;
	sample_count?: number | null;
	status?: string;
	detail?: unknown;
};

const WIDTH = 720;
const HEIGHT = 224;
const PAD = { top: 18, right: 18, bottom: 34, left: 44 };

function pathFor(values: Array<{ x: number; y: number }>): string {
	return values.map((point, index) => `${index === 0 ? "M" : "L"}${point.x.toFixed(2)},${point.y.toFixed(2)}`).join(" ");
}

function checkpointIdentity(point: EvaluationPoint): string {
	return point.checkpointId ?? point.checkpoint_id ?? (point.step === 0 ? "Base model" : `Step ${point.step ?? "—"}`);
}

function EvaluationReviewDialog({ evaluation, onClose }: { evaluation: EvaluationPoint; onClose: () => void }) {
	const closeRef = useRef<HTMLButtonElement>(null);
	const identity = checkpointIdentity(evaluation);
    const childId = (evaluation.detail as {childRunId?: string} | undefined)?.childRunId;
    const [tracePage, setTracePage] = useState<{snapshotId: string; rows: Array<{trialId?: string; traceDigest?: string}>; resultIds: string[]} | null>(null);
    const [source, setSource] = useState<unknown>(null);
    const [evidenceError, setEvidenceError] = useState<string | null>(null);
    const [loading, setLoading] = useState(false);
    const inspectRollouts = async () => {
        setLoading(true); setEvidenceError(null);
        try { setTracePage(await traceResearchClient.request("query", {query: {schemaVersion: "synth.trace-query.v2", evalJobIds: [childId], grain: "episodes", limit: 100}}) as typeof tracePage); }
        catch (error) { setEvidenceError(error instanceof Error ? error.message : String(error)); }
        finally { setLoading(false); }
    };
    const inspectSource = async (index: number) => {
        try { setSource(await traceResearchClient.request("source", {snapshot_id: tracePage!.snapshotId, result_id: tracePage!.resultIds[index], source_limit: 16000})); }
        catch (error) { setEvidenceError(error instanceof Error ? error.message : String(error)); }
    };
    const openTrace = async (digest: string) => {
        try { const visual = await openTraceReference(digest); onClose(); window.dispatchEvent(new CustomEvent(VISUAL_REFERENCE_OPENED_EVENT, {detail: visual})); }
        catch (error) { setEvidenceError(error instanceof Error ? error.message : String(error)); }
    };

	useEffect(() => {
		const onKeyDown = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
		document.addEventListener("keydown", onKeyDown);
		requestAnimationFrame(() => closeRef.current?.focus());
		return () => document.removeEventListener("keydown", onKeyDown);
	}, [onClose]);
	return <div className="ws-dialog-scrim" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }} data-testid="training-evaluation-dialog-scrim">
		<div className="ws-dialog training-evaluation-dialog" role="dialog" aria-modal="true" aria-labelledby="training-evaluation-dialog-title" data-testid="training-evaluation-dialog">
			<div className="ws-dialog-head"><div><span className="ws-eyebrow">{evaluation.phase ?? "checkpoint"} evaluation</span><h2 className="ws-dialog-title" id="training-evaluation-dialog-title">{identity}</h2></div><button ref={closeRef} type="button" className="ws-btn ws-btn-ghost ws-btn-small" onClick={onClose} aria-label="Close evaluation review">Close</button></div>
			<section className="run-progress-section" aria-label="Evaluation identity"><dl className="ws-kv"><dt>Step</dt><dd>{evaluation.step ?? "—"}</dd><dt>Status</dt><dd>{evaluation.status ?? "completed"}</dd><dt>Digest</dt><dd className="ws-mono">{evaluation.digest ?? evaluation.artifact_digest ?? "—"}</dd><dt>Evaluator</dt><dd>{evaluation.evaluator ?? "—"}</dd></dl></section>
			<section className="optimizer-eval-scorecard" aria-label="Evaluation scorecard"><span className="optimizer-eyebrow">Scorecard</span><table><thead><tr><th>Candidate</th><th>Stage</th><th>Valid</th><th>{evaluation.metric ?? "Reward"}</th><th>Macro-F1</th><th>Paired lift</th></tr></thead><tbody><tr><td>{identity}</td><td>{evaluation.phase ?? "checkpoint"}</td><td>{evaluation.paired_n ?? evaluation.sample_count ?? "—"}</td><td>{typeof evaluation.score === "number" ? evaluation.score.toFixed(3) : "—"}</td><td>{typeof evaluation.macro_f1 === "number" ? evaluation.macro_f1.toFixed(3) : "—"}</td><td>{typeof evaluation.delta === "number" ? `${evaluation.delta >= 0 ? "+" : ""}${evaluation.delta.toFixed(3)}` : "—"}</td></tr></tbody></table></section>
			{typeof evaluation.delta === "number" ? <section className="run-progress-section" aria-label="Paired uplift decision"><dl className="ws-kv"><dt>Verdict</dt><dd>{evaluation.verdict?.replaceAll("_", " ") ?? "inconclusive"}</dd><dt>{Math.round((evaluation.confidence ?? 0.95) * 100)}% interval</dt><dd>{typeof evaluation.ci_low === "number" && typeof evaluation.ci_high === "number" ? `${evaluation.ci_low >= 0 ? "+" : ""}${evaluation.ci_low.toFixed(3)} to ${evaluation.ci_high >= 0 ? "+" : ""}${evaluation.ci_high.toFixed(3)}` : "—"}</dd><dt>Claim</dt><dd>{evaluation.claim_ready === true ? "Uplift supported" : "Not established"}</dd></dl></section> : null}
            {childId ? <section className="training-evaluation-rollouts" aria-label="Child evaluation rollouts"><p>Child evaluation <code>{childId}</code></p><button type="button" disabled={loading} onClick={() => void inspectRollouts()}>{loading ? "Loading retained rollouts…" : "Inspect child evaluation rollouts"}</button>
                {tracePage ? <><p>Saved snapshot <code>{tracePage.snapshotId}</code></p>{tracePage.rows.map((row, index) => <div key={tracePage.resultIds[index]}><code>{row.trialId ?? `Rollout ${index + 1}`}</code><button type="button" onClick={() => void inspectSource(index)}>Exact source</button>{row.traceDigest ? <button type="button" onClick={() => void openTrace(row.traceDigest!)}>Open retained trace</button> : null}</div>)}</> : null}
                {evidenceError ? <p role="alert">{evidenceError}</p> : null}{source != null ? <details className="training-evaluation-evidence" open><summary>Exact snapshot source</summary><pre>{JSON.stringify(source, null, 2)}</pre></details> : null}
            </section> : null}
			{evaluation.detail != null ? <details className="training-evaluation-evidence"><summary>Evidence receipt</summary><pre>{JSON.stringify(evaluation.detail, null, 2)}</pre></details> : null}
		</div>
	</div>;
}

export function TrainingEvaluationCurve({ evaluations, testId }: { evaluations: EvaluationPoint[]; testId: string }) {
	const [selected, setSelected] = useState<EvaluationPoint | null>(null);
	const points = evaluations
		.filter((evaluation): evaluation is EvaluationPoint & { score: number } => typeof evaluation.score === "number")
		.map((evaluation, index) => ({ ...evaluation, step: typeof evaluation.step === "number" ? evaluation.step : index }));
	if (points.length === 0) return <section className="training-evaluation-plot" data-testid={testId} aria-label="Checkpoint evaluation evidence">
		<div className="training-evaluation-legend"><strong>Checkpoint evaluations</strong><small>{evaluations.length} observations · no scores returned</small></div>
		<div className="training-evaluation-ledger">{evaluations.map((point, index) => <button type="button" key={`${point.phase}-${point.step}-${index}`} data-phase={point.phase ?? "checkpoint"} onClick={() => setSelected(point)} aria-label={`Review ${point.phase ?? "checkpoint"} evaluation at step ${point.step ?? "unknown"}`}><span>{point.phase ?? "checkpoint"}</span><strong>—</strong><small>{point.status ?? "unscored"}</small><code>step {point.step ?? "—"} · {point.digest ?? point.artifact_digest ?? point.checkpointId ?? point.checkpoint_id ?? "no artifact digest"}</code></button>)}</div>
		{selected ? <EvaluationReviewDialog evaluation={selected} onClose={() => setSelected(null)} /> : null}
	</section>;
	const steps = points.map((point) => point.step);
	const rewards = points.map((point) => point.score);
	const losses = points.flatMap((point) => typeof point.loss === "number" ? [point.loss] : []);
	const xMin = Math.min(...steps);
	const xMax = Math.max(...steps);
	const yValues = [...rewards, ...losses];
	const yMinRaw = Math.min(...yValues);
	const yMaxRaw = Math.max(...yValues);
	const yPadding = Math.max((yMaxRaw - yMinRaw) * 0.12, 0.04);
	const yMin = Math.max(0, yMinRaw - yPadding);
	const yMax = yMaxRaw + yPadding;
	const plotWidth = WIDTH - PAD.left - PAD.right;
	const plotHeight = HEIGHT - PAD.top - PAD.bottom;
	const x = (step: number) => PAD.left + ((step - xMin) / Math.max(xMax - xMin, 1)) * plotWidth;
	const y = (value: number) => PAD.top + (1 - ((value - yMin) / Math.max(yMax - yMin, 0.001))) * plotHeight;
	const rewardPath = pathFor(points.map((point) => ({ x: x(point.step), y: y(point.score) })));
	const lossPoints = points.filter((point): point is typeof point & { loss: number } => typeof point.loss === "number");
	const lossPath = pathFor(lossPoints.map((point) => ({ x: x(point.step), y: y(point.loss) })));
	const ticks = [yMax, (yMin + yMax) / 2, yMin];

	return <section className="training-evaluation-plot" data-testid={testId} aria-label="Reward and loss by training checkpoint">
		<div className="training-evaluation-legend"><strong>Checkpoint evaluations</strong><span data-series="reward">Reward</span>{lossPoints.length > 0 ? <span data-series="loss">Loss</span> : null}<small>{new Set(points.filter((point) => point.phase !== "baseline").map(checkpointIdentity)).size} checkpoints · {points.length} observations</small></div>
		<svg viewBox={`0 0 ${WIDTH} ${HEIGHT}`} role="img" aria-label={`Reward across ${points.length} evaluation observations`}>
			{ticks.map((tick) => <g key={tick}><line x1={PAD.left} x2={WIDTH - PAD.right} y1={y(tick)} y2={y(tick)} className="training-chart-grid" /><text x={PAD.left - 8} y={y(tick) + 4} textAnchor="end">{tick.toFixed(2)}</text></g>)}
			<path d={rewardPath} className="training-chart-line training-chart-reward" />
			{lossPoints.length > 1 ? <path d={lossPath} className="training-chart-line training-chart-loss" /> : null}
			{points.map((point) => <g key={`${point.phase}-${point.step}`} className="training-chart-point" role="button" tabIndex={0} aria-label={`Review ${point.phase ?? "checkpoint"} evaluation at step ${point.step}`} onClick={() => setSelected(point)} onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); setSelected(point); } }}><circle cx={x(point.step)} cy={y(point.score)} r="5" /><text x={x(point.step)} y={HEIGHT - 10} textAnchor="middle">{point.step}</text><title>{`${point.phase ?? "checkpoint"} · step ${point.step} · reward ${point.score.toFixed(3)}`}</title></g>)}
		</svg>
		<div className="training-evaluation-ledger">{points.map((point) => <button type="button" key={`${point.phase}-${point.step}`} data-phase={point.phase ?? "checkpoint"} onClick={() => setSelected(point)} aria-label={`Review ${point.phase ?? "checkpoint"} evaluation at step ${point.step}`}><span>{point.phase ?? "checkpoint"}</span><strong>{point.score.toFixed(3)}</strong><small>{point.phase === "baseline" ? "reference" : typeof point.delta === "number" ? `${point.delta >= 0 ? "+" : ""}${point.delta.toFixed(3)} vs baseline` : point.metric ?? "reward"}</small><code>step {point.step} · {point.digest ?? point.artifact_digest ?? point.checkpointId ?? point.checkpoint_id ?? "pending digest"}</code></button>)}</div>
		{selected ? <EvaluationReviewDialog evaluation={selected} onClose={() => setSelected(null)} /> : null}
	</section>;
}
