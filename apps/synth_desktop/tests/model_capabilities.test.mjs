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

test("remote Muse Spark exposes OpenRouter reasoning and multimodal capabilities", () => {
	const capability = modelCapabilitiesForTarget("openrouter-muse-spark");
	const reasoning = capability?.knobs.find((knob) => knob.id === "reasoning");
	assert.deepEqual(capability?.target, { kind: "remote", models: ["meta/muse-spark-1.2"] });
	assert.equal(capability?.maxContextTokens, 1_048_576);
	assert.equal(reasoning?.defaultValue, "medium");
	assert.deepEqual(reasoning?.options.map((option) => option.transportValue), ["low", "medium", "high", "xhigh"]);
});

test("Luna defaults to XHigh for ChatGPT and OpenRouter", () => {
	for (const target of ["chatgpt-luna", "openrouter-luna"]) {
		const reasoning = modelCapabilitiesForTarget(target)?.knobs.find((knob) => knob.id === "reasoning");
		assert.equal(reasoning?.defaultValue, "xhigh");
	}
});
