# Hosted standalone SFT — optimizers-beta + Workshop

**Status:** Phase 1/2 wired 2026-08-12 for a local hosted try (catalog loader, Desktop start, Tinker subprocess). Do not claim A4/A6 until a paid dry-run receipt exists.  
**Trees:** `optimizers-beta-sft` (`josh/aug12-sft`) · workshop (`josh/aug12-optimizers-workshop-visuals`)  
**Does not live in:** public `synth_sft`, `goex.sft.v1`, backend `/api/v1/optimizers/runs` (`OptimizerAlgorithm` has no `sft`).

This is the implementation spec for **A4** (two hosted SFT jobs) and **A6** (one multi-checkpoint job with live campaigns). It does not reopen locked stream/envelope decisions.

Canonical vocabulary: [`live_optimizers_gepa.md`](./live_optimizers_gepa.md) § Hosted standalone SFT.  
Acceptance: [`aug_12_update.md`](./aug_12_update.md) A4 / A6.  
Producer README: `optimizers-beta-sft/crates/synth_sft/README.md`.

---

## What is already true

| Layer | Done | Not done |
| --- | --- | --- |
| Producer identity | `algorithm_id: "sft"`. Page `optimizer_event_page.v1`. Cursor = `sequence`. Slot `optimizer_run`. | Real Tinker `SftBackend` (module exists, every method fails closed). |
| Fixture backend | Deterministic campaigns, `optimizer.visual.ready` before train, one accelerator → second job `queued`, `checkpoint.ready` ≠ promoted, null val loss / null reward stay null. | Fixture is **not** an A4/A6 receipt. No `cost_usd`. |
| Historical Fine-tuning UX | Job / events / checkpoints / aligned metrics — the UI we emulate. `openai.rs` is a mock-HTTP contract test, not a live provider. | OpenAI Fine-tuning API is shut down. Do not submit `backend=openai_ft`. |
| Desktop | Recipe `sft.hosted.fixture.v1` POSTs `/v1/runs` and polls `/runs/:id/optimizer-events`. Opens `optimizer.sft.live.v1`. Ingest remaps producer sequence onto SQLite and stores `sourceSequenceNumber`. Nemotron recipe loads [`sft_tinker_base_models.toml`](./sft_tinker_base_models.toml), POSTs `backend=tinker`, and binds the local Craftax slot. | Cost/reward stay `—` until the producer emits them. Local `sft.craftax.gpt-oss.smoke.v1` is a separate Groq+Tinker Python smoke, not hosted SFT. |
| Visuals | `sft.training.metrics` paints aligned points. Null val loss → `—`. Campaigns created on `sft.checkpoint_evaluation.allocated`. `sft.checkpoint_rollout.completed` patches child reward/cost. Overlay uses `formatMissingUsd`. | Cost/reward stay `—` until the producer emits them. |

Two fake Craftax JSONL smokes **fail A4**. Hosted Tinker jobs are required. The shut-down OpenAI Fine-tuning API is not a substitute.

---

## Fine-tuning UX to emulate (not a provider)

The historical OpenAI Fine-tuning console is the **visual contract**, not a backend:

- one job: queued → running → succeeded / failed / cancelled
- timestamped metric events as aligned points (not parallel-array clouds)
- immutable checkpoints with step + train / val metrics
- files, hyperparameters, and trained tokens as inspectable metadata

Workshop `optimizer.sft.live.v1` should feel like that job page, plus Synth extensions (checkpoint-eval campaigns, `ready` ≠ promoted, child rollout reward/cost). `crates/synth_sft/src/openai.rs` and `examples/sft_openai_ft_containers.toml` are a mock-HTTP harness for that shape. `OpenAiFineTuningBackend::from_env` fails closed. Do not POST to `api.openai.com/v1/fine_tuning`.

---

## Target live run (A4 / A6 dogfood)

**Craftax Rust GameBench SFT, student = Nemotron 3.5 Lightning LoRA on Tinker, streamed through Workshop.**

