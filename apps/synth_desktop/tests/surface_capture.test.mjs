/**
 * The renderer half of host surface capture.
 *
 * The host resizes, snapshots its own webview, and restores; all it needs back
 * is a routing decision and an acknowledgement. These assertions pin the two
 * properties that decide whether a capture photographs the right thing:
 * the acknowledgement carries both scope and target, and only a plugin capture
 * navigates.
 */

import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { transformSync } from "esbuild";
import test from "node:test";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

const source = join(appRoot, "src/renderer/src/runtime/captureSurface.ts");
const compiled = join(compiledDir, "captureSurface.mjs");
writeFileSync(
	compiled,
	transformSync(readFileSync(source, "utf8"), { loader: "ts", format: "esm", target: "es2022" }).code
);
const { CAPTURE_PLUGIN_IDS, captureReadyToken, isCapturePluginId } = await import(
	pathToFileURL(compiled).href
);

const read = (relative) => readFileSync(join(appRoot, "src/renderer/src", relative), "utf8");

test("the readiness token carries scope and target together", () => {
	// A token of just the target would let an acknowledgement left over from a
	// previous capture satisfy this one, and photograph the wrong surface.
	assert.equal(captureReadyToken("plugin", "visuals"), "plugin:visuals");
	assert.equal(captureReadyToken("visual", "vis_1"), "visual:vis_1");
	assert.notEqual(captureReadyToken("plugin", "visuals"), captureReadyToken("element", "visuals"));
});

test("only destinations the display contract admits can be captured", () => {
	assert.deepEqual([...CAPTURE_PLUGIN_IDS], [
		"visuals", "reports", "experiments", "optimizers", "inventory", "inference", "computer-use"
	]);
	assert.equal(isCapturePluginId("visuals"), true);
	assert.equal(isCapturePluginId("settings"), false);
	assert.equal(isCapturePluginId(undefined), false);
});

test("a plugin capture acknowledges only after the route actually changed", () => {
	const app = read("App.tsx");
	// Acknowledging on the request rather than on the arrival is what produces
	// a screenshot of the page the app was leaving.
	assert.match(app, /if \(scope === "plugin" && c\.view\.kind !== target\) return;/);
	assert.match(app, /markCaptureReady\(scope, target\)/);
});

test("app and element captures never navigate", () => {
	const app = read("App.tsx");
	assert.match(app, /if \(captureRequest\.route && isCapturePluginId\(captureRequest\.target\)\)/);
	const protocol = read("runtime/captureSurface.ts");
	assert.match(protocol, /route\?: boolean/);
});

test("visual review keeps its own protocol untouched", () => {
	// Visual review is a certified chain: capture receipts gate mark_ready.
	// The new scopes must not have renamed anything under it.
	const app = read("App.tsx");
	assert.match(app, /window\.addEventListener\("synth:visual-review-capture", openReviewSurface\)/);
	const page = read("components/VisualsPage.tsx");
	assert.match(page, /__synthVisualReviewCapture/);
	assert.match(page, /synthReviewCaptureReady/);
});
