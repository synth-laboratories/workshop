import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src");
const host = readFileSync(join(root, "components/VisualHost.tsx"), "utf8");
const css = readFileSync(join(root, "styles/app.css"), "utf8");

test("product-owned primary optimizer visuals use terminal evidence instead of E1", () => {
	assert.match(host, /function productOwnedPrimaryOptimizerRunId/);
	assert.match(host, /optimizerVisualRole === "string"/);
	assert.match(host, /role === "primary"/);
	assert.match(host, /semantics === "baseline_eval_trace"/);
	assert.match(host, /primaryOptimizerRunId \? optimizerSealGate\.ready : authoringGateReady/);
	assert.match(host, /data-run-evidence-state/);
	assert.match(host, /data-visual-terminal/);
	assert.match(host, /state !== "accepted"/);
	assert.match(host, /Seal requires the E1 visual quality gate for this exact revision/);
});

test("reopened optimizer visuals show journal hydration instead of zero-like data", () => {
	assert.match(host, /optimizerRunId && !optimizerPayload/);
	assert.match(host, /data-testid="visual-optimizer-hydrating"/);
	assert.match(host, /Restoring run evidence/);
	assert.match(host, /Metrics and rollouts will appear together after the journal is hydrated/);
	assert.doesNotMatch(host, /Seal unavailable — durable run evidence/);
	assert.doesNotMatch(host, /Restoring durable run evidence/);
	assert.match(css, /\.visual-optimizer-hydrating\s*\{/);
	assert.match(css, /\.visual-optimizer-skeleton\s*\{/);
});
