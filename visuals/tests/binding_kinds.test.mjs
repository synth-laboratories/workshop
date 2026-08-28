/**
 * Every advertised visual binding kind must resolve through bindTemplateSlots.
 * Advertising a kind the host cannot load is how Trace inspector `local_cas`
 * bindings became guaranteed capture failures.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { bindTemplateSlots } from "../runtime/bind.ts";

const ADVERTISED = [
  "inline",
  "trace_v5",
  "local_cas",
  "run_ref",
  "live_sse",
  "fixture",
  "optimizer_run",
  "optimizer_snapshot",
  "query_snapshot"
];

function templateFor(kind) {
  return {
    id: `binding.${kind}.v1`,
    slots: [{
      name: "payload",
      description: kind,
      accepts: [kind],
      required: true
    }]
  };
}

test("every advertised binding kind is exercised by bindTemplateSlots", async () => {
  const loaders = {
    async loadFixture(source) {
      assert.equal(source, "fixtures/demo.json");
      return { from: "fixture" };
    },
    async loadTraceV5(source) {
      assert.equal(source, "sha256:trace");
      return { from: "trace_v5" };
    },
    async loadLocalCas(source) {
      assert.equal(source, "sha256:cas");
      return { from: "local_cas" };
    },
    async loadQuerySnapshot(source) {
      assert.equal(source, "snap_1");
      return { from: "query_snapshot" };
    },
    async loadRun(source) {
      assert.equal(source, "run_1");
      return { from: "run_ref" };
    },
    async loadOptimizerRun(source) {
      assert.equal(source, "opt_1");
      return { from: "optimizer_run" };
    },
    async loadOptimizerSnapshot(source) {
      assert.equal(source, "optsnap_1");
      return { from: "optimizer_snapshot" };
    }
  };

  const cases = {
    inline: { kind: "inline", data: { from: "inline" } },
    fixture: { kind: "fixture", source: "fixtures/demo.json" },
    trace_v5: { kind: "trace_v5", source: "sha256:trace" },
    local_cas: { kind: "local_cas", source: "sha256:cas" },
    query_snapshot: { kind: "query_snapshot", source: "snap_1" },
    run_ref: { kind: "run_ref", source: "run_1" },
    optimizer_run: { kind: "optimizer_run", source: "opt_1" },
    optimizer_snapshot: { kind: "optimizer_snapshot", source: "optsnap_1" },
    live_sse: {
      kind: "live_sse",
      source: "http://127.0.0.1:8098/rollouts/r1/stream",
      poll_url: "http://127.0.0.1:8098/rollouts/r1/events"
    }
  };

  assert.deepEqual(Object.keys(cases).sort(), [...ADVERTISED].sort());

  for (const kind of ADVERTISED) {
    const result = await bindTemplateSlots(
      templateFor(kind),
      [{ slot: "payload", ...cases[kind] }],
      loaders
    );
    assert.deepEqual(result.errors, [], `${kind} should resolve`);
    assert.ok(result.slots.payload, `${kind} should fill its slot`);
    if (kind === "live_sse") {
      assert.equal(result.slots.payload.data.sse_url, cases.live_sse.source);
    } else {
      assert.equal(result.slots.payload.data.from, kind);
    }
  }
});

test("trace_v5 and local_cas are distinct loaders, never aliases", async () => {
  const calls = [];
  const template = {
    id: "analysis.trace.v1",
    slots: [
      { name: "projection", accepts: ["trace_v5"], required: true },
      { name: "matrix", accepts: ["local_cas"], required: true }
    ]
  };
  const result = await bindTemplateSlots(template, [
    { slot: "projection", kind: "trace_v5", source: "sha256:trace" },
    { slot: "matrix", kind: "local_cas", source: "sha256:cas" }
  ], {
    async loadTraceV5(source) {
      calls.push(["trace_v5", source]);
      return { kind: "trace" };
    },
    async loadLocalCas(source) {
      calls.push(["local_cas", source]);
      return { kind: "cas" };
    }
  });
  assert.deepEqual(result.errors, []);
  assert.deepEqual(calls, [
    ["trace_v5", "sha256:trace"],
    ["local_cas", "sha256:cas"]
  ]);
  assert.equal(result.slots.projection.data.kind, "trace");
  assert.equal(result.slots.matrix.data.kind, "cas");
});

test("an advertised kind without its loader fails explicitly", async () => {
  const missing = await bindTemplateSlots(templateFor("local_cas"), [{
    slot: "payload",
    kind: "local_cas",
    source: "sha256:cas"
  }]);
  assert.match(missing.errors.join(" "), /No local CAS loader/);
});
