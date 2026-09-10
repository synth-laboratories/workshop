# Handoff: durable Candidate on the experiment spine

**For:** the engineer taking the next cut **besides visuals**.  
**Date:** 2026-08-26  
**Do not commit unless asked.** Parallel track: sourced/compose CUA lives in [`HANDOFF_SOURCED_VISUALS_CUA_2026-08-26.md`](./HANDOFF_SOURCED_VISUALS_CUA_2026-08-26.md) — do not mix.

Noun map (keep current): [`docs/qa/v08-visuals-data-model.md`](./qa/v08-visuals-data-model.md)

Durable authority is the **local store** (sqlite + journal + CAS). Do not introduce `CoreRuntime` as a product noun.

---

## What you are adding

**Candidate** as a durable class on the experiment spine.

Today a GEPA `candidate_id` exists only inside optimizer event JSON and private visual overlays. After this cut, the same identity is a sqlite row you can list from an experiment / optimizer_run, show in the Experiments inspector, and reopen after restart.

It is **not**:

- a fourth `experiment_nodes.kind` (`optimizer_run` | `eval_campaign` | `direct_evaluation` stay the only written member kinds)
- `eval_candidates` / `optimizer.policy-candidate-set.v1` (workspace files staged for eval recipes — different class; leave it)
- compare / promote
- writers for `forked_from` / `rerun_of`
- compose `optimizer_run` slot, sourced TSX, or any visuals family work
- a replacement for container `POST /candidates`

GEPA custom apply stays container-owned. Workshop **projects** producer identity; it does not re-apply, re-compile, or restart per seed.

---

## Tree

| Path | Branch | Git |
| --- | --- | --- |
| `/Users/joshuapurtell/GitHub/workshop-v08-release` | `codex/v08-release-integration` | **Dirty.** Lineage C1/C2/W1 is already in the working copy (uncommitted), mixed with sourced-visuals WIP. |

Leave `containers`, `optimizers`, `optimizers-beta`, `synth-mlx-rl` alone unless a producer field is missing — then say so; do not invent a fixture world.

File work only under `/Users/joshuapurtell/GitHub`.

**Stay out of visuals files.** Do not edit `visuals/`, `VisualHost.tsx`, `visuals/sourced.rs`, compose templates, `use-synth-visuals`, or the sourced CUA handoff. If you must touch a shared file (`migrations.rs`, `specta.rs`, `lib.rs`, `data.rs`, `protocol.ts`), keep the diff to Candidate and tell the visuals track.

---

## What already landed (do not rebuild)

Experiment is local-store rows. Renderer canvas is a projection.

```
session ──many──► ExperimentGroup (exp_…)
                    ├─ members   optimizer_run | eval_campaign | direct_evaluation
                    ├─ nodes     one row per member
                    ├─ edges     evaluated  (member → member)
                    └─ lineage   follow_up  (group → child group)
```

- Attach: `experiment_session_cursor` → `sessions.active_experiment_id` → oldest group. Do not attach when `session_ref` is empty.
- Optimizer `experiment_bind`: attach on create/seed/import; settle on terminal commit / cancel / stale reconcile. `optimizer_relationships.started_from` is not an experiment edge.
- Surfaces: `experiments_create_child` / `experiments_activate`. Forest when nothing selected; member DAG when selected. Layout coords are view state, not sqlite.
- Historical `baseline` | `variant` | `result` | `run` node rows stay readable; nothing new writes them.

Code: `apps/synth_desktop/src-tauri/src/experiments/`, `…/lineage/`, `…/optimizers/experiment_bind.rs`, `src/renderer/src/experiments/`, `src/renderer/src/lineage/`. Latest schema: **migration 38**.

Inspector today (`NodeInspector.tsx`) shows kind / status / metrics / evidence. It has no Candidate list.

---

## Shape of this cut

```
ExperimentGroup
  └── ExperimentNode kind=optimizer_run   (unchanged member)
        └── CandidateRecord[]             NEW durable class
              producer_candidate_id       GEPA / overlay id (not invented)
              kind                        load/run contract
              protocol_id                 bind/mutation dialect
              parent_ids[]                producer parent_candidate_ids
              metrics / status            folded from events
              content_digest?             CAS pointer if body is stored
              optimizer_run_id            member_id of the run
              experiment_id               group FK
```

Kind is the load/run contract (`policy_script`, `sourced_python`, `harness_module`, prompt overlay). `protocol_id` is the mutation dialect (`whole_file.v1`, `unified_diff.v1`, `harness_restart.v1`, `prompt_overlay.v1`). Do not add language-named kinds.