| Pin | Value | Notes |
| --- | --- | --- |
| Recipe id | `sft.craftax.nemotron-nano.tinker.v1` | Product-owned. Callers cannot supply commands, paths, or keys. |
| Algorithm | `sft` | Never `goex.sft.v1`. |
| Backend | `tinker` | Client-owned training loop. Sidecar must not impersonate Tinker HTTP. |
| Env | Craftax gold / GameBench container | Child evals are `synth.resource-ref.v1` `kind: container_rollout`. No NEV/frames in the optimizer log. |
| Student | Tinker `base_model` from [`sft_tinker_base_models.toml`](./sft_tinker_base_models.toml) | Run-configurable. Default `nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16`. Agent may set `base_model` on the recipe request; omitted uses the file default. |
| Teacher | Bounded Craftax traces | Reuse the smoke’s teacher-collection shape (seeds 101–104) or a pre-uploaded `training_file_id`. Do not silently swap GPT-OSS. |
| Checkpoints | At least two steps | e.g. `[10, 20]` smoke; live receipt can be larger but must stay bounded. The Python smoke only saves at the **end**; Phase 1 must save at each `checkpoint_steps` entry. |
| Campaign | `checkpoint_evaluation_seeds` + `container_url` | A6: prepare → `stream.subscribed` → allocate ref → `POST /rollouts` slot `stream` → `POST /reward`. |
| Accelerators | Default 1 | Second concurrent job is honestly `queued`. Distinct `dataset_digest` / `training_file_id`. |
| Visual | `optimizer.sft.live.v1` open **before** train | Bind slot `optimizer_run`. Reopen after Tinker/slots are gone. |

Held-out eval is measurement-only and must not be described as promotion evidence.

---

## Producer contract (optimizers-beta)

### HTTP (already)

```
POST /v1/runs          { algorithm: "sft", idempotency_key, config_toml }
GET  /v1/runs/:id
GET  /runs/:id/optimizer-events?after_sequence=&limit=
POST /v1/runs/:id/cancel
```

Auth: `Authorization: Bearer $OPTIMIZERS_BETA_SERVICE_TOKEN`.  
Page: `{ schema_version, run_id, after_sequence, next_sequence, terminal, events[] }`.  
Fail closed on sequence gaps. Skip only a trailing incomplete JSONL line. Heartbeats must not appear in the page.

### Event vocabulary that Workshop will project

Keep names from `live_optimizers_gepa.md`. Hosted beta already emits the subset below. Do not add `sft.step.metrics`.

| Event | Workshop action |
| --- | --- |
| `optimizer.visual.ready` | `delta.slot = optimizer_run`. Open visual before train. |
| `sft.training.queued` / `started` | Run status queued / running. Do **not** copy `sft.training.completed` `succeeded` onto the optimizer run. |
| `sft.training.metrics` | One aligned point: `step`, `epoch`, `train_loss`, `validation_loss` (nullable), `learning_rate`. |
| `sft.checkpoint.created` | Rail row, `ready=false`, `promoted=false`. |
| `sft.checkpoint.ready` | `ready=true`. **Not** promotion. |
| `sft.checkpoint_evaluation.allocated` | New campaign. Children = `artifact_refs` (`container_rollout`). Reward/cost still missing. |
| `sft.checkpoint_rollout.allocated` | Same child id; do not invent a second campaign. |
| `sft.checkpoint_rollout.completed` | Patch that child’s `attributes.reward` (nullable) and `attributes.cost_usd` **only if present**. `usage_delta` may carry tokens/rollouts without dollars. |
| `sft.checkpoint_evaluation.completed` | Campaign status completed. Mean score excludes null rewards. If every score is missing, do not promote. |
| `sft.checkpoint.promotion_evaluated` then `sft.checkpoint.promoted` | Promotion decision. |
| `optimizer.run.completed` / `failed` / `cancelled` | Terminal. Map HTTP `succeeded` → Desktop `completed`. |

Missing reward / cost / val loss stay JSON `null` or omitted. Never `0`.

### `cost_usd` rule

- **Fixture:** omit `cost_usd`. Tokens/rollouts in `usage_delta` are allowed. Workshop shows `—`.
- **Tinker:** emit `usage_delta.cost_usd` only from provider metering or a documented eval-lane tariff. Do not invent a Craftax-env dollar figure.
- Child eval cost is the **policy/inference** cost of that rollout, not the Tinker GPU-hour, unless the producer has a real meter for both and names them separately (`usage_delta.cost_usd` vs compute snapshot).

### Tinker backend (Phase 1)

`backend=tinker` is a first-class `SftBackendKind`. The plug-in point is already wired:

