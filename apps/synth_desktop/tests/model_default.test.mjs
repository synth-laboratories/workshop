import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const compiled = join(compiledDir, "model-default.mjs");
buildSync({
	entryPoints: [join(appRoot, "src/renderer/src/types/landing.ts")],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "node",
	outfile: compiled
});

const { resolveDefaultTargetId } = await import(pathToFileURL(compiled).href);
const preference = { model: "gpt-5.6-luna", effort: "xhigh", providers: ["chatgpt", "openrouter"] };

test("TOML default prefers ChatGPT Luna over OpenRouter Luna", () => {
	assert.equal(resolveDefaultTargetId(preference, { chatgpt: true, openrouter: true }), "chatgpt-luna");
});

test("TOML default falls back to local Laguna when ChatGPT and OpenRouter Luna are unavailable", () => {
	assert.equal(resolveDefaultTargetId(preference, { chatgpt: false, openrouter: true }), "local-laguna");
	assert.equal(resolveDefaultTargetId(preference, { chatgpt: false, openrouter: false }), "local-laguna");
});

test("unavailable configured providers fall back to most-used available model with recency ties", () => {
	const unavailable = { chatgpt: false, openrouter: false, synth: true };
	assert.equal(resolveDefaultTargetId(preference, unavailable, [
		{ targetId: "local-laguna", updatedAt: "2026-08-01T00:00:00Z" },
		{ targetId: "synth-cloud-laguna-s", updatedAt: "2026-08-02T00:00:00Z" },
		{ targetId: "synth-cloud-laguna-s", updatedAt: "2026-08-03T00:00:00Z" }
	]), "synth-cloud-laguna-s");
	assert.equal(resolveDefaultTargetId(preference, unavailable, [
		{ targetId: "local-laguna", updatedAt: "2026-08-01T00:00:00Z" },
		{ targetId: "synth-cloud-muse-spark", updatedAt: "2026-08-03T00:00:00Z" }
	]), "synth-cloud-muse-spark");
});
