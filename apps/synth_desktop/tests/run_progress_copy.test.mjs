/**
 * The copy rules for run progress.
 *
 * These read like typography tests, but each one is a truth rule: an
 * indeterminate bar that says "0%" is a lie, a $0.00 for unreported cost is a
 * lie, and a precise "~3 min" built on two samples is a lie. The strings are
 * where those lies would actually reach a person, so they are asserted here.
 */
import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

function bundle(relative, outName) {
	const outfile = join(compiledDir, outName);
	buildSync({
		entryPoints: [join(appRoot, relative)],
		bundle: true,
		format: "esm",
		target: "es2022",
		platform: "node",
		// The renderer resolves family internals through the same alias Vite and
		// tsconfig use; esbuild needs it spelled out.
		alias: { "@synth/visual-templates": join(appRoot, "../../visuals/families") },
		outfile
	});
	return pathToFileURL(outfile).href;
}

const {
	formatDurationMs,
	formatWork,
	formatWorkBreakdown,
	progressAriaText,
	statusBadgeClass,
	statusLabel
} = await import(bundle("src/renderer/src/runtime/runProgress/format.ts", "runProgressCopyFormat.mjs"));
const {
	costSummary,
	coverageLabel,
	coveredMetric,
	formatUsd,
	metricExplanation,
	metricSummary,
	unavailableMetric
} = await import(bundle("src/renderer/src/runtime/runProgress/usage.ts", "runProgressCopyUsage.mjs"));

function projection(overrides = {}) {
	return {
		schemaVersion: "run_progress.v1",
		runId: "run-a",
		runKind: "gepa",
		title: "GEPA · Banking77",
		status: "running",
		terminal: false,
		phase: { id: "minibatch", label: "Evaluating candidates", status: "active" },
		phases: [],
		work: {},
		timing: {},
		usage: {},
		capabilities: { pause: false, resume: false, cancel: false },
		milestones: [],
		warnings: [],
		details: [],
		cursorSeq: 1,
		stale: false,
		...overrides
	};
}

test("an indeterminate bar never claims a percentage", () => {
	const indeterminate = projection({
		progress: { semantics: "training steps", determinate: false }
	});
	assert.match(progressAriaText(indeterminate), /Evaluating candidates · progress not measurable/);
	assert.doesNotMatch(progressAriaText(indeterminate), /%/);
});

test("a determinate bar announces the share and what it measures", () => {
	const determinate = projection({
		progress: { fraction: 0.68, semantics: "rollout budget spent", determinate: true }
	});
	assert.equal(progressAriaText(determinate), "68% of rollout budget spent");
});

test("work with no denominator shows the count without inventing a total", () => {
	assert.equal(formatWork(projection({ work: { completed: 340, unit: "steps" } })), "340 steps");
	assert.equal(formatWork(projection({ work: { completed: 68, total: 100, unit: "rollouts" } })), "68 / 100 rollouts");
	assert.equal(formatWork(projection({ work: { unit: "rollouts" } })), null);
});

test("the breakdown omits what the producer never reported, and hides real zeros", () => {
	assert.equal(
		formatWorkBreakdown(projection({ work: { active: 4, queued: 7, failed: 0, retried: 0 } })),
		"4 active · 7 queued"
	);
	assert.equal(
		formatWorkBreakdown(projection({ work: { active: 4, failed: 2, retried: 1 } })),
		"4 active · 2 failed · 1 retried"
	);
	assert.equal(formatWorkBreakdown(projection({ work: {} })), null);
});

test("durations read in the unit a person would use", () => {
	assert.equal(formatDurationMs(undefined), "—");
	assert.equal(formatDurationMs(-1), "—");
	assert.equal(formatDurationMs(42_000), "42s");
	assert.equal(formatDurationMs(125_000), "2m 5s");
	assert.equal(formatDurationMs(120_000), "2m");
	assert.equal(formatDurationMs(3_600_000), "1h");
	assert.equal(formatDurationMs(5_400_000), "1h 30m");
});

test("status words and badge tones are one mapping, not per-surface guesses", () => {
	assert.equal(statusLabel("running"), "Running");
	assert.equal(statusLabel("cancelled"), "Cancelled");
	assert.match(statusBadgeClass("failed"), /ws-badge-danger/);
	assert.match(statusBadgeClass("completed"), /ws-badge-success/);
	assert.match(statusBadgeClass("paused"), /ws-badge-warn/);
	assert.match(statusBadgeClass("running"), /ws-badge-running/);
});

test("unreported cost is unavailable and names the gap; $0.00 is never printed for it", () => {
	const missing = unavailableMetric();
	assert.equal(costSummary(missing), "Cost unavailable · producer emitted no cost telemetry");
	assert.doesNotMatch(costSummary(missing), /\$/);
});

test("a reported zero is a value and reads as one", () => {
	const free = coveredMetric(0, "provider", 12, 12);
	assert.equal(metricSummary(free, formatUsd, "rollout"), "$0.00 reported · 100% rollout coverage");
});

test("a partially covered total says how much of the run has reported", () => {
	const partial = coveredMetric(0.42, "container", 74, 100);
	assert.equal(coverageLabel(partial), "74%");
	assert.equal(metricSummary(partial, formatUsd, "rollout"), "$0.42 reported · 74% rollout coverage");
	assert.equal(metricExplanation(partial, "rollout"), "74 of 100 rollouts reported it · container reported");
});

test("with no denominator a figure still says who vouched for it", () => {
	const undeclared = coveredMetric(12_430, "provider", 3);
	assert.equal(coverageLabel(undeclared), null);
	assert.equal(metricSummary(undeclared, (value) => `${value.toLocaleString("en-US")} tokens`), "12,430 tokens · provider reported");
	assert.match(metricExplanation(undeclared, "step"), /no denominator declared/);
});

test("small dollar amounts keep their significant digits", () => {
	assert.equal(formatUsd(0.0004), "$0.0004");
	assert.equal(formatUsd(3.28), "$3.28");
	assert.equal(formatUsd(0), "$0.00");
});
