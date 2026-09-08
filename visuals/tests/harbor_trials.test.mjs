import assert from "node:assert/strict";
import test from "node:test";
import { foldHarborTrials, harborSkillProgress } from "../runtime/harborTrials.ts";
const event = (kind, lane, payload = {}) => ({ kind, lane, run_id: "run", payload });

test("interleaved RuneBench completions settle their own rollout", () => {
  const rows = foldHarborTrials([
    event("trial.prepared", "a"), event("trial.prepared", "b"),
    event("env.episode.opened", "a", { task: "woodcutting" }),
    event("trial.completed", "a", { reward: 0 }), event("trial.failed", "b")
  ]);
  assert.equal(rows[0].status, "completed");
  assert.equal(rows[0].reward, 0);
  assert.equal(rows[0].instruction, "woodcutting");
  assert.equal(rows[0].verifierScript, undefined);
  assert.equal(rows[1].status, "failed");
  assert.equal(rows[1].reward, null);
});

test("explicit trial identity wins and ambiguous completion never selects the last trial", () => {
  const rows = foldHarborTrials([
    event("trial.planned", "lane", { trial_id: "a" }),
    event("trial.planned", "lane", { trial_id: "b" }),
    event("trial.completed", "lane", { reward: 99 }),
    event("trial.completed", "lane", { trial_id: "a", reward: 1 })
  ]);
  assert.equal(rows[0].reward, 1);
  assert.equal(rows[1].status, "planned");
  assert.equal(rows[1].reward, undefined);
});

test("verifier evidence remains authoritative across completion and duplicate prepare", () => {
  const rows = foldHarborTrials([
    event("verifier", "a", { "reward.txt": 0, script: "declared verifier" }),
    event("trial.completed", "a", { reward: 9 }), event("trial.prepared", "a")
  ]);
  assert.equal(rows[0].status, "verified");
  assert.equal(rows[0].reward, 0);
  assert.equal(rows[0].verifierScript, "declared verifier");
  assert.deepEqual(foldHarborTrials([event("trial.completed", null, { reward: 9 })]), []);
});

test("skill progress keeps interleaved rollouts separate and missing rate unknown", () => {
  const rows = harborSkillProgress([
    event("game.skill_sample", "a", { skill: "woodcutting", xp: 10 }),
    event("game.skill_sample", "b", { skill: "woodcutting", xp: 200, xp_per_min: 0 }),
    event("game.skill_sample", "a", { skill: "woodcutting", xp: 15, xp_per_min: 5 }),
    event("game.skill_sample", "c", { skill: "mining", xp: 20 })
  ]);
  assert.deepEqual(rows.map(({ xp, xpPerMin, samples }) => [xp, xpPerMin, samples]), [[15, 5, 2], [200, 0, 1], [20, null, 1]]);
});
