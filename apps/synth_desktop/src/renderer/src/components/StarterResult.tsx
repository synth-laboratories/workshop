import type { RunOutcome } from "../runtime/starterResult";

type Props = {
	result: RunOutcome;
	onOpenVisual?: () => void;
	onRefresh: () => void;
	onContinue?: (prompt: string) => void;
};

function metric(value: number | null): string {
	return value == null ? "Unavailable" : value.toFixed(3);
}

export function StarterResult({ result, onOpenVisual, onRefresh, onContinue }: Props) {
	return (
		<section id="starter-result" className="starter-result" data-testid="starter-result" aria-labelledby="starter-result-title" tabIndex={-1}>
			<header className="starter-result-header">
				<div><span className="optimizer-eyebrow">Starter result</span><h2 id="starter-result-title">{result.starter.title}</h2></div>
				<span className={`optimizer-status ${result.state}`} data-testid="starter-result-state">{result.state}</span>
			</header>
			<div className="starter-result-zones">
				<section aria-labelledby="starter-outcome-title" data-testid="starter-result-outcome">
					<h3 id="starter-outcome-title">Outcome</h3>
					<strong>{result.headlineMetric ? `${result.headlineMetric.label} ${result.headlineMetric.value.toFixed(3)}` : "Metric unavailable"}</strong>
					<p>{result.reason}</p>
					<small>Run <code>{result.runId}</code> · Cost {result.usage.costUsd == null ? "unavailable" : `$${result.usage.costUsd.toFixed(2)}`}</small>
				</section>
				<section aria-labelledby="starter-compare-title" data-testid="starter-result-compare">
					<h3 id="starter-compare-title">Compare</h3>
					<dl><dt>Baseline</dt><dd>{metric(result.comparison.baseline)}</dd><dt>Candidate</dt><dd>{metric(result.comparison.candidate)}</dd><dt>Delta</dt><dd>{metric(result.comparison.delta)}</dd></dl>
					<p>{result.comparison.reason}</p>
				</section>
				<section aria-labelledby="starter-evidence-title" data-testid="starter-result-evidence">
					<h3 id="starter-evidence-title">Evidence</h3>
					<strong>{result.evidence.complete && result.evidence.inspectable ? "Complete and inspectable" : "Incomplete or unavailable"}</strong>
					<p>{result.evidence.reason}</p>
					{result.evidence.references.length > 0 ? <ul>{result.evidence.references.map((reference) => <li key={`${reference.kind}:${reference.id}`}><span>{reference.kind}</span> <code>{reference.id}</code></li>)}</ul> : null}
					{result.visualId && onOpenVisual ? <button className="secondary-button" type="button" onClick={onOpenVisual}>Open existing visual</button> : null}
				</section>
				<section aria-labelledby="starter-continue-title" data-testid="starter-result-continue">
					<h3 id="starter-continue-title">Continue</h3>
					<p>Review the retained result before proposing one bounded change. Nothing runs automatically.</p>
					<div className="starter-result-actions"><button className="secondary-button" type="button" onClick={onRefresh}>Refresh result</button>{onContinue ? <button className="secondary-button" type="button" onClick={() => onContinue(result.nextExperimentPrompt)}>Propose next experiment</button> : null}</div>
				</section>
			</div>
		</section>
	);
}
