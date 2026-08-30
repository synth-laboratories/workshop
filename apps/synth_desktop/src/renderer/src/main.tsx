import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { bridges, installDesktopBridge } from "./runtime/desktopBridge";
import { DIAGNOSTIC_CODES, installVisualDiagnosticSink, reportDiagnostic } from "./runtime/diagnostics";
import { installRunProgressDiagnostics } from "./runtime/runProgress/subscription";
import { installRunProgressTelemetry } from "./runtime/runProgress/telemetry";
import { SynthConnectionProvider } from "./hooks/useSynthConnection";
import "./styles/tokens.css";
import "./styles/primitives.css";
import "./styles/app.css";
import "./styles/training.css";
import "./styles/usage.css";
import "./styles/runProgress.css";
import "./styles/chartVisual.css";

installDesktopBridge();
// Visual bundles emit through a host-installed sink; without it they are silent.
installVisualDiagnosticSink();
// The run-progress store stays importable outside a webview, so the renderer
// hands it the real reporter here rather than importing the bridge itself.
installRunProgressDiagnostics((report) => {
	reportDiagnostic({
		optimizerRunId: report.runId,
		streamId: report.runId,
		severity: report.severity,
		component: "renderer",
		event: report.event,
		code: report.code === "stream_replay_gap"
			? DIAGNOSTIC_CODES.streamReplayGap
			: DIAGNOSTIC_CODES.streamInterrupted,
		message: report.message,
		retryable: true,
		details: report.details
	});
});
// One record per finished run: the experience budgets this feature is held to
// (time to first progress, update latency, estimate coverage) are not visible
// from a screenshot, so they are measured rather than assumed.
installRunProgressTelemetry((record) => {
	reportDiagnostic({
		optimizerRunId: record.runId,
		streamId: record.runId,
		severity: "info",
		component: "renderer",
		event: "run_progress.experience_budget",
		code: "run_progress_experience_budget",
		message: `run progress served ${record.samples} samples with ${(record.estimateCoverage * 100).toFixed(0)}% estimate coverage`,
		details: {
			runKind: record.runKind,
			timeToFirstProgressMs: record.timeToFirstProgressMs ?? null,
			worstUpdateLatencyMs: record.worstUpdateLatencyMs ?? null,
			estimateCoverage: record.estimateCoverage,
			samples: record.samples,
			staleSamples: record.staleSamples
		}
	});
});
void bridges.desktop.getInstanceDiagnostics().then((identity) => {
	document.title = identity.displayName;
	document.documentElement.dataset.desktopInstance = identity.name ?? "canonical";
}).catch(() => undefined);

createRoot(document.getElementById("root")!).render(
	<StrictMode>
		<SynthConnectionProvider>
			<App />
		</SynthConnectionProvider>
	</StrictMode>
);