```text
execute_sft_on_with_cancellation
  SftBackendKind::Tinker
    → TinkerTrainingBackend::from_env()
    → execute_hosted_sft_with_backend  (same hosted driver the Fine-tuning UX mock uses)
```

Today `from_env()` returns `TINKER_RUNNER_UNIMPLEMENTED`. That is required. The sidecar must not speak forged Tinker HTTP.

Phase 1 fills in `crates/synth_sft/src/tinker.rs` only. Do not add a second event log, do not route through `synth_go_ex` SFT plugin, do not copy `goex.sft.v1`.

**Client-owned loop** (port, do not impersonate): `scripts/run_craftax_sft_uplift.py` `train_lora`:

1. `tinker.ServiceClient` + `create_lora_training_client(base_model, rank, …)`.
2. `forward_backward` / `optim_step` per step; emit `sft.training.metrics` with real train loss (null val loss if Tinker does not return one).
3. At each `checkpoint_steps` entry: `save_weights_for_sampler` + `save_state`. Mirror as `sft.checkpoint.created` then `sft.checkpoint.ready` with provider ids + digests.
4. `create_inference_target` from the sampler path so Containers can invoke the adapter.
5. Existing `ContainersCheckpointEvaluator` runs the A6 campaign (prepare / subscribed / allocate / `POST /rollouts` slot `stream` / `POST /reward`).
6. Cancel once, then persist `optimizer.run.cancelled`.
7. Write the same JSONL the page API already tails: `{workspace}/runs/{run_id}/events.jsonl`.

A trusted subprocess of that Python loop is allowed if it is product-owned and the sidecar only supervises it. Forging step metrics or checkpoint ids is not.

Refuse `base_model = "UNPINNED"` and empty model ids.

### Config TOML shape (live recipe)

Checked in as `optimizers-beta-sft/examples/sft_craftax_nemotron_tinker.toml`. Desktop POSTs the same keys (no caller-supplied model/command).

```toml
run_id = "sft_craftax_nemo_<suffix>"
backend = "tinker"
base_model = "nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16"  # or another id from docs/sft_tinker_base_models.toml
adapter = "lora_r16"
training_file_id = "file_train_<suffix>"
selection_file_id = "file_selection"
heldout_file_id = "file_heldout"
accelerator_slots = 1
checkpoint_steps = [10, 20]
campaign_rollouts_per_checkpoint = 2
evaluator_version = "craftax_gamebench.v1"
container_url = "http://127.0.0.1:8080"
checkpoint_evaluation_seeds = [501, 502]
checkpoint_evaluation_policy_harness = "react"
checkpoint_evaluation_plan_ref = "craftax_eval.v1"
checkpoint_evaluation_world_ref = "world:craftax"

[hyperparameters]
rank = 8
batch_size = 2
n_epochs = 1
```

Two A4 clicks must change `training_file_id` (and therefore `dataset_digest`). Same TOML with a new file id is a new job, not a replay.

---

## Workshop contract

Desktop is a **mirror**. It does not train.

1. `create` local `OptimizerRunRecord` (`algorithm_id: "sft"`, `source: "hosted"`).
2. `open_visual` → `optimizer.sft.live.v1`, slot `optimizer_run`.
3. `POST {SYNTH_OPTIMIZERS_BETA_URL}/v1/runs` with the recipe TOML.
4. Poll `GET {base}/runs/{id}/optimizer-events?after_sequence=` every 750ms.
5. `ingest_event_page`: fail closed on gap / dropped normalize / wrong `algorithm_id`; remap onto SQLite cursor; store `sourceSequenceNumber`.
6. Bus `optimizer.run.updated`. VisualHost reloads. Stop polling at `completed|failed|cancelled`.

Env: `SYNTH_OPTIMIZERS_BETA_URL` or `OPTIMIZERS_BETA_URL`, plus `OPTIMIZERS_BETA_SERVICE_TOKEN`. Craftax URL for the live recipe: `CRAFTAX_URL` or `http://127.0.0.1:8080` (`python -m craftax_gold`).

### Visual projection (A6 — this tree)

On `sft.checkpoint_rollout.completed`, patch the campaign child with the same `rollout_id`:

