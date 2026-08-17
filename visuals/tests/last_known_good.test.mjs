import assert from "node:assert/strict";
import test from "node:test";

import { rememberLastKnownGood, selectRenderedProjection } from "../runtime/lastKnownGood.ts";

test("a live failure keeps the last successful projection at the same identity", () => {
  const good = { revision: 4, events: [1, 2, 3] };
  const selected = selectRenderedProjection({
    live: null,
    lastKnownGood: good,
    liveFailed: true
  });
  assert.equal(selected.source, "lastKnownGood");
  assert.equal(selected.stale, true);
  assert.equal(selected.projection, good);
});

test("a successful live projection replaces last-known-good", () => {
  const previous = { revision: 3, events: [1] };
  const live = { revision: 4, events: [1, 2] };
  const remembered = rememberLastKnownGood(previous, live, false);
  assert.equal(remembered, live);
  const selected = selectRenderedProjection({ live, lastKnownGood: previous, liveFailed: false });
  assert.equal(selected.source, "live");
  assert.equal(selected.stale, false);
});

test("a failed live value never overwrites last-known-good", () => {
  const previous = { revision: 4, events: [1, 2] };
  assert.equal(rememberLastKnownGood(previous, { revision: 4, events: [] }, true), previous);
  assert.equal(rememberLastKnownGood(null, null, true), null);
});
