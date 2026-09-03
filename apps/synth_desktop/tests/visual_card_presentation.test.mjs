/**
 * Visual library card presentation, from the 2026-09-02 Banking77 review:
 * suite titles truncated at the differentiating word, and preview visuals whose
 * "session — · run — · trace —" line read as missing data rather than as
 * deliberately bundled evidence.
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

const source = join(appRoot, "src/renderer/src/runtime/templatePresentation.ts");
const compiled = join(compiledDir, "templatePresentation.mjs");
writeFileSync(
	compiled,
	transformSync(readFileSync(source, "utf8"), { loader: "ts", format: "esm", target: "es2022" }).code
);
const { visualCardIdentity, visualEvidenceMode, runProgressEvidenceMode } = await import(pathToFileURL(compiled).href);

const read = (relative) => readFileSync(join(appRoot, "src/renderer/src", relative), "utf8");

test("suite titles lead with the distinct name and carry the family as a badge", () => {
	assert.deepEqual(visualCardIdentity("Banking77 · CISPO training"), {
		name: "CISPO training",
		badge: "Banking77"
	});
	assert.deepEqual(visualCardIdentity("Banking77 · GEPA search"), {
		name: "GEPA search",
		badge: "Banking77"
	});
	// Only the leading qualifier moves; deeper separators stay in the name.
	assert.deepEqual(visualCardIdentity("Banking77 · SFT training · run 3"), {
		name: "SFT training · run 3",
		badge: "Banking77"
	});
	// A title with no family qualifier is left exactly as the author wrote it.
	assert.deepEqual(visualCardIdentity("Laguna Prompt Trim Preinstall"), {
		name: "Laguna Prompt Trim Preinstall"
	});
});

test("evidence mode separates a bound run from a template's bundled examples", () => {
	assert.equal(visualEvidenceMode({ sessionId: "sess_1" }), "bound");
	assert.equal(visualEvidenceMode({ runId: "opt_1" }), "bound");
	assert.equal(visualEvidenceMode({ traceId: "trace_1" }), "bound");
	// A run-scoped trace set of unknown size is still a binding, not an absence.
	assert.equal(visualEvidenceMode({ traceSetCount: null }), "bound");
	assert.equal(visualEvidenceMode({ metadata: { evidenceMode: "bundled_fixture" } }), "bundled");
	assert.equal(visualEvidenceMode({ bindings: { inputs: [{ input: "stream", kind: "fixture", source: "examples/events.json" }] } }), "bundled");
	assert.equal(visualEvidenceMode({}), "unbound");
	assert.equal(visualEvidenceMode({ sessionId: "", runId: null, traceId: undefined }), "unbound");
});

test("evidence workbenches hydrate the durable journal on cold reopen", () => {
	assert.equal(runProgressEvidenceMode("live.annotated_rollouts.v1"), "full");
	assert.equal(runProgressEvidenceMode("trace.workbench.v1"), "full");
	assert.equal(runProgressEvidenceMode("craftax.trace_workbench.v1"), "full");
	assert.equal(runProgressEvidenceMode("optimizer.run.v1"), "auto");
	assert.equal(runProgressEvidenceMode(undefined), "auto");
});

test("the library card shows a bundled-preview badge instead of three dashes", () => {
	const page = read("components/VisualsPage.tsx");
	assert.match(page, /evidenceMode === "bundled" \?/);
	assert.match(page, /Bundled preview/);
	assert.match(page, /identity\.badge/);
	assert.match(page, /visualCardIdentity\(visual\.displayName\?\.trim\(\) \|\| visual\.title\)/);
	assert.match(page, /<strong>\{identity\.name\}<\/strong>/);
	assert.match(page, /evidenceMode === "unbound" \?/);
	assert.match(page, /Not bound/);
	// Bound cards keep one short source line; the full triple stays in the
	// preview's details-and-provenance disclosure.
	assert.match(page, /testId=\{`visual-ops-\$\{visual\.id\}`\}\s*\n\s*compact\s*\n\s*oneLine/);
	assert.match(page, /testId=\{`visual-ops-preview-\$\{selected\.id\}`\}\s*\n\s*compact\s*\n\s*\/>/);
});

test("one-line provenance shows the most specific binding and cannot wrap", () => {
	const ops = read("components/VisualOpsLine.tsx");
	const oneLine = ops.slice(ops.indexOf("if (oneLine) {"), ops.indexOf("return (\n\t\t<span className={className} data-testid={testId}>\n\t\t\t<OpsPart kind=\"session\""));
	assert.match(oneLine, /traceId\?\.trim\(\)[\s\S]*runId\?\.trim\(\)[\s\S]*kind="session"/);
	const css = read("styles/app.css");
	assert.match(css, /\.visual-ops-one-line \{[\s\S]*?text-overflow: ellipsis/);
});

test("the list-and-preview split survives to the preview's real minimum width", () => {
	const css = read("styles/app.css");
	assert.match(css, /minmax\(240px, min\(var\(--visuals-list-width, 320px\), 420px, calc\(100% - 451px\)\)\) 7px minmax\(420px, 1fr\)/);
	assert.match(css, /@media \(max-width: 700px\)/);
	assert.match(css, /@container visuals-library \(max-width: 700px\)/);
	const page = read("components/VisualsPage.tsx");
	assert.match(page, /minPrimary=\{240\} maxPrimary=\{420\} minSecondary=\{420\}/);
});