- `attributes.reward` ← `delta.reward` / `delta.score` (keep `null`).
- `attributes.cost_usd` ← `usage_delta.cost_usd` only if finite.
- Do not create a new campaign. Do not plot parallel arrays.

Templates:

| Template | Must show |
| --- | --- |
| `optimizer.sft.live.v1` | Job, aligned curves, latest ckpt ready≠promoted, campaigns with per-child reward/cost as `—` until present. |
| `optimizer.sft.rollouts.v1` | Child table: rollout, stream, reward URL, reward, cost. |
| `optimizer.sft.checkpoints.v1` | Rail: created ≠ ready ≠ promoted. Selection score `—` if missing. |

`optimizer.run.v1` overlay must use `formatMissingUsd`, never `$0.00` for absent cost.

### Recipes

| Id | Availability now | Start |
| --- | --- | --- |
| `sft.hosted.fixture.v1` | Beta URL+token | Fixture POST. Conformance only. |
| `sft.craftax.gpt-oss.smoke.v1` | Craftax binary + Groq + Tinker keys | Local Python runner. Not A4. |
| `sft.craftax.nemotron-nano.tinker.v1` | `OPTIMIZERS_BETA_SERVICE_TOKEN` + local Craftax slot | POST `backend=tinker`. Student id from [`sft_tinker_base_models.toml`](./sft_tinker_base_models.toml). |

### Desktop enable gates

Do not enable the Optimizers card by editing TSX alone. Catalog `availability` is `available` when the student catalog parses and `OPTIMIZERS_BETA_SERVICE_TOKEN` is set (URL defaults to `http://127.0.0.1:8879`). Start still fails closed if Craftax is not listening or the student id is not in the TOML.

| Gate | Today |
| --- | --- |
| Student catalog | Loaded from [`sft_tinker_base_models.toml`](./sft_tinker_base_models.toml) |
| Local beta | `OPTIMIZERS_BETA_SERVICE_TOKEN`; URL env or `http://127.0.0.1:8879` |
| Local Craftax slot | `CRAFTAX_URL` or `http://127.0.0.1:8080`, must be the catalog façade |
| Training rows | `SYNTH_SFT_TRAIN_JSONL` (Desktop copies into TOML; beta also reads the env) |

---

## File map (implement against these)

### optimizers-beta-sft

| File | Role |
| --- | --- |
| `crates/synth_sft/src/tinker.rs` | `TinkerTrainingBackend`. `from_env` requires `TINKER_API_KEY`. Supervises `scripts/run_hosted_tinker_loop.py`. Catalog allowlist. |
| `crates/synth_sft/src/runtime.rs` | Already dispatches Tinker into `execute_hosted_sft_with_backend`. Do not add a second driver. |
| `crates/synth_sft/src/containers.rs` | A6 campaign. Reuse; do not fork. |
| `crates/synth_sft/src/openai.rs` | Historical Fine-tuning UX mock. `from_env` fails closed. Keep for hosted-driver tests; not an A4 path. |
| `examples/sft_craftax_nemotron_tinker.toml` | Live recipe shape. `base_model` must be an id from `workshop/docs/sft_tinker_base_models.toml`. |
| `scripts/run_hosted_tinker_loop.py` | Product-owned Tinker loop. Saves at each `checkpoint_steps` entry. |
| `scripts/run_craftax_sft_uplift.py` | Reference / local smoke only; not the hosted receipt. |
| `src/main.rs` | Already accepts `algorithm: sft`. No backend `/api/v1/optimizers/runs` change. |

### workshop

| File | Role |
| --- | --- |
| `apps/synth_desktop/src-tauri/src/optimizers/hosted_sft.rs` | Fixture + Nemotron start. Loads catalog; binds local Craftax slot. |
| `apps/synth_desktop/src-tauri/src/optimizers/tinker_catalog.rs` | Parses `docs/sft_tinker_base_models.toml`. |
| `apps/synth_desktop/src-tauri/src/optimizers/ingest.rs` | Page remap / fail-closed. Do not change for Tinker. |
| `apps/synth_desktop/src-tauri/src/optimizers/service.rs` | Routes both hosted recipes through `hosted_sft::start`. |
| `apps/synth_desktop/src-tauri/src/bin/synth_optimizers_mcp.rs` | `optimizer_start_recipe` enum includes the Nemotron id; optional `base_model`. |
| `apps/synth_desktop/src/renderer/src/components/OptimizersPage.tsx` | Card + `startRecipe`. Enabled only when catalog says available. |
| `visuals/templates/optimizer.sft.*.v1` + `optimizer.run.v1/components/projectEvents.ts` | A6 projection. Done in Phase 0. |
| `apps/synth_desktop/skills/use-synth-optimizers/references/sft.md` | Start JSON + local-slot prerequisites. |

