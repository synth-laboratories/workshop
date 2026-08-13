# Handoff: finish the Aug 12 floor (Containers + Workshop + G1)

**For:** the engineer picking this up  
**Date:** 2026-08-12 ~14:15 ET  
**Supersedes:** [`HANDOFF_FINISH_OPTIMIZERS_WORKSHOP_2026-08-12.md`](./HANDOFF_FINISH_OPTIMIZERS_WORKSHOP_2026-08-12.md) (14:10 — mocks only; A2 register and visuals_ipc C1-08 were still open)  
**Nothing committed or pushed.** Do not reopen locked decisions. Do not commit unless asked.

Canonical plan: [`PLAN_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md`](./PLAN_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md)  
Contract index: [`HANDOFF_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md`](./HANDOFF_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md)  
Acceptance + cut table: [`aug_12_update.md`](./aug_12_update.md) A1–A8  
Containers suite: [`container_compat.md`](./container_compat.md) §12  

**Do not use** [`aug_12_notes.md`](./aug_12_notes.md). **Do not** wait on Specta codegen.

Synth Style: general foundations, targeted affordances, hierarchies of clear nouns, **one umbrella layer**, fail-closed, missing ≠ 0. Compatibility layers stay isolated.

---

## What you are finishing

Headless Containers §12 is largely on disk (uncommitted). Workshop can bind it. What is **not** true yet is the product bar: paid Luna 10× in Desktop, Docker Harbor GameBench, two real Banking77 GEPA, hosted SFT, A8 Desktop.

Dogfood still:

> Find the Craftax Rust GameBench container, run exactly 10 rollouts, collect Trace V5 / rewards, open a visual that compares them.

Plus: two live GEPA (Luna vs Sol) you can flip; Harbor `live.harbor_eval.v1` open **before** start; A8 dig.bench capstone.

---

## Trees (do not mix them)

| Work | Path | Branch | Git | Notes |
| --- | --- | --- | --- | --- |
| Workshop W0–W2 + live templates | `/Users/joshuapurtell/Documents/GitHub/workshop` | `josh/aug12-optimizers-workshop-visuals` | **Uncommitted.** Mixed with ChatGPT OAuth / landing WIP | Split before commit |
| Containers §12 floor | `/Users/joshuapurtell/Documents/GitHub/containers` | `dev` (**behind `origin/dev` by 7**) | **Uncommitted.** Large `platform/` add | Rebase/merge origin/dev **before** PR. Do not force-push `dev` |
| G1 (intended) | `/Users/joshuapurtell/Documents/GitHub/optimizers-g1` | `josh/aug12-g1-gepa` | Uncommitted, cleaner | **Port DualGepaHub here** — see trap below |
| SFT | `/Users/joshuapurtell/Documents/GitHub/optimizers-beta-sft` | `josh/aug12-sft` | Uncommitted | `algorithm_id: "sft"`, not `goex.sft.v1` |

**Leave these alone unless you are porting**

| Path | Why |
| --- | --- |
| `/Users/joshuapurtell/Documents/GitHub/optimizers` (`main`) | Other dirty files **plus accidental DualGepaHub from this session**. Do not PR `main`. Copy G1 bits into `optimizers-g1`, then revert the accidental edit on `main` if it should not ship there |
| `/Users/joshuapurtell/Documents/GitHub/optimizers-beta` | MAPO branch `mapo-h2-preserve-20260706` |
| workshop `dev` worktree | `/Users/joshuapurtell/Documents/Codex/2026-08-11/ad/work/workshop-muse-cua` |

### Trap: DualGepaHub landed on the wrong tree

This session added `DualGepaHub` / `gepa_policy_ref` to:

`/Users/joshuapurtell/Documents/GitHub/optimizers/src/synth_optimizers/observability.py`

It is **not** in `optimizers-g1`. First G1 job: port that + `tests/test_g1_fail_closed.py::test_dual_gepa_hub_luna_vs_sol_does_not_cross_or_stall` into `optimizers-g1`. Do not claim A3.

---

## Locked (do not reopen)

