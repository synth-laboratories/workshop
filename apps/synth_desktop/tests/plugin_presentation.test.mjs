// Behavioral cover for plugin lifecycle presentation. Calls the real function
// so a phase-map edit cannot pass silently.

import test from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { transformSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

const source = join(appRoot, "src/renderer/src/runtime/pluginPresentation.ts");
const compiled = join(compiledDir, "pluginPresentation.mjs");
writeFileSync(compiled, transformSync(readFileSync(source, "utf8"), {
	loader: "ts",
	format: "esm",
	target: "es2022"
}).code);

const { pluginPresentation, findPluginStatus } = await import(pathToFileURL(compiled).href);

const status = (overrides = {}) => ({
	schemaVersion: "synth.plugin-status.v1",
	pluginId: "optimizers",
	enabled: true,
	phase: "ready",
	releaseChannel: "official",
	catalogVersion: "0.2.5",
	service: { phase: "ready", activeRuns: 0 },
	algorithms: [],
	templates: [],
	...overrides
});

test("a ready plugin is usable and reads Ready", () => {
	const view = pluginPresentation(status());
	assert.equal(view.label, "Ready");
	assert.equal(view.isUsable, true);
	assert.equal(view.isTransitional, false);
});

test("an absent bridge yields no status text and never blocks navigation", () => {
	const view = pluginPresentation(null);
	assert.equal(view.label, null);
	assert.equal(view.a11yLabel, null);
	// Unknown must not present as broken: the row still opens its page.
	assert.equal(view.isUsable, true);
});

test("a disabled plugin with live runs says so instead of implying they stopped", () => {
	// `disable` only clears the registry flag; the sidecar keeps running and
	// has no active-run guard, so this state is reachable in practice.
	const view = pluginPresentation(status({
		enabled: false,
		phase: "disabled",
		service: { phase: "ready", activeRuns: 3 }
	}));
	assert.equal(view.label, "Disabled · 3 running");
	assert.equal(view.activeRuns, 3);
	assert.equal(view.tone, "warning");
	assert.equal(view.isUsable, false);
	assert.match(view.a11yLabel, /3 runs still active/);
});

test("a plain disabled plugin is neutral, not an error", () => {
	const view = pluginPresentation(status({ enabled: false, phase: "disabled" }));
	assert.equal(view.label, "Disabled");
	assert.equal(view.tone, "neutral");
	assert.equal(view.isUsable, false);
});

test("unhealthy phases are distinguishable and never colour-only", () => {
	for (const phase of ["degraded", "stopped"]) {
		const view = pluginPresentation(status({ phase }));
		assert.equal(view.label, "Needs attention");
		assert.equal(view.tone, "warning");
		assert.ok(view.a11yLabel.includes(phase));
	}
	const error = pluginPresentation(status({ phase: "error", detail: "boom" }));
	assert.equal(error.label, "Error");
	assert.equal(error.tone, "danger");
	assert.equal(error.a11yLabel, "Error");
	assert.equal(error.detail, "boom");
});

test("not installed is stated, not hidden", () => {
	const view = pluginPresentation(status({ phase: "not_installed" }));
	assert.equal(view.label, "Not installed");
	assert.equal(view.isUsable, false);
});

test("transitional phases report progress and refuse work", () => {
	for (const phase of ["downloading", "verifying", "starting", "stopping", "updating", "removing"]) {
		const view = pluginPresentation(status({ phase }));
		assert.equal(view.isTransitional, true, phase);
		assert.equal(view.isUsable, false, phase);
		assert.match(view.a11yLabel, /in progress/, phase);
	}
});

test("needs_permissions names the missing grant instead of just saying no", () => {
	const view = pluginPresentation(status({
		phase: "needs_permissions",
		permissions: [
			{ id: "accessibility", label: "Accessibility", state: "denied" },
			{ id: "screen_recording", label: "Screen Recording", state: "granted" },
			{ id: "apple_events", label: "Apple Events", state: "not_applicable" }
		]
	}));
	assert.equal(view.label, "Needs permission");
	// Warning, not danger: nothing is broken and the fix is in System Settings.
	assert.equal(view.tone, "warning");
	assert.equal(view.isUsable, false);
	assert.equal(view.isTransitional, false);
	assert.equal(view.a11yLabel, "Needs permission: Accessibility");
});

test("needs_permissions is still legible when the grant list has not loaded", () => {
	const view = pluginPresentation(status({ phase: "needs_permissions" }));
	assert.equal(view.label, "Needs permission");
	assert.equal(view.a11yLabel, "Needs permission");
	assert.equal(view.isUsable, false);
});

test("an unrecognised future phase degrades to its own name, not to Ready", () => {
	const view = pluginPresentation(status({ phase: "quiescing" }));
	assert.equal(view.label, "quiescing");
	assert.equal(view.isUsable, false);
});

test("status lookup matches on plugin id and tolerates an empty registry", () => {
	const optimizers = status();
	assert.equal(findPluginStatus([optimizers], "optimizers"), optimizers);
	assert.equal(findPluginStatus([optimizers], "visuals"), null);
	assert.equal(findPluginStatus([optimizers], "laguna"), null);
	assert.equal(findPluginStatus(null, "optimizers"), null);
	assert.equal(findPluginStatus([], "optimizers"), null);
});
