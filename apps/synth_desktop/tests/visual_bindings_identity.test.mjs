import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const outfile = join(compiledDir, "visualBindingsIdentity.mjs");
buildSync({
	entryPoints: [join(appRoot, "src/renderer/src/runtime/visualBindings.ts")],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "node",
	outfile
});
const { optimizerRunIdFromBindings, traceIdFromBindings, traceSetCountFromBindings } = await import(pathToFileURL(outfile).href);

test("registry identities come from canonical visual bindings", () => {
	const bindings = {
		inputs: [
			{ input: "optimizer_run", kind: "optimizer_run", source: "opt_eval_craftax_2c75455d64c9" },
			{ input: "trace", kind: "trace_v5", source: "tracev5_aff8dbd6dee4809ad4de8860" }
		]
	};
	assert.equal(optimizerRunIdFromBindings(bindings), "opt_eval_craftax_2c75455d64c9");
	assert.equal(traceIdFromBindings(bindings), "tracev5_aff8dbd6dee4809ad4de8860");
});

test("conflicting binding identities fail closed", () => {
	assert.equal(optimizerRunIdFromBindings({ inputs: [
		{ kind: "optimizer_run", source: "run-a" },
		{ kind: "optimizer_run", source: "run-b" }
	] }), undefined);
	assert.equal(traceIdFromBindings({ inputs: [
		{ kind: "trace_v5", source: "trace-a" },
		{ kind: "trace_v5", source: "trace-b" }
	] }), undefined);
	assert.equal(traceSetCountFromBindings({ inputs: [
		{ kind: "trace_v5", source: "trace-a" },
		{ kind: "trace_v5", source: "trace-b" }
	] }), 2);
});

test("optimizer overview bindings report a trace set without inventing a primary trace", () => {
	const bindings = {
		inputs: [
			{
				input: "experiment",
				kind: "inline",
				data: { aggregate: { traceCount: 5 } }
			},
			{ input: "optimizer_run", kind: "optimizer_run", source: "opt_eval_craftax" }
		]
	};
	assert.equal(traceIdFromBindings(bindings), undefined);
	assert.equal(traceSetCountFromBindings(bindings), 5);
});