- Harbor is the **only** first-class external fold. OpenEnv/Prime = wraps. GameBench / τ-bench / **dig.bench** = **content**.
- Policy = `{harness, config, code?}`. No sibling `HarnessService`. No `harness_ref`.
- Two schemas only: `synth.trace-stream-event.v1` (eval/child) and `optimizer_event.v1` (search/train). `synth.stream-event.v1` is a **trait**, not a wrapper.
- Child eval = `synth.resource-ref.v1` `{kind: container_rollout, id, attributes: {stream_id, reward_url}}`. No signed `child_eval_ref`. No NEV/frames on the optimizer stream.
- `/reward` v1 = scalar + `node_results[]`. Missing → `null`, never `0`.
- One durable log. Consumer cursor = **sequence**. Never speak `nev_cursor`.
- Slot **`stream`** for live eval. Slot **`optimizer_run`** for optimizer family. Fail `live` and `jobs`.
- `stream.subscribed` is a non-advancing control ACK. HTTP 200 on GET is not ready. Heartbeats do not count.
- `telemetry.transport=auto` forbidden on visual / authoritative runs.
- Connect-before-start. Persist-before-publish. Uninstall sidecar ≠ delete events/visuals/templates.
- Do not Harbor-wrap or OpenEnv-wrap dig.bench. Do not invent frames for text games.

---

## What landed this session (uncommitted)

### Containers (`containers` / `dev`)

Umbrella: `CompatPlatform` in `src/synth_containers/platform/state.py`. Content families are `TargetRuntime` children under `platform/runtimes/` (`craftax`, `harbor`, `digbench`, `openenv`). Dispatch by `TargetRuntimeKind`, not `contracts.RuntimeFamily`.

| Piece | Where |
| --- | --- |
| Typed HTTP bodies | `platform/http_requests.py` |
| One `classify_plan_outcome` | `platform/reward_plan.py` — C2-07 `eval:gated` → gated, reward `None` |
| C4-06 hillclimb DAG | Parent `/reward` is **not** a copy of child env-sum. Gate first; combiner fail-closed |
| Nested child `/reward` | Uses **`pin.reward_kind`**, not parent spec |
| Occupancy | 11th lease → 429 `scale_leases` |
| WS | `/rollouts/{id}/ws`; advertised `websocket: derived` when SSE exists |
| `/info` | Now includes **`runtime_family`** (Desktop classifies from this) |
| Conformance | `tests/conformance/container_compat/run.py` + `write_pr_receipts.py` |
| Examples | `examples/craftax_ten_seeds.py`, `deo_nested_reward.py`, `headless_visual_consumer.py` |

Do **not** put Echo / dig.bench / Craftax episode loops back in `state.py` or `compat/`.

### Workshop

| Piece | Where |
| --- | --- |
| Shared C1-08 helpers | `apps/synth_desktop/src-tauri/src/container_stream.rs` |
| Eval driver subscribe-first | `eval_driver.rs` — prepare → declared poll until `stream.subscribed` → start. Slot `stream` |
| visuals_ipc subscribe-first | Same order. Native benchmark routes are refused; they must be folded inside normalized Containers. Declared poll only; never guess `/events` |
| Persist-before-publish | `storage/live_spool.rs`; both drivers return `spool_digest` |
| Family → template | `visuals/live_eval.rs` — Harbor/`live.harbor_eval.v1`, Craftax/`live.craftax.v1`, dig.bench/`live.digbench.v1` |
| In-app register | Data page attach + `hydrate_container` / visuals_ipc register write `metadata.liveEval` (template, slot `stream`, Harbor `policyRefs` luna_med+sol_med) and **open that visual before start** |
| Harbor frames | Register/bind refuses `live_frames=native` |
| `open_visual` | No longer defaults to `live.container_rollouts.v1`. Requires `templateId` or a classified family. Wrong template for family fails closed |
| W2 OptimizerManager | `optimizers/manager.rs` — install/start/stop/uninstall/status/pin. Loopback JSON `/health`, **not** a real `synth-optimizers` process. Uninstall ≠ delete service rows |
| Live templates | `visuals/templates/live.{craftax,harbor_eval,digbench}.v1/` + `liveEvalReducer.ts`. Deleted `live.dock_harbor.v1` on purpose |

### Optimizers G1 (partial, **wrong tree**)

On `/Users/joshuapurtell/Documents/GitHub/optimizers` (`main`): missing sequence/usage/reward stay `None`; `container_child_eval_ref`; `DualGepaHub` (in-memory two logs, Luna vs Sol `policy_ref`, flip-read does not stall). **Not Banking77. Not A3.**

`optimizers-g1` already has fail-closed tests + two-worker clamp. Merge DualGepaHub into that tree.

---

## Honest status vs A1–A8

