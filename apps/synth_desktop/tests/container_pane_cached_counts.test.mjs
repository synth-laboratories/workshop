import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

const source = join(appRoot, "src/renderer/src/components/ContainerPane.tsx");
const compiled = join(compiledDir, "containerPane.mjs");
buildSync({
	entryPoints: [source],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "node",
	outfile: compiled,
	external: ["react"],
	jsx: "automatic"
});

const { countLabel, taskMetadata } = await import(pathToFileURL(compiled).href);

test("cached instance counts are labeled cached, never as live readiness", () => {
	assert.equal(countLabel(5, true, "cached"), "5 cached");
	assert.equal(countLabel(7, true, "cached"), "7 cached");
	assert.equal(countLabel(5, true, "live"), "5 live");
	assert.equal(countLabel(5, true, "unavailable"), "Not reported");
	assert.equal(countLabel(5, false, "cached"), "Not reported");
});

test("missing task metadata is unavailable rather than malformed", () => {
	assert.deepEqual(taskMetadata(undefined, undefined), { tasks: [], instances: [], error: null });
	assert.deepEqual(taskMetadata(undefined, null), { tasks: [], instances: [], error: null });
});

test("reported empty task metadata remains an actionable schema error", () => {
	assert.equal(taskMetadata(undefined, {}).error, "invalid task info: expected an object");
});