Register-then-run stays the container contract: `POST /candidates` then `POST /rollout` with `candidate_id`. Workshop upserts a row when the producer emits identity (`candidate.accepted`, `delta.candidate_id`, `best_candidate_id`). Idempotent on `(optimizer_run_id, producer_candidate_id)`.

SFT / CISPO `optimizer_run` members with no candidate events → empty `candidates[]`. Do not invent CISPO candidates. Hosted RLVR is CISPO; do not add a generic `rlvr.*` candidate type.

### Suggested schema (migration 39)

New table, not a node kind. Sketch — fit existing id/timestamp style:

- `id` Workshop id (`can_…` or similar)
- `experiment_id` FK `experiment_groups`
- `optimizer_run_id` (member_id)
- `producer_candidate_id` TEXT NOT NULL
- `kind` / `protocol_id` TEXT (nullable until the producer declares them)
- `status` TEXT
- `parent_ids_json` TEXT NOT NULL DEFAULT `[]`
- `metrics_json` TEXT
- `content_digest` TEXT (nullable; bodies stay CAS)
- `created_at` / `updated_at`
- UNIQUE `(optimizer_run_id, producer_candidate_id)`

Do not put candidate rows on `experiment_edges`. Parentage is `parent_ids_json` (producer), not a new graph store.

### Producer

Fold from `optimizer_event.v1` already ingested on the run — `candidate_identity()` / `best_candidate_id` in `optimizers/service.rs` is the existing parse. Call upsert from the same persist path as `experiment_bind::settle_run` / event commit. Do not scrape visuals overlays. Do not re-read the pane.

### Read path

- Include `candidates[]` on `ExperimentGroup` **or** on the selected `optimizer_run` node — pick one, document it in the noun map, do not do both with different shapes.
- Experiments inspector: selecting an `optimizer_run` lists Candidate ids + status + producer id. Landmark e.g. `data-testid="experiment-candidate-list"`.
- Restart still shows the same rows (sqlite, not overlay JSON).

### MCP / Tauri

Candidate reads grew the existing DTO without a verb. The later compare/promote cut added `experiments_relate`, so the current command count is **264**. A field on an existing specta type needs regen but **not** a count bump.

`cargo test` takes **one** filter string.

```bash
# after a new command, from apps/synth_desktop/src-tauri:
cargo test -p synth-desktop --lib regenerate_protocol_bindings -- --ignored
```

Do not bump 264 unless you add another command.

---

## Pass

1. GEPA run with `session_ref` attaches as `optimizer_run`. Two distinct `candidate_id`s in events → two Candidate rows, same `optimizer_run_id`, unique `producer_candidate_id`.
2. Duplicate event / replay does not mint a third row.
3. `experiments_get` (or list) after process restart still returns those ids.
4. Inspector on that node shows both ids. No `kind=candidate` member node.
5. SFT/CISPO run without candidate events: member exists, `candidates[]` empty.
6. Noun map updated: Candidate is a class, not leftover overlay JSON.
7. Rust tests in `experiments/tests.rs` (and bind/fold if that is where upsert lives). One filter string.

## Fail (stop)

| Symptom | Why it is wrong |
| --- | --- |
| `experiment_nodes.kind = 'candidate'` | Fourth member kind. Ranking forbids it. |
| Candidate only in GEPA overlay / `bestResult` JSON | Not durable. |
| Workshop `POST /candidates` or re-apply per seed | Container owns apply. Register-then-run. |
| Reuse `eval_candidates` / `candidate_set_id` | Different class (eval policy files). |
| Compose `optimizer_run` slot or sourced TSX | Other track. |
| CISPO/SFT fake candidate rows | Empty list is honest. |
| `forked_from` / `compared_with` writers | Later. |
| Craftax 5×5 in-process stub | Gold is rust GameBench only; fail closed if gold is down. |
| `CoreRuntime` in UI copy / noun map | Local store is the authority. |

---

## Out of this cut

- Compare / promote
- `rerun_of` / `forked_from` producers
- `reports.ExperimentRecord` appendix JSON
- Window / `ArtifactRef` / chat remount
- Compose `optimizer_run` slot (visuals track; landed separately)
- Laguna vs Plugin
- One admission object for seal/pin/attach
- Advertised candidate inspector component on visuals

---

## Operator loop

1. Read this file + the Experiment zoom in the noun map.
2. Implement migration 39 + upsert + read DTO + inspector list.
3. Tests: in-memory sqlite fold from fixture GEPA envelopes (the service tests already emit `candidate_id` / `best_candidate_id`).
4. Update the noun map Candidate lines (class exists; leftover is compare/promote).
5. Do not commit unless asked.

If a GEPA live run is needed later: optimizer plugin must be installed; Craftax is `env:craftax_gold` only. First proof is sqlite tests, not CUA.
