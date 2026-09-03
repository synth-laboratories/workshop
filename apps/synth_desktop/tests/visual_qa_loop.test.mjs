/**
 * The parse half of run -> record -> parse -> note.
 *
 * Grouping is the part that decides whether a review is readable: the same
 * defect appears in every frame of a recording, so raw counts make one bug look
 * like forty, and a defect that comes and goes is a different problem from one
 * that is always there.
 */

import assert from "node:assert/strict";
import test from "node:test";
import { summarizeFindings, triage } from "../../../scripts/workshop-visual-loop.mjs";

const frame = (name, findings) => ({ name, ok: true, audit: { findings } });
const overflow = (target) => ({
	category: "responsive-geometry",
	rule: "horizontal-overflow",
	severity: "egregious",
	target,
	detail: "right edge 1200px exceeds the 1000px viewport"
});
const clipped = (target) => ({
	category: "truncation",
	rule: "clipped-text",
	severity: "review",
	target,
	detail: "needs 400px in a 100px box"
});

test("one defect across many frames is one finding, not many", () => {
	const summary = summarizeFindings([
		frame("frame-0000", [overflow(".sv-table")]),
		frame("frame-0001", [overflow(".sv-table")]),
		frame("frame-0002", [overflow(".sv-table")])
	]);
	assert.equal(summary.length, 1);
	assert.equal(summary[0].frameCount, 3);
	// Present in every frame it could be: structural, not a transient.
	assert.equal(summary[0].structural, true);
});

test("a defect that comes and goes is not structural", () => {
	const summary = summarizeFindings([
		frame("frame-0000", [overflow(".sv-table")]),
		frame("frame-0001", []),
		frame("frame-0002", [overflow(".sv-table")])
	]);
	assert.equal(summary[0].frameCount, 2);
	assert.equal(summary[0].structural, false);
});

test("the same rule on different targets stays separate", () => {
	const summary = summarizeFindings([frame("a", [overflow(".one"), overflow(".two")])]);
	assert.equal(summary.length, 2);
	// A single frame cannot establish that anything is structural.
	assert.equal(summary.every((finding) => finding.structural === false), true);
});

test("egregious findings sort ahead of judgement calls", () => {
	const summary = summarizeFindings([
		frame("a", [clipped(".title"), overflow(".table")]),
		frame("b", [clipped(".title")])
	]);
	assert.equal(summary[0].rule, "horizontal-overflow");
	const { fix, review } = triage(summary);
	assert.deepEqual(fix.map((f) => f.rule), ["horizontal-overflow"]);
	assert.deepEqual(review.map((f) => f.rule), ["clipped-text"]);
});

test("failed captures do not count toward structural presence", () => {
	// A capture that never landed cannot witness a defect; counting it would
	// make a real structural finding look intermittent.
	const summary = summarizeFindings([
		frame("a", [overflow(".t")]),
		frame("b", [overflow(".t")]),
		{ name: "c", ok: false, error: "capture failed" }
	]);
	assert.equal(summary[0].frameCount, 2);
	assert.equal(summary[0].structural, true);
});

test("no findings summarizes to nothing rather than to a claim of quality", () => {
	const summary = summarizeFindings([frame("a", []), frame("b", [])]);
	assert.deepEqual(summary, []);
	assert.deepEqual(triage(summary), { fix: [], review: [] });
});
