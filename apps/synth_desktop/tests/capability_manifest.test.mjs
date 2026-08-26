/**
 * v0.8 capability manifest — About + Diagnostics must tell the truth about
 * Optimizers vs CUA vs Laguna vs Intern. Source-string plus behavioral cover
 * so a copy edit cannot quietly re-merge those nouns.
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

const read = (rel) => readFileSync(join(renderer, rel), "utf8");

const compiled = join(compiledDir, "capabilityManifest.mjs");
buildSync({
	entryPoints: [join(renderer, "runtime/capabilityManifest.ts")],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "neutral",
	outfile: compiled
});

const { v08CapabilityRows, OPTIMIZERS_VISUAL_FAMILIES_BUNDLED } = await import(
	pathToFileURL(compiled).href
);

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

test("About and Diagnostics both mount capability-manifest", () => {
	const about = read("components/SettingsPage.tsx");
	const diagnostics = read("components/DiagnosticsPanel.tsx");
	assert.match(about, /data-testid="settings-about"/);
	assert.match(about, /CapabilityManifest/);
	assert.match(diagnostics, /CapabilityManifest/);
	assert.match(read("components/CapabilityManifest.tsx"), /data-testid="capability-manifest"/);
});

test("Intern/CloudDesk are listed unsupported; Laguna is not a plugin", () => {
	const catalog = read("runtime/capabilityManifest.ts");
	const nav = read("runtime/pluginNav.ts");
	assert.match(catalog, /intern \/ CloudDesk/);
	assert.match(catalog, /Unsupported in v0\.8 \(v0\.1 removal\)/);
	assert.match(catalog, /parallel LagunaStatus, not a plugin/);
	assert.match(catalog, /Local sidecar/);
	assert.doesNotMatch(nav, /id: "laguna"/);
	assert.doesNotMatch(nav, /pluginId: "laguna"/);
	assert.doesNotMatch(nav, /Intern|CloudDesk/);
});

test("optimizers visual families are bundled independently of sidecar phase", () => {
	const catalog = read("runtime/capabilityManifest.ts");
	assert.match(catalog, /plugin sidecar/);
	assert.match(catalog, /Optimizers visual families are bundled/);
	assert.match(catalog, /GEPA\/SFT recipe runner is not ready/);
	assert.match(catalog, /compose\/sourced visuals/);
	assert.match(catalog, /Bundled \(source families\)/);
	assert.match(catalog, /human-only plugin/);
	assert.match(catalog, /never agent-installable/);
});

test("not_installed sidecar does not imply the recipe runner is ready", () => {
	const rows = v08CapabilityRows({
		pluginStatuses: [status({ phase: "not_installed" })],
		lagunaPhase: "ready"
	});
	const optimizers = rows.find((row) => row.id === "optimizers");
	assert.equal(optimizers.kind, "plugin sidecar");
	assert.match(optimizers.thisBuild, /Not installed/);
	assert.match(optimizers.thisBuild, /GEPA\/SFT recipe runner is not ready/);
	assert.ok(optimizers.thisBuild.includes(OPTIMIZERS_VISUAL_FAMILIES_BUNDLED));
	assert.doesNotMatch(optimizers.thisBuild, /recipe runner is available/);

	const visuals = rows.find((row) => row.id === "compose/sourced visuals");
	assert.equal(visuals.kind, "bundled");
	assert.match(visuals.thisBuild, /Bundled \(source families\)/);
});

test("a ready sidecar says the recipe runner is available and still bundles families", () => {
	const rows = v08CapabilityRows({
		pluginStatuses: [status({ phase: "ready" })]
	});
	const optimizers = rows.find((row) => row.id === "optimizers");
	assert.match(optimizers.thisBuild, /^Ready —/);
	assert.match(optimizers.thisBuild, /GEPA\/SFT recipe runner is available/);
	assert.ok(optimizers.thisBuild.includes(OPTIMIZERS_VISUAL_FAMILIES_BUNDLED));
});

test("computer-use is human-only and never agent-installable", () => {
	const rows = v08CapabilityRows({
		pluginStatuses: [status({ pluginId: "computer-use", phase: "not_installed" })]
	});
	const cua = rows.find((row) => row.id === "computer-use");
	assert.equal(cua.kind, "human-only plugin");
	assert.match(cua.thisBuild, /Not installed/);
	assert.match(cua.thisBuild, /never agent-installable/);
});

test("Laguna this-build is a local sidecar phase, not a plugin row", () => {
	const rows = v08CapabilityRows({ lagunaPhase: "loading" });
	const laguna = rows.find((row) => row.id === "laguna");
	assert.equal(laguna.kind, "parallel LagunaStatus, not a plugin");
	assert.match(laguna.thisBuild, /Local sidecar · Loading/);
	assert.doesNotMatch(laguna.kind, /^plugin/);
});

test("Intern/CloudDesk stay unsupported in this build", () => {
	const intern = v08CapabilityRows().find((row) => row.id === "intern / CloudDesk");
	assert.equal(intern.kind, "unmounted");
	assert.equal(intern.thisBuild, "Unsupported in v0.8 (v0.1 removal)");
});

test("routes pass existing pluginStatuses and LagunaStatus phase, inventing no IPC", () => {
	const routes = read("routes.tsx");
	assert.match(routes, /<SettingsPage[\s\S]{0,1200}pluginStatuses=\{pluginStatuses\}/);
	assert.match(routes, /<DiagnosticsPanel[\s\S]{0,400}pluginStatuses=\{pluginStatuses\}[\s\S]{0,80}lagunaPhase=\{laguna\?\.phase\}/);
	assert.doesNotMatch(routes, /list_components/);
	assert.doesNotMatch(read("runtime/pluginNav.ts"), /id: "laguna"/);
});
