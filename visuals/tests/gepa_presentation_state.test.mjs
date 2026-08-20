import assert from "node:assert/strict";
import test from "node:test";
import {
  DEFAULT_GEPA_PRESENTATION_STATE,
  frontierCreditByCandidate,
  loadGepaPresentationState,
  normalizeGepaPresentationState,
  presentationStorageKey,
  resolvedSelection,
  saveGepaPresentationState,
  visibleCandidates
} from "../families/optimizers/_shared/optimizer.run.v1/overlays/gepa/presentationState.ts";

const gepa = {
  candidates: [
    { id: "seed", source: "seed", sequence: 1, status: "accepted", train_reward: 0.5, prompt: "baseline banking classifier" },
    { id: "proposal-10", parentId: "seed", generation: 1, sequence: 10, status: "rejected_minibatch", prompt: "security-first rewrite", decision: { outcome: "rejected" } },
    { id: "proposal-2", parentId: "seed", generation: 0, sequence: 2, status: "accepted", train_reward: 0.8, prompt: "transfer intent rewrite", decision: { outcome: "accepted" } }
  ],
  frontier: [{ candidateId: "seed" }, { candidateId: "proposal-2" }],
  evaluations: [
    { candidateId: "seed", exampleId: "a", stage: "seed_full_train", reward: 1, ref: { id: "eval-seed-a" } },
    { candidateId: "proposal-2", exampleId: "a", stage: "candidate_full_train", reward: 1, ref: { id: "eval-p2-a" } },
    { candidateId: "seed", exampleId: "b", stage: "seed_full_train", reward: 0, ref: { id: "eval-seed-b" } },
    { candidateId: "proposal-2", exampleId: "b", stage: "candidate_full_train", reward: 1, ref: { id: "eval-p2-b" } }
  ]
};

test("GEPA presentation state is versioned, run-scoped, and rejects invalid values", () => {
  const state = normalizeGepaPresentationState({
    query: "transfer",
    decision: "bogus",
    sort: "score",
    direction: "desc",
    selection: { runId: "wrong-run", kind: "candidate", id: "proposal-2", candidateId: "proposal-2" }
  }, "banking77");
  assert.equal(state.version, 1);
  assert.equal(state.decision, "all");
  assert.equal(state.sort, "score");
  assert.equal(state.selection.runId, "banking77");
  assert.equal(presentationStorageKey("banking77"), "synth.optimizer.gepa.presentation.v1:banking77");
});

test("candidate filtering and sorting are deterministic with missing values last", () => {
  const byScore = visibleCandidates(gepa, { ...DEFAULT_GEPA_PRESENTATION_STATE, sort: "score", direction: "desc" });
  assert.deepEqual(byScore.map((candidate) => candidate.id), ["proposal-2", "seed", "proposal-10"]);
  const filtered = visibleCandidates(gepa, { ...DEFAULT_GEPA_PRESENTATION_STATE, query: "security", decision: "rejected" });
  assert.deepEqual(filtered.map((candidate) => candidate.id), ["proposal-10"]);
  const frontier = visibleCandidates(gepa, { ...DEFAULT_GEPA_PRESENTATION_STATE, frontierOnly: true });
  assert.deepEqual(frontier.map((candidate) => candidate.id), ["seed", "proposal-2"]);
  assert.deepEqual(Object.fromEntries(frontierCreditByCandidate(gepa)), { seed: 1, "proposal-2": 2 });
});

test("local state round-trips without persisting fetched evidence", () => {
  const values = new Map();
  const storage = {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value)
  };
  const state = {
    ...DEFAULT_GEPA_PRESENTATION_STATE,
    query: "transfer",
    selection: { runId: "banking77", kind: "evaluation", id: "eval-p2-a", candidateId: "proposal-2", sequenceNumber: 7 }
  };
  saveGepaPresentationState("banking77", state, storage);
  const serialized = values.get(presentationStorageKey("banking77"));
  assert.doesNotMatch(serialized, /evaluations|candidates|reward/);
  assert.deepEqual(loadGepaPresentationState("banking77", storage), state);
  assert.equal(resolvedSelection(state.selection, gepa)?.id, "eval-p2-a");
  assert.equal(resolvedSelection({ runId: "banking77", kind: "evaluation", id: "missing" }, gepa), null);
});
