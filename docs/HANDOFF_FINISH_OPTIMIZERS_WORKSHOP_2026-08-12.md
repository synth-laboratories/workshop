# Handoff: finish Optimizers + Workshop visuals (Aug 12)

> **Superseded 2026-08-12 ~14:15 ET** by [`HANDOFF_FINISH_FLOOR_2026-08-12.md`](./HANDOFF_FINISH_FLOOR_2026-08-12.md).  
> This 14:10 note is missing visuals_ipc C1-08, in-app Harbor register, containers `runtime_family`, and the DualGepaHub-on-the-wrong-tree trap. Use the floor handoff.

**For:** the engineer picking this up  
**Date:** 2026-08-12 ~14:10 ET  
**Status:** Implementation started on mocks. **Nothing committed or pushed.** Three dirty trees. Do not reopen locked decisions.

Canonical plan: [`PLAN_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md`](./PLAN_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md)  
Contract index: [`HANDOFF_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md`](./HANDOFF_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md)  
Containers suite: [`container_compat.md`](./container_compat.md) §12

---

## What you are finishing

Make A1–A8 true in Desktop / Optimizers / optimizers-beta **without inventing a second stream**. Containers owns the floor receipt (C7/C8 signed off). This cut implemented the mock-safe Workshop + optimizer slices. Live Craftax/Harbor/dig.bench and real Banking77/SFT jobs still pin that receipt.

Dogfood still:

> Find the Craftax Rust GameBench container, run exactly 10 rollouts, collect Trace V5 / rewards, open a visual that compares them.

Plus: two live GEPA (Luna vs Sol) you can flip; two standalone SFT jobs; Harbor `live.harbor_eval.v1` open before start; A8 dig.bench capstone.

---

## Trees (do not mix them)

| Work | Path | Branch | Base | Git |
| --- | --- | --- | --- | --- |
| Workshop W0–W2 + templates | `/Users/joshuapurtell/Documents/GitHub/workshop` | `josh/aug12-optimizers-workshop-visuals` | v02 line (contains `dev`; raw `dev` is another worktree and too old for this desktop) | **Uncommitted.** Mixed with unrelated WIP — see below. |
| Optimizers G1 | `/Users/joshuapurtell/Documents/GitHub/optimizers-g1` | `josh/aug12-g1-gepa` | `origin/dev` @ `3c22926` | Worktree. Uncommitted. Clean-ish. |
| optimizers-beta S1+S2 | `/Users/joshuapurtell/Documents/GitHub/optimizers-beta-sft` | `josh/aug12-sft` | `origin/dev` @ `652d962` | Worktree. Uncommitted. Clean-ish. |

**Leave these alone**

| Path | Why |
| --- | --- |
| `/Users/joshuapurtell/Documents/GitHub/optimizers` | Original `main` worktree with **other** dirty files. G1 lives in `optimizers-g1`. |
| `/Users/joshuapurtell/Documents/GitHub/optimizers-beta` | MAPO branch `mapo-h2-preserve-20260706`. SFT lives in `optimizers-beta-sft`. |
| workshop `dev` worktree | `/Users/joshuapurtell/Documents/Codex/2026-08-11/ad/work/workshop-muse-cua` |

Nothing has been pushed. First job: **split / commit / PR** the three feature trees. Do not squash workshop ChatGPT OAuth and this visuals cut into one commit.

---

## Workshop dirty tree — split before you commit

`josh/aug12-optimizers-workshop-visuals` has **this cut** plus unrelated desktop WIP. Commit only the visuals/optimizer files unless you intend to ship OAuth too.

**This cut (include)**

