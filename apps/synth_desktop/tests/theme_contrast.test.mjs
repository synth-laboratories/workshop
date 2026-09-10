import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const styles = join(appRoot, "src/renderer/src/styles");

function read(rel) {
	return readFileSync(join(styles, rel), "utf8");
}

test("sidebar background follows --color-sidebar so dark theme is readable", () => {
	const css = read("app.css");
	assert.match(css, /\.sidebar\s*\{[^}]*background:\s*var\(--color-sidebar\)/s);
	assert.doesNotMatch(css, /\.sidebar\s*\{[^}]*background:\s*#f3f5f8/s);
});

test("dark theme remaps shell tokens including sidebar", () => {
	const css = read("app.css");
	const dark = css.match(/:root\[data-theme="dark"\]\s*\{[^}]+\}/)?.[0] ?? "";
	assert.match(dark, /--color-sidebar:/);
	assert.match(dark, /--color-text:/);
	assert.match(dark, /--color-card:/);
});

test("visual pane and preview chrome consume surface tokens", () => {
	const css = read("app.css");
	assert.match(css, /\.visual-pane\s*\{[^}]*background:\s*var\(--color-bg-subtle\)/s);
	assert.match(css, /\.visual-pane-head\s*\{[^}]*background:\s*var\(--color-card\)/s);
	assert.match(css, /\.visual-pane-foot\s*\{[^}]*background:\s*var\(--color-card\)/s);
	assert.match(css, /\.visuals-preview\s*\{[^}]*background:\s*var\(--color-surface\)/s);
	assert.match(css, /\.visuals-preview header h2\s*\{[^}]*color:\s*var\(--color-text\)/s);
	assert.match(css, /\.visual-empty-preview\s*\{[^}]*color:\s*var\(--color-text-muted\)/s);
	assert.match(css, /\.visual-empty-preview strong\s*\{[^}]*color:\s*var\(--color-text\)/s);
});

test("dark lineage canvas and nodes override the warm parchment", () => {
	const css = read("app.css");
	assert.match(css, /:root\[data-theme="dark"\] \.lineage-canvas\s*\{[^}]*background:\s*var\(--color-bg-subtle\)/s);
	assert.match(css, /:root\[data-theme="dark"\] \.lineage-node\s*\{[^}]*color:\s*var\(--color-text\)/s);
	assert.doesNotMatch(
		css,
		/:root\[data-theme="dark"\] \.lineage-canvas\s*\{[^}]*background:\s*#f9f1e5/s
	);
});

test("surface aliases follow card/bg tokens so dark theme re-points them", () => {
	const tokens = read("tokens.css");
	assert.match(tokens, /--color-surface:\s*var\(--color-card\)/);
	assert.match(tokens, /--color-surface-sunken:\s*var\(--color-bg-subtle\)/);
});

test("compact workbench space-allocation is a tokenized media policy", () => {
	const tokens = read("tokens.css");
	const css = read("app.css");
	assert.match(tokens, /--visual-pane-compact-max:\s*40vw/);
	assert.match(tokens, /html\.sidebar-hidden remains the existing sidebar toggle/);
	assert.match(css, /@media \(max-width: 1100px\)/);
	assert.match(css, /@media \(max-width: 860px\)/);
	assert.match(css, /html\.compact-workbench/);
	assert.match(css, /min\(var\(--visual-pane-compact-max,\s*40vw\)/);
	assert.match(css, /position:\s*absolute/);
	assert.match(css, /\.visuals-page-actions button[\s\S]*white-space:\s*nowrap/s);
	assert.match(
		css,
		/\.workbench\.with-side-panel\.with-container \.chat-transcript-scroll[\s\S]*padding-bottom/s
	);
	assert.match(css, /html\.compact-workbench \.experiment-row:not\(\.heading\)/);
	assert.match(css, /html\.compact-workbench \.experiment-task-disclosure/);
	assert.match(css, /\.inference-panel\s*\{[^}]*max-width:\s*none/s);
});

test("blank canvas empty state uses theme text tokens, not dark-on-dark hex", () => {
	const shell = readFileSync(
		join(appRoot, "../../visuals/families/analysis/blank.canvas.v1/shell.tsx"),
		"utf8"
	);
	const emptyState = shell.match(/if \(!document\?\.html\) \{[\s\S]*?\n  \}/)?.[0] ?? "";
	assert.match(emptyState, /className="visual-empty-preview"/);
	assert.doesNotMatch(emptyState, /#20242c|#697180/);
	assert.doesNotMatch(emptyState, /style=\{\{/);
});
