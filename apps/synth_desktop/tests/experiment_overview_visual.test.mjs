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
				{ id: "seed", label: "Seed prompt", baseline: true, score: 0.72, status: "completed" },
				{ id: "candidate-3", label: "Candidate 3", selected: true, score: 0.81, status: "evaluating" }
			],
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
