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

test("systems authoring requires screenshot-backed collision and density review", () => {
	const ipc = readFileSync(new URL("../src-tauri/src/visuals_ipc.rs", import.meta.url), "utf8");
	const authorSkill = readFileSync(new URL("../skills/author-synth-diagrams/SKILL.md", import.meta.url), "utf8");
	assert.match(ipc, /noTextCollisions/);
	assert.match(ipc, /focalDensity/);
	assert.match(ipc, /screenshotInspected/);
	assert.match(ipc, /visual review requires screenshot_path from capture_review/);
	assert.match(ipc, /unresolved automated findings/);
	assert.match(authorSkill, /capture_review.*wide and compact viewport sizes/);
	assert.match(authorSkill, /5–7 focal elements per beat/);
});

test("visual MCP exposes image-backed review capture", () => {
	const mcp = readFileSync(new URL("../src-tauri/src/bin/synth_visuals_mcp.rs", import.meta.url), "utf8");
	const stdio = readFileSync(new URL("../src-tauri/src/ipc/mcp_stdio.rs", import.meta.url), "utf8");
	assert.match(mcp, /capture_review/);
	assert.match(mcp, /visual_capture_review/);
	assert.match(mcp, /screenshot_path/);
	assert.match(mcp, /_mcpImage/);
	assert.match(mcp, /deterministic-svg/);
	assert.match(mcp, /desktop-window/);
	assert.match(mcp, /screencapture/);
	assert.match(stdio, /"type": "image"/);
	assert.match(stdio, /object\.remove\("_mcpImage"\)/);
});

test("screenshot-backed readiness applies to every visual family", () => {
	const ipc = readFileSync(new URL("../src-tauri/src/visuals_ipc.rs", import.meta.url), "utf8");
	const useSkill = readFileSync(new URL("../skills/use-synth-visuals/SKILL.md", import.meta.url), "utf8");
	assert.match(ipc, /checks\.push\("screenshotInspected"\)/);
	assert.match(ipc, /visual review requires screenshot_path from capture_review/);
	for (const family of ["evals", "optimizers", "UML/Mermaid", "static 2D", "Benjamin Dicken Style"]) assert.ok(useSkill.includes(family), family);
});
