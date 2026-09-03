# First-class live SFT + CISPO in Workshop (GEPA parity)

**Workshop remaining work** lives in
[`HANDOFF_WORKSHOP_SFT_CISPO_LIVE_VISUALS.md`](HANDOFF_WORKSHOP_SFT_CISPO_LIVE_VISUALS.md)
(click → live `metric_points`). This file is the producer / launch brief that
already landed.

**Status:** producer + Desktop launch path implemented. Public fixture Banking77
SFT/CISPO event pages and SSE were proven live. Desktop visual click-through is
not.
**Goal:** click a Desktop recipe, run public Tinker SFT / CISPO, and watch
`optimizer.sft.live.v1` / `optimizer.cispo.live.v1` update from durable collections
the same way GEPA live updates `optimizer.gepa.live.v1`.
**Non-goal:** a second paid gpt-oss-20b canary. Reuse
`optimizers/docs/receipts/tinker-gpt-oss-20b-banking77-canary-cispo/cispo.slime.v1.receipt.json`.

Identity (do not conflate):

| Surface | `algorithm_id` | Implementation |
|---|---|---|
| Standalone SFT | `sft` | `sft.tinker.v1` |
| True CISPO | `cispo` | `slime-reference` / `cispo.slime.v1` |
| GoEx SFT lane | `go-ex` | not this plan |
| Generic IS | — | refuse |

Craftax remains rust GameBench gold only (`env:craftax_gold`, `GoldCraftaxWorld`).
Public `optimizers` only. No `optimizers-beta`, no Keychain, no growing `cli.py`
(file-size allowlist: shrink only).

---

## What already landed (do not redo)

Python public services already emit GEPA-shaped pages:

- `optimizer_event_page.v1` (`run_id`, `log_id`, `after_sequence`, `next_sequence`, `terminal`)
- identity on every event: `attempt_id`, `type`, `sequence_number`, `optimizer_run_id`, `algorithm_id`
- flattened `train_loss` / `trainLoss` from nested `metrics.loss`
- collection aliases: `metric_points`, `candidates`, `evaluations` (+ CISPO `rollouts`)
- CISPO `GET /state/batch` and `cispo.clip.identity` (slime bounds clip_low=0, clip_high=5)

Workshop already:

- maps `sft.step.metrics` → `sft.training.metrics`, `cispo.update.completed` → `training.metrics`
- negotiates `optimizer.cispo.live.v1` (CISPO no longer reuses the SFT template)
- labels CISPO as CISPO
- polls JSON (not renderer SSE); `PROJECTION_FIRST_ALGORITHMS` includes `sft` and `cispo`
- admits hosted CISPO **only** via `TINKER_CISPO_VALIDATION_RECEIPT` (`validated` + `paid_update`). `SYNTH_OPTIMIZERS_CISPO_HOSTED_ADMITTED` still does nothing.

The remaining work is **launch + operate**, not another visual shell.

---

## How GEPA live actually works (clone the visual contract, keep the SFT/CISPO watch path)

```
OptimizersPage.startRecipe(recipeId)
  → OptimizerService.start_recipe
      → recipes::start
          → ensure sidecar (handshake → SYNTH_OPTIMIZER_BASE_URL)
          → create run + open_visual(optimizer.gepa.live.v1)
          → spawn run_recipe_worker
                → gepa run --config
                → loop ~750ms:
                     GET /runs/{id}/optimizer-events?after_sequence=N
                     → ingest_event_page → local optimizer_event.v1 journal
                     → bus optimizer.run.updated

VisualHost.subscribeToRun (projection-first)
  → wakeup / 750ms poll → runViewV2
  → refetch collections whose revision moved
  → GEPA: candidates | evaluations | proposer_calls
  → SFT/CISPO: metric_points | candidates | evaluations (| rollouts)
```

Desktop does **not** stream Python SSE into the visual. SSE is a CLI mirror.
Live UI = mapped events in the local journal + collection pages.

SFT/CISPO today use a sibling path (`create_and_watch` → sidecar job log →
`watch_job` → `append_mapped_event`). That is acceptable **if** the producer
page is `optimizer_event_page.v1` and mapping lands `sft.training.metrics` /
`training.metrics` / `cispo.clip.identity`. Do not invent a third ingest.
Do not fold SFT/CISPO into `gepa service`.