```
apps/synth_desktop/src-tauri/src/optimizers/manager.rs
apps/synth_desktop/src-tauri/src/optimizers/{mod,service}.rs   # bus publish + persist_run
apps/synth_desktop/src-tauri/src/storage/live_spool.rs
apps/synth_desktop/src-tauri/src/visuals/live_eval.rs
apps/synth_desktop/src-tauri/src/container_stream.rs
apps/synth_desktop/src-tauri/src/eval_driver.rs
apps/synth_desktop/src-tauri/src/visuals_ipc.rs
apps/synth_desktop/src-tauri/src/visuals/models.rs
apps/synth_desktop/src-tauri/src/lib.rs                       # OptimizerManager + container_stream
apps/synth_desktop/src-tauri/src/contract/{commands,events,specta}.rs
apps/synth_desktop/src-tauri/src/limits.rs
apps/synth_desktop/src/renderer/src/bridge/protocolConstants.ts
apps/synth_desktop/src/renderer/src/components/OptimizersPage.tsx  # missing cost → —
apps/synth_desktop/skills/run-live-container-evals/SKILL.md
visuals/runtime/{liveStream,liveEvalReducer,bind,index}.ts
visuals/chrome/useLiveEvalStream.ts
visuals/templates/live.{craftax,harbor_eval,digbench,container_rollouts}.v1/
visuals/templates/optimizer.gepa.*/
visuals/templates/optimizer.sft.*/
visuals/templates/optimizer.run.v1/components/{FamilyShell,projectEvents}.tsx
visuals/tests/{live_stream_contract,live_eval_reducer,optimizer_family,registry}.test.mjs
visuals/fixtures/live_{eval,container_rollout}_events.json
visuals/mcp/{tools.json,server.md}
package.json   # test:visuals --experimental-strip-types
docs/PLAN_*.md docs/HANDOFF_*.md   # if you want the docs in the PR
```

Deleted on purpose: `visuals/templates/live.dock_harbor.v1/` (renamed to harbor).

**Not this cut (leave on the v02 branch or a different PR)**

ChatGPT Codex OAuth (`codex_oauth.rs`, `ChatgptCodexSubscriptionCard.tsx`, oauth playwright/bombadil specs), activity placement invariant, landing/titlebar/composer/settings churn, `docs/SUBAGENTS_UX_PROPOSAL_2026-08-12.md`, `prototypes/`, intern handoff-package edits, unless you know they belong.

---

## Done (mocks / unit)

### W0 — bind + honesty (workshop)

- Live-eval slot is **`stream`**. `live` and `jobs` fail closed (TS bind + Rust `validate_bindings`). Intern `acceptance` is not forced to `stream`.
- `/events` is never a declared URL. `/rollouts/{id}/stream` is allowed when it is the declared SSE URL.
- `stream.subscribed` = ready, not evidence. Heartbeats ignored. Ingest de-dupes by identity.
- Persist-raw: `storage/live_spool.rs` → CAS kind `traces`. Eval driver + visuals IPC return `spoolDigest`.
- Missing reward/cost → `—` / `null`, never `$0.00`. `telemetry.transport=auto` refused.
- GEPA ingest: `OptimizerService::append_events` broadcasts `optimizer.run.updated` on `runtime:event`.
- Connect-before-start helper: `container_stream.rs`. Eval driver `POST /rollouts/prepare` → poll declared `transports.poll.url` until `stream.subscribed` → then `POST /rollouts`. **Needs Containers prepare/echo on the wire** or the 2s timeout fails closed.

### W1 / A8 templates (fixture only)

Same reducer `visuals/runtime/liveEvalReducer.ts`.

| Template | Fixture kinds | Must not |
| --- | --- | --- |
| `live.craftax.v1` | frames, RewardSignals, policy spans | `reward.txt`; invent map |
| `live.harbor_eval.v1` | trial → tools → verifier (`reward.txt`) | frames; slot `jobs` |
| `live.digbench.v1` | obs, legal_actions, stats, status | frames; dungeon; token in log |

**Not done:** TS-E01…E08 on **real** streams. A1 Desktop / A2 in-app register / A8 two harnesses against `api.digbench.ai`. Wait for C7-W + C3/C5 and C8 receipts.

