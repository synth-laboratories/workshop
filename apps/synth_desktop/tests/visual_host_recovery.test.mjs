import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const host = readFileSync(join(root, "src/renderer/src/components/VisualHost.tsx"), "utf8");
const publicError = readFileSync(join(root, "src/renderer/src/runtime/publicError.ts"), "utf8");
const groupedFixture = readFileSync(join(root, "tests/bombadil/run.mjs"), "utf8");
const minWidth = readFileSync(join(root, "tests/bombadil/minimum-width-replay.spec.ts"), "utf8");
const chrome = readFileSync(join(root, "src/renderer/src/components/VisualPaneChrome.tsx"), "utf8");

test("an injected renderer crash remounts the same identity and revision", () => {
	assert.match(host, /consumeInjectedRendererCrash\(/);
	assert.match(host, /injected renderer crash/);
	assert.match(host, /key=\{this\.state\.retry\}/);
	assert.match(host, /onRetry=\{\(\) => this\.setState/);
	assert.match(host, /visual-last-known-good/);
	assert.match(host, /binding\.kind === "fixture"/);
});

test("a structured Tauri rejection is presented with code and message, never [object Object]", () => {
	assert.match(publicError, /fromCompatibilityEnvelope/);
	assert.equal(publicError.includes("String(reason)"), false);
	assert.match(host, /publicError\(reason/);
	assert.match(host, /toPublicError\(reason/);
	assert.equal(host.includes("String(reason)"), false);
});

test("grouped Craftax and compact-width specs seed a real fixture, not an empty pane", () => {
	assert.match(groupedFixture, /kind: "fixture", source: "examples\/events\.json"/);
	assert.equal(groupedFixture.includes("data: { events: [] }"), false);
	assert.match(minWidth, /the_outputs_pane_actually_opened/);
	assert.match(minWidth, /layout\.current\.paneVisible/);
	assert.match(groupedFixture, /includeMinimumWidthReplay/);
	assert.match(groupedFixture, /"20s"/);
});

test("visual maintenance is first-class and keeps durable optimizer evidence", () => {
	assert.match(chrome, /Re-render with current template/);
	assert.match(chrome, /Restart evaluator/);
	assert.match(host, /bridges\.visuals\.update\(visualId/);
	assert.match(host, /templateRerender/);
	assert.match(host, /runFacets\(run\)\.containerId/);
	assert.match(host, /isTerminalRunStatus\(run\.status\)/);
	assert.match(host, /durable run evidence was retained/);
});