| | GEPA | SFT hosted today | CISPO hosted today |
|---|---|---|---|
| Launch | `recipes::start` + `gepa run` | `hosted_sft::start` → `create_and_watch` | `cispo::start_hosted` → `create_and_watch` |
| Producer | sidecar `GET /runs/{id}/optimizer-events` | public SFT `:8878` | public CISPO `:8880` |
| Watch | `ingest_event_page` | `drive_hosted_sft_job` + `append_mapped_event` | `drive_hosted_cispo_job` (receipt + **wrong payload** + **tunnel eval gate**) |
| Desktop starts producer? | **yes** (`gepa service`) | **no** | **no** |
| Visual | `optimizer.gepa.live.v1` | `optimizer.sft.live.v1` | `optimizer.cispo.live.v1` |

`hosted_sft::spawn_hosted_worker` (`ingest_event_page`) is `#[allow(dead_code)]`.
Live SFT is `drive_hosted_sft_job`. Do not run both.

---

## Why a click fails today

### Hosted SFT (`sft.banking77.nemotron-lightning.tinker.v1`)

Watch path is already live. Launch is operator-gated.

1. Desktop never starts `sft service`. Recipe is `unavailable` unless
   `SftOptimizerClient::from_env()` succeeds (`SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN`
   required) **and** `SYNTH_SFT_BANKING77_TRAIN_JSONL` is a real file.
2. Connect fails if `:8878` is down even when the recipe is marked available
   (no health ping).
3. Recipe copy still says “TINKER_API_KEY held by the Optimizers-beta executor”.
   Public `optimizers` `TinkerSftExecutor` is the executor. Fix the copy.
4. Banking77 TOML also names a classify slot (`127.0.0.1:8110` by default).
   Craftax hosted SFT wants GameBench gold on `:8098`.
5. `OptimizersPage` filters `sft` / `cispo` out of the agent-guide grid, so the
   MLX buttons in that card never render. Hosted SFT one-click lives on
   `TrainingWorkspace` (`sft.banking77.nemotron-lightning.tinker.v1`).
6. Mapping + visual are ready. `SYNTH_OPTIMIZERS_SFT_FIXTURE=1` on the **service**
   process is enough to emit `sft.step.metrics` without Tinker. The JSONL gate
   is local data, not paid compute.

### Hosted CISPO (`cispo.slime.hosted.v1`)

Ordered kill list. Each item fails even if the previous is fixed:

1. **Admission.** No `TINKER_CISPO_VALIDATION_RECEIPT` → placement
   `training.cispo.hosted` absent → UI shows “Hosted CISPO is not available”
   and offers `cispo.mlx.v1`.
2. **Wrong payload.** `cispo.rs` `start_hosted` calls `bind_cispo` (container)
   and sends `{algorithm, implementation, task, eps_high, evaluation, rollout}`.
   Public `validate_cispo_request` requires `cispo.request.v1` with
   `algorithm_id=cispo`, `implementation=slime-reference`,
   `implementation_version=cispo.slime.v1`, `provider=tinker`, plus `dataset`,
   `training`, `reward`, `mode`. First POST 400s.
3. **Tunnel eval gate.** `drive_hosted_cispo_job` always calls
   `validate_tunneled_evaluation_plan(config)` before submit. Public Banking77
   CISPO rewards in-process (`banking77.exact_label.v1`). Even a correct
   `config_json` wrapper fails this check unless the top-level `evaluation`
   is a container tunnel plan. **Skip the tunnel gate for public Tinker CISPO.**
   `drive_hosted_cispo_job` already prefers `config.config_json` when present.
4. **No CISPO CLI.** `serve_cispo_service` exists; there is no
   `synth-optimizers cispo service`. Operator cannot start `:8880` the way they
   start SFT. Do **not** grow `cli.py`. New module + `pyproject.toml` script.
5. **No fixture serve path.** `SftService.from_env` honors
   `SYNTH_OPTIMIZERS_SFT_FIXTURE=1`. `serve_cispo_service` always constructs
   `CispoService(database_path, background=True)` with `fixture=False`.
   `CispoService.from_fixture` is tests-only. Add
   `SYNTH_OPTIMIZERS_CISPO_FIXTURE=1` (or `--fixture`) so Workstream A is unpaid.
