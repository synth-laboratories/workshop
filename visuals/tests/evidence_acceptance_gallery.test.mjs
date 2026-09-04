import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (path) => readFileSync(join(root, path), "utf8");

test("the internal acceptance gallery exercises every shared evidence primitive", () => {
  const shell = read("templates-internal/evidence.acceptance.v1/shell.tsx");
  for (const primitive of [
    "EvidenceStateBanner",
    "ComparisonScopeNotice",
    "MetricValue",
    "ProvenanceHeader",
    "ReadinessBadge"
  ]) assert.match(shell, new RegExp(primitive));
  for (const state of ["live", "terminal", "partial", "stale", "unavailable"])
    assert.match(shell, new RegExp(`state: "${state}"`));
  assert.match(shell, /Claimed viewport differs from the native capture receipt/);
});

test("comparison scope is not presented as an evidence-completeness state", () => {
  const primitives = read("chrome/EvidencePrimitives.tsx");
  const comparison = primitives.slice(
    primitives.indexOf("export function ComparisonScopeNotice"),
    primitives.indexOf("export function ProvenanceHeader")
  );
  assert.match(comparison, /data-comparison-scope/);
  assert.doesNotMatch(comparison, /EvidenceStateBanner/);
  assert.doesNotMatch(comparison, /state=.*terminal/);
});

test("reward completeness requires an explicit terminal assertion", () => {
  const shell = read("families/analysis/reward.breakdown.v1/shell.tsx");
  assert.match(shell, /evidence_basis\.terminal === true \? "terminal" : "partial"/);
  assert.match(shell, /source closure was not asserted/);
});
