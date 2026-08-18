import assert from "node:assert/strict";
import test from "node:test";

import {
  consumeInjectedRendererCrash,
  resetInjectedRendererCrashes
} from "../runtime/crashInject.ts";

test("an injected crash throws once, then the same identity recovers on retry", () => {
  resetInjectedRendererCrashes();
  const visualId = "vis_recover";
  const revision = 7;
  assert.equal(consumeInjectedRendererCrash(visualId, revision, true), true);
  assert.equal(consumeInjectedRendererCrash(visualId, revision, true), false);
  assert.equal(consumeInjectedRendererCrash(visualId, revision, false), false);
});

test("a new revision can inject a crash without poisoning the recovered identity", () => {
  resetInjectedRendererCrashes();
  assert.equal(consumeInjectedRendererCrash("vis_a", 1, true), true);
  assert.equal(consumeInjectedRendererCrash("vis_a", 1, true), false);
  assert.equal(consumeInjectedRendererCrash("vis_a", 2, true), true);
  assert.equal(consumeInjectedRendererCrash("vis_b", 1, true), true);
});