| ID | Headless / mock | Desktop / live | Do not fake |
| --- | --- | --- | --- |
| **A1** Craftax Luna 10× | Containers HTTP + `craftax_ten_seeds.py`. Eval driver C1-08 | Visual-before-Luna on **paid** path is nightly. Not claimed | JSONL smoke as A1 |
| **A2** Harbor GameBench | Fixture + ATIF projection + C5-02 two configs + C5-06 subscribe | **In-app register + `live.harbor_eval.v1` first** against the **compat façade**. Docker Harbor + packaged GameBench still required | Harbor-wrapping GameBench wire; slot `jobs`; fake map |
| **A3** two GEPA | DualGepaHub + two spools (unit) | Real Banking77, Luna vs Sol proposers, flip live visual | JSONL as A3 |
| **A4 / A6** SFT | Adapter stubs in `optimizers-beta-sft` | Hosted Tinker | Fake Craftax SFT JSONL; shut-down OpenAI Fine-tuning API |
| **A5** streams | C1 poll/SSE/WS on façade | Consumers must use **declared** URLs only (client already fail-closed) | `transport=auto` |
| **A7** Echo | Out of cut | — | Unmodified image this cut |
| **A8** dig.bench | C8 mock headless | Both harnesses + `digbench_public --paid` | Harbor wrap; invented dungeon; token in log |

---

## Finish list (priority)

1. **Hygiene** — split workshop OAuth vs this cut; rebase containers `dev` onto `origin/dev`; port DualGepaHub `main` → `optimizers-g1`; do not PR MAPO or `optimizers` `main`.
2. **W2 spawn** — `OptimizerManager::start` still serves a signed catalog + fake loopback `/health`. Next: allowlisted `synth-optimizers gepa serve` (or `gepa run`) child, digest/signature, loopback auth, health. `recipes.rs` still spawns Banking77 via raw `uv` and **bypasses** the manager. Pin `sidecar_version × algorithm_version × recipe_version` through the manager. Stopping must not delete `OptimizerService` rows (already tested).
3. **A1 Desktop** — register Craftax façade (or gold HTTP), open `live.craftax.v1` first, C1-08, **then** first Luna call. `examples/craftax_ten_seeds.py` is the headless twin. Paid Luna = nightly; do not claim from fixtures.
4. **A2 live** — same register path against a **Docker Harbor** packaged GameBench task, not only `harbor_public` in-process. Two `policy_ref`s before start (already pinned luna/sol on register). Mid-trial bind refused (containers C5-02). Verifier ≡ `/reward` script node. ATIF is a projection.
5. **A3** — two real `algorithm_id: "gepa"` Banking77 runs, Luna vs Sol proposer `policy_ref`s, distinct logs/budgets/fronts/visuals, flip does not stall (C7-O02). Wire Desktop recipes to G1 child-eval refs. DualGepaHub is not this.
6. **A8 Desktop** — after C8 receipt: `live.digbench.v1` before `start_session`; basic vs agentic `policy_ref`; text only; `/reward` from `completed`/`game_over`; token never in the log; reopen with their session gone.
7. **A4 / A6** — hosted jobs only. Visual before train. Second job `queued` on one accelerator. Promotion ≠ `checkpoint.ready`.

Out of cut: Prime GSM8K, Chess OpenEnv, MAPO/RLVR/OHCO, merged Luna-vs-Sol overlay, two-board layout, Specta exporter, GEPA/SFT **on** dig.bench, Echo unmodified image.

---

## Workshop dirty tree — split before you commit

**This cut (include)**

```
apps/synth_desktop/src-tauri/src/container_stream.rs          # NEW
apps/synth_desktop/src-tauri/src/eval_driver.rs
apps/synth_desktop/src-tauri/src/visuals_ipc.rs
apps/synth_desktop/src-tauri/src/visuals/live_eval.rs
apps/synth_desktop/src-tauri/src/visuals/mod.rs
apps/synth_desktop/src-tauri/src/storage/live_spool.rs
apps/synth_desktop/src-tauri/src/optimizers/manager.rs
apps/synth_desktop/src-tauri/src/optimizers/{mod,service}.rs
apps/synth_desktop/src-tauri/src/lib.rs                       # hydrate liveEval + OptimizerManager
apps/synth_desktop/src-tauri/src/contract/{commands,events,specta}.rs
apps/synth_desktop/src/renderer/src/components/DataPage.tsx   # open classified visual on attach
visuals/runtime/{liveStream,liveEvalReducer,bind,index}.ts
visuals/templates/live.{craftax,harbor_eval,digbench}.v1/
visuals/templates/optimizer.gepa.*/ optimizer.sft.*/
visuals/tests/{live_stream_contract,live_eval_reducer,optimizer_family,registry}.test.mjs
docs/PLAN_*.md docs/HANDOFF_*.md docs/aug_12_update.md docs/container_compat.md
```

