/**
 * A plugin destination must name itself.
 *
 * The titlebar label was a hand-written chain, one branch per destination, and
 * three destinations were never added to it: Jesterky, Environment QA and
 * Computer Use fell through to the trailing default and wore the selected
 * model's name, which is how native acceptance found the Jesterky page titled
 * "GPT-5.6 Luna". The sidebar carries the same shape — one `<id>Active` prop per
 * destination — so both are pinned here against the same drift.
 */

import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const renderer = join(appRoot, "src/renderer/src");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

const compiled = join(compiledDir, "pluginNav.mjs");
buildSync({
	entryPoints: [join(renderer, "runtime/pluginNav.ts")],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "neutral",
	outfile: compiled
});
const { PLUGIN_NAV } = await import(pathToFileURL(compiled).href);

const controller = readFileSync(join(renderer, "hooks/useAppController.ts"), "utf8");
const app = readFileSync(join(renderer, "App.tsx"), "utf8");
const sidebar = readFileSync(join(renderer, "components/Sidebar.tsx"), "utf8");
const routes = readFileSync(join(renderer, "routes.tsx"), "utf8");

test("every destination is a reachable view kind", () => {
	for (const entry of PLUGIN_NAV) {
		assert.match(
			routes,
			new RegExp(`\\|\\s*\\{\\s*kind:\\s*"${entry.id}"`),
			`${entry.id} has a sidebar row but no MainView kind`
		);
	}
});

test("the titlebar takes destination names from the nav table, not a hand-written chain", () => {
	// The fix that matters: one lookup, so a new destination cannot be added
	// without a name. If this is ever expanded back into per-id branches, the
	// per-destination assertion below is what catches an omission.
	assert.match(
		controller,
		/PLUGIN_NAV\.find\(\(entry\) => entry\.id === view\.kind\)/,
		"tabLabel should resolve destinations through PLUGIN_NAV"
	);
	for (const entry of PLUGIN_NAV) {
		const branch = new RegExp(`view\\.kind === "${entry.id}"[\\s\\S]{0,40}\\?`);
		assert.ok(
			!branch.test(controller),
			`${entry.id} is still spelled out in the tabLabel chain; the table should name it`
		);
	}
});

test("no destination falls through to the selected model's name", () => {
	// The trailing default names the execution target. It must only be reached
	// by views that are not destinations at all.
	const tail = controller.slice(controller.indexOf("const tabLabel ="));
	const fallback = tail.indexOf("EXECUTION_TARGETS.find");
	assert.ok(fallback > 0, "the tabLabel fallback should still exist");
	const chain = tail.slice(0, fallback);
	for (const entry of PLUGIN_NAV) {
		assert.ok(
			!chain.includes(`"${entry.id}"`) || chain.includes("PLUGIN_NAV"),
			`${entry.id} must be named before the model-name fallback`
		);
	}
});

test("exactly one sidebar row can be active, and it follows the view", () => {
	for (const entry of PLUGIN_NAV) {
		// Every row's active flag is derived from the current view kind alone,
		// so two rows can never be lit at once and a row can never stay lit
		// after navigation.
		const prop = `${entry.id.replace(/-([a-z])/g, (_, c) => c.toUpperCase())}Active`;
		assert.match(
			app,
			new RegExp(`${prop}=\\{c\\.view\\.kind === "${entry.id}"\\}`),
			`${prop} must be derived from view.kind === "${entry.id}"`
		);
		assert.match(sidebar, new RegExp(`\\b${prop}\\b`), `${prop} must reach the sidebar`);
	}
});

test("a destination tab is not announced as a chat tab", () => {
	assert.match(app, /isChatTab=\{c\.view\.kind === "chat" \|\| c\.view\.kind === "landing"\}/);
});
