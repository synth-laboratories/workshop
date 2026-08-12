import { useCallback, useEffect, useState } from "react";
import { COMMANDS, invokeCommand } from "../bridge";
import { formatCount, formatMs, formatTps, useInferenceMonitor } from "./InferencePanel";

import { MODEL_OBSERVABILITY_REFRESH_MS } from "../limits";

export type CloudModelPerformance = {
	modelId: string;
	provider: string;
	sampleCount: number;
	inputTokens: number;
	cachedInputTokens: number;
	outputTokens: number;
	totalTokens: number;
	outputTpsP50: number;
	outputTpsP95: number;
	totalTpmP50: number;
	totalTpmP95: number;
	latencyMsP50: number;
	latencyMsP95: number;
};

export type CloudModelPerformanceSnapshot = {
	windowMinutes: number;
	generatedAt: string;
	models: CloudModelPerformance[];
};

const WINDOW_MINUTES = 24 * 60;
const REFRESH_MS = MODEL_OBSERVABILITY_REFRESH_MS;

export function formatTpm(value: number | null | undefined): string {
	if (typeof value !== "number" || !Number.isFinite(value) || value < 0) return "Unavailable";
	return Math.round(value).toLocaleString("en-US");
}

type MetricRow = { label: string; p50: string; p95: string; unit?: string };

function displayMetric(value: string, unit?: string): string {
	return value === "Unavailable" || !unit ? value : `${value} ${unit}`;
}

function MetricsTable({ rows }: { rows: MetricRow[] }) {
	return (
		<table className="model-o11y-metrics">
			<thead>
				<tr><th>Metric</th><th>Median <span>p50</span></th><th>Tail <span>p95</span></th></tr>
			</thead>
			<tbody>
				{rows.map((row) => (
					<tr key={row.label}>
						<th scope="row">{row.label}</th>
						<td>{displayMetric(row.p50, row.unit)}</td>
						<td>{displayMetric(row.p95, row.unit)}</td>
					</tr>
				))}
			</tbody>
		</table>
	);
}

export function ModelObservabilitySettings() {
	const local = useInferenceMonitor({ visible: true, historyLimit: 30 });
	const [cloud, setCloud] = useState<CloudModelPerformanceSnapshot | null>(null);
	const [cloudError, setCloudError] = useState<string | null>(null);
	const [refreshing, setRefreshing] = useState(false);

	const refresh = useCallback(async () => {
		setRefreshing(true);
		try {
			setCloud(await invokeCommand<CloudModelPerformanceSnapshot>(COMMANDS.MODEL_PERFORMANCE_GET, { windowMinutes: WINDOW_MINUTES }));
			setCloudError(null);
		} catch (reason) {
			const raw = reason instanceof Error ? reason.message : String(reason);
			setCloud(null);
			setCloudError(summarizeCloudTelemetryError(raw));
		} finally {
			setRefreshing(false);
		}
	}, []);

	useEffect(() => {
		void refresh();
		const timer = window.setInterval(() => void refresh(), REFRESH_MS);
		return () => window.clearInterval(timer);
	}, [refresh]);

	const rolling = local.snapshot?.rolling;
	const localSamples = rolling?.requestsCompleted ?? 0;

	return (
		<section className="model-o11y" data-testid="model-observability">
			<header className="model-o11y-head">
				<div>
					<h4>Observability</h4>
					<p>Measured throughput and latency for local Laguna and settled Synth Cloud traffic (24h).</p>
				</div>
				<button type="button" className="settings-secondary-btn" onClick={() => void refresh()} disabled={refreshing}>
					{refreshing ? "Refreshing…" : "Refresh"}
				</button>
			</header>

			<div className="model-o11y-grid">
				<article className="model-o11y-card" data-testid="model-o11y-local">
					<header><span>LOCAL · MLX</span><strong>{local.snapshot?.model ?? "Laguna XS 2.1"}</strong></header>
					{rolling ? (
						<>
							<MetricsTable rows={[
								{ label: "Output speed", p50: formatTps(rolling.decodeTpsP50), p95: formatTps(rolling.decodeTpsP95), unit: "tok/s" },
								{
									label: "Output rate",
									p50: formatTpm(rolling.decodeTpsP50 === null ? null : rolling.decodeTpsP50 * 60),
									p95: formatTpm(rolling.decodeTpsP95 === null ? null : rolling.decodeTpsP95 * 60),
									unit: "tok/min"
								},
								{ label: "Latency", p50: formatMs(rolling.latencyP50Ms), p95: formatMs(rolling.latencyP95Ms) }
							]} />
							<footer>{formatCount(localSamples)} completed · {formatCount(rolling.outputTokens)} output tokens · daemon lifetime</footer>
						</>
					) : <p className="model-o11y-empty">{local.error ?? "Reading local telemetry…"}</p>}
				</article>

				{cloud?.models.map((model) => (
					<article className="model-o11y-card" key={`${model.provider}:${model.modelId}`} data-testid="model-o11y-cloud">
						<header><span>SYNTH CLOUD · {model.provider}</span><strong>{model.modelId}</strong></header>
						<MetricsTable rows={[
							{ label: "End-to-end output", p50: formatTps(model.outputTpsP50), p95: formatTps(model.outputTpsP95), unit: "tok/s" },
							{ label: "Total rate", p50: formatTpm(model.totalTpmP50), p95: formatTpm(model.totalTpmP95), unit: "tok/min" },
							{ label: "Latency", p50: formatMs(model.latencyMsP50), p95: formatMs(model.latencyMsP95) }
						]} />
						<footer>{formatCount(model.sampleCount)} settled · {formatCount(model.totalTokens)} total tokens · {cloud.windowMinutes}m window</footer>
					</article>
				))}

				{cloudError ? (
					<article className="model-o11y-card model-o11y-card-muted" data-testid="model-o11y-cloud-unavailable">
						<header><span>SYNTH CLOUD</span><strong>Telemetry unavailable</strong></header>
						<p className="model-o11y-empty">{cloudError}</p>
					</article>
				) : null}
			</div>
			{cloud && cloud.models.length === 0 && !cloudError ? (
				<p className="model-o11y-empty">No settled Synth Cloud requests in the last 24 hours.</p>
			) : null}
			<p className="model-o11y-definition">Cloud TPS is end-to-end output rate: output tokens ÷ dispatch-to-settlement time, including prefill and first-token latency. It is not decode-only TPS. TPM uses all processed tokens over that interval; cached input is included.</p>
		</section>
	);
}

function summarizeCloudTelemetryError(raw: string): string {
	const text = raw.trim();
	if (/sign in/i.test(text)) return "Sign in under Account to read Synth Cloud telemetry.";
	if (/backend-api|dns|failed to lookup|connection refused|error sending request|timed out|timeout/i.test(text)) {
		return "Synth Cloud telemetry could not be reached. Check Account → Synth backend URL, or try again when online.";
	}
	if (/401|403|unauthorized|forbidden/i.test(text)) {
		return "Synth Cloud rejected this key for telemetry. Re-check Account credentials.";
	}
	// Keep short product copy; never surface internal hostnames or raw transport stacks.
	return "Synth Cloud telemetry is unavailable right now.";
}
