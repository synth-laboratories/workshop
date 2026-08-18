import assert from "node:assert/strict";
import test from "node:test";

import { presentRuntimeError, presentRuntimeErrorMessage } from "../runtime/presentError.ts";

test("a structured Tauri rejection never becomes [object Object]", () => {
  const presented = presentRuntimeError({
    code: "visual_observation_unavailable",
    message: "The pane has not published a rendered observation.",
    remediation: "Show the visual and wait for it to settle."
  });
  assert.equal(presented.code, "visual_observation_unavailable");
  assert.match(presented.message, /rendered observation/);
  const line = presentRuntimeErrorMessage({
    code: "visual_observation_unavailable",
    message: "The pane has not published a rendered observation."
  });
  assert.equal(line.includes("[object Object]"), false);
  assert.match(line, /visual_observation_unavailable/);
});

test("an object without a message field still names the code", () => {
  const presented = presentRuntimeError({ code: "visual_render_failed" });
  assert.equal(presented.code, "visual_render_failed");
  assert.equal(presented.message.includes("[object Object]"), false);
  assert.match(presented.message, /visual_render_failed/);
});

test("String(reason) of a plain object is never used as the message", () => {
  const reason = { nested: { foo: 1 } };
  assert.equal(String(reason), "[object Object]");
  assert.equal(presentRuntimeError(reason).message, "Visual runtime failed");
});
