import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const host = readFileSync(join(root, "src/renderer/src/components/VisualHost.tsx"), "utf8");

test("visual readiness receipts publish only when semantic facts change", () => {
  const boundary = host.slice(
    host.indexOf("function VisualObservationBoundary"),
    host.indexOf("const openReference", host.indexOf("function VisualObservationBoundary"))
  );
  assert.match(boundary, /lastPublishedObservation = useRef<string \| null>\(null\)/);
  assert.match(boundary, /if \(lastPublishedObservation\.current === observationKey\) return/);
  assert.match(boundary, /lastPublishedObservation\.current = observationKey;\s*void visualBridge\.reportObservation\(observation\)/);
  const keyBody = boundary.slice(boundary.indexOf("const observationKey"), boundary.indexOf("if (lastPublishedObservation.current"));
  assert.equal(keyBody.includes("observedAt"), false);
});
