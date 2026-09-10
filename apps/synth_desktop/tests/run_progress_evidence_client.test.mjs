/**
 * `EvidenceClient` — the coverage contract.
 *
 * Raw evidence is browsed, not streamed. The client's job is to remember what
 * it holds so the same bytes never cross the bridge twice, including across a
 * window that widens backwards — the case a cursor cannot express.
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
const outfile = join(compiledDir, "runProgressEvidence.mjs");
buildSync({
	entryPoints: [join(appRoot, "src/renderer/src/runtime/runProgress/evidence.ts")],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "node",
	outfile
});
const { createEvidenceClient } = await import(pathToFileURL(outfile).href);

/** Ranges sorted and coalesced, mirroring the backend's normalization. */
function normalize(ranges) {
	const sorted = ranges.filter((r) => r.from <= r.to).sort((a, b) => a.from - b.from || a.to - b.to);
	const merged = [];
	for (const range of sorted) {
		const last = merged[merged.length - 1];
		if (last && range.from <= last.to + 1) last.to = Math.max(last.to, range.to);
		else merged.push({ ...range });
	}
	return merged;
}

function complement(window, held) {
	const gaps = [];
	let cursor = window.from;
	for (const range of normalize(held)) {
		if (range.to < cursor) continue;
		if (range.from > window.to) break;
		if (range.from > cursor) gaps.push({ from: cursor, to: range.from - 1 });
		cursor = Math.max(cursor, range.to + 1);
		if (cursor > window.to) return gaps;
	}
	if (cursor <= window.to) gaps.push({ from: cursor, to: window.to });
	return gaps;
}

/** A backend holding `total` events, faithful to the Rust implementation. */
function fakeBackend(total, pageLimit = 200) {
	const sent = [];
	return {
		sent,
		async evidencePage(_runId, window, held, limit) {
			const cap = Math.min(limit ?? pageLimit, pageLimit);
			const clamped = { from: Math.max(1, window.from), to: Math.min(total, window.to) };
			const gaps = complement(clamped, held ?? []);
			if (gaps.length === 0) {
				return { events: [], range: null, coverage: normalize(held ?? []), complete: true, tailCursor: total };
			}
			const gap = gaps[0];
			const events = [];
			for (let s = gap.from; s <= gap.to && events.length < cap; s += 1) {
				events.push({ sequenceNumber: s, type: "x" });
			}
			const covered = events.length === cap
				? { from: gap.from, to: events[events.length - 1].sequenceNumber }
				: gap;
			sent.push(...events.map((e) => e.sequenceNumber));
			const coverage = normalize([...(held ?? []), covered]);
			return {
				events,
				range: covered,
				coverage,
				complete: complement(clamped, coverage).length === 0,
				tailCursor: total
			};
		}
	};
}

test("a widening window transfers only the hole, never what is already held", async () => {
	const backend = fakeBackend(2259);
	const client = createEvidenceClient("run-a", backend);

	const tail = await client.load({ from: 2000, to: 2259 });
	assert.equal(tail.length, 260);
	assert.equal(client.tail(), 2259);

	const sentAfterTail = backend.sent.length;
	const all = await client.load({ from: 1, to: 2259 });
	assert.equal(all.length, 2259, "the client returns the whole window");
	assert.equal(
		backend.sent.length - sentAfterTail,
		1999,
		"only the 1..1999 hole crossed the bridge; the tail was not re-sent"
	);
	assert.deepEqual(client.coverage(), [{ from: 1, to: 2259 }]);
});

test("re-loading a covered window sends nothing at all", async () => {
	const backend = fakeBackend(500);
	const client = createEvidenceClient("run-a", backend);
	await client.load({ from: 1, to: 500 });
	const sent = backend.sent.length;
	const again = await client.load({ from: 1, to: 500 });
	assert.equal(again.length, 500);
	assert.equal(backend.sent.length, sent, "a covered window is free");
});

test("disjoint spans stay disjoint until the gap between them is asked for", async () => {
	// The case a cursor cannot express: holding 1..100 and 400..500, "after
	// 500" fetches nothing and leaves 101..399 missing forever.
	const backend = fakeBackend(500);
	const client = createEvidenceClient("run-a", backend);
	await client.load({ from: 1, to: 100 });
	await client.load({ from: 400, to: 500 });
	assert.deepEqual(client.coverage(), [{ from: 1, to: 100 }, { from: 400, to: 500 }]);

	const middle = await client.load({ from: 150, to: 200 });
	assert.equal(middle.length, 51);
	assert.deepEqual(
		client.coverage(),
		[{ from: 1, to: 100 }, { from: 150, to: 200 }, { from: 400, to: 500 }]
	);
});

test("a bounded page is driven to completion rather than handed back short", async () => {
	const backend = fakeBackend(1000, 100);
	const client = createEvidenceClient("run-a", backend, 100);
	const events = await client.load({ from: 1, to: 1000 });
	assert.equal(events.length, 1000, "the client pages until the window is covered");
	assert.deepEqual(client.coverage(), [{ from: 1, to: 1000 }]);
	assert.equal(new Set(backend.sent).size, backend.sent.length, "no event was sent twice");
});

test("an empty or inverted window asks the backend nothing", async () => {
	const backend = fakeBackend(100);
	const client = createEvidenceClient("run-a", backend);
	assert.deepEqual(await client.load({ from: 50, to: 10 }), []);
	assert.equal(backend.sent.length, 0);
});
