import { InferencePanel } from "./InferencePanel";

export function LocalInferencePage({ onBack }: { onBack: () => void }) {
	return (
		<section className="ws-page" data-testid="local-inference-page">
			<header className="ws-page-head">
				<button type="button" className="ws-back" onClick={onBack}>← Back</button>
				<div className="ws-page-head-text">
					<h1 className="ws-title">Local Inference</h1>
					<p className="ws-lede">Local model residency, generation activity, latency, and request health.</p>
				</div>
			</header>
			<InferencePanel visible />
		</section>
	);
}