Deleted on purpose: `visuals/templates/live.dock_harbor.v1/`.

**Not this cut** (leave on the v02 branch or a different PR): ChatGPT Codex OAuth (`codex_oauth.rs`, `ChatgptCodexSubscriptionCard.tsx`, oauth specs), activity placement invariant, landing/titlebar/composer/settings churn, `docs/SUBAGENTS_UX_PROPOSAL_2026-08-12.md`, `prototypes/`.

**Containers this cut:** `src/synth_containers/platform/**`, conformance runner, examples listed above, `runtime_family` on `/info`. Do not mix unrelated tracing/compat WIP into the floor PR if you can split it.

---

## How to run (last green this session)

```bash
# containers floor
cd /Users/joshuapurtell/Documents/GitHub/containers
uv run --with pytest pytest tests/test_container_compat_conformance.py \
  tests/test_craftax_eval_examples.py tests/test_container_compat_floor.py \
  tests/test_platform_leftovers.py tests/test_http_requests.py -q
uv run python tests/conformance/container_compat/write_pr_receipts.py

# workshop visuals + rust
cd /Users/joshuapurtell/Documents/GitHub/workshop
node --test visuals/tests/registry.test.mjs \
  visuals/tests/live_eval_reducer.test.mjs \
  visuals/tests/live_stream_contract.test.mjs
cd apps/synth_desktop/src-tauri
cargo test --lib live_eval
cargo test --lib eval_driver
cargo test --lib visuals_ipc
cargo test --lib container_stream
cargo test --lib optimizers::manager

# G1 — run in optimizers-g1 after you port DualGepaHub
cd /Users/joshuapurtell/Documents/GitHub/optimizers-g1
uv run --with pytest pytest tests/test_g1_fail_closed.py -q
```

This session: containers leftovers metadata + start_rollout; `live_eval` 4; `eval_driver` 17; `visuals_ipc` 6 (includes 2s C1-08 timeout); DualGepaHub 6 on the **wrong** optimizers tree.

---

## Wire shapes (copy these)

**C1-08 order (eval driver + visuals_ipc)**

```text
POST /rollouts/prepare
GET  {declared transports.poll.url}?after=0   until kind=stream.subscribed AND ready=true
POST /rollouts  { rollout_id, slot: "stream", telemetry.transport ≠ auto }
```

Native GameBench engines are not a Workshop protocol. Register a Containers fold that
exposes normalized prepare/start/status/events/reward. Workshop refuses missing prepare
instead of falling through to `/step` or `/event_log`.

**Harbor liveEval metadata** (written on register)

```json
{
  "family": "harbor",
  "templateId": "live.harbor_eval.v1",
  "slot": "stream",
  "liveFrames": "unsupported",
  "policyRefs": [
    {"harness": "harbor_fused", "config": "luna_med"},
    {"harness": "harbor_fused", "config": "sol_med"}
  ]
}
```

**Child eval ref**

```json
{
  "schema": "synth.resource-ref.v1",
  "kind": "container_rollout",
  "id": "rollout_…",
  "attributes": {
    "stream_id": "stream:…",
    "reward_url": "/reward?rollout_id=rollout_…"
  }
}
```

---

## If you get stuck

- Visual connected after first model call → you skipped C1-08 or opened `live.container_rollouts.v1`.
- Harbor map / frames → you bound Craftax template or ignored `live_frames=unsupported`.
- Reward `0` on incomplete → you filled missing. Stop. Return `null`.
- Second GEPA stalls when you flip → singleton worker or shared spool. C7-O02 / DualGepaHub pattern.
- `recipes.rs` “works” without OptimizerManager → that is the W2 gap, not a pass.
- dig.bench needs a second stream or Harbor wrap → A1–A7 were not actually done.

Ask Containers only if the **receipt** is missing one of the five freezes (full stream descriptor on prepare/create, `stream.subscribed`, typed occupancy, advertised retention, sequence cursor). Workshop already fail-closes client-side.
