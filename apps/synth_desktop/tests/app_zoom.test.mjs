import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const testsDir = dirname(fileURLToPath(import.meta.url));
const appRoot = join(testsDir, "..");
const rendererRoot = join(appRoot, "src/renderer/src");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const compiled = join(compiledDir, "appZoom.mjs");
buildSync({
	entryPoints: [join(rendererRoot, "runtime/appZoom.ts")],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "neutral",
	outfile: compiled
});
const { DEFAULT_ZOOM_PERCENT, stepZoomPercent, zoomShortcutAction } = await import(
	pathToFileURL(compiled).href
);

test("Command/Ctrl plus minus and 0 step page zoom", () => {
	assert.equal(stepZoomPercent(100, 1), 110);
	assert.equal(stepZoomPercent(100, -1), 90);
	assert.equal(stepZoomPercent(200, 1), 200);
	assert.equal(stepZoomPercent(75, -1), 75);
	assert.equal(stepZoomPercent(DEFAULT_ZOOM_PERCENT, 1), 110);
	assert.equal(
		zoomShortcutAction({ key: "=", metaKey: true, ctrlKey: false, altKey: false, code: "Equal" }),
		"in"
	);
	assert.equal(
		zoomShortcutAction({ key: "-", metaKey: true, ctrlKey: false, altKey: false, code: "Minus" }),
		"out"
	);
	assert.equal(
		zoomShortcutAction({ key: "0", metaKey: true, ctrlKey: false, altKey: false, code: "Digit0" }),
		"reset"
	);
	assert.equal(
		zoomShortcutAction({ key: "=", metaKey: false, ctrlKey: false, altKey: false, code: "Equal" }),
		null
	);
});

test("zoom HUD is mounted on the overlay shell", () => {
	const overlays = readFileSync(join(rendererRoot, "components/AppOverlays.tsx"), "utf8");
	const hud = readFileSync(join(rendererRoot, "components/ZoomHud.tsx"), "utf8");
	const css = readFileSync(join(rendererRoot, "styles/app.css"), "utf8");
	assert.match(overlays, /<ZoomHud \/>/);
	assert.match(hud, /data-testid="zoom-indicator"/);
	assert.match(hud, /role="status"/);
	assert.match(css, /\.zoom-hud\s*\{/);
});
