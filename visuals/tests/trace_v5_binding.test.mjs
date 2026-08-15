import assert from "node:assert/strict";
import test from "node:test";

import { bindTemplateSlots } from "../runtime/bind.ts";

const template = {
  id: "trace.rollout_inspector.v1",
  slots: [{
    name: "projection",
    description: "Read-only rollout inspection projection",
    accepts: ["inline", "trace_v5"],
    required: true,
    schema: "synth.trace-projection.rollout-inspector.v1",
  }],
};

test("trace_v5 invokes the injected resolver and fills its declared slot", async () => {
  const calls = [];
  const payload = { schema_version: "synth.trace-projection.rollout-inspector.v1", visual: { items: [] } };
  const result = await bindTemplateSlots(template, [{
    slot: "projection",
    kind: "trace_v5",
    source: "sha256:sealed",
  }], {
    async loadTraceV5(digest) {
      calls.push(digest);
      return payload;
    },
  });

  assert.deepEqual(calls, ["sha256:sealed"]);
  assert.deepEqual(result.errors, []);
  assert.equal(result.slots.projection.data, payload);
  assert.equal(result.slots.projection.source, "sha256:sealed");
});

test("trace_v5 fails explicitly when its archive identity or resolver is missing", async () => {
  const missingDigest = await bindTemplateSlots(template, [{ slot: "projection", kind: "trace_v5" }]);
  assert.match(missingDigest.errors.join(" "), /requires a sealed trace digest/i);

  const missingResolver = await bindTemplateSlots(template, [{
    slot: "projection",
    kind: "trace_v5",
    source: "sha256:sealed",
  }]);
  assert.match(missingResolver.errors.join(" "), /No Trace V5 loader/);
});