---

## Implementation phases

### Phase 0 — Workshop prep (done)

- [x] This spec.
- [x] Project `sft.checkpoint_rollout.completed` onto campaign children.
- [x] Live + rollouts templates show reward/cost with `—`.
- [x] Overlay missing cost is `—`.
- [x] Checkpoint rail: `created` is not `ready`.
- [x] Catalog `sft.craftax.nemotron-nano.tinker.v1` as unavailable; `start_recipe` fails closed with this doc’s recipe id.
- [x] Tests: missing reward stays `—`; present reward/cost patch; ready ≠ promoted.

### Phase 0.5 — Implementation prep (this cut)

- [x] `synth_sft::tinker` module + `TinkerTrainingBackend` fail-closed; runtime dispatches through the hosted driver.
- [x] Example TOML for the Tinker recipe; parse test; empty/`UNPINNED` refused.
- [x] Desktop `start_craftax_nemotron` fail-closed behind `TINKER_RUNNER_READY`.
- [x] Student ids catalogued in [`sft_tinker_base_models.toml`](./sft_tinker_base_models.toml) (loader not wired).
- [x] Optimizers card bound to catalog availability; MCP enum lists the recipe; skill documents the gate.
- [x] Pointers from `live_optimizers_gepa.md` and the Aug 12 handoff.

### Phase 1 — optimizers-beta Tinker runner

- [x] Implement `TinkerTrainingBackend` (`from_env`, `submit`, poll/metrics, checkpoints at `checkpoint_steps`, cancel, inference target). Sidecar still does not impersonate Tinker HTTP. `base_model` must be an id from [`sft_tinker_base_models.toml`](./sft_tinker_base_models.toml).
- [ ] Real checkpoints + A6 Containers campaign via existing evaluator; `cost_usd` only from a meter. Needs a paid dry-run.
- [ ] `cargo test -p synth_sft` plus one paid dry-run receipt (not a fixture).
- [x] `from_env` requires `TINKER_API_KEY` and submit talks to Tinker through the product-owned Python loop.

### Phase 2 — Desktop start

- [x] Load [`sft_tinker_base_models.toml`](./sft_tinker_base_models.toml); agent-configurable `base_model` on the run from that list (default = 3.5 Lightning). Not a Rust constant.
- [x] Card enables from catalog; MCP start is the same `start_recipe` call; skill start JSON.
- [x] Open visual before submit. Poll the same page API. No new ingest.
- [x] Bind local Craftax façade (`127.0.0.1:8080` or `CRAFTAX_URL`).

### Phase 3 — A4 / A6 live receipt

- [ ] Two jobs, different `dataset_digest`. One accelerator → second `queued`, then starts without corrupting the first log.
- [ ] One multi-checkpoint job: visual before train, concurrent campaigns, promotion after eval, reopen after slots gone.
- [ ] Do not claim A4/A6 from fixture or GPT-OSS JSONL smoke.

---

## Out of scope

- Publishing `synth_sft`.
- Adding `sft` to backend `OptimizerAlgorithm`.
- GELO plugin `goex.sft.v1` presented as standalone SFT.
- Inventing cost/reward in Workshop or in the fixture.
- GEPA/SFT on dig.bench.
- Unpinned “Nemotron 3.5 Nano” string in a paid recipe.
- Calling the shut-down OpenAI Fine-tuning API, or presenting `backend=openai_ft` as an A4/A6 receipt.

---

## Verify

```bash
# Workshop (Phase 0 / 0.5)
cd /Users/joshuapurtell/Documents/GitHub/workshop
cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --lib \
  optimizers::ingest optimizers::hosted_sft lists_hosted lists_craftax_nemotron \
  sft_training_completed
node --experimental-strip-types --test visuals/tests/optimizer_family.test.mjs

# Producer
cd /Users/joshuapurtell/Documents/GitHub/optimizers-beta-sft
cargo test -p synth_sft
```
