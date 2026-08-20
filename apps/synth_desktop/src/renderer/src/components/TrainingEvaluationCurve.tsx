type EvaluationPoint = {
	phase?: string;
	step?: number | null;
	score?: number | null;
	loss?: number | null;
	delta?: number | null;
	checkpointId?: string | null;
	checkpoint_id?: string | null;
	digest?: string | null;
	artifact_digest?: string | null;
	metric?: string;
};

const WIDTH = 720;
const HEIGHT = 224;
const PAD = { top: 18, right: 18, bottom: 34, left: 44 };

function pathFor(values: Array<{ x: number; y: number }>): string {
	return values.map((point, index) => `${index === 0 ? "M" : "L"}${point.x.toFixed(2)},${point.y.toFixed(2)}`).join(" ");
}

export function TrainingEvaluationCurve({ evaluations, testId }: { evaluations: EvaluationPoint[]; testId: string }) {
	const points = evaluations
		.filter((evaluation): evaluation is EvaluationPoint & { score: number } => typeof evaluation.score === "number")
		.map((evaluation, index) => ({ ...evaluation, step: typeof evaluation.step === "number" ? evaluation.step : index }));
	if (points.length === 0) return null;
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
		<div className="training-evaluation-legend"><strong>Checkpoint evaluations</strong><span data-series="reward">Reward</span>{lossPoints.length > 0 ? <span data-series="loss">Loss</span> : null}<small>{points.filter((point) => point.phase === "checkpoint").length} checkpoints · {points.length} observations</small></div>
		<svg viewBox={`0 0 ${WIDTH} ${HEIGHT}`} role="img" aria-label={`Reward across ${points.length} evaluation observations`}>
			{ticks.map((tick) => <g key={tick}><line x1={PAD.left} x2={WIDTH - PAD.right} y1={y(tick)} y2={y(tick)} className="training-chart-grid" /><text x={PAD.left - 8} y={y(tick) + 4} textAnchor="end">{tick.toFixed(2)}</text></g>)}
			<path d={rewardPath} className="training-chart-line training-chart-reward" />
			{lossPoints.length > 1 ? <path d={lossPath} className="training-chart-line training-chart-loss" /> : null}
			{points.map((point) => <g key={`${point.phase}-${point.step}`} className="training-chart-point"><circle cx={x(point.step)} cy={y(point.score)} r="5" /><text x={x(point.step)} y={HEIGHT - 10} textAnchor="middle">{point.step}</text><title>{`${point.phase ?? "checkpoint"} · step ${point.step} · reward ${point.score.toFixed(3)}`}</title></g>)}
		</svg>
		<div className="training-evaluation-ledger">{points.map((point) => <article key={`${point.phase}-${point.step}`} data-phase={point.phase ?? "checkpoint"}><span>{point.phase ?? "checkpoint"}</span><strong>{point.score.toFixed(3)}</strong><small>{point.phase === "baseline" ? "reference" : typeof point.delta === "number" ? `${point.delta >= 0 ? "+" : ""}${point.delta.toFixed(3)} vs baseline` : point.metric ?? "reward"}</small><code>step {point.step} · {point.digest ?? point.artifact_digest ?? point.checkpointId ?? point.checkpoint_id ?? "pending digest"}</code></article>)}</div>
	</section>;
}
