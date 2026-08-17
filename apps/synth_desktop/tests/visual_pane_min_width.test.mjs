import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

test("the visual pane keeps the 320px certification floor", () => {
  const css = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src/styles/app.css"),
    "utf8"
  );
  assert.match(css, /\.visual-pane\s*\{[^}]*min-width:\s*320px/s);
  assert.match(css, /minmax\(320px,\s*min\(var\(--visual-pane-width/);
});
