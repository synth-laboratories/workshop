import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
const compiled = join(compiledDir, "modelCapabilities.mjs");
mkdirSync(compiledDir, { recursive: true });
buildSync({ entryPoints: [join(appRoot, "src/renderer/src/runtime/modelCapabilities.ts")], bundle: true, format: "esm", target: "es2022", platform: "node", outfile: compiled });
const { modelCapabilitiesForTarget } = await import(pathToFileURL(compiled).href);

test("Laguna XS separates display and transport thinking values", () => {
	const reasoning = modelCapabilitiesForTarget("local-laguna")?.knobs.find((knob) => knob.id === "reasoning");
	assert.deepEqual(reasoning?.options, [
		{ displayValue: "Minimal", transportValue: "none" },
		{ displayValue: "Max", transportValue: "high" }
	]);
});
