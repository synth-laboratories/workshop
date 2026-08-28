import { createRoot } from "react-dom/client";
import "./styles/tokens.css";
import "./styles/primitives.css";
import "./styles/app.css";
import "../../../../../visuals/chrome/tokens.css";
import { Shell } from "../../../../../visuals/families/experiments/experiment.overview.v1/shell";

const experiment = {
	schemaVersion: "synth.experiment.overview.v1",
	experimentId: "craftax_codeact_harness_n30_20260817",
	title: "Craftax CodeAct harness study",
	question: "Which CodeAct structure and guidance improves achievements per rollout while preserving an inspectable recurrent policy loop?",
	status: "completed",
	progress: { phase: "Trace review complete", completed: 180, total: 180, elapsed: "4h 38m", eta: "Complete", usage: "52.0M tokens", cost: "Not recorded" },
	metrics: [
		{ label: "Baseline reward", value: "15.66" },
		{ label: "Best reward", value: "16.38", tone: "positive" as const },
		{ label: "Lift", value: "+0.72", tone: "positive" as const, detail: "Survival guidance vs baseline" },
		{ label: "Evaluated", value: "180 rollouts" }
	],
	comparison: {
		primaryMetric: "reward",
		columns: [
			{ id: "reward", label: "Mean reward", format: "number" as const, direction: "higher" as const },
			{ id: "achievements", label: "Achievements", format: "number" as const, direction: "higher" as const },
			{ id: "actions", label: "Env actions", format: "number" as const, direction: "higher" as const },
			{ id: "turns", label: "LLM turns", format: "number" as const, direction: "lower" as const },
			{ id: "latency", label: "Mean wall time", format: "duration" as const, direction: "lower" as const }
		]
	},
	arms: [
		{ id: "base", label: "Baseline CodeAct", baseline: true, status: "completed", score: 15.66, detail: "60 actions/leg · keep 4 exchanges", metrics: { reward: 15.66, achievements: 16.27, actions: 965.27, turns: 56.4, latency: 288.62 } },
		{ id: "memory", label: "Memory/carry guidance", status: "completed", score: 14.15, detail: "Explicit state carry and memory structure", metrics: { reward: 14.15, achievements: 14.47, actions: 597.77, turns: 38.8, latency: 304.77 } },
		{ id: "survival", label: "Survival-first guidance", selected: true, status: "completed", score: 16.38, detail: "Risk banner and survival priority", metrics: { reward: 16.38, achievements: 16.8, actions: 1163.67, turns: 64.93, latency: 317.78 } },
		{ id: "manual", label: "Emergency direct-action mode", status: "completed", score: 5.71, detail: "Switches from write_program to act at low health", metrics: { reward: 5.71, achievements: 5.87, actions: 186.9, turns: 76.37, latency: 239.17 } },
		{ id: "t10", label: "10-turn budget", status: "completed", score: 10.05, detail: "Short model-call budget", metrics: { reward: 10.05, achievements: 10.23, actions: 196.17, turns: 10, latency: 66.72 } },
		{ id: "t20", label: "20-turn budget", status: "completed", score: 12.8, detail: "Medium model-call budget", metrics: { reward: 12.8, achievements: 13, actions: 368.2, turns: 19.87, latency: 119.81 } }
	],
	evidence: [
		{ id: "turn-traces", title: "180 saved rollout traces", kind: "trace", status: "ready", summary: "Each rollout retains generated Python programs, intent, actions executed, rewards, achievements, stop details, and token usage." },
		{ id: "survival-review", title: "Survival guidance trace review", kind: "trace-comparison", status: "ready", summary: "The selected arm gains +0.72 mean reward and executes more environment actions, but the observed difference remains small enough to require uncertainty analysis." },
		{ id: "manual-failure", title: "Direct-action mode failure analysis", kind: "failure-review", status: "ready", summary: "777 no-program turns reveal a harness/tool-mode failure; the low reward must not be interpreted as evidence against all direct action control." }
	],
	lineage: [
		{ id: "authority", label: "Craftax Rust authority", kind: "environment" },
		{ id: "codeact", label: "write_program + act", kind: "policy interface" },
		{ id: "variants", label: "6 harness variants", kind: "experiment" },
		{ id: "traces", label: "180 trace-backed rollouts", kind: "evidence" },
		{ id: "selection", label: "Survival-first", kind: "provisional selection" }
	],
	limitations: [
		"This UI is replaying completed real CodeAct runs; a fresh paid run is blocked because OPENROUTER_API_KEY is not available to the current shell.",
		"The +0.72 reward lift was described by the original study as inside noise and is a provisional selection, not a confirmed winner.",
		"Provider cost was not persisted in these result files, so the UI shows it as missing rather than estimating it."
	]
};

createRoot(document.getElementById("root")!).render(
	<main className="synth-visual-root" style={{ maxWidth: 760, margin: "24px auto", padding: "0 16px" }}>
		<Shell experiment={experiment} />
	</main>
);
