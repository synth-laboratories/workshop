import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src");
const read = (path) => readFileSync(join(root, path), "utf8");

test("shared pane separator exposes bounded keyboard and pointer behavior", () => {
	const handle = read("components/PaneResizeHandle.tsx");
	for (const contract of ["ResizeObserver", "aria-valuemin", "aria-valuemax", "aria-valuenow", "ArrowLeft", "ArrowRight", "event.shiftKey", "onPointerCancel", "onLostPointerCapture", "window.addEventListener(\"blur\"", "releasePointerCapture", "settleAfterLayout", "onDoubleClick"]) {
		assert.ok(handle.includes(contract), contract);
	}
	assert.match(handle, /parent\.getBoundingClientRect\(\)/);
	assert.match(handle, /PANE_KEYBOARD_STEP_PX = 40/);
	assert.match(handle, /key === "Home"/);
	assert.match(handle, /key === "End"/);
	assert.match(handle, /namedPaneElement/);
	assert.doesNotMatch(
		handle,
		/direction === "sidebar" \|\| direction === "primary"[\s\S]*ArrowLeft \? delta/
	);
});

test("Visuals library has an independently persisted list-to-preview separator", () => {
	const page = read("components/VisualsPage.tsx");
	const schema = read("preferences/schema.ts");
	assert.match(page, /direction="primary"/);
	assert.match(page, /visualsListWidth/);
	assert.match(schema, /synth\.visuals\.list-width/);
	assert.match(schema, /hasCanonicalVisualsWidth/);
});

test("both splitters stack against their actual content containers", () => {
	const css = read("styles/app.css");
	assert.match(css, /container-name: main-workbench/);
	assert.match(css, /container-name: visuals-library/);
	assert.match(css, /@container main-workbench \(max-width: 900px\)/);
	// The split survives to the width where the preview loses its 420px minimum,
	// not to the width where the list loses a comfortable 320px.
	assert.match(css, /@container visuals-library \(max-width: 700px\)/);
	assert.match(css, /\.workbench\.with-visual > \.pane-resize-handle[\s\S]*display: none/);
	assert.match(css, /\.visuals-layout > \.primary-resize-handle \{ display: none; \}/);
});
