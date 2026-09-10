import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { transformSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const renderer = join(appRoot, "src/renderer/src");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const source = join(renderer, "lineage/layoutDag.ts");
const compiled = join(compiledDir, "layoutDag.mjs");
writeFileSync(compiled, transformSync(readFileSync(source, "utf8"), { loader: "ts", format: "esm", target: "es2022", sourcefile: source }).code);
const { NODE_HEIGHT, NODE_WIDTH, fitRankedToViewport, rankDag } = await import(pathToFileURL(compiled).href);

const read = (rel) => readFileSync(join(renderer, rel), "utf8");

function assertInside(ranked, view, viewport) {
	for (const node of ranked) {
		const left = node.x * view.scale + view.x;
		const top = node.y * view.scale + view.y;
		const right = (node.x + NODE_WIDTH) * view.scale + view.x;
		const bottom = (node.y + NODE_HEIGHT) * view.scale + view.y;
		assert.ok(left >= -0.5, `${node.id} left ${left}`);
		assert.ok(top >= -0.5, `${node.id} top ${top}`);
		assert.ok(right <= viewport.width + 0.5, `${node.id} right ${right}`);
		assert.ok(bottom <= viewport.height + 0.5, `${node.id} bottom ${bottom}`);
	}
}

test("rankDag plus fit keeps a three-node lineage inside a 400×280 viewport", () => {
	const viewport = { width: 400, height: 280 };
	const members = [
		{ id: "run", kind: "optimizer_run", title: "Run", status: "failed" },
		{ id: "eval", kind: "eval_campaign", title: "Eval", status: "failed" },
		{ id: "direct", kind: "direct_evaluation", title: "Direct", status: "failed" },
	];
	const isolated = rankDag(members, []);
	assertInside(isolated, fitRankedToViewport(isolated, viewport), viewport);

	const chained = rankDag(members, [
		{ id: "e1", sourceId: "run", targetId: "eval", relation: "evaluated" },
		{ id: "e2", sourceId: "eval", targetId: "direct", relation: "evaluated" },
	]);
	assertInside(chained, fitRankedToViewport(chained, viewport), viewport);
});

test("lineage canvas keeps Home/End on the listbox and exposes Fit/Recenter", () => {
	const canvas = read("lineage/LineageCanvas.tsx");
	assert.match(canvas, /event\.key === "Home"/);
	assert.match(canvas, /event\.key === "End"/);
	assert.match(canvas, /aria-activedescendant/);
	assert.match(canvas, /data-testid="lineage-fit"/);
	assert.match(canvas, /data-testid="lineage-recenter"/);
	assert.match(canvas, /tabIndex=\{-1\}/);
});

test("experiment search no-matches offers a clear-search empty state", () => {
	const index = read("experiments/ExperimentIndex.tsx");
	assert.match(index, /data-testid="experiments-no-results"/);
	assert.match(index, /data-testid="experiments-clear-search"/);
	assert.match(index, /No experiments match/);
	assert.match(index, /Clear search/);
});

test("experiment header splits task from provenance and inspector hides raw JSON", () => {
	const workspace = read("experiments/ExperimentWorkspace.tsx");
	const inspector = read("experiments/NodeInspector.tsx");
	assert.match(workspace, /data-testid="experiment-header-task"/);
	assert.match(workspace, /data-testid="experiment-header-provenance"/);
	assert.doesNotMatch(workspace, /missing\(group\.task\) · \{missing\(group\.model\)\}/);
	assert.match(inspector, /data-testid="inspector-technical-details"/);
	assert.match(inspector, /Technical details/);
	assert.match(inspector, /data-testid="inspector-failure"/);
	assert.match(inspector, /formatNodeFailureReason/);
});
