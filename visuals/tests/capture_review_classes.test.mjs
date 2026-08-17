/**
 * Capture/review must work for every supported product visual class.
 * Templates that declare an observation contract certify from that contract;
 * templates that do not certify from the screenshot.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { bindTemplateSlots } from "../runtime/bind.ts";
import {
  CAPTURE_REVIEW_PRODUCT_CLASSES,
  captureEvidenceKind
} from "../runtime/captureEvidence.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function loadTemplate(id) {
  const relatives = {
    "optimizer.gepa.live.v1": "families/optimizers/gepa/optimizer.gepa.live.v1/template.json",
    "trace.rollout_inspector.v1": "families/analysis/trace.rollout_inspector.v1/template.json",
    "live.craftax.v1": "families/first_class_example_containers/live.craftax.v1/template.json",
    "optimizer.eval.live.v1": "families/optimizers/eval/optimizer.eval.live.v1/template.json",
    "optimizer.sft.live.v1": "families/optimizers/sft/optimizer.sft.live.v1/template.json",
    "diagram.mermaid.v1": "families/diagrams/diagram.mermaid.v1/template.json"
  };
  return JSON.parse(readFileSync(join(root, relatives[id]), "utf8"));
}

test("GEPA, Trace inspector, Craftax, eval, and SFT declare truthful observation contracts", () => {
  for (const id of CAPTURE_REVIEW_PRODUCT_CLASSES) {
    const template = loadTemplate(id);
    assert.equal(template.id, id);
    assert.equal(captureEvidenceKind(template), "observation", `${id} must declare an observation contract`);
    assert.equal(template.observationContract.schemaVersion, "synth.visual-observation-contract.v1");
  }
});

test("a template without a contract certifies from the screenshot, not a missing observation", () => {
  const mermaid = loadTemplate("diagram.mermaid.v1");
  assert.equal(captureEvidenceKind(mermaid), "screenshot");
  assert.equal(mermaid.observationContract, undefined);
});

test("product class fixtures bind without unresolved slots", async () => {
  const cases = [
    {
      id: "optimizer.gepa.live.v1",
      binding: { slot: "optimizer_run", kind: "inline", data: { from: "gepa", events: [{ kind: "run.started" }] } }
    },
    {
      id: "optimizer.sft.live.v1",
      binding: { slot: "optimizer_run", kind: "inline", data: { from: "sft", events: [{ kind: "train.step" }] } }
    },
    {
      id: "optimizer.eval.live.v1",
      binding: { slot: "optimizer_run", kind: "inline", data: { from: "eval", events: [{ kind: "trial.scored" }] } }
    },
    {
      id: "live.craftax.v1",
      binding: {
        slot: "stream",
        kind: "inline",
        data: { events: [{ kind: "observation", payload: { text: "You see a tree." } }] }
      }
    },
    {
      id: "trace.rollout_inspector.v1",
      binding: { slot: "projection", kind: "inline", data: { from: "trace_v5", events: [{ kind: "span.opened" }] } }
    }
  ];
  for (const fixture of cases) {
    const template = loadTemplate(fixture.id);
    const result = await bindTemplateSlots(template, [fixture.binding]);
    assert.deepEqual(result.errors, [], `${fixture.id} fixture should bind`);
    assert.ok(result.slots[fixture.binding.slot], `${fixture.id} should fill its slot`);
  }
});