### W2 — OptimizerManager

`apps/synth_desktop/src-tauri/src/optimizers/manager.rs`

Tauri: `optimizer_sidecar_{install,start,stop,version,status,uninstall}`. Events: `optimizer:status` (not a second eval stream).

Uninstall deletes `versions/{version}/` only. Tests prove runs/events/visuals/spools/templates remain. Active pinned runs refuse uninstall.

**Gap:** `recipes.rs` still spawns Banking77 GEPA via `uv` itself. Manager owns a signed catalog + loopback `/health` sidecar, not the recipe worker yet. Next: recipes should start through OptimizerManager (or the sidecar should be the real `synth-optimizers` process).

### A3/A4/A6 visual family (fixture only)

Slot **`optimizer_run`**, not `stream`. Child evals are `container_rollout` resource-refs (`stream_id` + `/reward`). No NEV/frames on the optimizer stream. No merged Luna-vs-Sol Pareto overlay (out of cut).

`optimizer.run.v1` remains the unknown-algorithm fallback.

### G1 — optimizers (`optimizers-g1`)

- Missing `sequence_number` stays `None`. Missing usage/reward stay `null`. Present `0` is still `0`.
- `container_child_eval_ref(rollout_id, stream_id, reward_url)` → `synth.resource-ref.v1` / `kind: container_rollout`. No signed `child_eval_ref`.
- Service `worker_count` clamped to `[2, 10]`. Singleton refused. Slots `synth-gepa-service-slot-00/01`.
- Luna vs Sol = two `policy_ref`s `{harness: gepa_proposer, config: luna_med|sol_med}`. Child eval is a second pin. No `harness_ref`.
- One spool per `optimizer_run_id`. Dual-write `events.jsonl` + `events.optimizer.jsonl`.

**Not claimed:** A3 live Banking77 flip. JSONL is a bounded wiring recipe only. Real create-rollout still waits on Containers C7-O stream descriptor echo.

### S1+S2 — optimizers-beta (`optimizers-beta-sft`)

New crate `crates/synth_sft`. `algorithm_id: "sft"` — **not** `goex.sft.v1`.

- Accelerator pool default 1 (`OPTIMIZERS_BETA_HOSTED_SFT_ACCELERATOR_SLOTS`). Second job honestly `queued`; first log untouched.
- Visual-ready / `run.started` before first train metric.
- `sft.checkpoint.ready` ≠ promotion. Promotion = `sft.checkpoint.promotion_evaluated` then `sft.checkpoint.promoted`.
- Campaigns = `container_rollout` refs. Metrics keyed `(checkpoint_id, split_role, step, evaluator_version)`.
- Replay from JSONL after `drop_provider`.

**Not claimed:** A4/A6 against real Tinker + real Containers campaigns.

Implementation spec + enable gates: [`optimizers_beta_sft.md`](./optimizers_beta_sft.md). Desktop catalogs `sft.craftax.nemotron-nano.tinker.v1` as unavailable until `TINKER_RUNNER_READY` and a pinned Tinker model id. Beta `TinkerTrainingBackend::from_env` fails closed; do not impersonate Tinker HTTP. The historical OpenAI Fine-tuning console is the UX to emulate, not a live provider.

---

## Finish list (priority)

