// Behavioral cover for trace-inspector identity. These call the real functions
// rather than matching source text, so a rename cannot make them pass and a
// logic change cannot make them silently keep passing.

import test from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { transformSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

const source = join(appRoot, "src/renderer/src/runtime/traceInspector.ts");
const compiled = join(compiledDir, "traceInspector.mjs");
writeFileSync(compiled, transformSync(readFileSync(source, "utf8"), {
	loader: "ts",
	format: "esm",
	target: "es2022"
}).code);

const {
	findTraceInspectorVisual,
	traceInspectability,
	traceInspectorVisualId,
	traceInspectorCreateRequest,
	traceDigestBinding,
	TRACE_INSPECTOR_TEMPLATE
} = await import(pathToFileURL(compiled).href);

const trace = (overrides = {}) => ({
	id: "trace_alpha",
	title: "Craftax Rust · dogfood trace 3",
	digest: "sha256:aaaa1111",
	metadata: {},
	...overrides
});

const inspectorVisual = (digest, overrides = {}) => ({
	id: `vis_trace_${digest}`,
	templateId: TRACE_INSPECTOR_TEMPLATE,
	title: "whatever",
	bindings: {
		schemaVersion: "synth.visual-bindings.v1",
		slots: [{ slot: "projection", kind: "trace_v5", source: digest, schema: "s" }]
	},
	metadata: { traceDigest: digest },
	...overrides
});

test("a visual bound to the same sealed digest is reused", () => {
	const subject = trace();
	const found = findTraceInspectorVisual([inspectorVisual("sha256:aaaa1111")], subject);
	assert.equal(found?.id, "vis_trace_sha256:aaaa1111");
});

test("a re-sealed trace does not reuse the previous archive's inspector", () => {
	// Same record id and same title, different sealed digest. Matching on the
	// record id would open the old archive under the new trace's name.
	const stale = inspectorVisual("sha256:bbbb2222", {
		traceId: "trace_alpha",
		metadata: { traceRecordId: "trace_alpha", traceDigest: "sha256:bbbb2222" }
	});
	assert.equal(findTraceInspectorVisual([stale], trace()), undefined);
});

test("a visual of another template is never treated as a trace inspector", () => {
	const other = inspectorVisual("sha256:aaaa1111", { templateId: "analysis.visual.v1" });
	assert.equal(traceDigestBinding(other), null);
	// metadata alone still matches: the binding check is the fallback, not the gate.
	const bare = { ...other, metadata: {} };
	assert.equal(findTraceInspectorVisual([bare], trace()), undefined);
});

test("inspector identity is derived from the digest and is stable", () => {
	const id = traceInspectorVisualId(trace());
	assert.equal(id, "vis_trace_aaaa1111");
	assert.equal(id, traceInspectorVisualId(trace({ title: "renamed", id: "trace_other" })));
});

test("identity falls back to the record id when a digest is unusable", () => {
	assert.equal(traceInspectorVisualId(trace({ digest: "sha256:" })), "vis_trace_trace_alpha");
});

test("the create request binds the projection slot to the sealed digest", () => {
	const request = traceInspectorCreateRequest(trace());
	const slot = request.bindings.slots[0];
	assert.equal(slot.kind, "trace_v5");
	assert.equal(slot.source, "sha256:aaaa1111");
	assert.equal(request.metadata.traceDigest, "sha256:aaaa1111");
	assert.equal(request.id, "vis_trace_aaaa1111");
});

test("each unavailable trace keeps an honest, visible label", () => {
	assert.deepEqual(traceInspectability(trace()), { eligible: true, label: "Inspect" });
	assert.deepEqual(
		traceInspectability(trace({ metadata: { quarantined: true } })),
		{ eligible: false, label: "Quarantined" }
	);
	assert.deepEqual(
		traceInspectability(trace({ metadata: { trusted: false } })),
		{ eligible: false, label: "Quarantined" }
	);
	assert.deepEqual(
		traceInspectability(trace({ metadata: { validationStatus: "INVALID" } })),
		{ eligible: false, label: "Quarantined" }
	);
	assert.deepEqual(
		traceInspectability(trace({ metadata: { selfContained: false } })),
		{ eligible: false, label: "Archive incomplete" }
	);
	assert.deepEqual(
		traceInspectability(trace({ metadata: { compatibilityLevel: "opaque" } })),
		{ eligible: false, label: "Unsupported" }
	);
});
