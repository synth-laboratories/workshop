/**
 * The binding envelope is the contract between what a writer persists and what
 * the renderer can read. It is tested here as behaviour, not as source text.
 *
 * Goal: no input reaches the renderer as "zero slots, no error". A shape that
 * cannot be read is a rejection with a message; a legacy shape is upgraded and
 * says so. The v0.4 acceptance run failed because neither was true — ten
 * correct live-stream descriptors, filed under a slot key, resolved to an empty
 * list and an empty pane.
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

const { resolveVisualBindings, propsFromBindings, bindTemplateSlots } = await import(
  "../runtime/bind.ts"
);
const { replayStreamsFromBindings } = await import("../runtime/replayClient.ts");

/** The exact bindings persisted by the failing v0.4 CUA acceptance run. */
function incidentBindings() {
  return {
    stream: Array.from({ length: 10 }, (_, index) => ({
      slot: "stream",
      kind: "live_sse",
      source: `http://127.0.0.1:8114/rollouts/roll_${index}/stream`,
      poll_url: `http://127.0.0.1:8114/rollouts/roll_${index}/events`,
      schema: "synth.trace-stream-event.v1"
    }))
  };
}

test("the canonical envelope resolves unchanged", () => {
  const resolved = resolveVisualBindings({
    schemaVersion: "synth.visual-bindings.v1",
    slots: [{ slot: "stream", kind: "live_sse", source: "http://127.0.0.1:8114/rollouts/r1/stream" }]
  });
  assert.equal(resolved.status, "canonical");
  assert.equal(resolved.slots.length, 1);
  assert.equal(resolved.error, null);
});

test("the slot-keyed map that rendered nothing now resolves to ten live streams", () => {
  const resolved = resolveVisualBindings(incidentBindings());

  assert.equal(resolved.status, "upgraded");
  assert.equal(resolved.slots.length, 10);
  assert.deepEqual(resolved.upgradedSlots, ["stream"]);
  assert.ok(resolved.slots.every((slot) => slot.slot === "stream" && slot.kind === "live_sse"));

  // And the transport the renderer would actually open.
  const { streams, missingTransport } = replayStreamsFromBindings(resolved.slots);
  assert.equal(streams.length, 10);
  assert.equal(missingTransport.length, 0);
});

test("the slot key is authoritative over a descriptor's own claim", () => {
  const resolved = resolveVisualBindings({
    stream: { slot: "somewhere_else", kind: "live_sse", source: "http://127.0.0.1:8114/rollouts/r1/stream" }
  });
  assert.equal(resolved.slots[0].slot, "stream");
});

test("a legacy prop bag becomes inline slots and keeps its data", () => {
  const resolved = resolveVisualBindings({ matrix: [1, 2, 3], title: "x" });
  assert.equal(resolved.status, "upgraded");
  assert.equal(resolved.slots.length, 2);
  assert.ok(resolved.slots.every((slot) => slot.kind === "inline"));
  assert.deepEqual(resolved.slots.find((slot) => slot.slot === "matrix").data, [1, 2, 3]);
});

test("inline chart data carrying a kind field stays inline data", () => {
  // `{kind: "bar"}` is a chart spec, not a transport. Reinterpreting it would
  // be exactly the kind of silent guess this contract exists to prevent.
  const resolved = resolveVisualBindings({ chart: { kind: "bar", series: [] } });
  assert.equal(resolved.slots[0].kind, "inline");
});

test("unreadable bindings are rejected with a reason, never emptied", () => {
  for (const [label, value] of [
    ["mixed descriptors and data", {
      stream: { kind: "live_sse", source: "http://127.0.0.1:8114/rollouts/r1/stream" },
      notes: [1, 2, 3]
    }],
    ["a future schema version", { schemaVersion: "synth.visual-bindings.v2", slots: [] }],
    ["a scalar", 7],
    ["null", null]
  ]) {
    const resolved = resolveVisualBindings(value);
    assert.equal(resolved.status, "rejected", `${label} must be rejected`);
    assert.equal(resolved.slots.length, 0);
    assert.ok(resolved.error && resolved.error.length > 0, `${label} must carry a reason`);
  }
});

test("empty bindings are canonical, not an upgrade and not an error", () => {
  const resolved = resolveVisualBindings({});
  assert.equal(resolved.status, "canonical");
  assert.equal(resolved.slots.length, 0);
  assert.equal(resolved.error, null);
});

test("propsFromBindings surfaces a rejection instead of passing the raw object through", () => {
  const rejected = propsFromBindings({
    stream: { kind: "live_sse", source: "http://127.0.0.1:8114/rollouts/r1/stream" },
    notes: [1, 2]
  });
  assert.equal(rejected.errors.length, 1);
  assert.deepEqual(rejected.props, {});

  // The old behaviour: the raw map became the prop bag, so `props.stream` was
  // an array of binding descriptors that every consumer then coerced to {}.
  const upgraded = propsFromBindings(incidentBindings());
  assert.equal(upgraded.errors.length, 0);
  assert.ok(!Array.isArray(upgraded.props.stream));
  assert.equal(typeof upgraded.props.stream.sse_url, "string");
});

test("bindTemplateSlots refuses unreadable bindings rather than reporting a missing slot", async () => {
  const template = {
    id: "live.craftax.v1",
    slots: [{ name: "stream", accepts: ["live_sse"], required: true, multiple: true }]
  };
  const rejected = await bindTemplateSlots(template, {
    stream: { kind: "live_sse", source: "http://127.0.0.1:8114/rollouts/r1/stream" },
    notes: [1]
  });
  assert.match(rejected.errors[0], /mix descriptors and inline data|unreadable/);

  const upgraded = await bindTemplateSlots(template, incidentBindings());
  assert.equal(upgraded.errors.length, 0);
  assert.equal(upgraded.slots.stream.data.length, 10);
});

test("the Rust and TypeScript readers share one binding vocabulary", () => {
  // Two implementations decide this shape: Rust writes it, TypeScript renders
  // it. A kind accepted by one and not the other is a visual that persists and
  // then cannot be read — the failure mode in miniature.
  const rust = readFileSync(join(root, "../apps/synth_desktop/src-tauri/src/visuals/models.rs"), "utf8");
  const typescript = readFileSync(join(root, "runtime/bind.ts"), "utf8");
  const start = rust.indexOf("pub const VISUAL_BINDING_KINDS");
  assert.ok(start >= 0, "expected VISUAL_BINDING_KINDS in models.rs");
  const declaration = rust.slice(start, rust.indexOf("];", start));
  const kinds = (declaration.match(/"[a-z0-9_]+"/g) ?? []).map((quoted) => quoted.replaceAll('"', ""));
  assert.ok(kinds.length >= 8, "expected the Rust binding vocabulary");
  for (const kind of kinds) {
    assert.ok(typescript.includes(`"${kind}"`), `TypeScript must accept binding kind ${kind}`);
  }
});
