/**
 * Regression tests for preference normalization.
 *
 * The 16k bug: `clampNumber` coerced unset values (null / "") through
 * `Number()` to 0, then clamped them UP to the 16,000 autocompact floor —
 * so every fresh store booted with 16k limits for all models and caused
 * mid-turn autocompact loops. Normalize must yield documented defaults for
 * anything unset and must remediate blobs the bug already poisoned.
 */
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { transformSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

function compile(relative, outName) {
	const source = join(appRoot, relative);
	const compiled = join(compiledDir, outName);
	writeFileSync(
		compiled,
		transformSync(readFileSync(source, "utf8"), {
			loader: "ts",
			format: "esm",
			target: "es2022",
			sourcefile: source
		}).code
	);
	return pathToFileURL(compiled).href;
}

const {
	DEFAULT_AUTO_COMPACT_TOKEN_LIMITS,
	DEFAULT_PREFERENCES,
	PREFERENCES_SCHEMA_VERSION,
	clampNumber,
	normalizePreferences
} = await import(compile("src/renderer/src/preferences/schema.ts", "PreferencesSchema.mjs"));

const DEFAULT_LIMITS = { ...DEFAULT_AUTO_COMPACT_TOKEN_LIMITS };

test("clampNumber treats unset as fallback, never as zero clamped to the floor", () => {
	assert.equal(clampNumber(null, 16_000, 945_000, 250_000), 250_000);
	assert.equal(clampNumber(undefined, 16_000, 945_000, 250_000), 250_000);
	assert.equal(clampNumber("", 16_000, 945_000, 250_000), 250_000);
	assert.equal(clampNumber(32_000, 16_000, 945_000, 250_000), 32_000);
	assert.equal(clampNumber(1, 16_000, 945_000, 250_000), 16_000);
});

test("fresh and empty stores normalize to the documented autocompact defaults", () => {
	for (const raw of [null, undefined, {}, { agentContext: {} }, { agentContext: { autoCompactTokenLimits: {} } }]) {
		assert.deepEqual(
			normalizePreferences(raw).agentContext.autoCompactTokenLimits,
			DEFAULT_LIMITS,
			`raw=${JSON.stringify(raw)}`
		);
	}
	assert.deepEqual(normalizePreferences(DEFAULT_PREFERENCES).agentContext.autoCompactTokenLimits, DEFAULT_LIMITS);
});

test("blobs poisoned with the 16k floor before v4 are remediated to defaults", () => {
	const poisoned = {
		schemaVersion: 3,
		agentContext: { autoCompactTokenLimits: { lagunaXs: 16_000, lagunaS: 16_000, luna: 16_000 } }
	};
	assert.deepEqual(normalizePreferences(poisoned).agentContext.autoCompactTokenLimits, DEFAULT_LIMITS);
	// A blob with no version at all predates every schema — same remediation.
	const unversioned = { agentContext: { autoCompactTokenLimits: { lagunaXs: 16_000, lagunaS: 16_000, luna: 16_000 } } };
	assert.deepEqual(normalizePreferences(unversioned).agentContext.autoCompactTokenLimits, DEFAULT_LIMITS);
});

test("16k and lower compact limits always normalize to model defaults", () => {
	for (const schemaVersion of [undefined, 3, PREFERENCES_SCHEMA_VERSION]) {
		for (const poisonedValue of [null, "", -1, 0, 1, 15_999, 16_000, "16000"]) {
			const raw = {
				...(schemaVersion === undefined ? {} : { schemaVersion }),
				agentContext: {
					autoCompactTokenLimit: poisonedValue,
					autoCompactTokenLimits: {
						lagunaXs: poisonedValue,
						lagunaS: poisonedValue,
						luna: poisonedValue
					}
				}
			};
			assert.deepEqual(
				normalizePreferences(raw).agentContext.autoCompactTokenLimits,
				DEFAULT_LIMITS,
				`schema=${schemaVersion} value=${JSON.stringify(poisonedValue)}`
			);
		}
	}
});

test("intentional customs at the current schema survive normalization", () => {
	const custom = {
		schemaVersion: PREFERENCES_SCHEMA_VERSION,
		agentContext: { autoCompactTokenLimits: { lagunaXs: 32_000, lagunaS: 500_000, luna: 250_000 } }
	};
	assert.deepEqual(
		normalizePreferences(custom).agentContext.autoCompactTokenLimits,
		{ lagunaXs: 32_000, lagunaS: 500_000, luna: 250_000 }
	);
});

test("unset appearance numbers fall back to defaults instead of their minimums", () => {
	const normalized = normalizePreferences({ appearance: { chatFontSize: null, codeFontSize: "" } });
	assert.equal(normalized.appearance.chatFontSize, DEFAULT_PREFERENCES.appearance.chatFontSize);
	assert.equal(normalized.appearance.codeFontSize, DEFAULT_PREFERENCES.appearance.codeFontSize);
});

test("showMascot defaults off and only true survives", () => {
	assert.equal(DEFAULT_PREFERENCES.appearance.showMascot, false);
	assert.equal(normalizePreferences({}).appearance.showMascot, false);
	assert.equal(normalizePreferences({ appearance: { showMascot: "yes" } }).appearance.showMascot, false);
	assert.equal(normalizePreferences({ appearance: { showMascot: true } }).appearance.showMascot, true);
});
