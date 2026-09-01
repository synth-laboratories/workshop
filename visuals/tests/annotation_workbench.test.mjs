import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { bindTemplateSlots } from "../runtime/bind.ts";
import { rubricCriterionView, rubricScore } from "../families/analysis/analysis.annotation_workbench.v1/rubric.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const template = JSON.parse(
  readFileSync(join(root, "families/analysis/analysis.annotation_workbench.v1/template.json"), "utf8")
);
const craftax = JSON.parse(readFileSync(join(root, "fixtures/annotation_workbench_craftax.json"), "utf8"));

test("annotation workbench binds a fixture evidence projection", async () => {
  const result = await bindTemplateSlots(template, [{
    input: "evidence",
    kind: "fixture",
    source: "fixtures/annotation_workbench_craftax.json"
  }], {
    async loadFixture() {
      return craftax;
    }
  });
  assert.deepEqual(result.errors, []);
  assert.equal(result.slots.evidence.data.schemaVersion, "synth.annotation-workbench.v1");
  assert.equal(result.slots.evidence.data.campaign.title, "Craftax · GLM failure analysis");
  assert.equal(result.slots.evidence.data.rubric.available, false);
});

test("annotation_evidence_head and verifier_result_v2 are distinct loaders", async () => {
  const calls = [];
  const result = await bindTemplateSlots(template, [
    { input: "evidence", kind: "annotation_evidence_head", source: "sha256:head" },
    { input: "rubric", kind: "verifier_result_v2", source: "sha256:rubric" }
  ], {
    async loadAnnotationEvidenceHead(source) {
      calls.push(["evidence", source]);
      return craftax;
    },
    async loadVerifierResult(source) {
      calls.push(["rubric", source]);
      return { available: true, criteria: [{ id: "grounding", label: "Grounding", judgment: "pass" }] };
    }
  });
  assert.deepEqual(result.errors, []);
  assert.deepEqual(calls, [
    ["evidence", "sha256:head"],
    ["rubric", "sha256:rubric"]
  ]);
});

test("a workbench without a verifier result still binds and does not invent a score", async () => {
  const result = await bindTemplateSlots(template, [{
    input: "evidence",
    kind: "inline",
    data: craftax
  }]);
  assert.deepEqual(result.errors, []);
  assert.equal(result.slots.rubric, undefined);
  assert.equal(result.slots.evidence.data.rubric.available, false);
  assert.equal(result.slots.evidence.data.rubric.digest, null);
});

test("sealed verifier snake-case criteria render human labels and judgments", () => {
  assert.deepEqual(rubricCriterionView({
    criterion_id: "state_grounding",
    verdict: "pass",
    rationale: "The trace supports the judgment.",
    score: 2
  }, 0), {
    id: "state_grounding",
    label: "State grounding",
    judgment: "pass",
    rationale: "The trace supports the judgment.",
    score: 2
  });
  assert.equal(rubricScore(0.4722222222), "47.2%");
});
