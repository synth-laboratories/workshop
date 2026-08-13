import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src");
const read = (path) => readFileSync(join(root, path), "utf8");

test("VisualHost routes both systems visual kinds before template shells", () => {
	const host = read("components/VisualHost.tsx");
	assert.match(host, /diagram\.systems\.v1/);
	assert.match(host, /rendererKind === "systems"/);
	assert.match(host, /diagram\.systems\.dynamic\.v1/);
	assert.match(host, /rendererKind === "systems-dynamic"/);
	assert.ok(host.indexOf("isSystemsDynamic") < host.indexOf("isMermaid"));
});

test("static systems maps retain Mermaid-class source and rendition controls", () => {
	const surface = read("components/SystemsMapVisual.tsx");
	for (const label of ["Zoom in", "Zoom out", "Fit", "Source", "Copy source", "Export SVG", "Retry"]) assert.ok(surface.includes(label), label);
	assert.match(surface, /rendition\?\.\(visualId, "svg"/);
	assert.match(surface, /SYSTEMS MAP · 2D/);
});

test("dynamic systems explainers are declarative and expose deterministic playback controls", () => {
	const surface = read("components/SystemsDynamicVisual.tsx");
	for (const label of ["Play", "Pause", "Replay", "Previous beat", "Next beat", "Reduced motion", "Explainer timeline", "Copy source", "Export still", "Retry"]) assert.ok(surface.includes(label), label);
	assert.match(surface, /posterTimeMs/);
	assert.match(surface, /prefers-reduced-motion/);
	assert.match(surface, /scene\.reducedMotion === "final"/);
	assert.match(surface, /easedProgress/);
	assert.match(surface, /progress \* progress \* \(3 - 2 \* progress\)/);
	assert.match(surface, /event\.easing !== "step-end" \|\| raw >= 1/);
	assert.match(surface, /durationMs\) \? Math\.max/);
	assert.match(surface, /scene\.notes\?\.map/);
	assert.match(surface, /stateAt\(scene, edge\.id \?\? `\$\{edge\.from\}-\$\{edge\.to\}`, timeMs, edge\)/);
	assert.match(surface, /state\.directed === false/);
	assert.match(surface, /edgeGeometry/);
	assert.match(surface, /styleClass\(state\.style\)/);
	assert.doesNotMatch(surface, /dangerouslySetInnerHTML|eval\(|new Function|<iframe/);
});
