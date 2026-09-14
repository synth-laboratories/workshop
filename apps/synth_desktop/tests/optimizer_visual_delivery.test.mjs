import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

test("native optimizer reopen broadcasts its committed presentation event", () => {
  const source = readFileSync(new URL("../src-tauri/src/visuals_ipc.rs", import.meta.url), "utf8");
  const start = source.indexOf('path.ends_with("/open_visual")');
  const route = source.slice(start, source.indexOf('path.ends_with("/refresh")', start));
  assert.match(route, /open_visual_in_session\(id\.to_string\(\), session_ref\)/);
  assert.match(route, /core\.broadcast_committed\(event\.clone\(\)\);\s*Ok\(json!/);
});
