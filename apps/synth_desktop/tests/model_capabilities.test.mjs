import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

const source = join(appRoot, "src/renderer/src/runtime/modelCapabilities.ts");
const compiled = join(compiledDir, "modelCapabilities.mjs");
buildSync({
	entryPoints: [source],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "node",
	outfile: compiled
});

const { modelCapabilitiesForTarget } = await import(pathToFileURL(compiled).href);

test("Laguna XS presents minimal and max thinking without changing transport values", () => {
	const reasoning = modelCapabilitiesForTarget("local-laguna")?.knobs.find((knob) => knob.id === "reasoning");
	assert.deepEqual(reasoning?.options, [
		{ displayValue: "Minimal", transportValue: "none" },
		{ displayValue: "Max", transportValue: "high" }
	]);
	assert.equal(reasoning?.defaultValue, "high");
});
