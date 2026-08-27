import assert from "node:assert/strict";
import test from "node:test";

import { hydrateVisualRecord } from "../src/renderer/src/runtime/visualHydration.ts";

test("direct library open restores the authoritative saved binding envelope", async () => {
	const listed = {
		id: "vis_trace",
		bindings: {},
	};
	const saved = {
		id: "vis_trace",
		bindings: {
			schemaVersion: "synth.visual-bindings.v1",
			inputs: [{ input: "optimizer_run", kind: "optimizer_run", source: "opt_eval_1" }],
		},
	};
	const requested = [];

	const hydrated = await hydrateVisualRecord(listed, async (visualId) => {
		requested.push(visualId);
		return saved;
	});

	assert.deepEqual(requested, ["vis_trace"]);
	assert.equal(hydrated.bindings.inputs[0].source, "opt_eval_1");
});

test("direct library hydration rejects a mismatched registry identity", async () => {
	await assert.rejects(
		() => hydrateVisualRecord({ id: "vis_trace" }, async () => ({ id: "vis_other" })),
		/returned vis_other for vis_trace/
	);
});