6. **UI is a container form.** When admitted, OptimizersPage hosted block still
   asks for Local Container URL, task, warm-start checkpoint. Prompt text still
   says `cispo.banking77.mlx.v1`. There is no Banking77-Tinker CISPO recipe
   parallel to hosted SFT.

---

## Operator runbook (after this plan)

Processes Desktop will **not** invent inside the GEPA sidecar:

| Process | Bind | Start |
|---|---|---|
| Workshop Desktop | — | app |
| GEPA sidecar | ephemeral | Desktop-owned |
| Public SFT | `127.0.0.1:8878` | `synth-optimizers sft service --db .sft/service.sqlite --bind 127.0.0.1:8878` |
| Public CISPO | `127.0.0.1:8880` | **new** `synth-optimizers-cispo service --db .cispo/service.sqlite --bind 127.0.0.1:8880` |

Env (fixture, Workstream A/B):

```bash
export SYNTH_OPTIMIZERS_SFT_SERVICE_URL=http://127.0.0.1:8878
export SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN=local-qa-token
export SYNTH_OPTIMIZERS_CISPO_SERVICE_URL=http://127.0.0.1:8880
export SYNTH_OPTIMIZERS_CISPO_SERVICE_TOKEN=local-qa-token
export SYNTH_OPTIMIZERS_SFT_FIXTURE=1
export SYNTH_OPTIMIZERS_CISPO_FIXTURE=1
export TINKER_CISPO_VALIDATION_RECEIPT=/Users/joshuapurtell/GitHub/optimizers/docs/receipts/tinker-gpt-oss-20b-banking77-canary-cispo/cispo.slime.v1.receipt.json
# Banking77 hosted SFT recipe availability (local file, not Tinker):
# export SYNTH_SFT_BANKING77_TRAIN_JSONL=/path/to/train.jsonl
```

Paid path later: drop the `*_FIXTURE` flags; set `TINKER_API_KEY` on the
**service** processes only. Never write `SYNTH_OPTIMIZERS_*` into `os.environ`
under `src/` except `SYNTH_OPTIMIZERS_TERMINAL`.

Live visual loop (already true once events map):

```
public service journal
  → Desktop poll GET /v1/runs/{id}/optimizer-events?after_sequence=
  → append_mapped_event
  → kernel metric_points (and candidates / evaluations / rollouts)
  → bus optimizer.run.updated
  → subscribeToRun invalidates collections
  → optimizer.sft.live.v1 / optimizer.cispo.live.v1 hydrates curves
```

---

## Workstreams (implement in this order)

### A. Fixture live loop for SFT (unpaid)

Prove one hosted SFT job updates Workshop visuals without Tinker.

**Python (`optimizers`)**

- SFT `--follow` treats `completed` as terminal. Today `_sft_submit` only
  exits on `succeeded|failed|cancelled`; public jobs finish as `completed`
  and hang. Prefer a sibling helper used by `_sft_submit` so `cli.py` does
  not grow; a one-token add of `"completed"` is acceptable only if the
  allowlist test still passes.
- Do **not** add CISPO subcommands to `cli.py`.

**Workshop**

- Operator starts `sft service` (out of A: no Desktop auto-spawn).
- Document the command + env on the hosted SFT recipe `prerequisites`.
- Strip “Optimizers-beta executor” from recipe copy.
- Confirm `drive_hosted_sft_job` + `watch_job` with fixture SFT produces
  `sft.training.metrics` and `optimizer.sft.live.v1` `metric_points` while
  status is still `running`.
- Classify `:8110` is **not** required for the train-loss series. If the
  hosted TOML blocks submit without it, add a fixture-friendly evaluation
  skip or a bounded fixture recipe that still uses the public service.
  Do not invent a Craftax world.

**Acceptance A**

1. `sft service` + `SYNTH_OPTIMIZERS_SFT_FIXTURE=1` + hosted Banking77 (or a
   documented fixture sibling) → visual shows train loss from `metric_points`
   while the job is still `running`.
2. Disconnecting SSE does not stop the job.
3. `sft submit --follow` exits on `completed`.

### B. Hosted CISPO launches a real slime request (fixture, then visual)

Replace the container-bind stub. Keep receipt admission.

**Python (`optimizers`)**

