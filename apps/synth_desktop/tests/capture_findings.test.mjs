/**
 * The machine-checkable half of the visual standard.
 *
 * These rules decide what a review can flag without a human. Each is separated
 * by severity: "egregious" means mechanically decidable and safe to act on,
 * "review" means the measurement is certain but the judgement is not.
 */

import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { transformSync } from "esbuild";
import test from "node:test";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const source = join(appRoot, "src/renderer/src/runtime/captureFindings.ts");
const compiled = join(compiledDir, "captureFindings.mjs");
writeFileSync(
	compiled,
	transformSync(readFileSync(source, "utf8"), { loader: "ts", format: "esm", target: "es2022" }).code
);
const {
	auditElements,
	findClippedText,
	findHorizontalOverflow,
	findIllegibleText,
	findPlaceholderSaturation
} = await import(pathToFileURL(compiled).href);

const element = (overrides = {}) => ({
	tag: "div",
	rect: { x: 0, y: 0, width: 100, height: 20 },
	scrollWidth: 100,
	clientWidth: 100,
	fontSize: 12,
	text: "",
	overflowX: "visible",
	textOverflow: "clip",
	...overrides
});

test("overflow is measured against the viewport, with a tolerance", () => {
	const viewport = { width: 1000 };
	// Sub-pixel and rounding noise must not become a finding; a real offender
	// is a box that visibly leaves the pane.
	assert.equal(findHorizontalOverflow([element({ rect: { x: 0, y: 0, width: 1003, height: 10 } })], viewport).length, 0);
	const offenders = findHorizontalOverflow(
		[element({ testid: "wide-table", rect: { x: 600, y: 0, width: 600, height: 10 } })],
		viewport
	);
	assert.equal(offenders.length, 1);
	assert.equal(offenders[0].severity, "egregious");
	assert.match(offenders[0].target, /wide-table/);
	assert.match(offenders[0].detail, /1200px exceeds the 1000px viewport/);
});

test("only clipped text counts as truncation, never wrapping", () => {
	// A wrapping element legitimately reports scrollWidth > clientWidth.
	const wrapping = element({ text: "a long label", scrollWidth: 400, clientWidth: 100 });
	assert.deepEqual(findClippedText([wrapping]), []);

	const ellipsized = element({
		testid: "card-title",
		text: "Banking77 · CISPO training",
		scrollWidth: 400,
		clientWidth: 100,
		textOverflow: "ellipsis"
	});
	const found = findClippedText([ellipsized]);
	assert.equal(found.length, 1);
	// Ellipsis is often deliberate; whether it ate the differentiating word is
	// a judgement, so this never auto-fixes.
	assert.equal(found[0].severity, "review");
	assert.equal(found[0].category, "truncation");

	// An element with no text cannot be truncated text.
	assert.deepEqual(findClippedText([element({ scrollWidth: 999, clientWidth: 10, overflowX: "hidden" })]), []);
});

test("text below the legibility floor is egregious", () => {
	assert.deepEqual(findIllegibleText([element({ text: "ok", fontSize: 9 })]), []);
	const tiny = findIllegibleText([element({ text: "STEP", fontSize: 8 })]);
	assert.equal(tiny.length, 1);
	assert.equal(tiny[0].severity, "egregious");
	// A zero font-size is a measurement failure, not a finding.
	assert.deepEqual(findIllegibleText([element({ text: "x", fontSize: 0 })]), []);
});

test("placeholder saturation needs enough leaves to mean anything", () => {
	const dash = (n) => Array.from({ length: n }, () => element({ text: "—" }));
	const real = (n) => Array.from({ length: n }, (_, i) => element({ text: `0.${i}` }));

	// Honest missingness in a small panel is not a finding.
	assert.deepEqual(findPlaceholderSaturation([...dash(3), ...real(2)]), []);
	// A surface that is mostly placeholder reads as broken, not as empty.
	const saturated = findPlaceholderSaturation([...dash(8), ...real(2)]);
	assert.equal(saturated.length, 1);
	assert.match(saturated[0].detail, /8 of 10/);
	assert.equal(saturated[0].category, "missing-evidence");
	// Mixed evidence stays quiet.
	assert.deepEqual(findPlaceholderSaturation([...dash(4), ...real(6)]), []);
});

test("an audit aggregates rules and counts them by name", () => {
	const audit = auditElements(
		[
			element({ testid: "a", rect: { x: 900, y: 0, width: 400, height: 10 } }),
			element({ testid: "b", text: "tiny", fontSize: 7 }),
			element({ testid: "c", text: "tiny too", fontSize: 6 })
		],
		{ width: 1000, height: 800 }
	);
	assert.equal(audit.elementCount, 3);
	assert.equal(audit.counts["horizontal-overflow"], 1);
	assert.equal(audit.counts["illegible-text"], 2);
	assert.equal(audit.findings.length, 3);
	assert.equal(audit.viewport.width, 1000);
});

test("a clean surface produces no findings", () => {
	const audit = auditElements([element({ text: "Heldout uplift" }), element({ text: "+0.12" })], {
		width: 1440,
		height: 900
	});
	assert.deepEqual(audit.findings, []);
	assert.deepEqual(audit.counts, {});
});
