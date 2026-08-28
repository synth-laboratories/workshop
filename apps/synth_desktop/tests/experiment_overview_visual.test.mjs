import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const source = join(appRoot, "../../visuals/families/experiments/experiment.overview.v1/shell.tsx");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const compiled = join(compiledDir, "ExperimentOverviewVisual.mjs");

buildSync({
	entryPoints: [source],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "neutral",
	jsx: "automatic",
	outfile: compiled,
	loader: { ".css": "empty" },
	external: ["react", "react/jsx-runtime", "react-dom", "react-dom/server"]
});

const { Shell } = await import(pathToFileURL(compiled).href);

test("experiment overview renders question, progress, variants, evidence, and lineage", () => {
	const html = renderToStaticMarkup(createElement(Shell, {
		experiment: {
			schemaVersion: "synth.experiment.overview.v1",
			experimentId: "exp_banking77_gepa_01",
			title: "Banking77 prompt optimization",
			question: "Can GEPA improve heldout classification accuracy?",
			status: "running",
			progress: { phase: "Candidate evaluation", completed: 38, total: 50, elapsed: "7m 42s", eta: "2m 20s", usage: "140 rollouts", cost: "$1.84" },
			metrics: [{ label: "Baseline", value: "0.72" }, { label: "Best train", value: "0.81" }, { label: "Heldout", value: null }, { label: "Lift", value: null }],
			arms: [
				{ id: "seed", label: "Seed prompt", baseline: true, score: 0.72, status: "completed", metrics: { accuracy: 0.72, cost: 0.61 } },
				{ id: "candidate-3", label: "Candidate 3", selected: true, score: 0.81, status: "evaluating", metrics: { accuracy: 0.81, cost: null } }
			],
			comparison: {
				primaryMetric: "accuracy",
				columns: [
					{ id: "accuracy", label: "Accuracy", format: "percent", direction: "higher" },
					{ id: "cost", label: "Cost", format: "currency", direction: "lower" }
				]
			},
			evidence: [{ id: "eval-distribution", title: "Train score distribution", kind: "distribution", status: "ready", visualId: "visual_eval_1" }],
			lineage: [{ id: "dataset", label: "Dataset", kind: "source" }, { id: "search", label: "GEPA search", kind: "optimizer" }, { id: "selection", label: "Selection", kind: "result" }],
			limitations: ["Heldout evaluation has not completed."]
		}
	}));

	assert.match(html, /visual-experiment-overview/);
	assert.match(html, /Can GEPA improve heldout classification accuracy/);
	assert.match(html, /38\/50/);
	assert.match(html, /Seed prompt · baseline/);
	assert.match(html, /Candidate 3 · selected/);
	assert.match(html, /Run comparison/);
	assert.match(html, /81.0%/);
	assert.match(html, /\$0.61/);
	assert.match(html, /Missing measurements are shown as/);
	assert.match(html, /Train score distribution/);
	assert.match(html, /Dataset/);
	assert.match(html, /Heldout evaluation has not completed/);
});

test("experiment overview keeps missing measurements distinct from zero", () => {
	const html = renderToStaticMarkup(createElement(Shell, {
		experiment: {
			title: "Incomplete experiment",
			status: "planned",
			metrics: [
				{ label: "Missing", value: null },
				{ label: "Observed zero", value: 0 }
			]
		}
	}));

	assert.match(html, /Missing<\/span><strong[^>]*>—<\/strong>/);
	assert.match(html, /Observed zero<\/span><strong[^>]*>0<\/strong>/);
	assert.doesNotMatch(html, /No variants have been recorded[\s\S]*0 variants/);
});

test("experiment overview leads with a compact summary and conclusion, then opens results", () => {
	const html = renderToStaticMarkup(createElement(Shell, {
		experiment: {
			title: "Craftax harness study",
			question: "Which guidance improves reward efficiency?",
			status: "complete",
			hypotheses: [
				{ id: "survival-steps", claim: "Survival saves steps", verdict: "true", confidence: "low", why: "73.5 vs 102 mean steps; n=2" },
				{ id: "generalizes", claim: "The result generalizes", verdict: "needs_more_analysis", confidence: "low", why: "Only two seeds per arm" }
			],
			assessment: { summary: "Survival is more efficient; manual has higher raw reward.", confidence: "low", nextStep: "Run ten seeds." },
			evidence: [{ id: "baseline", title: "Baseline receipt", status: "verified" }],
			progress: { phase: "all variants complete", completed: 6, total: 6 }
		}
	}));

	assert.match(html, /Experiment summary/);
	assert.match(html, /Conclusion/);
	assert.match(html, /Survival saves steps/);
	assert.match(html, /True/);
	assert.match(html, /Needs more analysis/);
	assert.match(html, /low/);
	assert.match(html, /73.5 vs 102 mean steps/);
	assert.doesNotMatch(html, /Question:/);
	assert.match(html, /<details[^>]*class="sv-section"[^>]*open/);
	assert.match(html, /Comparison &amp; results/);
	assert.match(html, /Survival is more efficient/);
});