- New module `src/synth_optimizers/cispo_cli.py`: `service|submit|watch|cancel`.
- New console script in `pyproject.toml`:
  `synth-optimizers-cispo = "synth_optimizers.cispo_cli:main"`.
  Do not add a `cispo` subparser to `cli.py`.
- `serve_cispo_service` (or the CLI wrapper) honors
  `SYNTH_OPTIMIZERS_CISPO_FIXTURE=1` the way SFT honors `SFT_FIXTURE`.
- Default bind `127.0.0.1:8880`. Token from
  `SYNTH_OPTIMIZERS_CISPO_SERVICE_TOKEN`.

**`cispo.rs` `start_hosted`**

- Stop requiring `bind_cispo` for the public Tinker recipe.
- Build `config_json` isomorphic to
  `cispo_recipe(mode="learning_signal").request`
  (`cispo.request.v1`, slime-reference / `cispo.slime.v1`, Banking77 fixture
  examples unless a dataset is configured).
- Pass `{ "config_json": <request> }` into `create_and_watch`.

**`sidecar_training.rs` `drive_hosted_cispo_job`**

- If `config.config_json` is a `cispo.request.v1`, **do not** call
  `validate_tunneled_evaluation_plan`. Submit `config_json` as today.
- Keep receipt fail-closed.

**Recipe / UI**

- New recipe id parallel to hosted SFT: `cispo.banking77.slime.tinker.v1`.
  Keep `cispo.slime.hosted.v1` as an alias or retire it in the same PR.
- Availability: receipt **and** `CispoOptimizerClient::from_env().is_ok()`.
- When admitted, primary hosted button is that recipe, not `cispo.mlx.v1`.
- Drop Local Container URL from the public Tinker CISPO launch.
- Fix prompt text `cispo.banking77.mlx.v1`.
- Align `contract/runtimes.rs` bounded recipes.
- `TrainingWorkspace` tinker placement already points at
  `cispo.slime.hosted.v1`; retarget it.

**Acceptance B (fixture)**

1. Receipt path set, CISPO service on `:8880` with fixture executor.
2. Click hosted CISPO → public service accepts `config_json` (no 400).
3. `optimizer.cispo.live.v1` shows clip `0 … 5`, then `metric_points` / rollouts
   while the job is still running.
4. No Tinker HTTP in this acceptance.

### C. Desktop operator UX (still not “spawn inside GEPA sidecar”)

- Recipe prerequisites list the exact `sft service` / `cispo service` commands
  and env names.
- Recipe `availabilityReason` names the missing token/URL/receipt/JSONL, not a
  generic “sidecar”.
- Optional Desktop health ping (`GET /health` on 8878/8880) so the catalog
  shows `unavailable: public SFT service not listening` instead of a later
  connect error.
- Surface hosted SFT/CISPO on OptimizersPage the way TrainingWorkspace does
  (SFT/CISPO are currently filtered out of the guide grid).

**Out of scope unless it is cheap:** auto-spawn `sft service` like GEPA.

### D. Ingest cleanup (after A/B work)

- Unify `sidecar_training::mapped_event_draft` onto
  `training_adapter::adapt_source_fact` so there is one mapping table.
- Leave `hosted_sft::spawn_hosted_worker` dead unless we deliberately switch
  SFT to the GEPA ingest helper.
- Python SSE stays a journal mirror. Do not block live visuals on mapping SSE
  to `optimizer_event.v1`.

### E. Paid path (only after A+B green)

- Same recipes, drop `*_FIXTURE`, set `TINKER_API_KEY` on the **service**
  process.
- CISPO stays receipt-gated; do not mint a second canary.
- Cost ceiling already on hosted SFT (`$10`). Keep CISPO hosted at `$10`.
  Fail-closed if receipt `paid_update` is false.

---

## Exact CISPO `config_json` Desktop must send

Source of truth: `optimizers/src/synth_optimizers/recipes/banking77.py`
`cispo_recipe(mode="learning_signal")`. Prefer embedding that object, not a
hand-stripped subset. Minimum fields `validate_cispo_request` requires:

