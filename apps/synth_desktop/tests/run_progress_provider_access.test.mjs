import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

const outfile = join(compiledDir, "runProgressProviderAccess.mjs");
buildSync({
	entryPoints: [join(appRoot, "src/renderer/src/runtime/runProgress/providerAccess.ts")],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "node",
	outfile
});
const { providerAccessFromSecrets } = await import(pathToFileURL(outfile).href);

test("a terminal GEPA run does not invent a missing credential after lease cleanup", () => {
	assert.equal(providerAccessFromSecrets({
		terminal: true,
		proxyRunning: true
	}), undefined);
});

test("an absent optional lease is not proof that an active provider credential is missing", () => {
	assert.equal(providerAccessFromSecrets({
		terminal: false,
		proxyRunning: true
	}), undefined);
});

test("a live capability remains observable as Workshop-proxied provider access", () => {
	const access = providerAccessFromSecrets({
		terminal: false,
		proxyRunning: true,
		capability: {
			provider: "openrouter",
			status: "active",
			displaySuffix: "…abcd",
			usedCalls: 1,
			maxCalls: 24,
			usedCostUsd: 0.05,
			maxCostUsd: 2.45
		}
	});
	assert.deepEqual(access, {
		provider: "openrouter",
		status: "healthy",
		suffix: "…abcd",
		usedCalls: 1,
		maxCalls: 24,
		usedCostUsd: 0.05,
		maxCostUsd: 2.45,
		note: "Via Workshop proxy"
	});
});