test("experiment overview renders optional evidence modules only when supplied", () => {
	const html = renderToStaticMarkup(createElement(Shell, {
		experiment: {
			title: "Full evidence experiment",
			hypotheses: [{ id: "h1", claim: "Guidance improves reward", verdict: "false", confidence: "medium", why: "1.3 vs 3.2" }],
			results: { rollouts: [{ id: "r1", label: "Seed 46", reward: 1.3, steps: 32, achievements: 2, stopReason: "llm_turn_cap", traceId: "trace_46" }] },
			traces: { prominence: "summary", items: [{ id: "t1", label: "Seed 46 trace", traceId: "trace_46", reward: 1.3, steps: 32, summary: "One code error." }] },
			task: { name: "craftax-singleplayer", split: "eval" },
			runtime: { model: "openai/gpt-5.6-luna", containerId: "ctr_123" },
			artifacts: [{ id: "results", label: "Results", path: "/tmp/results.json" }],
			provenance: { repository: "gamebench-craftax", commit: "abc123", dirty: false }
		}
	}));
	assert.match(html, /Rollout results/);
	assert.match(html, /trace_46/);
	assert.match(html, /<button[^>]*data-reference-kind="trace"[^>]*data-reference-value="trace_46"/);
	assert.match(html, /data-reference-container-id="ctr_123"/);
	assert.match(html, /<details[^>]*open=""[^>]*>[\s\S]*Traces/);
	assert.match(html, /Run context/);
	assert.match(html, /openai\/gpt-5.6-luna/);
	assert.match(html, /results.json/);
	assert.match(html, /gamebench-craftax/);

	const minimal = renderToStaticMarkup(createElement(Shell, { experiment: { title: "Minimal", hypotheses: [{ id: "h", claim: "A", verdict: "unresolved" }] } }));
	assert.doesNotMatch(minimal, /Results &amp; assessment|Traces|Run context|Artifacts|Method &amp; caveats/);
});

test("experiment overview does not offer a dead inspector action for lite seals", () => {
	const html = renderToStaticMarkup(createElement(Shell, { experiment: {
		title: "Lite trace",
		results: { rollouts: [{ id: "r1", traceId: "rollout_1" }] },
		traces: { items: [{ id: "t1", traceId: "rollout_1", summary: "75 events; lite seal" }] }
	} }));
	assert.match(html, /Unavailable/);
	assert.doesNotMatch(html, /data-reference-value="rollout_1"/);
});

test("RuneBench clip renders CAS frames, synchronized actions, health, terminal controls, and exports", () => {
	const digest = "a".repeat(64);
	const events = [
		{ sequenceNumber: 1, type: "eval.trial.event", delta: { trial_id: "trial-a", containerEvent: {
			kind: "frame", rollout_id: "rollout-a", payload: {
				frame_index: 0, elapsed_ms: 1000, sha256: `sha256:${digest}`,
				media: { casDigest: digest, mediaType: "image/png", width: 400, height: 300 },
				stream_health: { frames_captured: 1, frames_dropped: 0, bytes_captured: 2048, average_capture_latency_ms: 42, source_interval_ms: 1000 }
			}
		} } },
		{ sequenceNumber: 2, type: "eval.trial.event", delta: { trial_id: "trial-a", containerEvent: {
			kind: "agent.action", rollout_id: "rollout-a", payload: { elapsed_ms: 900, frame_index: 0, tool: "execute_code", status: "completed", arguments_preview: "await bot.chopTree()" }
		} } },
		{ sequenceNumber: 3, type: "eval.trial.event", delta: { trial_id: "trial-a", containerEvent: {
			kind: "trial.completed", rollout_id: "rollout-a", payload: { clip: { mp4: "http://127.0.0.1:8104/rollouts/rollout-a/clip.mp4" } }
		} } }
	];
	const html = renderToStaticMarkup(createElement(Shell, {
		experiment: { title: "RuneBench", status: "completed" },
		run: { status: "completed" }, events
	}));
	assert.match(html, /Game client clip/);
	assert.match(html, /Loading retained frame/);
	assert.match(html, /Jump to latest/);
	assert.match(html, /Playback speed/);
	assert.match(html, /Previous frame/);
	assert.match(html, /Synchronized actions/);
	assert.match(html, /execute_code/);
	assert.match(html, /42 ms/);
	assert.match(html, /2\.0 KiB\/s/);
	assert.match(html, /Download MP4/);
	assert.match(html, /Export WebM/);
	assert.match(html, /Keyboard:/);
});

test("RuneBench running clip exposes the fragmented encoded video mode", () => {
	const digest = "b".repeat(64);
	const html = renderToStaticMarkup(createElement(Shell, {
		experiment: { title: "RuneBench live", status: "running" },
		run: { status: "running" },
		events: [{ sequenceNumber: 1, type: "eval.trial.event", delta: { trial_id: "trial-a", containerEvent: {
			kind: "frame", rollout_id: "rollout-a", payload: {
				frame_index: 0, elapsed_ms: 1000, live_video_url: "http://127.0.0.1:8104/rollouts/rollout-a/live.mp4",
				media: { casDigest: digest, mediaType: "image/png", width: 400, height: 300 }
			}
		} } }]
	}));
	assert.match(html, /Encoded live video/);
	assert.match(html, /Frame timeline/);
});