1. **Commit/PR hygiene** — three repos, this-cut files only. Push with `-u`. Do not force-push `dev`/`main`.
2. **W2 → recipes** — Banking77 smoke should pin sidecar_version × algorithm_version × recipe_version through OptimizerManager, not a raw `uv` spawn that bypasses install/digest.
3. **Wire G1 into Desktop** — Workshop `OptimizerService` / recipes should speak the G1 child-eval ref and two-worker service. Today Desktop still has its own smoke recipe.
4. **Wire SFT into Desktop** — `sft_recipes.rs` + `optimizer.sft.*` templates bind `algorithm_id: "sft"` events from the beta sidecar, not `goex.sft.v1`.
5. **W1 live** after Containers C7-W + C3/C5: bind declared stream, wait `stream.subscribed`, first Luna call, scrub ≤ cursor, seal, reopen from CAS + Trace V5. `prepare` + poll must match what Containers actually echoes.
6. **A8 live** after C8: `live.digbench.v1` ready before `start_session`; basic vs agentic `policy_ref`; token never in log; reopen with their session gone.
7. **A3** — two real Banking77 GEPA, Luna vs Sol, flip visual, other log must not stall (C7-O02).
8. **A4 / A6** — two hosted SFT jobs distinct `dataset_digest`; one multi-checkpoint job with campaigns.

Out of cut (do not start): Prime GSM8K, Chess OpenEnv, MAPO/RLVR/OHCO, merged Luna-vs-Sol overlay, two-board layout, Specta exporter, GEPA/SFT **on** dig.bench.

---

## Locked (do not reopen)

- Two schemas only: `synth.trace-stream-event.v1` (eval/child) and `optimizer_event.v1` (search/train). `synth.stream-event.v1` is a **trait**, not a wrapper.
- No signed `child_eval_ref`. No sibling `harness_ref`. Policy = `{harness, config, code?}`.
- `/reward` v1 = scalar + `node_results[]`. Missing stays missing.
- Consumer cursor = **sequence**. Never speak `nev_cursor`.
- Slot `stream` for live eval. Slot `optimizer_run` for optimizer family. Fail `live` and `jobs`.
- `auto` transport forbidden on visual/authoritative runs.
- Uninstall sidecar ≠ delete mirrored events/visuals/templates.
- Harbor is the only first-class external fold. dig.bench is content, not a Harbor wrap.

---

## Tests to re-run after you touch things

```bash
# workshop
cd /Users/joshuapurtell/Documents/GitHub/workshop
node --experimental-strip-types --test visuals/tests/*.test.mjs
cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --lib \
  persist_dedupes_and_reopens forbids_live_and_jobs_slots \
  append_events_publishes_optimizer policy_rollout_forwards \
  validates_canonical_bindings optimizers::manager

# G1
cd /Users/joshuapurtell/Documents/GitHub/optimizers-g1
python3 -m pytest tests/test_g1_fail_closed.py -q
cargo test -p synth_optimizer_platform --lib -- missing_sequence present_sequence child_eval luna_and_sol dual_writes two_spools
cargo test -p synth_gepa service_refuses_singleton

# SFT
cd /Users/joshuapurtell/Documents/GitHub/optimizers-beta-sft
cargo test -p synth_sft
```

Last green (2026-08-12): visuals 16+7 family tests; manager 7; G1 pytest 7 + cargo 8; `synth_sft` 6.

---

## Child eval ref shape (copy this)

```json
{
  "schema": "synth.resource-ref.v1",
  "kind": "container_rollout",
  "id": "rollout_…",
  "role": "candidate_evaluation",
  "attributes": {
    "stream_id": "…",
    "reward_url": "/reward?rollout_id=rollout_…"
  }
}
```

Optimizer events carry **links**, not NEV/frames.

---

## If Containers asks what is still blocking Desktop

Five freezes from the plan, still required on the **receipt** (not just the doc):

1. Create-rollout (or `/rollouts/prepare`) echoes full stream descriptor.
2. Non-advancing `stream.subscribed` before first paid/mutating event.
3. Typed occupancy refusal when `scale_leases` exhausted.
4. Artifact retention advertised (`run` vs TTL).
5. Consumer cursor = sequence.

Plus: C1-09 / C7-W01 fail slots `live` **and** `jobs`; no `telemetry.transport=auto` on visual/authoritative runs.

Workshop already fails closed on those client-side. Live A1/A2/A8 will not pass until the engine echoes them.