```json
{
  "schema_version": "cispo.request.v1",
  "algorithm_id": "cispo",
  "implementation": "slime-reference",
  "implementation_version": "cispo.slime.v1",
  "provider": "tinker",
  "model_id": "openai/gpt-oss-20b",
  "mode": "learning_signal",
  "dataset": {
    "recipe_id": "banking77.cispo.v1",
    "examples": [],
    "heldout_locked": true
  },
  "training": {
    "updates": 1,
    "group_size": 2,
    "prompts_per_update": 1,
    "eps_clip": 1.0,
    "eps_clip_high": 4.0,
    "checkpoint_every_updates": 1
  },
  "reward": { "version": "banking77.exact_label.v1", "task": "banking77" },
  "evaluation": {
    "scorer_version": "banking77.exact_label.v1",
    "heldout_locked": true,
    "mode": "learning_signal"
  }
}
```

Public `CispoService._executor_config` fills fixture examples when `examples`
are missing. Desktop may omit rows for fixture; paid runs must send a real split.
The recipe also sets `renderer_version` (`renderers.gpt-oss.low.v1`), `seed`,
indexes, and `system_prompt` — include those when copying `cispo_recipe`.

HTTP:

```http
POST /v1/runs
Authorization: Bearer $SYNTH_OPTIMIZERS_CISPO_SERVICE_TOKEN
{
  "algorithm": "cispo",
  "run_id": "cispo_hosted_<id>",
  "idempotency_key": "cispo_hosted_<id>",
  "config_json": { …cispo.request.v1… }
}
```

Slime clip (do not change): `eps_clip>=1` (lower `1-eps_clip` → 0),
`eps_clip_high=4` (upper 5). Stop-grad on ratio. Unbiased group std.

---

## File ownership

| Change | Repo | Files |
|---|---|---|
| CISPO CLI | `optimizers` | **new** `src/synth_optimizers/cispo_cli.py`; `pyproject.toml` script `synth-optimizers-cispo` |
| CISPO fixture serve | `optimizers` | `cispo_service.py` `serve_cispo_service` honors `SYNTH_OPTIMIZERS_CISPO_FIXTURE` |
| SFT follow terminal | `optimizers` | sibling helper, or one-token `"completed"` in `_sft_submit` if allowlist permits |
| Hosted CISPO payload | `workshop-readmodel-cua` | `apps/synth_desktop/src-tauri/src/optimizers/cispo.rs` `start_hosted` |
| Skip tunnel eval for public CISPO | `workshop-readmodel-cua` | `sidecar_training.rs` `drive_hosted_cispo_job` |
| Recipe catalog / UI | `workshop-readmodel-cua` | `cispo.rs`, `OptimizersPage.tsx`, `TrainingWorkspace.tsx`, `contract/runtimes.rs` |
| Hosted SFT copy | `workshop-readmodel-cua` | `hosted_sft.rs` prerequisites (drop beta) |
| Mapping unify | `workshop-readmodel-cua` | `sidecar_training.rs`, `training_adapter.rs` |
| Health ping | `workshop-readmodel-cua` | `sft_client.rs`, `cispo_client.rs`, recipe `availability` |
| Docs | both | this file; `optimizers/docs/MIGRATION_TINKER_SFT_CISPO.md` pointer |

`cli.py` / `hosted.py` / `o11y.py` stay allowlisted (shrink only).

---

## Acceptance (done when)

1. Operator starts SFT `:8878` (fixture) and CISPO `:8880` (fixture) with tokens.
2. Desktop hosted SFT recipe is `available`, launches, opens `optimizer.sft.live.v1`,
   and the train-loss series grows from `metric_points` before terminal.
3. With the existing slime receipt, hosted CISPO recipe is `available`, launches a
   **valid** `cispo.request.v1` (no container bind), opens `optimizer.cispo.live.v1`,
   shows clip identity then metric/rollout collections while running.
4. Killing the HTTP event-stream client does not stop either job.
5. Visuals never rebuild charts from the raw journal.
6. No second paid canary; no `optimizers-beta`; Craftax gold-only if that recipe is used.

---

## Suggested implementation PRs

**PR1 (A):** SFT `--follow` accepts `completed` + hosted SFT prerequisite copy
(no beta) + fixture runbook. Prove `optimizer.sft.live.v1` `metric_points` live.
**PR2 (B):** CISPO CLI + fixture serve flag + `start_hosted` real `config_json` +
skip tunnel eval + UI recipe rename. Prove `optimizer.cispo.live.v1` live.
**PR3 (C/D):** catalog health + OptimizersPage hosted buttons + mapping unify.
**PR4 (E):** paid, only if PR1 and PR2 were watched live.
