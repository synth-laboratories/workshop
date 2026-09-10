import assert from "node:assert/strict";
import test from "node:test";

import { comparisonContractVerdict } from "../runtime/evidenceGrammar.ts";

const contract = { digest: "sha256:one", benchmark: "craftax", metric: "mean_reward" };

test("like-for-like ranking requires a declared comparison contract", () => {
  assert.deepEqual(
    comparisonContractVerdict("like_for_like", undefined, [{ comparison_contract_digest: "sha256:one" }], "mean_reward"),
    { comparable: false, reason: "missing_contract" }
  );
});

test("every ranked row must bind the exact same comparison contract", () => {
  assert.deepEqual(
    comparisonContractVerdict("like_for_like", contract, [
      { comparison_contract_digest: "sha256:one" },
      { comparison_contract_digest: "sha256:two" }
    ], "mean_reward"),
    { comparable: false, reason: "row_contract_mismatch" }
  );
});

test("matching contract identities unlock ranking", () => {
  assert.deepEqual(
    comparisonContractVerdict("like_for_like", contract, [
      { comparison_contract_digest: "sha256:one" },
      { comparison_contract_digest: "sha256:one" }
    ], "mean_reward"),
    { comparable: true, reason: null }
  );
});

test("the ranked metric must be the metric named by the shared contract", () => {
  assert.deepEqual(
    comparisonContractVerdict("like_for_like", contract, [
      { comparison_contract_digest: "sha256:one" },
      { comparison_contract_digest: "sha256:one" }
    ], "cost_usd"),
    { comparable: false, reason: "metric_mismatch" }
  );
});

test("a run catalog remains descriptive even if rows happen to share a digest", () => {
  assert.equal(
    comparisonContractVerdict("run_catalog", contract, [{ comparison_contract_digest: "sha256:one" }]).comparable,
    false
  );
});
