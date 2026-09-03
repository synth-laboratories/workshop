/**
 * Render receipts — the checkable claim that a visual revision rendered.
 *
 * Not a copy of the evidence: the kernel projection is already durable and,
 * since reads stopped taking the write lock, readable without the producer.
 * What these pin is the ability to notice when local evidence no longer
 * supports what a visual already showed.
 */
import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const outfile = join(compiledDir, "visualRenderReceipt.mjs");
buildSync({
	entryPoints: [join(appRoot, "src/renderer/src/runtime/runProgress/receipt.ts")],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "node",
	outfile
});
const { verifyAgainstReceipt, visualDataDigest } = await import(pathToFileURL(outfile).href);

const view = (projectionRevision, extra = {}) => ({
	algorithm: "gepa",
	header: { runId: "run-a", projectionRevision, lifecycle: "terminal" },
	projection: { candidates: [1, 2, 3], ...extra }
});

function receipt(overrides = {}) {
	return {
		visualId: "vis-1",
		visualRevision: 3,
		optimizerRunId: "run-a",
		templateId: "optimizer.run.v1",
		templateVersion: "tpl-abc",
		projectionRevision: 12,
		dataDigest: visualDataDigest(view(12)),
		tailCursor: 2259,
		renderedAt: "2026-08-31T12:00:00Z",
		...overrides
	};
}

const local = (projectionRevision, v = null, templateVersion = "tpl-abc") => ({
	optimizerRunId: "run-a",
	projectionRevision,
	dataDigest: visualDataDigest(v ?? view(projectionRevision)),
	templateVersion
});

test("a digest is stable across key order and sensitive to content", () => {
	const a = visualDataDigest({ header: { runId: "r", projectionRevision: 1 }, projection: { x: 1, y: 2 } });
	const b = visualDataDigest({ projection: { y: 2, x: 1 }, header: { projectionRevision: 1, runId: "r" } });
	assert.equal(a, b, "an incidental key reorder must not read as corruption");
	const c = visualDataDigest({ header: { runId: "r", projectionRevision: 1 }, projection: { x: 1, y: 3 } });
	assert.notEqual(a, c, "changed content must change the digest");
});

test("evidence at or ahead of the last render is current", () => {
	assert.equal(verifyAgainstReceipt(receipt(), local(12)).kind, "current");
	assert.equal(verifyAgainstReceipt(receipt(), local(13)).kind, "current",
		"a revision that advances is normal, not a mismatch");
});

test("evidence behind what already rendered is a reported regression", () => {
	const verdict = verifyAgainstReceipt(receipt(), local(9));
	assert.equal(verdict.kind, "regressed");
	assert.equal(verdict.renderedRevision, 12);
	assert.equal(verdict.localRevision, 9);
});

test("the same revision carrying different content is detected", () => {
	// The case a bare revision comparison misses entirely.
	const verdict = verifyAgainstReceipt(receipt(), local(12, view(12, { candidates: [9, 9, 9] })));
	assert.equal(verdict.kind, "content_changed");
	assert.equal(verdict.projectionRevision, 12);
});

test("a template change makes the comparison meaningless, not failed", () => {
	// Different code legitimately renders the same projection differently, so
	// its digest is not comparable across versions.
	const verdict = verifyAgainstReceipt(receipt(), local(12, null, "tpl-xyz"));
	assert.equal(verdict.kind, "unverified");
	assert.equal(verdict.reason, "template_changed");
});

test("no prior render, or a different run, is unverified rather than wrong", () => {
	assert.equal(verifyAgainstReceipt(null, local(1)).kind, "unverified");
	assert.equal(verifyAgainstReceipt(undefined, local(1)).reason, "no_receipt");
	const other = verifyAgainstReceipt(receipt({ optimizerRunId: "run-b" }), local(12));
	assert.equal(other.kind, "unverified");
	assert.equal(other.reason, "different_run");
});
