import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const source = join(appRoot, "src/renderer/src/runtime/paidComputeUsd.ts");
const compiled = join(compiledDir, "paidComputeUsd.mjs");
buildSync({
	entryPoints: [source],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "node",
	outfile: compiled
});

const { parseUsdAmount, formatUsdMicros } = await import(pathToFileURL(compiled).href);

test("settings validate negative, malformed, and over-precision amounts", () => {
	assert.equal(parseUsdAmount("0.10").micros, 100_000);
	assert.equal(parseUsdAmount("1").micros, 1_000_000);
	assert.equal(parseUsdAmount("0.000001").micros, 1);
	assert.match(parseUsdAmount("-0.10").error ?? "", /negative|signed/);
	assert.match(parseUsdAmount("+0.10").error ?? "", /signed/);
	assert.match(parseUsdAmount("1e-1").error ?? "", /exponent/);
	assert.match(parseUsdAmount("0.1234567").error ?? "", /six fractional/);
	assert.match(parseUsdAmount("abc").error ?? "", /decimal/);
	assert.match(parseUsdAmount("").error ?? "", /Enter/);
	assert.equal(formatUsdMicros(60_000), "0.06");
	assert.equal(formatUsdMicros(100_000), "0.10");
	assert.equal(formatUsdMicros(250_000), "0.25");
	assert.equal(formatUsdMicros(18_000), "0.018");
	assert.equal(formatUsdMicros(1_000_000), "1.00");
});
